//! Block states by their ids, for the hooks that are handed nothing else.
//!
//! Horizon's level hooks (a random tick, bone meal, `mayPlaceOn`, `canSurvive`) see a block as a
//! bare [`BlockStateId`] and can only ask for the same block with one property changed. So the
//! table is built in two steps: the default state of every block the rules name, from the first
//! tick's host ([`Defaults::read`]), then every state of those blocks, by walking their
//! properties from the default in the first level hook ([`States::build`]).

use std::collections::{HashMap, HashSet};

use horizon_client::mod_api::{BlockStateId, Host, LevelHost};

use crate::farm::model::{Block, Condition, Kind, Mildew, Plant, Pot, Segment, Tray};

const AIR: [&str; 3] = ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"];
const FARMLAND: &str = "minecraft:farmland";
const WATER: &str = "minecraft:water";
/// The blocks a Beldía seed is tried against before a world's tag is read; each is kept only
/// when `#hempdustry:beldia_soil` holds it.
const SOIL_CANDIDATES: [&str; 3] = [
    "minecraft:sand",
    "minecraft:red_sand",
    "minecraft:suspicious_sand",
];
const FACINGS: [&str; 4] = ["north", "south", "west", "east"];

/// The default state of each block the table needs, read off the host.
#[derive(Clone, Debug)]
pub(crate) struct Defaults {
    crops: Vec<(Kind, BlockStateId)>,
    pot: BlockStateId,
    tray: BlockStateId,
    farmland: BlockStateId,
    air: Vec<BlockStateId>,
    water: BlockStateId,
    soil: Vec<BlockStateId>,
}

impl Defaults {
    /// None until the registries are there (or if our content mod isn't loaded).
    pub(crate) fn read(host: &dyn Host) -> Option<Self> {
        let state = |name: &str| host.default_state(name);
        let crops = Kind::ALL
            .into_iter()
            .map(|kind| Some((kind, state(kind.block())?)))
            .collect::<Option<Vec<_>>>()?;
        let soil = SOIL_CANDIDATES
            .into_iter()
            .filter(|name| {
                host.block_name_in_tag(name, "hempdustry:beldia_soil")
                    .unwrap_or(false)
            })
            .filter_map(state)
            .collect();
        Some(Self {
            crops,
            pot: state(Pot::BLOCK)?,
            tray: state(Tray::BLOCK)?,
            farmland: state(FARMLAND)?,
            air: AIR.into_iter().filter_map(state).collect(),
            water: state(WATER)?,
            soil,
        })
    }
}

/// Every state of the blocks the farming rules read, both ways.
#[derive(Debug, Default)]
pub(crate) struct States {
    plants: HashMap<BlockStateId, Plant>,
    plant_ids: HashMap<Plant, BlockStateId>,
    pots: HashMap<BlockStateId, Pot>,
    trays: HashMap<BlockStateId, Tray>,
    farmland: HashMap<BlockStateId, u8>,
    air: HashSet<BlockStateId>,
    water: HashSet<BlockStateId>,
    soil: HashSet<BlockStateId>,
}

/// Every state reached from `start` by setting each `(key, values)` in turn to each value.
fn walk(
    level: &dyn LevelHost,
    start: BlockStateId,
    props: &[(&str, Vec<String>)],
) -> Vec<(BlockStateId, Vec<String>)> {
    let mut states = vec![(start, Vec::new())];
    for (key, values) in props {
        states = states
            .into_iter()
            .flat_map(|(id, chosen)| {
                values.iter().filter_map(move |value| {
                    let next = level.state_with_property(id, key, value)?;
                    let mut chosen = chosen.clone();
                    chosen.push(value.clone());
                    Some((next, chosen))
                })
            })
            .collect();
    }
    states
}

fn strings<T: ToString>(values: impl IntoIterator<Item = T>) -> Vec<String> {
    values.into_iter().map(|v| v.to_string()).collect()
}

