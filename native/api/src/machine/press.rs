//! The Hemp Press (`HempPressBlockEntity`): a screw press heated from below, one slot in and one
//! out. Kief presses into hashish, bubble hash into filtered hashish, filtered hashish into rosin,
//! a retted stem into fibre (the `hempdustry:pressing` recipes). One squeeze is a furnace's 200
//! ticks; without heat or room the progress cools off two ticks a tick, as the oven's does.
//!
//! The heat is the block below, the Infuser's rule; a hopper under the press would sit in the
//! heat, so the press pushes its output out of its back, one item at a hopper's cadence, into
//! whatever container stands there.
//!
//! No Horizon host here: the recipes and stack sizes come from a [`Bench`], the container behind
//! the press from a [`Pour`].

use horizon_client::mod_api::ItemStack;

pub use super::infuser::{PUSH_COOLDOWN, PUSH_IDLE_COOLDOWN, Poured};

pub const INPUT_SLOT: usize = 0;
pub const OUTPUT_SLOT: usize = 1;
pub const SLOT_COUNT: usize = 2;
/// Ticks one squeeze takes: a furnace's exact 200, so nine pieces cost what nine iron cost.
pub const PRESS_TIME: i32 = 200;

/// The menu's data slots (`ContainerData`), in the Java mod's order.
pub const DATA_PROGRESS: usize = 0;
pub const DATA_PRESS_TIME: usize = 1;
pub const DATA_HEATED: usize = 2;
pub const DATA_COUNT: usize = 3;

/// What the press asks the game about items.
pub trait Bench {
    /// What one of `stack` presses into (a `hempdustry:pressing` recipe), None when the press
    /// won't take it.
    fn result_for(&self, stack: &ItemStack) -> Option<ItemStack>;
    /// How many of `stack` one slot holds.
    fn max_stack(&self, stack: &ItemStack) -> u32;
}

/// The container behind the press.
pub trait Pour {
    /// Puts `one` (a single item) in as a hopper would.
    fn pour(&mut self, one: &ItemStack) -> Poured;
}

/// The press's state: `Progress` is saved; the heat is read from below every tick, and the
/// push cooldown is a few ticks of timer not worth a save field (Java's own rules).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Press {
    pub progress: i32,
    pub heated: bool,
    pub push_cooldown: i32,
}

/// What a tick did, for the game to act on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Ticked {
    /// The progress or the slots changed: save it.
    pub changed: bool,
    /// A batch is being squeezed: heated, something to press and room for it. The block's
    /// `pressing`, which the renderer times the platen from; `lit` is [`Press::heated`].
    pub pressing: bool,
}

/// Whether `stack` goes in the input slot (`canPlaceItem`): only what presses into something.
#[must_use]
pub fn accepts(slot: usize, stack: &ItemStack, bench: &dyn Bench) -> bool {
    slot == INPUT_SLOT && bench.result_for(stack).is_some()
}

impl Press {
    /// The menu's data slots, the press time included so the arrow reads the press's own clock.
    #[must_use]
    pub fn data(&self, press_time: i32) -> [i32; DATA_COUNT] {
        [self.progress, press_time, i32::from(self.heated)]
    }

    /// `HempPressBlockEntity.tick`: read the heat, squeeze or cool off, push one item out.
    pub fn tick(
        &mut self,
        slots: &mut [Option<ItemStack>],
        heated: bool,
        press_time: i32,
        bench: &dyn Bench,
        behind: &mut dyn Pour,
    ) -> Ticked {
        // Not saved, so no change to save.
        self.heated = heated;
        let mut changed = false;
        let pressing = heated && can_press(slots, bench);
        if pressing {
            self.progress += 1;
            if self.progress >= press_time {
                self.progress = 0;
                press(slots, bench);
            }
            changed = true;
        } else if self.progress > 0 {
            // Cools off rather than freezing, as a furnace loses progress when its fire goes out.
            self.progress = (self.progress - 2).max(0);
            changed = true;
        }
        changed |= self.push_output(slots, behind);
        Ticked { changed, pressing }
    }

