//! The farming rules played on a small world of blocks, with the random's draws scripted.

use std::collections::{HashMap, HashSet, VecDeque};

use super::interact::{self, Effect, Held};
use super::model::{
    Block, Condition, Kind, LightSource, Mildew, Plant, Pos, Pot, Segment, Tray, above,
};
use super::{Config, World, harvest, plant};

/// A world of blocks: air where nothing is set, full light, open sky, no water.
#[derive(Default)]
struct TestWorld {
    blocks: HashMap<Pos, Block>,
    lights: HashMap<Pos, LightSource>,
    roofed: HashSet<Pos>,
    water: HashSet<Pos>,
    sand: HashSet<Pos>,
    brightness: Option<u8>,
    /// The next draws of `next_float`; 0.99 (no luck) once they run out.
    floats: VecDeque<f32>,
    /// The next draws of `next_int`; 1 (no luck) once they run out.
    ints: VecDeque<i32>,
    config: Config,
}

impl TestWorld {
    fn with(blocks: &[(Pos, Block)]) -> Self {
        Self {
            blocks: blocks.iter().copied().collect(),
            ..Self::default()
        }
    }

    fn plant(&self, pos: Pos) -> Plant {
        match self.block(pos) {
            Block::Plant(plant) => plant,
            other => panic!("no plant at {pos:?}: {other:?}"),
        }
    }
}

impl World for TestWorld {
    fn block(&self, pos: Pos) -> Block {
        self.blocks.get(&pos).copied().unwrap_or(Block::Air)
    }
    fn set(&mut self, pos: Pos, block: Block) -> bool {
        self.blocks.insert(pos, block) != Some(block)
    }
    fn raw_brightness(&self, _pos: Pos) -> u8 {
        self.brightness.unwrap_or(15)
    }
    fn light_source(&self, pos: Pos) -> LightSource {
        self.lights
            .get(&pos)
            .copied()
            .unwrap_or(match self.block(pos) {
                Block::Air | Block::Plant(_) => LightSource::NONE,
                _ => LightSource {
                    tier: Condition::Natural,
                    opaque: true,
                },
            })
    }
    fn is_water(&self, pos: Pos) -> bool {
        self.water.contains(&pos)
    }
    fn is_beldia_soil(&self, pos: Pos) -> bool {
        self.sand.contains(&pos)
    }
    fn roofed(&self, pos: Pos) -> bool {
        self.roofed.contains(&pos)
    }
    fn gassed(&self, _pos: Pos) -> bool {
        false
    }
    fn mildew_resistant(&self, _kind: Kind) -> bool {
        false
    }
    fn next_float(&mut self) -> f32 {
        self.floats.pop_front().unwrap_or(0.99)
    }
    fn next_int(&mut self, bound: i32) -> i32 {
        self.ints.pop_front().unwrap_or(1).min(bound - 1)
    }
    fn config(&self) -> Config {
        self.config
    }
}

/// What a click did besides its writes.
#[derive(Default)]
struct TestActor {
    effects: Vec<Effect>,
}

impl TestActor {
    fn drops(&self) -> Vec<(&'static str, u32)> {
        self.effects
            .iter()
            .filter_map(|e| match e {
                Effect::Drop { item, count, .. } => Some((*item, *count)),
                _ => None,
            })
            .collect()
    }
    fn told(&self) -> Vec<&'static str> {
        self.effects
            .iter()
            .filter_map(|e| match e {
                Effect::Tell(key) => Some(*key),
                _ => None,
            })
            .collect()
    }
    fn count(&self, effect: &Effect) -> usize {
        self.effects.iter().filter(|e| *e == effect).count()
    }
}

const ROOT: Pos = [0, 64, 0];
const FLOOR: Pos = [0, 63, 0];
const WET: Block = Block::Farmland { moisture: 7 };

fn lower(kind: Kind, age: u8) -> Block {
    Block::Plant(Plant::new(kind, age, Segment::Lower))
}

fn field(kind: Kind, age: u8) -> TestWorld {
    TestWorld::with(&[(FLOOR, WET), (ROOT, lower(kind, age))])
}

#[test]
fn purple_kush_grows_its_upper_half_at_four() {
    let mut world = field(Kind::Indica, 3);
    world.floats.push_back(0.0); // the growth roll
    plant::random_tick(&mut world, ROOT);
    assert_eq!(world.plant(ROOT).age, 4);
    let upper = world.plant(above(ROOT, 1));
    assert_eq!((upper.segment, upper.age), (Segment::Upper, 4));
}

