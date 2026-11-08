//! The Decarboxylator (`DecarboxylatorBlockEntity`): a slow oven of three trays that share one fire
//! and one collection slot. Each tray cooks on its own timer; the fire burns while anything cooks,
//! so a full oven is three times as fuel-efficient as a lone tray; a tray's item is used up when it
//! is done, as a furnace's input is; progress cools off, two ticks a tick, without fire or room.
//!
//! No Horizon host here: what an item cooks into and how long it burns come from a [`Kitchen`].

use std::ops::Range;

use horizon_client::mod_api::ItemStack;

pub const FUEL_SLOT: usize = 0;
/// The three trays are contiguous from here.
pub const FIRST_TRAY_SLOT: usize = 1;
pub const TRAY_COUNT: usize = 3;
/// The trays' slots.
pub const TRAYS: Range<usize> = FIRST_TRAY_SLOT..FIRST_TRAY_SLOT + TRAY_COUNT;
pub const OUTPUT_SLOT: usize = 4;
pub const SLOT_COUNT: usize = 5;
/// Ticks one item spends in a tray: between ore smelting (200) and a campfire (600).
pub const COOK_TIME: i32 = 500;

/// The menu's data slots (`ContainerData`), in the Java mod's order.
pub const DATA_BURN_TIME: usize = 0;
pub const DATA_FUEL_TIME: usize = 1;
pub const DATA_FIRST_PROGRESS: usize = 2;
pub const DATA_COOK_TIME: usize = 5;
pub const DATA_COUNT: usize = 6;

/// What the oven asks the game about items.
pub trait Kitchen {
    /// What one of `stack` cooks into (a `hempdustry:decarboxylating` recipe), None when the trays
    /// won't take it.
    fn result_for(&self, stack: &ItemStack) -> Option<ItemStack>;
    /// Ticks one of `stack` burns in a furnace; 0 when it is no fuel.
    fn burn_time(&self, stack: &ItemStack) -> i32;
    /// What the last of a fuel leaves in its slot (a lava bucket's bucket).
    fn remainder(&self, item: &str) -> Option<ItemStack>;
    /// How many of `stack` one slot holds.
    fn max_stack(&self, stack: &ItemStack) -> u32;
}

/// The oven's own state, saved with it (`BurnTime`, `FuelTime`, `Progress`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Oven {
    /// Ticks of fire left.
    pub burn_time: i32,
    /// What the fuel now burning was worth, for the flame's height.
    pub fuel_time: i32,
    /// Each tray's ticks of cooking.
    pub progress: [i32; TRAY_COUNT],
}

/// What a tick did, for the game to act on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Ticked {
    /// The oven's state or slots changed (the fire burning down included): save it.
    pub changed: bool,
    /// The fire lit or went out: the block's `lit` is now this.
    pub lit: Option<bool>,
}

impl Oven {
    #[must_use]
    pub const fn is_burning(&self) -> bool {
        self.burn_time > 0
    }

    /// The menu's data slots, `cook_time` included so the arrows read the oven's own clock.
    #[must_use]
    pub const fn data(&self, cook_time: i32) -> [i32; DATA_COUNT] {
        [
            self.burn_time,
            self.fuel_time,
            self.progress[0],
            self.progress[1],
            self.progress[2],
            cook_time,
        ]
    }

