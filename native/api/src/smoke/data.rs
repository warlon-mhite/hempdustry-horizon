//! The data the smoking rules read: strains (`data/<ns>/hempdustry/strain/`), how each device
//! smokes (`hempdustry/device/`), the green-out (`hempdustry/green_out/`) and the edibles' effects
//! (`hempdustry/edible/`), parsed from the same JSON the Java mod's codecs read. A device or the
//! green-out with no file of its own smokes by the numbers the Java mod ships ([`DeviceStats::built_in`],
//! [`GreenOut::built_in`]); a strain exists only as a file.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// `MobEffectCategory`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Beneficial,
    Harmful,
    Neutral,
}

/// One status effect to give: `MobEffectInstance` with its default flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    /// `minecraft:speed`.
    pub id: String,
    pub duration: i32,
    /// 0 for level I.
    pub amplifier: i32,
}

impl Effect {
    #[must_use]
    pub fn new(id: &str, duration: i32, amplifier: i32) -> Self {
        Self {
            id: namespaced(id),
            duration,
            amplifier,
        }
    }

    /// `MobEffectInstance.CODEC`: `id`, `duration` (default 0), `amplifier` (default 0).
    fn parse(value: &Value) -> Option<Self> {
        Some(Self::new(
            value["id"].as_str()?,
            int(&value["duration"]).unwrap_or(0),
            int(&value["amplifier"]).unwrap_or(0),
        ))
    }
}

/// `Strain.SmokeEffect`.
#[derive(Clone, Debug, PartialEq)]
pub struct SmokeEffect {
    pub effect: String,
    pub base_amplifier: i32,
    pub scales: bool,
    pub duration_factor: f32,
    pub on_exhale: bool,
    pub chance: f32,
    pub max_amplifier: i32,
}

impl SmokeEffect {
    /// `SmokeEffect.NO_CAP`.
    pub const NO_CAP: i32 = 255;

    fn parse(value: &Value) -> Option<Self> {
        Some(Self {
            effect: namespaced(value["effect"].as_str()?),
            base_amplifier: int(&value["base_amplifier"]).unwrap_or(0),
            scales: value["scales"].as_bool().unwrap_or(true),
            duration_factor: float(&value["duration_factor"]).unwrap_or(1.0),
            on_exhale: value["on_exhale"].as_bool().unwrap_or(false),
            chance: float(&value["chance"])
                .filter(|c| (0.0..=1.0).contains(c))
                .unwrap_or(1.0),
            max_amplifier: int(&value["max_amplifier"])
                .filter(|m| (0..=Self::NO_CAP).contains(m))
                .unwrap_or(Self::NO_CAP),
        })
    }
}

/// `Strain`: what the smoking rules read of one.
#[derive(Clone, Debug, PartialEq)]
pub struct Strain {
    /// `hempdustry:indica`.
    pub id: String,
    /// `translation_key`: the lang key of its name, `hempdustry.strain.indica`.
    pub translation_key: String,
    /// `color`: the packed tint, as RGB.
    pub color: i32,
    /// The item that packs as this strain (`buds`).
    pub buds: String,
    /// `flower`: present for a strain that grows on a plant, absent for hash and the like
    /// (`wildFlower().isPresent()`, the mod-wide "this grew on a plant").
    pub flower: Option<String>,
    pub green_out_factor: f32,
    pub cough_factor: f32,
    pub dose_per_item: i32,
    pub effects: Vec<SmokeEffect>,
}

impl Strain {
    /// `Strain.CODEC`: `translation_key`, `color` and `buds` are required; a file without one
    /// is no strain.
    #[must_use]
    pub fn parse(id: &str, value: &Value) -> Option<Self> {
        Some(Self {
            id: namespaced(id),
            translation_key: value["translation_key"].as_str()?.to_owned(),
            color: int(&value["color"])?,
            buds: namespaced(value["buds"].as_str()?),
            flower: value["flower"].as_str().map(namespaced),
            green_out_factor: float(&value["green_out_factor"]).unwrap_or(1.0),
            cough_factor: float(&value["cough_factor"]).unwrap_or(1.0),
            dose_per_item: int(&value["dose_per_item"]).unwrap_or(1),
            effects: value["effects"]
                .as_array()
                .map(|list| list.iter().filter_map(SmokeEffect::parse).collect())
                .unwrap_or_default(),
        })
    }

    /// Grew on a plant.
    #[must_use]
    pub fn is_plant(&self) -> bool {
        self.flower.is_some()
    }

    /// `ModStrains.isPlantMatter`: grew on a plant, or is schwag (spent like a bud in a vaporizer).
    #[must_use]
    pub fn is_plant_matter(&self) -> bool {
        self.is_plant() || self.id == "hempdustry:schwag"
    }

