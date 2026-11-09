//! `HempdustryConfig`: the server's balance knobs, `config/hempdustry.json`, in the Java mod's
//! format, so a file carries over between the two editions either way.
//!
//! Missing fields keep their defaults, so a half-written file or one from an older version still
//! loads. Values outside their sane range are clamped and reported ([`Clamped`]), not rejected. A
//! file that can't be read at all (not JSON, not an object, a value of the wrong type) is an
//! [`Error`]: nothing in it applies and the caller leaves it as it is, so the typo can be fixed.
//! [`Settings::to_json`] writes the whole file back, every knob at its value and a `_comment`
//! first in each section, exactly as the Java mod writes it.

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

use crate::farm;
use crate::machine::infuser::Timings;
use crate::smoke::policy::Policy;

/// The file's name in the game's `config/` folder.
pub const FILE: &str = "hempdustry.json";

/// The longest simmer the Infuser's screen can be told about (`Infuser.MAX_TIME`).
const MAX_INFUSER_TIME: i32 = 32_000;

/// `HempdustryConfig.Client`: read by a client, sent to nobody. Kept so the file round-trips;
/// the biome tint and the ripe aroma aren't drawn by this edition yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Client {
    pub update_check: bool,
    /// 0 paints the crops as drawn, 1 is vanilla's full grass tint.
    pub biome_tint_strength: f64,
    pub ripe_aroma: bool,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            update_check: true,
            biome_tint_strength: 0.35,
            ripe_aroma: true,
        }
    }
}

/// `HempdustryConfig.World`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct World {
    /// A speed: 2.0 grows twice as fast.
    pub crop_growth_multiplier: f64,
    /// A speed for the Decarboxylator and the Hemp Press.
    pub machine_speed_multiplier: f64,
    /// Off, the creepers' goal is still added, it just never starts.
    pub creepers_seek_hemp: bool,
    pub mildew: bool,
}

impl Default for World {
    fn default() -> Self {
        Self {
            crop_growth_multiplier: 1.0,
            machine_speed_multiplier: 1.0,
            creepers_seek_hemp: true,
            mildew: true,
        }
    }
}

/// `HempdustryConfig.Loot`: the mod's additions to vanilla loot tables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loot {
    pub enabled: bool,
    pub chance_multiplier: f64,
}

impl Default for Loot {
    fn default() -> Self {
        Self {
            enabled: true,
            chance_multiplier: 1.0,
        }
    }
}

/// The whole config, as the rules read it. [`Settings::default`] is the mod as it ships.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    pub client: Client,
    pub effects: Policy,
    pub world: World,
    pub infuser: Timings,
    pub loot: Loot,
}

/// The settings every rule set reads, shared: replaced whole when the file is read again, so a
/// reader always sees one file's values.
#[derive(Clone, Debug, Default)]
pub struct Live(Arc<RwLock<Settings>>);

impl Live {
    /// The settings now in force.
    #[must_use]
    pub fn get(&self) -> Settings {
        *self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Puts `settings` in force for every reader.
    pub fn set(&self, settings: Settings) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = settings;
    }
}

/// A value [`Settings::parse`] had to pull into its range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clamped {
    pub name: &'static str,
    pub was: f64,
    pub now: f64,
}

impl fmt::Display for Clamped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "config {} was {}, clamped to {}",
            self.name, self.was, self.now
        )
    }
}

/// Why a file was not applied.
#[derive(Debug)]
pub enum Error {
    /// Valid JSON, but not an object.
    NotAnObject,
    /// Not JSON, or a value of the wrong type.
    Invalid(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject => f.write_str("not a JSON object"),
            Self::Invalid(e) => write!(f, "bad value: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotAnObject => None,
            Self::Invalid(e) => Some(e),
        }
    }
}

impl Settings {
    /// `HempdustryConfig.load`'s reading: `text` laid over the defaults, then clamped. An empty
    /// file (or `null`) is all defaults, the way to get a fresh one.
    ///
    /// # Errors
    ///
    /// [`Error::NotAnObject`] for JSON that is not an object, [`Error::Invalid`] for text that is
    /// not JSON or holds a value of the wrong type. Nothing of such a file applies.
    pub fn parse(text: &str) -> Result<(Self, Vec<Clamped>), Error> {
        if text.trim().is_empty() {
            return Ok((Self::default(), Vec::new()));
        }
        let json: Value = serde_json::from_str(text).map_err(Error::Invalid)?;
        let raw = match json {
            Value::Null => RawConfig::default(),
            Value::Object(_) => RawConfig::deserialize(json).map_err(Error::Invalid)?,
            _ => return Err(Error::NotAnObject),
        };
        let mut clamped = Vec::new();
        let settings = raw.into_settings(&mut clamped);
        Ok((settings, clamped))
    }

