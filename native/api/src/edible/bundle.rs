//! `EdibleBundle`: what every edible does, read from `data/<ns>/hempdustry/edible/default.json`
//! as the Java mod's codec reads it, and [`Bundle::built_in`] when the file is missing or refused.

use std::collections::BTreeMap;

use serde_json::Value;

use super::MAX_TIER;
use crate::machine::infuser::Quality;
use crate::smoke::data::namespaced;

/// Four values, one per potency tier (tier I first).
type PerTier<T> = [T; MAX_TIER as usize];

/// When an edible of one quality kicks in (`EdibleBundle.Onset`): never before `min`, never
/// after `max`, most often at `mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Onset {
    pub min: i32,
    pub mode: i32,
    pub max: i32,
}

impl Onset {
    /// One roll of the triangle over the window, from `u`, a uniform draw in `0..1`
    /// (`random.nextDouble()`), in ticks.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's `Math.round` of a value already inside the int window"
    )]
    #[must_use]
    pub fn roll(self, u: f64) -> i32 {
        if self.max == self.min {
            return self.min;
        }
        let (min, mode, max) = (
            f64::from(self.min),
            f64::from(self.mode),
            f64::from(self.max),
        );
        let width = max - min;
        let rising = (mode - min) / width;
        let ticks = if u < rising {
            min + (u * width * (mode - min)).sqrt()
        } else {
            max - ((1.0 - u) * width * (max - mode)).sqrt()
        };
        // `Math.round` (ties up), then `Math.clamp` into the window.
        ((ticks + 0.5).floor() as i32).clamp(self.min, self.max)
    }

    /// `Onset.CODEC`: three positive ints, `min <= mode <= max`.
    fn parse(value: &Value) -> Option<Self> {
        let onset = Self {
            min: positive(&value["min"])?,
            mode: positive(&value["mode"])?,
            max: positive(&value["max"])?,
        };
        (onset.min <= onset.mode && onset.mode <= onset.max).then_some(onset)
    }
}

/// One quality: when it kicks in, and how long a tier-IV high lasts (`EdibleBundle.Grade`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grade {
    pub onset: Onset,
    pub duration_ticks: i32,
}

impl Grade {
    fn parse(value: &Value) -> Option<Self> {
        Some(Self {
            onset: Onset::parse(&value["onset_ticks"])?,
            duration_ticks: positive(&value["duration_ticks"])?,
        })
    }
}

/// One effect of the bundle (`EdibleBundle.Effect`): it lands `arrives_ticks` after the edible
/// kicks in and lasts to the end of the high, unless it has a length of its own per tier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleEffect {
    /// `minecraft:slowness`.
    pub effect: String,
    pub amplifier_by_tier: PerTier<i32>,
    pub arrives_ticks: i32,
    pub duration_by_tier: Option<PerTier<i32>>,
    /// Replaces [`Self::amplifier_by_tier`] for the qualities it names.
    pub amplifier_by_quality: BTreeMap<Quality, PerTier<i32>>,
}

impl BundleEffect {
    /// The amplifier at `tier_index` (tier I is 0) for an edible of `quality`.
    #[must_use]
    pub fn amplifier(&self, tier_index: usize, quality: Quality) -> i32 {
        self.amplifier_by_quality
            .get(&quality)
            .unwrap_or(&self.amplifier_by_tier)[tier_index]
    }

    fn new(effect: &str, amplifiers: PerTier<i32>, arrives_ticks: i32) -> Self {
        Self {
            effect: namespaced(effect),
            amplifier_by_tier: amplifiers,
            arrives_ticks,
            duration_by_tier: None,
            amplifier_by_quality: BTreeMap::new(),
        }
    }

