//! The farming rules' [`World`] over Horizon's [`Host`], for the hooks that get one: a level's
//! tick (where queued random ticks and bone meal are played) and a player's click.

use horizon_client::mod_api::{
    EquipmentSlot, Host, ItemStack, LevelRef, NativeHeightmap, NativeServerPlayer, RandomSource,
};

use crate::criteria::{self, HarvestTest};
use crate::farm::harvest::Harvest;
use crate::farm::interact::Effect;
use crate::farm::model::{Block, Condition, Kind, LightSource, Plant, Pos, Pot, Tray, above};
use crate::farm::{Config, World};
use crate::states::States;

/// `Level.setBlock`'s `UPDATE_CLIENTS`: how a crop's own growth writes itself.
const UPDATE_CLIENTS: i32 = 2;

/// `Block.UPDATE_ALL | Block.UPDATE_SUPPRESS_DROPS`: how a harvest takes the blocks that yield nothing.
const UPDATE_ALL_NO_DROPS: i32 = 3 | 32;

/// `GameType.CREATIVE`, as `NativeServerPlayer.game_mode` numbers it.
const CREATIVE: i32 = 1;

/// The hotbar, slots 0 to 8 of `Host::carried_stack`.
const HOTBAR_SLOTS: i32 = 9;

/// The off hand in `Host::carried_stack`'s numbers.
const OFF_HAND: i32 = -106;

/// Blocks that hold water without being water (`FluidState.is(FluidTags.WATER)`), besides any
/// block whose `waterlogged` is true.
const WATERY: [&str; 6] = [
    "minecraft:water",
    "minecraft:bubble_column",
    "minecraft:kelp",
    "minecraft:kelp_plant",
    "minecraft:seagrass",
    "minecraft:tall_seagrass",
];

/// A block state as `/setblock` spells it, `name[key=value,…]`, taken apart.
pub struct StateText<'a> {
    pub name: &'a str,
    props: Vec<(&'a str, &'a str)>,
}

impl<'a> StateText<'a> {
    #[must_use]
    pub fn parse(text: &'a str) -> Self {
        let Some((name, rest)) = text.split_once('[') else {
            return Self {
                name: text,
                props: Vec::new(),
            };
        };
        let props = rest
            .trim_end_matches(']')
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .collect();
        Self { name, props }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.props.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    /// The block as the farming rules see it.
    #[must_use]
    pub fn block(&self) -> Block {
        let flag = |key| self.get(key) == Some("true");
        let number = |key| self.get(key).and_then(|v| v.parse().ok()).unwrap_or(0);
        match self.name {
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" => Block::Air,
            "minecraft:farmland" => Block::Farmland {
                moisture: number("moisture"),
            },
            Pot::BLOCK => Block::Pot(Pot {
                fertility: number("fertility"),
                watered: flag("watered"),
            }),
            Tray::BLOCK => Block::Tray(Tray {
                level: number("level"),
                fed: flag("fed"),
                powered: flag("powered"),
            }),
            name => Kind::from_block(name)
                .and_then(|kind| Plant::read(kind, |key| self.get(key).map(str::to_owned)))
                .map_or(Block::Other, Block::Plant),
        }
    }
}

/// The level a hook runs in, through the host.
pub struct HostWorld<'a> {
    pub host: &'a mut dyn Host,
    pub dimension: &'a str,
    level: Option<LevelRef>,
    states: &'a States,
    config: Config,
}

impl<'a> HostWorld<'a> {
    pub fn new(
        host: &'a mut dyn Host,
        dimension: &'a str,
        states: &'a States,
        config: Config,
    ) -> Self {
        let level = host.level(dimension);
        Self {
            host,
            dimension,
            level,
            states,
            config,
        }
    }

    /// The block state at `pos`, as text; None where nothing is loaded.
    fn text(&self, pos: Pos) -> Option<String> {
        self.host.level_block_state(self.level?, pos)
    }

    fn in_tag(&self, name: &str, tag: &str) -> bool {
        self.host.block_name_in_tag(name, tag).unwrap_or(false)
    }
}

