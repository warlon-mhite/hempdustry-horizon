//! The Infuser ([`crate::machine::infuser`]) wired to Horizon: its block entity in the Java mod's
//! keys, its tick (heat read from the block below, `filled` and `infusing` written back), milk
//! poured by hand or by a dispenser, the spout into the container it faces, the comparator, the
//! spill when it breaks, and its menu and screen.
//!
//! A tub's spout cooldown and comparator level are not saved (as in Java): they live here, by
//! level and position, because a comparator is asked from a level hook that can't read a block
//! entity.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError};

use horizon_client::mod_api::{
    AnalogOutputEvent, BlockEntityTickEvent, Compound, ContainerAcceptsEvent, DestroyBlockEvent,
    Direction, DispenseItemEvent, EquipmentSlot, Flow, Hand, HasAnalogOutputEvent, Host,
    Ingredient, ItemStack, LevelDimension, LevelHost, LootDropsEvent, MenuQuickMoveEvent,
    MenuSlotRulesEvent, ModBlockEntity, ModContainer, NativeDispensed, NativeScreen,
    RightClickBlockV2Event, ScreenDrawEvent, Tag,
};

use super::{Machines, block_id};
use crate::farm::interact::Effect;
use crate::host::Hands;
use crate::machine::fuel::ItemFacts;
use crate::machine::infuser::{
    self, DATA_COUNT, Game, Gauge, Hemp, OUTPUT_SLOT, Poured, Quality, SLOT_COUNT, Tub,
};

/// The Infuser: its block, its block entity type and its menu type share the id.
pub(crate) const INFUSER: &str = "hempdustry:infuser";
const INFUSING: &str = "hempdustry:infusing";
/// What may sit under the tub; one with a `lit` must be lit.
const HEAT_SOURCES: &str = "hempdustry:heat_sources";
const STRENGTH: &str = "hempdustry:strength";
const QUALITY: &str = "hempdustry:quality";
const BUCKET: &str = "minecraft:bucket";

/// The tub's saved keys, the Java mod's (`saveAdditional`).
const PROGRESS: &str = "Progress";
const HAVE_MILK: &str = "HaveMilk";
const BATCH_UNWASHED: &str = "BatchUnwashed";
const BATCH_WASHED: &str = "BatchWashed";
const BATCH_SCORCHED: &str = "BatchScorched";
const PREVIEW_SHOWN: &str = "PreviewShown";

/// `InfuserScreenHandler`'s slots, item top-left in the 176 x 166 panel; the two retired slots
/// have none, so the menu skips them.
const SLOTS: [Option<[f32; 2]>; SLOT_COUNT] = [
    None,
    Some([62.0, 17.0]),
    Some([62.0, 53.0]),
    Some([138.0, 35.0]),
    None,
];
/// `InfuserScreen`: the panel, and the sprites right of it in the sheet.
const PANEL: [f32; 2] = [176.0, 166.0];
const BAR: [f32; 4] = [176.0, 0.0, 44.0, 5.0];
const BAR_AT: [f32; 2] = [84.0, 39.0];
const NOTCH: [f32; 4] = [176.0, 5.0, 2.0, 7.0];
const MARK: [f32; 4] = [178.0, 5.0, 2.0, 7.0];
const FLAME: [f32; 4] = [180.0, 5.0, 14.0, 14.0];
const FLAME_AT: [f32; 2] = [99.0, 54.0];
const MILK: [f32; 4] = [194.0, 5.0, 14.0, 12.0];
const MILK_AT: [f32; 2] = [27.0, 37.0];
const SHEET: &str = "hempdustry:gui/container/infuser";

/// The level event a dispenser plays for a click, and for a failure.
const DISPENSE_CLICK: i32 = 1000;
const DISPENSE_FAIL: i32 = 1001;

/// The `hempdustry:infusing` recipe: what counts as milk and as each hemp, and the butter. The
/// first one a data load holds is the machine's (the Java mod's rule).
pub(super) struct Recipe {
    container: Ingredient,
    hemp: Ingredient,
    washed: Ingredient,
    scorched: Option<Ingredient>,
    result: ItemStack,
}

