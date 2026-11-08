//! The machines ([`crate::machine`]) wired to Horizon: each one's block entity, its container and
//! hopper faces, its tick, its menu's slot rules and shift-clicks, and its screen: the
//! Decarboxylator here, the Infuser in [`infuser`]. The Sifting Box ([`sifting`]) is a vessel with
//! no block entity, and the cauldron ([`cauldron`]) is vanilla's.
//!
//! Burn times: a native mod can't ask Horizon how long an item burns, so the items' default
//! components are read as Horizon loads them (`modify_default_components`) and the burn-time
//! providers as a data load hands them over; [`fuel`] evaluates them as Horizon's furnace does.
//! Our own burn times are written out in the same hook, so vanilla's furnaces burn hemp too.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::machine::decarb::{self, Kitchen, Oven};
use crate::machine::fuel::{self, ItemFacts, Providers};
use crate::smoke::data::{java_float, java_round};
use horizon_client::mod_api::{
    AddServerReloadListenersEvent, BlockEntityContainerEvent, BlockEntityEvent,
    BlockEntityTickEvent, BlockEntityTicksEvent, Compound, ContainerAcceptsEvent, Flow, Host,
    Ingredient, ItemStack, MenuQuickMoveEvent, MenuSlotRulesEvent, ModContainer,
    ModifyDefaultComponentsEvent, NativeReloadListener, NativeScreen, ScreenDrawEvent, ScreenEvent,
    ServerReloadListenerApplyEvent, Tag,
};
use infuser::INFUSER;

pub(crate) mod cauldron;
pub(crate) mod infuser;
pub(crate) mod sifting;

/// The Decarboxylator: its block, its block entity type and its menu type share the id.
pub(crate) const DECARBOXYLATOR: &str = "hempdustry:decarboxylator";
const DECARBOXYLATING: &str = "hempdustry:decarboxylating";
const PROVIDERS: &str = "hempdustry:context_int_providers";

/// The oven's saved keys, the Java mod's (`saveAdditional`).
const BURN_TIME: &str = "BurnTime";
const FUEL_TIME: &str = "FuelTime";
const PROGRESS: &str = "Progress";

/// `DecarboxylatorScreenHandler`'s slots, item top-left in the 176 x 166 panel.
const SLOTS: [[f32; 2]; decarb::SLOT_COUNT] = [
    [26.0, 54.0],
    [62.0, 17.0],
    [62.0, 35.0],
    [62.0, 53.0],
    [121.0, 35.0],
];
/// `DecarboxylatorScreen`: the panel, and the flame and arrow drawn from the sheet's right side.
const PANEL: [f32; 2] = [176.0, 166.0];
const FLAME: [f32; 4] = [176.0, 0.0, 14.0, 14.0];
const FLAME_AT: [f32; 2] = [26.0, 36.0];
const ARROW: [f32; 4] = [176.0, 14.0, 24.0, 16.0];
const ARROW_X: f32 = 85.0;
/// The sheet in the GUI atlas: `textures/gui/<path>.png` is `<namespace>:gui/<path>`.
const SHEET: &str = "hempdustry:gui/container/decarboxylator";

/// One `hempdustry:decarboxylating` recipe: what a tray takes, what it gives.
struct Recipe {
    ingredient: Ingredient,
    result: ItemStack,
}

/// The recipes of a data load, with the identity of the JSON they came from, to notice a reload.
#[derive(Default)]
struct Recipes {
    from: Vec<usize>,
    list: Vec<Recipe>,
}

#[derive(Default)]
pub(crate) struct Machines {
    items: RwLock<HashMap<String, ItemFacts>>,
    providers: RwLock<Arc<Providers>>,
    recipes: RwLock<Arc<Recipes>>,
    infusing: RwLock<Arc<infuser::Recipes>>,
    tubs: RwLock<infuser::Tubs>,
    settling: Mutex<Vec<sifting::Settling>>,
    /// Players whose click a box took, and the game time it did.
    claims: Mutex<HashMap<u128, i64>>,
}

/// The game's answers, as the oven asks them.
struct GameKitchen<'a> {
    recipes: &'a Recipes,
    items: &'a HashMap<String, ItemFacts>,
    providers: &'a Providers,
}

