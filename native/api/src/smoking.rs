//! The smoking rules ([`crate::smoke`]) wired to Horizon: the mod's data components, the data
//! files they read, a hit from each smokeable, the bong's draw, the exhale that lands later and
//! the cooldown every smokeable shares.
//!
//! Horizon has no item cooldown a mod can start, so the cooldown is the mod's own: the end tick
//! per player, checked before a hit, and `hempdustry:cooldown_until` on the stack that was used,
//! as the Java mod marks it.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use horizon_client::mod_api::{
    AddServerReloadListenersEvent, ComponentTypesEvent, Compound, EquipmentSlot, Flow, Host,
    InventoryTickEvent, ItemModelPropertyEvent, ItemStack, LivingEntityUseItemFinishEvent,
    LivingEntityUseItemStartEvent, NativeCriterion, NativeEffect, NativeReloadListener,
    NativeServerPlayer, OnUseTickEvent, ParticleOptions, ParticleType, PlayerRespawnedEvent,
    ServerReloadListenerApplyEvent, ServerTickEvent, ServerTickPhase, Tag, TooltipEvent,
    UseAnimation, UseAnimationEvent, UseDurationEvent, UseItemEvent,
};
use horizon_client::text::{GRAY, Text};
use serde_json::{Map, Value};

use crate::smoke::hit::{Dice, Hit, Rules, Smoker, Spent, Worn};
use crate::smoke::name::{self, Labels, PackedName};
use crate::smoke::{Catalog, Category, Device, Effect, Exhale, Policy, SmokeContents};

pub(crate) const SMOKE_CONTENTS: &str = "hempdustry:smoke_contents";
pub(crate) const CHARGES: &str = "hempdustry:charges";
pub(crate) const COOLDOWN_UNTIL: &str = "hempdustry:cooldown_until";

/// Every data component the Java mod registers in code. Registered here so stacks carrying
/// them load, save and draw as in Java (the item definitions of the bongs, pipe and vaporizer
/// branch on `smoke_contents`); the edibles' three are read by the edibles' port.
const COMPONENT_TYPES: [&str; 6] = [
    SMOKE_CONTENTS,
    "hempdustry:potency",
    CHARGES,
    COOLDOWN_UNTIL,
    "hempdustry:strength",
    "hempdustry:quality",
];

/// The smoke advancement trigger (`SmokeCriterion`).
const SMOKE_TRIGGER: &str = "hempdustry:smoke";

const STRAINS: &str = "hempdustry:strains";
const DEVICES: &str = "hempdustry:devices";
const GREEN_OUT: &str = "hempdustry:green_out";
const EDIBLES: &str = "hempdustry:edibles";

/// `SoundSource.PLAYERS`.
const PLAYERS: usize = 7;
const CREATIVE: i32 = 1;
/// `SmokingDeviceItem.WISP_EVERY_TICKS`.
const WISP_EVERY_TICKS: i32 = 5;
const OVERWORLD: &str = "minecraft:overworld";

/// The pieces of the hemp outfit, head to feet.
const OUTFIT: [(EquipmentSlot, &str); 4] = [
    (EquipmentSlot::Head, "hempdustry:hemp_beanie"),
    (EquipmentSlot::Chest, "hempdustry:hemp_shirt"),
    (EquipmentSlot::Legs, "hempdustry:hemp_harem_pants"),
    (EquipmentSlot::Feet, "hempdustry:flip_flops"),
];

