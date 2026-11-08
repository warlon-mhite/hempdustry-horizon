//! Hempdustry's behaviour for Horizon: the launcher links this crate into the game and calls
//! [`hooks`]. The rules themselves live in [`farm`], [`smoke`] and [`machine`], with no Horizon
//! host; the rest of this crate wires them to Horizon's hook points.
//!
//! A level hook sees a block only as a state id, so the hooks that must answer at once
//! (`may_place_on`, `can_survive`, the bone meal questions) read [`states::States`], and the work
//! that needs the whole world (a random tick, a bone meal step) is queued and played at the end
//! of that level's tick, with the full host.

mod criteria;
pub mod edible;
mod edibles;
pub mod farm;
mod host;
mod level;
mod loot;
pub mod machine;
mod machines;
pub mod smoke;
mod smoking;
mod states;

use std::sync::{Mutex, OnceLock, PoisonError};

use horizon_client::mod_api::{
    AddServerReloadListenersEvent, AnalogOutputEvent, BlockEntityContainerEvent, BlockEntityEvent,
    BlockEntitySubmitEvent, BlockEntityTickEvent, BlockEntityTicksEvent, CanSurviveEvent,
    ComponentTypesEvent, Compound, ContainerAcceptsEvent, DestroyBlockEvent, DispenseItemEvent,
    Flow, HasAnalogOutputEvent, Hooks, Host, InventoryTickEvent, IsBonemealSuccessEvent,
    IsValidBonemealTargetEvent, ItemModelPropertyEvent, ItemStack, LevelDimension, LevelHost,
    LevelRandomHost, LevelTickPostEvent, LevelWriteHost, LivingEntityUseItemFinishEvent,
    LivingEntityUseItemStartEvent, LootDropsEvent, LootFunctionEvent, LootTableLoadEvent,
    MayPlaceOnEvent, MenuQuickMoveEvent, MenuSlotRulesEvent, ModContainer,
    ModifyDefaultComponentsEvent, NativeDispensed, NativeScreen, OnUseTickEvent,
    PerformBonemealEvent, PlaceBlockEvent, PlayerRespawnedEvent, RandomSource, RandomTickEvent,
    RightClickBlockV2Event, ScreenDrawEvent, ScreenEvent, ServerReloadListenerApplyEvent,
    ServerTickEvent, StandaloneModelsEvent, TickEvent, TooltipEvent, UseAnimationEvent,
    UseDurationEvent, UseItemEvent, UseItemOnEvent,
};

use farm::harvest;
use farm::interact::{self, Held};
use farm::medium::Medium;
use farm::model::{Block, Pos, above};
use farm::{Config, plant};
use host::{HostWorld, StateText};
use level::LevelWorld;
use states::{Defaults, States};

/// The game calls this once, at start.
#[must_use]
pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Hempdustry::default())
}

/// What a level's tick end plays for a plant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Work {
    RandomTick,
    BoneMeal,
}

#[derive(Debug)]
struct Job {
    /// A level hook names only the three built-in dimensions; a datapack's own level reads as
    /// the Overworld there.
    dimension: LevelDimension,
    pos: Pos,
    work: Work,
}

#[derive(Default)]
struct Hempdustry {
    defaults: OnceLock<Defaults>,
    states: OnceLock<States>,
    queue: Mutex<Vec<Job>>,
    smoking: smoking::Smoking,
    edibles: edibles::Edibles,
    machines: machines::Machines,
}

impl Hempdustry {
    /// The state table, built in the first level hook once the defaults are known.
    fn states(&self, level: &dyn LevelHost) -> Option<&States> {
        if let Some(states) = self.states.get() {
            return Some(states);
        }
        let defaults = self.defaults.get()?;
        Some(self.states.get_or_init(|| States::build(level, defaults)))
    }

    fn push(&self, dimension: LevelDimension, pos: Pos, work: Work) {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Job {
                dimension,
                pos,
                work,
            });
    }

    /// The queued work of the level `dimension`, taken out of the queue.
    fn take(&self, dimension: &str) -> Vec<Job> {
        let dimension = dimension_of(dimension);
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let (mine, rest) = queue.drain(..).partition(|job| job.dimension == dimension);
        *queue = rest;
        mine
    }
}

fn pos_of(pos: horizon_client::mod_api::BlockPos) -> Pos {
    [pos.x, pos.y, pos.z]
}

/// The built-in dimension a level id is, as a level hook names it.
fn dimension_of(id: &str) -> LevelDimension {
    match id {
        "minecraft:the_nether" => LevelDimension::Nether,
        "minecraft:the_end" => LevelDimension::End,
        _ => LevelDimension::Overworld,
    }
}

/// Water boils away in the Nether (vanilla's `WATER_EVAPORATES` environment attribute there).
fn boils_away(dimension: LevelDimension) -> bool {
    dimension == LevelDimension::Nether
}