    /// The whole file as the Java mod writes it: its sections in the codec's order, each with
    /// its `_comment` first and its knobs in alphabetical order, two-space indents, a final
    /// newline.
    #[must_use]
    pub fn to_json(&self) -> String {
        let raw = RawConfig::from(*self);
        let root = Node::Map(vec![
            ("client".to_owned(), section(COMMENT_CLIENT, &raw.client)),
            ("effects".to_owned(), section(COMMENT_EFFECTS, &raw.effects)),
            ("world".to_owned(), section(COMMENT_WORLD, &raw.world)),
            ("infuser".to_owned(), section(COMMENT_INFUSER, &raw.infuser)),
            ("loot".to_owned(), section(COMMENT_LOOT, &raw.loot)),
        ]);
        let mut text = serde_json::to_string_pretty(&root).unwrap_or_default();
        text.push('\n');
        text
    }

    /// What the farming rules read.
    #[must_use]
    pub const fn farm(&self) -> farm::Config {
        farm::Config {
            crop_growth_multiplier: self.world.crop_growth_multiplier,
            mildew: self.world.mildew,
        }
    }

    /// `ticks` (a machine's time at speed 1) after `world.machineSpeedMultiplier`, never under 1:
    /// `DecarboxylatorBlockEntity.cookTime`, `HempPressBlockEntity.pressTime`.
    #[must_use]
    pub fn machine_time(&self, ticks: i32) -> i32 {
        java_round(f64::from(ticks) / self.world.machine_speed_multiplier).max(1)
    }
}

/// `Math.round(double)` cast to `int`: halves up, saturating at the ends.
fn java_round(x: f64) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's (int) Math.round saturates as Rust's float-to-int cast does"
    )]
    let rounded = (x + 0.5).floor() as i32;
    rounded
}

/// One value pulled into `min..=max`, noted when it moved.
fn clamp_f64(name: &'static str, value: f64, min: f64, max: f64, out: &mut Vec<Clamped>) -> f64 {
    let now = value.clamp(min, max);
    if now.total_cmp(&value).is_ne() {
        out.push(Clamped {
            name,
            was: value,
            now,
        });
    }
    now
}

fn clamp_i32(name: &'static str, value: i32, min: i32, max: i32, out: &mut Vec<Clamped>) -> i32 {
    let now = value.clamp(min, max);
    if now != value {
        out.push(Clamped {
            name,
            was: f64::from(value),
            now: f64::from(now),
        });
    }
    now
}

/// A Java `int` field: any JSON number, its fraction dropped, as the codec's `Number.intValue`
/// reads one, so `4.0` is 4.
fn java_int<'de, D: Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    let x = f64::deserialize(d)?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Number.intValue truncates and saturates, as this cast does"
    )]
    let int = x as i32;
    Ok(int)
}

// The file's shape, field for field. `default` on every struct is the Java mod's merge over the
// defaults; unknown keys (`_comment`) are ignored.

#[derive(Deserialize, Serialize, Default)]
#[serde(default)]
struct RawConfig {
    client: RawClient,
    effects: RawEffects,
    world: RawWorld,
    infuser: RawInfuser,
    loot: RawLoot,
}

#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RawClient {
    update_check: bool,
    biome_tint_strength: f64,
    ripe_aroma: bool,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "the switches of `HempdustryConfig.Effects`, one for one"
)]
#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RawEffects {
    enabled: bool,
    #[serde(deserialize_with = "java_int")]
    max_level: i32,
    #[serde(deserialize_with = "java_int")]
    max_buff_level: i32,
    duration_multiplier: f64,
    cooldown_multiplier: f64,
    green_out_chance_multiplier: f64,
    onset_multiplier: f64,
    green_out: bool,
    nausea: bool,
    munchies: bool,
    debuffs: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RawWorld {
    crop_growth_multiplier: f64,
    machine_speed_multiplier: f64,
    creepers_seek_hemp: bool,
    mildew: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RawInfuser {
    #[serde(deserialize_with = "java_int")]
    min_time_ticks: i32,
    #[serde(deserialize_with = "java_int")]
    full_time_ticks: i32,
}

#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RawLoot {
    enabled: bool,
    chance_multiplier: f64,
}

