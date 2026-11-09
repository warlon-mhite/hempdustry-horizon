//! The smoking rules against the numbers the Java mod's balance table states (durations,
//! levels, green-out odds, wear), with the strains as the Java data files write them.
#![expect(
    clippy::unreadable_literal,
    reason = "colours copied as the Java data files write them"
)]

use serde_json::json;

use super::*;
use crate::smoke::hit::{EXHALE_DELAY_TICKS, NAUSEA_DURATION_TICKS};

/// Vanilla's categories of the effects these strains use.
fn category(id: &str) -> Category {
    match id {
        "minecraft:resistance"
        | "minecraft:speed"
        | "minecraft:haste"
        | "minecraft:night_vision"
        | "minecraft:slow_falling"
        | "minecraft:invisibility"
        | "minecraft:absorption"
        | "minecraft:regeneration" => Category::Beneficial,
        _ => Category::Harmful,
    }
}

fn catalog() -> Catalog {
    let mut catalog = Catalog::default();
    catalog.set_strains(&[
        (
            "hempdustry:indica".into(),
            json!({"buds":"hempdustry:indica_buds","color":11893759,
            "effects":[{"effect":"minecraft:resistance"},{"effect":"minecraft:hunger"},
                {"effect":"minecraft:mining_fatigue"}],
            "flower":"hempdustry:indica_flower","model_index":1,"seeds":"hempdustry:indica_seeds",
            "translation_key":"hempdustry.strain.indica"}),
        ),
        (
            "hempdustry:hashish".into(),
            json!({"buds":"hempdustry:hashish","color":7031343,
            "effects":[{"effect":"minecraft:night_vision","scales":false},
                {"effect":"minecraft:resistance"},{"effect":"minecraft:slowness","scales":false},
                {"effect":"minecraft:hunger"}],
            "model_index":3,"translation_key":"hempdustry.strain.hashish"}),
        ),
        (
            "hempdustry:schwag".into(),
            json!({"buds":"hempdustry:schwag","color":9202236,
            "cough_factor":0.5,"effects":[{"chance":0.6,"duration_factor":0.14285715,
                "effect":"minecraft:poison","max_amplifier":1},{"effect":"minecraft:hunger"}],
            "green_out_factor":0.0,"translation_key":"hempdustry.strain.schwag"}),
        ),
        (
            "hempdustry:beldia".into(),
            json!({"buds":"hempdustry:beldia_buds","color":14264654,
            "cough_factor":0.5,"effects":[{"effect":"minecraft:invisibility","scales":false},
                {"duration_factor":0.25,"effect":"minecraft:blindness","on_exhale":true,
                    "scales":false},{"effect":"minecraft:hunger"}],
            "flower":"hempdustry:beldia_crop","model_index":7,
            "translation_key":"hempdustry.strain.beldia"}),
        ),
        (
            "hempdustry:rosin".into(),
            json!({"buds":"hempdustry:rosin","color":14199098,
            "dose_per_item":3,"effects":[{"effect":"minecraft:resistance"},
                {"effect":"minecraft:slowness","scales":false},{"effect":"minecraft:hunger"}],
            "green_out_factor":2.0,"model_index":6,"translation_key":"hempdustry.strain.rosin"}),
        ),
    ]);
    catalog.set_edibles(&[(
        "hempdustry:default".into(),
        json!({"effects":[{"effect":"minecraft:slowness"},{"effect":"minecraft:absorption"},
            {"effect":"minecraft:resistance"},{"effect":"minecraft:regeneration"}]}),
    )]);
    catalog
}

/// A random that answers from a script, then 1 and 0.99 (no roll lands) once it runs out.
#[derive(Default)]
struct Script {
    ints: Vec<i32>,
    floats: Vec<f32>,
    asked: Vec<i32>,
}

