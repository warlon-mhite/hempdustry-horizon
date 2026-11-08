//! Taking a hit (`Smoking.takeHit`, `SmokingDeviceItem.hit`, `SpliffItem.use`), as a plan the
//! game's adapter carries out: what to give now and on the exhale, the sounds, the green-out,
//! the cooldown, and what is left of the device.

use super::contents::SmokeContents;
use super::data::{Catalog, Category, Effect, java_round};
use super::device::Device;
use super::policy::Policy;

/// The random a hit draws from, in the Java mod's order.
pub trait Dice {
    /// `nextInt(bound)`: 0 inclusive to `bound` exclusive.
    fn next_int(&mut self, bound: i32) -> i32;
    /// `nextFloat`: 0 inclusive to 1 exclusive.
    fn next_float(&mut self) -> f32;

    /// A one-in-`odds` roll; never for odds of 0 or less.
    fn one_in(&mut self, odds: i32) -> bool {
        odds > 0 && self.next_int(odds) == 0
    }
}

/// `Smoking.EXHALE_DELAY_TICKS`: the smoke and the exhale's effects land ~2 s after the hit.
pub const EXHALE_DELAY_TICKS: i32 = 38;
/// `Smoking.NAUSEA_DURATION_TICKS`.
pub const NAUSEA_DURATION_TICKS: i32 = 140;

/// The player taking the hit, as the rules read them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Smoker {
    /// Wears the whole hemp outfit (beanie, shirt, harem pants, flip-flops), which halves the
    /// cough and nausea odds.
    pub full_outfit: bool,
}

/// What a green-out does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GreenOutHit {
    /// The full green-out (dose at `full_from_dose` or more): the mod's buffs go and saturation
    /// drops to zero; else only the spins.
    pub full: bool,
    /// The effects taken away first (`buffsTheModGrants`), on a full one.
    pub remove: Vec<String>,
    pub effects: Vec<Effect>,
}

/// One hit, to carry out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// When `hempdustry:smoking` plays: 0 at once.
    pub sound_delay: i32,
    /// Given at once.
    pub effects: Vec<Effect>,
    /// Given with the exhale, [`Hit::exhale_delay`] ticks on.
    pub exhaled: Vec<Effect>,
    /// When the exhale's smoke shows.
    pub exhale_delay: i32,
    pub green_out: Option<GreenOutHit>,
    /// `hempdustry:coughing` plays.
    pub cough: bool,
    pub nausea: Option<Effect>,
    /// The cooldown every smokeable takes; a full green-out locks out.
    pub cooldown_ticks: i32,
}

/// What it takes to know about effects and the data.
pub struct Rules<'a> {
    pub catalog: &'a Catalog,
    pub policy: &'a Policy,
    pub category: &'a dyn Fn(&str) -> Category,
}

impl Rules<'_> {
    /// `Smoking.takeHit` with the device's numbers and the cooldown it arms (`hit`'s
    /// `startCooldown`). None for a device the data doesn't know.
    pub fn take_hit(
        &self,
        device: Device,
        contents: &SmokeContents,
        smoker: Smoker,
        dice: &mut dyn Dice,
    ) -> Option<Hit> {
        let stats = self.catalog.device(device.key())?;
        let green_out = self.catalog.green_out();
        let sound_delay = device.sound_delay_at_hit();
        let primary = contents.primary_strain(self.catalog);

        let base = green_out_chance(green_out.one_in(contents.dose()), stats.green_out_factor);
        let smoothed = scaled(base, primary.map(|s| s.green_out_factor));
        let green_odds = self.policy.green_out_one_in(smoothed);
        let greened_out = dice.one_in(green_odds);

        let mut hit = Hit {
            sound_delay,
            effects: Vec::new(),
            exhaled: Vec::new(),
            exhale_delay: EXHALE_DELAY_TICKS + sound_delay,
            green_out: None,
            cough: false,
            nausea: None,
            cooldown_ticks: 0,
        };
        let category = self.category;
        let bonus = contents.buff_bonus(self.catalog, category);
        if greened_out {
            hit.green_out = Some(self.green_out(contents.dose()));
        } else {
            let mut float = || dice.next_float();
            let now = contents.effects(
                self.catalog,
                self.policy,
                stats.duration_ticks,
                false,
                category,
                &mut float,
            );
            hit.effects = self.policy.filter(now, bonus, category);
            let later = contents.effects(
                self.catalog,
                self.policy,
                stats.duration_ticks,
                true,
                category,
                &mut float,
            );
            hit.exhaled = self.policy.filter(later, bonus, category);
        }

        let cough_odds = eased(
            smoker,
            scaled(stats.cough_one_in, primary.map(|s| s.cough_factor)),
        );
        hit.cough = dice.one_in(cough_odds);
        let nausea_odds = eased(smoker, self.policy.nausea_one_in(stats.nausea_one_in));
        if !greened_out && dice.one_in(nausea_odds) {
            let nausea = Effect::new("minecraft:nausea", NAUSEA_DURATION_TICKS, 0);
            hit.nausea = self.policy.filter(vec![nausea], 0, category).pop();
        }

        let locked_out = greened_out && contents.dose() >= green_out.full_from_dose;
        hit.cooldown_ticks = self.policy.cooldown(if locked_out {
            green_out.lockout_ticks
        } else {
            stats.cooldown_ticks
        });
        Some(hit)
    }

    /// `Smoking.greenOut`.
    fn green_out(&self, dose: i32) -> GreenOutHit {
        let green_out = self.catalog.green_out();
        let full = dose >= green_out.full_from_dose;
        GreenOutHit {
            full,
            remove: if full {
                self.catalog
                    .buffs_the_mod_grants(self.category)
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            },
            effects: self.policy.filter(
                if full {
                    green_out.full
                } else {
                    green_out.spins
                },
                0,
                self.category,
            ),
        }
    }
}