impl Recipe {
    /// Which hemp `stack` is, in absorption's order: a stack two ingredients take is the cheaper.
    fn hemp(&self, stack: &ItemStack) -> Option<Hemp> {
        Hemp::ALL.into_iter().find(|hemp| {
            self.ingredient(*hemp)
                .is_some_and(|ingredient| ingredient.test(stack))
        })
    }

    const fn ingredient(&self, hemp: Hemp) -> Option<&Ingredient> {
        match hemp {
            Hemp::Scorched => self.scorched.as_ref(),
            Hemp::Unwashed => Some(&self.hemp),
            Hemp::Washed => Some(&self.washed),
        }
    }
}

/// The infusing recipe of a data load, with the identity of the JSON it came from.
#[derive(Default)]
pub(super) struct Recipes {
    from: Vec<usize>,
    first: Option<Recipe>,
}

/// What a tub keeps between ticks and doesn't save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Live {
    comparator: i32,
    push_cooldown: i32,
}

/// A tub by level and position.
pub(super) type At = (u8, [i32; 3]);

pub(super) fn at(dimension: LevelDimension, pos: [i32; 3]) -> At {
    let level = match dimension {
        LevelDimension::Overworld => 0,
        LevelDimension::Nether => 1,
        LevelDimension::End => 2,
    };
    (level, pos)
}

/// The tub's questions, answered by the game: the recipe, the stamped butter, the spout.
struct HostGame<'a> {
    host: &'a mut dyn Host,
    recipe: Option<&'a Recipe>,
    /// The block the spout faces, and its side the butter goes in by.
    spout: Option<([i32; 3], Direction)>,
}

impl Game for HostGame<'_> {
    fn is_milk(&self, stack: &ItemStack) -> bool {
        self.recipe
            .is_some_and(|recipe| recipe.container.test(stack))
    }

    fn hemp(&self, stack: &ItemStack) -> Option<Hemp> {
        self.recipe?.hemp(stack)
    }

    fn butter(&self, strength: i32, quality: Quality) -> Option<ItemStack> {
        let mut butter = self.recipe?.result.clone();
        self.host
            .set_component(&mut butter, STRENGTH, Tag::Int(strength))
            .ok()?;
        self.host
            .set_component(&mut butter, QUALITY, Tag::String(quality.name().into()))
            .ok()?;
        Some(butter)
    }

    fn shows(&self, shown: &ItemStack, strength: i32, quality: Quality) -> bool {
        let Some(recipe) = self.recipe else {
            return false;
        };
        shown.item == recipe.result.item
            && shown.count == recipe.result.count
            && matches!(self.host.component(shown, STRENGTH), Ok(Some(Tag::Int(n))) if *n == strength)
            && matches!(
                self.host.component(shown, QUALITY),
                Ok(Some(Tag::String(name)))
                    if name.0.iter().copied().eq(quality.name().encode_utf16())
            )
    }

    /// As a hopper puts it in. The preview is one item, so anything taken is all of it.
    fn pour(&mut self, stack: &ItemStack) -> Poured {
        let Some((pos, side)) = self.spout else {
            return Poured::Nothing;
        };
        if is_air(&self.host.block(pos)) {
            return Poured::Nothing;
        }
        match self.host.insert_item(pos, side, stack.clone()) {
            Some(left) if left.count >= stack.count => Poured::Full,
            _ => Poured::Moved,
        }
    }
}