impl Dice for Script {
    fn next_int(&mut self, bound: i32) -> i32 {
        self.asked.push(bound);
        if self.ints.is_empty() {
            1
        } else {
            self.ints.remove(0)
        }
    }
    fn next_float(&mut self) -> f32 {
        if self.floats.is_empty() {
            0.99
        } else {
            self.floats.remove(0)
        }
    }
}

fn effect(hit: &Hit, id: &str) -> Option<Effect> {
    hit.effects.iter().find(|e| e.id == id).cloned()
}

fn hit(device: Device, contents: &SmokeContents, dice: &mut Script) -> Hit {
    let catalog = catalog();
    let policy = Policy::default();
    let rules = Rules {
        catalog: &catalog,
        policy: &policy,
        category: &category,
    };
    rules
        .take_hit(device, contents, Smoker::default(), dice)
        .unwrap()
}

#[test]
fn a_bong_of_three_holds_its_buffs_at_two_and_lasts_half_as_long_again() {
    let mut dice = Script::default();
    let hit = hit(
        Device::Bong,
        &SmokeContents::of("hempdustry:indica", 3),
        &mut dice,
    );
    // The table: dose 3 is level II for the buffs, 75 s; Hunger and the strain's cost at III.
    assert_eq!(
        effect(&hit, "minecraft:resistance"),
        Some(Effect::new("resistance", 1500, 1))
    );
    assert_eq!(
        effect(&hit, "minecraft:hunger"),
        Some(Effect::new("hunger", 1500, 2))
    );
    assert_eq!(
        effect(&hit, "minecraft:mining_fatigue"),
        Some(Effect::new("mining_fatigue", 1500, 2))
    );
    assert_eq!(hit.cooldown_ticks, 100);
    assert_eq!(hit.sound_delay, 0, "the bong's bubbling was the draw");
    assert_eq!(hit.exhale_delay, EXHALE_DELAY_TICKS);
}

#[test]
fn each_device_rolls_the_green_out_cough_and_nausea_at_its_own_odds() {
    let rolls = |device, dose| {
        let mut dice = Script::default();
        hit(
            device,
            &SmokeContents::of("hempdustry:indica", dose),
            &mut dice,
        );
        dice.asked
    };
    // Green-out (none at dose 1), then cough, then nausea.
    assert_eq!(rolls(Device::Pipe, 1), [4, 50]);
    assert_eq!(rolls(Device::Pipe, 2), [12, 4, 50]);
    assert_eq!(rolls(Device::Bong, 3), [4, 3, 5]);
    assert_eq!(rolls(Device::Spliff, 2), [24, 6, 500]);
    assert_eq!(rolls(Device::Spliff, 3), [8, 6, 500]);
    assert_eq!(rolls(Device::Vaporizer, 1), [50, 200]);
}

#[test]
fn a_full_green_out_strips_the_mods_buffs_and_locks_every_smokeable_out() {
    let mut dice = Script {
        ints: vec![0],
        ..Script::default()
    };
    let hit = hit(
        Device::Bong,
        &SmokeContents::of("hempdustry:indica", 3),
        &mut dice,
    );
    let green = hit.green_out.clone().unwrap();
    assert!(green.full);
    assert!(
        hit.effects.is_empty() && hit.exhaled.is_empty(),
        "no buzz on a green-out"
    );
    assert_eq!(green.effects[0], Effect::new("nausea", 700, 0));
    assert_eq!(
        green.remove,
        [
            "minecraft:absorption",
            "minecraft:invisibility",
            "minecraft:night_vision",
            "minecraft:regeneration",
            "minecraft:resistance"
        ]
    );
    assert_eq!(hit.cooldown_ticks, 1200);
    assert_eq!(dice.asked, [4, 3], "no nausea roll on a green-out");
}

#[test]
fn the_spins_only_slow_and_keep_the_plain_cooldown() {
    let mut dice = Script {
        ints: vec![0],
        ..Script::default()
    };
    let hit = hit(
        Device::Pipe,
        &SmokeContents::of("hempdustry:indica", 2),
        &mut dice,
    );
    let green = hit.green_out.unwrap();
    assert!(!green.full && green.remove.is_empty());
    assert_eq!(green.effects[1], Effect::new("slowness", 300, 1));
    assert_eq!(hit.cooldown_ticks, 60);
}

