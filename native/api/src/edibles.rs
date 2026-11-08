//! The edible rules ([`crate::edible`]) wired to Horizon: eating a dosed edible from the hand, a
//! slice off a placed space cake, the doses waiting on the player, and the tooltips.
//!
//! What is still to land is saved with the player (`hempdustry:pending_edibles`, a player
//! attachment), so an edible eaten just before a logout lands after the login; the clock stops
//! while the player is away. Dying drops it: the attachment isn't copied to the respawned
//! player, as the Java mod's isn't.

use std::sync::{Mutex, PoisonError};

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, EquipmentSlot, Flow, Hand, Holder,
    Host, ItemStack, LivingEntityUseItemFinishEvent, NativeEffect, NativeServerPlayer,
    PlaceBlockEvent, RightClickBlockV2Event, ServerTickEvent, ServerTickPhase, Tag, TooltipEvent,
};
use horizon_client::text::{AQUA, DARK_GRAY, GRAY, LIGHT_PURPLE, Text, WHITE};

use crate::edible::{self, MAX_BITES, MAX_TIER, Pending, Rules, SPACE_CAKE, Waiting};
use crate::machine::infuser::Quality;
use crate::smoke::{Effect, Policy};
use crate::smoking::{Smoking, SplitMix, category};

const POTENCY: &str = "hempdustry:potency";
const QUALITY: &str = "hempdustry:quality";
const STRENGTH: &str = "hempdustry:strength";
const CANNABUTTER: &str = "hempdustry:cannabutter";

const CREATIVE: i32 = 1;
/// `Block.UPDATE_ALL`, as `CakeBlock.eat` writes the bite.
const UPDATE_ALL: i32 = 3;

/// `hempdustry:pending_edibles`, saved with the player and not kept through a death.
static PENDING: AttachmentType<Waiting> = AttachmentType {
    namespace: "hempdustry",
    name: "pending_edibles",
    default: Waiting::default,
    codec: Some(AttachmentCodec {
        encode: encode_waiting,
        decode: decode_waiting,
    }),
    copy_on_death: false,
    sync: false,
};

#[derive(Default)]
pub(crate) struct Edibles {
    dice: Mutex<SplitMix>,
    /// The players with doses on their way, so a tick reads only their attachments.
    waiting: Mutex<Vec<u128>>,
}

impl Edibles {
    /// `EdibleItem.finishUsingItem`: the edible is eaten as vanilla eats it (the hand's
    /// leftover, a bhang's bucket), then its dose is queued, read off the stack as it was.
    pub(crate) fn use_finish(
        &self,
        smoking: &Smoking,
        host: &mut dyn Host,
        event: &LivingEntityUseItemFinishEvent<'_>,
    ) {
        if !edible::is_edible(&event.stack.item) || event.entity.is_some() {
            return;
        }
        let Some(player) = host.acting_player() else {
            return;
        };
        let (tier, quality) = dose_of(host, event.stack);
        self.consume(smoking, host, player.uuid, tier, quality);
    }

    /// `SpaceCakeBlock.getStateForPlacement`: a placed cake carries its stack's dose. Horizon's
    /// placement hook sees only the item's name, so the dose is written once the cake is down.
    pub(crate) fn place_block(host: &mut dyn Host, event: &PlaceBlockEvent<'_>) {
        if &*event.stack.item != SPACE_CAKE {
            return;
        }
        let (tier, quality) = dose_of(host, event.stack);
        let potency = tier.clamp(0, MAX_TIER).to_string();
        host.set_block_property(event.pos, "potency", &potency);
        host.set_block_property(event.pos, "quality", quality.name());
    }

