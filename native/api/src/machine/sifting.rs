//! The Sifting Box (`SiftingBoxBlock`): a screened tub that separates trichome heads from plant
//! matter, as a composter fills. A vessel, not a machine: no block entity, only its state
//! (`level` 0–8, `filled`, `content`).
//!
//! Dry, buds and leaf go over the screen into nine kief, and kief re-sifts into nine filtered
//! kief. Filled with water and jacketed in ice on all four sides, the same plant matter washes
//! into nine bubble hash, at the jacket's rate: at best equal to dry, never better. Plant matter
//! and powder never share a screen; a wash takes plant matter only.
//!
//! No Horizon host here: the glue says what the player holds, what the four sides are, and rolls
//! the dice.

/// The level at which the screen is full and the batch settles.
pub const FULL_LEVEL: u8 = 7;
/// The level at which there is a batch to take out.
pub const READY_LEVEL: u8 = 8;
/// Ticks between "the screen filled up" and "there is something in it": the composter's delay.
pub const SETTLE_DELAY: i64 = 20;
/// What one batch hands over: nine, because a bar is nine.
pub const YIELD: u32 = 9;

/// The chance one fan leaf moves the screen one level: about 47 leaves a batch.
pub const TRIM_CHANCE: f32 = 0.15;
/// One bud moves the screen one level for certain: 7 buds a batch.
pub const FLOWER_CHANCE: f32 = 1.0;
/// One resinous bud (Beldía) moves it two levels: 4 buds a batch.
pub const RESINOUS_CHANCE: f32 = 2.0;
/// The chance one kief moves a re-sift one level: 28 kief a batch, the dry road's whole cost.
pub const KIEF_CHANCE: f32 = 0.25;

/// Blue ice: parity with dry sifting.
pub const BLUE_ICE_RATE: f32 = 1.0;
/// Packed ice: about 9 buds a batch.
pub const PACKED_ICE_RATE: f32 = 0.75;
/// Plain ice (and frosted ice): about 14 buds a batch, and it melts in the light.
pub const ICE_RATE: f32 = 0.5;

/// The item tags the screen reads, in the order Java asks them.
pub const RESINOUS_TAG: &str = "hempdustry:siftable/resinous";
pub const FLOWER_TAG: &str = "hempdustry:siftable/flower";
pub const TRIM_TAG: &str = "hempdustry:siftable/trim";
pub const KIEF_TAG: &str = "hempdustry:siftable/kief";

/// What is on the screen: plant matter and powder never share one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Content {
    #[default]
    Plant,
    Kief,
}

impl Content {
    /// The blockstate value.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plant => "plant",
            Self::Kief => "kief",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "plant" => Some(Self::Plant),
            "kief" => Some(Self::Kief),
            _ => None,
        }
    }
}

/// The box's blockstate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Screen {
    /// 0–8, as a composter's.
    pub level: u8,
    /// Water in the tub: which of the two processes runs. Spent when the batch is taken out.
    pub filled: bool,
    pub content: Content,
}

/// Something the screen takes, by the tag it is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Siftable {
    Resinous,
    Flower,
    Trim,
    Kief,
}

impl Siftable {
    /// Every kind, in the order its tag is asked (`sieveChance`): an item in two tags is the first.
    pub const ALL: [Self; 4] = [Self::Resinous, Self::Flower, Self::Trim, Self::Kief];

    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Resinous => RESINOUS_TAG,
            Self::Flower => FLOWER_TAG,
            Self::Trim => TRIM_TAG,
            Self::Kief => KIEF_TAG,
        }
    }

    /// How far one of it moves the screen at full rate.
    #[must_use]
    pub const fn chance(self) -> f32 {
        match self {
            Self::Resinous => RESINOUS_CHANCE,
            Self::Flower => FLOWER_CHANCE,
            Self::Trim => TRIM_CHANCE,
            Self::Kief => KIEF_CHANCE,
        }
    }

    /// Which screen it belongs on.
    #[must_use]
    pub const fn content(self) -> Content {
        match self {
            Self::Resinous | Self::Flower | Self::Trim => Content::Plant,
            Self::Kief => Content::Kief,
        }
    }
}

/// An ice block of the jacket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ice {
    Blue,
    Packed,
    /// Ice, or frosted ice: Frost Walker keeps a jacket alive while its wearer stands there.
    Plain,
}

