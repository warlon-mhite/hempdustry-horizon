//! What a loaded smokeable is called: "Purple Kush Bong III", "Purple Kush Moon Rock", and the
//! contents line "With Hashish" under a spliff or moon rock that carries a pinch of hash
//! (`SmokeContents.packedName`, `loadName`, `hashAdditive`).
//!
//! Only translation keys come out of here; the game's language fills them in, so a strain
//! costs one lang key whatever it is loaded in.

use serde_json::Value;

use super::contents::{Entry, SmokeContents};
use super::data::Catalog;

/// `hempdustry.strain.blend`: a load of two or more strains with no single plant to name.
pub const BLEND: &str = "hempdustry.strain.blend";
/// `hempdustry.spliff.with`: the contents line, its argument the hash's name.
pub const WITH: &str = "hempdustry.spliff.with";

/// The strains the Java mod ships, as `(id, name key, grows on a plant)`. A client of a server
/// never receives the strain files (the Java mod syncs them as a registry, which Horizon
/// doesn't), so it names a load from these; a datapack's own strain has no name there.
const BUILT_IN: [(&str, &str, bool); 9] = [
    ("hempdustry:indica", "hempdustry.strain.indica", true),
    ("hempdustry:sativa", "hempdustry.strain.sativa", true),
    ("hempdustry:beldia", "hempdustry.strain.beldia", true),
    ("hempdustry:hashish", "hempdustry.strain.hashish", false),
    ("hempdustry:charas", "hempdustry.strain.charas", false),
    (
        "hempdustry:filtered_hashish",
        "hempdustry.strain.filtered_hashish",
        false,
    ),
    ("hempdustry:rosin", "hempdustry.strain.rosin", false),
    ("hempdustry:schwag", "hempdustry.strain.schwag", false),
    (
        "hempdustry:scorched_hemp",
        "hempdustry.strain.scorched_hemp",
        false,
    ),
];

/// A strain, as its name needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label<'a> {
    /// The lang key of its name, `hempdustry.strain.indica`.
    pub key: &'a str,
    /// It grew on a plant (`wildFlower().isPresent()`), so it wins a tie and is never the hash.
    pub plant: bool,
}

/// Where strains get their names: the data files when this game has them, else the strains
/// the mod ships.
#[derive(Clone, Copy, Debug)]
pub enum Labels<'a> {
    Data(&'a Catalog),
    BuiltIn,
}

impl<'a> Labels<'a> {
    /// The data's strains, or the shipped ones when this game has loaded none.
    #[must_use]
    pub fn of(catalog: &'a Catalog) -> Self {
        if catalog.strains.is_empty() {
            Self::BuiltIn
        } else {
            Self::Data(catalog)
        }
    }

    /// The label of strain `id` (`hempdustry:indica`), if it is known.
    #[must_use]
    pub fn label(self, id: &str) -> Option<Label<'a>> {
        match self {
            Self::Data(catalog) => catalog.strain(id).map(|strain| Label {
                key: &strain.translation_key,
                plant: strain.is_plant(),
            }),
            Self::BuiltIn => BUILT_IN
                .iter()
                .find(|(built_in, _, _)| *built_in == id)
                .map(|&(_, key, plant)| Label { key, plant }),
        }
    }

    /// `SmokeContents.CODEC` as these labels know the strains: an entry whose strain has no
    /// label is dropped.
    #[must_use]
    pub fn contents(self, value: &Value) -> SmokeContents {
        SmokeContents::parse_with(value, |id| self.label(id).is_some())
    }
}

/// The name of what is loaded: one strain's key, or the blend's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load<'a> {
    Strain(&'a str),
    Blend,
}

impl<'a> Load<'a> {
    #[must_use]
    pub fn key(self) -> &'a str {
        match self {
            Self::Strain(key) => key,
            Self::Blend => BLEND,
        }
    }
}