impl Default for RawClient {
    fn default() -> Self {
        RawConfig::from(Settings::default()).client
    }
}
impl Default for RawEffects {
    fn default() -> Self {
        RawConfig::from(Settings::default()).effects
    }
}
impl Default for RawWorld {
    fn default() -> Self {
        RawConfig::from(Settings::default()).world
    }
}
impl Default for RawInfuser {
    fn default() -> Self {
        RawConfig::from(Settings::default()).infuser
    }
}
impl Default for RawLoot {
    fn default() -> Self {
        RawConfig::from(Settings::default()).loot
    }
}

impl From<Settings> for RawConfig {
    fn from(s: Settings) -> Self {
        let Settings {
            client,
            effects: e,
            world,
            infuser,
            loot,
        } = s;
        Self {
            client: RawClient {
                update_check: client.update_check,
                biome_tint_strength: client.biome_tint_strength,
                ripe_aroma: client.ripe_aroma,
            },
            effects: RawEffects {
                enabled: e.enabled,
                max_level: e.max_level,
                max_buff_level: e.max_buff_level,
                duration_multiplier: e.duration_multiplier,
                cooldown_multiplier: e.cooldown_multiplier,
                green_out_chance_multiplier: e.green_out_chance_multiplier,
                onset_multiplier: e.onset_multiplier,
                green_out: e.green_out,
                nausea: e.nausea,
                munchies: e.munchies,
                debuffs: e.debuffs,
            },
            world: RawWorld {
                crop_growth_multiplier: world.crop_growth_multiplier,
                machine_speed_multiplier: world.machine_speed_multiplier,
                creepers_seek_hemp: world.creepers_seek_hemp,
                mildew: world.mildew,
            },
            infuser: RawInfuser {
                min_time_ticks: infuser.min,
                full_time_ticks: infuser.full,
            },
            loot: RawLoot {
                enabled: loot.enabled,
                chance_multiplier: loot.chance_multiplier,
            },
        }
    }
}

impl RawConfig {
    /// Every value pulled into its range (`HempdustryConfig.clamped`), each move noted.
    fn into_settings(self, out: &mut Vec<Clamped>) -> Settings {
        let Self {
            client,
            effects,
            world,
            infuser,
            loot,
        } = self;
        let min = clamp_i32(
            "infuser.minTimeTicks",
            infuser.min_time_ticks,
            20,
            MAX_INFUSER_TIME - 20,
            out,
        );
        let full = clamp_i32(
            "infuser.fullTimeTicks",
            infuser.full_time_ticks,
            min + 20,
            MAX_INFUSER_TIME,
            out,
        );
        Settings {
            client: Client {
                update_check: client.update_check,
                biome_tint_strength: clamp_f64(
                    "client.biomeTintStrength",
                    client.biome_tint_strength,
                    0.0,
                    1.0,
                    out,
                ),
                ripe_aroma: client.ripe_aroma,
            },
            effects: effects.into_policy(out),
            world: World {
                crop_growth_multiplier: clamp_f64(
                    "world.cropGrowthMultiplier",
                    world.crop_growth_multiplier,
                    0.05,
                    20.0,
                    out,
                ),
                machine_speed_multiplier: clamp_f64(
                    "world.machineSpeedMultiplier",
                    world.machine_speed_multiplier,
                    0.05,
                    20.0,
                    out,
                ),
                creepers_seek_hemp: world.creepers_seek_hemp,
                mildew: world.mildew,
            },
            infuser: Timings { min, full },
            loot: Loot {
                enabled: loot.enabled,
                chance_multiplier: clamp_f64(
                    "loot.chanceMultiplier",
                    loot.chance_multiplier,
                    0.0,
                    10.0,
                    out,
                ),
            },
        }
    }
}