impl Ice {
    /// The ice a block is, by id.
    #[must_use]
    pub fn of(block: &str) -> Option<Self> {
        match block {
            "minecraft:blue_ice" => Some(Self::Blue),
            "minecraft:packed_ice" => Some(Self::Packed),
            "minecraft:ice" | "minecraft:frosted_ice" => Some(Self::Plain),
            _ => None,
        }
    }

    #[must_use]
    pub const fn rate(self) -> f32 {
        match self {
            Self::Blue => BLUE_ICE_RATE,
            Self::Packed => PACKED_ICE_RATE,
            Self::Plain => ICE_RATE,
        }
    }
}

/// The jacket's rate (`jacketRate`): the weakest of the four sides, 0 when any side isn't ice.
#[must_use]
pub fn jacket_rate(sides: [Option<Ice>; 4]) -> f32 {
    sides
        .into_iter()
        .try_fold(BLUE_ICE_RATE, |worst, side| {
            side.map(|ice| worst.min(ice.rate()))
        })
        .unwrap_or(0.0)
}

impl Screen {
    /// The rate this box runs at (`rate`): 1 dry, the jacket's when wet (0 without one).
    #[must_use]
    pub fn rate(self, jacket: f32) -> f32 {
        if self.filled { jacket } else { 1.0 }
    }

    /// A batch is waiting.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        self.level == READY_LEVEL
    }

    /// The full screen settles (`tick`): `FULL_LEVEL` to `READY_LEVEL`, None otherwise.
    #[must_use]
    pub const fn settled(self) -> Option<Self> {
        if self.level == FULL_LEVEL {
            Some(Self {
                level: READY_LEVEL,
                ..self
            })
        } else {
            None
        }
    }
}

/// What the player holds, as the box asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    WaterBucket,
    Siftable(Siftable),
    /// Anything else, an empty hand included.
    Other,
}

/// What a click with an item does (`useItemOn`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub enum Click {
    /// A water bucket into an empty dry box: it fills, the empty bucket comes back.
    Fill,
    /// A water bucket the box turns down: nothing happens, and the bucket isn't poured on top of
    /// the box either (Java's `CONSUME`).
    Hold,
    /// One item onto the screen at `chance`, the screen stamped with its content when empty.
    Sift { screen: Screen, chance: f32 },
    /// Not taken (Java's `super`): a ready box still gives its batch to the main hand, else the
    /// item does what it does. `tell` names the action-bar line, for the two rules a player can't
    /// see.
    Refuse { tell: Option<&'static str> },
}

pub const NEEDS_ICE: &str = "hempdustry.sifting_box.needs_ice";
pub const NO_PLANT_IN_KIEF: &str = "hempdustry.sifting_box.no_plant_in_kief";
pub const NO_KIEF_IN_PLANT: &str = "hempdustry.sifting_box.no_kief_in_plant";

/// A click on `screen` holding `held`, with the four sides' jacket at `jacket`.
pub fn click(screen: Screen, held: Held, jacket: f32) -> Click {
    let level = screen.level;
    let siftable = match held {
        // Only into an empty box: a half-sifted dry batch must not turn into a wash.
        Held::WaterBucket if !screen.filled && level == 0 => return Click::Fill,
        // A ready box falls through, so the bucket's holder can empty it.
        Held::WaterBucket if level != READY_LEVEL => return Click::Hold,
        Held::WaterBucket | Held::Other => return Click::Refuse { tell: None },
        Held::Siftable(siftable) => siftable,
    };
    let content = siftable.content();
    let chance = siftable.chance() * screen.rate(jacket);
    let refused = level >= FULL_LEVEL
        || chance <= 0.0
        || (screen.filled && content != Content::Plant)
        || (level > 0 && content != screen.content);
    if refused {
        return Click::Refuse {
            tell: refusal_line(screen, content),
        };
    }
    let screen = if level == 0 {
        Screen { content, ..screen }
    } else {
        screen
    };
    Click::Sift { screen, chance }
}

/// `explainRefusal`: why the box turned down something it would sift, when the reason can't be
/// seen. A full screen says nothing, as a composter.
fn refusal_line(screen: Screen, content: Content) -> Option<&'static str> {
    if screen.level >= FULL_LEVEL {
        return None;
    }
    if screen.filled {
        return (content == Content::Plant).then_some(NEEDS_ICE);
    }
    if screen.level > 0 && content != screen.content {
        return Some(match content {
            Content::Plant => NO_PLANT_IN_KIEF,
            Content::Kief => NO_KIEF_IN_PLANT,
        });
    }
    None
}