impl World for HostWorld<'_> {
    fn block(&self, pos: Pos) -> Block {
        self.text(pos)
            .map_or(Block::Other, |text| StateText::parse(&text).block())
    }

    fn set(&mut self, pos: Pos, block: Block) -> bool {
        match block {
            Block::Plant(plant) => match self.states.plant_id(&plant) {
                Some(id) => self.host.set_block(self.dimension, pos, id, UPDATE_CLIENTS),
                None => false,
            },
            Block::Air => match self.host.default_state("minecraft:air") {
                Some(id) => self.host.set_block(self.dimension, pos, id, UPDATE_CLIENTS),
                None => false,
            },
            // A bed keeps whatever else it carries (a tray's facing): only its own values move.
            Block::Pot(pot) => {
                let a = self.host.set_block_property_with(
                    pos,
                    "fertility",
                    &pot.fertility.to_string(),
                    UPDATE_CLIENTS,
                );
                let b = self.host.set_block_property_with(
                    pos,
                    "watered",
                    &pot.watered.to_string(),
                    UPDATE_CLIENTS,
                );
                a || b
            }
            Block::Tray(tray) => {
                let a = self.host.set_block_property_with(
                    pos,
                    "level",
                    &tray.level.to_string(),
                    UPDATE_CLIENTS,
                );
                let b = self.host.set_block_property_with(
                    pos,
                    "fed",
                    &tray.fed.to_string(),
                    UPDATE_CLIENTS,
                );
                let c = self.host.set_block_property_with(
                    pos,
                    "powered",
                    &tray.powered.to_string(),
                    UPDATE_CLIENTS,
                );
                a || b || c
            }
            Block::Farmland { .. } | Block::Other => false,
        }
    }

    fn raw_brightness(&self, pos: Pos) -> u8 {
        let (block, sky) = self.host.light(pos);
        block.max(sky)
    }

    fn light_source(&self, pos: Pos) -> LightSource {
        let Some(text) = self.text(pos) else {
            return LightSource::NONE;
        };
        let state = StateText::parse(&text);
        let opaque = self.opaque(pos, state.name);
        let lit = state.get("lit");
        let tier = if self.in_tag(state.name, "hempdustry:grow_lamps") {
            if lit == Some("false") {
                Condition::Natural
            } else {
                Condition::GrowLamp
            }
        } else if self.host.light(pos).0 < 15
            || self.in_tag(state.name, "hempdustry:not_grow_lights")
        {
            // A block's own light is its emission: no block beside a light of 15 reaches 15.
            Condition::Natural
        } else if lit.is_some() {
            Condition::Lamp
        } else {
            Condition::Ambient
        };
        LightSource { tier, opaque }
    }

    fn is_water(&self, pos: Pos) -> bool {
        self.text(pos).is_some_and(|text| {
            let state = StateText::parse(&text);
            WATERY.contains(&state.name) || state.get("waterlogged") == Some("true")
        })
    }

    fn is_beldia_soil(&self, pos: Pos) -> bool {
        self.text(pos)
            .is_some_and(|text| self.in_tag(StateText::parse(&text).name, "hempdustry:beldia_soil"))
    }

    fn roofed(&self, pos: Pos) -> bool {
        self.host
            .surface_heights(
                self.dimension,
                &[(pos[0], pos[2])],
                NativeHeightmap::MotionBlockingNoLeaves,
            )
            .ok()
            .and_then(|heights| heights.first().copied().flatten())
            .is_some_and(|height| height > pos[1])
    }

    fn gassed(&self, _pos: Pos) -> bool {
        // Vanilla's sulfur gas over a potent sulfur block is not read yet: no plant is gassed.
        false
    }

    fn mildew_resistant(&self, kind: Kind) -> bool {
        self.in_tag(kind.block(), "hempdustry:mildew_resistant")
    }

    fn next_float(&mut self) -> f32 {
        let dimension = self.dimension;
        self.host
            .level_random(dimension)
            .map_or(0.5, |random| random.next_float())
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        let dimension = self.dimension;
        self.host
            .level_random(dimension)
            .and_then(|random| random.next_int_bound(bound).ok())
            .unwrap_or(0)
    }

    fn config(&self) -> Config {
        self.config
    }
}