    /// One item of the output to the container behind (`pushOutput`): whether one went.
    fn push_output(&mut self, slots: &mut [Option<ItemStack>], behind: &mut dyn Pour) -> bool {
        let Some(output) = slots[OUTPUT_SLOT].as_mut() else {
            return false;
        };
        if self.push_cooldown > 0 {
            self.push_cooldown -= 1;
            return false;
        }
        self.push_cooldown = PUSH_COOLDOWN;
        let mut one = output.clone();
        one.count = 1;
        match behind.pour(&one) {
            Poured::Moved => {
                output.count -= 1;
                if output.count == 0 {
                    slots[OUTPUT_SLOT] = None;
                }
                true
            }
            // A real neighbour with no room: it may have some in 8 ticks.
            Poured::Full => false,
            Poured::Nothing => {
                self.push_cooldown = PUSH_IDLE_COOLDOWN;
                false
            }
        }
    }
}

/// Something to press and somewhere for it to go (`canPress`).
fn can_press(slots: &[Option<ItemStack>], bench: &dyn Bench) -> bool {
    let Some(result) = slots[INPUT_SLOT]
        .as_ref()
        .and_then(|input| bench.result_for(input))
    else {
        return false;
    };
    match &slots[OUTPUT_SLOT] {
        None => result.count <= bench.max_stack(&result),
        Some(output) => {
            output.same_item_same_components(&result)
                && output.count + result.count <= bench.max_stack(output)
        }
    }
}

