//! The mod's own advancement trigger `hempdustry:harvest_hemp`: a ripe plant harvested, with how
//! many trims it had, the light it kept a record of and the bed it grew in.
//!
//! Horizon tests each criterion through a closure while it holds the host, so the closure can't
//! ask the host whether the bed is in a tag. It answers from what is known and notes the tags it
//! lacked; [`HarvestTest::missing`] lists them, and the caller fires once more with them looked
//! up. A criterion granted on the first pass was granted rightly (no unknown tag decided it), and
//! the second pass only tests the ones still open.

use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::Value;

use crate::farm::model::Plant;
use crate::host::StateText;

/// The trigger's id, as the advancement files name it.
pub const HARVEST_HEMP: &str = "hempdustry:harvest_hemp";

/// One harvest, as the criterion's conditions read it.
pub struct HarvestTest<'a> {
    pub cuts: u8,
    /// The `condition` the plant kept.
    pub condition: &'static str,
    /// The block under the lower segment, as `/setblock` spells it.
    pub bed: StateText<'a>,
    /// Whether the bed is in a block tag, for the tags already looked up.
    pub tags: HashMap<String, bool>,
    missing: RefCell<Vec<String>>,
}

impl<'a> HarvestTest<'a> {
    #[must_use]
    pub fn new(plant: &Plant, bed: StateText<'a>) -> Self {
        Self {
            cuts: plant.cut_count(),
            condition: plant.condition.name(),
            bed,
            tags: HashMap::new(),
            missing: RefCell::default(),
        }
    }

    /// `HarvestHempCriterion.Conditions.matches`: every filter present must pass.
    #[must_use]
    pub fn matches(&self, conditions: &Value) -> bool {
        ints_match(&conditions["cuts"], i64::from(self.cuts))
            && conditions["condition"]
                .as_str()
                .is_none_or(|wanted| wanted == self.condition)
            && (conditions["bed"].is_null() || self.bed_matches(&conditions["bed"]))
    }

    /// The tags a test asked about that weren't looked up, each once; emptied by the call.
    pub fn missing(&self) -> Vec<String> {
        std::mem::take(&mut *self.missing.borrow_mut())
    }

    /// A vanilla `BlockPredicate` on the bed: `blocks` and `state`. Our beds carry no block
    /// entity, so a predicate on `nbt` or `components` never holds.
    fn bed_matches(&self, predicate: &Value) -> bool {
        let Some(fields) = predicate.as_object() else {
            return false;
        };
        if fields.contains_key("nbt") || fields.contains_key("components") {
            return false;
        }
        let blocks = match &predicate["blocks"] {
            Value::Null => true,
            Value::String(one) => self.is_block(one),
            Value::Array(list) => list
                .iter()
                .filter_map(Value::as_str)
                .any(|id| self.is_block(id)),
            _ => false,
        };
        blocks
            && predicate["state"].as_object().is_none_or(|state| {
                state.iter().all(|(key, wanted)| {
                    self.bed
                        .get(key)
                        .is_some_and(|value| state_value_matches(wanted, value))
                })
            })
    }

    /// `id` is the bed's block, or `#tag` holds it.
    fn is_block(&self, id: &str) -> bool {
        let Some(tag) = id.strip_prefix('#') else {
            return id == self.bed.name;
        };
        if let Some(&known) = self.tags.get(tag) {
            return known;
        }
        let mut missing = self.missing.borrow_mut();
        if !missing.iter().any(|t| t == tag) {
            missing.push(tag.to_owned());
        }
        false
    }
}

/// `MinMaxBounds.Ints`: absent, an exact number, or `{min, max}` with either left out.
fn ints_match(bounds: &Value, value: i64) -> bool {
    match bounds {
        Value::Null => true,
        Value::Number(exact) => exact.as_i64() == Some(value),
        Value::Object(range) => {
            range
                .get("min")
                .and_then(Value::as_i64)
                .is_none_or(|min| value >= min)
                && range
                    .get("max")
                    .and_then(Value::as_i64)
                    .is_none_or(|max| value <= max)
        }
        _ => false,
    }
}

