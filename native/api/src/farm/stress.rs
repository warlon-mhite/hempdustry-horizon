//! Everything besides a lost light that can turn a flowering plant stressed (`PlantStress`).
//! Losing the light is certain; everything here is a roll. Beds carry the risk and fields
//! barely do. When two causes meet on one step the plant rolls once, at the worse odds, and
//! stress is kept for good.

use super::light;
use super::medium::Medium;
use super::model::{Block, Condition, Plant, Pos, above};
use super::world::World;

/// Bone meal on a flowering plant in a fed bed, per bone meal.
pub const OVERFED: f32 = 1.0 / 9.0;
/// A bottle poured into a pot that is still wet.
pub const OVERWATERED: f32 = 1.0 / 4.0;
/// A growth step on farmland at moisture 0.
pub const DRY_FIELD: f32 = 1.0 / 20.0;

/// The odds one growth step stresses a flowering plant standing on `floor`.
#[must_use]
pub fn of_ground(floor: Block) -> f32 {
    match (Medium::of(floor), floor) {
        (Some(medium), _) => medium.stress(),
        (None, Block::Farmland { moisture: 0 }) => DRY_FIELD,
        _ => 0.0,
    }
}

/// The record a plant carries after a growth step from `from_age` to `to_age`: the light's
/// verdict first, then one roll at the worst odds among the ground and `extra` (the overfeeding a
/// bone meal step brings).
pub fn after_step(
    world: &mut dyn World,
    lower: Pos,
    record: Condition,
    from_age: u8,
    to_age: u8,
    height: i32,
    extra: f32,
) -> Condition {
    let next = record.after_step(from_age, light::over(world, lower, height));
    if next == Condition::Stressed || to_age < Plant::FLOWERING_AGE {
        return next;
    }
    let chance = of_ground(world.block(above(lower, -1))).max(extra);
    if chance > 0.0 && world.next_float() < chance {
        Condition::Stressed
    } else {
        next
    }
}

/// The odds a bone meal step on `plant` (at `lower`) overfeeds it: flowering already, in a fed bed.
#[must_use]
pub fn of_bone_meal(world: &dyn World, lower: Pos, plant: &Plant) -> f32 {
    let fed = Medium::of(world.block(above(lower, -1))).is_some_and(Medium::fed);
    if plant.is_flowering() && fed {
        OVERFED
    } else {
        0.0
    }
}

/// Rolls `chance` against the plant whose lower segment is at `lower`, outside any growth step,
/// and on a hit writes the record up every segment at once. A ripe plant is past it.
pub fn roll(world: &mut dyn World, lower: Pos, chance: f32) {
    let Block::Plant(plant) = world.block(lower) else {
        return;
    };
    if !plant.is_flowering()
        || plant.condition == Condition::Stressed
        || plant.is_ripe()
        || world.next_float() >= chance
    {
        return;
    }
    let mut pos = lower;
    while let Block::Plant(mut segment) = world.block(pos) {
        if segment.kind != plant.kind {
            break;
        }
        segment.condition = Condition::Stressed;
        world.set(pos, Block::Plant(segment));
        pos = above(pos, 1);
    }
}
