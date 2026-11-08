//! The edible rules against the numbers the Java mod's `EdibleBundle` and `EdibleEffects` state,
//! with the bundle as the Java data file writes it.

use serde_json::json;

use super::bundle::Onset;
use super::*;

/// Vanilla's categories of the bundle's effects.
fn category(id: &str) -> Category {
    match id {
        "minecraft:absorption" | "minecraft:resistance" | "minecraft:regeneration" => {
            Category::Beneficial
        }
        _ => Category::Harmful,
    }
}

/// `data/hempdustry/hempdustry/edible/default.json`, as the Java datagen writes it.
fn shipped() -> serde_json::Value {
    let tiers = |a: [i32; 4]| json!(a);
    json!({
        "effects": [
            {"amplifier_by_quality": {"perfect": tiers([0, 0, 0, 0])},
             "amplifier_by_tier": tiers([0, 0, 1, 1]), "arrives_ticks": 0, "effect": "minecraft:slowness"},
            {"amplifier_by_tier": tiers([0, 1, 2, 3]), "arrives_ticks": 160, "effect": "minecraft:absorption"},
            {"amplifier_by_tier": tiers([0, 0, 0, 0]), "arrives_ticks": 160, "effect": "minecraft:resistance"},
            {"amplifier_by_tier": tiers([0, 0, 0, 0]), "arrives_ticks": 400, "effect": "minecraft:hunger"},
            {"amplifier_by_tier": tiers([0, 0, 0, 0]), "arrives_ticks": 600,
             "duration_by_tier": tiers([100, 200, 300, 400]), "effect": "minecraft:regeneration"}
        ],
        "qualities": {
            "clean": {"duration_ticks": 7200, "onset_ticks": {"max": 2700, "min": 1300, "mode": 1800}},
            "perfect": {"duration_ticks": 9600, "onset_ticks": {"max": 1860, "min": 1740, "mode": 1800}},
            "rough": {"duration_ticks": 3600, "onset_ticks": {"max": 5200, "min": 900, "mode": 2400}},
            "standard": {"duration_ticks": 5400, "onset_ticks": {"max": 3900, "min": 1100, "mode": 2000}}
        },
        "tier_share": [0.625, 0.75, 0.875, 1.0]
    })
}

fn rules<'a>(bundle: &'a Bundle, policy: &'a Policy) -> Rules<'a> {
    Rules {
        bundle,
        policy,
        category: &category,
    }
}

/// `(effect, delay, duration, amplifier)` of each dose, for comparing with the Java tables.
fn table(doses: &[Dose]) -> Vec<(&str, i32, i32, i32)> {
    doses
        .iter()
        .map(|d| {
            (
                d.effect.id.trim_start_matches("minecraft:"),
                d.delay,
                d.effect.duration,
                d.effect.amplifier,
            )
        })
        .collect()
}

#[test]
fn the_shipped_file_reads_as_the_built_in_bundle() {
    assert_eq!(Bundle::parse(&shipped()), Some(Bundle::built_in()));
}

#[test]
fn a_file_the_codec_refuses_is_left_out() {
    let mut onset_backwards = shipped();
    onset_backwards["qualities"]["rough"]["onset_ticks"]["min"] = json!(6000);
    assert_eq!(Bundle::parse(&onset_backwards), None, "min past max");
    let mut three_tiers = shipped();
    three_tiers["tier_share"] = json!([0.5, 0.75, 1.0]);
    assert_eq!(Bundle::parse(&three_tiers), None, "three tier shares");
    let mut no_perfect = shipped();
    no_perfect["qualities"]
        .as_object_mut()
        .map(|q| q.remove("perfect"));
    assert_eq!(Bundle::parse(&no_perfect), None, "a quality left out");
    let mut unknown_grade = shipped();
    unknown_grade["effects"][0]["amplifier_by_quality"] = json!({"legendary": [0, 0, 0, 0]});
    assert_eq!(
        Bundle::parse(&unknown_grade),
        None,
        "a grade that isn't one"
    );
}