/// What the player holds, as the click rules care.
fn held(host: &dyn Host, stack: &ItemStack) -> Held {
    let tagged = |tag| host.server_item_in_tag(stack, tag).unwrap_or(false);
    match &*stack.item {
        "minecraft:water_bucket" => Held::WaterBucket,
        "minecraft:potion"
            if stack.extra.as_ref().and_then(|e| e.potion.as_deref())
                == Some("minecraft:water") =>
        {
            Held::WaterBottle
        }
        "minecraft:shears" => Held::Shears,
        "minecraft:bone_meal" => Held::Fertilizer,
        _ if tagged("c:tools/shear") => Held::Shears,
        _ if tagged("c:fertilizers") => Held::Fertilizer,
        _ => Held::Other,
    }
}

impl Hooks for Hempdustry {
    /// The default states, read once the registries are up.
    fn tick(&self, host: &mut dyn Host, _event: &mut TickEvent) {
        if self.defaults.get().is_none()
            && let Some(defaults) = Defaults::read(host)
        {
            let _ = self.defaults.set(defaults);
        }
    }

    /// A plant's lower segment takes its own random tick, at the end of this level's tick.
    fn random_tick(&self, level: &mut dyn LevelWriteHost, event: &mut RandomTickEvent) -> Flow {
        let Some(states) = self.states(level) else {
            return Flow::Continue;
        };
        let Some(plant) = states.plant(event.state) else {
            return Flow::Continue;
        };
        if plant.is_lower() {
            self.push(event.dimension, pos_of(event.pos), Work::RandomTick);
        }
        Flow::Cancel
    }

    fn level_tick_post(&self, host: &mut dyn Host, event: &mut LevelTickPostEvent<'_>) {
        self.machines.settle_boxes(host, event.dimension);
        let jobs = self.take(event.dimension);
        let Some(states) = self.states.get() else {
            return;
        };
        if jobs.is_empty() {
            return;
        }
        let mut world = HostWorld::new(host, event.dimension, states, Config::default());
        for job in jobs {
            match job.work {
                Work::RandomTick => plant::random_tick(&mut world, job.pos),
                Work::BoneMeal => plant::bone_meal(&mut world, job.pos),
            }
        }
    }

    fn may_place_on(&self, level: &dyn LevelHost, event: &mut MayPlaceOnEvent) -> Option<bool> {
        let states = self.states(level)?;
        let plant = states.plant(event.state)?;
        let ground = states.block(event.ground);
        Some(plant::may_stand_on(
            plant,
            ground,
            states.is_soil(event.ground),
        ))
    }

    /// A segment above the root stands on its own plant whatever the light (a crop's own rule
    /// asks for light, which only the root needs).
    fn can_survive(&self, level: &dyn LevelHost, event: &mut CanSurviveEvent) -> Option<bool> {
        let states = self.states(level)?;
        let plant = states.plant(event.state)?;
        if plant.is_lower() {
            return None;
        }
        let below = level.block_state(above(pos_of(event.pos), -1));
        Some(plant::may_stand_on(plant, states.block(below), false))
    }

    fn is_valid_bonemeal_target(
        &self,
        level: &dyn LevelHost,
        event: &mut IsValidBonemealTargetEvent,
    ) -> Option<bool> {
        let states = self.states(level)?;
        let pos = pos_of(event.pos);
        match states.block(event.state) {
            Block::Plant(plant) => {
                let world = LevelWorld { level, states };
                Some(plant::takes_bone_meal(&world, pos, plant))
            }
            block => Medium::of(block).map(|medium| medium.takes_food(boils_away(event.dimension))),
        }
    }

    fn is_bonemeal_success(
        &self,
        level: &mut dyn LevelRandomHost,
        event: &mut IsBonemealSuccessEvent,
    ) {
        let Some(states) = self.states(level) else {
            return;
        };
        match states.block(event.state) {
            Block::Plant(_) => {
                if let Some(random) = level.random() {
                    event.success = random
                        .next_int_bound(plant::BONEMEAL_FAILURE_CHANCE)
                        .unwrap_or(1)
                        != 0;
                }
            }
            Block::Pot(_) | Block::Tray(_) => event.success = true,
            _ => {}
        }
    }