/// `Smoking.greenOutChanceOneIn(greenOut, dose, deviceFactor)`.
fn green_out_chance(base: i32, device_factor: f32) -> i32 {
    if base <= 0 || device_factor <= 0.0 {
        return 0;
    }
    java_round(base as f32 * device_factor).max(1)
}

/// `smoothed` / `harshened`: odds of one in `odds` scaled by the primary strain's factor; a
/// factor of 0 rules it out, no strain leaves it be.
fn scaled(odds: i32, factor: Option<f32>) -> i32 {
    let Some(factor) = factor else {
        return odds;
    };
    if odds <= 0 {
        return odds;
    }
    if factor <= 0.0 {
        return 0;
    }
    java_round(odds as f32 * factor).max(1)
}

/// `Smoking.easedByOutfit`: the whole outfit doubles the odds against.
fn eased(smoker: Smoker, odds: i32) -> i32 {
    if odds <= 0 || !smoker.full_outfit {
        return odds;
    }
    i32::try_from(2 * i64::from(odds)).unwrap_or(i32::MAX)
}

/// A device's stack as a hit wears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Worn {
    /// `hempdustry:charges`: hits left in the bowl.
    pub charges: i32,
    /// `minecraft:damage`.
    pub damage: u32,
    /// The Unbreaking level on it.
    pub unbreaking: i32,
}

/// What a hit leaves of a device (`SmokingDeviceItem.hit`'s second half).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spent {
    pub damage: u32,
    /// Worn out: the stack is gone.
    pub broken: bool,
    /// The charges left; None when the bowl is spent and the device unpacked.
    pub charges: Option<i32>,
    /// Scorched hemp handed back (the vaporizer's spent bowl).
    pub scorched: i32,
}

impl Rules<'_> {
    /// One hit off a packed device: a charge, and a point of wear unless that point would
    /// break a device whose bowl still holds a hit. The last charge unpacks it, and a vaporizer
    /// gives back what it spent, when all of it grew as plant matter. A creative player spends
    /// nothing: None.
    pub fn spend(
        &self,
        device: Device,
        contents: &SmokeContents,
        worn: Worn,
        creative: bool,
        dice: &mut dyn Dice,
    ) -> Option<Spent> {
        if creative || device == Device::Spliff {
            return None;
        }
        let max = device.max_damage();
        let remaining = worn.charges - 1;
        let last_point = worn.damage + 1 >= max;
        let mut spent = Spent {
            damage: worn.damage,
            broken: false,
            charges: Some(remaining),
            scorched: 0,
        };
        if remaining <= 0 || !last_point {
            spent.damage += unbroken_points(1, worn.unbreaking, dice);
            spent.broken = spent.damage >= max;
        }
        if remaining <= 0 {
            spent.charges = None;
            let all_plant = contents.entries.iter().all(|e| {
                self.catalog
                    .strain(&e.strain)
                    .is_some_and(super::data::Strain::is_plant_matter)
            });
            let yield_ = self.catalog.bowl(device.key()).map_or(0, |b| b.spent_yield);
            if yield_ > 0 && all_plant {
                spent.scorched = yield_;
            }
        }
        Some(spent)
    }
}

/// `EnchantmentHelper.processDurabilityChange` with vanilla's Unbreaking on a tool: each point
/// is kept away with a chance of level / (level + 1) (`remove_binomial`).
fn unbroken_points(points: u32, unbreaking: i32, dice: &mut dyn Dice) -> u32 {
    if unbreaking <= 0 {
        return points;
    }
    #[allow(clippy::cast_precision_loss)]
    let chance = unbreaking as f32 / (unbreaking + 1) as f32;
    let removed = (0..points).filter(|_| dice.next_float() < chance).count();
    points - u32::try_from(removed).unwrap_or(points)
}