#[test]
fn strength_falls_into_four_even_quartiles() {
    let tiers: Vec<i32> = (0..=26).map(tier_from_strength).collect();
    assert_eq!(&tiers[..=6], &[1; 7], "0 to 6");
    assert_eq!(&tiers[7..=12], &[2; 6]);
    assert_eq!(&tiers[13..=18], &[3; 6]);
    assert_eq!(&tiers[19..], &[4; 8], "19 to 24 and past the cap");
}

#[test]
fn an_offset_steps_the_tier_and_stays_inside_the_ladder() {
    assert_eq!(
        apply_offset(1, -1),
        1,
        "toast from a weak butter is still I"
    );
    assert_eq!(apply_offset(4, 1), 4, "dawamesk from the strongest is IV");
    assert_eq!(apply_offset(2, 1), 3);
    assert_eq!(potency_offset("hempdustry:space_cookie"), Some(-1));
    assert_eq!(potency_offset("hempdustry:space_cake"), Some(0));
    assert_eq!(potency_offset("hempdustry:dawamesk"), Some(1));
    assert_eq!(
        potency_offset("hempdustry:bhang_bucket"),
        None,
        "no butter in bhang"
    );
    assert!(is_edible("hempdustry:bhang_bucket"));
    assert!(!is_edible("hempdustry:space_cake"), "eaten off the block");
    assert!(
        !is_edible("hempdustry:hemp_flapjack"),
        "hemp-seed food carries no dose"
    );
}

#[test]
fn the_onset_roll_spans_the_window_and_peaks_at_the_mode() {
    let rough = Onset {
        min: 900,
        mode: 2400,
        max: 5200,
    };
    assert_eq!(rough.roll(0.0), 900);
    assert_eq!(rough.roll(0.999_999_999), 5200);
    // The mode sits at the share of the window below it: (2400 - 900) / 4300.
    assert_eq!(rough.roll(1500.0 / 4300.0), 2400);
    let point = Onset {
        min: 1800,
        mode: 1800,
        max: 1800,
    };
    assert_eq!(point.roll(0.7), 1800);
    // Symmetric window, a quarter of the way into the draw: min + sqrt(0.25 * 100 * 50).
    let even = Onset {
        min: 0,
        mode: 50,
        max: 100,
    };
    assert_eq!(even.roll(0.25), 35);
}

#[test]
fn the_onset_multiplier_scales_the_window_too() {
    let bundle = Bundle::built_in();
    let half = Policy {
        onset_multiplier: 0.5,
        ..Policy::default()
    };
    let rules = rules(&bundle, &half);
    assert_eq!(rules.roll_onset(Quality::Rough, 0.0), 450);
    assert_eq!(rules.roll_onset(Quality::Perfect, 0.5), 900);
}

#[test]
fn a_perfect_tier_four_brownie_keeps_slowness_at_one_and_absorption_at_two() {
    let bundle = Bundle::built_in();
    let policy = Policy::default();
    let doses = rules(&bundle, &policy).doses(4, Quality::Perfect, 0.5);
    // Perfect kicks in at 1800 at the draw's middle; the high lasts 8:00.
    assert_eq!(
        table(&doses),
        [
            ("slowness", 1800, 9600, 0),
            ("absorption", 1960, 9440, 1),
            ("resistance", 1960, 9440, 0),
            ("hunger", 2200, 9200, 0),
            ("regeneration", 2400, 400, 0),
        ]
    );
}

#[test]
fn a_rough_tier_one_toast_is_five_eighths_of_a_plain_potion() {
    let bundle = Bundle::built_in();
    let policy = Policy::default();
    let doses = rules(&bundle, &policy).bundle(1, Quality::Rough, 900);
    assert_eq!(
        table(&doses),
        [
            ("slowness", 900, 2250, 0),
            ("absorption", 1060, 2090, 0),
            ("resistance", 1060, 2090, 0),
            ("hunger", 1300, 1850, 0),
            ("regeneration", 1500, 100, 0),
        ]
    );
}

