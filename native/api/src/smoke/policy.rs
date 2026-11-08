//! `EffectPolicy`: the effect switches of the Java mod's config, applied to everything a hit
//! gives. [`Policy::default`] is the config as it ships (`HempdustryConfig.Effects.DEFAULT`).

use super::data::{Category, Effect};

/// `HempdustryConfig.Effects`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the switches of `HempdustryConfig.Effects`, one for one"
)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Policy {
    pub enabled: bool,
    /// Levels, not amplifiers: 4 caps every effect at IV.
    pub max_level: i32,
    pub max_buff_level: i32,
    pub duration_multiplier: f64,
    pub cooldown_multiplier: f64,
    /// Scales every quality's come-up window (`onsetMultiplier`, 0.05 to 10).
    pub onset_multiplier: f64,
    pub green_out_chance_multiplier: f64,
    pub green_out: bool,
    pub nausea: bool,
    pub munchies: bool,
    pub debuffs: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_level: 4,
            max_buff_level: 2,
            duration_multiplier: 1.0,
            cooldown_multiplier: 1.0,
            onset_multiplier: 1.0,
            green_out_chance_multiplier: 1.0,
            green_out: true,
            nausea: true,
            munchies: true,
            debuffs: true,
        }
    }
}

impl Policy {
    /// `EffectPolicy.filter(effects, buffBonus)`: what the config lets through, durations scaled
    /// and levels capped.
    #[must_use]
    pub fn filter(
        &self,
        effects: Vec<Effect>,
        buff_bonus: i32,
        category: &dyn Fn(&str) -> Category,
    ) -> Vec<Effect> {
        if !self.enabled {
            return Vec::new();
        }
        effects
            .into_iter()
            .filter(|e| self.allowed(e, category(&e.id)))
            .map(|e| Effect {
                duration: self.duration(e.duration),
                amplifier: self.amplifier(category(&e.id), e.amplifier, buff_bonus),
                id: e.id,
            })
            .collect()
    }

    /// `EffectPolicy.filterKeepingDuration`: [`Self::filter`] with no buff bonus, for effects
    /// whose durations the caller has already scaled (an edible's).
    #[must_use]
    pub fn filter_keeping_duration(
        &self,
        effects: Vec<Effect>,
        category: &dyn Fn(&str) -> Category,
    ) -> Vec<Effect> {
        if !self.enabled {
            return Vec::new();
        }
        effects
            .into_iter()
            .filter(|e| self.allowed(e, category(&e.id)))
            .map(|e| Effect {
                amplifier: self.amplifier(category(&e.id), e.amplifier, 0),
                ..e
            })
            .collect()
    }

    fn allowed(&self, effect: &Effect, category: Category) -> bool {
        if !self.munchies && effect.id == "minecraft:hunger" {
            return false;
        }
        if !self.nausea && effect.id == "minecraft:nausea" {
            return false;
        }
        self.debuffs || category != Category::Harmful
    }

    /// `EffectPolicy.duration`.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's `(int) Math.round`: its ties go up where `round` goes away from zero, which differs only below zero, where both clamp"
    )]
    #[must_use]
    pub fn duration(&self, ticks: i32) -> i32 {
        (f64::from(ticks) * self.duration_multiplier)
            .round()
            .max(1.0) as i32
    }

    /// `EffectPolicy.amplifier`: at most `max_level`, a buff at most `max_buff_level` (one more
    /// with a concentrate's bonus).
    #[must_use]
    pub fn amplifier(&self, category: Category, amplifier: i32, buff_bonus: i32) -> i32 {
        let mut cap = self.max_level;
        if category == Category::Beneficial {
            cap = cap.min(self.max_buff_level + buff_bonus);
        }
        amplifier.min(cap - 1)
    }

    /// `EffectPolicy.maxBuffLevel`.
    #[must_use]
    pub fn max_buff_level(&self, buff_bonus: i32) -> i32 {
        self.max_level.min(self.max_buff_level + buff_bonus)
    }

    /// `EffectPolicy.cooldown`.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's `(int) Math.round`: its ties go up where `round` goes away from zero, which differs only below zero, where both clamp"
    )]
    #[must_use]
    pub fn cooldown(&self, ticks: i32) -> i32 {
        (f64::from(ticks) * self.cooldown_multiplier)
            .round()
            .max(0.0) as i32
    }

    /// `EffectPolicy.greenOutChanceOneIn`: 0 (never) when the config rules it out.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's `(int) Math.round`: its ties go up where `round` goes away from zero, which differs only below zero, where both clamp"
    )]
    #[must_use]
    pub fn green_out_one_in(&self, one_in: i32) -> i32 {
        if one_in <= 0
            || !self.enabled
            || !self.green_out
            || !self.debuffs
            || self.green_out_chance_multiplier <= 0.0
        {
            return 0;
        }
        (f64::from(one_in) / self.green_out_chance_multiplier)
            .round()
            .clamp(1.0, f64::from(i32::MAX)) as i32
    }

    /// `EffectPolicy.onset`: an edible's come-up of `ticks`, scaled, and kept inside its window
    /// `min..=max` scaled the same way (never under a tick).
    #[must_use]
    pub fn onset(&self, ticks: i32, min: i32, max: i32) -> i32 {
        let scale = |t: i32| java_round(f64::from(t) * self.onset_multiplier);
        let low = scale(min).max(1);
        let high = scale(max).max(1);
        scale(ticks).clamp(low, high.max(low))
    }

    /// `EffectPolicy.nauseaChanceOneIn`.
    #[must_use]
    pub fn nausea_one_in(&self, one_in: i32) -> i32 {
        if self.nausea { one_in } else { 0 }
    }
}

/// Java's `(int) Math.round(double)` for the ticks a multiplier gives: ties go up, and a value
/// past the int range saturates as the cast does.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Java's `(int)` of a long, which saturates as `as` does for values this mod makes"
)]
fn java_round(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}