impl HostWorld<'_> {
    /// `BlockState.isSolidRender`, as near as the host tells it: a whole-block collision box that
    /// isn't glass (`#minecraft:impermeable`) or leaves.
    fn opaque(&self, pos: Pos, name: &str) -> bool {
        let boxes = self.host.block_collision(pos);
        let whole = matches!(boxes.as_slice(), [b] if *b == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        whole
            && !self.in_tag(name, "minecraft:impermeable")
            && !self.in_tag(name, "minecraft:leaves")
    }

    /// The acting player's main-hand stack, what their break rolls the loot for. Read from the
    /// acting player's own slots: their entity handle isn't found while their turn runs.
    fn main_hand(&self) -> Option<ItemStack> {
        self.host.equipment(EquipmentSlot::MainHand)
    }

    /// Takes the plant as the acting player breaks one block of it (or its bed): the harvest
    /// advancement for a ripe plant, read before anything goes; the blocks above the lower
    /// segment without drops, top down; then the lower segment with the player's loot, when the
    /// game isn't breaking it itself. A creative player gets no loot.
    pub fn harvest(&mut self, harvest: &Harvest) {
        let Some(player) = self.host.acting_player() else {
            return;
        };
        if harvest.is_ripe() {
            self.trigger_harvest(&player, harvest);
        }
        let air = self.host.default_state("minecraft:air");
        for &pos in &harvest.clear {
            if let Some(air) = air {
                self.host
                    .set_block(self.dimension, pos, air, UPDATE_ALL_NO_DROPS);
            }
        }
        if !harvest.break_lower {
            return;
        }
        if player.game_mode == CREATIVE {
            if let Some(air) = air {
                self.host
                    .set_block(self.dimension, harvest.lower, air, UPDATE_ALL_NO_DROPS);
            }
        } else {
            let tool = self.main_hand();
            self.host.destroy_block(harvest.lower, tool.as_ref(), false);
        }
    }

    /// `HarvestHempCriterion.trigger`: fired twice when a criterion asks about a bed tag the
    /// first pass didn't know (see [`HarvestTest`]).
    fn trigger_harvest(&mut self, player: &NativeServerPlayer, harvest: &Harvest) {
        let bed = self.text(above(harvest.lower, -1)).unwrap_or_default();
        let mut test = HarvestTest::new(&harvest.plant, StateText::parse(&bed));
        let fire = |host: &mut dyn Host, test: &HarvestTest<'_>| {
            host.trigger_criterion(player.uuid, criteria::HARVEST_HEMP, &|criterion| {
                test.matches(criterion.conditions)
            })
        };
        fire(self.host, &test);
        let missing = test.missing();
        if missing.is_empty() {
            return;
        }
        let bed_name = test.bed.name;
        for tag in missing {
            let known = self.in_tag(bed_name, &tag);
            test.tags.insert(tag, known);
        }
        fire(self.host, &test);
    }

    /// The acting player's slot holding `used`, the stack a click used, in
    /// [`Host::carried_stack`]'s numbers: the main hand's hotbar slot, else the off hand. The
    /// player's own slots, because their entity handle isn't found while their turn runs. The
    /// selected slot's number isn't told: it is the first hotbar slot holding that very stack,
    /// the same whichever of two equal stacks it is.
    fn hand_slot(&mut self, used: &ItemStack) -> Option<i32> {
        if self.host.equipment(EquipmentSlot::MainHand).as_ref() == Some(used) {
            return (0..HOTBAR_SLOTS)
                .find(|&slot| self.host.carried_stack(slot).is_some_and(|s| s == used));
        }
        (self.host.equipment(EquipmentSlot::OffHand).as_ref() == Some(used)).then_some(OFF_HAND)
    }

    /// Carries out a click's effects for the acting player, who used `used` (a stack in one of
    /// their hands).
    pub fn apply(&mut self, used: &ItemStack, effects: Vec<Effect>) {
        let Some(player) = self.host.acting_player() else {
            return;
        };
        let creative = player.game_mode == CREATIVE;
        let slot = self.hand_slot(used);
        let mut held = Some(used.clone());
        let mut give = Vec::new();
        for effect in effects {
            match effect {
                Effect::DamageHeld if !creative => held = held.and_then(damaged),
                Effect::ConsumeHeld if !creative => held = held.and_then(|stack| fewer(stack, 1)),
                Effect::EmptyHeld(item) => {
                    if !creative {
                        held = held.and_then(|stack| fewer(stack, 1));
                    }
                    match &held {
                        None => held = Some(plain(item, 1)),
                        Some(_) => give.push(item),
                    }
                }
                Effect::DamageHeld | Effect::ConsumeHeld => {}
                Effect::Drop { pos, item, count } => self.drop_item(pos, item, count),
                Effect::Sound {
                    pos,
                    sound,
                    volume,
                    pitch,
                } => {
                    // Sound source 4 is `SoundSource.BLOCKS`.
                    self.host.play_sound(sound, centre(pos), 4, volume, pitch);
                }
                Effect::Tell(key) => {
                    let line = horizon_client::mod_api::Text::translate(key, &[]);
                    self.host.action_bar(player.uuid, line);
                }
                Effect::GrowthSparks(pos) => {
                    // Level event 1505 as players see it: green sparks and bone meal's sound.
                    let sparks = horizon_client::mod_api::ParticleOptions::simple(
                        horizon_client::mod_api::ParticleType::HappyVillager,
                    );
                    self.host.send_particles(
                        &sparks,
                        None,
                        centre(pos),
                        [0.25, 0.25, 0.25],
                        0.0,
                        15,
                    );
                    self.host
                        .play_sound("minecraft:item.bone_meal.use", centre(pos), 4, 1.0, 1.0);
                }
                Effect::Splash(pos) => {
                    let splash = horizon_client::mod_api::ParticleOptions::simple(
                        horizon_client::mod_api::ParticleType::Splash,
                    );
                    let at = [
                        f64::from(pos[0]) + 0.5,
                        f64::from(pos[1]) + 1.0,
                        f64::from(pos[2]) + 0.5,
                    ];
                    self.host
                        .send_particles(&splash, None, at, [0.5, 0.0, 0.5], 1.0, 5);
                }
            }
        }
        if let Some(slot) = slot
            && held.as_ref() != Some(used)
            && let Some(stack) = self.host.carried_stack(slot)
        {
            match held {
                Some(left) => *stack = left,
                // A count of 0 empties the slot.
                None => stack.count = 0,
            }
        }
        for item in give {
            if creative && self.host.count_items(item) > 0 {
                continue;
            }
            if self.host.give_items(item, 1) == 0 {
                self.drop_item(player_block(player.position), item, 1);
            }
        }
    }

    /// Drops `count` of `item` at `pos` as `Block.popResource` does, near the block's centre.
    fn drop_item(&mut self, pos: Pos, item: &str, count: u32) {
        let mut nbt = horizon_client::mod_api::Compound::new();
        let mut stack = horizon_client::mod_api::Compound::new();
        stack.insert(
            "id".into(),
            horizon_client::mod_api::Tag::String(item.into()),
        );
        stack.insert(
            "count".into(),
            horizon_client::mod_api::Tag::Int(i32::try_from(count).unwrap_or(1)),
        );
        nbt.insert("Item".into(), horizon_client::mod_api::Tag::Compound(stack));
        let at = centre(pos);
        self.host.summon(self.dimension, "minecraft:item", at, nbt);
    }
}

