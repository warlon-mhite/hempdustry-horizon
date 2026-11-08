use super::*;

const UNWASHED: &str = "hempdustry:decarboxylated_hemp";
const WASHED: &str = "hempdustry:washed_decarboxylated_hemp";
const SCORCHED: &str = "hempdustry:scorched_hemp";
const BUTTER: &str = "hempdustry:cannabutter";

/// The shipped recipe. A butter's strength and grade ride in its `damage` here, as the test has
/// no host to stamp components with.
struct Shipped {
    spout: Poured,
    poured: Vec<ItemStack>,
}

impl Shipped {
    const fn new() -> Self {
        Self {
            spout: Poured::Moved,
            poured: Vec::new(),
        }
    }
}

fn stamp(strength: i32, quality: Quality) -> u32 {
    u32::try_from(strength).unwrap_or(0) * 10 + quality as u32
}

impl Game for Shipped {
    fn is_milk(&self, stack: &ItemStack) -> bool {
        &*stack.item == "minecraft:milk_bucket"
    }
    fn hemp(&self, stack: &ItemStack) -> Option<Hemp> {
        match &*stack.item {
            SCORCHED => Some(Hemp::Scorched),
            UNWASHED => Some(Hemp::Unwashed),
            WASHED => Some(Hemp::Washed),
            _ => None,
        }
    }
    fn butter(&self, strength: i32, quality: Quality) -> Option<ItemStack> {
        let mut butter = ItemStack::new(BUTTER, 1);
        butter.damage = stamp(strength, quality);
        Some(butter)
    }
    fn shows(&self, shown: &ItemStack, strength: i32, quality: Quality) -> bool {
        &*shown.item == BUTTER && shown.damage == stamp(strength, quality)
    }
    fn pour(&mut self, stack: &ItemStack) -> Poured {
        if self.spout == Poured::Moved {
            self.poured.push(stack.clone());
        }
        self.spout
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "a slot's value, as the tub holds it"
)]
fn stack(item: &str, count: u32) -> Option<ItemStack> {
    Some(ItemStack::new(item, count))
}

/// The shipped recipe's three hemps, 64 to a stack.
#[expect(clippy::unnecessary_wraps, reason = "the shape `Tub::spill` takes")]
fn representative(hemp: Hemp) -> Option<(ItemStack, u32)> {
    let item = match hemp {
        Hemp::Scorched => SCORCHED,
        Hemp::Unwashed => UNWASHED,
        Hemp::Washed => WASHED,
    };
    Some((ItemStack::new(item, 1), 64))
}

fn count(slots: &[Option<ItemStack>], slot: usize) -> u32 {
    slots[slot].as_ref().map_or(0, |s| s.count)
}

/// A tub with milk, and `a` and `b` in the hemp slots.
fn tub_with(a: Option<ItemStack>, b: Option<ItemStack>) -> (Tub, [Option<ItemStack>; SLOT_COUNT]) {
    let mut tub = Tub::default();
    assert!(tub.pour_milk());
    (tub, [None, a, b, None, None])
}

fn run(tub: &mut Tub, slots: &mut [Option<ItemStack>], game: &mut Shipped, ticks: i32) {
    for _ in 0..ticks {
        let _ = tub.tick(slots, true, Timings::default(), game);
    }
}

fn shown(slots: &[Option<ItemStack>]) -> Option<u32> {
    slots[OUTPUT_SLOT].as_ref().map(|s| s.damage)
}

#[test]
fn both_anchors_land_on_standard_and_perfect_is_a_gate() {
    // An all-unwashed batch at a full simmer, an all-washed one at the minimum.
    assert_eq!(Quality::of(100, 0), Quality::Standard);
    assert_eq!(Quality::of(0, 100), Quality::Standard);
    assert_eq!(Quality::of(99, 0), Quality::Rough);
    // One short on either dial is Clean, never Perfect.
    assert_eq!(Quality::of(100, 99), Quality::Clean);
    assert_eq!(Quality::of(99, 100), Quality::Clean);
    assert_eq!(Quality::of(100, 100), Quality::Perfect);
    // Clean needs both: 60 + 100, 100 + 60.
    assert_eq!(Quality::of(60, 100), Quality::Clean);
    assert_eq!(Quality::of(100, 59), Quality::Standard);
    // Nothing but scorched hemp stays Rough however long it simmers.
    assert_eq!(Quality::of(100, -100), Quality::Rough);
    assert_eq!(Quality::of(100, -150), Quality::Rough);
}

