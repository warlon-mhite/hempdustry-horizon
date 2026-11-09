//! Packing by hand: an empty pipe, bong or vaporizer used with buds, hashish, rosin or a moon
//! rock in the other hand takes one item per use into its bowl, up to the bowl's `max_dose`.
//!
//! The Java mod packs in the crafting grid (`PackingRecipe`), which Horizon's crafting hook can't
//! read yet. A load packed here is exactly one the grid makes: the same `smoke_contents`, the
//! bowl's full `charges`, and the device's own damage and enchantments, since the device stack is
//! kept and only its two components change. The rules are the recipe's, one item at a time:
//!
//! - One item is worth its strain's `dose_per_item` (1 for buds and hash, 3 for rosin).
//! - A bowl holds one strain; a second strain is refused (no mixing, as in the grid).
//! - A moon rock is a whole bowl on its own: it goes into an empty device only, and nothing goes
//!   in after it.
//! - The cap is on the largest entry: past `max_dose` nothing more goes in.
//! - Adding to a bowl stops once it has been hit: a top-up would refill charges already smoked.

use super::contents::{Entry, SmokeContents};
use super::data::{Catalog, Strain};
use super::device::Device;

/// `ModItems.MOON_ROCK`.
pub const MOON_ROCK: &str = "hempdustry:moon_rock";

/// What the other hand holds, as a load.
#[derive(Clone, Debug, PartialEq)]
pub enum Load<'a> {
    /// One item of a strain's buds (or hash, rosin, schwag: whatever packs as a strain).
    Buds(&'a Strain),
    /// A moon rock, carrying its own load.
    MoonRock(SmokeContents),
}

impl<'a> Load<'a> {
    /// The load `item` is, with `moon_rock` the moon rock's own contents when it is one.
    #[must_use]
    pub fn of(
        catalog: &'a Catalog,
        item: &str,
        moon_rock: impl FnOnce() -> SmokeContents,
    ) -> Option<Self> {
        if item == MOON_ROCK {
            let load = moon_rock();
            return (!load.is_empty()).then_some(Self::MoonRock(load));
        }
        catalog.from_buds(item).map(Self::Buds)
    }
}

/// A device's bowl after one more item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packed {
    pub contents: SmokeContents,
    /// The bowl's hits, all of them: nothing has been smoked yet.
    pub charges: i32,
}

/// One use of `device` holding `contents` with `charges` left, with `load` in the other hand:
/// the new bowl, or `None` when nothing goes in (the click then smokes, as without a load).
#[must_use]
pub fn pack(
    catalog: &Catalog,
    device: Device,
    contents: &SmokeContents,
    charges: i32,
    load: &Load<'_>,
) -> Option<Packed> {
    let bowl = catalog.bowl(device.key())?;
    let next = if contents.is_empty() {
        match load {
            Load::Buds(strain) => SmokeContents::of(&strain.id, strain.dose_per_item),
            Load::MoonRock(rock) => rock.clone(),
        }
    } else {
        let Load::Buds(strain) = load else {
            return None;
        };
        let [only] = contents.entries.as_slice() else {
            return None;
        };
        if only.strain != strain.id || charges != bowl.hits {
            return None;
        }
        SmokeContents {
            entries: vec![Entry {
                strain: only.strain.clone(),
                count: only.count.checked_add(strain.dose_per_item)?,
            }],
        }
    };
    let level = next.entries.iter().map(|e| e.count).max().unwrap_or(0);
    (1..=bowl.max_dose).contains(&level).then_some(Packed {
        contents: next,
        charges: bowl.hits,
    })
}