#[test]
fn schwag_never_greens_out_and_its_poison_lands_six_times_in_ten() {
    let contents = SmokeContents::of("hempdustry:schwag", 3);
    let mut lands = Script {
        floats: vec![0.59],
        ..Script::default()
    };
    let hit_ = hit(Device::Spliff, &contents, &mut lands);
    // A seventh of the dose-3 spliff's 1350 ticks (the third bud buys half again), capped at II.
    assert_eq!(
        effect(&hit_, "minecraft:poison"),
        Some(Effect::new("poison", 193, 1))
    );
    assert_eq!(
        lands.asked,
        [3, 500],
        "no green-out roll; cough halved to 1-in-3"
    );
    let mut misses = Script {
        floats: vec![0.6],
        ..Script::default()
    };
    assert_eq!(
        effect(
            &hit(Device::Spliff, &contents, &mut misses),
            "minecraft:poison"
        ),
        None
    );
}

#[test]
fn beldia_blinds_on_the_exhale_not_the_hit() {
    let mut dice = Script::default();
    let hit = hit(
        Device::Pipe,
        &SmokeContents::of("hempdustry:beldia", 1),
        &mut dice,
    );
    assert_eq!(
        effect(&hit, "minecraft:invisibility"),
        Some(Effect::new("invisibility", 700, 0))
    );
    assert!(effect(&hit, "minecraft:blindness").is_none());
    assert_eq!(hit.exhaled, [Effect::new("blindness", 175, 0)]);
}

#[test]
fn a_bud_with_a_pinch_of_hash_goes_one_level_past_the_cap() {
    let contents = SmokeContents {
        entries: vec![
            contents::Entry {
                strain: "hempdustry:indica".into(),
                count: 3,
            },
            contents::Entry {
                strain: "hempdustry:hashish".into(),
                count: 1,
            },
        ],
    };
    let mut dice = Script::default();
    let hit = hit(Device::Bong, &contents, &mut dice);
    let resistance: Vec<_> = hit
        .effects
        .iter()
        .filter(|e| e.id == "minecraft:resistance")
        .collect();
    // The bud's Resistance at III; the hash's own at I, held to the plain cap.
    assert_eq!(resistance[0].amplifier, 2);
    assert_eq!(resistance[1].amplifier, 0);
    assert_eq!(effect(&hit, "minecraft:night_vision").unwrap().amplifier, 0);
    // Dose 4: the green-out odds stay at the last listed, 1-in-4.
    assert_eq!(dice.asked[0], 4);
}

#[test]
fn the_whole_outfit_doubles_the_cough_and_nausea_odds_but_not_the_green_out() {
    let catalog = catalog();
    let policy = Policy::default();
    let rules = Rules {
        catalog: &catalog,
        policy: &policy,
        category: &category,
    };
    let mut dice = Script::default();
    rules
        .take_hit(
            Device::Bong,
            &SmokeContents::of("hempdustry:indica", 3),
            Smoker { full_outfit: true },
            &mut dice,
        )
        .unwrap();
    assert_eq!(dice.asked, [4, 6, 10]);
}

#[test]
fn a_nausea_roll_gives_seven_seconds_of_it() {
    let mut dice = Script {
        ints: vec![1, 1, 0],
        ..Script::default()
    };
    let hit = hit(
        Device::Bong,
        &SmokeContents::of("hempdustry:indica", 2),
        &mut dice,
    );
    assert_eq!(
        hit.nausea,
        Some(Effect::new("nausea", NAUSEA_DURATION_TICKS, 0))
    );
    assert!(!hit.cough);
}