/// One squeeze done: the result in, one input spent.
fn press(slots: &mut [Option<ItemStack>], bench: &dyn Bench) {
    let Some(result) = slots[INPUT_SLOT]
        .as_ref()
        .and_then(|input| bench.result_for(input))
    else {
        return;
    };
    match slots[OUTPUT_SLOT].as_mut() {
        None => slots[OUTPUT_SLOT] = Some(result),
        Some(output) => output.count += result.count,
    }
    if let Some(input) = slots[INPUT_SLOT].as_mut() {
        input.count -= 1;
        if input.count == 0 {
            slots[INPUT_SLOT] = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIEF: &str = "hempdustry:kief";
    const HASHISH: &str = "hempdustry:hashish";
    const STEM: &str = "hempdustry:retted_hemp_stem";
    const FIBER: &str = "hempdustry:hemp_fiber";

    struct Recipes;

    impl Bench for Recipes {
        fn result_for(&self, stack: &ItemStack) -> Option<ItemStack> {
            match &*stack.item {
                KIEF => Some(ItemStack::new(HASHISH, 1)),
                STEM => Some(ItemStack::new(FIBER, 8)),
                _ => None,
            }
        }

        fn max_stack(&self, _stack: &ItemStack) -> u32 {
            64
        }
    }

    /// Records what was poured; answers `answer`.
    struct Behind {
        answer: Poured,
        got: u32,
    }

    impl Pour for Behind {
        fn pour(&mut self, one: &ItemStack) -> Poured {
            assert_eq!(one.count, 1);
            if self.answer == Poured::Moved {
                self.got += 1;
            }
            self.answer
        }
    }

    fn nothing() -> Behind {
        Behind {
            answer: Poured::Nothing,
            got: 0,
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "a slot's value, as a machine holds it"
    )]
    fn stack(item: &str, count: u32) -> Option<ItemStack> {
        Some(ItemStack::new(item, count))
    }

    fn count(slots: &[Option<ItemStack>], slot: usize) -> u32 {
        slots[slot].as_ref().map_or(0, |s| s.count)
    }

    #[test]
    fn a_heated_press_squeezes_one_kief_into_hashish_every_two_hundred_ticks() {
        let mut press = Press::default();
        let mut slots = [stack(KIEF, 9), None];
        let mut behind = nothing();
        for tick in 1..PRESS_TIME {
            let ticked = press.tick(&mut slots, true, PRESS_TIME, &Recipes, &mut behind);
            assert!(ticked.pressing && ticked.changed, "tick {tick}");
        }
        assert_eq!(count(&slots, OUTPUT_SLOT), 0);
        let _ = press.tick(&mut slots, true, PRESS_TIME, &Recipes, &mut behind);
        assert_eq!(press.progress, 0);
        assert_eq!(count(&slots, INPUT_SLOT), 8);
        assert_eq!(slots[OUTPUT_SLOT], stack(HASHISH, 1));
    }

    #[test]
    fn a_cold_press_does_nothing_and_cools_off_two_ticks_a_tick() {
        let mut press = Press {
            progress: 5,
            ..Press::default()
        };
        let mut slots = [stack(KIEF, 1), None];
        let ticked = press.tick(&mut slots, false, PRESS_TIME, &Recipes, &mut nothing());
        assert!(!ticked.pressing && ticked.changed);
        assert_eq!(press.progress, 3);
        let _ = press.tick(&mut slots, false, PRESS_TIME, &Recipes, &mut nothing());
        let _ = press.tick(&mut slots, false, PRESS_TIME, &Recipes, &mut nothing());
        assert_eq!(press.progress, 0);
        let idle = press.tick(&mut slots, false, PRESS_TIME, &Recipes, &mut nothing());
        assert!(!idle.changed);
        assert!(!press.heated);
    }

    #[test]
    fn the_press_takes_only_what_presses() {
        assert!(accepts(INPUT_SLOT, &ItemStack::new(KIEF, 1), &Recipes));
        assert!(!accepts(
            INPUT_SLOT,
            &ItemStack::new("minecraft:dirt", 1),
            &Recipes
        ));
        assert!(!accepts(OUTPUT_SLOT, &ItemStack::new(KIEF, 1), &Recipes));
        let mut press = Press::default();
        let mut slots = [stack("minecraft:dirt", 1), None];
        let ticked = press.tick(&mut slots, true, PRESS_TIME, &Recipes, &mut nothing());
        assert!(!ticked.pressing);
    }

    #[test]
    fn a_full_or_different_output_stops_the_squeeze() {
        let mut press = Press::default();
        let mut slots = [stack(STEM, 4), stack(FIBER, 60)];
        let ticked = press.tick(&mut slots, true, PRESS_TIME, &Recipes, &mut nothing());
        assert!(!ticked.pressing, "8 more fibre don't fit on 60");
        let mut slots = [stack(KIEF, 4), stack(FIBER, 1)];
        let ticked = press.tick(&mut slots, true, PRESS_TIME, &Recipes, &mut nothing());
        assert!(!ticked.pressing, "hashish doesn't stack on fibre");
        let mut slots = [stack(STEM, 4), stack(FIBER, 56)];
        let ticked = press.tick(&mut slots, true, PRESS_TIME, &Recipes, &mut nothing());
        assert!(ticked.pressing);
    }

    #[test]
    fn the_output_goes_out_of_the_back_one_item_at_a_hoppers_cadence() {
        let mut press = Press::default();
        let mut slots = [None, stack(HASHISH, 3)];
        let mut behind = Behind {
            answer: Poured::Moved,
            got: 0,
        };
        let mut moved_at = Vec::new();
        for tick in 0..20 {
            let ticked = press.tick(&mut slots, false, PRESS_TIME, &Recipes, &mut behind);
            if ticked.changed {
                moved_at.push(tick);
            }
        }
        assert_eq!(moved_at, [0, 9, 18]);
        assert_eq!(behind.got, 3);
        assert_eq!(slots[OUTPUT_SLOT], None);
    }

    #[test]
    fn nothing_behind_waits_longer_and_a_full_container_keeps_the_cadence() {
        let mut press = Press::default();
        let mut slots = [None, stack(HASHISH, 1)];
        let _ = press.tick(&mut slots, false, PRESS_TIME, &Recipes, &mut nothing());
        assert_eq!(press.push_cooldown, PUSH_IDLE_COOLDOWN);
        let mut full = Behind {
            answer: Poured::Full,
            got: 0,
        };
        let mut press = Press::default();
        let _ = press.tick(&mut slots, false, PRESS_TIME, &Recipes, &mut full);
        assert_eq!(press.push_cooldown, PUSH_COOLDOWN);
        assert_eq!(count(&slots, OUTPUT_SLOT), 1);
    }

    #[test]
    fn the_menu_reads_progress_time_and_heat() {
        let press = Press {
            progress: 42,
            heated: true,
            push_cooldown: 3,
        };
        assert_eq!(press.data(PRESS_TIME), [42, PRESS_TIME, 1]);
    }
}