    /// `Strain.effects(dose, durationTicks, onExhale, buffBonus, random)`: one hit's effects of
    /// this strain at `dose`, those of the hit or of the exhale. An effect with a `chance` below
    /// one draws `next_float` once.
    pub fn effects(
        &self,
        dose: i32,
        duration_ticks: i32,
        on_exhale: bool,
        buff_bonus: i32,
        category: &dyn Fn(&str) -> Category,
        next_float: &mut dyn FnMut() -> f32,
    ) -> Vec<Effect> {
        let mut out = Vec::with_capacity(self.effects.len());
        for effect in &self.effects {
            if effect.on_exhale != on_exhale {
                continue;
            }
            if effect.chance < 1.0 && next_float() >= effect.chance {
                continue;
            }
            let mut amplifier =
                effect.base_amplifier + if effect.scales { (dose - 1).max(0) } else { 0 };
            if effect.scales && category(&effect.effect) == Category::Beneficial {
                amplifier += buff_bonus;
            }
            amplifier = amplifier.min(effect.max_amplifier);
            out.push(Effect {
                id: effect.effect.clone(),
                duration: java_round(java_float(duration_ticks) * effect.duration_factor).max(1),
                amplifier,
            });
        }
        out
    }
}

/// `DeviceStats.Bowl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bowl {
    pub hits: i32,
    pub max_dose: i32,
    pub spent_yield: i32,
}

/// `DeviceStats`: how one device smokes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceStats {
    pub duration_ticks: i32,
    pub cooldown_ticks: i32,
    pub cough_one_in: i32,
    pub nausea_one_in: i32,
    pub green_out_factor: f32,
    pub bowl: Option<Bowl>,
}

impl DeviceStats {
    /// `DeviceStats.CODEC`; every field but `bowl` is required.
    #[must_use]
    pub fn parse(value: &Value) -> Option<Self> {
        let bowl = &value["bowl"];
        Some(Self {
            duration_ticks: int(&value["duration_ticks"]).filter(|t| *t > 0)?,
            cooldown_ticks: int(&value["cooldown_ticks"]).filter(|t| *t >= 0)?,
            cough_one_in: int(&value["cough_one_in"]).filter(|t| *t >= 0)?,
            nausea_one_in: int(&value["nausea_one_in"]).filter(|t| *t >= 0)?,
            green_out_factor: float(&value["green_out_factor"])?,
            bowl: if bowl.is_null() {
                None
            } else {
                Some(Bowl {
                    hits: int(&bowl["hits"]).filter(|t| *t > 0)?,
                    max_dose: int(&bowl["max_dose"]).filter(|t| *t > 0)?,
                    spent_yield: int(&bowl["spent_yield"]).filter(|t| *t >= 0)?,
                })
            },
        })
    }

    /// What the Java mod ships for the device file `key` (`DeviceType`, `SPLIFF_BUILT_IN`).
    #[must_use]
    pub fn built_in(key: &str) -> Option<Self> {
        let device = |duration, cooldown, cough, nausea, hits, max_dose, spent_yield| Self {
            duration_ticks: duration,
            cooldown_ticks: cooldown,
            cough_one_in: cough,
            nausea_one_in: nausea,
            green_out_factor: 1.0,
            bowl: Some(Bowl {
                hits,
                max_dose,
                spent_yield,
            }),
        };
        Some(match key {
            "hempdustry:spliff" => Self {
                duration_ticks: 900,
                cooldown_ticks: 80,
                cough_one_in: 6,
                nausea_one_in: 500,
                green_out_factor: 2.0,
                bowl: None,
            },
            "hempdustry:wooden_pipe" => device(700, 60, 4, 50, 2, 2, 0),
            "hempdustry:bong" => device(1000, 100, 3, 5, 4, 3, 0),
            "hempdustry:vaporizer" => device(800, 60, 50, 200, 2, 1, 1),
            _ => return None,
        })
    }
}

/// `GreenOut`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GreenOut {
    pub one_in_by_dose: Vec<i32>,
    pub full_from_dose: i32,
    pub spins: Vec<Effect>,
    pub full: Vec<Effect>,
    pub lockout_ticks: i32,
}