impl Kitchen for GameKitchen<'_> {
    fn result_for(&self, stack: &ItemStack) -> Option<ItemStack> {
        self.recipes
            .list
            .iter()
            .find(|recipe| recipe.ingredient.test(stack))
            .map(|recipe| recipe.result.clone())
    }

    fn burn_time(&self, stack: &ItemStack) -> i32 {
        self.items
            .get(&*stack.item)
            .and_then(|facts| facts.burn_time.as_ref())
            .map_or(0, |time| self.providers.burn_ticks(time))
    }

    fn remainder(&self, item: &str) -> Option<ItemStack> {
        self.items
            .get(item)
            .and_then(|facts| facts.remainder.as_deref())
            .map(|left| ItemStack::new(left, 1))
    }

    fn max_stack(&self, stack: &ItemStack) -> u32 {
        self.items
            .get(&*stack.item)
            .map_or(64, |facts| facts.max_stack)
    }
}

/// A block's id, from an event that may name a whole state.
fn block_id(block: &str) -> &str {
    block.split('[').next().unwrap_or(block)
}

impl Machines {
    /// Every item's defaults pass through here as Horizon reads them: ours have their burn time
    /// written out, and what a fuel slot needs to know of each is kept.
    pub(crate) fn modify_default_components(&self, event: &mut ModifyDefaultComponentsEvent<'_>) {
        fuel::inline_own_fuel(event.components);
        let facts = ItemFacts::read(event.item, event.components);
        self.items
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(event.item.to_owned(), facts);
    }

    pub(crate) fn add_reload_listeners(event: &mut AddServerReloadListenersEvent<'_>) {
        event.listeners.push(NativeReloadListener::json(
            PROVIDERS,
            "context_int_provider",
        ));
    }

    pub(crate) fn reload(&self, event: &ServerReloadListenerApplyEvent<'_>) {
        if event.listener.id == PROVIDERS {
            *self
                .providers
                .write()
                .unwrap_or_else(PoisonError::into_inner) =
                Arc::new(Providers::from_files(event.files));
        }
    }

    /// The decarboxylating recipes, read again when the data load changed them.
    fn refresh_recipes(&self, host: &dyn Host) {
        let found = host.recipes_of_type(DECARBOXYLATING);
        let from: Vec<usize> = found
            .iter()
            .map(|r| Arc::as_ptr(&r.json) as usize)
            .collect();
        if self
            .recipes
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .from
            == from
        {
            return;
        }
        let list = found
            .iter()
            .filter_map(|recipe| {
                Some(Recipe {
                    ingredient: host.recipe_ingredient(&recipe.json["ingredient"])?,
                    result: host.recipe_result(&recipe.json["result"])?,
                })
            })
            .collect();
        *self.recipes.write().unwrap_or_else(PoisonError::into_inner) =
            Arc::new(Recipes { from, list });
    }

