//! The Sifting Box ([`crate::machine::sifting`]) wired to Horizon: a click on
//! `right_click_block_v2` (filling the tub, an item over the screen, taking a batch out), the
//! settle a full screen waits for, and the comparator reading the level.
//!
//! The box has no block entity, so its whole state is its blockstate. Java settles a full screen
//! with a scheduled block tick; a click hook can't schedule one in Horizon, so the mod keeps the
//! boxes waiting to settle and settles them at the end of their level's tick. That list isn't
//! saved: a box left full over a reload settles a second after its next click.

use std::sync::PoisonError;

use horizon_client::mod_api::{
    AnalogOutputEvent, BlockStateId, Compound, EquipmentSlot, Flow, Hand, HasAnalogOutputEvent,
    Host, ItemStack, LevelDimension, LevelHost, NativeServerPlayer, RandomSource,
    RightClickBlockV2Event, Tag, Text,
};

use super::Machines;
use crate::farm::interact::Effect;
use crate::host::Hands;
use crate::machine::sifting::{
    self, Click, Content, FULL_LEVEL, Held, Ice, READY_LEVEL, SETTLE_DELAY, Screen, Siftable,
    Sifted, YIELD,
};

pub(crate) const SIFTING_BOX: &str = "hempdustry:sifting_box";
/// Only the box has a `content` property.
const CONTENT: &str = "content";

const FILL_SUCCESS: &str = "minecraft:block.composter.fill_success";
const FILL: &str = "minecraft:block.composter.fill";
const READY: &str = "minecraft:block.composter.ready";
const EMPTY: &str = "minecraft:block.composter.empty";
const BUCKET_EMPTY: &str = "minecraft:item.bucket.empty";
const BUCKET_FILL: &str = "minecraft:item.bucket.fill";
/// `SoundSource.BLOCKS`'s ordinal.
const SOUND_BLOCKS: usize = 4;
/// `ItemEntity.setDefaultPickUpDelay`.
const PICKUP_DELAY: i16 = 10;

/// A full box waiting to settle, and the game time it is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Settling {
    dimension: LevelDimension,
    pos: [i32; 3],
    due: i64,
}

/// The box at `pos`, or None for another block.
fn read(host: &dyn Host, pos: [i32; 3]) -> Option<Screen> {
    if host.block(pos) != SIFTING_BOX {
        return None;
    }
    Some(Screen {
        level: host.block_property(pos, "level")?.parse().ok()?,
        filled: host.block_property(pos, "filled")? == "true",
        content: Content::from_name(&host.block_property(pos, CONTENT)?)?,
    })
}

/// Writes what changed from `from` to `to`, the level first.
fn write(host: &mut dyn Host, pos: [i32; 3], from: Screen, to: Screen) {
    if from.level != to.level {
        host.set_block_property(pos, "level", &to.level.to_string());
    }
    if from.filled != to.filled {
        host.set_block_property(pos, "filled", if to.filled { "true" } else { "false" });
    }
    if from.content != to.content {
        host.set_block_property(pos, CONTENT, to.content.name());
    }
}

/// What `stack` is to the screen; the tags in Java's order.
fn held(host: &dyn Host, stack: Option<&ItemStack>) -> Held {
    let Some(stack) = stack else {
        return Held::Other;
    };
    if &*stack.item == "minecraft:water_bucket" {
        return Held::WaterBucket;
    }
    Siftable::ALL
        .into_iter()
        .find(|kind| host.server_item_in_tag(stack, kind.tag()).unwrap_or(false))
        .map_or(Held::Other, Held::Siftable)
}

/// The jacket round the box at `pos`: its four horizontal sides.
fn jacket(host: &dyn Host, pos: [i32; 3]) -> f32 {
    let [x, y, z] = pos;
    let sides = [[x, y, z - 1], [x, y, z + 1], [x - 1, y, z], [x + 1, y, z]];
    sifting::jacket_rate(sides.map(|side| Ice::of(&host.block(side))))
}

fn sound(pos: [i32; 3], sound: &'static str, volume: f32, pitch: f32) -> Effect {
    Effect::Sound {
        pos,
        sound,
        volume,
        pitch,
    }
}

fn play(host: &mut dyn Host, pos: [i32; 3], sound: &str) {
    let at = [
        f64::from(pos[0]) + 0.5,
        f64::from(pos[1]) + 0.5,
        f64::from(pos[2]) + 0.5,
    ];
    host.play_sound(sound, at, SOUND_BLOCKS, 1.0, 1.0);
}