pub(super) fn is_air(block: &str) -> bool {
    matches!(
        block,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// Whether the block below heats the tub (`isHeatedFrom`): in `#hempdustry:heat_sources`, and
/// lit if it can be.
pub(super) fn heated_from(host: &dyn Host, below: [i32; 3]) -> bool {
    let block = host.block(below);
    host.block_name_in_tag(&block, HEAT_SOURCES)
        .unwrap_or(false)
        && host
            .block_property(below, "lit")
            .is_none_or(|lit| lit == "true")
}

/// The spout's block and the side of it the butter goes in by, from the tub's `facing`.
fn spout(host: &dyn Host, pos: [i32; 3]) -> Option<([i32; 3], Direction)> {
    let facing = match host.block_property(pos, "facing")?.as_str() {
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => return None,
    };
    let [dx, dy, dz] = facing.normal();
    Some(([pos[0] + dx, pos[1] + dy, pos[2] + dz], facing.opposite()))
}

impl Machines {
    /// The infusing recipe, read again when the data load changed it.
    fn refresh_infusing(&self, host: &dyn Host) {
        let found = host.recipes_of_type(INFUSING);
        let from: Vec<usize> = found
            .iter()
            .map(|r| Arc::as_ptr(&r.json) as usize)
            .collect();
        if self
            .infusing
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .from
            == from
        {
            return;
        }
        let first = found.first().and_then(|recipe| {
            let json = &recipe.json;
            Some(Recipe {
                container: host.recipe_ingredient(&json["container"])?,
                hemp: host.recipe_ingredient(&json["hemp"])?,
                washed: host.recipe_ingredient(&json["washed_hemp"])?,
                // Optional: a pack's recipe written before scorched hemp existed still loads.
                scorched: host.recipe_ingredient(&json["scorched_hemp"]),
                result: host.recipe_result(&json["result"])?,
            })
        });
        *self
            .infusing
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Arc::new(Recipes { from, first });
    }

    fn infusing(&self) -> Arc<Recipes> {
        Arc::clone(&self.infusing.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn live(&self, at: At) -> Live {
        self.tubs
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&at)
            .copied()
            .unwrap_or_default()
    }

    fn set_live(&self, at: At, live: Live) {
        self.tubs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(at, live);
    }

    /// Each hemp as an item and how many a stack holds, for a spill.
    fn representatives(&self, recipe: Option<&Recipe>) -> [Option<(ItemStack, u32)>; 3] {
        let items = self.items.read().unwrap_or_else(PoisonError::into_inner);
        Hemp::ALL.map(|hemp| {
            let first = recipe?.ingredient(hemp)?.order.first()?;
            let max = items.get(first).map_or(64, |facts| facts.max_stack);
            Some((ItemStack::new(first, 1), max))
        })
    }

    pub(super) fn infuser_block_entity() -> (String, Compound) {
        let mut nbt = Compound::new();
        save(&mut nbt, &Tub::default());
        (INFUSER.to_owned(), nbt)
    }

    /// `getSlotsForFace`: the hemp slots from above and the sides. Nothing out of the bottom: that
    /// is where the heat is, and Horizon can't hold a Rough preview back from a hopper there.
    /// The spout is the way out.
    pub(super) fn infuser_container() -> ModContainer {
        let hemp: Vec<usize> = infuser::HEMP_SLOTS.collect();
        ModContainer {
            slots: SLOT_COUNT,
            faces: [
                Vec::new(),
                hemp.clone(),
                hemp.clone(),
                hemp.clone(),
                hemp.clone(),
                hemp.clone(),
            ],
            inputs: hemp,
            outputs: Vec::new(),
            menu: Some(INFUSER.to_owned()),
        }
    }

    /// `InfuserBlockEntity.tick`, on the server.
    pub(super) fn tick_infuser(&self, host: &mut dyn Host, event: &mut BlockEntityTickEvent<'_>) {
        self.refresh_infusing(host);
        let recipes = self.infusing();
        let pos = event.pos;
        let dimension = host.dimension();
        let here = at(crate::dimension_of(dimension), pos);
        let heated = heated_from(host, [pos[0], pos[1] - 1, pos[2]]);
        let spout = spout(host, pos);
        let mut live = self.live(here);
        let mut tub = load(&event.entity.nbt);
        tub.push_cooldown = live.push_cooldown;
        let mut slots = event.entity.items.clone();
        slots.resize(SLOT_COUNT, None);
        let timings = self.settings.get().infuser;
        let (ticked, state) = {
            let mut game = HostGame {
                host: &mut *host,
                recipe: recipes.first.as_ref(),
                spout,
            };
            let ticked = tub.tick(&mut slots, heated, timings, &mut game);
            (ticked, tub.state(&slots, timings, &game))
        };
        if ticked.changed {
            let entity = &mut *event.entity;
            entity.items = slots;
            save(&mut entity.nbt, &tub);
        }
        let data = tub.data(timings);
        if event.entity.data != data {
            event.entity.data = data.to_vec();
        }
        for stack in ticked.ejected {
            pop_out(host, dimension, pos, stack);
        }
        for (key, value) in [("filled", state.filled), ("infusing", state.infusing)] {
            let value = if value { "true" } else { "false" };
            if host.block_property(pos, key).as_deref() != Some(value) {
                host.set_block_property(pos, key, value);
            }
        }
        let comparator = tub.comparator(timings);
        if comparator != live.comparator {
            live.comparator = comparator;
            host.update_output_neighbors(dimension, pos);
        }
        live.push_cooldown = tub.push_cooldown;
        self.set_live(here, live);
    }

    /// A right click with milk on an empty tub pours it, as a water bucket fills a cauldron: the
    /// empty comes back (a creative player keeps the full one). Anything else, milk on a full
    /// tub included, falls through to the block, which opens the screen (the lectern's rule).
    pub(crate) fn infuser_right_click(
        &self,
        host: &mut dyn Host,
        event: &RightClickBlockV2Event,
    ) -> Flow {
        if host.block(event.pos) != INFUSER {
            return Flow::Continue;
        }
        let slot = match event.hand {
            Hand::Main => EquipmentSlot::MainHand,
            Hand::Off => EquipmentSlot::OffHand,
        };
        let Some(held) = host.equipment(slot) else {
            return Flow::Continue;
        };
        let Some(empty) = self.pour(host, event.pos, &held) else {
            return Flow::Continue;
        };
        let effects = vec![
            Effect::EmptyHeld(empty.into()),
            Effect::Sound {
                pos: event.pos,
                sound: "minecraft:item.bucket.empty",
                volume: 1.0,
                pitch: 1.0,
            },
        ];
        let dimension = host.dimension();
        Hands { host, dimension }.apply(&held, effects);
        Flow::Cancel
    }

    /// A dispenser facing a tub pours its milk in and keeps the empty; on a full tub it fails and
    /// keeps the milk rather than spitting it out. Other items, and milk facing anything else,
    /// are vanilla's.
    pub(crate) fn infuser_dispense(
        &self,
        host: &mut dyn Host,
        event: &DispenseItemEvent<'_>,
    ) -> Option<NativeDispensed> {
        let [dx, dy, dz] = event.facing.normal();
        let target = [event.pos[0] + dx, event.pos[1] + dy, event.pos[2] + dz];
        if event.stack.count != 1 || host.block(target) != INFUSER {
            return None;
        }
        self.refresh_infusing(host);
        let recipes = self.infusing();
        let recipe = recipes.first.as_ref()?;
        if !recipe.container.test(event.stack) {
            return None;
        }
        match self.pour(host, target, event.stack) {
            Some(empty) => Some((Some(ItemStack::new(&empty, 1)), DISPENSE_CLICK)),
            None => Some((Some(event.stack.clone()), DISPENSE_FAIL)),
        }
    }

    /// Pours `milk` into the tub at `pos` if it counts as milk and the tub is empty: the tub
    /// fills at once (a quick second click already sees it full) and the empty container's id
    /// comes back, the milk's craft remainder or else a bucket.
    fn pour(&self, host: &mut dyn Host, pos: [i32; 3], milk: &ItemStack) -> Option<String> {
        self.refresh_infusing(host);
        let recipes = self.infusing();
        if !recipes
            .first
            .as_ref()
            .is_some_and(|recipe| recipe.container.test(milk))
        {
            return None;
        }
        if load(&host.mod_block_entity(pos)?.nbt).have_milk {
            return None;
        }
        let mut poured = false;
        host.change_mod_block_entity(pos, &mut |entity: &mut ModBlockEntity| {
            let mut tub = load(&entity.nbt);
            poured = tub.pour_milk();
            if poured {
                save(&mut entity.nbt, &tub);
            }
        });
        if !poured {
            return None;
        }
        host.set_block_property(pos, "filled", "true");
        host.play_sound(
            "minecraft:item.bucket.empty",
            centre(pos),
            SOUND_BLOCKS,
            1.0,
            1.0,
        );
        let items = self.items.read().unwrap_or_else(PoisonError::into_inner);
        Some(
            items
                .get(&*milk.item)
                .and_then(|facts: &ItemFacts| facts.remainder.clone())
                .unwrap_or_else(|| BUCKET.to_owned()),
        )
    }

    /// A player breaks the tub: the preview goes in the bin and the dissolved hemp joins the
    /// slots the game spills next (the two retired slots and the emptied output have room for
    /// the three hemps).
    pub(crate) fn infuser_destroyed(&self, host: &mut dyn Host, event: &DestroyBlockEvent) {
        if host.block(event.pos) != INFUSER {
            return;
        }
        let recipes = self.infusing();
        let representatives = self.representatives(recipes.first.as_ref());
        host.change_mod_block_entity(event.pos, &mut |entity: &mut ModBlockEntity| {
            let mut tub = load(&entity.nbt);
            let mut slots = entity.items.clone();
            slots.resize(SLOT_COUNT, None);
            let spilled = tub.spill(&mut slots, &|hemp| {
                representatives[hemp_index(hemp)].clone()
            });
            // The retired slots are emptied every tick, and the preview has just gone: room for
            // the three hemps, at most 24 of each.
            let free: Vec<usize> = [
                infuser::RETIRED_MILK_SLOT,
                OUTPUT_SLOT,
                infuser::RETIRED_BUCKET_SLOT,
            ]
            .into_iter()
            .filter(|&slot| slots[slot].is_none())
            .collect();
            for (slot, stack) in free.into_iter().zip(spilled) {
                slots[slot] = Some(stack);
            }
            entity.items = slots;
            save(&mut entity.nbt, &tub);
        });
        self.forget(crate::dimension_of(host.dimension()), event.pos);
    }

    /// An explosion breaks the tub: its loot rolls while it still stands, so the preview is
    /// taken out before the game spills the slots, and the dissolved hemp drops with the block.
    pub(crate) fn infuser_exploded(&self, host: &mut dyn Host, event: &mut LootDropsEvent<'_>) {
        let context = event.context;
        let Some((pos, _)) = context.block else {
            return;
        };
        if context.explosion.is_none() || host.block(pos) != INFUSER {
            return;
        }
        let recipes = self.infusing();
        let representatives = self.representatives(recipes.first.as_ref());
        let mut spilled = Vec::new();
        host.change_mod_block_entity(pos, &mut |entity: &mut ModBlockEntity| {
            let mut tub = load(&entity.nbt);
            let mut slots = entity.items.clone();
            slots.resize(SLOT_COUNT, None);
            spilled = tub.spill(&mut slots, &|hemp| {
                representatives[hemp_index(hemp)].clone()
            });
            entity.items = slots;
            save(&mut entity.nbt, &tub);
        });
        event.drops.extend(spilled);
        self.forget(crate::dimension_of(context.dimension), pos);
    }

    fn forget(&self, dimension: LevelDimension, pos: [i32; 3]) {
        self.tubs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&at(dimension, pos));
    }

    /// Whether `state` is a tub's: the only block with an `infusing` property.
    fn is_tub(level: &dyn LevelHost, state: horizon_client::mod_api::BlockStateId) -> bool {
        level
            .state_with_property(state, "infusing", "false")
            .is_some()
    }

    pub(crate) fn infuser_has_analog_output(
        level: &dyn LevelHost,
        event: &mut HasAnalogOutputEvent,
    ) {
        if Self::is_tub(level, event.state) {
            event.output = true;
        }
    }

    /// The comparator reads the batch's progress, not how full the slots are.
    pub(crate) fn infuser_analog_output(
        &self,
        level: &dyn LevelHost,
        event: &mut AnalogOutputEvent,
    ) {
        if Self::is_tub(level, event.state) {
            let pos = [event.pos.x, event.pos.y, event.pos.z];
            event.output = self.live(at(event.dimension, pos)).comparator;
        }
    }

    /// `Slot.mayPlace`: hemp in the hemp slots; the preview is take-only.
    pub(super) fn infuser_slot_rules(&self, event: &mut MenuSlotRulesEvent<'_>) {
        let recipes = self.infusing();
        let recipe = recipes.first.as_ref();
        event.may_place = infuser::HEMP_SLOTS.contains(&event.slot)
            && recipe.and_then(|r| r.hemp(event.stack)).is_some();
    }

    /// A hopper above or beside the tub puts only hemp in (`canPlaceItemThroughFace`).
    pub(super) fn infuser_accepts(&self, event: &ContainerAcceptsEvent<'_>) -> bool {
        let recipes = self.infusing();
        infuser::HEMP_SLOTS.contains(&event.slot)
            && recipes
                .first
                .as_ref()
                .and_then(|recipe| recipe.hemp(event.stack))
                .is_some()
    }

    /// `quickMoveStack` from the player's inventory: hemp to the hemp slots. The other way, the
    /// preview included, is the game's routing; the batch closes at the next tick.
    pub(super) fn infuser_quick_move(
        &self,
        host: &dyn Host,
        event: &mut MenuQuickMoveEvent<'_>,
    ) -> Flow {
        if event.slot < event.container_slots {
            return Flow::Continue;
        }
        self.refresh_infusing(host);
        let recipes = self.infusing();
        if recipes
            .first
            .as_ref()
            .and_then(|recipe| recipe.hemp(event.stack))
            .is_none()
        {
            return Flow::Continue;
        }
        let max = self
            .items
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&*event.stack.item)
            .map_or(64, |facts| facts.max_stack);
        let mut slots = event.entity.items.clone();
        slots.resize(SLOT_COUNT, None);
        if crate::machine::move_into(&mut slots, infuser::HEMP_SLOTS, event.stack, max) {
            event.entity.items = slots;
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    pub(super) fn infuser_screen() -> NativeScreen {
        NativeScreen {
            width: PANEL[0],
            height: PANEL[1],
            slots: SLOTS.to_vec(),
            inventory_y: 84.0,
            inventory_x: 8.0,
            own_labels: false,
            own_background: true,
        }
    }

    /// `InfuserScreen`: the panel, the simmer bar scaled to this batch's job, the dark notch at
    /// the earliest take and the bright mark at the next grade, the milk and the flame. No numbers.
    pub(super) fn infuser_screen_draw(host: &dyn Host, event: &mut ScreenDrawEvent<'_>) {
        let [ox, oy] = event.origin;
        let panel = [0.0, 0.0, PANEL[0], PANEL[1]];
        if !host.sprite(event.batch, SHEET, panel, [ox, oy, PANEL[0], PANEL[1]]) {
            return;
        }
        let mut data = [0; DATA_COUNT];
        for (to, from) in data.iter_mut().zip(event.data.iter()) {
            *to = *from;
        }
        let gauge = Gauge::read(&data);
        let filled = bar_pixels(gauge.bar());
        if filled > 0.0 {
            let src = [BAR[0], BAR[1], filled, BAR[3]];
            host.sprite(
                event.batch,
                SHEET,
                src,
                [ox + BAR_AT[0], oy + BAR_AT[1], filled, BAR[3]],
            );
        }
        let mut mark = |fraction: f32, sprite: [f32; 4]| {
            let offset =
                (bar_pixels(fraction) - (sprite[2] / 2.0).floor()).clamp(0.0, BAR[2] - sprite[2]);
            let dst = [
                ox + BAR_AT[0] + offset,
                oy + BAR_AT[1] - 1.0,
                sprite[2],
                sprite[3],
            ];
            host.sprite(event.batch, SHEET, sprite, dst);
        };
        mark(gauge.minimum_mark(), NOTCH);
        if let Some(next) = gauge.next_grade_mark() {
            mark(next, MARK);
        }
        if data[infuser::DATA_FILLED] != 0 {
            let dst = [ox + MILK_AT[0], oy + MILK_AT[1], MILK[2], MILK[3]];
            host.sprite(event.batch, SHEET, MILK, dst);
        }
        if data[infuser::DATA_HEATED] != 0 {
            let dst = [ox + FLAME_AT[0], oy + FLAME_AT[1], FLAME[2], FLAME[3]];
            host.sprite(event.batch, SHEET, FLAME, dst);
        }
    }
}

/// `Math.round(fraction * BAR_W)`, in pixels.
fn bar_pixels(fraction: f32) -> f32 {
    crate::smoke::data::java_float(crate::smoke::data::java_round(fraction * BAR[2]))
}

const fn hemp_index(hemp: Hemp) -> usize {
    match hemp {
        Hemp::Scorched => 0,
        Hemp::Unwashed => 1,
        Hemp::Washed => 2,
    }
}

/// `SoundSource.BLOCKS`'s ordinal.
const SOUND_BLOCKS: usize = 4;

fn centre(pos: [i32; 3]) -> [f64; 3] {
    [
        f64::from(pos[0]) + 0.5,
        f64::from(pos[1]) + 0.5,
        f64::from(pos[2]) + 0.5,
    ]
}

/// Pops `stack` out of the top of the tub (`Containers.dropItemStack`, whatever `doTileDrops`
/// says: these are a player's own items).
fn pop_out(host: &mut dyn Host, dimension: &str, pos: [i32; 3], stack: ItemStack) {
    let at = [
        f64::from(pos[0]) + 0.5,
        f64::from(pos[1]) + 1.0,
        f64::from(pos[2]) + 0.5,
    ];
    let mut item = Compound::new();
    item.insert("id".into(), Tag::String((&*stack.item).into()));
    item.insert("count".into(), Tag::Int(1));
    let mut nbt = Compound::new();
    nbt.insert("Item".into(), Tag::Compound(item));
    if let Some(id) = host.summon(dimension, "minecraft:item", at, nbt) {
        host.set_item_entity(dimension, id, stack);
    }
}

/// The tub as the Java mod saves it; missing keys read as nothing (`getIntOr`, `getBooleanOr`).
fn load(nbt: &Compound) -> Tub {
    let int = |key: &str| match nbt.get(&key.into()) {
        Some(Tag::Int(n)) => *n,
        _ => 0,
    };
    let flag = |key: &str| matches!(nbt.get(&key.into()), Some(Tag::Byte(n)) if *n != 0);
    Tub {
        progress: int(PROGRESS),
        have_milk: flag(HAVE_MILK),
        batch: infuser::Batch {
            unwashed: int(BATCH_UNWASHED),
            washed: int(BATCH_WASHED),
            scorched: int(BATCH_SCORCHED),
        },
        preview_shown: flag(PREVIEW_SHOWN),
        ..Tub::default()
    }
}

fn save(nbt: &mut Compound, tub: &Tub) {
    nbt.insert(PROGRESS.into(), Tag::Int(tub.progress));
    nbt.insert(HAVE_MILK.into(), Tag::Byte(i8::from(tub.have_milk)));
    nbt.insert(BATCH_UNWASHED.into(), Tag::Int(tub.batch.unwashed));
    nbt.insert(BATCH_WASHED.into(), Tag::Int(tub.batch.washed));
    nbt.insert(BATCH_SCORCHED.into(), Tag::Int(tub.batch.scorched));
    nbt.insert(PREVIEW_SHOWN.into(), Tag::Byte(i8::from(tub.preview_shown)));
}

/// What the tubs keep between ticks, by level and position.
pub(super) type Tubs = HashMap<At, Live>;

/// The block of an event that may name a whole state is a tub.
pub(super) fn is_infuser(block: &str) -> bool {
    block_id(block) == INFUSER
}