    /// Runs `f` with the game's answers about items.
    fn with_kitchen<R>(&self, f: impl FnOnce(&GameKitchen<'_>) -> R) -> R {
        let recipes = Arc::clone(&self.recipes.read().unwrap_or_else(PoisonError::into_inner));
        let providers = Arc::clone(
            &self
                .providers
                .read()
                .unwrap_or_else(PoisonError::into_inner),
        );
        let items = self.items.read().unwrap_or_else(PoisonError::into_inner);
        f(&GameKitchen {
            recipes: &recipes,
            items: &items,
            providers: &providers,
        })
    }

    pub(crate) fn block_entity(event: &BlockEntityEvent<'_>) -> Option<(String, Compound)> {
        if infuser::is_infuser(event.block) {
            return Some(Self::infuser_block_entity());
        }
        (block_id(event.block) == DECARBOXYLATOR).then(|| {
            let mut nbt = Compound::new();
            save(&mut nbt, &Oven::default());
            (DECARBOXYLATOR.to_owned(), nbt)
        })
    }

    pub(crate) fn block_entity_ticks(event: &mut BlockEntityTicksEvent<'_>) {
        if block_id(event.block) == DECARBOXYLATOR || infuser::is_infuser(event.block) {
            event.ticks = true;
        }
    }

    /// `getSlotsForFace`, as a furnace: trays from above, fuel from the sides, the collection
    /// slot out the bottom. The fuel slot's leftover bucket stays for a player to take: Horizon
    /// can't let a hopper take the bucket and not the fuel.
    pub(crate) fn block_entity_container(
        event: &BlockEntityContainerEvent<'_>,
    ) -> Option<ModContainer> {
        use decarb::{FUEL_SLOT, OUTPUT_SLOT, SLOT_COUNT};
        if infuser::is_infuser(event.block) {
            return Some(Self::infuser_container());
        }
        (block_id(event.block) == DECARBOXYLATOR).then(|| {
            let trays: Vec<usize> = decarb::TRAYS.collect();
            let side = vec![FUEL_SLOT];
            ModContainer {
                slots: SLOT_COUNT,
                faces: [
                    vec![OUTPUT_SLOT],
                    trays.clone(),
                    side.clone(),
                    side.clone(),
                    side.clone(),
                    side,
                ],
                inputs: [vec![FUEL_SLOT], trays].concat(),
                outputs: vec![OUTPUT_SLOT],
                menu: Some(DECARBOXYLATOR.to_owned()),
            }
        })
    }

    /// `DecarboxylatorBlockEntity.tick`, on the server.
    pub(crate) fn block_entity_tick(
        &self,
        host: &mut dyn Host,
        event: &mut BlockEntityTickEvent<'_>,
    ) {
        if event.entity.id == INFUSER {
            self.tick_infuser(host, event);
            return;
        }
        if event.entity.id != DECARBOXYLATOR {
            return;
        }
        self.refresh_recipes(host);
        let mut oven = load(&event.entity.nbt);
        let mut slots = event.entity.items.clone();
        slots.resize(decarb::SLOT_COUNT, None);
        let ticked = self.with_kitchen(|kitchen| oven.tick(&mut slots, decarb::COOK_TIME, kitchen));
        if ticked.changed {
            let entity = &mut *event.entity;
            entity.items = slots;
            save(&mut entity.nbt, &oven);
        }
        let data = oven.data(decarb::COOK_TIME);
        if event.entity.data != data {
            event.entity.data = data.to_vec();
        }
        if let Some(lit) = ticked.lit {
            host.set_block_property(event.pos, "lit", if lit { "true" } else { "false" });
        }
    }

    /// `Slot.mayPlace` in the oven's menu: no level is at hand, so the recipes and fuels are the
    /// ones the last tick and data load read.
    pub(crate) fn menu_slot_rules(&self, event: &mut MenuSlotRulesEvent<'_>) {
        if event.menu == INFUSER {
            self.infuser_slot_rules(event);
            return;
        }
        if event.menu == DECARBOXYLATOR && event.slot < decarb::SLOT_COUNT {
            event.may_place =
                self.with_kitchen(|kitchen| decarb::accepts(event.slot, event.stack, kitchen));
        }
    }

    /// A hopper or dropper puts only what belongs in a slot (`canPlaceItemThroughFace`).
    pub(crate) fn container_accepts(&self, event: &ContainerAcceptsEvent<'_>) -> Flow {
        if event.entity.id == INFUSER {
            return if self.infuser_accepts(event) {
                Flow::Continue
            } else {
                Flow::Cancel
            };
        }
        if event.entity.id != DECARBOXYLATOR
            || self.with_kitchen(|kitchen| decarb::accepts(event.slot, event.stack, kitchen))
        {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `quickMoveStack` from the player's inventory: what cooks to the trays, fuel to the fuel
    /// slot. The other way, and anything the oven doesn't take, is the game's own routing.
    pub(crate) fn menu_quick_move(
        &self,
        host: &dyn Host,
        event: &mut MenuQuickMoveEvent<'_>,
    ) -> Flow {
        if event.entity.id == INFUSER {
            return self.infuser_quick_move(host, event);
        }
        if event.entity.id != DECARBOXYLATOR || event.slot < event.container_slots {
            return Flow::Continue;
        }
        self.refresh_recipes(host);
        let moved = self.with_kitchen(|kitchen| {
            let Some(range) = decarb::quick_move_slots(event.stack, kitchen) else {
                return false;
            };
            let mut slots = event.entity.items.clone();
            slots.resize(decarb::SLOT_COUNT, None);
            let max = kitchen.max_stack(event.stack);
            let moved = crate::machine::move_into(&mut slots, range, event.stack, max);
            if moved {
                event.entity.items = slots;
            }
            moved
        });
        if moved { Flow::Cancel } else { Flow::Continue }
    }

    pub(crate) fn screen(event: &ScreenEvent<'_>) -> Option<NativeScreen> {
        if infuser::is_infuser(event.block) {
            return Some(Self::infuser_screen());
        }
        (block_id(event.block) == DECARBOXYLATOR).then(|| NativeScreen {
            width: PANEL[0],
            height: PANEL[1],
            slots: SLOTS.iter().copied().map(Some).collect(),
            inventory_y: 84.0,
            inventory_x: 8.0,
            own_labels: false,
            own_background: true,
        })
    }

    /// `DecarboxylatorScreen`: the panel, the flame burning down from the top, and each tray's
    /// arrow filling on its own clock, no numbers (vanilla's furnace convention).
    pub(crate) fn screen_draw(host: &dyn Host, event: &mut ScreenDrawEvent<'_>) {
        if infuser::is_infuser(event.block) {
            Self::infuser_screen_draw(host, event);
            return;
        }
        if block_id(event.block) != DECARBOXYLATOR {
            return;
        }
        let [ox, oy] = event.origin;
        let panel = [0.0, 0.0, PANEL[0], PANEL[1]];
        if !host.sprite(event.batch, SHEET, panel, [ox, oy, PANEL[0], PANEL[1]]) {
            return;
        }
        let data = |i: usize| event.data.get(i).copied().unwrap_or(0);
        let (burn, fuel_time, cook_time) = (
            data(decarb::DATA_BURN_TIME),
            data(decarb::DATA_FUEL_TIME),
            data(decarb::DATA_COOK_TIME),
        );
        if burn > 0 {
            let lit = scaled(burn, fuel_time, FLAME[3]);
            if lit > 0.0 {
                let src = [FLAME[0], FLAME[1] + FLAME[3] - lit, FLAME[2], lit];
                let dst = [
                    ox + FLAME_AT[0],
                    oy + FLAME_AT[1] + FLAME[3] - lit,
                    FLAME[2],
                    lit,
                ];
                host.sprite(event.batch, SHEET, src, dst);
            }
        }
        for tray in 0..decarb::TRAY_COUNT {
            let filled = scaled(
                data(decarb::DATA_FIRST_PROGRESS + tray),
                cook_time,
                ARROW[2],
            );
            if filled > 0.0 {
                let y = oy + SLOTS[decarb::FIRST_TRAY_SLOT + tray][1];
                let src = [ARROW[0], ARROW[1], filled, ARROW[3]];
                host.sprite(event.batch, SHEET, src, [ox + ARROW_X, y, filled, ARROW[3]]);
            }
        }
    }
}

/// `value / of` of `size` pixels, rounded as `Math.round(float)`: the Java screens' bars.
fn scaled(value: i32, of: i32, size: f32) -> f32 {
    if of <= 0 {
        return 0.0;
    }
    let fraction = (java_float(value) / java_float(of)).clamp(0.0, 1.0);
    java_float(java_round(fraction * size))
}

/// The oven as the Java mod saves it; missing keys read as nothing, as `getIntOr` reads them.
fn load(nbt: &Compound) -> Oven {
    let int = |key: &str| match nbt.get(&key.into()) {
        Some(Tag::Int(n)) => *n,
        _ => 0,
    };
    let mut progress = [0; decarb::TRAY_COUNT];
    if let Some(Tag::IntArray(saved)) = nbt.get(&PROGRESS.into()) {
        for (tray, value) in progress.iter_mut().zip(saved) {
            *tray = *value;
        }
    }
    Oven {
        burn_time: int(BURN_TIME),
        fuel_time: int(FUEL_TIME),
        progress,
    }
}

fn save(nbt: &mut Compound, oven: &Oven) {
    nbt.insert(BURN_TIME.into(), Tag::Int(oven.burn_time));
    nbt.insert(FUEL_TIME.into(), Tag::Int(oven.fuel_time));
    nbt.insert(PROGRESS.into(), Tag::IntArray(oven.progress.to_vec()));
}
