//! The machines' rules, as the Java mod plays them, with no Horizon host: the Decarboxylator
//! ([`decarb`]), the Infuser ([`infuser`]) and furnace fuel ([`fuel`]). `crate::machines` wires
//! them to Horizon's hooks.

use std::ops::Range;

use horizon_client::mod_api::ItemStack;

pub mod decarb;
pub mod fuel;
pub mod infuser;

/// `AbstractContainerMenu.moveItemStackTo` into `range`, front to back: first onto stacks of the
/// same item, then into empty slots, `max` to a slot. Takes what it moves out of `stack`; returns
/// whether any moved.
pub fn move_into(
    slots: &mut [Option<ItemStack>],
    range: Range<usize>,
    stack: &mut ItemStack,
    max: u32,
) -> bool {
    let start = stack.count;
    for slot in range.clone() {
        if stack.count == 0 {
            break;
        }
        if let Some(here) = slots[slot].as_mut()
            && here.same_item_same_components(stack)
        {
            let room = max.saturating_sub(here.count).min(stack.count);
            here.count += room;
            stack.count -= room;
        }
    }
    for slot in range {
        if stack.count == 0 {
            break;
        }
        if slots[slot].is_none() {
            let mut placed = stack.clone();
            placed.count = stack.count.min(max);
            stack.count -= placed.count;
            slots[slot] = Some(placed);
        }
    }
    stack.count < start
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_shift_click_tops_up_before_it_fills_an_empty_slot() {
        let mut slots = [None, None, stack("hempdustry:indica_buds", 60), None, None];
        let mut moving = ItemStack::new("hempdustry:indica_buds", 10);
        assert!(move_into(&mut slots, 1..4, &mut moving, 64));
        assert_eq!(
            [count(&slots, 1), count(&slots, 2), count(&slots, 3)],
            [6, 64, 0]
        );
        assert_eq!(moving.count, 0);
        let mut full = [
            None,
            stack("minecraft:dirt", 1),
            stack("minecraft:dirt", 1),
            stack("minecraft:dirt", 1),
            None,
        ];
        let mut moving = ItemStack::new("hempdustry:indica_buds", 10);
        assert!(!move_into(&mut full, 1..4, &mut moving, 64));
        assert_eq!(moving.count, 10);
    }
}
