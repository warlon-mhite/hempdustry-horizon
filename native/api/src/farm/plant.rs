//! The hemp crops' own rules: how a plant grows, takes bone meal, stands, and finds its root.
//!
//! The canonical age lives on the lower segment and only it random-ticks and takes bone meal; it
//! drives the whole plant. Purple Kush and Beldía stand two blocks tall from age 4; Lemon Haze
//! two from age 4 and three from age 6. Reaching the next height needs the space free: a plant
//! that is boxed in stops one stage short of needing that space and waits, and its shape is
//! reconciled against its age on every random tick, so it fills in once the space clears.

use super::growth;
use super::medium::Medium;
use super::mildew;
use super::model::{Block, Kind, Plant, Pos, Segment, above};
use super::stress;
use super::world::World;

/// One in this many bone meal applications does nothing (but the meal is used up).
pub const BONEMEAL_FAILURE_CHANCE: i32 = 3;

/// Whether the plant may grow into `pos`: empty space, or a segment above the root it already owns.
fn can_occupy(world: &dyn World, pos: Pos, kind: Kind) -> bool {
    match world.block(pos) {
        Block::Air => true,
        Block::Plant(plant) => plant.kind == kind && !plant.is_lower(),
        _ => false,
    }
}

/// The highest age the plant whose lower segment is at `pos` may grow to, given what is above it.
/// Holding the age back keeps the plant honest: the age is what the loot table and the models
/// read, so an age-7 lone stalk would harvest like a finished plant.
#[must_use]
pub fn max_age_for(world: &dyn World, pos: Pos, kind: Kind) -> u8 {
    if !can_occupy(world, above(pos, 1), kind) {
        return Plant::TWO_TALL_AGE - 1;
    }
    if kind.height() == 3 && !can_occupy(world, above(pos, 2), kind) {
        return Plant::THREE_TALL_AGE - 1;
    }
    Plant::MAX_AGE
}

/// Where the lower segment of the plant `plant` at `pos` belongs to sits. A Lemon Haze upper is
/// one block up while the plant is two tall and two once it is three tall.
#[must_use]
pub fn find_lower(world: &dyn World, pos: Pos, plant: Plant) -> Pos {
    match plant.segment {
        Segment::Lower => pos,
        Segment::Middle => above(pos, -1),
        Segment::Upper => {
            let below = world.block(above(pos, -1));
            match below.plant() {
                Some(p) if p.kind == plant.kind && p.segment == Segment::Middle => above(pos, -2),
                _ => above(pos, -1),
            }
        }
    }
}

/// The lower segment of the plant `pos` is part of, when it is one.
#[must_use]
pub fn root(world: &dyn World, pos: Pos) -> Option<(Pos, Plant)> {
    let plant = *world.block(pos).plant()?;
    let lower_pos = find_lower(world, pos, plant);
    let lower = *world.block(lower_pos).plant()?;
    (lower.kind == plant.kind && lower.is_lower()).then_some((lower_pos, lower))
}

/// Whether the plant at `pos` can stand on `ground`, as its template's `mayPlaceOn` asks:
/// a lower segment on farmland or a bed of ours (Beldía on its soil instead of farmland), a
/// segment above on the right part of its own plant.
#[must_use]
pub fn may_stand_on(plant: Plant, ground: Block, ground_is_beldia_soil: bool) -> bool {
    match plant.segment {
        Segment::Lower => {
            ground.is_medium()
                || match plant.kind {
                    Kind::Indica | Kind::Sativa => matches!(ground, Block::Farmland { .. }),
                    Kind::Beldia => ground_is_beldia_soil,
                }
        }
        Segment::Middle => ground
            .plant()
            .is_some_and(|p| p.kind == plant.kind && p.is_lower()),
        // Two tall it stands on the lower segment, three tall on the middle one.
        Segment::Upper => ground
            .plant()
            .is_some_and(|p| p.kind == plant.kind && p.segment != Segment::Upper),
    }
}

/// Bone meal is refused, and so not used up, once the plant has grown as far as its headroom
/// allows; without that, bone meal would push a boxed-in plant to maturity at a stunted height.
/// A dry Beldía takes none either: bone meal is growth too.
#[must_use]
pub fn takes_bone_meal(world: &dyn World, pos: Pos, plant: Plant) -> bool {
    plant.is_lower()
        && plant.age < max_age_for(world, pos, plant.kind)
        && (plant.kind != Kind::Beldia || growth::beldia_watered(world, above(pos, -1)))
}

/// Whether one bone meal grows the plant this time (the meal is used up either way).
pub fn bone_meal_succeeds(world: &mut dyn World) -> bool {
    world.next_int(BONEMEAL_FAILURE_CHANCE) != 0
}

/// One bone meal step: a flat stage, and on a flowering plant in a fed bed a chance of
/// overfeeding it. A flat stage sets a floor of seven lucky bone meals from seed to harvest.
pub fn bone_meal(world: &mut dyn World, pos: Pos) {
    let Block::Plant(plant) = world.block(pos) else {
        return;
    };
    if !plant.is_lower() {
        return;
    }
    let extra = stress::of_bone_meal(world, pos, plant);
    set_age(world, pos, plant.age + 1, extra);
}

