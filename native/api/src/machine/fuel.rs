//! Furnace fuel as Minecraft 26.3 states it: an item's `minecraft:cooking_fuel` component names its
//! burn time as a `context_int_provider`, an expression or a reference to one of the data files
//! (`minecraft:cooking/time_coal` is 1600 divided by a factor, 1 in a furnace and 2 in a fast one).
//!
//! A native mod can't ask Horizon how long an item burns, so the machines read the components
//! themselves ([`ItemFacts`], recorded as Horizon reads the items' defaults) and the provider files
//! ([`Providers`], from a data load), and evaluate them the way Horizon's own furnace does.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

/// The component that makes an item a fuel.
pub const COOKING_FUEL: &str = "minecraft:cooking_fuel";
const MAX_STACK_SIZE: &str = "minecraft:max_stack_size";
const CRAFTING_REMAINDER: &str = "minecraft:crafting_remainder";
/// Our burn times' references, `hempdustry:cooking/time_<ticks>` (`ModFuelAndCompost`).
const OWN_TIME: &str = "hempdustry:cooking/time_";
/// How deep a reference may chain before it counts as nothing, as in Horizon.
const MAX_DEPTH: u32 = 16;

/// What the machines read of an item's default components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemFacts {
    /// `cooking_fuel.burn_time`: the provider, unevaluated.
    pub burn_time: Option<Value>,
    /// What a used-up fuel leaves behind (a lava bucket's bucket): `getCraftingRemainder`.
    pub remainder: Option<String>,
    pub max_stack: u32,
}

impl Default for ItemFacts {
    fn default() -> Self {
        Self {
            burn_time: None,
            remainder: None,
            max_stack: 64,
        }
    }
}

impl ItemFacts {
    /// Read from `item`'s default components, as `items.json` writes them. Vanilla's craft
    /// remainders are no component (`Item.Properties.craftRemainder`), so they come from
    /// Horizon's table of them.
    #[must_use]
    pub fn read(item: &str, components: &Map<String, Value>) -> Self {
        let remainder = components
            .get(CRAFTING_REMAINDER)
            .and_then(|value| value["id"].as_str().or_else(|| value.as_str()))
            .or_else(|| {
                horizon_client::items::CRAFT_REMAINDERS
                    .iter()
                    .find(|(of, _)| *of == item)
                    .map(|(_, left)| *left)
            })
            .map(str::to_owned);
        Self {
            burn_time: components
                .get(COOKING_FUEL)
                .map(|fuel| fuel["burn_time"].clone())
                .filter(|time| !time.is_null()),
            remainder,
            max_stack: components
                .get(MAX_STACK_SIZE)
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .filter(|&n| n > 0)
                .unwrap_or(64),
        }
    }
}

/// Our own burn times written out where they are named: `hempdustry:cooking/time_<ticks>` becomes
/// the expression the Java mod's data file holds for it. Horizon's furnaces resolve only
/// `minecraft` references, so without this a hemp stem burns for nothing in them. Returns whether
/// `components` changed.
pub fn inline_own_fuel(components: &mut Map<String, Value>) -> bool {
    let Some(fuel) = components.get_mut(COOKING_FUEL) else {
        return false;
    };
    let Some(ticks) = fuel["burn_time"]
        .as_str()
        .and_then(|name| name.strip_prefix(OWN_TIME))
        .and_then(|ticks| ticks.parse::<u32>().ok())
    else {
        return false;
    };
    fuel["burn_time"] = own_time(ticks);
    true
}

/// `ModFuelAndCompost`'s burn time: `ticks` over vanilla's reduction factor, halved in a blast
/// furnace or smoker as vanilla's own fuels are.
fn own_time(ticks: u32) -> Value {
    json!({
        "type": "minecraft:div",
        "left": ticks,
        "right": {
            "type": "minecraft:conditional",
            "condition": "minecraft:block/fast_cooking",
            "on_false": "minecraft:cooking/normal_burn_time_reduction_factor",
            "on_true": "minecraft:cooking/fast_burn_time_reduction_factor"
        }
    })
}

/// The `context_int_provider` files of a data load, by id (`minecraft:cooking/time_coal`).
#[derive(Clone, Debug, Default)]
pub struct Providers {
    ints: HashMap<String, Value>,
}

impl Providers {
    /// From a reload listener's files, ids as the listener names them.
    #[must_use]
    pub fn from_files(files: &[(String, Value)]) -> Self {
        Self {
            ints: files
                .iter()
                .map(|(id, value)| (qualified(id), value.clone()))
                .collect(),
        }
    }