/// The levels one item moves the screen: the whole part of `chance` for certain and the fraction
/// as a roll, `u` the roll (`nextFloat`, 0 to 1).
#[must_use]
pub fn steps(chance: f32, u: f32) -> u8 {
    let whole = chance.trunc();
    let extra = u8::from(u < chance - whole);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a chance is between 0 and 2, so its whole part fits a level"
    )]
    let whole = whole.clamp(0.0, f32::from(READY_LEVEL)) as u8;
    whole + extra
}

/// What one item did to the screen (`sift`), for the glue's write and sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum Sifted {
    /// The screen rose to this; at `FULL_LEVEL` it now settles.
    Rose(Screen),
    /// The roll moved nothing: the item is spent all the same, and the box is left as it was
    /// (an empty one isn't stamped).
    Missed,
}

/// One item onto `screen` at `chance`, `u` the roll. A level past full is lost, as in a composter.
pub fn sift(screen: Screen, chance: f32, u: f32) -> Sifted {
    let steps = steps(chance, u);
    if steps == 0 {
        return Sifted::Missed;
    }
    Sifted::Rose(Screen {
        level: FULL_LEVEL.min(screen.level.saturating_add(steps)),
        ..screen
    })
}

/// What a batch is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Batch {
    /// A dry sift of plant matter.
    Kief,
    /// An ice-water wash.
    BubbleHash,
    /// A dry re-sift of kief.
    FilteredKief,
}

impl Batch {
    #[must_use]
    pub const fn item(self) -> &'static str {
        match self {
            Self::Kief => "hempdustry:kief",
            Self::BubbleHash => "hempdustry:bubble_hash",
            Self::FilteredKief => "hempdustry:filtered_kief",
        }
    }
}

