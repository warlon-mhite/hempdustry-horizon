//! The mod's own loot entry `hempdustry:spoiling`, rebuilt from what Horizon's loot engines can run.
//!
//! In the Java mod, a spoiling entry wraps another entry and turns some of what it makes into a
//! worse item (a ripe plant's buds into schwag): a conversion, never an extra drop. Horizon's
//! engines know no entry types but vanilla's (an unknown one makes nothing), though they call a
//! mod's own loot *functions*. So each table is rewritten as it loads:
//!
//! - the spoiling entry becomes its child, carrying the spoiling entry's conditions as well, with
//!   the function [`KEEP`] last: it rolls how many items spoil and keeps the rest;
//! - right after the pool that held it comes a pool of one entry, the spoiled item, whose
//!   function [`SPOILED`] sets its count to what the roll before it took (none: nothing drops).
//!
//! A roll runs its pools in order on one thread, so the count passes between them in a
//! thread-local. As in Java, the roll takes one draw from the loot random (the context's seed)
//! and decides on a random seeded from that, read as `RandomSource.create(seed)` reads it.

use std::cell::Cell;

use serde_json::{Map, Value, json};

/// The entry type the Java mod registers.
pub(crate) const ENTRY: &str = "hempdustry:spoiling";
/// The function that keeps what didn't spoil.
pub(crate) const KEEP: &str = "hempdustry:spoiling_keep";
/// The function that drops what did.
pub(crate) const SPOILED: &str = "hempdustry:spoiling_spoiled";

thread_local! {
    /// How many items the last [`KEEP`] of this thread's roll spoiled.
    static SPOILED_COUNT: Cell<i32> = const { Cell::new(0) };
}

/// Rewrites every spoiling entry of `table`. False when it has none, and the table is unchanged.
pub(crate) fn rewrite_table(table: &mut Value) -> bool {
    let Some(pools) = table.get_mut("pools").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut rewritten = Vec::with_capacity(pools.len());
    let mut changed = false;
    for mut pool in pools.drain(..) {
        let mut spoils_into = None;
        if let Some(entries) = pool.get_mut("entries").and_then(Value::as_array_mut) {
            for entry in entries {
                rewrite_entry(entry, &mut spoils_into);
            }
        }
        rewritten.push(pool);
        if let Some(into) = spoils_into {
            changed = true;
            rewritten.push(json!({
                "rolls": 1,
                "entries": [{"type": "minecraft:item", "name": into, "modifier": [{"function": SPOILED}]}],
            }));
        }
    }
    *pools = rewritten;
    changed
}

/// Rewrites `entry` and the entries inside it; `into` takes the spoiled item of the last one met.
fn rewrite_entry(entry: &mut Value, into: &mut Option<Value>) {
    if let Some(children) = entry.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            rewrite_entry(child, into);
        }
    }
    if entry["type"] != ENTRY {
        return;
    }
    let Some(Value::Object(mut child)) = entry.get_mut("child").map(Value::take) else {
        *entry = json!({"type": "minecraft:empty"});
        return;
    };
    // Both entries' conditions decide whether it fires.
    let conditions: Vec<Value> = [&*entry, &Value::Object(child.clone())]
        .into_iter()
        .flat_map(|e| {
            let one = e.get("condition").cloned();
            let many = e
                .get("conditions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            one.into_iter().chain(many)
        })
        .collect();
    child.remove("condition");
    if !conditions.is_empty() {
        child.insert("conditions".into(), Value::Array(conditions));
    }
    let mut keep = Map::new();
    keep.insert("function".into(), KEEP.into());
    for key in ["enchantment", "one_item_chances", "each_item_chances"] {
        if let Some(value) = entry.get(key) {
            keep.insert(key.into(), value.clone());
        }
    }
    let mut modifier = match child.remove("modifier") {
        Some(Value::Array(list)) => list,
        Some(Value::Null) | None => Vec::new(),
        Some(one) => vec![one],
    };
    modifier.push(Value::Object(keep));
    child.insert("modifier".into(), Value::Array(modifier));
    *into = Some(entry["into"].clone());
    let mut child = Value::Object(child);
    rewrite_entry(&mut child, into);
    *entry = child;
}

/// [`KEEP`]: of `count` items, how many are kept; the spoiled rest is left for [`SPOILED`].
/// `level` is the tool's level of the spoiling's enchantment.
pub(crate) fn keep(data: &Value, seed: i64, level: i32, count: i32) -> i32 {
    let spoiled = spoiled_of(data, seed, level, count);
    SPOILED_COUNT.with(|c| c.set(spoiled));
    count - spoiled
}

/// [`SPOILED`]: the count the roll before it spoiled, taken.
pub(crate) fn take_spoiled() -> i32 {
    SPOILED_COUNT.with(|c| c.replace(0))
}

/// `SpoilingEntry.spoil`: one roll for a single item, then one for each item left.
fn spoiled_of(data: &Value, seed: i64, level: i32, count: i32) -> i32 {
    let mut random = JavaRandom::new(seed);
    let mut spoiled = 0;
    if count > 0 && random.next_float() < chance(&data["one_item_chances"], level) {
        spoiled = 1;
    }
    let each = chance(&data["each_item_chances"], level);
    if each > 0.0 {
        let after_one = spoiled;
        for _ in after_one..count {
            if random.next_float() < each {
                spoiled += 1;
            }
        }
    }
    spoiled
}

