//! Edibles, as the Java mod's `EdibleEffects` and `EdibleScheduler` play them: the toast, the
//! cookie, the brownie, the dawamesk, the bhang and a slice of space cake.
//!
//! An edible carries two axes from the butter it was made with: **potency** (tier I to IV) is how
//! hard, **quality** how predictable and how long. Eating one queues the bundle of
//! [`bundle::Bundle`]: nothing lands at once; the first effect comes after a come-up rolled from
//! the quality's window, and the rest follow it up a ramp. What is still to land waits on the
//! player ([`Waiting`]).
//!
//! Like [`crate::smoke`], nothing here knows Horizon: the data comes in as JSON, the come-up's
//! roll as a uniform draw, and the doses go out as plain [`Effect`]s.

pub mod bundle;

#[cfg(test)]
mod tests;

pub use bundle::Bundle;

use crate::machine::infuser::Quality;
use crate::smoke::{Category, Effect, Policy};

/// The highest potency tier.
pub const MAX_TIER: i32 = 4;

/// Cannabutter's strength cap, the Infuser's batch cap, so the quartiles line up.
const STRENGTH_CAP: i32 = 24;

/// `EdibleEffects.tierFromStrength`: a butter's strength (1 to 24) as a tier (1 to 4), by even
/// quartiles.
#[must_use]
pub fn tier_from_strength(strength: i32) -> i32 {
    let clamped = strength.clamp(1, STRENGTH_CAP);
    ((clamped - 1) * MAX_TIER / STRENGTH_CAP + 1).clamp(1, MAX_TIER)
}

/// `EdibleEffects.applyOffset`: a tier with an edible's own step, kept inside I to IV.
#[must_use]
pub fn apply_offset(tier: i32, offset: i32) -> i32 {
    (tier + offset).clamp(1, MAX_TIER)
}

/// `InfusedEdible`'s offsets: the edible's own step on the potency ladder (+1 concentrates, -1
/// spreads the butter thin). None for an item no butter goes into.
#[must_use]
pub fn potency_offset(item: &str) -> Option<i32> {
    match item {
        "hempdustry:cannabutter_toast" | "hempdustry:space_cookie" => Some(-1),
        "hempdustry:space_brownie" | "hempdustry:space_cake" => Some(0),
        "hempdustry:dawamesk" => Some(1),
        _ => None,
    }
}

/// The items eaten from the hand that dose their eater (`EdibleItem`, `BhangItem`).
#[must_use]
pub fn is_edible(item: &str) -> bool {
    item == "hempdustry:bhang_bucket" || (potency_offset(item).is_some() && item != SPACE_CAKE)
}

/// The cake's block and its item.
pub const SPACE_CAKE: &str = "hempdustry:space_cake";

/// One effect of the bundle and how many ticks after eating it lands (`EdibleEffects.Dose`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dose {
    pub delay: i32,
    pub effect: Effect,
}

/// The rules an edible plays by in one world.
pub struct Rules<'a> {
    pub bundle: &'a Bundle,
    pub policy: &'a Policy,
    /// Vanilla's category of an effect.
    pub category: &'a dyn Fn(&str) -> Category,
}