#[test]
fn lemon_haze_stands_three_tall_at_six() {
    let mut world = field(Kind::Sativa, 5);
    plant::set_age(&mut world, ROOT, 6, 0.0);
    assert_eq!(world.plant(above(ROOT, 1)).segment, Segment::Middle);
    assert_eq!(world.plant(above(ROOT, 2)).segment, Segment::Upper);
}

#[test]
fn a_boxed_in_plant_stops_one_stage_short_and_refuses_bone_meal() {
    let mut world = field(Kind::Indica, 3);
    world.set(above(ROOT, 1), Block::Other);
    plant::set_age(&mut world, ROOT, 4, 0.0);
    assert_eq!(world.plant(ROOT).age, 3);
    assert!(!plant::takes_bone_meal(&world, ROOT, world.plant(ROOT)));
}

#[test]
fn lemon_haze_with_two_blocks_of_headroom_stops_at_five() {
    let mut world = field(Kind::Sativa, 4);
    world.set(above(ROOT, 2), Block::Other);
    plant::set_age(&mut world, ROOT, 7, 0.0);
    assert_eq!(world.plant(ROOT).age, 5);
    assert_eq!(world.plant(above(ROOT, 1)).segment, Segment::Upper);
}

#[test]
fn a_plant_fills_in_once_its_headroom_clears() {
    // Grown to 5 as a two-tall Lemon Haze under a block, which then goes.
    let mut world = field(Kind::Sativa, 7);
    world.set(
        above(ROOT, 1),
        Block::Plant(Plant::new(Kind::Sativa, 7, Segment::Upper)),
    );
    plant::random_tick(&mut world, ROOT);
    assert_eq!(world.plant(above(ROOT, 1)).segment, Segment::Middle);
    assert_eq!(world.plant(above(ROOT, 2)).segment, Segment::Upper);
}

#[test]
fn a_finished_plant_built_over_keeps_its_age() {
    let mut world = field(Kind::Indica, 7);
    world.set(above(ROOT, 1), Block::Other);
    plant::random_tick(&mut world, ROOT);
    assert_eq!(world.plant(ROOT).age, 7);
}

#[test]
fn growth_keeps_the_trims() {
    let mut world = field(Kind::Indica, 3);
    let mut trimmed = world.plant(ROOT);
    trimmed.trimmed_early = true;
    world.set(ROOT, Block::Plant(trimmed));
    plant::set_age(&mut world, ROOT, 4, 0.0);
    assert!(world.plant(ROOT).trimmed_early);
}

#[test]
fn a_dark_plant_does_not_grow() {
    let mut world = field(Kind::Indica, 2);
    world.brightness = Some(8);
    world.floats.push_back(0.0);
    plant::random_tick(&mut world, ROOT);
    assert_eq!(world.plant(ROOT).age, 2);
}

#[test]
fn dry_beldia_does_not_grow_and_watered_beldia_does() {
    let mut world = TestWorld::with(&[(ROOT, lower(Kind::Beldia, 1))]);
    world.sand.insert(FLOOR);
    world.floats.push_back(0.0);
    plant::random_tick(&mut world, ROOT);
    assert_eq!(world.plant(ROOT).age, 1);
    world.water.insert([4, 63, -4]);
    world.floats.push_back(0.0);
    plant::random_tick(&mut world, ROOT);
    assert_eq!(world.plant(ROOT).age, 2);
}

#[test]
fn beldia_stands_on_sand_and_the_others_on_farmland() {
    let beldia = Plant::new(Kind::Beldia, 0, Segment::Lower);
    let kush = Plant::new(Kind::Indica, 0, Segment::Lower);
    assert!(plant::may_stand_on(beldia, Block::Other, true));
    assert!(!plant::may_stand_on(beldia, WET, false));
    assert!(plant::may_stand_on(kush, WET, false));
    assert!(plant::may_stand_on(
        kush,
        Block::Pot(Pot {
            fertility: 0,
            watered: false
        }),
        false
    ));
    assert!(!plant::may_stand_on(kush, Block::Other, true));
}