/// Taking a ready batch out (`collect`): the batch, and the box empty and dry again.
#[must_use]
pub fn collect(screen: Screen) -> Option<(Batch, Screen)> {
    if !screen.is_ready() {
        return None;
    }
    let batch = match (screen.content, screen.filled) {
        // A wash never holds powder.
        (Content::Kief, _) => Batch::FilteredKief,
        (Content::Plant, true) => Batch::BubbleHash,
        (Content::Plant, false) => Batch::Kief,
    };
    Some((batch, Screen::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRY: Screen = Screen {
        level: 0,
        filled: false,
        content: Content::Plant,
    };
    const WET: Screen = Screen {
        filled: true,
        ..DRY
    };
    const BLUE: [Option<Ice>; 4] = [Some(Ice::Blue); 4];

    fn sift_with(before: Screen, held: Siftable, jacket: f32, u: f32) -> Screen {
        match click(before, Held::Siftable(held), jacket) {
            Click::Sift { screen, chance } => match sift(screen, chance, u) {
                Sifted::Rose(s) => s,
                Sifted::Missed => before,
            },
            other => panic!("not sifted: {other:?}"),
        }
    }

    #[test]
    fn seven_buds_fill_a_dry_screen_and_it_settles_into_nine_kief() {
        let mut screen = DRY;
        for _ in 0..7 {
            screen = sift_with(screen, Siftable::Flower, 0.0, 0.99);
        }
        assert_eq!(screen.level, FULL_LEVEL);
        assert_eq!(
            click(screen, Held::Siftable(Siftable::Flower), 0.0),
            Click::Refuse { tell: None }
        );
        assert_eq!(collect(screen), None);
        let ready = screen.settled().expect("a full screen settles");
        assert_eq!(collect(ready), Some((Batch::Kief, Screen::default())));
        assert_eq!(YIELD, 9);
    }

    #[test]
    fn a_resinous_bud_is_two_levels_and_four_fill_a_batch() {
        let mut screen = DRY;
        for _ in 0..4 {
            screen = sift_with(screen, Siftable::Resinous, 0.0, 0.99);
        }
        assert_eq!(screen.level, FULL_LEVEL);
    }

    #[test]
    fn a_roll_moves_the_fraction_and_the_whole_part_is_certain() {
        assert_eq!(steps(TRIM_CHANCE, 0.14), 1);
        assert_eq!(steps(TRIM_CHANCE, 0.15), 0);
        assert_eq!(steps(FLOWER_CHANCE, 0.999), 1);
        // A resinous bud in a packed-ice wash: one level and a coin flip on a second.
        let chance = RESINOUS_CHANCE * PACKED_ICE_RATE;
        assert_eq!(steps(chance, 0.49), 2);
        assert_eq!(steps(chance, 0.5), 1);
        assert_eq!(steps(0.0, 0.0), 0);
    }

    #[test]
    fn a_missed_leaf_leaves_the_box_as_it_was() {
        // A missed kief leaves an empty box a plant screen: nothing was written.
        let after = sift_with(DRY, Siftable::Kief, 0.0, 0.9);
        assert_eq!(after, DRY);
        assert!(matches!(
            click(DRY, Held::Siftable(Siftable::Trim), 0.0),
            Click::Sift { .. }
        ));
    }

    #[test]
    fn a_water_bucket_fills_only_an_empty_dry_box() {
        assert_eq!(click(DRY, Held::WaterBucket, 0.0), Click::Fill);
        let part = Screen { level: 3, ..DRY };
        assert_eq!(click(part, Held::WaterBucket, 0.0), Click::Hold);
        assert_eq!(click(WET, Held::WaterBucket, 0.0), Click::Hold);
        // A ready box lets the bucket's holder take the batch.
        let ready = Screen {
            level: READY_LEVEL,
            ..DRY
        };
        assert_eq!(
            click(ready, Held::WaterBucket, 0.0),
            Click::Refuse { tell: None }
        );
    }

    #[test]
    fn a_wash_needs_its_jacket_and_says_so() {
        assert_eq!(
            click(WET, Held::Siftable(Siftable::Flower), 0.0),
            Click::Refuse {
                tell: Some(NEEDS_ICE)
            }
        );
        let mut screen = WET;
        for _ in 0..7 {
            screen = sift_with(screen, Siftable::Flower, jacket_rate(BLUE), 0.99);
        }
        let ready = screen.settled().expect("full");
        assert_eq!(collect(ready), Some((Batch::BubbleHash, Screen::default())));
    }

    #[test]
    fn a_wash_takes_plant_matter_only_and_quietly() {
        assert_eq!(
            click(WET, Held::Siftable(Siftable::Kief), 1.0),
            Click::Refuse { tell: None }
        );
    }

    #[test]
    fn plant_and_powder_never_share_a_dry_screen() {
        let plant = Screen { level: 2, ..DRY };
        assert_eq!(
            click(plant, Held::Siftable(Siftable::Kief), 0.0),
            Click::Refuse {
                tell: Some(NO_KIEF_IN_PLANT)
            }
        );
        let kief = Screen {
            level: 2,
            content: Content::Kief,
            ..DRY
        };
        assert_eq!(
            click(kief, Held::Siftable(Siftable::Trim), 0.0),
            Click::Refuse {
                tell: Some(NO_PLANT_IN_KIEF)
            }
        );
        // Buds and leaf mix freely.
        assert!(matches!(
            click(plant, Held::Siftable(Siftable::Trim), 0.0),
            Click::Sift { .. }
        ));
    }

    #[test]
    fn kief_re_sifts_into_filtered_kief() {
        let screen = sift_with(DRY, Siftable::Kief, 0.0, 0.1);
        assert_eq!(
            screen,
            Screen {
                level: 1,
                content: Content::Kief,
                ..DRY
            }
        );
        let ready = Screen {
            level: READY_LEVEL,
            ..screen
        };
        assert_eq!(
            collect(ready),
            Some((Batch::FilteredKief, Screen::default()))
        );
    }

    #[test]
    fn the_jacket_is_its_weakest_side_and_nothing_with_a_gap() {
        assert!((jacket_rate(BLUE) - BLUE_ICE_RATE).abs() < f32::EPSILON);
        let mixed = [
            Some(Ice::Blue),
            Some(Ice::Packed),
            Some(Ice::Blue),
            Some(Ice::Plain),
        ];
        assert!((jacket_rate(mixed) - ICE_RATE).abs() < f32::EPSILON);
        let gap = [Some(Ice::Blue), None, Some(Ice::Blue), Some(Ice::Blue)];
        assert!(jacket_rate(gap).abs() < f32::EPSILON);
        assert_eq!(Ice::of("minecraft:frosted_ice"), Some(Ice::Plain));
        assert_eq!(Ice::of("minecraft:snow_block"), None);
        // Dry ignores the jacket.
        assert!((DRY.rate(0.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_level_past_full_is_lost() {
        let six = Screen { level: 6, ..DRY };
        assert_eq!(
            sift_with(six, Siftable::Resinous, 0.0, 0.99).level,
            FULL_LEVEL
        );
    }
}