#[test]
fn time_needed_is_the_grade_solved_for_time() {
    assert_eq!(Quality::Standard.time_needed(50), Some(50));
    assert_eq!(Quality::Clean.time_needed(50), None);
    assert_eq!(Quality::Clean.time_needed(100), Some(60));
    assert_eq!(Quality::Standard.time_needed(100), Some(0));
    assert_eq!(Quality::Perfect.time_needed(99), None);
    assert_eq!(Quality::Perfect.time_needed(100), Some(100));
    assert_eq!(Quality::Rough.time_needed(-100), Some(0));
    assert_eq!(Quality::Standard.time_needed(-100), None);
}

#[test]
fn scorched_hemp_counts_a_quarter_rounded_up_and_pulls_purity_down() {
    let all_scorched = Batch {
        scorched: 24,
        ..Batch::default()
    };
    assert_eq!(all_scorched.strength(), 6);
    assert_eq!(all_scorched.purity(), Some(-100));
    let one = Batch {
        scorched: 1,
        ..Batch::default()
    };
    assert_eq!(one.strength(), 1, "never an empty butter");
    let half = Batch {
        washed: 12,
        scorched: 12,
        ..Batch::default()
    };
    assert_eq!((half.strength(), half.purity()), (15, Some(0)));
    // Floored, both ways: one unwashed among 99 washed reads 99; -33.3 reads -34.
    let nearly = Batch {
        washed: 99,
        unwashed: 1,
        ..Batch::default()
    };
    assert_eq!(nearly.purity(), Some(99));
    let low = Batch {
        unwashed: 2,
        scorched: 1,
        ..Batch::default()
    };
    assert_eq!(low.purity(), Some(-34));
    assert_eq!(Batch::default().purity(), None);
}

#[test]
fn a_washed_batch_dissolves_one_per_250_ticks_and_previews_from_the_minimum() {
    let mut game = Shipped::new();
    let (mut tub, mut slots) = tub_with(stack(WASHED, 30), None);
    run(&mut tub, &mut slots, &mut game, 249);
    assert_eq!(tub.batch.items(), 0);
    run(&mut tub, &mut slots, &mut game, 1);
    assert_eq!(tub.batch.washed, 1);
    run(&mut tub, &mut slots, &mut game, 5749);
    assert_eq!(shown(&slots), None, "nothing to take before 6000");
    run(&mut tub, &mut slots, &mut game, 1);
    // The cap: 24 dissolved, 6 left in the slot, however much room there was.
    assert_eq!((tub.batch.washed, count(&slots, 1)), (24, 6));
    assert_eq!(shown(&slots), Some(stamp(24, Quality::Standard)));
    run(&mut tub, &mut slots, &mut game, 1000);
    assert_eq!(tub.batch.washed, 24, "the batch is locked after the window");
}

#[test]
fn hemp_loaded_halfway_through_the_window_cannot_catch_up() {
    let mut game = Shipped::new();
    let (mut tub, mut slots) = tub_with(stack(UNWASHED, 1), None);
    run(&mut tub, &mut slots, &mut game, 3000);
    assert_eq!(tub.batch.unwashed, 1);
    slots[2] = stack(UNWASHED, 64);
    run(&mut tub, &mut slots, &mut game, 3000);
    assert_eq!(tub.batch.unwashed, 13, "12 more turns, one each");
}

#[test]
fn absorption_spends_scorched_then_unwashed_then_washed() {
    let mut game = Shipped::new();
    let (mut tub, mut slots) = tub_with(stack(WASHED, 2), stack(UNWASHED, 1));
    run(&mut tub, &mut slots, &mut game, 250);
    assert_eq!(tub.batch.unwashed, 1);
    run(&mut tub, &mut slots, &mut game, 250);
    assert_eq!((tub.batch.unwashed, tub.batch.washed), (1, 1));
    let (mut tub, mut slots) = tub_with(stack(UNWASHED, 1), stack(SCORCHED, 1));
    run(&mut tub, &mut slots, &mut game, 250);
    assert_eq!(tub.batch.scorched, 1);
}