    fn perform_bonemeal(&self, level: &mut dyn LevelWriteHost, event: &mut PerformBonemealEvent) {
        let Some(states) = self.states(level) else {
            return;
        };
        let pos = pos_of(event.pos);
        match states.block(event.state) {
            Block::Plant(_) => self.push(event.dimension, pos, Work::BoneMeal),
            Block::Pot(pot) => {
                let fertility = Medium::Pot(pot).fed_once();
                if let Block::Pot(fed) = fertility.block() {
                    level.set_block_property(pos, "fertility", &fed.fertility.to_string(), 2);
                }
            }
            Block::Tray(_) => {
                level.set_block_property(pos, "fed", "true", 2);
            }
            _ => {}
        }
    }

    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        // A water cauldron washes hemp and rets stems (`ModCauldronBehaviors`).
        if machines::cauldron::use_on(host, event) == Flow::Cancel {
            return Flow::Cancel;
        }
        let Some(states) = self.states.get() else {
            return Flow::Continue;
        };
        let dimension = host.dimension();
        let ours = host
            .level(dimension)
            .and_then(|level| host.level_block_state(level, event.pos))
            .is_some_and(|text| {
                !matches!(
                    StateText::parse(&text).block(),
                    Block::Other | Block::Air | Block::Farmland { .. }
                )
            });
        if !ours {
            return Flow::Continue;
        }
        let held = held(host, event.stack);
        let mut world = HostWorld::new(host, dimension, states, Config::default());
        let mut effects = Vec::new();
        let claimed = interact::use_item_on(
            &mut world,
            held,
            event.pos,
            boils_away(dimension_of(dimension)),
            &mut effects,
        );
        world.apply(event.stack, effects);
        if claimed {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    /// A player breaking any block of a plant takes it whole, yielding once; breaking a bed takes
    /// the plant on it first. The game then breaks the block itself as usual.
    fn destroy_block(&self, host: &mut dyn Host, event: &mut DestroyBlockEvent) -> Flow {
        self.machines.infuser_destroyed(host, event);
        self.machines.press_destroyed(host, event);
        let Some(states) = self.states.get() else {
            return Flow::Continue;
        };
        let pos = event.pos;
        let block = states.block(event.state);
        let dimension = host.dimension();
        let mut world = HostWorld::new(host, dimension, states, Config::default());
        let harvest = match block {
            Block::Plant(_) => harvest::of_break(&world, pos),
            Block::Pot(_) | Block::Tray(_) => harvest::of_bed_break(&world, pos),
            _ => None,
        };
        if let Some(harvest) = harvest {
            world.harvest(&harvest);
        }
        Flow::Continue
    }

    /// Rebuilds the `hempdustry:spoiling` entries, which Horizon's engines can't run, from
    /// vanilla entries and the two functions below ([`loot`]).
    fn loot_table_load(&self, event: &mut LootTableLoadEvent<'_>) {
        loot::rewrite_table(event.table);
    }

    fn loot_function(&self, event: &mut LootFunctionEvent<'_>) {
        match event.kind {
            loot::KEEP => {
                let level = event
                    .ctx
                    .tool
                    .as_ref()
                    .map_or(0, |tool| loot::level_of(event.data, &tool.enchantments));
                event.stack.count =
                    loot::keep(event.data, event.ctx.seed, level, event.stack.count);
            }
            loot::SPOILED => event.stack.count = loot::take_spoiled(),
            _ => {}
        }
    }

    // Smoking (`smoking`): the components, the data, a hit from each smokeable, the bong's
    // draw, the exhale and the shared cooldown.

    fn component_types(&self, event: &mut ComponentTypesEvent<'_>) {
        smoking::Smoking::component_types(event);
    }

    fn add_server_reload_listeners(
        &self,
        _host: &mut dyn Host,
        event: &mut AddServerReloadListenersEvent<'_>,
    ) {
        smoking::Smoking::add_reload_listeners(event);
        machines::Machines::add_reload_listeners(event);
    }

    fn server_reload_listener_apply(
        &self,
        _host: &mut dyn Host,
        event: &mut ServerReloadListenerApplyEvent<'_>,
    ) {
        self.smoking.reload(event);
        self.machines.reload(event);
    }

    fn use_item(&self, host: &mut dyn Host, event: &mut UseItemEvent<'_>) -> Flow {
        // What the client sends after a click a Sifting Box took: Java's client never would.
        if self.machines.took_click(host) {
            return Flow::Cancel;
        }
        self.smoking.use_item(host, event)
    }

    fn use_duration(&self, host: &dyn Host, event: &mut UseDurationEvent<'_>) {
        smoking::Smoking::use_duration(host, event);
    }

    fn use_animation(&self, host: &dyn Host, event: &mut UseAnimationEvent<'_>) {
        smoking::Smoking::use_animation(host, event);
    }

    fn item_model_property(&self, _host: &dyn Host, event: &mut ItemModelPropertyEvent<'_>) {
        smoking::Smoking::item_model_property(event);
    }

    fn tooltip(&self, host: &dyn Host, event: &mut TooltipEvent<'_>) {
        self.smoking.tooltip(host, event);
        edibles::Edibles::tooltip(host, event);
    }

    fn living_entity_use_item_start(
        &self,
        host: &mut dyn Host,
        event: &mut LivingEntityUseItemStartEvent<'_>,
    ) -> Flow {
        self.smoking.use_start(host, event)
    }

    fn on_use_tick(&self, host: &mut dyn Host, event: &mut OnUseTickEvent<'_>) {
        smoking::Smoking::use_tick(host, event);
    }

    fn living_entity_use_item_finish(
        &self,
        host: &mut dyn Host,
        event: &mut LivingEntityUseItemFinishEvent<'_>,
    ) {
        self.smoking.use_finish(host, event);
        self.edibles.use_finish(&self.smoking, host, event);
    }

    fn inventory_tick(&self, host: &mut dyn Host, event: &mut InventoryTickEvent) {
        smoking::Smoking::inventory_tick(host, event);
    }

    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        self.smoking.server_tick(host, event);
        self.edibles.server_tick(host, event);
    }

