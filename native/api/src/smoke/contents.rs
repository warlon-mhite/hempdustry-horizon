//! `SmokeContents`: what is loaded in a spliff, pipe, bong or vaporizer, the payload of the
//! `hempdustry:smoke_contents` component. The value is the Java codec's own shape,
//! `{entries: [{strain, count}]}` (and the bare list it still reads), so a stack moves between
//! the two games unchanged.

use serde_json::{Value, json};

use super::data::{Catalog, Category, Effect, Strain, namespaced};
use super::policy::Policy;

/// One strain's share of the load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `hempdustry:indica`.
    pub strain: String,
    pub count: i32,
}

/// The load. Empty means an unpacked device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SmokeContents {
    pub entries: Vec<Entry>,
}

impl SmokeContents {
    /// A single-strain load.
    #[must_use]
    pub fn of(strain: &str, count: i32) -> Self {
        Self {
            entries: vec![Entry {
                strain: namespaced(strain),
                count,
            }],
        }
    }

    /// `SmokeContents.CODEC`: the record form or the old bare list. An entry naming a strain
    /// `catalog` doesn't define is dropped, as the Java codec drops it, so the device reads as
    /// unpacked rather than lost.
    #[must_use]
    pub fn parse(value: &Value, catalog: &Catalog) -> Self {
        let list = match value {
            Value::Array(list) => list.as_slice(),
            Value::Object(_) => value["entries"].as_array().map_or(&[][..], Vec::as_slice),
            _ => &[],
        };
        let entries = list
            .iter()
            .filter_map(|entry| {
                let strain = namespaced(entry["strain"].as_str()?);
                let count = i32::try_from(entry["count"].as_i64()?).ok()?;
                catalog.strain(&strain)?;
                Some(Entry { strain, count })
            })
            .collect();
        Self { entries }
    }

    /// The record form, as the Java codec writes it.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "entries": self.entries.iter()
                .map(|e| json!({"strain": e.strain, "count": e.count}))
                .collect::<Vec<_>>()
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total buds loaded: the dose, which drives the green-out odds.
    #[must_use]
    pub fn dose(&self) -> i32 {
        self.entries.iter().map(|e| e.count).sum()
    }

    /// The dominant entry by count; a tie goes to the plant.
    fn primary_entry<'a>(&'a self, catalog: &'a Catalog) -> Option<(&'a Entry, &'a Strain)> {
        let mut best: Option<(&Entry, &Strain)> = None;
        for entry in &self.entries {
            let Some(strain) = catalog.strain(&entry.strain) else {
                continue;
            };
            let better = best.is_none_or(|(b, bs)| {
                entry.count > b.count
                    || (entry.count == b.count && strain.is_plant() && !bs.is_plant())
            });
            if better {
                best = Some((entry, strain));
            }
        }
        best
    }

    /// The dominant strain, which decides the green-out and cough factors.
    #[must_use]
    pub fn primary_strain<'a>(&'a self, catalog: &'a Catalog) -> Option<&'a Strain> {
        self.primary_entry(catalog).map(|(_, strain)| strain)
    }

    /// `buffBonus`: one when a bud is loaded with a concentrate (a strain with no flower and a
    /// beneficial effect), the only way past the buff cap.
    #[must_use]
    pub fn buff_bonus(&self, catalog: &Catalog, category: &dyn Fn(&str) -> Category) -> i32 {
        let mut bud = false;
        let mut concentrate = false;
        for strain in self
            .entries
            .iter()
            .filter_map(|e| catalog.strain(&e.strain))
        {
            if strain.is_plant() {
                bud = true;
            } else if strain
                .effects
                .iter()
                .any(|e| category(&e.effect) == Category::Beneficial)
            {
                concentrate = true;
            }
        }
        i32::from(bud && concentrate)
    }

    /// `SmokeContents.effects(durationTicks, onExhale, random)`: every effect one hit applies,
    /// before the policy's filter. A bud past the buff cap buys half the device's duration
    /// each; a bud loaded with a concentrate goes one level past the cap.
    pub fn effects(
        &self,
        catalog: &Catalog,
        policy: &Policy,
        duration_ticks: i32,
        on_exhale: bool,
        category: &dyn Fn(&str) -> Category,
        next_float: &mut dyn FnMut() -> f32,
    ) -> Vec<Effect> {
        let load = self.buff_bonus(catalog, category);
        let mut out = Vec::new();
        for entry in &self.entries {
            let Some(strain) = catalog.strain(&entry.strain) else {
                continue;
            };
            let bonus = if strain.is_plant() { load } else { 0 };
            let past = (entry.count - policy.max_buff_level(bonus)).max(0);
            let duration = duration_ticks + duration_ticks * past / 2;
            for effect in strain.effects(
                entry.count,
                duration,
                on_exhale,
                bonus,
                category,
                next_float,
            ) {
                let amplifier = policy.amplifier(category(&effect.id), effect.amplifier, bonus);
                out.push(Effect {
                    amplifier,
                    ..effect
                });
            }
        }
        out
    }
}