#[test]
fn a_switched_off_policy_gives_nothing_and_no_green_out() {
    let catalog = catalog();
    let policy = Policy {
        enabled: false,
        ..Policy::default()
    };
    let rules = Rules {
        catalog: &catalog,
        policy: &policy,
        category: &category,
    };
    let mut dice = Script {
        ints: vec![0, 1, 1],
        ..Script::default()
    };
    let hit = rules
        .take_hit(
            Device::Bong,
            &SmokeContents::of("hempdustry:indica", 3),
            Smoker::default(),
            &mut dice,
        )
        .unwrap();
    assert!(hit.green_out.is_none() && hit.effects.is_empty());
}

fn spend(device: Device, contents: &SmokeContents, worn: Worn, dice: &mut Script) -> Option<Spent> {
    let catalog = catalog();
    let policy = Policy::default();
    let rules = Rules {
        catalog: &catalog,
        policy: &policy,
        category: &category,
    };
    rules.spend(device, contents, worn, false, dice)
}

#[test]
fn a_hit_takes_a_charge_and_a_point_and_the_last_charge_unpacks() {
    let indica = SmokeContents::of("hempdustry:indica", 1);
    let mut dice = Script::default();
    let worn = Worn {
        charges: 4,
        damage: 0,
        unbreaking: 0,
    };
    assert_eq!(
        spend(Device::Bong, &indica, worn, &mut dice),
        Some(Spent {
            damage: 1,
            broken: false,
            charges: Some(3),
            scorched: 0
        })
    );
    let last = Worn {
        charges: 1,
        damage: 5,
        unbreaking: 0,
    };
    assert_eq!(
        spend(Device::Bong, &indica, last, &mut dice),
        Some(Spent {
            damage: 6,
            broken: false,
            charges: None,
            scorched: 0
        })
    );
}

#[test]
fn the_last_point_of_wear_waits_for_the_bowl_to_empty() {
    let indica = SmokeContents::of("hempdustry:indica", 1);
    let mut dice = Script::default();
    // A pipe (8 uses) at 7 with a hit still in the bowl keeps its last point...
    let held = spend(
        Device::Pipe,
        &indica,
        Worn {
            charges: 2,
            damage: 7,
            unbreaking: 0,
        },
        &mut dice,
    );
    assert_eq!(
        held,
        Some(Spent {
            damage: 7,
            broken: false,
            charges: Some(1),
            scorched: 0
        })
    );
    // ...and breaks on the hit that empties it.
    let gone = spend(
        Device::Pipe,
        &indica,
        Worn {
            charges: 1,
            damage: 7,
            unbreaking: 0,
        },
        &mut dice,
    );
    assert_eq!(gone.map(|s| s.broken), Some(true));
}

#[test]
fn unbreaking_spares_a_point_at_its_vanilla_odds() {
    let indica = SmokeContents::of("hempdustry:indica", 1);
    let worn = Worn {
        charges: 4,
        damage: 0,
        unbreaking: 3,
    };
    // Level III keeps the point away three times in four.
    let mut spared = Script {
        floats: vec![0.74],
        ..Script::default()
    };
    assert_eq!(
        spend(Device::Bong, &indica, worn, &mut spared)
            .unwrap()
            .damage,
        0
    );
    let mut worn_out = Script {
        floats: vec![0.75],
        ..Script::default()
    };
    assert_eq!(
        spend(Device::Bong, &indica, worn, &mut worn_out)
            .unwrap()
            .damage,
        1
    );
}

#[test]
fn a_spent_vaporizer_bowl_gives_back_scorched_hemp_only_from_plant_matter() {
    let mut dice = Script::default();
    let last = Worn {
        charges: 1,
        damage: 0,
        unbreaking: 0,
    };
    let bud = spend(
        Device::Vaporizer,
        &SmokeContents::of("hempdustry:schwag", 1),
        last,
        &mut dice,
    );
    assert_eq!(bud.unwrap().scorched, 1);
    let hash = spend(
        Device::Vaporizer,
        &SmokeContents::of("hempdustry:hashish", 1),
        last,
        &mut dice,
    );
    assert_eq!(hash.unwrap().scorched, 0);
    let pipe = spend(
        Device::Pipe,
        &SmokeContents::of("hempdustry:indica", 1),
        last,
        &mut dice,
    );
    assert_eq!(pipe.unwrap().scorched, 0);
}

