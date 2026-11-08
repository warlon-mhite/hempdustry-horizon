//! What the farming rules read and write: the hemp plants, the beds they grow in and the ground
//! around them, as plain values. The block states are the Java mod's, property for property, so a
//! world moves between the two editions with every plant as it was.

/// A block position, `[x, y, z]`.
pub type Pos = [i32; 3];

/// `pos` moved by `(dx, dy, dz)`.
#[must_use]
pub const fn offset(pos: Pos, dx: i32, dy: i32, dz: i32) -> Pos {
    [pos[0] + dx, pos[1] + dy, pos[2] + dz]
}

/// `pos` moved `n` blocks up (down for a negative `n`).
#[must_use]
pub const fn above(pos: Pos, n: i32) -> Pos {
    offset(pos, 0, n, 0)
}

/// The hemp crops. Each is its own block; the strain is the block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Purple Kush, `hempdustry:indica_crop`: two blocks tall.
    Indica,
    /// Lemon Haze, `hempdustry:sativa_crop`: three blocks tall, and slower.
    Sativa,
    /// Beldía, `hempdustry:beldia_crop`: Purple Kush's shape, grown on sand beside water.
    Beldia,
}

impl Kind {
    pub const ALL: [Self; 3] = [Self::Indica, Self::Sativa, Self::Beldia];

    /// The block id.
    #[must_use]
    pub const fn block(self) -> &'static str {
        match self {
            Self::Indica => "hempdustry:indica_crop",
            Self::Sativa => "hempdustry:sativa_crop",
            Self::Beldia => "hempdustry:beldia_crop",
        }
    }

    #[must_use]
    pub fn from_block(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.block() == name)
    }

    /// How many blocks tall the grown plant stands, which is also where its light is read.
    #[must_use]
    pub const fn height(self) -> i32 {
        match self {
            Self::Sativa => 3,
            Self::Indica | Self::Beldia => 2,
        }
    }

    /// The property naming which block of the plant this is: vanilla's `half` on the two-tall
    /// crops, `segment` on the three-tall one.
    #[must_use]
    pub const fn segment_key(self) -> &'static str {
        match self {
            Self::Sativa => "segment",
            Self::Indica | Self::Beldia => "half",
        }
    }

    /// The divisor of the growth roll. Wheat's is 25; Lemon Haze's 35 makes each tick about 40%
    /// less likely to advance it, so it finishes noticeably later than Purple Kush.
    #[must_use]
    pub const fn growth_resistance(self) -> f32 {
        match self {
            Self::Sativa => 35.0,
            Self::Indica | Self::Beldia => 25.0,
        }
    }
}

/// Which block of a plant this is. Two-tall plants use only `Lower` and `Upper`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    Lower,
    Middle,
    Upper,
}

impl Segment {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::Middle => "middle",
            Self::Upper => "upper",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "lower" => Some(Self::Lower),
            "middle" => Some(Self::Middle),
            "upper" => Some(Self::Upper),
            _ => None,
        }
    }
}

/// The light a plant grew under, the `condition` property: a record the plant keeps, never a
/// reading of the lamp at harvest. In tier order, weakest first; [`Condition::after_step`] relies
/// on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Condition {
    /// Sun, torches, or nothing counted: the baseline.
    Natural,
    /// Any other block at light 15: glowstone, sea lantern, shroomlight, froglights, lanterns.
    Ambient,
    /// A block at light 15 that is switched on: a redstone lamp, a copper bulb.
    Lamp,
    /// `#hempdustry:grow_lamps`: the Grow Lamp, and whatever a pack adds.
    GrowLamp,
    /// Lost its lamp while flowering, or stressed some other way. Harvests a bud short and seeded.
    Stressed,
}

impl Condition {
    pub const KEY: &'static str = "condition";
    pub const ALL: [Self; 5] = [
        Self::Natural,
        Self::Ambient,
        Self::Lamp,
        Self::GrowLamp,
        Self::Stressed,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Natural => "natural",
            Self::Ambient => "ambient",
            Self::Lamp => "lamp",
            Self::GrowLamp => "grow_lamp",
            Self::Stressed => "stressed",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == value)
    }

    /// Anything but [`Condition::Natural`]: a plant that grew under a light, or one that was
    /// stressed. Charas rubs at the lamp's odds off all of them.
    #[must_use]
    pub const fn artificial(self) -> bool {
        !matches!(self, Self::Natural)
    }
}