    fn place_block(&self, host: &mut dyn Host, event: &mut PlaceBlockEvent<'_>) {
        edibles::Edibles::place_block(host, event);
    }

    fn player_respawned(&self, _host: &mut dyn Host, event: &mut PlayerRespawnedEvent<'_>) {
        self.smoking.respawned(event);
    }

    // Machines (`machines`): the Decarboxylator's block entity, container, tick, menu and screen,
    // and the items' fuel facts every fuel slot reads.

    fn modify_default_components(&self, event: &mut ModifyDefaultComponentsEvent<'_>) {
        self.machines.modify_default_components(event);
    }

    fn block_entity(
        &self,
        _host: &mut dyn Host,
        event: &mut BlockEntityEvent<'_>,
    ) -> Option<(String, Compound)> {
        machines::Machines::block_entity(event)
    }

    fn block_entity_ticks(&self, _host: &dyn Host, event: &mut BlockEntityTicksEvent<'_>) {
        machines::Machines::block_entity_ticks(event);
    }

    fn block_entity_container(
        &self,
        _host: &dyn Host,
        event: &mut BlockEntityContainerEvent<'_>,
    ) -> Option<ModContainer> {
        machines::Machines::block_entity_container(event)
    }

    fn block_entity_tick(&self, host: &mut dyn Host, event: &mut BlockEntityTickEvent<'_>) {
        self.machines.block_entity_tick(host, event);
    }

    fn menu_slot_rules(&self, event: &mut MenuSlotRulesEvent<'_>) {
        self.machines.menu_slot_rules(event);
    }

    fn container_accepts(&self, _host: &dyn Host, event: &mut ContainerAcceptsEvent<'_>) -> Flow {
        self.machines.container_accepts(event)
    }

    fn menu_quick_move(&self, host: &mut dyn Host, event: &mut MenuQuickMoveEvent<'_>) -> Flow {
        self.machines.menu_quick_move(host, event)
    }

    fn screen(&self, _host: &dyn Host, event: &mut ScreenEvent<'_>) -> Option<NativeScreen> {
        machines::Machines::screen(event)
    }

    fn screen_draw(&self, host: &dyn Host, event: &mut ScreenDrawEvent<'_>) {
        machines::Machines::screen_draw(host, event);
    }

    // The Hemp Press's moving parts, drawn on the client.

    fn standalone_models(&self, event: &mut StandaloneModelsEvent<'_>) {
        machines::Machines::standalone_models(event);
    }

    fn block_entity_submit(&self, host: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
        self.machines.press_submit(host, event);
    }

    // The Infuser's pour, by hand or by a dispenser, its spill when exploded, and its comparator;
    // the Sifting Box's click, settle and comparator.

    fn right_click_block_v2(
        &self,
        host: &mut dyn Host,
        event: &mut RightClickBlockV2Event,
    ) -> Flow {
        if self.edibles.cake_click(&self.smoking, host, event) == Flow::Cancel
            || self.machines.sifting_box_click(host, event) == Flow::Cancel
        {
            return Flow::Cancel;
        }
        self.machines.infuser_right_click(host, event)
    }

    fn dispense_item(
        &self,
        host: &mut dyn Host,
        event: &mut DispenseItemEvent<'_>,
    ) -> Option<NativeDispensed> {
        self.machines.infuser_dispense(host, event)
    }

    fn loot_drops(&self, host: &mut dyn Host, event: &mut LootDropsEvent<'_>) {
        self.machines.infuser_exploded(host, event);
    }

    fn has_analog_output(&self, level: &dyn LevelHost, event: &mut HasAnalogOutputEvent) {
        machines::Machines::infuser_has_analog_output(level, event);
        machines::Machines::sifting_box_has_analog_output(level, event);
    }

    fn analog_output(&self, level: &dyn LevelHost, event: &mut AnalogOutputEvent) {
        self.machines.infuser_analog_output(level, event);
        machines::Machines::sifting_box_analog_output(level, event);
    }
}
