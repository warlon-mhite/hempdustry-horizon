use std::collections::{HashSet, VecDeque};

use super::*;

/// A flat world at y 64: hemp columns of a given height, roofs, creepers by plants, unloaded
/// columns.
#[derive(Default)]
struct Plot {
    /// (x, z) → plant height above the ground at y 64 (its root at 65).
    hemp: Vec<([i32; 2], i32)>,
    roofed: HashSet<[i32; 2]>,
    taken: HashSet<Pos>,
    unloaded: HashSet<[i32; 2]>,
}

impl Plot {
    fn plant(mut self, x: i32, z: i32, height: i32) -> Self {
        self.hemp.push(([x, z], height));
        self
    }
    fn height(&self, x: i32, z: i32) -> i32 {
        self.hemp
            .iter()
            .find(|(at, _)| *at == [x, z])
            .map_or(0, |(_, h)| *h)
    }
}

impl Field for Plot {
    fn loaded(&self, x: i32, z: i32) -> bool {
        !self.unloaded.contains(&[x, z])
    }
    fn top(&self, x: i32, z: i32) -> i32 {
        if self.roofed.contains(&[x, z]) {
            return 70;
        }
        64 + self.height(x, z)
    }
    fn is_hemp(&self, [x, y, z]: Pos) -> bool {
        y > 64 && y <= 64 + self.height(x, z)
    }
    fn taken(&self, root: Pos) -> bool {
        self.taken.contains(&root)
    }
}

/// Answers each draw from a script, checking it lies in the asked range.
struct Script(VecDeque<i32>);

impl Script {
    fn of(values: &[i32]) -> Self {
        Self(values.iter().copied().collect())
    }
}

impl Dice for Script {
    fn between(&mut self, min: i32, max: i32) -> i32 {
        let v = self
            .0
            .pop_front()
            .unwrap_or_else(|| panic!("no draw left for {min}..={max}"));
        assert!((min..=max).contains(&v), "{v} not in {min}..={max}");
        v
    }
}

const AT: Pos = [0, 65, 0];

#[test]
fn half_of_all_creepers_care_by_the_uuids_low_bit() {
    assert!(cares_about_hemp(0x1234_0000_0000_0000_0000_0000_0000_0010));
    assert!(!cares_about_hemp(0x1234_0000_0000_0000_0000_0000_0000_0011));
}

#[test]
fn a_two_tall_open_sky_plant_in_a_sampled_column_is_cover_at_its_root() {
    let plot = Plot::default().plant(10, -3, 2);
    let cover = find_cover(&plot, &mut Script::of(&[10, -3]), AT, NOTICE_RANGE, 1);
    assert_eq!(cover, Some([10, 65, -3]));
}

#[test]
fn seedlings_roofed_plants_unloaded_and_taken_columns_are_passed_over() {
    let mut plot = Plot::default()
        .plant(1, 1, 1)
        .plant(2, 2, 3)
        .plant(3, 3, 3)
        .plant(4, 4, 2)
        .plant(5, 5, 3);
    plot.roofed.insert([2, 2]);
    plot.unloaded.insert([3, 3]);
    plot.taken.insert([4, 65, 4]);
    let mut dice = Script::of(&[1, 1, 2, 2, 3, 3, 4, 4, 5, 5]);
    let cover = find_cover(&plot, &mut dice, AT, NOTICE_RANGE, 5);
    assert_eq!(cover, Some([5, 65, 5]));
    assert!(dice.0.is_empty());
}

#[test]
fn a_plant_rooted_more_than_three_blocks_above_or_below_is_out_of_reach() {
    let plot = Plot::default().plant(6, 6, 2);
    let low = [0, 61, 0];
    let high = [0, 69, 0];
    assert_eq!(
        find_cover(&plot, &mut Script::of(&[6, 6]), low, 24, 1),
        None
    );
    assert_eq!(
        find_cover(&plot, &mut Script::of(&[6, 6]), high, 24, 1),
        None
    );
    assert!(find_cover(&plot, &mut Script::of(&[6, 6]), [0, 62, 0], 24, 1).is_some());
}

#[test]
fn nothing_found_after_every_sample_is_no_cover() {
    let plot = Plot::default();
    let mut dice = Script::of(&[0; 16]);
    assert_eq!(
        find_cover(&plot, &mut dice, AT, NOTICE_RANGE, NOTICE_SAMPLES),
        None
    );
    assert!(dice.0.is_empty(), "eight samples, two draws each");
}

#[test]
fn a_visit_notices_from_afar_then_prowls_close_and_rests_after_its_last_step() {
    let plot = Plot::default().plant(10, 0, 2).plant(12, 0, 2);
    let mut prowl = Prowl::default();
    assert_eq!(prowl.interval(), NOTICE_CHANCE);

    // Noticed: a 24-block glance, then the visit's length is drawn.
    let mut dice = Script::of(&[10, 0, 3]);
    assert_eq!(
        prowl.next_spot(&plot, &mut dice, AT, 100),
        Some([10, 65, 0])
    );
    prowl.started(&mut dice, 100);
    assert!(prowl.prowling());
    assert_eq!(prowl.interval(), PROWL_CHANCE);

    // Three prowl steps within 5 blocks; the third is the last.
    for now in [200, 250, 300] {
        let mut dice = Script::of(&[2, 0]);
        assert_eq!(
            prowl.next_spot(&plot, &mut dice, [10, 65, 0], now),
            Some([12, 65, 0])
        );
        prowl.started(&mut Script::of(&[1500]), now);
    }
    assert!(!prowl.prowling());
    assert!(prowl.resting(300 + 1499));
    assert!(!prowl.resting(300 + 1500));
    assert_eq!(prowl.interval(), NOTICE_CHANCE);
}

#[test]
fn a_prowl_that_finds_no_more_field_ends_the_visit_in_a_rest() {
    let plot = Plot::default().plant(10, 0, 2);
    let mut prowl = Prowl::default();
    let _ = prowl.next_spot(&plot, &mut Script::of(&[10, 0]), AT, 0);
    prowl.started(&mut Script::of(&[MAX_PROWLS]), 0);
    // Led out of the field: six empty samples, then the rest.
    let mut dice = Script::of(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, MIN_REST]);
    assert_eq!(prowl.next_spot(&plot, &mut dice, [40, 65, 40], 50), None);
    assert!(!prowl.prowling());
    assert!(prowl.resting(50 + i64::from(MIN_REST) - 1));
}

#[test]
fn a_creeper_not_in_a_visit_finding_nothing_does_not_rest() {
    let mut prowl = Prowl::default();
    assert_eq!(
        prowl.next_spot(&Plot::default(), &mut Script::of(&[0; 16]), AT, 9),
        None
    );
    assert!(!prowl.resting(9));
}

#[test]
fn the_lurk_is_drawn_once_when_the_walk_ends_and_held_until_it_runs_out() {
    let mut prowl = Prowl::default();
    assert!(prowl.lurk(true, &mut Script::of(&[]), 10), "still walking");
    assert!(prowl.lurk(false, &mut Script::of(&[60]), 10));
    assert!(prowl.lurk(false, &mut Script::of(&[]), 69));
    assert!(!prowl.lurk(false, &mut Script::of(&[]), 70));
    prowl.stopped();
    assert!(
        prowl.lurk(false, &mut Script::of(&[MAX_LURK]), 100),
        "a new lurk"
    );
}