    /// `DecarboxylatorBlockEntity.tick`.
    pub fn tick(
        &mut self,
        slots: &mut [Option<ItemStack>],
        cook_time: i32,
        kitchen: &dyn Kitchen,
    ) -> Ticked {
        let was_burning = self.is_burning();
        let mut changed = false;
        if self.is_burning() {
            // A change too: the saved state is the oven's only memory.
            self.burn_time -= 1;
            changed = true;
        }
        // Light the fire only when there is something to cook, so no fuel is wasted on an idle
        // oven (a furnace's contract).
        if !self.is_burning() && (0..TRAY_COUNT).any(|tray| can_cook(slots, tray, kitchen)) {
            changed |= self.light(slots, kitchen);
        }
        for tray in 0..TRAY_COUNT {
            if self.is_burning() && can_cook(slots, tray, kitchen) {
                self.progress[tray] += 1;
                if self.progress[tray] >= cook_time {
                    self.progress[tray] = 0;
                    cook(slots, tray, kitchen);
                }
                changed = true;
            } else if self.progress[tray] > 0 {
                // Cools off rather than freezing, as a furnace loses progress without fuel.
                self.progress[tray] = (self.progress[tray] - 2).max(0);
                changed = true;
            }
        }
        let lit = (was_burning != self.is_burning()).then_some(self.is_burning());
        Ticked {
            changed: changed || lit.is_some(),
            lit,
        }
    }

    /// Burns one of the fuel slot's items, if it is a fuel; the last one leaves its remainder.
    fn light(&mut self, slots: &mut [Option<ItemStack>], kitchen: &dyn Kitchen) -> bool {
        let Some(fuel) = slots[FUEL_SLOT].as_mut() else {
            return false;
        };
        let time = kitchen.burn_time(fuel);
        if time <= 0 {
            return false;
        }
        self.burn_time = time;
        self.fuel_time = time;
        fuel.count -= 1;
        if fuel.count == 0 {
            let item = fuel.item.clone();
            slots[FUEL_SLOT] = kitchen.remainder(&item);
        }
        true
    }
}

/// Whether `tray` holds something the oven takes and its result fits the collection slot. The
/// slot is shared, so a tray whose result differs from what it holds waits, as a furnace does.
fn can_cook(slots: &[Option<ItemStack>], tray: usize, kitchen: &dyn Kitchen) -> bool {
    let Some(result) = slots[FIRST_TRAY_SLOT + tray]
        .as_ref()
        .and_then(|input| kitchen.result_for(input))
    else {
        return false;
    };
    match &slots[OUTPUT_SLOT] {
        None => result.count <= kitchen.max_stack(&result),
        Some(output) => {
            output.same_item_same_components(&result)
                && output.count + result.count <= kitchen.max_stack(output)
        }
    }
}

/// One of `tray`'s items, done: its result joins the collection slot.
fn cook(slots: &mut [Option<ItemStack>], tray: usize, kitchen: &dyn Kitchen) {
    let slot = FIRST_TRAY_SLOT + tray;
    let Some(result) = slots[slot]
        .as_ref()
        .and_then(|input| kitchen.result_for(input))
    else {
        return;
    };
    match slots[OUTPUT_SLOT].as_mut() {
        None => slots[OUTPUT_SLOT] = Some(result),
        Some(output) => output.count += result.count,
    }
    if let Some(input) = slots[slot].as_mut() {
        input.count -= 1;
        if input.count == 0 {
            slots[slot] = None;
        }
    }
}

/// `canPlaceItem`: fuel in the fuel slot, what cooks in a tray, nothing in the collection slot.
#[must_use]
pub fn accepts(slot: usize, stack: &ItemStack, kitchen: &dyn Kitchen) -> bool {
    match slot {
        FUEL_SLOT => kitchen.burn_time(stack) > 0,
        OUTPUT_SLOT => false,
        _ => kitchen.result_for(stack).is_some(),
    }
}

