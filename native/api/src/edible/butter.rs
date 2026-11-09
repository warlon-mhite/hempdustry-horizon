//! The `hempdustry:cannabutter` ingredient kind: cannabutter that makes one exact dosed result.
//!
//! The Java mod bakes with two recipe types of its own (`hempdustry:infused_shaped` and
//! `_shapeless`) whose `assemble` reads the butter in the grid and writes its tier and quality
//! onto the result (`Infusion.transfer`). Horizon crafts no recipe type of a mod's, and its
//! crafting hook doesn't see the grid. What it does run is a mod's own *ingredient* kind, asked to
//! match each concrete stack. So the build (`tools/build.py`) writes each infused recipe out as
//! plain vanilla recipes, one per result it can make (a potency and a quality, with that dose on
//! its result), each taking this kind in the butter's place. Exactly one of a recipe's variants
//! accepts any given butter, so the grid shows the result the Java recipe would build.

use serde_json::{Map, Value};

use super::{MAX_TIER, apply_offset, tier_from_strength};
use crate::machine::infuser::Quality;

/// The kind's id, as the generated recipes name it in `type`.
pub const KIND: &str = "hempdustry:cannabutter";

/// The item the kind matches.
pub const ITEM: &str = "hempdustry:cannabutter";

/// One generated recipe's butter: the potency its result carries, the edible's own step on the
/// ladder (the Java recipe's `offset`) and the quality copied across.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Butter {
    potency: i32,
    offset: i32,
    quality: Quality,
}

impl Butter {
    /// Reads `{"potency": 1..=4, "offset": n, "quality": "rough"}`, the fields the build writes
    /// beside `type`. None for anything else: the kind then matches nothing, and the recipe
    /// fails to load as one with an unknown ingredient does.
    #[must_use]
    pub fn from_json(data: &Map<String, Value>) -> Option<Self> {
        let int = |key| {
            data.get(key)
                .and_then(Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
        };
        let potency = int("potency").filter(|p| (1..=MAX_TIER).contains(p))?;
        let offset = int("offset")?;
        let quality = Quality::from_name(data.get("quality")?.as_str()?)?;
        Some(Self {
            potency,
            offset,
            quality,
        })
    }

    /// Whether a butter of this `strength` and `quality` makes this variant's result. A butter
    /// without them reads as `Infusion.transfer` reads it: strength 1, Rough.
    #[must_use]
    pub fn accepts(self, strength: Option<i32>, quality: Option<Quality>) -> bool {
        let tier = tier_from_strength(strength.unwrap_or(1));
        apply_offset(tier, self.offset) == self.potency
            && quality.unwrap_or(Quality::Rough) == self.quality
    }
}