#[test]
fn the_contents_read_both_java_forms_and_drop_an_unknown_strain() {
    let catalog = catalog();
    let record = json!({"entries":[{"strain":"hempdustry:indica","count":2},
        {"strain":"other:gone","count":1}]});
    assert_eq!(
        SmokeContents::parse(&record, &catalog),
        SmokeContents::of("hempdustry:indica", 2)
    );
    let bare = json!([{"strain":"hempdustry:indica","count":2}]);
    assert_eq!(
        SmokeContents::parse(&bare, &catalog),
        SmokeContents::of("hempdustry:indica", 2)
    );
    let written = SmokeContents::of("hempdustry:indica", 2).to_json();
    assert_eq!(
        written,
        json!({"entries":[{"strain":"hempdustry:indica","count":2}]})
    );
}

#[test]
fn buds_pack_as_their_strain() {
    let catalog = catalog();
    assert_eq!(
        catalog
            .from_buds("hempdustry:indica_buds")
            .map(|s| s.id.as_str()),
        Some("hempdustry:indica")
    );
    assert_eq!(
        catalog
            .from_buds("hempdustry:rosin")
            .map(|s| s.dose_per_item),
        Some(3)
    );
    assert!(catalog.from_buds("minecraft:wheat").is_none());
}

// Names: `SmokeContents.packedName`, `loadName`, `hashAdditive`, and the lang keys of en_us.

mod names {
    use serde_json::json;

    use super::catalog;
    use crate::smoke::data::Catalog;
    use crate::smoke::name::{
        BLEND, Labels, Load, PackedName, hash_additive, packed_name, with_line,
    };
    use crate::smoke::{SmokeContents, contents::Entry};

    const INDICA: &str = "hempdustry.strain.indica";
    const HASHISH: &str = "hempdustry.strain.hashish";

    fn load(entries: &[(&str, i32)]) -> SmokeContents {
        SmokeContents {
            entries: entries
                .iter()
                .map(|&(strain, count)| Entry {
                    strain: format!("hempdustry:{strain}"),
                    count,
                })
                .collect(),
        }
    }

    #[test]
    fn a_spliff_of_two_buds_is_named_for_the_strain_with_its_level() {
        let catalog = catalog();
        let name = packed_name(
            "hempdustry:spliff",
            &load(&[("indica", 2)]),
            Labels::of(&catalog),
        );
        assert_eq!(
            name,
            Some(PackedName {
                format: "item.hempdustry.spliff.packed",
                load: Load::Strain(INDICA),
                level: Some(2),
            })
        );
    }

    #[test]
    fn every_glass_is_a_bong_and_one_bud_has_no_numeral() {
        let catalog = catalog();
        for item in [
            "hempdustry:bong",
            "hempdustry:red_bong",
            "hempdustry:tinted_bong",
        ] {
            let name = packed_name(item, &load(&[("indica", 1)]), Labels::of(&catalog));
            assert_eq!(
                name,
                Some(PackedName {
                    format: "item.hempdustry.bong.packed",
                    load: Load::Strain(INDICA),
                    level: None,
                }),
                "{item}"
            );
        }
    }

    #[test]
    fn a_moon_rock_never_shows_a_level() {
        let catalog = catalog();
        let name = packed_name(
            "hempdustry:moon_rock",
            &load(&[("indica", 3)]),
            Labels::of(&catalog),
        );
        assert_eq!(
            name.map(|n| (n.format, n.level)),
            Some(("item.hempdustry.moon_rock.of", None))
        );
    }