    /// `SpaceCakeBlock.useItemOn` and `useWithoutItem`: a click on the cake is always a bite,
    /// whatever the hand holds (never vanilla's candle), and a bite doses the eater. Taken here
    /// because the template cake would turn into a vanilla candle cake.
    pub(crate) fn cake_click(
        &self,
        smoking: &Smoking,
        host: &mut dyn Host,
        event: &RightClickBlockV2Event,
    ) -> Flow {
        if host.block(event.pos) != SPACE_CAKE {
            return Flow::Continue;
        }
        let slot = match event.hand {
            Hand::Main => EquipmentSlot::MainHand,
            Hand::Off => EquipmentSlot::OffHand,
        };
        let candle = host.equipment(slot).is_some_and(|s| is_candle(&s.item));
        // `useWithoutItem` is asked for the main hand only.
        if matches!(event.hand, Hand::Off) {
            return if candle { Flow::Cancel } else { Flow::Continue };
        }
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        if !self.bite(smoking, host, &player, event.pos) && !candle {
            // Not hungry: the hand's item gets its turn, as after vanilla's PASS.
            return Flow::Continue;
        }
        Flow::Cancel
    }

    /// `CakeBlock.eat`, then the cake's dose: false when the player can't eat now.
    fn bite(
        &self,
        smoking: &Smoking,
        host: &mut dyn Host,
        player: &NativeServerPlayer,
        pos: [i32; 3],
    ) -> bool {
        let Some((food, saturation, exhaustion)) = host.player_food(player.uuid) else {
            return false;
        };
        if !edible::can_eat(player.game_mode == CREATIVE, food) {
            return false;
        }
        // Read the dose before the bite rewrites the block.
        let potency = host
            .block_property(pos, "potency")
            .and_then(|p| p.parse::<i32>().ok())
            .unwrap_or(0);
        let quality = host
            .block_property(pos, "quality")
            .and_then(|q| Quality::from_name(&q))
            .unwrap_or(Quality::Rough);
        let bites = host
            .block_property(pos, "bites")
            .and_then(|b| b.parse::<i32>().ok())
            .unwrap_or(0);
        let (food, saturation) = edible::after_bite(food, saturation);
        host.set_player_status(
            player.uuid,
            None,
            Some((food, saturation, exhaustion)),
            None,
        );
        if bites < MAX_BITES {
            host.set_block_property(pos, "bites", &(bites + 1).to_string());
        } else if let Some(air) = host.default_state("minecraft:air") {
            let dimension = host.dimension();
            host.set_block(dimension, pos, air, UPDATE_ALL);
        }
        self.consume(smoking, host, player.uuid, potency, quality);
        true
    }

    /// `EdibleEffects.consume`: the whole staggered sequence queued on the player.
    fn consume(
        &self,
        smoking: &Smoking,
        host: &mut dyn Host,
        uuid: u128,
        tier: i32,
        quality: Quality,
    ) {
        let catalog = smoking.catalog();
        let bundle = catalog.edible();
        let policy = Policy::default();
        let rules = Rules {
            bundle: &bundle,
            policy: &policy,
            category: &category,
        };
        let u = self
            .dice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .next_double();
        let doses = rules.doses(tier, quality, u);
        if doses.is_empty() {
            return;
        }
        let Ok(mut waiting) = PENDING.get(host, Holder::Player(uuid)) else {
            return;
        };
        for dose in doses {
            waiting.schedule(dose);
        }
        if PENDING.set(host, Holder::Player(uuid), waiting).is_ok() {
            self.watch(uuid);
        }
    }

    fn watch(&self, uuid: u128) {
        let mut waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
        if !waiting.contains(&uuid) {
            waiting.push(uuid);
        }
    }

    /// `EdibleScheduler.tick`: each waiting dose a tick closer for every player on and alive, and
    /// the ones whose time has come applied. A player lying dead waits for the respawn, which
    /// drops the list.
    pub(crate) fn server_tick(&self, host: &mut dyn Host, event: &ServerTickEvent<'_>) {
        if !matches!(event.phase, ServerTickPhase::ServerTickEnd { .. }) {
            return;
        }
        let players = host.server_players();
        // A player who logs back in with doses saved is picked up on their first tick here.
        for player in &players {
            if !self.is_watched(player.uuid)
                && PENDING
                    .get_existing(host, Holder::Player(player.uuid))
                    .is_ok_and(|w| w.is_some_and(|w| !w.is_empty()))
            {
                self.watch(player.uuid);
            }
        }
        let watched: Vec<u128> = self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for uuid in watched {
            let Some(player) = players.iter().find(|p| p.uuid == uuid) else {
                // Away: the clock stops, and the saved list waits for the login.
                self.unwatch(uuid);
                continue;
            };
            if player.health <= 0.0 {
                continue;
            }
            let Ok(Some(mut waiting)) = PENDING.get_existing(host, Holder::Player(uuid)) else {
                self.unwatch(uuid);
                continue;
            };
            let landed = waiting.tick();
            let done = waiting.is_empty();
            let _ = PENDING.set(host, Holder::Player(uuid), waiting);
            give(host, uuid, &landed);
            if done {
                self.unwatch(uuid);
            }
        }
    }