/// A loaded item's name: `format`'s `%s` filled with the load's name, then the level's Roman
/// numeral (vanilla's `enchantment.level.<n>`) when there is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedName<'a> {
    pub format: &'static str,
    pub load: Load<'a>,
    /// The primary entry's count, from two on (vanilla writes no "I" either).
    pub level: Option<i32>,
}

/// How an item names its load: as a device ("%s Bong", with the level) or as the moon rock
/// ("%s Moon Rock", always the same thing, so no level).
fn format_of(item: &str) -> Option<(&'static str, bool)> {
    let path = item.strip_prefix("hempdustry:")?;
    Some(match path {
        "spliff" => ("item.hempdustry.spliff.packed", true),
        "wooden_pipe" => ("item.hempdustry.wooden_pipe.packed", true),
        "vaporizer" => ("item.hempdustry.vaporizer.packed", true),
        "moon_rock" => ("item.hempdustry.moon_rock.of", false),
        // The packed format is the device's, so every glass names itself "Bong": the colour
        // is on the model, and the name has room for one thing.
        _ if super::Device::of_item(item) == Some(super::Device::Bong) => {
            ("item.hempdustry.bong.packed", true)
        }
        _ => return None,
    })
}

/// The dominant entry by count; a tie goes to the plant. Entries without a label are skipped.
fn primary<'c, 'a>(
    contents: &'c SmokeContents,
    labels: Labels<'a>,
) -> Option<(&'c Entry, Label<'a>)> {
    let mut best: Option<(&Entry, Label<'a>)> = None;
    for entry in &contents.entries {
        let Some(label) = labels.label(&entry.strain) else {
            continue;
        };
        let better = best.is_none_or(|(b, bl)| {
            entry.count > b.count || (entry.count == b.count && label.plant && !bl.plant)
        });
        if better {
            best = Some((entry, label));
        }
    }
    best
}

/// `hashAdditive`: in a load of exactly two, the strainless entry riding with a plant. A load
/// whose primary is itself strainless has no plant to name, so it stays a blend.
#[must_use]
pub fn hash_additive<'a>(contents: &SmokeContents, labels: Labels<'a>) -> Option<Label<'a>> {
    if contents.entries.len() != 2 {
        return None;
    }
    let (primary_entry, primary_label) = primary(contents, labels)?;
    if !primary_label.plant {
        return None;
    }
    contents
        .entries
        .iter()
        .filter(|entry| !std::ptr::eq(*entry, primary_entry))
        .filter_map(|entry| labels.label(&entry.strain))
        .find(|label| !label.plant)
}

/// `loadName`: "Mixed" for a blend that isn't a plant with its hash, else the primary strain.
#[must_use]
pub fn load_name<'a>(contents: &SmokeContents, labels: Labels<'a>) -> Option<Load<'a>> {
    if contents.entries.len() > 1 && hash_additive(contents, labels).is_none() {
        return Some(Load::Blend);
    }
    primary(contents, labels).map(|(_, label)| Load::Strain(label.key))
}

/// The name `item` takes with `contents` loaded, or None when it keeps its own (not a
/// smokeable, or nothing loaded).
#[must_use]
pub fn packed_name<'a>(
    item: &str,
    contents: &SmokeContents,
    labels: Labels<'a>,
) -> Option<PackedName<'a>> {
    let (format, levelled) = format_of(item)?;
    if contents.is_empty() {
        return None;
    }
    let load = load_name(contents, labels)?;
    let level = primary(contents, labels)
        .map(|(entry, _)| entry.count)
        .filter(|&count| levelled && count > 1);
    Some(PackedName {
        format,
        load,
        level,
    })
}

/// The contents line's argument: the hash in a spliff or a moon rock ("With Hashish"). As in
/// the Java mod, the devices' tooltips have no contents line.
#[must_use]
pub fn with_line<'a>(item: &str, contents: &SmokeContents, labels: Labels<'a>) -> Option<&'a str> {
    if !matches!(item, "hempdustry:spliff" | "hempdustry:moon_rock") {
        return None;
    }
    hash_additive(contents, labels).map(|label| label.key)
}
