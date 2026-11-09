//! `SeekHempCropGoal`: a creeper's ordinary wandering ends up in a hemp field now and then.
//!
//! A grown hemp plant is two or three blocks of green, and a creeper standing in one is almost
//! invisible. The goal is a wander and nothing else: the same speed, odds and "nobody is near,
//! so stand still" rule as the wandering it sits beside at priority 5, only *where* it wanders to
//! differs. A creeper glances at a few random columns ([`find_cover`]), so the odds of noticing a
//! field grow with its area. A column counts when its top is hemp at least two blocks tall under
//! open sky, with no other creeper by it: a field draws one creeper at a time.
//!
//! Once there it prowls ([`Prowl`]): a few short wanders from plant to plant, each followed by a
//! lurk of two to six seconds standing in the crop, then a minute to three of rest before the
//! same creeper looks at hemp again. Only about half of all creepers care at all, decided by the
//! UUID ([`cares_about_hemp`]) so it holds across saves. Nothing is saved: a chunk reload only
//! makes a creeper look at the fields around it once more.
//!
//! The plant test is `#hempdustry:hemp_crops`, so a strain joins the day it joins the tag.

use crate::farm::model::Pos;

/// `WaterAvoidingRandomStrollGoal`'s speed on a creeper. Faster towards a field would give it away.
pub const SPEED: f64 = 0.8;
/// One in this many ticks a creeper glances around for a field: half as often as it wanders.
pub const NOTICE_CHANCE: i32 = 240;
/// Half the side of the square a creeper looks across, in blocks.
pub const NOTICE_RANGE: i32 = 24;
/// Columns looked at per glance. A 9×9 field 15 blocks off is hit about one glance in four.
pub const NOTICE_SAMPLES: u32 = 8;
/// Odds of the next prowl step once a lurk is over: six times vanilla's, so the field usually wins.
pub const PROWL_CHANCE: i32 = 20;
pub const PROWL_RANGE: i32 = 5;
pub const PROWL_SAMPLES: u32 = 6;
pub const MIN_PROWLS: i32 = 3;
pub const MAX_PROWLS: i32 = 6;
/// Two to six seconds standing in the crop after each step.
pub const MIN_LURK: i32 = 40;
pub const MAX_LURK: i32 = 120;
/// One to three minutes before the same creeper looks at hemp again.
pub const MIN_REST: i32 = 1200;
pub const MAX_REST: i32 = 3600;
/// A creeper this close to a column is already in that field.
pub const TAKEN_RADIUS: f64 = 8.0;
/// A plant's roots this far above or below the creeper's feet are out of reach.
pub const MAX_Y_DIFFERENCE: i32 = 3;

/// What a creeper's glance reads of the level.
pub trait Field {
    /// The column's chunk is loaded. A wandering mob never loads one.
    fn loaded(&self, x: i32, z: i32) -> bool;
    /// The highest block of the column that isn't air (`WORLD_SURFACE` minus one).
    fn top(&self, x: i32, z: i32) -> i32;
    /// The block at `pos` is in `#hempdustry:hemp_crops`.
    fn is_hemp(&self, pos: Pos) -> bool;
    /// Another creeper stands within [`TAKEN_RADIUS`] of the plant whose root is `root`.
    fn taken(&self, root: Pos) -> bool;
}

/// The creeper's own random (`mob.getRandom()`).
pub trait Dice {
    /// `nextIntBetweenInclusive(min, max)`.
    fn between(&mut self, min: i32, max: i32) -> i32;
}

/// `caresAboutHemp`: whether this creeper is one of the half that notice hemp, from the low bit
/// of its UUID's least significant half.
#[must_use]
pub const fn cares_about_hemp(uuid: u128) -> bool {
    uuid & 1 == 0
}

/// `findCover`: looks at `samples` random columns within `range` of `at` (the creeper's block)
/// and gives the root of the first grown, open-sky hemp plant no other creeper is standing by.
pub fn find_cover(
    field: &dyn Field,
    dice: &mut dyn Dice,
    at: Pos,
    range: i32,
    samples: u32,
) -> Option<Pos> {
    let [cx, cy, cz] = at;
    for _ in 0..samples {
        let x = cx + dice.between(-range, range);
        let z = cz + dice.between(-range, range);
        if !field.loaded(x, z) {
            continue;
        }
        let mut y = field.top(x, z);
        let mut height = 0;
        while field.is_hemp([x, y, z]) {
            y -= 1;
            height += 1;
        }
        if height < 2 {
            continue;
        }
        let root = [x, y + 1, z];
        if (root[1] - cy).abs() > MAX_Y_DIFFERENCE || field.taken(root) {
            continue;
        }
        return Some(root);
    }
    None
}

/// One creeper's visit: how many prowl steps are left, when its rest ends, and the lurk it is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Prowl {
    prowls_left: i32,
    resting_until: i64,
    /// When the current lurk ends; 0 while still walking.
    lurking_until: i64,
}

impl Prowl {
    /// Resting: this creeper ignores hemp until the rest is over.
    #[must_use]
    pub const fn resting(&self, now: i64) -> bool {
        now < self.resting_until
    }

    /// In a field, between prowl steps.
    #[must_use]
    pub const fn prowling(&self) -> bool {
        self.prowls_left > 0
    }

    /// `setInterval`: the odds the stroll is tried with, one in this many ticks.
    #[must_use]
    pub const fn interval(&self) -> i32 {
        if self.prowling() {
            PROWL_CHANCE
        } else {
            NOTICE_CHANCE
        }
    }

    /// `getPosition`: the next spot, a plant near enough for the visit's stage. A prowl that
    /// finds no more field has walked out of it, or been led out: the visit is over.
    pub fn next_spot(
        &mut self,
        field: &dyn Field,
        dice: &mut dyn Dice,
        at: Pos,
        now: i64,
    ) -> Option<Pos> {
        let prowling = self.prowling();
        let (range, samples) = if prowling {
            (PROWL_RANGE, PROWL_SAMPLES)
        } else {
            (NOTICE_RANGE, NOTICE_SAMPLES)
        };
        let spot = find_cover(field, dice, at, range, samples);
        if spot.is_none() && prowling {
            self.rest(dice, now);
        }
        spot
    }

    /// `start`, after the walk is set off: a visit begins with three to six steps; the last
    /// step of one sends the creeper to rest.
    pub fn started(&mut self, dice: &mut dyn Dice, now: i64) {
        if self.prowls_left == 0 {
            self.prowls_left = dice.between(MIN_PROWLS, MAX_PROWLS);
        } else {
            self.prowls_left -= 1;
            if self.prowls_left == 0 {
                self.rest(dice, now);
            }
        }
    }

    /// `canContinueToUse` once the walk is done (`walking` false): the lurk where it ended,
    /// drawn on its first tick. True while it lasts.
    pub fn lurk(&mut self, walking: bool, dice: &mut dyn Dice, now: i64) -> bool {
        if walking {
            return true;
        }
        if self.lurking_until == 0 {
            self.lurking_until = now + i64::from(dice.between(MIN_LURK, MAX_LURK));
        }
        now < self.lurking_until
    }

    /// `stop`: the lurk is over.
    pub const fn stopped(&mut self) {
        self.lurking_until = 0;
    }

    fn rest(&mut self, dice: &mut dyn Dice, now: i64) {
        self.prowls_left = 0;
        self.resting_until = now + i64::from(dice.between(MIN_REST, MAX_REST));
    }
}

#[cfg(test)]
mod tests;