    fn is_watched(&self, uuid: u128) -> bool {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&uuid)
    }

    fn unwatch(&self, uuid: u128) {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|&u| u != uuid);
    }

    /// `EdibleEffects.appendTooltip` and `CannabutterItem.appendHoverText`: the lines under the
    /// name that say what a stack carries.
    pub(crate) fn tooltip(host: &dyn Host, event: &mut TooltipEvent<'_>) {
        let stack = event.stack;
        // Asked every frame for every hovered stack: leave others' items before any lookup.
        if !stack.item.starts_with("hempdustry:") {
            return;
        }
        let lines = if &*stack.item == CANNABUTTER {
            butter_lines(host, stack)
        } else if edible::is_edible(&stack.item) || &*stack.item == SPACE_CAKE {
            edible_lines(host, stack)
        } else {
            return;
        };
        let at = event.lines.len().min(1);
        for (i, line) in lines.into_iter().enumerate() {
            event.lines.insert(at + i, (line, u32::MAX));
        }
    }
}

/// The potency and quality a stack carries: 0 (undosed) and Rough when it carries none.
fn dose_of(host: &dyn Host, stack: &ItemStack) -> (i32, Quality) {
    let potency = match host.component(stack, POTENCY) {
        Ok(Some(Tag::Int(n))) => *n,
        _ => 0,
    };
    (potency, quality_of(host, stack).unwrap_or(Quality::Rough))
}

fn quality_of(host: &dyn Host, stack: &ItemStack) -> Option<Quality> {
    match host.component(stack, QUALITY) {
        Ok(Some(Tag::String(name))) => Quality::from_name(&name.to_string_lossy()),
        _ => None,
    }
}

/// "Potency: II" in gray, then "Quality: Clean" in dark gray; nothing for an undosed stack.
fn edible_lines(host: &dyn Host, stack: &ItemStack) -> Vec<Text> {
    let (potency, _) = dose_of(host, stack);
    if potency <= 0 {
        return Vec::new();
    }
    let level = Text::translate(&format!("enchantment.level.{potency}"), &[]);
    let mut lines = vec![Text::translate("hempdustry.edible.potency", &[level]).with_color(GRAY)];
    if let Some(quality) = quality_of(host, stack) {
        let grade = Text::translate(&quality.translation_key(), &[]);
        lines.push(Text::translate("hempdustry.edible.quality", &[grade]).with_color(DARK_GRAY));
    }
    lines
}

/// The butter's grade, coloured up the ladder as vanilla's rarities climb, then "Dose: 12".
fn butter_lines(host: &dyn Host, stack: &ItemStack) -> Vec<Text> {
    let mut lines = Vec::new();
    if let Some(quality) = quality_of(host, stack) {
        let colour = match quality {
            Quality::Rough => GRAY,
            Quality::Standard => WHITE,
            Quality::Clean => AQUA,
            Quality::Perfect => LIGHT_PURPLE,
        };
        lines.push(Text::translate(&quality.translation_key(), &[]).with_color(colour));
    }
    if let Ok(Some(Tag::Int(strength))) = host.component(stack, STRENGTH) {
        let dose = Text::literal(strength.to_string());
        lines.push(
            Text::translate("item.hempdustry.cannabutter.strength", &[dose]).with_color(GRAY),
        );
    }
    lines
}

/// `#minecraft:candles`: vanilla's candle and its sixteen colours.
fn is_candle(item: &str) -> bool {
    item.strip_prefix("minecraft:")
        .is_some_and(|name| name == "candle" || name.ends_with("_candle"))
}