#[test]
fn rough_slowness_climbs_with_the_tier_where_perfect_holds() {
    let bundle = Bundle::built_in();
    let policy = Policy::default();
    let rules = rules(&bundle, &policy);
    let slowness = |quality| rules.bundle(3, quality, 1)[0].effect.amplifier;
    assert_eq!(slowness(Quality::Rough), 1);
    assert_eq!(slowness(Quality::Clean), 1);
    assert_eq!(slowness(Quality::Perfect), 0);
}

#[test]
fn the_config_switches_reach_an_edible() {
    let bundle = Bundle::built_in();
    let no_munchies = Policy {
        munchies: false,
        ..Policy::default()
    };
    let doses = rules(&bundle, &no_munchies).doses(2, Quality::Standard, 0.5);
    assert!(doses.iter().all(|d| d.effect.id != "minecraft:hunger"));
    assert_eq!(doses.len(), 4);
    let no_debuffs = Policy {
        debuffs: false,
        ..Policy::default()
    };
    let doses = rules(&bundle, &no_debuffs).doses(2, Quality::Standard, 0.5);
    assert_eq!(
        doses
            .iter()
            .map(|d| d.effect.id.as_str())
            .collect::<Vec<_>>(),
        [
            "minecraft:absorption",
            "minecraft:resistance",
            "minecraft:regeneration"
        ]
    );
    let off = Policy {
        enabled: false,
        ..Policy::default()
    };
    assert!(
        rules(&bundle, &off)
            .doses(4, Quality::Perfect, 0.5)
            .is_empty()
    );
}

#[test]
fn the_duration_multiplier_scales_the_high_but_not_regeneration() {
    let bundle = Bundle::built_in();
    let double = Policy {
        duration_multiplier: 2.0,
        ..Policy::default()
    };
    let doses = rules(&bundle, &double).bundle(4, Quality::Rough, 0);
    assert_eq!(doses[0].effect.duration, 7200, "slowness: twice 3:00");
    assert_eq!(
        doses[4].effect.duration, 400,
        "regeneration keeps its own length"
    );
}

#[test]
fn an_undosed_edible_queues_nothing() {
    let bundle = Bundle::built_in();
    let policy = Policy::default();
    assert!(
        rules(&bundle, &policy)
            .doses(0, Quality::Perfect, 0.5)
            .is_empty()
    );
}

#[test]
fn doses_land_after_their_delay_in_the_order_eaten() {
    let effect = |id: &str| Effect::new(id, 100, 0);
    let mut waiting = Waiting::default();
    waiting.schedule(Dose {
        delay: 3,
        effect: effect("slowness"),
    });
    waiting.schedule(Dose {
        delay: 1,
        effect: effect("hunger"),
    });
    waiting.schedule(Dose {
        delay: 0,
        effect: effect("speed"),
    });
    let ids = |landed: Vec<Effect>| landed.into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(
        ids(waiting.tick()),
        ["minecraft:hunger", "minecraft:speed"],
        "a delay of 0 still waits a tick"
    );
    assert!(waiting.tick().is_empty());
    assert_eq!(ids(waiting.tick()), ["minecraft:slowness"]);
    assert!(waiting.is_empty());
}

#[test]
fn a_bite_of_cake_feeds_two_and_never_oversaturates() {
    assert_eq!(after_bite(10, 1.0), (12, 1.4));
    assert_eq!(after_bite(19, 19.0), (20, 19.4));
    assert_eq!(after_bite(0, 0.0), (2, 0.4));
    assert!(can_eat(false, 19));
    assert!(!can_eat(false, MAX_FOOD));
    assert!(can_eat(true, MAX_FOOD), "a creative player always may");
}
