//! The growth roll every hemp crop shares, and the ground's answers to it.

use super::model::{Block, Kind, Pos, offset};
use super::world::World;

/// The odds that one random tick advances a plant, before any cap: above 1.0 means every tick.
///
/// Vanilla grows a crop with odds of `1 / (floor(resistance / moisture) + 1)`. A speed multiplier
/// must not go inside that floor, where the cast eats it (1.2, 1.5 and 2.0 once grew Purple Kush
/// at the same speed), so it scales the odds instead. Dry ground (moisture 0) never grows.
#[must_use]
pub fn chance(resistance: f32, moisture: f32, multiplier: f64) -> f32 {
    if moisture <= 0.0 {
        return 0.0;
    }
    // `(int) (resistance / moisture)`, as Java truncates toward zero.
    #[allow(clippy::cast_possible_truncation)]
    let steps = (resistance / moisture) as i32;
    #[allow(clippy::cast_possible_truncation)]
    let odds = (multiplier / f64::from(steps + 1)) as f32;
    odds
}

/// Rolls one random tick's growth at the configured speed times `bonus` (a bed's and a lamp's
/// factors, which multiply the odds for the same reason the config does).
pub fn rolls(world: &mut dyn World, resistance: f32, moisture: f32, bonus: f32) -> bool {
    let multiplier = world.config().crop_growth_multiplier * f64::from(bonus);
    world.next_float() < chance(resistance, moisture, multiplier)
}

/// Vanilla's `CropBlock.getGrowthSpeed` for a `kind` plant at `pos` in the open ground: the
/// farmland under and around it (3 for wet, 1 for dry, a quarter of that for the eight around),
/// halved when the same crop crowds it in a row both ways or on a diagonal.
#[must_use]
pub fn field_speed(world: &dyn World, pos: Pos, kind: Kind) -> f32 {
    let mut speed = 1.0_f32;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let mut block_speed = match world.block(offset(pos, dx, -1, dz)) {
                Block::Farmland { moisture } if moisture > 0 => 3.0,
                Block::Farmland { .. } => 1.0,
                _ => 0.0,
            };
            if dx != 0 || dz != 0 {
                block_speed /= 4.0;
            }
            speed += block_speed;
        }
    }
    let same = |dx, dz| world.block(offset(pos, dx, 0, dz)).is_crop(kind);
    let horizontal = same(-1, 0) || same(1, 0);
    let vertical = same(0, -1) || same(0, 1);
    let diagonal = same(-1, -1) || same(1, -1) || same(1, 1) || same(-1, 1);
    if (horizontal && vertical) || diagonal {
        speed /= 2.0;
    }
    speed
}

/// Farmland's hydration reach, horizontally (`FarmlandBlock.isNearWater`).
pub const WATER_REACH: i32 = 4;

/// Whether the ground at `floor` counts as watered for Beldía: a bed of ours answers for itself,
/// sand needs water within farmland's own box, four blocks out, level with it or one above.
#[must_use]
pub fn beldia_watered(world: &dyn World, floor: Pos) -> bool {
    if world.block(floor).is_medium() {
        return true;
    }
    for dy in 0..=1 {
        for dx in -WATER_REACH..=WATER_REACH {
            for dz in -WATER_REACH..=WATER_REACH {
                if world.is_water(offset(floor, dx, dy, dz)) {
                    return true;
                }
            }
        }
    }
    false
}

/// A lone Beldía on sand with water in reach reads as a lone plant on wet farmland.
const BELDIA_WATERED_MOISTURE: f32 = 10.0;

/// The moisture the roll reads for a plant in the open ground (not in a bed of ours). Zero means
/// "doesn't grow this tick".
#[must_use]
pub fn field_moisture(world: &dyn World, pos: Pos, kind: Kind) -> f32 {
    match kind {
        Kind::Indica | Kind::Sativa => field_speed(world, pos, kind),
        Kind::Beldia => {
            if !beldia_watered(world, offset(pos, 0, -1, 0)) {
                return 0.0;
            }
            // On sand no farmland adds, so vanilla's speed is its base of 1, halved to 0.5 when
            // crowded: scaled up to wet farmland's 10 or 5, so Beldía wants rows as wheat does.
            if field_speed(world, pos, kind) < 1.0 {
                BELDIA_WATERED_MOISTURE / 2.0
            } else {
                BELDIA_WATERED_MOISTURE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_multiplier_scales_the_odds_not_the_floor() {
        // Purple Kush at full moisture: 25 / 10 floors to 2, odds 1 in 3.
        assert!((chance(25.0, 10.0, 1.0) - 1.0 / 3.0).abs() < 1e-6);
        assert!((chance(25.0, 10.0, 1.5) - 0.5).abs() < 1e-6);
        assert!((chance(25.0, 10.0, 2.0) - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn dry_ground_never_grows() {
        assert_eq!(chance(25.0, 0.0, 10.0), 0.0);
    }
}