/// The game time in the level in place.
fn now(host: &dyn Host) -> Option<i64> {
    host.level(host.dimension())
        .and_then(|level| level.game_time(host))
}

impl Machines {
    /// `useItemOn`, then `useWithoutItem` for the main hand: the box's whole click.
    ///
    /// The client's copy of the box is an oak planks template that takes no click, so after a
    /// click the box took, the client goes on to the item's own use and to the other hand, as it
    /// does after a `PASS`: a water bucket would pour on top of the box. Java's client knows the
    /// box took it (`CONSUME`) and sends neither. So a click the box takes is remembered for the
    /// player, and what the client sends after it in that tick is dropped ([`Self::took_click`]).
    pub(crate) fn sifting_box_click(
        &self,
        host: &mut dyn Host,
        event: &RightClickBlockV2Event,
    ) -> Flow {
        let Some(screen) = read(host, event.pos) else {
            return Flow::Continue;
        };
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        if self.took_click(host) {
            return Flow::Cancel;
        }
        let flow = self.box_click(host, event, screen, &player);
        if flow == Flow::Cancel
            && let Some(now) = now(host)
        {
            self.claims
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(player.uuid, now);
        }
        flow
    }

    /// Whether the acting player's click on a box was taken this tick or the last: the client's
    /// follow-up (the item's use, the other hand's turn) is then dropped.
    pub(crate) fn took_click(&self, host: &dyn Host) -> bool {
        let Some(player) = host.acting_player() else {
            return false;
        };
        let Some(now) = now(host) else {
            return false;
        };
        let mut claims = self.claims.lock().unwrap_or_else(PoisonError::into_inner);
        if claims.is_empty() {
            return false;
        }
        claims.retain(|_, at| now - *at <= 1);
        let took = claims.contains_key(&player.uuid);
        drop(claims);
        took
    }

    fn box_click(
        &self,
        host: &mut dyn Host,
        event: &RightClickBlockV2Event,
        screen: Screen,
        player: &NativeServerPlayer,
    ) -> Flow {
        let pos = event.pos;
        let main = host.equipment(EquipmentSlot::MainHand);
        let off = host.equipment(EquipmentSlot::OffHand);
        // Sneaking with anything in hand skips the block (`isSecondaryUseActive`).
        if player.sneaking && (main.is_some() || off.is_some()) {
            return Flow::Continue;
        }
        let main_hand = matches!(event.hand, Hand::Main);
        let stack = if main_hand { main } else { off };
        let jacket = if screen.filled {
            jacket(host, pos)
        } else {
            1.0
        };
        let dimension = host.dimension();
        match sifting::click(screen, held(host, stack.as_ref()), jacket) {
            Click::Fill => {
                let Some(bucket) = stack else {
                    return Flow::Continue;
                };
                write(
                    host,
                    pos,
                    screen,
                    Screen {
                        filled: true,
                        ..screen
                    },
                );
                let effects = vec![
                    Effect::EmptyHeld("minecraft:bucket".into()),
                    sound(pos, BUCKET_EMPTY, 1.0, 1.0),
                ];
                Hands { host, dimension }.apply(&bucket, effects);
                Flow::Cancel
            }
            Click::Hold => Flow::Cancel,
            Click::Sift {
                screen: stamped,
                chance,
            } => {
                let Some(item) = stack else {
                    return Flow::Continue;
                };
                let u = host
                    .level_random(dimension)
                    .map_or(0.5, RandomSource::next_float);
                let mut effects = vec![Effect::ConsumeHeld];
                match sifting::sift(stamped, chance, u) {
                    Sifted::Rose(next) => {
                        write(host, pos, screen, next);
                        effects.push(sound(pos, FILL_SUCCESS, 1.0, 1.0));
                        if next.level == FULL_LEVEL {
                            self.settle_later(host, pos);
                        }
                    }
                    Sifted::Missed if screen.filled => {
                        effects.push(sound(pos, BUCKET_FILL, 0.6, 1.4));
                    }
                    Sifted::Missed => effects.push(sound(pos, FILL, 1.0, 1.0)),
                }
                Hands { host, dimension }.apply(&item, effects);
                Flow::Cancel
            }
            Click::Refuse { tell } => {
                if let Some(key) = tell {
                    host.action_bar(player.uuid, Text::translate(key, &[]));
                }
                if !main_hand {
                    return Flow::Continue;
                }
                if let Some((batch, empty)) = sifting::collect(screen) {
                    write(host, pos, screen, empty);
                    spawn_batch(host, dimension, pos, batch.item());
                    play(host, pos, EMPTY);
                    return Flow::Cancel;
                }
                if screen.level == FULL_LEVEL {
                    // A full box whose settle was lost to a reload.
                    self.settle_later(host, pos);
                }
                Flow::Continue
            }
        }
    }