    #[test]
    fn a_plant_with_hash_keeps_the_plant_name_and_the_spliff_says_with_hash() {
        let catalog = catalog();
        let labels = Labels::of(&catalog);
        let contents = load(&[("hashish", 1), ("indica", 2)]);
        let name = packed_name("hempdustry:spliff", &contents, labels);
        assert_eq!(
            name.map(|n| (n.load, n.level)),
            Some((Load::Strain(INDICA), Some(2)))
        );
        assert_eq!(
            with_line("hempdustry:spliff", &contents, labels),
            Some(HASHISH)
        );
        assert_eq!(
            with_line("hempdustry:moon_rock", &contents, labels),
            Some(HASHISH)
        );
        assert_eq!(with_line("hempdustry:wooden_pipe", &contents, labels), None);
    }

    #[test]
    fn a_tie_between_plant_and_hash_goes_to_the_plant_whatever_the_order() {
        let catalog = catalog();
        let labels = Labels::of(&catalog);
        for contents in [
            load(&[("hashish", 1), ("indica", 1)]),
            load(&[("indica", 1), ("hashish", 1)]),
        ] {
            let name = packed_name("hempdustry:wooden_pipe", &contents, labels);
            assert_eq!(name.map(|n| n.load), Some(Load::Strain(INDICA)));
        }
    }

    #[test]
    fn two_plants_or_two_hashes_read_as_mixed() {
        let catalog = catalog();
        let labels = Labels::of(&catalog);
        for contents in [
            load(&[("indica", 2), ("beldia", 1)]),
            load(&[("hashish", 1), ("rosin", 1)]),
        ] {
            let name = packed_name("hempdustry:spliff", &contents, labels);
            assert_eq!(name.map(|n| n.load.key()), Some(BLEND));
            assert_eq!(hash_additive(&contents, labels), None);
        }
    }

    #[test]
    fn an_empty_load_or_another_item_keeps_its_own_name() {
        let catalog = catalog();
        let labels = Labels::of(&catalog);
        assert_eq!(
            packed_name("hempdustry:spliff", &SmokeContents::default(), labels),
            None
        );
        assert_eq!(
            packed_name("hempdustry:indica_buds", &load(&[("indica", 1)]), labels),
            None
        );
    }

    #[test]
    fn a_game_without_the_strain_files_names_the_shipped_strains() {
        let empty = Catalog::default();
        let labels = Labels::of(&empty);
        assert!(matches!(labels, Labels::BuiltIn));
        let contents = labels.contents(&json!({"entries": [
            {"strain": "hempdustry:indica", "count": 2},
            {"strain": "hempdustry:hashish", "count": 1},
            {"strain": "addon:purple_haze", "count": 5},
        ]}));
        assert_eq!(contents, load(&[("indica", 2), ("hashish", 1)]));
        let name = packed_name("hempdustry:spliff", &contents, labels);
        assert_eq!(
            name.map(|n| (n.load, n.level)),
            Some((Load::Strain(INDICA), Some(2)))
        );
        assert_eq!(
            with_line("hempdustry:spliff", &contents, labels),
            Some(HASHISH)
        );
    }
}

/// Packing by hand, one item per use, against `PackingRecipe`'s rules.
mod packing {
    use super::*;
    use crate::smoke::contents::Entry;
    use crate::smoke::pack::{Load, MOON_ROCK, Packed, pack};