impl States {
    pub(crate) fn build(level: &dyn LevelHost, defaults: &Defaults) -> Self {
        let mut states = Self::default();
        let flags = || strings([false, true]);
        for &(kind, default) in &defaults.crops {
            let segments: Vec<Segment> = if kind.height() == 3 {
                vec![Segment::Lower, Segment::Middle, Segment::Upper]
            } else {
                vec![Segment::Lower, Segment::Upper]
            };
            let props = [
                ("age", strings(0..=Plant::MAX_AGE)),
                (
                    kind.segment_key(),
                    strings(segments.iter().map(|s| s.name())),
                ),
                ("trimmed_early", flags()),
                ("trimmed_late", flags()),
                ("rubbed", flags()),
                (
                    Condition::KEY,
                    strings(Condition::ALL.iter().map(|c| c.name())),
                ),
                (Mildew::KEY, strings(Mildew::ALL.iter().map(|m| m.name()))),
            ];
            for (id, values) in walk(level, default, &props) {
                let get = |key: &str| {
                    props
                        .iter()
                        .position(|(k, _)| *k == key)
                        .and_then(|i| values.get(i).cloned())
                };
                if let Some(plant) = Plant::read(kind, get) {
                    states.plants.insert(id, plant);
                    states.plant_ids.insert(plant, id);
                }
            }
        }
        let pot_props = [
            ("fertility", strings(0..=Pot::MAX_FERTILITY)),
            ("watered", flags()),
        ];
        for (id, v) in walk(level, defaults.pot, &pot_props) {
            if let [fertility, watered] = v.as_slice() {
                let pot = Pot {
                    fertility: fertility.parse().unwrap_or(0),
                    watered: watered == "true",
                };
                states.pots.insert(id, pot);
            }
        }
        let tray_props = [
            ("level", strings(0..=Tray::MAX_LEVEL)),
            ("fed", flags()),
            ("powered", flags()),
            ("facing", strings(FACINGS)),
        ];
        for (id, v) in walk(level, defaults.tray, &tray_props) {
            if let [level, fed, powered, _] = v.as_slice() {
                let tray = Tray {
                    level: level.parse().unwrap_or(0),
                    fed: fed == "true",
                    powered: powered == "true",
                };
                states.trays.insert(id, tray);
            }
        }
        for (id, v) in walk(level, defaults.farmland, &[("moisture", strings(0..=7_u8))]) {
            if let Some(moisture) = v.first().and_then(|m| m.parse().ok()) {
                states.farmland.insert(id, moisture);
            }
        }
        for (id, _) in walk(level, defaults.water, &[("level", strings(0..=15_u8))]) {
            states.water.insert(id);
        }
        states.air.extend(defaults.air.iter().copied());
        states.soil.extend(defaults.soil.iter().copied());
        states
    }

    /// The block a state is, as far as the rules can tell from the id alone.
    #[must_use]
    pub(crate) fn block(&self, id: BlockStateId) -> Block {
        if let Some(&plant) = self.plants.get(&id) {
            Block::Plant(plant)
        } else if let Some(&pot) = self.pots.get(&id) {
            Block::Pot(pot)
        } else if let Some(&tray) = self.trays.get(&id) {
            Block::Tray(tray)
        } else if let Some(&moisture) = self.farmland.get(&id) {
            Block::Farmland { moisture }
        } else if self.air.contains(&id) {
            Block::Air
        } else {
            Block::Other
        }
    }

    #[must_use]
    pub(crate) fn plant(&self, id: BlockStateId) -> Option<Plant> {
        self.plants.get(&id).copied()
    }

    /// The state that is `plant`, to write it.
    #[must_use]
    pub(crate) fn plant_id(&self, plant: Plant) -> Option<BlockStateId> {
        self.plant_ids.get(&plant).copied()
    }

    #[must_use]
    pub(crate) fn is_water(&self, id: BlockStateId) -> bool {
        self.water.contains(&id)
    }

    #[must_use]
    pub(crate) fn is_soil(&self, id: BlockStateId) -> bool {
        self.soil.contains(&id)
    }
}
