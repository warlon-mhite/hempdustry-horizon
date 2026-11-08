//! The smokeables: the spliff, and the devices of `DeviceType` (the pipe, the eighteen bongs,
//! the vaporizer), with what of each stays code in the Java mod.

/// The sixteen dye colours, in `DyeColor` order: each has a glass bong.
const DYES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// The particle a device's exhale shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exhale {
    /// `ParticleTypes.CAMPFIRE_COSY_SMOKE`.
    CampfireSmoke,
    /// `ParticleTypes.CLOUD`.
    Cloud,
}

/// What a hit is taken from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Spliff,
    Pipe,
    Bong,
    Vaporizer,
}

impl Device {
    /// The smokeable `item` is, if it is one.
    #[must_use]
    pub fn of_item(item: &str) -> Option<Self> {
        let path = item.strip_prefix("hempdustry:")?;
        Some(match path {
            "spliff" => Self::Spliff,
            "wooden_pipe" => Self::Pipe,
            "vaporizer" => Self::Vaporizer,
            "bong" | "tinted_bong" => Self::Bong,
            _ if path
                .strip_suffix("_bong")
                .is_some_and(|dye| DYES.contains(&dye)) =>
            {
                Self::Bong
            }
            _ => return None,
        })
    }

    /// Every item that smokes, for the cooldown they share.
    #[must_use]
    pub fn items() -> Vec<String> {
        let mut all: Vec<String> = ["spliff", "wooden_pipe", "vaporizer", "bong", "tinted_bong"]
            .iter()
            .map(|p| format!("hempdustry:{p}"))
            .collect();
        all.extend(DYES.iter().map(|dye| format!("hempdustry:{dye}_bong")));
        all
    }

    /// The device's file in `hempdustry/device/` (all eighteen bongs share one).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Spliff => "hempdustry:spliff",
            Self::Pipe => "hempdustry:wooden_pipe",
            Self::Bong => "hempdustry:bong",
            Self::Vaporizer => "hempdustry:vaporizer",
        }
    }

    /// `DeviceType.maxDamage`; a spliff is used up instead.
    #[must_use]
    pub fn max_damage(self) -> u32 {
        match self {
            Self::Spliff => 0,
            Self::Pipe => 8,
            Self::Bong => 24,
            Self::Vaporizer => 32,
        }
    }

    /// `DeviceType.drawTicks`: the bong is drawn on over time, the others hit at once.
    #[must_use]
    pub fn draw_ticks(self) -> i32 {
        match self {
            Self::Bong => 30,
            _ => 0,
        }
    }

    /// `DeviceType.soundDelayTicks`: the bong's bubbling comes first.
    #[must_use]
    pub fn sound_delay_ticks(self) -> i32 {
        match self {
            Self::Bong => 10,
            _ => 0,
        }
    }

    /// The delay of the smoking sound when the hit lands, once the draw is over.
    #[must_use]
    pub fn sound_delay_at_hit(self) -> i32 {
        (self.sound_delay_ticks() - self.draw_ticks()).max(0)
    }

    #[must_use]
    pub fn exhale(self) -> Exhale {
        match self {
            Self::Vaporizer => Exhale::Cloud,
            _ => Exhale::CampfireSmoke,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bong_is_a_bong_and_nothing_else_is_a_device() {
        assert_eq!(Device::items().len(), 21);
        for item in Device::items() {
            assert!(Device::of_item(&item).is_some(), "{item}");
        }
        assert_eq!(
            Device::of_item("hempdustry:light_blue_bong"),
            Some(Device::Bong)
        );
        for other in [
            "hempdustry:bong_packed",
            "hempdustry:teal_bong",
            "minecraft:bong",
            "hempdustry:indica_buds",
        ] {
            assert_eq!(Device::of_item(other), None, "{other}");
        }
    }
}