/// A `table_bonus`-style list read at `level`: the last entry covers every higher level, and an
/// empty or absent list never fires.
fn chance(chances: &Value, level: i32) -> f32 {
    let Some(list) = chances.as_array().filter(|l| !l.is_empty()) else {
        return 0.0;
    };
    let at = usize::try_from(level).unwrap_or(0).min(list.len() - 1);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the Java codec reads the chance as a float"
    )]
    list[at].as_f64().map_or(0.0, |c| c as f32)
}

/// The enchantment level the spoiling names, on `tool`'s enchantments; 0 with none named.
pub(crate) fn level_of(data: &Value, enchantments: &[(String, i32)]) -> i32 {
    data["enchantment"].as_str().map_or(0, |wanted| {
        enchantments
            .iter()
            .find(|(id, _)| id == wanted)
            .map_or(0, |&(_, level)| level)
    })
}

/// `LegacyRandomSource` (`RandomSource.create(seed)`): `java.util.Random`'s generator.
struct JavaRandom(i64);

impl JavaRandom {
    const MULTIPLIER: i64 = 0x5_DEEC_E66D;
    const MASK: i64 = (1 << 48) - 1;

    const fn new(seed: i64) -> Self {
        Self((seed ^ Self::MULTIPLIER) & Self::MASK)
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(Self::MULTIPLIER).wrapping_add(0xB)) & Self::MASK;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "Java's `(int)` of a long keeps the low 32 bits, as `as` does"
        )]
        let out = (self.0 >> (48 - bits)) as i32;
        out
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "`Random.nextFloat`: 24 bits, exact in an f32"
    )]
    fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bud_table() -> Value {
        json!({"pools": [
            {"rolls": 1, "entries": [{"type": "minecraft:alternatives", "children": [
                {"type": ENTRY, "condition": {"type": "minecraft:match_block", "state": {"age": "7"}},
                 "into": "hempdustry:schwag", "enchantment": "minecraft:fortune",
                 "each_item_chances": [0.25, 0.2, 0.15, 0.1],
                 "child": {"type": "minecraft:item", "name": "hempdustry:indica_buds",
                           "modifier": [{"type": "minecraft:set_count", "count": 2}]}},
                {"type": "minecraft:item", "name": "hempdustry:hemp_leaf"}
            ]}]},
            {"rolls": 1, "entries": [{"type": "minecraft:item", "name": "hempdustry:indica_seeds"}]}
        ]})
    }

    #[test]
    fn a_spoiling_entry_becomes_its_child_and_a_pool_for_what_spoils() {
        let mut table = bud_table();
        assert!(rewrite_table(&mut table));
        let pools = table["pools"].as_array().expect("pools");
        assert_eq!(
            pools.len(),
            3,
            "the spoiled pool comes right after the one it pairs with"
        );

        let bud = &pools[0]["entries"][0]["children"][0];
        assert_eq!(bud["type"], "minecraft:item");
        assert_eq!(bud["name"], "hempdustry:indica_buds");
        assert_eq!(
            bud["conditions"][0]["type"], "minecraft:match_block",
            "the spoiling's condition stays"
        );
        assert_eq!(
            bud["modifier"][0]["type"], "minecraft:set_count",
            "the child's functions run first"
        );
        assert_eq!(bud["modifier"][1]["function"], KEEP);
        assert_eq!(bud["modifier"][1]["each_item_chances"][0], 0.25);

        let spoiled = &pools[1]["entries"][0];
        assert_eq!(spoiled["name"], "hempdustry:schwag");
        assert_eq!(spoiled["modifier"][0]["function"], SPOILED);
        assert_eq!(pools[2]["entries"][0]["name"], "hempdustry:indica_seeds");
    }

    #[test]
    fn a_table_without_spoiling_is_left_alone() {
        let mut table = json!({"pools": [{"rolls": 1, "entries": [{"type": "minecraft:item", "name": "minecraft:stick"}]}]});
        let before = table.clone();
        assert!(!rewrite_table(&mut table));
        assert_eq!(table, before);
    }

    #[test]
    fn what_spoils_and_what_is_kept_add_up_to_what_was_made() {
        let data = json!({"each_item_chances": [0.5]});
        for seed in 0..200 {
            let kept = keep(&data, seed, 0, 4);
            let spoiled = take_spoiled();
            assert_eq!(kept + spoiled, 4);
            assert_eq!(take_spoiled(), 0, "taken once");
        }
    }

    #[test]
    fn certain_odds_spoil_everything_and_none_spoil_nothing() {
        assert_eq!(keep(&json!({"each_item_chances": [1.0]}), 7, 0, 3), 0);
        assert_eq!(take_spoiled(), 3);
        assert_eq!(keep(&json!({}), 7, 0, 3), 3);
        assert_eq!(take_spoiled(), 0);
        assert_eq!(
            keep(&json!({"one_item_chances": [1.0]}), 7, 0, 3),
            2,
            "one item, once"
        );
    }

    #[test]
    fn fortune_reads_the_odds_at_its_level_and_the_last_beyond() {
        let data = json!({"enchantment": "minecraft:fortune", "each_item_chances": [1.0, 0.0]});
        let fortune = |level| vec![("minecraft:fortune".to_owned(), level)];
        assert_eq!(level_of(&data, &fortune(3)), 3);
        assert_eq!(level_of(&data, &[]), 0);
        assert_eq!(keep(&data, 1, 0, 2), 0);
        assert_eq!(keep(&data, 1, 5, 2), 2);
        take_spoiled();
    }

    #[test]
    fn the_random_is_java_s() {
        // new java.util.Random(42).nextFloat() == 0.7275637f
        let mut random = JavaRandom::new(42);
        assert!((random.next_float() - 0.727_563_7).abs() < 1e-6);
    }
}