#[test]
fn an_upper_stands_only_on_its_own_plant() {
    let upper = Plant::new(Kind::Sativa, 6, Segment::Upper);
    let middle = Plant::new(Kind::Sativa, 6, Segment::Middle);
    assert!(plant::may_stand_on(upper, Block::Plant(middle), false));
    assert!(plant::may_stand_on(upper, lower(Kind::Sativa, 6), false));
    assert!(!plant::may_stand_on(upper, Block::Plant(upper), false));
    assert!(!plant::may_stand_on(upper, lower(Kind::Indica, 6), false));
    assert!(!plant::may_stand_on(middle, Block::Plant(middle), false));
}

#[test]
fn a_ripening_plant_drinks_its_pot() {
    let pot = Block::Pot(Pot {
        fertility: 2,
        watered: true,
    });
    let mut world = TestWorld::with(&[(FLOOR, pot), (ROOT, lower(Kind::Indica, 6))]);
    plant::set_age(&mut world, ROOT, 7, 0.0);
    assert_eq!(
        world.block(FLOOR),
        Block::Pot(Pot {
            fertility: 1,
            watered: false
        })
    );
}

#[test]
fn a_lamp_taken_away_while_flowering_stresses_the_plant() {
    let mut world = field(Kind::Indica, 3);
    let mut lit = world.plant(ROOT);
    lit.condition = Condition::Lamp;
    world.set(ROOT, Block::Plant(lit));
    plant::set_age(&mut world, ROOT, 4, 0.0);
    assert_eq!(world.plant(ROOT).condition, Condition::Stressed);
    assert_eq!(world.plant(above(ROOT, 1)).condition, Condition::Stressed);
}

#[test]
fn a_grow_lamp_reaches_the_plants_around_it() {
    let mut world = field(Kind::Indica, 0);
    let lamp = LightSource {
        tier: Condition::GrowLamp,
        opaque: false,
    };
    world.lights.insert([1, 66, 1], lamp);
    plant::set_age(&mut world, ROOT, 1, 0.0);
    assert_eq!(world.plant(ROOT).condition, Condition::GrowLamp);
}

#[test]
fn an_opaque_block_hides_the_lamp_above_it() {
    let mut world = field(Kind::Indica, 0);
    world.set([0, 66, 0], Block::Other);
    world.lights.insert(
        [0, 67, 0],
        LightSource {
            tier: Condition::Lamp,
            opaque: false,
        },
    );
    plant::set_age(&mut world, ROOT, 1, 0.0);
    assert_eq!(world.plant(ROOT).condition, Condition::Natural);
}

#[test]
fn shears_trim_each_window_once() {
    let mut world = field(Kind::Indica, 3);
    let mut actor = TestActor::default();
    assert!(interact::use_item_on(
        &mut world,
        Held::Shears,
        ROOT,
        false,
        &mut actor.effects
    ));
    assert!(world.plant(ROOT).trimmed_early);
    assert_eq!(actor.drops(), [("hempdustry:hemp_leaf", 1)]);
    assert_eq!(actor.count(&Effect::DamageHeld), 1);
    assert!(!interact::use_item_on(
        &mut world,
        Held::Shears,
        ROOT,
        false,
        &mut actor.effects
    ));
    assert_eq!(actor.told(), ["hempdustry.trim.already"]);
}

#[test]
fn shears_on_the_upper_half_trim_the_plant() {
    let mut world = field(Kind::Indica, 4);
    plant::set_age(&mut world, ROOT, 4, 0.0);
    let mut actor = TestActor::default();
    assert!(interact::use_item_on(
        &mut world,
        Held::Shears,
        above(ROOT, 1),
        false,
        &mut actor.effects
    ));
    assert!(world.plant(ROOT).trimmed_late);
}

#[test]
fn a_seedling_is_too_young_to_trim() {
    let mut world = field(Kind::Indica, 1);
    let mut actor = TestActor::default();
    assert!(!interact::use_item_on(
        &mut world,
        Held::Shears,
        ROOT,
        false,
        &mut actor.effects
    ));
    assert_eq!(actor.told(), ["hempdustry.trim.too_young"]);
}

#[test]
fn a_lucky_rub_gives_charas_and_a_leaf() {
    let mut world = field(Kind::Sativa, 7);
    world.ints.push_back(0);
    let mut actor = TestActor::default();
    assert!(interact::use_item_on(
        &mut world,
        Held::Shears,
        ROOT,
        false,
        &mut actor.effects
    ));
    assert!(world.plant(ROOT).rubbed);
    assert_eq!(
        actor.drops(),
        [("hempdustry:hemp_leaf", 1), ("hempdustry:charas", 1)]
    );
}