impl GreenOut {
    /// `GreenOut.CODEC`.
    #[must_use]
    pub fn parse(value: &Value) -> Option<Self> {
        let effects = |key: &str| -> Option<Vec<Effect>> {
            value[key].as_array()?.iter().map(Effect::parse).collect()
        };
        let one_in_by_dose: Vec<i32> = value["one_in_by_dose"]
            .as_array()?
            .iter()
            .map(|v| int(v).filter(|n| *n >= 0))
            .collect::<Option<_>>()?;
        if one_in_by_dose.is_empty() {
            return None;
        }
        Some(Self {
            one_in_by_dose,
            full_from_dose: int(&value["full_from_dose"]).filter(|n| *n > 0)?,
            spins: effects("spins")?,
            full: effects("full")?,
            lockout_ticks: int(&value["lockout_ticks"]).filter(|n| *n >= 0)?,
        })
    }

    /// `GreenOut.BUILT_IN`.
    #[must_use]
    pub fn built_in() -> Self {
        let set = |nausea, rest| {
            vec![
                Effect::new("minecraft:nausea", nausea, 0),
                Effect::new("minecraft:slowness", rest, 1),
                Effect::new("minecraft:weakness", rest, 1),
                Effect::new("minecraft:mining_fatigue", rest, 1),
            ]
        };
        Self {
            one_in_by_dose: vec![0, 12, 4],
            full_from_dose: 3,
            spins: set(400, 300),
            full: set(700, 600),
            lockout_ticks: 1200,
        }
    }

    /// `GreenOut.oneIn`: the odds at `dose`, the last entry for any dose past the list.
    #[must_use]
    pub fn one_in(&self, dose: i32) -> i32 {
        if dose <= 0 {
            return 0;
        }
        let index = usize::try_from(dose)
            .unwrap_or(usize::MAX)
            .min(self.one_in_by_dose.len())
            - 1;
        self.one_in_by_dose[index]
    }
}

/// Everything loaded from the data, as one world sees it.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub strains: BTreeMap<String, Strain>,
    devices: BTreeMap<String, DeviceStats>,
    green_out: Option<GreenOut>,
    /// The effects the edibles give (`EdibleBundle`'s `effects[].effect`).
    pub edible_effects: BTreeSet<String>,
}

/// The Java mod's balance files are read under one default key each.
const GREEN_OUT_KEY: &str = "hempdustry:default";
const EDIBLE_KEY: &str = "hempdustry:default";

impl Catalog {
    /// Replaces the strains with `files` (`(id, json)`), as a data load lists them. A file the
    /// codec refuses is left out.
    pub fn set_strains(&mut self, files: &[(String, Value)]) {
        self.strains = files
            .iter()
            .filter_map(|(id, json)| Strain::parse(id, json).map(|s| (s.id.clone(), s)))
            .collect();
    }

    pub fn set_devices(&mut self, files: &[(String, Value)]) {
        self.devices = files
            .iter()
            .filter_map(|(id, json)| DeviceStats::parse(json).map(|d| (namespaced(id), d)))
            .collect();
    }

    pub fn set_green_out(&mut self, files: &[(String, Value)]) {
        self.green_out = files
            .iter()
            .find(|(id, _)| namespaced(id) == GREEN_OUT_KEY)
            .and_then(|(_, json)| GreenOut::parse(json));
    }

    pub fn set_edibles(&mut self, files: &[(String, Value)]) {
        self.edible_effects = files
            .iter()
            .filter(|(id, _)| namespaced(id) == EDIBLE_KEY)
            .flat_map(|(_, json)| json["effects"].as_array().into_iter().flatten())
            .filter_map(|effect| effect["effect"].as_str().map(namespaced))
            .collect();
    }

    #[must_use]
    pub fn strain(&self, id: &str) -> Option<&Strain> {
        self.strains.get(id)
    }

    /// `DeviceStats.of`: the device's file, else what the Java mod ships.
    #[must_use]
    pub fn device(&self, key: &str) -> Option<DeviceStats> {
        self.devices
            .get(key)
            .copied()
            .or_else(|| DeviceStats::built_in(key))
    }

    /// `DeviceStats.bowl`: the device file's bowl, else the built-in one.
    #[must_use]
    pub fn bowl(&self, key: &str) -> Option<Bowl> {
        self.device(key)
            .and_then(|d| d.bowl)
            .or_else(|| DeviceStats::built_in(key).and_then(|d| d.bowl))
    }

    #[must_use]
    pub fn green_out(&self) -> GreenOut {
        self.green_out.clone().unwrap_or_else(GreenOut::built_in)
    }

    /// `Strain.fromBuds`: the strain `item` packs as; ours first when two strains share buds,
    /// then by id.
    #[must_use]
    pub fn from_buds(&self, item: &str) -> Option<&Strain> {
        self.strains
            .values()
            .filter(|s| s.buds == item)
            .min_by_key(|s| (!s.id.starts_with("hempdustry:"), s.id.as_str()))
    }