#[test]
fn no_heat_no_milk_or_no_hemp_pauses_the_batch() {
    let mut game = Shipped::new();
    let (mut tub, mut slots) = tub_with(stack(UNWASHED, 4), None);
    run(&mut tub, &mut slots, &mut game, 300);
    for _ in 0..100 {
        let _ = tub.tick(&mut slots, false, Timings::default(), &mut game);
    }
    assert_eq!(tub.progress, 300, "heat lost: a pause, not a reset");
    let mut dry = Tub::default();
    let mut slots = [None, stack(UNWASHED, 4), None, None, None];
    run(&mut dry, &mut slots, &mut game, 10);
    assert_eq!(dry.progress, 0, "no milk");
    let (mut empty, mut slots) = tub_with(None, None);
    run(&mut empty, &mut slots, &mut game, 10);
    assert_eq!(empty.progress, 0, "no hemp");
    assert!(!empty.state(&slots, Timings::default(), &game).infusing);
    assert!(empty.state(&slots, Timings::default(), &game).filled);
}

#[test]
fn a_full_tub_refuses_more_milk() {
    let mut tub = Tub::default();
    assert!(tub.pour_milk());
    assert!(!tub.pour_milk());
}

#[test]
fn a_perfect_batch_is_poured_at_the_full_simmer_and_the_tub_empties() {
    let mut game = Shipped::new();
    let (mut tub, mut slots) = tub_with(stack(WASHED, 24), None);
    run(&mut tub, &mut slots, &mut game, 17_999);
    assert_eq!(shown(&slots), Some(stamp(24, Quality::Clean)));
    assert!(game.poured.is_empty());
    assert_eq!(tub.comparator(Timings::default()), 13, "never 14");
    run(&mut tub, &mut slots, &mut game, 1);
    assert_eq!(game.poured.len(), 1);
    assert_eq!(game.poured[0].damage, stamp(24, Quality::Perfect));
    assert_eq!(slots[OUTPUT_SLOT], None);
    assert!(!tub.have_milk);
    assert_eq!((tub.progress, tub.batch.items()), (0, 0));
}

#[test]
fn a_half_washed_batch_is_done_at_two_thirds_and_stops_simmering() {
    let mut game = Shipped::new();
    game.spout = Poured::Nothing;
    let (mut tub, mut slots) = tub_with(stack(WASHED, 12), stack(UNWASHED, 12));
    run(&mut tub, &mut slots, &mut game, 11_999);
    assert!(!tub.is_at_best_quality(Timings::default()));
    run(&mut tub, &mut slots, &mut game, 1);
    assert!(tub.is_at_best_quality(Timings::default()));
    assert_eq!(tub.comparator(Timings::default()), 15);
    run(&mut tub, &mut slots, &mut game, 500);
    assert_eq!(
        tub.progress, 12_000,
        "nothing left to gain: the clock stops"
    );
    assert!(!tub.state(&slots, Timings::default(), &game).infusing);
    assert_eq!(shown(&slots), Some(stamp(24, Quality::Standard)));
}

#[test]
fn the_spout_waits_eight_ticks_on_a_full_chest_and_twenty_on_nothing() {
    let mut game = Shipped::new();
    game.spout = Poured::Full;
    let (mut tub, mut slots) = tub_with(stack(WASHED, 12), stack(UNWASHED, 12));
    run(&mut tub, &mut slots, &mut game, 12_000);
    assert_eq!(tub.push_cooldown, PUSH_COOLDOWN);
    game.spout = Poured::Nothing;
    run(&mut tub, &mut slots, &mut game, 9);
    assert_eq!(tub.push_cooldown, PUSH_IDLE_COOLDOWN);
    game.spout = Poured::Moved;
    run(&mut tub, &mut slots, &mut game, 20);
    assert!(game.poured.is_empty());
    run(&mut tub, &mut slots, &mut game, 1);
    assert_eq!(game.poured.len(), 1);
}

#[test]
fn a_preview_taken_by_any_route_closes_the_batch_at_the_next_tick() {
    let mut game = Shipped::new();
    let (mut tub, mut slots) = tub_with(stack(UNWASHED, 24), None);
    run(&mut tub, &mut slots, &mut game, 6000);
    assert!(tub.preview_shown);
    // A hopper, a click or a shift-click took it.
    slots[OUTPUT_SLOT] = None;
    let ticked = tub.tick(&mut slots, true, Timings::default(), &mut game);
    assert!(ticked.changed);
    assert!(!tub.have_milk);
    assert_eq!(shown(&slots), None, "no second butter from the same batch");
    assert!(tub.pour_milk(), "the tub takes milk again");
}

#[test]
fn a_reload_that_lowers_the_minimum_does_not_close_a_batch_never_shown() {
    let mut game = Shipped::new();
    let (mut tub, mut slots) = tub_with(stack(UNWASHED, 24), None);
    run(&mut tub, &mut slots, &mut game, 3000);
    let lower = Timings {
        min: 2000,
        full: 18_000,
    };
    let _ = tub.tick(&mut slots, true, lower, &mut game);
    assert!(tub.have_milk);
    assert!(shown(&slots).is_some(), "ready now: the preview appears");
}

