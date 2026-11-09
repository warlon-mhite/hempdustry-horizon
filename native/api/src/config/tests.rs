#![expect(
    clippy::float_cmp,
    reason = "every float here is read or clamped to an exact value, never computed"
)]

use super::*;

/// The file the Java mod writes on its first start (2.1.0, 26.3), untouched.
const JAVA_DEFAULTS: &str = include_str!("java-defaults.json");

fn parse(text: &str) -> (Settings, Vec<Clamped>) {
    Settings::parse(text).unwrap_or_else(|e| panic!("should parse: {e}"))
}

#[test]
fn the_defaults_write_the_java_mods_file_byte_for_byte() {
    assert_eq!(Settings::default().to_json(), JAVA_DEFAULTS);
}

#[test]
fn the_java_mods_file_reads_as_the_defaults_with_nothing_clamped() {
    assert_eq!(parse(JAVA_DEFAULTS), (Settings::default(), Vec::new()));
}

#[test]
fn an_empty_file_or_null_is_all_defaults() {
    for text in ["", "  \n", "null"] {
        assert_eq!(parse(text).0, Settings::default(), "{text:?}");
    }
}

#[test]
fn a_half_written_file_keeps_the_defaults_it_does_not_mention() {
    let (s, clamped) = parse(r#"{"world": {"mildew": false}, "effects": {"maxLevel": 3}}"#);
    assert!(clamped.is_empty());
    assert!(!s.world.mildew);
    assert_eq!(s.effects.max_level, 3);
    assert_eq!(s.world.crop_growth_multiplier, 1.0);
    assert_eq!(s.effects.max_buff_level, 2);
    assert_eq!(s.infuser, Timings::default());
    assert_eq!(s.client, Client::default());
}

#[test]
fn comments_and_unknown_keys_are_ignored() {
    let (s, _) =
        parse(r#"{"_comment": "hi", "world": {"_comment": "x", "unknown": 3}, "other": []}"#);
    assert_eq!(s, Settings::default());
}

#[test]
fn values_out_of_range_are_clamped_and_reported() {
    let (s, clamped) = parse(
        r#"{"effects": {"maxLevel": 50, "cooldownMultiplier": -1},
            "world": {"cropGrowthMultiplier": 0.0},
            "infuser": {"minTimeTicks": 10, "fullTimeTicks": 15}}"#,
    );
    assert_eq!(s.effects.max_level, 10);
    assert_eq!(s.effects.cooldown_multiplier, 0.0);
    assert_eq!(s.world.crop_growth_multiplier, 0.05);
    assert_eq!(s.infuser, Timings { min: 20, full: 40 });
    let names: Vec<_> = clamped.iter().map(|c| c.name).collect();
    assert_eq!(
        names,
        [
            "infuser.minTimeTicks",
            "infuser.fullTimeTicks",
            "effects.maxLevel",
            "effects.cooldownMultiplier",
            "world.cropGrowthMultiplier",
        ]
    );
    assert_eq!(
        clamped[2].to_string(),
        "config effects.maxLevel was 50, clamped to 10"
    );
}

#[test]
fn the_infuser_keeps_full_twenty_ticks_past_min_and_under_the_ceiling() {
    let (s, _) = parse(r#"{"infuser": {"minTimeTicks": 40000, "fullTimeTicks": 50000}}"#);
    assert_eq!(
        s.infuser,
        Timings {
            min: 31_980,
            full: 32_000
        }
    );
}

#[test]
fn a_whole_number_written_with_a_fraction_reads_as_java_does() {
    let (s, _) = parse(r#"{"effects": {"maxLevel": 4.0, "maxBuffLevel": 3.7}}"#);
    assert_eq!((s.effects.max_level, s.effects.max_buff_level), (4, 3));
}

#[test]
fn a_file_that_cannot_be_read_is_an_error_not_defaults() {
    assert!(matches!(Settings::parse("[1, 2]"), Err(Error::NotAnObject)));
    assert!(matches!(Settings::parse("3"), Err(Error::NotAnObject)));
    assert!(matches!(
        Settings::parse("{\"world\": "),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        Settings::parse(r#"{"world": {"mildew": "no"}}"#),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        Settings::parse(r#"{"world": 5}"#),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn a_changed_config_survives_its_own_round_trip() {
    let (s, _) = parse(
        r#"{"effects": {"enabled": false, "onsetMultiplier": 2.5},
            "world": {"creepersSeekHemp": false, "machineSpeedMultiplier": 3.0},
            "infuser": {"minTimeTicks": 1200, "fullTimeTicks": 2400},
            "loot": {"chanceMultiplier": 0.5}, "client": {"ripeAroma": false}}"#,
    );
    assert_eq!(parse(&s.to_json()), (s, Vec::new()));
}

#[test]
fn machine_time_divides_by_the_speed_rounding_as_java_and_never_reaches_zero() {
    let at = |speed| {
        let mut s = Settings::default();
        s.world.machine_speed_multiplier = speed;
        s
    };
    assert_eq!(at(1.0).machine_time(500), 500);
    assert_eq!(at(2.0).machine_time(500), 250);
    assert_eq!(at(3.0).machine_time(500), 167);
    assert_eq!(at(3.0).machine_time(200), 67);
    assert_eq!(at(0.05).machine_time(200), 4000);
    assert_eq!(at(20.0).machine_time(10), 1);
    assert_eq!(at(20.0).machine_time(1), 1);
}

#[test]
fn the_farming_rules_get_growth_and_mildew() {
    let (s, _) = parse(r#"{"world": {"cropGrowthMultiplier": 2.0, "mildew": false}}"#);
    assert_eq!(
        s.farm(),
        farm::Config {
            crop_growth_multiplier: 2.0,
            mildew: false
        }
    );
}
