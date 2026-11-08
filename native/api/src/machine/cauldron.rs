//! What the mod does with a plain water cauldron (`ModCauldronBehaviors`): it washes decarboxylated
//! hemp (the cannabutter chain's middle step: washed hemp is what makes a Clean or Perfect butter
//! reachable) and rets hemp stems (soaked stalks, which the crafting grid breaks for more fibre).
//!
//! Both are 1:1: a cauldron changes what an item is, it never multiplies one. Water is charged by
//! the amount soaked, not by the click: a cauldron that can't cover the whole stack soaks what it
//! can and leaves the rest in hand. Neither can be automated, as vanilla keeps its own cauldron
//! washes manual.

/// Decarboxylated hemp one level of water rinses: a full cauldron covers a stack of 64.
pub const WASH_PER_LEVEL: u32 = 22;
/// Stems one level rets: soaking a stalk takes more than a rinse.
pub const RET_PER_LEVEL: u32 = 16;

/// What soaking makes of one item, and how many one level of water covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Soak {
    pub output: &'static str,
    pub per_level: u32,
}

/// The soak for `item`, None when the cauldron does nothing with it.
#[must_use]
pub fn soak_for(item: &str) -> Option<Soak> {
    match item {
        "hempdustry:decarboxylated_hemp" => Some(Soak {
            output: "hempdustry:washed_decarboxylated_hemp",
            per_level: WASH_PER_LEVEL,
        }),
        "hempdustry:hemp_stem" => Some(Soak {
            output: "hempdustry:retted_hemp_stem",
            per_level: RET_PER_LEVEL,
        }),
        _ => None,
    }
}

/// What one click soaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct Soaked {
    /// Items soaked, as many outputs made.
    pub count: u32,
    /// Water levels spent.
    pub levels: u32,
}

impl Soak {
    /// One click with `held` items on a cauldron holding `levels` of water: as many as the water
    /// covers, and the levels that took, rounded up. None when there is nothing to soak.
    #[must_use]
    pub fn soak(self, held: u32, levels: u32) -> Option<Soaked> {
        let count = held.min(levels.saturating_mul(self.per_level));
        if count == 0 {
            return None;
        }
        Some(Soaked {
            count,
            levels: levels.min(count.div_ceil(self.per_level)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wash() -> Soak {
        soak_for("hempdustry:decarboxylated_hemp").unwrap_or(Soak {
            output: "",
            per_level: 0,
        })
    }

    #[test]
    fn a_full_cauldron_washes_a_stack_of_64_for_all_three_levels() {
        assert_eq!(
            wash().soak(64, 3),
            Some(Soaked {
                count: 64,
                levels: 3
            })
        );
    }

    #[test]
    fn water_is_charged_by_the_amount_not_the_click() {
        assert_eq!(
            wash().soak(10, 3),
            Some(Soaked {
                count: 10,
                levels: 1
            })
        );
        assert_eq!(
            wash().soak(23, 3),
            Some(Soaked {
                count: 23,
                levels: 2
            })
        );
    }

    #[test]
    fn a_cauldron_short_of_water_soaks_what_it_can() {
        assert_eq!(
            wash().soak(50, 1),
            Some(Soaked {
                count: 22,
                levels: 1
            })
        );
        assert_eq!(wash().soak(50, 0), None);
        assert_eq!(wash().soak(0, 3), None);
    }

    #[test]
    fn stems_ret_sixteen_a_level_and_other_items_are_not_soaked() {
        let ret = soak_for("hempdustry:hemp_stem");
        assert_eq!(
            ret.map(|soak| soak.output),
            Some("hempdustry:retted_hemp_stem")
        );
        assert_eq!(
            ret.and_then(|soak| soak.soak(40, 2)),
            Some(Soaked {
                count: 32,
                levels: 2
            })
        );
        assert_eq!(soak_for("hempdustry:washed_decarboxylated_hemp"), None);
        assert_eq!(soak_for("hempdustry:indica_buds"), None);
    }
}