    /// Queues the full box at `pos` (in the level in place) to settle in `SETTLE_DELAY` ticks;
    /// one already waiting keeps its time.
    fn settle_later(&self, host: &dyn Host, pos: [i32; 3]) {
        let dimension = host.dimension();
        let Some(now) = host
            .level(dimension)
            .and_then(|level| level.game_time(host))
        else {
            return;
        };
        let dimension = crate::dimension_of(dimension);
        let mut settling = self.settling.lock().unwrap_or_else(PoisonError::into_inner);
        if !settling
            .iter()
            .any(|s| s.dimension == dimension && s.pos == pos)
        {
            settling.push(Settling {
                dimension,
                pos,
                due: now + SETTLE_DELAY,
            });
        }
    }

    /// At the end of the level `dimension`'s tick: the boxes due settle (`tick`), if they are
    /// still full boxes.
    pub(crate) fn settle_boxes(&self, host: &mut dyn Host, dimension: &str) {
        let level = crate::dimension_of(dimension);
        let Some(now) = host.level(dimension).and_then(|l| l.game_time(host)) else {
            return;
        };
        let due: Vec<[i32; 3]> = {
            let mut settling = self.settling.lock().unwrap_or_else(PoisonError::into_inner);
            if settling.is_empty() {
                return;
            }
            let (due, waiting) = settling
                .drain(..)
                .partition(|s| s.dimension == level && s.due <= now);
            *settling = waiting;
            drop(settling);
            due.into_iter().map(|s: Settling| s.pos).collect()
        };
        for pos in due {
            if let Some(screen) = read(host, pos)
                && let Some(ready) = screen.settled()
            {
                write(host, pos, screen, ready);
                play(host, pos, READY);
            }
        }
    }

    /// A comparator beside the box reads its level, as a composter's.
    pub(crate) fn sifting_box_has_analog_output(
        level: &dyn LevelHost,
        event: &mut HasAnalogOutputEvent,
    ) {
        if is_box(level, event.state) {
            event.output = true;
        }
    }

    pub(crate) fn sifting_box_analog_output(level: &dyn LevelHost, event: &mut AnalogOutputEvent) {
        if !is_box(level, event.state) {
            return;
        }
        if let Some(n) = (0..=READY_LEVEL).find(|n| {
            level.state_with_property(event.state, "level", &n.to_string()) == Some(event.state)
        }) {
            event.output = i32::from(n);
        }
    }
}

fn is_box(level: &dyn LevelHost, state: BlockStateId) -> bool {
    level
        .state_with_property(state, CONTENT, Content::Plant.name())
        .is_some()
}

/// The batch, `YIELD` of `item`, popped out over the box as Java's `collect` throws it: a little
/// above the rim, scattered, with an item entity's own toss and pickup delay.
fn spawn_batch(host: &mut dyn Host, dimension: &str, pos: [i32; 3], item: &str) {
    let mut roll = || {
        host.level_random(dimension)
            .map_or(0.5, RandomSource::next_float)
    };
    // `Vec3.offsetRandom(random, 0.7F)`.
    let mut offset = || f64::from((roll() - 0.5) * 0.7);
    let at = [
        f64::from(pos[0]) + 0.5 + offset(),
        f64::from(pos[1]) + 1.01 + offset(),
        f64::from(pos[2]) + 0.5 + offset(),
    ];
    let mut toss = || {
        host.level_random(dimension)
            .map_or(0.5, RandomSource::next_double)
            * 0.2
            - 0.1
    };
    let motion = [toss(), 0.2, toss()];
    let mut stack = Compound::new();
    stack.insert("id".into(), Tag::String(item.into()));
    stack.insert("count".into(), Tag::Int(i32::try_from(YIELD).unwrap_or(1)));
    let mut nbt = Compound::new();
    nbt.insert("Item".into(), Tag::Compound(stack));
    nbt.insert("PickupDelay".into(), Tag::Short(PICKUP_DELAY));
    nbt.insert(
        "Motion".into(),
        Tag::List(motion.map(|v| Tag::Double(v.to_bits())).to_vec()),
    );
    host.summon(dimension, "minecraft:item", at, nbt);
}