/// Powdery mildew, the `mildew` property: a record the plant keeps, in order of harm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mildew {
    Clean,
    /// Reached by vanilla's sulfur gas while flowering: each bud a little likelier to be schwag.
    Sulfured,
    /// White powder on the leaves and buds: each bud much likelier to be schwag.
    Mildewed,
}

impl Mildew {
    pub const KEY: &'static str = "mildew";
    pub const ALL: [Self; 3] = [Self::Clean, Self::Sulfured, Self::Mildewed];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Sulfured => "sulfured",
            Self::Mildewed => "mildewed",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == value)
    }
}

/// One block of a hemp plant: every property its block state carries.
///
/// The age, the trims and the records are canonical on the lower segment; the blocks above mirror
/// the age and copy the records, because whatever draws a block reads only that block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Plant {
    pub kind: Kind,
    /// 0 to [`Plant::MAX_AGE`].
    pub age: u8,
    pub segment: Segment,
    pub trimmed_early: bool,
    pub trimmed_late: bool,
    pub rubbed: bool,
    pub condition: Condition,
    pub mildew: Mildew,
}

impl Plant {
    pub const MAX_AGE: u8 = 7;
    /// The age a plant starts flowering at: the age both crops first stand two blocks tall.
    pub const FLOWERING_AGE: u8 = 4;
    /// From this age a plant wants its second block.
    pub const TWO_TALL_AGE: u8 = 4;
    /// From this age Lemon Haze wants its third.
    pub const THREE_TALL_AGE: u8 = 6;

    /// The block's default state with `age` and `segment`: unworked, natural light, clean. The
    /// booleans are spelled out, as the Java mod's `Defoliation.unworked` does: a boolean property
    /// left out of a default state reads true.
    #[must_use]
    pub const fn new(kind: Kind, age: u8, segment: Segment) -> Self {
        Self {
            kind,
            age,
            segment,
            trimmed_early: false,
            trimmed_late: false,
            rubbed: false,
            condition: Condition::Natural,
            mildew: Mildew::Clean,
        }
    }

    /// A block above the lower one at `age`, carrying a copy of `lower`'s records.
    #[must_use]
    pub const fn segment_of(lower: Self, age: u8, segment: Segment) -> Self {
        let mut plant = Self::new(lower.kind, age, segment);
        plant.condition = lower.condition;
        plant.mildew = lower.mildew;
        plant
    }

    /// `to` with `self`'s trims and records: what every rewrite of a lower segment must carry
    /// across (`Defoliation.carryOver`).
    #[must_use]
    pub const fn carry_onto(&self, mut to: Self) -> Self {
        to.trimmed_early = self.trimmed_early;
        to.trimmed_late = self.trimmed_late;
        to.rubbed = self.rubbed;
        to.condition = self.condition;
        to.mildew = self.mildew;
        to
    }

    #[must_use]
    pub const fn is_lower(&self) -> bool {
        matches!(self.segment, Segment::Lower)
    }

    #[must_use]
    pub const fn is_ripe(&self) -> bool {
        self.age >= Self::MAX_AGE
    }

    #[must_use]
    pub const fn is_flowering(&self) -> bool {
        self.age >= Self::FLOWERING_AGE
    }

    /// How many of the two trims the plant has had: the rub moves no buds and isn't counted.
    #[must_use]
    pub const fn cut_count(&self) -> u8 {
        self.trimmed_early as u8 + self.trimmed_late as u8
    }