    /// Ticks `burn_time` gives in a furnace that is not fast (a Decarboxylator is a slow oven).
    #[must_use]
    pub fn burn_ticks(&self, burn_time: &Value) -> i32 {
        let ticks = self.eval(burn_time, false, 0);
        // A tick count: whole, and inside an int, as Horizon's furnace casts it.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "Horizon's furnace casts the f64 to i32 the same way: toward zero, saturating"
        )]
        let ticks = ticks as i32;
        ticks.max(0)
    }

    /// Horizon's `Providers::eval`: numbers, references, `div` (toward zero, 0 over 0), `mul`,
    /// `add`/`sum`, `sub`, `constant`, and `conditional` on `minecraft:block/fast_cooking`.
    /// Anything else, or a reference chain past [`MAX_DEPTH`], is 0.
    fn eval(&self, value: &Value, fast: bool, depth: u32) -> f64 {
        if depth > MAX_DEPTH {
            return 0.0;
        }
        match value {
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::String(name) => self
                .ints
                .get(&qualified(name))
                .map_or(0.0, |v| self.eval(v, fast, depth + 1)),
            Value::Object(o) => {
                let side = |k: &str| self.eval(o.get(k).unwrap_or(&Value::Null), fast, depth + 1);
                match o.get("type").and_then(Value::as_str).unwrap_or("") {
                    "minecraft:div" => {
                        let right = side("right");
                        if right == 0.0 {
                            0.0
                        } else {
                            (side("left") / right).trunc()
                        }
                    }
                    "minecraft:mul" => side("left") * side("right"),
                    "minecraft:add" | "minecraft:sum" => side("left") + side("right"),
                    "minecraft:sub" => side("left") - side("right"),
                    "minecraft:constant" => side("value"),
                    "minecraft:conditional" => {
                        let on_fast = o.get("condition").and_then(Value::as_str)
                            == Some("minecraft:block/fast_cooking");
                        side(if on_fast && fast {
                            "on_true"
                        } else {
                            "on_false"
                        })
                    }
                    _ => 0.0,
                }
            }
            _ => 0.0,
        }
    }
}

/// A bare id is `minecraft:`'s, as vanilla reads one.
fn qualified(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vanilla's providers as the 26.3 data writes them.
    fn vanilla() -> Providers {
        let time = |left: u32| {
            json!({"type": "minecraft:div", "left": left, "right": {
                "type": "minecraft:conditional", "condition": "minecraft:block/fast_cooking",
                "on_false": "minecraft:cooking/normal_burn_time_reduction_factor",
                "on_true": "minecraft:cooking/fast_burn_time_reduction_factor"}})
        };
        Providers::from_files(&[
            ("minecraft:cooking/time_coal".into(), time(1600)),
            ("minecraft:cooking/time_dried_kelp_block".into(), time(4001)),
            (
                "minecraft:cooking/normal_burn_time_reduction_factor".into(),
                json!(1),
            ),
            (
                "minecraft:cooking/fast_burn_time_reduction_factor".into(),
                json!(2),
            ),
        ])
    }

    #[test]
    #[expect(clippy::float_cmp, reason = "a whole number of ticks, exact in f64")]
    fn coal_burns_1600_ticks_in_a_slow_oven() {
        assert_eq!(
            vanilla().burn_ticks(&json!("minecraft:cooking/time_coal")),
            1600
        );
        // The fast branch exists, but an oven never takes it.
        let providers = vanilla();
        assert_eq!(
            providers.eval(&json!("minecraft:cooking/time_dried_kelp_block"), true, 0),
            2000.0
        );
        assert_eq!(
            providers.burn_ticks(&json!("minecraft:cooking/time_dried_kelp_block")),
            4001
        );
    }

    #[test]
    fn our_own_reference_is_written_out_and_burns_as_named() {
        let mut components = Map::new();
        components.insert(
            COOKING_FUEL.into(),
            json!({"burn_time": "hempdustry:cooking/time_400",
                "speed_multiplier": "minecraft:cooking/speed_default"}),
        );
        assert!(inline_own_fuel(&mut components));
        let facts = ItemFacts::read("hempdustry:hemp_stem", &components);
        assert_eq!(vanilla().burn_ticks(&facts.burn_time.unwrap()), 400);
        // Written out once, it is no longer a reference: a second pass leaves it.
        assert!(!inline_own_fuel(&mut components));
        assert_eq!(
            components[COOKING_FUEL]["speed_multiplier"],
            "minecraft:cooking/speed_default"
        );
    }

    #[test]
    fn an_unknown_reference_or_shape_burns_for_nothing() {
        let providers = vanilla();
        assert_eq!(
            providers.burn_ticks(&json!("minecraft:cooking/time_nope")),
            0
        );
        assert_eq!(
            providers.burn_ticks(&json!({"type": "minecraft:random", "left": 9})),
            0
        );
        // A reference to itself stops at the depth limit.
        let looping = Providers::from_files(&[("a:loop".into(), json!("a:loop"))]);
        assert_eq!(looping.burn_ticks(&json!("a:loop")), 0);
    }

    #[test]
    fn the_facts_read_stack_size_and_remainder_with_vanilla_defaults() {
        let bucket: Map<String, Value> = serde_json::from_value(json!({
            "minecraft:max_stack_size": 1,
            "minecraft:cooking_fuel": {"burn_time": "minecraft:cooking/time_lava_bucket"}
        }))
        .unwrap();
        let facts = ItemFacts::read("minecraft:lava_bucket", &bucket);
        assert_eq!(facts.max_stack, 1);
        assert_eq!(facts.remainder.as_deref(), Some("minecraft:bucket"));
        assert_eq!(
            ItemFacts::read("minecraft:stone", &Map::new()),
            ItemFacts::default()
        );
    }
}