/// Where a shift-click from the player's inventory sends `stack` (`quickMoveStack`): the trays when
/// it cooks, else the fuel slot when it burns, else nowhere in the oven.
#[must_use]
pub fn quick_move_slots(stack: &ItemStack, kitchen: &dyn Kitchen) -> Option<Range<usize>> {
    if kitchen.result_for(stack).is_some() {
        Some(TRAYS)
    } else if kitchen.burn_time(stack) > 0 {
        Some(FUEL_SLOT..FUEL_SLOT + 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped recipes (buds 4, leaf 1, schwag 2), coal and a lava bucket as fuel.
    struct Shipped;

    impl Kitchen for Shipped {
        fn result_for(&self, stack: &ItemStack) -> Option<ItemStack> {
            let count = match &*stack.item {
                "hempdustry:indica_buds" | "hempdustry:sativa_buds" => 4,
                "hempdustry:schwag" => 2,
                "hempdustry:hemp_leaf" => 1,
                _ => return None,
            };
            Some(ItemStack::new("hempdustry:decarboxylated_hemp", count))
        }
        fn burn_time(&self, stack: &ItemStack) -> i32 {
            match &*stack.item {
                "minecraft:coal" => 1600,
                "minecraft:lava_bucket" => 20000,
                _ => 0,
            }
        }
        fn remainder(&self, item: &str) -> Option<ItemStack> {
            (item == "minecraft:lava_bucket").then(|| ItemStack::new("minecraft:bucket", 1))
        }
        fn max_stack(&self, stack: &ItemStack) -> u32 {
            if stack.item.ends_with("_bucket") {
                1
            } else {
                64
            }
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "a slot's value, as the oven holds it"
    )]
    fn stack(item: &str, count: u32) -> Option<ItemStack> {
        Some(ItemStack::new(item, count))
    }

    fn count(slots: &[Option<ItemStack>], slot: usize) -> u32 {
        slots[slot].as_ref().map_or(0, |s| s.count)
    }

    fn run(oven: &mut Oven, slots: &mut [Option<ItemStack>], ticks: u32) {
        for _ in 0..ticks {
            let _ = oven.tick(slots, COOK_TIME, &Shipped);
        }
    }

    #[test]
    fn a_bud_takes_500_ticks_and_gives_four_and_is_spent_only_then() {
        let mut oven = Oven::default();
        let mut slots = [
            stack("minecraft:coal", 1),
            stack("hempdustry:indica_buds", 2),
            None,
            None,
            None,
        ];
        let first = oven.tick(&mut slots, COOK_TIME, &Shipped);
        assert_eq!(first.lit, Some(true));
        assert_eq!(count(&slots, FUEL_SLOT), 0, "the fuel is charged up front");
        run(&mut oven, &mut slots, 498);
        assert_eq!(
            count(&slots, FIRST_TRAY_SLOT),
            2,
            "the bud stays in the tray while it cooks"
        );
        assert_eq!(count(&slots, OUTPUT_SLOT), 0);
        run(&mut oven, &mut slots, 1);
        assert_eq!(count(&slots, FIRST_TRAY_SLOT), 1);
        assert_eq!(count(&slots, OUTPUT_SLOT), 4);
    }

    #[test]
    fn three_trays_cook_on_one_fire_each_on_its_own_timer() {
        let mut oven = Oven::default();
        let mut slots = [
            stack("minecraft:coal", 1),
            stack("hempdustry:indica_buds", 1),
            None,
            stack("hempdustry:hemp_leaf", 1),
            None,
        ];
        run(&mut oven, &mut slots, 250);
        slots[FIRST_TRAY_SLOT + 1] = stack("hempdustry:sativa_buds", 1);
        run(&mut oven, &mut slots, 250);
        // The first and third trays are done (4 + 1); the second is halfway.
        assert_eq!(count(&slots, OUTPUT_SLOT), 5);
        assert_eq!(oven.progress, [0, 250, 0]);
        // One coal (1600 ticks) burns once for all three, down from the tick after it lit.
        assert_eq!(oven.burn_time, 1600 - 499);
    }

    #[test]
    fn without_fuel_nothing_lights_and_progress_cools_two_a_tick() {
        let mut oven = Oven {
            progress: [10, 0, 1],
            ..Oven::default()
        };
        let mut slots = [None, stack("hempdustry:indica_buds", 1), None, None, None];
        let ticked = oven.tick(&mut slots, COOK_TIME, &Shipped);
        assert_eq!(ticked.lit, None);
        assert_eq!(oven.progress, [8, 0, 0]);
        // An idle oven with fuel keeps its fuel: nothing to cook, nothing burns.
        let mut idle = Oven::default();
        let mut slots = [stack("minecraft:coal", 3), None, None, None, None];
        let ticked = idle.tick(&mut slots, COOK_TIME, &Shipped);
        assert!(!ticked.changed);
        assert_eq!(count(&slots, FUEL_SLOT), 3);
    }

    #[test]
    fn a_full_or_different_collection_slot_stalls_the_tray() {
        let mut oven = Oven::default();
        let mut slots = [
            stack("minecraft:coal", 1),
            stack("hempdustry:indica_buds", 1),
            None,
            None,
            stack("hempdustry:decarboxylated_hemp", 61),
        ];
        let ticked = oven.tick(&mut slots, COOK_TIME, &Shipped);
        assert_eq!(ticked.lit, None, "4 more would not fit in 64: no fire");
        assert_eq!(count(&slots, FUEL_SLOT), 1);
        slots[OUTPUT_SLOT] = stack("minecraft:dirt", 1);
        let _ = oven.tick(&mut slots, COOK_TIME, &Shipped);
        assert_eq!(count(&slots, FUEL_SLOT), 1);
    }

    #[test]
    fn the_fire_goes_out_and_says_so_and_a_lava_bucket_leaves_its_bucket() {
        let mut oven = Oven {
            burn_time: 1,
            fuel_time: 1600,
            ..Oven::default()
        };
        let mut slots = [None, stack("hempdustry:hemp_leaf", 1), None, None, None];
        let ticked = oven.tick(&mut slots, COOK_TIME, &Shipped);
        assert_eq!(ticked.lit, Some(false));
        slots[FUEL_SLOT] = stack("minecraft:lava_bucket", 1);
        let ticked = oven.tick(&mut slots, COOK_TIME, &Shipped);
        assert_eq!(ticked.lit, Some(true));
        assert_eq!(
            slots[FUEL_SLOT].as_ref().map(|s| &*s.item),
            Some("minecraft:bucket")
        );
        assert_eq!(oven.fuel_time, 20000);
    }

    #[test]
    fn slots_take_only_what_belongs_and_a_shift_click_routes_by_kind() {
        let bud = ItemStack::new("hempdustry:indica_buds", 1);
        let coal = ItemStack::new("minecraft:coal", 1);
        let dirt = ItemStack::new("minecraft:dirt", 1);
        assert!(accepts(FIRST_TRAY_SLOT + 2, &bud, &Shipped));
        assert!(!accepts(FUEL_SLOT, &bud, &Shipped));
        assert!(accepts(FUEL_SLOT, &coal, &Shipped));
        assert!(!accepts(OUTPUT_SLOT, &bud, &Shipped));
        assert_eq!(quick_move_slots(&bud, &Shipped), Some(1..4));
        assert_eq!(quick_move_slots(&coal, &Shipped), Some(0..1));
        assert_eq!(quick_move_slots(&dirt, &Shipped), None);
    }

    #[test]
    fn a_fire_with_nothing_to_cook_still_burns_down_and_goes_out() {
        let mut oven = Oven {
            burn_time: 2,
            fuel_time: 1600,
            ..Oven::default()
        };
        let mut slots = [stack("minecraft:coal", 3), None, None, None, None];
        let first = oven.tick(&mut slots, COOK_TIME, &Shipped);
        assert!(first.changed, "the countdown must be saved");
        assert_eq!((oven.burn_time, first.lit), (1, None));
        let second = oven.tick(&mut slots, COOK_TIME, &Shipped);
        assert_eq!(second.lit, Some(false));
        assert_eq!(count(&slots, FUEL_SLOT), 3, "no work: no new fuel");
    }

    #[test]
    fn the_data_slots_are_the_java_order() {
        let oven = Oven {
            burn_time: 7,
            fuel_time: 1600,
            progress: [1, 2, 3],
        };
        assert_eq!(oven.data(COOK_TIME), [7, 1600, 1, 2, 3, 500]);
    }
}