#[test]
fn trimming_a_mildewed_plant_cures_it_and_throws_the_leaf_away() {
    let mut world = field(Kind::Indica, 3);
    let mut sick = world.plant(ROOT);
    sick.mildew = Mildew::Mildewed;
    world.set(ROOT, Block::Plant(sick));
    let mut actor = TestActor::default();
    assert!(interact::use_item_on(
        &mut world,
        Held::Shears,
        ROOT,
        false,
        &mut actor.effects
    ));
    let plant = world.plant(ROOT);
    assert!(plant.trimmed_early);
    assert_eq!(plant.mildew, Mildew::Clean);
    assert!(actor.drops().is_empty());
}

#[test]
fn a_bottle_on_the_plant_waters_its_pot() {
    let pot = Block::Pot(Pot {
        fertility: 0,
        watered: false,
    });
    let mut world = TestWorld::with(&[(FLOOR, pot), (ROOT, lower(Kind::Indica, 2))]);
    let mut actor = TestActor::default();
    assert!(interact::use_item_on(
        &mut world,
        Held::WaterBottle,
        ROOT,
        false,
        &mut actor.effects
    ));
    assert_eq!(
        world.block(FLOOR),
        Block::Pot(Pot {
            fertility: 0,
            watered: true
        })
    );
    assert_eq!(actor.count(&Effect::EmptyHeld("minecraft:glass_bottle")), 1);
}

#[test]
fn a_full_pot_lets_the_bone_meal_through() {
    let full = Pot {
        fertility: Pot::MAX_FERTILITY,
        watered: false,
    };
    let mut world = TestWorld::with(&[(FLOOR, Block::Pot(full))]);
    let mut actor = TestActor::default();
    assert!(!interact::use_item_on(
        &mut world,
        Held::Fertilizer,
        FLOOR,
        false,
        &mut actor.effects
    ));
    assert_eq!(actor.count(&Effect::ConsumeHeld), 0);
}

#[test]
fn a_bucket_fills_a_tray_but_boils_away_in_the_nether() {
    let empty = Tray {
        level: 0,
        fed: false,
        powered: false,
    };
    let mut world = TestWorld::with(&[(FLOOR, Block::Tray(empty))]);
    let mut actor = TestActor::default();
    assert!(!interact::use_item_on(
        &mut world,
        Held::WaterBucket,
        FLOOR,
        true,
        &mut actor.effects
    ));
    assert!(interact::use_item_on(
        &mut world,
        Held::WaterBucket,
        FLOOR,
        false,
        &mut actor.effects
    ));
    assert_eq!(world.block(FLOOR), Block::Tray(Tray { level: 3, ..empty }));
    assert!(interact::use_item_on(
        &mut world,
        Held::WaterBucket,
        FLOOR,
        false,
        &mut actor.effects
    ));
    assert_eq!(actor.count(&Effect::EmptyHeld("minecraft:bucket")), 1);
}

#[test]
fn bone_meal_grows_a_stage_and_a_failure_still_uses_it() {
    let mut world = field(Kind::Indica, 2);
    world.ints.push_back(0); // the one-in-three failure
    assert!(!plant::bone_meal_succeeds(&mut world));
    assert!(plant::bone_meal_succeeds(&mut world));
    plant::bone_meal(&mut world, ROOT);
    assert_eq!(world.plant(ROOT).age, 3);
}

#[test]
fn mildew_spreads_to_a_neighbour_under_a_roof() {
    let mut world = field(Kind::Indica, 2);
    let mut sick = world.plant(ROOT);
    sick.mildew = Mildew::Mildewed;
    world.set(ROOT, Block::Plant(sick));
    let neighbour = [1, 64, 0];
    world.set(neighbour, lower(Kind::Indica, 2));
    world.roofed.extend([ROOT, neighbour]);
    world.ints.push_back(4); // NEIGHBOURS[4] is (1, 0)
    world.floats.push_back(0.0); // the spread roll
    super::mildew::random_tick(&mut world, ROOT);
    assert_eq!(world.plant(neighbour).mildew, Mildew::Mildewed);
}

