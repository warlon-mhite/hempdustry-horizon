//! Taking a plant: breaking any block of it takes the whole plant, which yields once.
//!
//! The loot table drops only for the lower segment, which carries the age, the trims and the
//! records. So when a player breaks a block above it, the lower segment is broken for them with
//! its loot; every other block of the plant goes without drops. Breaking the bed under a plant
//! harvests the plant first, as the player, so the bed never strands a floating stalk.

use super::model::{Plant, Pos, above};
use super::plant;
use super::world::World;

/// What breaking one block of a plant takes with it, worked out before anything is removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Harvest {
    /// Where the plant's lower segment is.
    pub lower: Pos,
    /// The lower segment as it stands: what the loot table and the harvest advancement read.
    pub plant: Plant,
    /// The blocks above the lower segment that go without drops, top down so none is left
    /// standing on air. The block the player broke is never one: the game removes that one.
    pub clear: Vec<Pos>,
    /// The lower segment must be broken here, with the player's loot, because the player broke
    /// another block (one above it, or the bed under it).
    pub break_lower: bool,
}

impl Harvest {
    /// A ripe plant: its harvest counts for the advancements.
    #[must_use]
    pub const fn is_ripe(&self) -> bool {
        self.plant.is_ripe()
    }
}

/// What a player breaking the block at `broken` takes. None when it isn't a plant, or is a
/// block above a lower segment that is gone (it then breaks alone, with no loot).
#[must_use]
pub fn of_break(world: &dyn World, broken: Pos) -> Option<Harvest> {
    let (lower, plant) = plant::root(world, broken)?;
    let clear = (1..plant.kind.height())
        .rev()
        .map(|offset| above(lower, offset))
        .filter(|&pos| pos != broken)
        .filter(|&pos| {
            world
                .block(pos)
                .plant()
                .is_some_and(|p| p.kind == plant.kind && !p.is_lower())
        })
        .collect();
    Some(Harvest {
        lower,
        plant,
        clear,
        break_lower: lower != broken,
    })
}

/// What a player breaking the bed at `bed` takes with it: the plant standing on it, whole and
/// with its loot, before the bed goes (`GrowMedium.harvestAbove`).
#[must_use]
pub fn of_bed_break(world: &dyn World, bed: Pos) -> Option<Harvest> {
    let top = above(bed, 1);
    let mut harvest = of_break(world, top)?;
    // The player broke the bed, not the plant: the block on the bed is ours to break too.
    if harvest.lower == top {
        harvest.break_lower = true;
    } else {
        harvest.clear.insert(0, top);
    }
    Some(harvest)
}