    fn parse(value: &Value) -> Option<Self> {
        let amplifiers = |v: &Value| per_tier(v, |n| (0..=255).contains(&n));
        let by_quality = match &value["amplifier_by_quality"] {
            Value::Null => BTreeMap::new(),
            Value::Object(map) => map
                .iter()
                .map(|(name, list)| Some((Quality::from_name(name)?, amplifiers(list)?)))
                .collect::<Option<_>>()?,
            _ => return None,
        };
        let duration_by_tier = match &value["duration_by_tier"] {
            Value::Null => None,
            list => Some(per_tier(list, |n| n > 0)?),
        };
        Some(Self {
            effect: namespaced(value["effect"].as_str()?),
            amplifier_by_tier: amplifiers(&value["amplifier_by_tier"])?,
            arrives_ticks: int(&value["arrives_ticks"]).filter(|&n| n >= 0)?,
            duration_by_tier,
            amplifier_by_quality: by_quality,
        })
    }
}

/// `EdibleBundle`: the qualities, how much of the high each tier gets, and the effects.
#[derive(Clone, Debug, PartialEq)]
pub struct Bundle {
    /// By [`Quality`], lowest first.
    pub qualities: [Grade; 4],
    pub tier_share: PerTier<f32>,
    pub effects: Vec<BundleEffect>,
}

impl Bundle {
    #[must_use]
    pub fn grade(&self, quality: Quality) -> Grade {
        self.qualities[quality as usize]
    }

    /// `EdibleBundle.BUILT_IN`: 2.0.1's effects and durations, 2.1's onsets.
    #[must_use]
    pub fn built_in() -> Self {
        let grade = |min, mode, max, duration_ticks| Grade {
            onset: Onset { min, mode, max },
            duration_ticks,
        };
        let mut slowness = BundleEffect::new("slowness", [0, 0, 1, 1], 0);
        slowness
            .amplifier_by_quality
            .insert(Quality::Perfect, [0; 4]);
        let mut regeneration = BundleEffect::new("regeneration", [0; 4], 600);
        regeneration.duration_by_tier = Some([100, 200, 300, 400]);
        Self {
            qualities: [
                grade(900, 2400, 5200, 3600),
                grade(1100, 2000, 3900, 5400),
                grade(1300, 1800, 2700, 7200),
                grade(1740, 1800, 1860, 9600),
            ],
            tier_share: [0.625, 0.75, 0.875, 1.0],
            effects: vec![
                slowness,
                BundleEffect::new("absorption", [0, 1, 2, 3], 160),
                BundleEffect::new("resistance", [0; 4], 160),
                BundleEffect::new("hunger", [0; 4], 400),
                regeneration,
            ],
        }
    }

    /// `EdibleBundle.CODEC`; None for a file the codec refuses.
    #[must_use]
    pub fn parse(value: &Value) -> Option<Self> {
        let qualities = &value["qualities"];
        let grades = Quality::ALL.map(|q| Grade::parse(&qualities[q.name()]));
        let [Some(rough), Some(standard), Some(clean), Some(perfect)] = grades else {
            return None;
        };
        let tier_share = per_tier_float(&value["tier_share"])?;
        let effects = value["effects"]
            .as_array()?
            .iter()
            .map(BundleEffect::parse)
            .collect::<Option<_>>()?;
        Some(Self {
            qualities: [rough, standard, clean, perfect],
            tier_share,
            effects,
        })
    }
}

fn int(value: &Value) -> Option<i32> {
    value.as_i64().and_then(|n| i32::try_from(n).ok())
}

/// `ExtraCodecs.POSITIVE_INT`.
fn positive(value: &Value) -> Option<i32> {
    int(value).filter(|&n| n > 0)
}

/// A list of exactly four ints, each passing `valid`.
fn per_tier(value: &Value, valid: impl Fn(i32) -> bool) -> Option<PerTier<i32>> {
    let list = value.as_array()?;
    let ints: Vec<i32> = list.iter().map(int).collect::<Option<_>>()?;
    let ints: PerTier<i32> = ints.try_into().ok()?;
    ints.iter().all(|&n| valid(n)).then_some(ints)
}

/// `ExtraCodecs.POSITIVE_FLOAT.listOf(4, 4)`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the Java codec reads these as floats"
)]
fn per_tier_float(value: &Value) -> Option<PerTier<f32>> {
    let floats: Vec<f32> = value
        .as_array()?
        .iter()
        .map(|v| v.as_f64().map(|f| f as f32).filter(|&f| f > 0.0))
        .collect::<Option<_>>()?;
    floats.try_into().ok()
}