impl Rules<'_> {
    /// `EdibleEffects.durationTicks`: how long a tier-IV high of `quality` lasts, scaled by the
    /// config.
    #[must_use]
    pub fn duration_ticks(&self, quality: Quality) -> i32 {
        self.policy
            .duration(self.bundle.grade(quality).duration_ticks)
    }

    /// `EdibleEffects.rollOnsetTicks`: the come-up for `quality` from `u`, a uniform draw in
    /// `0..1`, scaled by the config, window and all.
    #[must_use]
    pub fn roll_onset(&self, quality: Quality, u: f64) -> i32 {
        let window = self.bundle.grade(quality).onset;
        self.policy.onset(window.roll(u), window.min, window.max)
    }

    /// `EdibleEffects.bundle`: the whole staggered sequence with `onset` as the moment it kicks
    /// in, before the config's switches. Each effect lands its `arrives_ticks` after the onset
    /// and ends with the high, unless it has a length of its own per tier (not scaled by the
    /// config, as the Java mod's Regeneration never was).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's `(int)` of a positive double, which truncates as `as` does"
    )]
    #[must_use]
    pub fn bundle(&self, tier: i32, quality: Quality, onset: i32) -> Vec<Dose> {
        let index = usize::try_from(tier.clamp(1, MAX_TIER) - 1).unwrap_or(0);
        let share = f64::from(self.bundle.tier_share[index]);
        let duration = (f64::from(self.duration_ticks(quality)) * share) as i32;
        self.bundle
            .effects
            .iter()
            .filter_map(|effect| {
                let length = effect
                    .duration_by_tier
                    .map_or(duration - effect.arrives_ticks, |ticks| ticks[index]);
                (length > 0).then(|| Dose {
                    delay: onset + effect.arrives_ticks,
                    effect: Effect::new(&effect.effect, length, effect.amplifier(index, quality)),
                })
            })
            .collect()
    }

    /// `EdibleEffects.consume`: what eating an edible of `tier` and `quality` queues, through the
    /// config's switches (`filterKeepingDuration`: the durations are already scaled). An undosed
    /// edible (tier 0) queues nothing.
    #[must_use]
    pub fn doses(&self, tier: i32, quality: Quality, u: f64) -> Vec<Dose> {
        if tier <= 0 {
            return Vec::new();
        }
        let onset = self.roll_onset(quality, u);
        self.bundle(tier, quality, onset)
            .into_iter()
            .filter_map(|dose| {
                let mut allowed = self
                    .policy
                    .filter_keeping_duration(vec![dose.effect], self.category);
                allowed.pop().map(|effect| Dose {
                    delay: dose.delay,
                    effect,
                })
            })
            .collect()
    }
}

/// One effect on its way down, and the ticks it has left (`EdibleScheduler.Pending`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub ticks_left: i32,
    pub effect: Effect,
}

/// What one player has eaten and is still to feel, oldest first (`hempdustry:pending_edibles`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Waiting(pub Vec<Pending>);

impl Waiting {
    /// `EdibleScheduler.schedule`: `dose` lands in its delay, at least a tick from now.
    pub fn schedule(&mut self, dose: Dose) {
        self.0.push(Pending {
            ticks_left: dose.delay.max(1),
            effect: dose.effect,
        });
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// One server tick (`EdibleScheduler.tick`): every dose a tick closer, and the effects whose
    /// time has come, in the order they were eaten.
    pub fn tick(&mut self) -> Vec<Effect> {
        let mut landed = Vec::new();
        self.0.retain_mut(|pending| {
            if pending.ticks_left > 1 {
                pending.ticks_left -= 1;
                true
            } else {
                landed.push(pending.effect.clone());
                false
            }
        });
        landed
    }
}

/// `FoodConstants`: the food bar's top.
pub const MAX_FOOD: i32 = 20;

/// A bite of cake (`CakeBlock.eat`): its nutrition and saturation modifier.
const CAKE_NUTRITION: i32 = 2;
const CAKE_SATURATION_MODIFIER: f32 = 0.1;

/// `CakeBlock.MAX_BITES`: the bite that takes the last slice removes the cake.
pub const MAX_BITES: i32 = 6;

/// `Player.canEat(false)`: an invulnerable (creative) player always may, others while hungry.
#[must_use]
pub const fn can_eat(invulnerable: bool, food: i32) -> bool {
    invulnerable || food < MAX_FOOD
}

/// `FoodData.eat(2, 0.1F)` for a bite of cake: the food and saturation it leaves, from `food`
/// and `saturation`. Saturation never passes the food level.
#[must_use]
pub fn after_bite(food: i32, saturation: f32) -> (i32, f32) {
    // `FoodConstants.saturationByModifier`: nutrition × modifier × 2.
    #[expect(clippy::cast_precision_loss, reason = "Java's `(float)` of a two")]
    let added = CAKE_NUTRITION as f32 * CAKE_SATURATION_MODIFIER * 2.0;
    let food = (food + CAKE_NUTRITION).clamp(0, MAX_FOOD);
    #[expect(
        clippy::cast_precision_loss,
        reason = "Java's `(float)` of the food level, at most 20"
    )]
    let saturation = (saturation + added).clamp(0.0, food as f32);
    (food, saturation)
}
