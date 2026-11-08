//! The beds a hemp plant can grow in besides farmland: the Grow Pot and the Hydro Tray
//! (`GrowMedium`). Each answers the growth roll's questions for itself, and a plant in one is
//! free of vanilla's crowding penalty: it has soil or solution of its own.

use super::model::{Block, Pot, Tray};

/// Watered: vanilla's best, a lone plant on moist farmland ringed by moist farmland.
pub const WET_MOISTURE: f32 = 10.0;
/// Dry: dry farmland ringed by dry farmland.
pub const DRY_MOISTURE: f32 = 4.0;
/// A watered pot with food in it.
pub const FERTILE_SPEED: f32 = 1.5;
/// A tray with water, nutrients and the pump running.
pub const PUMPED_SPEED: f32 = 2.0;

/// The odds one growth step in a dry bed stresses a flowering plant.
pub const DRY_BED: f32 = 1.0 / 3.0;
/// Still water and no air: the roots rot.
pub const STAGNANT: f32 = 1.0 / 6.0;

/// A bed of ours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Medium {
    Pot(Pot),
    Tray(Tray),
}

impl Medium {
    #[must_use]
    pub const fn of(block: Block) -> Option<Self> {
        match block {
            Block::Pot(pot) => Some(Self::Pot(pot)),
            Block::Tray(tray) => Some(Self::Tray(tray)),
            _ => None,
        }
    }

    #[must_use]
    pub const fn block(self) -> Block {
        match self {
            Self::Pot(pot) => Block::Pot(pot),
            Self::Tray(tray) => Block::Tray(tray),
        }
    }

    /// The moisture the plant above reads, in place of vanilla's farmland count.
    #[must_use]
    pub const fn moisture(self) -> f32 {
        let wet = match self {
            Self::Pot(pot) => pot.watered,
            Self::Tray(tray) => tray.level > 0,
        };
        if wet { WET_MOISTURE } else { DRY_MOISTURE }
    }

    /// The factor this bed multiplies the growth odds by. Food only works in water.
    #[must_use]
    pub const fn speed(self) -> f32 {
        match self {
            Self::Pot(pot) if pot.watered && pot.fertility > 0 => FERTILE_SPEED,
            Self::Tray(tray) if tray.level > 0 && tray.fed && tray.powered => PUMPED_SPEED,
            _ => 1.0,
        }
    }

    /// Food in the bed already: bone meal on a flowering plant here overfeeds it.
    #[must_use]
    pub const fn fed(self) -> bool {
        match self {
            Self::Pot(pot) => pot.fertility > 0,
            Self::Tray(tray) => tray.fed,
        }
    }

    /// The odds one growth step here stresses a flowering plant.
    #[must_use]
    pub const fn stress(self) -> f32 {
        match self {
            Self::Pot(pot) if pot.watered => 0.0,
            Self::Pot(_) => DRY_BED,
            Self::Tray(tray) if tray.level == 0 => DRY_BED,
            Self::Tray(tray) if tray.powered => 0.0,
            Self::Tray(_) => STAGNANT,
        }
    }

    /// The bed after a plant ripened in it: a pot loses a charge of food and its water; a tray a
    /// level of water, and its nutrients with the last of it. None when nothing changes.
    #[must_use]
    pub fn spent(self) -> Option<Self> {
        let spent = match self {
            Self::Pot(pot) => Self::Pot(Pot {
                fertility: pot.fertility.saturating_sub(1),
                watered: false,
            }),
            Self::Tray(tray) => {
                let left = tray.level.checked_sub(1)?;
                Self::Tray(Tray {
                    level: left,
                    fed: left > 0 && tray.fed,
                    ..tray
                })
            }
        };
        (spent != self).then_some(spent)
    }

    /// Whether bone meal (or any fertiliser) can go in: a pot until it is full, a tray when it
    /// has unfed water in it and the water doesn't boil away here.
    #[must_use]
    pub const fn takes_food(self, boils_away: bool) -> bool {
        match self {
            Self::Pot(pot) => pot.fertility < Pot::MAX_FERTILITY,
            Self::Tray(tray) => tray.level > 0 && !tray.fed && !boils_away,
        }
    }

    /// The bed with one charge of food in it (bone meal's `performBonemeal`).
    #[must_use]
    pub fn fed_once(self) -> Self {
        match self {
            Self::Pot(pot) => Self::Pot(Pot {
                fertility: (pot.fertility + 1).min(Pot::MAX_FERTILITY),
                ..pot
            }),
            Self::Tray(tray) => Self::Tray(Tray { fed: true, ..tray }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn pot(fertility: u8, watered: bool) -> Medium {
        Medium::Pot(Pot { fertility, watered })
    }

    const fn tray(level: u8, fed: bool, powered: bool) -> Medium {
        Medium::Tray(Tray {
            level,
            fed,
            powered,
        })
    }

    #[test]
    fn a_dry_pot_gives_nothing_however_well_fed() {
        assert_eq!(pot(3, false).speed(), 1.0);
        assert_eq!(pot(3, true).speed(), FERTILE_SPEED);
    }

    #[test]
    fn a_tray_needs_water_food_and_the_pump() {
        assert_eq!(tray(3, true, true).speed(), PUMPED_SPEED);
        assert_eq!(tray(3, true, false).speed(), 1.0);
        assert_eq!(tray(0, true, true).speed(), 1.0);
    }

    #[test]
    fn the_nutrients_go_with_the_last_of_the_water() {
        assert_eq!(tray(2, true, true).spent(), Some(tray(1, true, true)));
        assert_eq!(tray(1, true, true).spent(), Some(tray(0, false, true)));
        assert_eq!(tray(0, false, true).spent(), None);
    }

    #[test]
    fn a_ripening_plant_drinks_the_pot() {
        assert_eq!(pot(2, true).spent(), Some(pot(1, false)));
        assert_eq!(pot(0, false).spent(), None);
    }

    #[test]
    fn stale_water_rots_and_a_dry_bed_starves() {
        assert_eq!(tray(2, false, false).stress(), STAGNANT);
        assert_eq!(tray(0, false, true).stress(), DRY_BED);
        assert_eq!(tray(2, false, true).stress(), 0.0);
        assert_eq!(pot(0, false).stress(), DRY_BED);
    }
}