#[test]
fn mildew_never_starts_in_a_field() {
    let mut world = field(Kind::Indica, 2);
    world.floats.extend([0.0, 0.0]);
    plant::set_age(&mut world, ROOT, 3, 0.0);
    assert_eq!(world.plant(ROOT).mildew, Mildew::Clean);
}

/// A ripe plant of `kind` on a pot under [`ROOT`], at its full height.
fn grown(kind: Kind) -> TestWorld {
    let ripe = Plant::new(kind, Plant::MAX_AGE, Segment::Lower);
    let mut world = TestWorld::with(&[
        (
            FLOOR,
            Block::Pot(Pot {
                fertility: 0,
                watered: false,
            }),
        ),
        (ROOT, Block::Plant(ripe)),
    ]);
    let segments: &[Segment] = if kind.height() == 3 {
        &[Segment::Middle, Segment::Upper]
    } else {
        &[Segment::Upper]
    };
    for (n, &segment) in (1..).zip(segments) {
        world.set(
            above(ROOT, n),
            Block::Plant(Plant::segment_of(ripe, Plant::MAX_AGE, segment)),
        );
    }
    world
}

#[test]
fn breaking_the_lower_segment_clears_the_rest_without_drops() {
    let world = grown(Kind::Sativa);
    let harvest = harvest::of_break(&world, ROOT).expect("a plant");
    assert_eq!(harvest.lower, ROOT);
    assert_eq!(harvest.clear, vec![above(ROOT, 2), above(ROOT, 1)]);
    assert!(
        !harvest.break_lower,
        "the game breaks the lower segment itself, with its loot"
    );
    assert!(harvest.is_ripe());
}

#[test]
fn breaking_the_top_breaks_the_lower_segment_for_its_loot() {
    for kind in Kind::ALL {
        let world = grown(kind);
        let top = above(ROOT, kind.height() - 1);
        let harvest = harvest::of_break(&world, top).expect("a plant");
        assert_eq!(harvest.lower, ROOT, "{kind:?}");
        assert!(harvest.break_lower, "{kind:?}");
        assert!(
            !harvest.clear.contains(&top),
            "the game removes the broken block: {kind:?}"
        );
        assert_eq!(
            harvest.clear.len(),
            usize::try_from(kind.height() - 2).unwrap_or(0),
            "{kind:?}"
        );
    }
}

#[test]
fn breaking_the_middle_of_lemon_haze_takes_top_and_root() {
    let world = grown(Kind::Sativa);
    let harvest = harvest::of_break(&world, above(ROOT, 1)).expect("a plant");
    assert_eq!(harvest.clear, vec![above(ROOT, 2)]);
    assert!(harvest.break_lower);
}

#[test]
fn a_young_plant_harvests_but_is_not_ripe() {
    let world = field(Kind::Indica, 2);
    let harvest = harvest::of_break(&world, ROOT).expect("a plant");
    assert!(harvest.clear.is_empty());
    assert!(!harvest.is_ripe());
}

#[test]
fn a_stalk_with_no_root_breaks_alone() {
    let mut world = grown(Kind::Indica);
    world.set(ROOT, Block::Air);
    assert_eq!(harvest::of_break(&world, above(ROOT, 1)), None);
}

#[test]
fn another_crop_above_is_not_cleared() {
    let mut world = field(Kind::Indica, 7);
    world.set(
        above(ROOT, 1),
        Block::Plant(Plant::new(Kind::Beldia, 7, Segment::Upper)),
    );
    assert!(
        harvest::of_break(&world, ROOT)
            .expect("a plant")
            .clear
            .is_empty()
    );
}

#[test]
fn breaking_the_bed_takes_the_plant_whole_first() {
    let world = grown(Kind::Sativa);
    let harvest = harvest::of_bed_break(&world, FLOOR).expect("a plant on the bed");
    assert_eq!(harvest.lower, ROOT);
    assert!(harvest.break_lower);
    assert_eq!(harvest.clear, vec![above(ROOT, 2), above(ROOT, 1)]);
}

#[test]
fn an_empty_bed_takes_nothing() {
    let world = TestWorld::with(&[(
        FLOOR,
        Block::Pot(Pot {
            fertility: 0,
            watered: false,
        }),
    )]);
    assert_eq!(harvest::of_bed_break(&world, FLOOR), None);
}
