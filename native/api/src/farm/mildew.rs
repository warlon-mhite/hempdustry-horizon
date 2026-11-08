//! Powdery mildew: the price of the stale air under a roof. It starts only on a plant under a
//! roof, spreads one plant at a time to the eight around it, is cut away by a trim, and vanilla's
//! sulfur gas clears and prevents it. It never kills a plant, and a ripe plant is past it.

use super::model::{Block, Mildew, Plant, Pos, above, offset};
use super::world::World;

/// The odds a growth step under a roof brings mildew.
pub const ONSET: f32 = 1.0 / 150.0;
/// The odds a mildewed plant's random tick infects the neighbour it picked.
pub const SPREAD: f32 = 1.0 / 2.0;

const NEIGHBOURS: [[i32; 2]; 8] = [
    [-1, -1],
    [0, -1],
    [1, -1],
    [-1, 0],
    [1, 0],
    [-1, 1],
    [0, 1],
    [1, 1],
];

/// What a plant's random tick does about mildew, after any growth: nothing outdoors; under a
/// roof the gas clears or sulfurs it, and a mildewed plant out of the gas reaches for a
/// neighbour. With the config switch off, a plant that still has it is cleaned instead.
pub fn random_tick(world: &mut dyn World, lower: Pos) {
    let Block::Plant(plant) = world.block(lower) else {
        return;
    };
    let now = plant.mildew;
    if !world.config().mildew {
        if now != Mildew::Clean {
            write(world, lower, Mildew::Clean);
        }
        return;
    }
    // Under a roof only gas or a mildewed plant changes anything, so a clean field skips the roof
    // check, a heightmap read and the dearest part of this tick. No random is drawn before.
    if now != Mildew::Mildewed && !world.gassed(lower) {
        return;
    }
    if !world.roofed(lower) {
        return;
    }
    if world.gassed(lower) {
        let next = match now {
            Mildew::Mildewed if plant.is_flowering() => Mildew::Sulfured,
            Mildew::Mildewed => Mildew::Clean,
            Mildew::Clean if plant.is_flowering() && !plant.is_ripe() => Mildew::Sulfured,
            other => other,
        };
        if next != now {
            write(world, lower, next);
        }
        return;
    }
    if now == Mildew::Mildewed {
        spread(world, lower);
    }
}

/// The record after a growth step of `plant` at `lower`: kept, or mildew caught at [`ONSET`].
/// Draws from the random only under a roof, so a field plant's step takes the draws it always did.
pub fn after_step(world: &mut dyn World, lower: Pos, plant: Plant) -> Mildew {
    if plant.mildew == Mildew::Mildewed || !can_catch(world, lower, plant) {
        return plant.mildew;
    }
    if world.next_float() < ONSET && !world.gassed(lower) {
        Mildew::Mildewed
    } else {
        plant.mildew
    }
}

fn spread(world: &mut dyn World, lower: Pos) {
    let pick = world.next_int(8);
    let Some([dx, dz]) = usize::try_from(pick)
        .ok()
        .and_then(|i| NEIGHBOURS.get(i).copied())
    else {
        return;
    };
    let target = offset(lower, dx, 0, dz);
    let Block::Plant(plant) = world.block(target) else {
        return;
    };
    // An upper segment stands on its own plant: the infection lands on that plant's lower one only.
    if plant.mildew == Mildew::Mildewed || world.block(above(target, -1)).is_crop(plant.kind) {
        return;
    }
    if world.next_float() >= SPREAD || !can_catch(world, target, plant) || world.gassed(target) {
        return;
    }
    write(world, target, Mildew::Mildewed);
}

/// Mildew may take hold of this plant at all: switched on, a crop that doesn't resist it, unripe,
/// under a roof.
fn can_catch(world: &dyn World, lower: Pos, plant: Plant) -> bool {
    world.config().mildew
        && !world.mildew_resistant(plant.kind)
        && !plant.is_ripe()
        && world.roofed(lower)
}

/// Writes `value` on the plant at `lower` and every segment above it at once.
pub fn write(world: &mut dyn World, lower: Pos, value: Mildew) {
    let Block::Plant(root) = world.block(lower) else {
        return;
    };
    let mut pos = lower;
    while let Block::Plant(mut segment) = world.block(pos) {
        if segment.kind != root.kind {
            break;
        }
        segment.mildew = value;
        world.set(pos, Block::Plant(segment));
        pos = above(pos, 1);
    }
}