/// A random tick of the block at `pos`: a lower segment grows, reconciles its shape, and takes
/// its mildew tick. Every other block of a plant does nothing.
pub fn random_tick(world: &mut dyn World, pos: Pos) {
    let Block::Plant(plant) = world.block(pos) else {
        return;
    };
    if !plant.is_lower() {
        return;
    }
    let kind = plant.kind;
    let age = plant.age;
    let mut grown = age;
    if age < Plant::MAX_AGE && world.raw_brightness(pos) >= 9 {
        // A bed of ours answers for itself: its own moisture, and no crowding penalty.
        let (moisture, bed) = match Medium::of(world.block(above(pos, -1))) {
            Some(medium) => (medium.moisture(), medium.speed()),
            None => (growth::field_moisture(world, pos, kind), 1.0),
        };
        let bonus = bed * plant.condition.speed_at(age);
        // Purple Kush and Beldía skip the roll on dry ground; Lemon Haze rolls at zero odds.
        let roll = kind == Kind::Sativa || moisture > 0.0;
        if roll && growth::rolls(world, kind.growth_resistance(), moisture, bonus) {
            grown += 1;
        }
    }
    // Reconcile whether or not the plant aged: this repairs a plant whose height fell behind its
    // age. Guarded on there being something to write, so a finished field costs a block read.
    if grown != age || !is_shape_settled(world, pos, plant) {
        set_age(world, pos, grown, 0.0);
    }
    mildew::random_tick(world, pos);
}

/// Whether the plant whose lower segment `lower` is at `pos` already carries the segments its
/// age calls for, so a reconciling [`set_age`] at the same age would write nothing above it.
fn is_shape_settled(world: &dyn World, pos: Pos, lower: Plant) -> bool {
    let age = lower.age;
    let kind = lower.kind;
    let mid = world.block(above(pos, 1));
    if kind.height() == 2 {
        if mid == Block::Plant(Plant::segment_of(lower, age, Segment::Upper)) {
            return true;
        }
        let upper_present = mid
            .plant()
            .is_some_and(|p| p.kind == kind && p.segment == Segment::Upper);
        return !(upper_present || (age >= Plant::TWO_TALL_AGE && mid == Block::Air));
    }
    if age < Plant::TWO_TALL_AGE {
        return true;
    }
    if !can_occupy(world, above(pos, 1), kind) {
        return true;
    }
    if age >= Plant::THREE_TALL_AGE && can_occupy(world, above(pos, 2), kind) {
        let top = world.block(above(pos, 2));
        return mid == Block::Plant(Plant::segment_of(lower, age, Segment::Middle))
            && top == Block::Plant(Plant::segment_of(lower, age, Segment::Upper));
    }
    mid == Block::Plant(Plant::segment_of(lower, age, Segment::Upper))
}

/// Sets the lower segment at `pos` to `new_age` and reconciles the segments above with the height
/// that age calls for, as far as there is room. `new_age` is held to [`max_age_for`], but never
/// below an age the plant already has: a finished plant later built over keeps its harvest.
/// `extra` is the stress odds this step brings beyond the ground's (a bone meal's overfeeding).
pub fn set_age(world: &mut dyn World, pos: Pos, new_age: u8, extra: f32) {
    let current = world.block(pos);
    let Some(&plant) = current.plant() else {
        return;
    };
    let kind = plant.kind;
    let current_age = if plant.is_lower() { plant.age } else { 0 };
    let new_age = new_age
        .min(Plant::MAX_AGE)
        .min(current_age.max(max_age_for(world, pos, kind)));

    // Rebuilt from the default state, so the trims and the records are carried across.
    let mut lower = plant.carry_onto(Plant::new(kind, new_age, Segment::Lower));
    if new_age > current_age {
        lower.condition = stress::after_step(
            world,
            pos,
            plant.condition,
            current_age,
            new_age,
            kind.height(),
            extra,
        );
        lower.mildew = mildew::after_step(world, pos, plant);
        if new_age == Plant::MAX_AGE {
            let floor_pos = above(pos, -1);
            if let Some(spent) = Medium::of(world.block(floor_pos)).and_then(Medium::spent) {
                world.set(floor_pos, spent.block());
            }
        }
    }
    world.set(pos, Block::Plant(lower));

    let mid_pos = above(pos, 1);
    if kind.height() == 2 {
        let mid = world.block(mid_pos);
        let upper_present = mid
            .plant()
            .is_some_and(|p| p.kind == kind && p.segment == Segment::Upper);
        if upper_present || (new_age >= Plant::TWO_TALL_AGE && mid == Block::Air) {
            world.set(
                mid_pos,
                Block::Plant(Plant::segment_of(lower, new_age, Segment::Upper)),
            );
        }
        return;
    }
    let top_pos = above(pos, 2);
    let mid_free = can_occupy(world, mid_pos, kind);
    if new_age >= Plant::THREE_TALL_AGE && mid_free && can_occupy(world, top_pos, kind) {
        // Bottom up: the new upper needs the middle under it in place, or it fails to stand.
        world.set(
            mid_pos,
            Block::Plant(Plant::segment_of(lower, new_age, Segment::Middle)),
        );
        world.set(
            top_pos,
            Block::Plant(Plant::segment_of(lower, new_age, Segment::Upper)),
        );
    } else if new_age >= Plant::TWO_TALL_AGE && mid_free {
        world.set(
            mid_pos,
            Block::Plant(Plant::segment_of(lower, new_age, Segment::Upper)),
        );
    }
}