/// A `StatePropertiesPredicate` entry: an exact value, or `{min, max}` over an integer property.
fn state_value_matches(wanted: &Value, value: &str) -> bool {
    match wanted {
        Value::String(exact) => exact == value,
        Value::Bool(exact) => exact.to_string() == value,
        Value::Number(exact) => exact.to_string() == value,
        Value::Object(range) => {
            let Ok(value) = value.parse::<i64>() else {
                return false;
            };
            let bound = |key| {
                range
                    .get(key)
                    .and_then(|b: &Value| b.as_i64().or_else(|| b.as_str()?.parse().ok()))
            };
            bound("min").is_none_or(|min| value >= min)
                && bound("max").is_none_or(|max| value <= max)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::farm::model::{Condition, Kind, Segment};
    use serde_json::json;

    fn ripe() -> Plant {
        let mut plant = Plant::new(Kind::Indica, Plant::MAX_AGE, Segment::Lower);
        plant.trimmed_early = true;
        plant.trimmed_late = true;
        plant.condition = Condition::GrowLamp;
        plant
    }

    const TRAY: &str = "hempdustry:hydro_tray[facing=north,fed=true,level=2,powered=false]";

    #[test]
    fn top_shelf_wants_both_cuts_a_grow_lamp_and_a_tray() {
        let top_shelf = json!({"bed": {"blocks": "hempdustry:hydro_tray"}, "condition": "grow_lamp", "cuts": 2});
        assert!(HarvestTest::new(&ripe(), StateText::parse(TRAY)).matches(&top_shelf));

        let mut once = ripe();
        once.trimmed_late = false;
        assert!(!HarvestTest::new(&once, StateText::parse(TRAY)).matches(&top_shelf));

        let pot = StateText::parse("hempdustry:grow_pot[fertility=0,watered=false]");
        assert!(!HarvestTest::new(&ripe(), pot).matches(&top_shelf));
    }

    #[test]
    fn no_conditions_match_every_harvest() {
        let test = HarvestTest::new(
            &Plant::new(Kind::Sativa, 7, Segment::Lower),
            StateText::parse("minecraft:farmland[moisture=7]"),
        );
        assert!(test.matches(&Value::Null));
        assert!(test.matches(&json!({})));
    }

    #[test]
    fn cuts_take_a_range() {
        let test = HarvestTest::new(&ripe(), StateText::parse(TRAY));
        assert!(test.matches(&json!({"cuts": {"min": 1}})));
        assert!(!test.matches(&json!({"cuts": {"max": 1}})));
    }

    #[test]
    fn the_bed_state_is_compared_value_by_value() {
        let test = HarvestTest::new(&ripe(), StateText::parse(TRAY));
        assert!(test.matches(&json!({"bed": {"blocks": ["hempdustry:grow_pot", "hempdustry:hydro_tray"], "state": {"fed": "true"}}})));
        assert!(!test.matches(&json!({"bed": {"state": {"fed": "false"}}})));
        assert!(test.matches(&json!({"bed": {"state": {"level": {"min": "1", "max": "3"}}}})));
    }

    #[test]
    fn a_bed_tag_is_asked_for_then_answered() {
        let wants_tag = json!({"bed": {"blocks": "#hempdustry:grow_media"}});
        let mut test = HarvestTest::new(&ripe(), StateText::parse(TRAY));
        assert!(
            !test.matches(&wants_tag),
            "unknown tags answer no on the first pass"
        );
        assert_eq!(test.missing(), vec!["hempdustry:grow_media".to_owned()]);
        test.tags.insert("hempdustry:grow_media".to_owned(), true);
        assert!(test.matches(&wants_tag));
        assert!(test.missing().is_empty());
    }
}