fn centre(pos: Pos) -> [f64; 3] {
    [
        f64::from(pos[0]) + 0.5,
        f64::from(pos[1]) + 0.5,
        f64::from(pos[2]) + 0.5,
    ]
}

#[allow(clippy::cast_possible_truncation)]
fn player_block(position: [f64; 3]) -> Pos {
    [
        position[0].floor() as i32,
        position[1].floor() as i32,
        position[2].floor() as i32,
    ]
}

fn plain(item: &str, count: u32) -> ItemStack {
    ItemStack {
        item: item.into(),
        count,
        damage: 0,
        extra: None,
    }
}

/// `stack` with `n` fewer, or None when that empties it.
fn fewer(mut stack: ItemStack, n: u32) -> Option<ItemStack> {
    stack.count = stack.count.checked_sub(n).filter(|left| *left > 0)?;
    Some(stack)
}

/// Vanilla's shears last 238 uses.
const SHEARS_DURABILITY: u32 = 238;

/// `stack` one use more worn: vanilla's shears break on their last use. A pack's shears, whose
/// durability isn't read here, only wear.
fn damaged(mut stack: ItemStack) -> Option<ItemStack> {
    stack.damage += 1;
    if &*stack.item == "minecraft:shears" && stack.damage >= SHEARS_DURABILITY {
        return None;
    }
    Some(stack)
}