fn give(host: &mut dyn Host, uuid: u128, effects: &[Effect]) {
    for effect in effects {
        host.add_effect(
            uuid,
            &NativeEffect {
                id: effect.id.clone(),
                amplifier: u8::try_from(effect.amplifier.clamp(0, 255)).unwrap_or(u8::MAX),
                duration: effect.duration,
                ambient: false,
                particles: true,
            },
        );
    }
}

/// `{pending: [{ticks_left, effect: {id, duration, amplifier}}]}`, `EdibleScheduler.Pending`'s
/// shape.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature `AttachmentCodec` takes"
)]
fn encode_waiting(waiting: &Waiting) -> Result<Compound, AttachmentError> {
    let list = waiting
        .0
        .iter()
        .map(|pending| {
            let effect = Compound::from([
                ("id".into(), Tag::String(pending.effect.id.as_str().into())),
                ("duration".into(), Tag::Int(pending.effect.duration)),
                ("amplifier".into(), Tag::Int(pending.effect.amplifier)),
            ]);
            Tag::Compound(Compound::from([
                ("ticks_left".into(), Tag::Int(pending.ticks_left)),
                ("effect".into(), Tag::Compound(effect)),
            ]))
        })
        .collect();
    Ok(Compound::from([("pending".into(), Tag::List(list))]))
}

/// A dose the save can't read is dropped rather than failing the whole list.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the signature `AttachmentCodec` takes"
)]
fn decode_waiting(data: &Compound) -> Result<Waiting, AttachmentError> {
    let Some(Tag::List(list)) = data.get(&"pending".into()) else {
        return Ok(Waiting::default());
    };
    let int = |c: &Compound, key: &str| match c.get(&key.into()) {
        Some(Tag::Int(n)) => Some(*n),
        _ => None,
    };
    let pending = list
        .iter()
        .filter_map(|tag| {
            let Tag::Compound(entry) = tag else {
                return None;
            };
            let Some(Tag::Compound(effect)) = entry.get(&"effect".into()) else {
                return None;
            };
            let Some(Tag::String(id)) = effect.get(&"id".into()) else {
                return None;
            };
            Some(Pending {
                ticks_left: int(entry, "ticks_left").filter(|&n| n > 0)?,
                effect: Effect::new(
                    &id.to_string_lossy(),
                    int(effect, "duration")?,
                    int(effect, "amplifier").unwrap_or(0),
                ),
            })
        })
        .collect();
    Ok(Waiting(pending))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edible::Dose;

    #[test]
    fn the_waiting_list_round_trips_through_its_save() {
        let mut waiting = Waiting::default();
        waiting.schedule(Dose {
            delay: 1800,
            effect: Effect::new("slowness", 9600, 0),
        });
        waiting.schedule(Dose {
            delay: 2400,
            effect: Effect::new("regeneration", 400, 0),
        });
        let saved = encode_waiting(&waiting).expect("encodes");
        assert_eq!(decode_waiting(&saved), Ok(waiting));
    }

    #[test]
    fn a_save_with_a_broken_dose_keeps_the_rest() {
        let good = Tag::Compound(Compound::from([
            ("ticks_left".into(), Tag::Int(5)),
            (
                "effect".into(),
                Tag::Compound(Compound::from([
                    ("id".into(), Tag::String("minecraft:hunger".into())),
                    ("duration".into(), Tag::Int(100)),
                ])),
            ),
        ]));
        let broken = Tag::Compound(Compound::from([("ticks_left".into(), Tag::Int(0))]));
        let saved = Compound::from([("pending".into(), Tag::List(vec![broken, good]))]);
        let waiting = decode_waiting(&saved).expect("decodes");
        assert_eq!(waiting.0.len(), 1);
        assert_eq!(waiting.0[0].effect, Effect::new("hunger", 100, 0));
    }

    #[test]
    fn every_candle_colour_is_a_candle() {
        assert!(is_candle("minecraft:candle"));
        assert!(is_candle("minecraft:light_blue_candle"));
        assert!(!is_candle("minecraft:candle_cake"));
        assert!(!is_candle("hempdustry:space_cake"));
    }
}