    /// Every property as the block state spells it, in the order the block declares them.
    #[must_use]
    pub fn properties(&self) -> [(&'static str, String); 7] {
        [
            ("age", self.age.to_string()),
            (self.kind.segment_key(), self.segment.name().to_owned()),
            ("trimmed_early", self.trimmed_early.to_string()),
            ("trimmed_late", self.trimmed_late.to_string()),
            ("rubbed", self.rubbed.to_string()),
            (Condition::KEY, self.condition.name().to_owned()),
            (Mildew::KEY, self.mildew.name().to_owned()),
        ]
    }

    /// Reads a plant back from its properties, `get` naming each value. None for a block that
    /// isn't a hemp crop or a value out of range.
    pub fn read(kind: Kind, get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let flag = |key: &str| get(key).map(|v| v == "true");
        let age: u8 = get("age")?.parse().ok()?;
        let segment = Segment::parse(&get(kind.segment_key())?)?;
        if age > Self::MAX_AGE || (kind.height() == 2 && segment == Segment::Middle) {
            return None;
        }
        Some(Self {
            kind,
            age,
            segment,
            trimmed_early: flag("trimmed_early")?,
            trimmed_late: flag("trimmed_late")?,
            rubbed: flag("rubbed")?,
            condition: Condition::parse(&get(Condition::KEY)?)?,
            mildew: Mildew::parse(&get(Mildew::KEY)?)?,
        })
    }
}

/// The Grow Pot, `hempdustry:grow_pot`: soil that holds food well and water badly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pot {
    /// Bone meal in the soil, 0 to [`Pot::MAX_FERTILITY`].
    pub fertility: u8,
    /// One bottle of water, drunk by the plant that ripens.
    pub watered: bool,
}

impl Pot {
    pub const BLOCK: &'static str = "hempdustry:grow_pot";
    pub const MAX_FERTILITY: u8 = 3;
}

/// The Hydro Tray, `hempdustry:hydro_tray`: a deep-water-culture reservoir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tray {
    /// Water in the reservoir, 0 to [`Tray::MAX_LEVEL`]; one level is drunk per plant that ripens.
    pub level: u8,
    /// Nutrients mixed into that water.
    pub fed: bool,
    /// The air pump, run by redstone.
    pub powered: bool,
}

impl Tray {
    pub const BLOCK: &'static str = "hempdustry:hydro_tray";
    pub const MAX_LEVEL: u8 = 3;
}

/// A block as the farming rules see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Block {
    /// Air, cave air or void air.
    Air,
    Plant(Plant),
    Pot(Pot),
    Tray(Tray),
    /// Farmland (`#minecraft:grows_crops` on the ground), with its moisture 0 to 7.
    Farmland {
        moisture: u8,
    },
    /// Anything else.
    Other,
}

impl Block {
    #[must_use]
    pub const fn plant(&self) -> Option<&Plant> {
        match self {
            Self::Plant(plant) => Some(plant),
            _ => None,
        }
    }

    /// The same crop block as `kind`, any segment.
    #[must_use]
    pub fn is_crop(&self, kind: Kind) -> bool {
        self.plant().is_some_and(|p| p.kind == kind)
    }

    /// A bed of ours, which answers the growth roll's questions for itself.
    #[must_use]
    pub const fn is_medium(&self) -> bool {
        matches!(self, Self::Pot(_) | Self::Tray(_))
    }
}

/// What a light above a plant counts as (`GrowLight.of`), and whether the block stops light
/// walking further up (`BlockState.isSolidRender`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightSource {
    pub tier: Condition,
    pub opaque: bool,
}

impl LightSource {
    /// Air, glass, a plant: no light of its own, and light passes.
    pub const NONE: Self = Self {
        tier: Condition::Natural,
        opaque: false,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plant_reads_back_from_the_properties_it_writes() {
        for kind in Kind::ALL {
            let mut plant = Plant::new(kind, 5, Segment::Upper);
            plant.trimmed_late = true;
            plant.condition = Condition::GrowLamp;
            plant.mildew = Mildew::Sulfured;
            let props = plant.properties();
            let get = |key: &str| {
                props
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.clone())
            };
            assert_eq!(Plant::read(kind, get), Some(plant));
        }
    }

    #[test]
    fn a_two_tall_plant_has_no_middle() {
        let props = Plant::new(Kind::Sativa, 6, Segment::Middle).properties();
        let get = |key: &str| {
            let key = if key == "half" { "segment" } else { key };
            props
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(Plant::read(Kind::Indica, get), None);
    }

    #[test]
    fn a_new_plant_is_unworked() {
        let plant = Plant::new(Kind::Indica, 0, Segment::Lower);
        assert!(!plant.trimmed_early && !plant.trimmed_late && !plant.rubbed);
        assert_eq!(
            (plant.condition, plant.mildew),
            (Condition::Natural, Mildew::Clean)
        );
    }
}