impl RawEffects {
    /// `Effects.clamped`.
    fn into_policy(self, out: &mut Vec<Clamped>) -> Policy {
        Policy {
            enabled: self.enabled,
            max_level: clamp_i32("effects.maxLevel", self.max_level, 1, 10, out),
            max_buff_level: clamp_i32("effects.maxBuffLevel", self.max_buff_level, 1, 10, out),
            duration_multiplier: clamp_f64(
                "effects.durationMultiplier",
                self.duration_multiplier,
                0.05,
                10.0,
                out,
            ),
            cooldown_multiplier: clamp_f64(
                "effects.cooldownMultiplier",
                self.cooldown_multiplier,
                0.0,
                10.0,
                out,
            ),
            green_out_chance_multiplier: clamp_f64(
                "effects.greenOutChanceMultiplier",
                self.green_out_chance_multiplier,
                0.0,
                100.0,
                out,
            ),
            onset_multiplier: clamp_f64(
                "effects.onsetMultiplier",
                self.onset_multiplier,
                0.05,
                10.0,
                out,
            ),
            green_out: self.green_out,
            nausea: self.nausea,
            munchies: self.munchies,
            debuffs: self.debuffs,
        }
    }
}

/// A JSON tree whose objects keep the order they are built in, whatever `serde_json`'s map is.
enum Node {
    Leaf(Value),
    Map(Vec<(String, Node)>),
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Leaf(v) => v.serialize(s),
            Self::Map(entries) => {
                let mut map = s.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

/// A section: its comment, then its knobs in alphabetical order (`HempdustryConfig.comment`).
fn section(comment: &'static str, raw: &impl Serialize) -> Node {
    let mut knobs: Vec<(String, Value)> = match serde_json::to_value(raw) {
        Ok(Value::Object(map)) => map.into_iter().collect(),
        _ => Vec::new(),
    };
    knobs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut entries = Vec::with_capacity(knobs.len() + 1);
    entries.push(("_comment".to_owned(), Node::Leaf(Value::from(comment))));
    entries.extend(knobs.into_iter().map(|(k, v)| (k, Node::Leaf(v))));
    Node::Map(entries)
}

const COMMENT_CLIENT: &str = "Read by a client, sent to nobody. updateCheck=false stops this client checking for a newer release on start-up; a dedicated server never asks. Turn it off in a modpack, where the update it would announce cannot be taken anyway. biomeTintStrength is 0-1: how far hemp growing in the world is pulled towards the local grass colour, so a plant does not clash with the biome around it. 0 paints the crops exactly as drawn, 1 is the full tint vanilla gives grass. ripeAroma=false stops the wisps that drift off ripe plants; vanilla's Particles setting thins or hides them along with every other particle.";
const COMMENT_EFFECTS: &str = "enabled=false is 'industrial hemp only': no drug effects anywhere. maxLevel caps every effect the mod applies; maxBuffLevel caps only the helpful ones (Speed, Resistance, Absorption...), so a big dose still costs its full Hunger. Raise maxBuffLevel for a stronger mod in a modded pack. The multipliers are 0.05-10, except cooldownMultiplier, 0-10 (0 is no cooldown), and greenOutChanceMultiplier, 0-100. greenOutChanceMultiplier 0 disables green-outs, as do greenOut=false and enabled=false.";
const COMMENT_WORLD: &str = "cropGrowthMultiplier and machineSpeedMultiplier are speeds: 2.0 is twice as fast. machineSpeed drives the Decarboxylator and the Hemp Press; the Infuser has its own section. Whether bees pollinate hemp is the #minecraft:bee_growables tag, not a setting here. creepersSeekHemp=false stops creepers now and then wandering into grown, open-sky hemp fields; the goal is still added to them, it simply never starts, so it takes effect on the next /hempdustry reload. mildew=false keeps powdery mildew out of roofed grows: no plant catches it, and a plant that has it is clean again on its next random tick.";
const COMMENT_INFUSER: &str = "Ticks. minTime is the earliest a batch can be pulled, fullTime a full simmer. 20 ticks = 1 second. Defaults are 5 and 15 minutes; the ceiling is 32000 (about 26 minutes), which is as long a simmer as the screen can be told about.";
const COMMENT_LOOT: &str = "The mod's additions to vanilla loot tables: hemp seeds in grass and chests, music discs in chests, shipwreck fibre. enabled=false removes all of them. Creepers dropping the two discs is the #minecraft:creeper_drop_music_discs tag, not a setting here.";

#[cfg(test)]
mod tests;
