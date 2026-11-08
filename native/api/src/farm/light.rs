//! The light record (`GrowLight`): the light a plant grew under, kept on the plant.
//!
//! The first growth step writes the tier overhead; every later step can only lower it to the
//! weakest light the plant has grown under, and losing the light turns a flowering plant
//! stressed and a younger one back to natural. So a lamp moved over at the end, or shuttled
//! between plants, buys nothing.

use super::model::{Condition, Plant, Pos, offset};
use super::world::World;

/// Grow Lamp speed while the plant is young: the long days growers give a plant before flowering.
pub const VEG_SPEED: f32 = 1.1;
/// How far above the plant's full-grown top a light still counts.
const REACH: i32 = 3;

impl Condition {
    /// The record after one growth step taken from `from_age`, with `now` overhead.
    #[must_use]
    pub fn after_step(self, from_age: u8, now: Self) -> Self {
        if from_age == 0 {
            return now;
        }
        if matches!(self, Self::Natural | Self::Stressed) {
            return self;
        }
        if now == Self::Natural {
            return if from_age + 1 >= Plant::FLOWERING_AGE {
                Self::Stressed
            } else {
                Self::Natural
            };
        }
        now.min(self)
    }

    /// The speed factor this record gives the growth roll at `age`.
    #[must_use]
    pub fn speed_at(self, age: u8) -> f32 {
        if self == Self::GrowLamp && age < Plant::FLOWERING_AGE {
            VEG_SPEED
        } else {
            1.0
        }
    }
}

/// The best light over the plant whose lower segment is at `lower` and which stands `height`
/// blocks tall grown. A Grow Lamp reaches the 3×3 plants below it; any other light only the plant
/// directly underneath.
#[must_use]
pub fn over(world: &dyn World, lower: Pos, height: i32) -> Condition {
    let mut best = Condition::Natural;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let found = scan(world, offset(lower, dx, height, dz));
            if found == Condition::GrowLamp {
                return Condition::GrowLamp;
            }
            if dx == 0 && dz == 0 {
                best = found;
            }
        }
    }
    best
}

/// Walks up from `from`: light passes glass and plants, and an opaque block stops it.
fn scan(world: &dyn World, from: Pos) -> Condition {
    for i in 0..REACH {
        let source = world.light_source(offset(from, 0, i, 0));
        if source.tier != Condition::Natural || source.opaque {
            return source.tier;
        }
    }
    Condition::Natural
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_step_takes_the_light_overhead() {
        assert_eq!(
            Condition::Natural.after_step(0, Condition::GrowLamp),
            Condition::GrowLamp
        );
    }

    #[test]
    fn a_later_step_only_lowers_the_record() {
        assert_eq!(
            Condition::Ambient.after_step(2, Condition::GrowLamp),
            Condition::Ambient
        );
        assert_eq!(
            Condition::GrowLamp.after_step(2, Condition::Lamp),
            Condition::Lamp
        );
        assert_eq!(
            Condition::Natural.after_step(2, Condition::GrowLamp),
            Condition::Natural
        );
    }

    #[test]
    fn losing_the_light_stresses_only_a_flowering_plant() {
        assert_eq!(
            Condition::Lamp.after_step(2, Condition::Natural),
            Condition::Natural
        );
        assert_eq!(
            Condition::Lamp.after_step(3, Condition::Natural),
            Condition::Stressed
        );
    }

    #[test]
    fn a_stressed_plant_stays_stressed() {
        assert_eq!(
            Condition::Stressed.after_step(5, Condition::GrowLamp),
            Condition::Stressed
        );
    }
}