/// What lands a while after a hit (`SmokeScheduler`).
#[derive(Debug)]
enum Later {
    Exhale(Exhale),
    Effects(Vec<Effect>),
    Sound(&'static str),
}

#[derive(Debug)]
struct Pending {
    player: u128,
    ticks_left: i32,
    what: Later,
}

#[derive(Default)]
pub(crate) struct Smoking {
    catalog: RwLock<Arc<Catalog>>,
    /// Each player's cooldown end, in game ticks.
    cooldowns: Mutex<HashMap<u128, i64>>,
    pending: Mutex<Vec<Pending>>,
    dice: Mutex<SplitMix>,
}

/// Vanilla's category of an effect; an effect Horizon doesn't know counts as neutral.
fn category(id: &str) -> Category {
    use horizon_client::brewing::Category as C;
    match horizon_client::brewing::effect(id).map(|e| e.category) {
        Some(C::Beneficial) => Category::Beneficial,
        Some(C::Harmful) => Category::Harmful,
        _ => Category::Neutral,
    }
}

impl Smoking {
    fn catalog(&self) -> Arc<Catalog> {
        Arc::clone(&self.catalog.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub(crate) fn component_types(event: &mut ComponentTypesEvent<'_>) {
        event
            .types
            .extend(COMPONENT_TYPES.iter().map(|&id| id.to_owned()));
    }

    pub(crate) fn add_reload_listeners(event: &mut AddServerReloadListenersEvent<'_>) {
        for (id, folder) in [
            (STRAINS, "hempdustry/strain"),
            (DEVICES, "hempdustry/device"),
            (GREEN_OUT, "hempdustry/green_out"),
            (EDIBLES, "hempdustry/edible"),
        ] {
            event.listeners.push(NativeReloadListener::json(id, folder));
        }
    }

    /// A data load hands over one folder's files: the strains reload with `/reload` too,
    /// where the Java mod needs a restart.
    pub(crate) fn reload(&self, event: &ServerReloadListenerApplyEvent<'_>) {
        let set: fn(&mut Catalog, &[(String, Value)]) = match event.listener.id.as_str() {
            STRAINS => Catalog::set_strains,
            DEVICES => Catalog::set_devices,
            GREEN_OUT => Catalog::set_green_out,
            EDIBLES => Catalog::set_edibles,
            // Another mod's folder: no lock, and no copy of a catalog a hit still reads.
            _ => return,
        };
        let mut guard = self.catalog.write().unwrap_or_else(PoisonError::into_inner);
        set(Arc::make_mut(&mut guard), event.files);
    }

    /// What `stack` has loaded, its strains resolved against the data.
    fn contents(&self, host: &dyn Host, stack: &ItemStack) -> SmokeContents {
        match host.component(stack, SMOKE_CONTENTS) {
            Ok(Some(tag)) => SmokeContents::parse(&json_of(tag), &self.catalog()),
            _ => SmokeContents::default(),
        }
    }

    /// Whether `stack` names any load, read without the data (a client has none).
    fn looks_packed(host: &dyn Host, stack: &ItemStack) -> bool {
        match host.component(stack, SMOKE_CONTENTS) {
            Ok(Some(tag)) => match json_of(tag) {
                Value::Array(list) => !list.is_empty(),
                value => value["entries"].as_array().is_some_and(|l| !l.is_empty()),
            },
            _ => false,
        }
    }

    fn on_cooldown(&self, host: &dyn Host, player: u128) -> bool {
        let now = game_time(host);
        self.cooldowns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&player)
            .is_some_and(|&until| now < until)
    }

    /// `use`: the spliff, the pipe and the vaporizer hit on the click; the bong is drawn on
    /// over time and hits when the draw is done.
    pub(crate) fn use_item(&self, host: &mut dyn Host, event: &UseItemEvent<'_>) -> Flow {
        let Some(device) = Device::of_item(&event.stack.item) else {
            return Flow::Continue;
        };
        if device.draw_ticks() > 0 {
            return Flow::Continue;
        }
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        let contents = self.contents(host, event.stack);
        if contents.is_empty() || self.on_cooldown(host, player.uuid) {
            return Flow::Continue;
        }
        let used = event.stack.clone();
        let slot = hand_slot(host, &used);
        let left = self.hit(host, &player, device, &contents, used.clone());
        if let Some(slot) = slot
            && let Some(stack) = host.carried_stack(slot)
        {
            match left {
                Some(left) => *stack = left,
                None => stack.count = 0,
            }
        }
        Flow::Cancel
    }

    /// `getUseDuration`: a packed bong is drawn on for its draw time.
    pub(crate) fn use_duration(host: &dyn Host, event: &mut UseDurationEvent<'_>) {
        if let Some(device) = Device::of_item(&event.stack.item)
            && device.draw_ticks() > 0
            && Self::looks_packed(host, event.stack)
        {
            event.duration = device.draw_ticks();
        }
    }

    /// `hempdustry:has/<component>`: the build writes our item definitions' `has_component`
    /// conditions on a component of ours this way (Horizon reads that condition for vanilla's
    /// components only), and it holds as vanilla's would: the stack carries the component.
    pub(crate) fn item_model_property(event: &mut ItemModelPropertyEvent<'_>) {
        if let Some(component) = event.property.strip_prefix("hempdustry:has/") {
            let id = format!("hempdustry:{component}");
            event.value = matches!(event.stack.patch.get(&id), Some(Some(_)));
        }
    }

    /// `getName` and `appendHoverText` of the spliff, the devices and the moon rock: a loaded
    /// one is named for its load ("Purple Kush Bong III"), and a spliff or moon rock carrying
    /// hash says so under the name ("With Hashish", grey italic). A custom name wins, as in
    /// Java. Horizon has no item-name hook, so the name changes in the tooltip only.
    pub(crate) fn tooltip(&self, host: &dyn Host, event: &mut TooltipEvent<'_>) {
        let stack = event.stack;
        // Asked every frame for every hovered stack: leave others' items before any lookup.
        if !stack.item.starts_with("hempdustry:") {
            return;
        }
        let Ok(Some(tag)) = host.component(stack, SMOKE_CONTENTS) else {
            return;
        };
        let catalog = self.catalog();
        let labels = Labels::of(&catalog);
        let contents = labels.contents(&json_of(tag));
        if stack.custom_name().is_none()
            && let Some(name) = name::packed_name(&stack.item, &contents, labels)
            && let Some((line, _)) = event.lines.first_mut()
        {
            *line = name_text(&name);
        }
        if let Some(hash) = name::with_line(&stack.item, &contents, labels) {
            let text = Text::translate(name::WITH, &[Text::translate(hash, &[])])
                .with_color(GRAY)
                .italic();
            // Right under the name, where `appendHoverText` puts it.
            let at = event.lines.len().min(1);
            event.lines.insert(at, (text, u32::MAX));
        }
    }

    /// `getUseAnimation`: drawn on like a goat horn.
    pub(crate) fn use_animation(host: &dyn Host, event: &mut UseAnimationEvent<'_>) {
        if let Some(device) = Device::of_item(&event.stack.item)
            && device.draw_ticks() > 0
            && Self::looks_packed(host, event.stack)
        {
            *event.animation = UseAnimation::TootHorn;
        }
    }

    /// No draw while every smokeable cools down, or from a load the data doesn't know.
    pub(crate) fn use_start(
        &self,
        host: &dyn Host,
        event: &LivingEntityUseItemStartEvent<'_>,
    ) -> Flow {
        if event.entity.is_some() || Device::of_item(&event.stack.item).is_none() {
            return Flow::Continue;
        }
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        if self.on_cooldown(host, player.uuid) || self.contents(host, event.stack).is_empty() {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    /// `onUseTick`: the bong bubbles as the draw starts (on the drawer's client) and wisps of
    /// smoke rise from its bowl (on the server).
    pub(crate) fn use_tick(host: &mut dyn Host, event: &OnUseTickEvent<'_>) {
        let Some(device) = Device::of_item(&event.stack.item) else {
            return;
        };
        if event.entity.is_some() {
            return;
        }
        let drawn = device.draw_ticks() - event.remaining;
        if drawn == 0
            && device == Device::Bong
            && let Some(player) = host.player()
        {
            host.play_local_sound(
                "hempdustry:bonghit",
                [player.x, player.y, player.z],
                1.0,
                1.0,
                0,
            );
        }
        if drawn % WISP_EVERY_TICKS == 0
            && let Some(player) = host.acting_player()
        {
            let at = bowl(&player);
            let smoke = ParticleOptions::simple(ParticleType::Smoke);
            host.send_particles(&smoke, None, at, [0.02, 0.02, 0.02], 0.005, 1);
        }
    }

    /// `finishUsingItem`: the draw is done, the bong hits.
    pub(crate) fn use_finish(
        &self,
        host: &mut dyn Host,
        event: &mut LivingEntityUseItemFinishEvent<'_>,
    ) {
        let Some(device) = Device::of_item(&event.stack.item) else {
            return;
        };
        if event.entity.is_some() {
            return;
        }
        let Some(player) = host.acting_player() else {
            return;
        };
        let Some(held) = event.result.clone() else {
            return;
        };
        let contents = self.contents(host, &held);
        if contents.is_empty() {
            return;
        }
        let left = self.hit(host, &player, device, &contents, held.clone());
        *event.result = Some(left.unwrap_or(ItemStack { count: 0, ..held }));
    }

    /// `Smoking.expire`: the cooldown mark leaves the stack once it lapses, so the stack merges
    /// with its like again.
    pub(crate) fn inventory_tick(host: &mut dyn Host, event: &InventoryTickEvent) {
        let now = game_time(host);
        let Some(stack) = host.carried_stack(event.slot) else {
            return;
        };
        if Device::of_item(&stack.item).is_none() {
            return;
        }
        let mut stack = stack.clone();
        let until = match host.component(&stack, COOLDOWN_UNTIL) {
            Ok(Some(tag)) => number(tag),
            _ => return,
        };
        if until.is_some_and(|until| now < until) {
            return;
        }
        if host.remove_component(&mut stack, COOLDOWN_UNTIL).is_ok()
            && let Some(slot) = host.carried_stack(event.slot)
        {
            *slot = stack;
        }
    }

    /// One hit from `stack`, which `player` holds: the hit's effects and sounds, the cooldown,
    /// and what is left of the stack (None: used up or broken).
    fn hit(
        &self,
        host: &mut dyn Host,
        player: &NativeServerPlayer,
        device: Device,
        contents: &SmokeContents,
        mut stack: ItemStack,
    ) -> Option<ItemStack> {
        let catalog = self.catalog();
        let policy = Policy::default();
        let rules = Rules {
            catalog: &catalog,
            policy: &policy,
            category: &category,
        };
        let smoker = Smoker {
            full_outfit: OUTFIT.iter().all(|(slot, item)| {
                host.equipment(*slot)
                    .is_some_and(|stack| &*stack.item == *item)
            }),
        };
        let mut dice = self.dice.lock().unwrap_or_else(PoisonError::into_inner);
        let hit = rules.take_hit(device, contents, smoker, &mut *dice)?;
        self.carry_out(host, player, device, &hit, &stack, contents);

        let now = game_time(host);
        if hit.cooldown_ticks > 0 {
            let until = now + i64::from(hit.cooldown_ticks);
            self.cooldowns
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(player.uuid, until);
            let _ = host.set_component(&mut stack, COOLDOWN_UNTIL, Tag::Long(until));
        }

        let creative = player.game_mode == CREATIVE;
        if device == Device::Spliff {
            if !creative {
                stack.count = stack.count.saturating_sub(1);
            }
            return (stack.count > 0).then_some(stack);
        }
        let worn = Worn {
            charges: match host.component(&stack, CHARGES) {
                Ok(Some(tag)) => number(tag).and_then(|n| i32::try_from(n).ok()).unwrap_or(0),
                _ => 0,
            },
            damage: stack.damage,
            unbreaking: unbreaking(&stack),
        };
        let Some(spent) = rules.spend(device, contents, worn, creative, &mut *dice) else {
            return Some(stack);
        };
        drop(dice);
        Self::wear(host, player, stack, spent)
    }

    /// Carries out what a hit leaves of a device.
    fn wear(
        host: &mut dyn Host,
        player: &NativeServerPlayer,
        mut stack: ItemStack,
        spent: Spent,
    ) -> Option<ItemStack> {
        if spent.scorched > 0 {
            let count = u32::try_from(spent.scorched).unwrap_or(0);
            let given = host.give_items("hempdustry:scorched_hemp", count);
            if given < count {
                drop_at(host, player, "hempdustry:scorched_hemp", count - given);
            }
        }
        if spent.broken {
            host.play_sound(
                "minecraft:entity.item.break",
                player.position,
                PLAYERS,
                0.8,
                1.0,
            );
            return None;
        }
        stack.damage = spent.damage;
        if let Some(left) = spent.charges {
            let _ = host.set_component(&mut stack, CHARGES, Tag::Int(left));
        } else {
            let _ = host.remove_component(&mut stack, SMOKE_CONTENTS);
            let _ = host.remove_component(&mut stack, CHARGES);
        }
        Some(stack)
    }

    /// `Smoking.takeHit`'s feedback, in its order: the smoking sound, the green-out or the
    /// effects (the exhale's queued), the advancement, the exhale's smoke, a cough, nausea.
    fn carry_out(
        &self,
        host: &mut dyn Host,
        player: &NativeServerPlayer,
        device: Device,
        hit: &Hit,
        stack: &ItemStack,
        contents: &SmokeContents,
    ) {
        let uuid = player.uuid;
        if hit.sound_delay > 0 {
            self.later(uuid, hit.sound_delay, Later::Sound("hempdustry:smoking"));
        } else {
            host.play_sound("hempdustry:smoking", player.position, PLAYERS, 1.0, 1.0);
        }
        if let Some(green_out) = &hit.green_out {
            if green_out.full {
                for id in &green_out.remove {
                    host.remove_effect(uuid, id);
                }
                if let Some((food, _, exhaustion)) = host.player_food(uuid) {
                    host.set_player_status(uuid, None, Some((food, 0.0, exhaustion)), None);
                }
            }
            give(host, uuid, &green_out.effects);
        } else {
            give(host, uuid, &hit.effects);
            if !hit.exhaled.is_empty() {
                self.later(uuid, hit.exhale_delay, Later::Effects(hit.exhaled.clone()));
            }
        }
        let time_of_day = host
            .level(OVERWORLD)
            .and_then(|level| host.level_day_time(level))
            .unwrap_or(0)
            .rem_euclid(24_000);
        host.trigger_criterion(uuid, SMOKE_TRIGGER, &|criterion| {
            smoke_matches(criterion, time_of_day, stack, contents)
        });
        self.later(uuid, hit.exhale_delay, Later::Exhale(device.exhale()));
        if hit.cough {
            host.play_sound("hempdustry:coughing", player.position, PLAYERS, 1.0, 1.0);
        }
        if let Some(nausea) = &hit.nausea {
            give(host, uuid, std::slice::from_ref(nausea));
        }
    }

    fn later(&self, player: u128, delay: i32, what: Later) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Pending {
                player,
                ticks_left: delay.max(1),
                what,
            });
    }

    /// `SmokeScheduler.tick`, at each server tick's end: what is due lands on its player, if
    /// they are still on and alive.
    pub(crate) fn server_tick(&self, host: &mut dyn Host, event: &ServerTickEvent<'_>) {
        if !matches!(event.phase, ServerTickPhase::ServerTickEnd { .. }) {
            return;
        }
        let due: Vec<Pending> = {
            let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            if pending.is_empty() {
                return;
            }
            for p in pending.iter_mut() {
                p.ticks_left -= 1;
            }
            let (due, rest) = pending.drain(..).partition(|p| p.ticks_left <= 0);
            *pending = rest;
            due
        };
        let players = host.server_players();
        for pending in due {
            let Some(player) = players
                .iter()
                .find(|p| p.uuid == pending.player && p.health > 0.0)
            else {
                continue;
            };
            let Some(level) = host.level(&player.dimension) else {
                continue;
            };
            match pending.what {
                Later::Effects(effects) => give(host, player.uuid, &effects),
                Later::Sound(sound) => {
                    host.level_play_sound(level, sound, player.position, PLAYERS, 1.0, 1.0);
                }
                Later::Exhale(exhale) => {
                    let particle = ParticleOptions::simple(match exhale {
                        Exhale::CampfireSmoke => ParticleType::CampfireCosySmoke,
                        Exhale::Cloud => ParticleType::Cloud,
                    });
                    let at = mouth(player);
                    host.level_send_particles(level, &particle, None, at, [0.02; 3], 0.005, 8);
                }
            }
        }
    }

    /// A player who died loses the exhale's effects still on their way.
    pub(crate) fn respawned(&self, event: &PlayerRespawnedEvent<'_>) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|p| p.player != event.player || !matches!(p.what, Later::Effects(_)));
    }
}

/// `SmokeCriterion.Conditions.matches`: `time` (the day's tick), `strain` (in the load) and
/// `item`, each that is there.
fn smoke_matches(
    criterion: &NativeCriterion<'_>,
    time_of_day: i64,
    stack: &ItemStack,
    contents: &SmokeContents,
) -> bool {
    let conditions = criterion.conditions;
    let time = &conditions["time"];
    let in_window = time["min"].as_i64().is_some_and(|min| time_of_day >= min)
        && time["max"].as_i64().is_some_and(|max| time_of_day <= max);
    if !time.is_null() && !in_window {
        return false;
    }
    if let Some(strain) = conditions["strain"].as_str()
        && !contents
            .entries
            .iter()
            .any(|e| e.strain == crate::smoke::data::namespaced(strain))
    {
        return false;
    }
    criterion.item("item", stack)
}

/// `SmokeContents.packedName` as text: the format filled with the load's name, then a space
/// and vanilla's Roman numeral.
fn name_text(name: &PackedName<'_>) -> Text {
    let text = Text::translate(name.format, &[Text::translate(name.load.key(), &[])]);
    match name.level {
        Some(level) => text
            .append(Text::literal(" "))
            .append(Text::translate(&format!("enchantment.level.{level}"), &[])),
        None => text,
    }
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

/// The game tick, the same in every level.
fn game_time(host: &dyn Host) -> i64 {
    host.level(OVERWORLD)
        .and_then(|level| host.level_game_time(level))
        .unwrap_or(0)
}

/// The Unbreaking level on `stack`.
fn unbreaking(stack: &ItemStack) -> i32 {
    stack
        .extra
        .as_ref()
        .and_then(|extra| {
            extra
                .enchantments
                .iter()
                .find(|(id, _)| id.trim_start_matches("minecraft:") == "unbreaking")
        })
        .map_or(0, |(_, level)| i32::try_from(*level).unwrap_or(i32::MAX))
}

/// The acting player's slot holding `used`, in [`Host::carried_stack`]'s numbers: the main
/// hand's hotbar slot (the first holding that very stack), else the off hand.
fn hand_slot(host: &mut dyn Host, used: &ItemStack) -> Option<i32> {
    if host.equipment(EquipmentSlot::MainHand).as_ref() == Some(used) {
        return (0..9).find(|&slot| host.carried_stack(slot).is_some_and(|s| s == used));
    }
    (host.equipment(EquipmentSlot::OffHand).as_ref() == Some(used)).then_some(-106)
}

/// `Entity.getLookAngle` from the player's yaw and pitch.
fn look(player: &NativeServerPlayer) -> [f64; 3] {
    let pitch = f64::from(player.pitch).to_radians();
    let yaw = -f64::from(player.yaw).to_radians();
    [
        yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
}

fn eye(player: &NativeServerPlayer) -> [f64; 3] {
    let [x, y, z] = player.position;
    [x, y + f64::from(player.eye_height), z]
}

/// `Smoking.spawnExhale`: half a block in front of the face, a little below the eyes.
fn mouth(player: &NativeServerPlayer) -> [f64; 3] {
    let [lx, ly, lz] = look(player);
    let [x, y, z] = eye(player);
    [x + lx * 0.5, y - 0.1 + ly * 0.5, z + lz * 0.5]
}

/// `SmokingDeviceItem.wisp`: the bowl, held in the right hand, ahead and to the side.
fn bowl(player: &NativeServerPlayer) -> [f64; 3] {
    let flat = |yaw: f32| {
        let yaw = -f64::from(yaw).to_radians();
        [yaw.sin(), yaw.cos()]
    };
    let [fx, fz] = flat(player.yaw);
    let [sx, sz] = flat(player.yaw + 90.0);
    let [x, y, z] = eye(player);
    [x + fx * 0.4 + sx * 0.3, y - 0.5, z + fz * 0.4 + sz * 0.3]
}

fn drop_at(host: &mut dyn Host, player: &NativeServerPlayer, item: &str, count: u32) {
    let mut stack = Compound::new();
    stack.insert("id".into(), Tag::String(item.into()));
    stack.insert("count".into(), Tag::Int(i32::try_from(count).unwrap_or(1)));
    let mut nbt = Compound::new();
    nbt.insert("Item".into(), Tag::Compound(stack));
    host.summon(&player.dimension, "minecraft:item", player.position, nbt);
}

fn number(tag: &Tag) -> Option<i64> {
    match tag {
        Tag::Byte(n) => Some(i64::from(*n)),
        Tag::Short(n) => Some(i64::from(*n)),
        Tag::Int(n) => Some(i64::from(*n)),
        Tag::Long(n) => Some(*n),
        _ => None,
    }
}

/// An NBT value as JSON, as the Java codecs read either.
fn json_of(tag: &Tag) -> Value {
    match tag {
        Tag::End => Value::Null,
        Tag::Byte(n) => Value::from(*n),
        Tag::Short(n) => Value::from(*n),
        Tag::Int(n) => Value::from(*n),
        Tag::Long(n) => Value::from(*n),
        Tag::Float(bits) => Value::from(f32::from_bits(*bits)),
        Tag::Double(bits) => Value::from(f64::from_bits(*bits)),
        Tag::ByteArray(bytes) => bytes
            .iter()
            .map(|b| Value::from(i8::from_ne_bytes([*b])))
            .collect(),
        Tag::String(text) => Value::String(text.to_string_lossy()),
        Tag::List(list) => list.iter().map(json_of).collect(),
        Tag::Compound(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.to_string_lossy(), json_of(v)))
                .collect::<Map<_, _>>(),
        ),
        Tag::IntArray(ints) => ints.iter().copied().map(Value::from).collect(),
        Tag::LongArray(longs) => longs.iter().copied().map(Value::from).collect(),
    }
}

/// `SplitMix64`: the hit's rolls. The Java mod rolls on `ThreadLocalRandom`, unseeded too.
struct SplitMix(u64);

impl Default for SplitMix {
    fn default() -> Self {
        Self(RandomState::new().hash_one(0x5EED_u64))
    }
}

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Dice for SplitMix {
    fn next_int(&mut self, bound: i32) -> i32 {
        let Ok(bound) = u64::try_from(bound) else {
            return 0;
        };
        if bound == 0 {
            return 0;
        }
        i32::try_from(self.next() % bound).unwrap_or(0)
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "24 random bits, exact in an f32"
    )]
    fn next_float(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}