#[test]
fn breaking_the_tub_returns_the_batch_never_the_preview() {
    let mut game = Shipped::new();
    game.spout = Poured::Nothing;
    let (mut tub, mut slots) = tub_with(stack(UNWASHED, 30), stack(SCORCHED, 2));
    run(&mut tub, &mut slots, &mut game, 6000);
    assert!(slots[OUTPUT_SLOT].is_some());
    let spilled = tub.spill(&mut slots, &representative);
    assert_eq!(slots[OUTPUT_SLOT], None);
    let got: Vec<(&str, u32)> = spilled.iter().map(|s| (&*s.item, s.count)).collect();
    assert_eq!(got, vec![(SCORCHED, 2), (UNWASHED, 22)]);
    assert!(
        tub.spill(&mut slots, &representative).is_empty(),
        "only once"
    );
}

#[test]
fn an_old_worlds_retired_slots_are_handed_back() {
    let mut game = Shipped::new();
    let mut tub = Tub::default();
    let mut slots = [
        stack("minecraft:milk_bucket", 1),
        None,
        None,
        None,
        stack("minecraft:bucket", 2),
    ];
    let ticked = tub.tick(&mut slots, false, Timings::default(), &mut game);
    let got: Vec<&str> = ticked.ejected.iter().map(|s| &*s.item).collect();
    assert_eq!(got, vec!["minecraft:milk_bucket", "minecraft:bucket"]);
    assert!(ticked.changed);
    assert!(slots.iter().all(Option::is_none));
}

#[test]
fn only_the_hemp_slots_take_and_a_shift_click_sends_hemp_there() {
    let game = Shipped::new();
    let hemp = ItemStack::new(SCORCHED, 1);
    let milk = ItemStack::new("minecraft:milk_bucket", 1);
    assert!(accepts(1, &hemp, &game));
    assert!(accepts(2, &hemp, &game));
    for slot in [0, 3, 4] {
        assert!(!accepts(slot, &hemp, &game));
    }
    assert!(!accepts(1, &milk, &game));
    assert_eq!(quick_move_slots(&hemp, &game), Some(1..3));
    assert_eq!(quick_move_slots(&milk, &game), None);
}

#[test]
fn the_comparator_reads_progress_not_fullness() {
    let tub = Tub::default();
    assert_eq!(tub.comparator(Timings::default()), 0);
    let early = Tub {
        progress: 1,
        have_milk: true,
        batch: Batch {
            unwashed: 1,
            ..Batch::default()
        },
        ..Tub::default()
    };
    assert_eq!(early.comparator(Timings::default()), 1);
}

#[test]
fn the_data_slots_are_the_java_order() {
    let tub = Tub {
        progress: 7,
        heated: true,
        have_milk: true,
        batch: Batch {
            washed: 3,
            unwashed: 1,
            ..Batch::default()
        },
        ..Tub::default()
    };
    assert_eq!(tub.data(Timings::default()), [7, 1, 75, 6000, 18_000, 1]);
    assert_eq!(
        Tub::default().data(Timings::default())[DATA_PURITY],
        NO_BATCH
    );
}

#[test]
fn the_bar_measures_the_job_and_its_marks_move_with_purity() {
    let half = Gauge::read(&[6000, 1, 50, 6000, 18_000, 1]);
    assert_eq!(half.finish(), 12_000);
    assert!((half.bar() - 0.5).abs() < f32::EPSILON);
    assert!((half.minimum_mark() - 0.5).abs() < f32::EPSILON);
    // Standard is this batch's last grade: its mark is the bar's end.
    assert_eq!(half.next_grade_mark(), Some(1.0));
    let spotless = Gauge::read(&[6000, 1, 100, 6000, 18_000, 1]);
    assert_eq!(spotless.finish(), 18_000);
    // Standard already; Clean at time 60: (6000 + 0.6 * 12000) / 18000.
    let clean = spotless.next_grade_mark().unwrap_or_default();
    assert!((clean - 13_200.0 / 18_000.0).abs() < 1e-6);
    let none = Gauge::read(&[0, 0, NO_BATCH, 6000, 18_000, 0]);
    assert_eq!(none.finish(), 18_000);
    assert_eq!(none.next_grade_mark(), None);
    let done = Gauge::read(&[12_000, 1, 50, 6000, 18_000, 1]);
    assert_eq!(done.next_grade_mark(), None, "nothing left to wait for");
}