    /// `Smoking.buffsTheModGrants`: every beneficial effect an edible or a strain gives.
    #[must_use]
    pub fn buffs_the_mod_grants(&self, category: &dyn Fn(&str) -> Category) -> BTreeSet<String> {
        self.edible_effects
            .iter()
            .cloned()
            .chain(
                self.strains
                    .values()
                    .flat_map(|s| s.effects.iter().map(|e| e.effect.clone())),
            )
            .filter(|id| category(id) == Category::Beneficial)
            .collect()
    }
}

/// `minecraft:` added to a bare id, as `Identifier.parse` reads one.
#[must_use]
pub fn namespaced(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

fn int(value: &Value) -> Option<i32> {
    value.as_i64().and_then(|n| i32::try_from(n).ok())
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the Java codecs read these fields as floats"
)]
fn float(value: &Value) -> Option<f32> {
    value.as_f64().map(|f| f as f32)
}

/// Java's `(float)` of an int: the nearest f32 (exact up to 2^24, which ticks and odds stay
/// far below).
#[expect(
    clippy::cast_precision_loss,
    reason = "the same rounding as Java's `(float)`"
)]
#[must_use]
pub const fn java_float(value: i32) -> f32 {
    value as f32
}

/// `Math.round(float)`: the nearest int, a tie toward positive infinity, NaN as 0 and the
/// ends saturated. The half is added in f64, where it is exact: in f32 the sum can round up
/// first (Java's `Math.round(0.49999997f)` is 0).
#[expect(
    clippy::cast_possible_truncation,
    reason = "saturating, and NaN as 0, as `Math.round` does"
)]
#[must_use]
pub fn java_round(value: f32) -> i32 {
    (f64::from(value) + 0.5).floor() as i32
}

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "colours copied as the Java data files write them"
)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_strain_file_reads_with_the_codecs_defaults() {
        let schwag = Strain::parse(
            "hempdustry:schwag",
            &json!({"buds":"hempdustry:schwag","color":9202236,"translation_key":"hempdustry.strain.schwag",
                "cough_factor":0.5,"green_out_factor":0.0,
                "effects":[{"chance":0.6,"duration_factor":0.14285715,"effect":"minecraft:poison",
                    "max_amplifier":1},{"effect":"minecraft:hunger"}]}),
        )
        .unwrap();
        assert!(!schwag.is_plant() && schwag.is_plant_matter());
        assert_eq!(schwag.dose_per_item, 1);
        assert_eq!(schwag.effects[0].max_amplifier, 1);
        assert_eq!(schwag.effects[1].max_amplifier, SmokeEffect::NO_CAP);
        assert!(schwag.effects[1].scales);
        assert_eq!(schwag.translation_key, "hempdustry.strain.schwag");
        // Each required field missing in turn.
        for missing in ["buds", "color", "translation_key"] {
            let mut file = json!({"buds": "x:b", "color": 1, "translation_key": "k"});
            if let Some(fields) = file.as_object_mut() {
                fields.remove(missing);
            }
            assert!(Strain::parse("x:y", &file).is_none(), "{missing}");
        }
    }

    /// `Math.round(float)`'s results as the Java docs define them: nearest, ties up.
    #[test]
    fn java_round_matches_math_round_at_its_edges() {
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(-2.5), -2);
        assert_eq!(java_round(0.499_999_97), 0);
        assert_eq!(java_round(8_388_609.0), 8_388_609);
        assert_eq!(java_round(f32::NAN), 0);
        assert_eq!(java_round(3.0e9), i32::MAX);
    }

    #[test]
    fn the_green_out_odds_hold_at_the_last_dose_listed() {
        let green = GreenOut::built_in();
        assert_eq!([0, 1, 2, 3, 7].map(|d| green.one_in(d)), [0, 0, 12, 4, 4]);
    }

    #[test]
    fn a_device_without_a_file_smokes_as_shipped_and_a_bad_file_too() {
        let mut catalog = Catalog::default();
        catalog.set_devices(&[(
            "hempdustry:wooden_pipe".into(),
            json!({"duration_ticks": 0, "cooldown_ticks": 1}),
        )]);
        assert_eq!(
            catalog.device("hempdustry:wooden_pipe"),
            DeviceStats::built_in("hempdustry:wooden_pipe")
        );
        assert_eq!(catalog.bowl("hempdustry:bong").unwrap().hits, 4);
        assert!(catalog.bowl("hempdustry:spliff").is_none());
    }

    #[test]
    fn java_rounding_goes_half_up() {
        assert_eq!([0.5, 1.5, -0.5, 2.4999].map(java_round), [1, 2, 0, 2]);
    }
}