    fn buds<'a>(catalog: &'a Catalog, item: &str) -> Load<'a> {
        Load::of(catalog, item, SmokeContents::default).expect(item)
    }

    /// Uses `device` again and again with `load`, from empty, until nothing more goes in.
    fn fill(catalog: &Catalog, device: Device, load: &Load<'_>) -> Vec<SmokeContents> {
        let mut bowl = SmokeContents::default();
        let mut charges = 0;
        let mut steps = Vec::new();
        while let Some(Packed {
            contents,
            charges: c,
        }) = pack(catalog, device, &bowl, charges, load)
        {
            steps.push(contents.clone());
            bowl = contents;
            charges = c;
        }
        steps
    }

    fn one(strain: &str, count: i32) -> SmokeContents {
        SmokeContents::of(&format!("hempdustry:{strain}"), count)
    }

    #[test]
    fn each_use_adds_one_bud_up_to_the_bowls_max_dose() {
        let catalog = catalog();
        let indica = buds(&catalog, "hempdustry:indica_buds");
        assert_eq!(
            fill(&catalog, Device::Bong, &indica),
            [one("indica", 1), one("indica", 2), one("indica", 3)]
        );
        assert_eq!(
            fill(&catalog, Device::Pipe, &indica),
            [one("indica", 1), one("indica", 2)]
        );
        assert_eq!(
            fill(&catalog, Device::Vaporizer, &indica),
            [one("indica", 1)]
        );
        assert!(fill(&catalog, Device::Spliff, &indica).is_empty());
    }

    #[test]
    fn a_packed_bowl_holds_every_charge_of_its_device() {
        let catalog = catalog();
        let indica = buds(&catalog, "hempdustry:indica_buds");
        for (device, hits) in [(Device::Pipe, 2), (Device::Bong, 4), (Device::Vaporizer, 2)] {
            let packed = pack(&catalog, device, &SmokeContents::default(), 0, &indica);
            assert_eq!(packed.map(|p| p.charges), Some(hits), "{device:?}");
        }
    }

    #[test]
    fn rosin_fills_a_bong_in_one_piece_and_fits_nothing_else() {
        let catalog = catalog();
        let rosin = buds(&catalog, "hempdustry:rosin");
        assert_eq!(fill(&catalog, Device::Bong, &rosin), [one("rosin", 3)]);
        assert!(fill(&catalog, Device::Pipe, &rosin).is_empty());
        assert!(fill(&catalog, Device::Vaporizer, &rosin).is_empty());
    }

    #[test]
    fn a_second_strain_is_refused() {
        let catalog = catalog();
        let hash = buds(&catalog, "hempdustry:hashish");
        let bowl = one("indica", 1);
        assert_eq!(pack(&catalog, Device::Bong, &bowl, 4, &hash), None);
    }

    #[test]
    fn a_bowl_once_hit_takes_no_more() {
        let catalog = catalog();
        let indica = buds(&catalog, "hempdustry:indica_buds");
        let bowl = one("indica", 1);
        assert_eq!(pack(&catalog, Device::Bong, &bowl, 3, &indica), None);
        assert!(pack(&catalog, Device::Bong, &bowl, 4, &indica).is_some());
    }

    #[test]
    fn a_moon_rock_is_a_whole_bowl_for_a_bong_only() {
        let catalog = catalog();
        let rock = SmokeContents {
            entries: vec![
                Entry {
                    strain: "hempdustry:indica".into(),
                    count: 3,
                },
                Entry {
                    strain: "hempdustry:hashish".into(),
                    count: 1,
                },
            ],
        };
        let load = Load::of(&catalog, MOON_ROCK, || rock.clone()).expect("a loaded rock");
        assert_eq!(
            fill(&catalog, Device::Bong, &load),
            std::slice::from_ref(&rock)
        );
        assert!(fill(&catalog, Device::Pipe, &load).is_empty());
        // Nothing after it, and it goes on nothing.
        let indica = buds(&catalog, "hempdustry:indica_buds");
        assert_eq!(pack(&catalog, Device::Bong, &rock, 4, &indica), None);
        assert_eq!(
            pack(&catalog, Device::Bong, &one("indica", 1), 4, &load),
            None
        );
        // A rock that carries nothing is no load.
        assert_eq!(Load::of(&catalog, MOON_ROCK, SmokeContents::default), None);
    }

    #[test]
    fn what_packs_as_no_strain_is_no_load() {
        let catalog = catalog();
        for item in [
            "minecraft:wheat",
            "hempdustry:indica_seeds",
            "hempdustry:spliff",
        ] {
            assert_eq!(
                Load::of(&catalog, item, SmokeContents::default),
                None,
                "{item}"
            );
        }
    }
}
