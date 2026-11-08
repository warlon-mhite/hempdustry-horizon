//! Right-clicks with an item on a plant or a bed: trimming and rubbing with shears
//! (`Defoliation.tryCut`), a water bottle into a pot, a water bucket into a tray, and fertiliser
//! into either (`GrowMedium.tryFeed`).
//!
//! The world changes as the click is played; what it does to the player and around them comes
//! back as a list of [`Effect`]s for the game to carry out.

use std::borrow::Cow;

use super::medium::Medium;
use super::mildew;
use super::model::{Block, Condition, Mildew, Plant, Pos, Pot, Tray, above};
use super::plant;
use super::stress;
use super::world::World;

/// What the player holds, as far as these rules care.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// Anything in `#c:tools/shear`.
    Shears,
    /// A potion of water.
    WaterBottle,
    WaterBucket,
    /// Anything in `#c:fertilizers`: bone meal, and whatever a pack tags.
    Fertilizer,
    Other,
}

/// Something a click does besides the blocks it writes.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// One durability point off the held tool.
    DamageHeld,
    /// One of the held stack used up.
    ConsumeHeld,
    /// The held stack emptied (`ItemUtils.createFilledResult`): a water bottle into a glass
    /// bottle, a water bucket into a bucket, a milk into what it leaves.
    EmptyHeld(Cow<'static, str>),
    /// `count` of the held stack soaked into as many `into`, a cauldron's wash: they come back
    /// where they fit (`placeItemBackInInventory`). A creative player spends nothing and gets one
    /// if they have none.
    Exchange { count: u32, into: &'static str },
    /// `count` of `item` dropped at `pos` (`Block.popResource`).
    Drop {
        pos: Pos,
        item: &'static str,
        count: u32,
    },
    /// A sound at `pos` for everyone near.
    Sound {
        pos: Pos,
        sound: &'static str,
        volume: f32,
        pitch: f32,
    },
    /// A line on the player's action bar, by translation key.
    Tell(&'static str),
    /// Vanilla's "that was fertilised" sparks and sound (level event 1505).
    GrowthSparks(Pos),
    /// Five splashes over `pos`, as a bottle poured onto dirt.
    Splash(Pos),
}

/// The late-vegetative trim's age, the last age the plant is one block tall.
pub const EARLY_AGE: u8 = 3;
/// The early-flowering trim, after the stretch into two blocks.
pub const LATE_AGES: std::ops::RangeInclusive<u8> = 4..=5;
/// Ripe enough to rub for resin.
pub const RUB_AGES: std::ops::RangeInclusive<u8> = 6..=7;
/// Odds, as one in N, that a rub yields charas: once per plant, so about four plants a pinch.
pub const CHARAS_ONE_IN: i32 = 4;
/// The rub's odds on a plant that grew under any light but the sun, or was stressed.
pub const CHARAS_ONE_IN_UNDER_LIGHTS: i32 = 8;

pub const LEAF: &str = "hempdustry:hemp_leaf";
pub const CHARAS: &str = "hempdustry:charas";
const PICK: &str = "minecraft:block.sweet_berry_bush.pick_berries";
const HONEY: &str = "minecraft:block.honey_block.break";

/// A click with `held` on the block at `pos`. True when the click was taken (the hand swings and
/// the item's own use doesn't run); false lets the item do what it does. `boils_away` says water
/// evaporates here (the Nether).
pub fn use_item_on(
    world: &mut dyn World,
    held: Held,
    pos: Pos,
    boils_away: bool,
    out: &mut Vec<Effect>,
) -> bool {
    match world.block(pos) {
        Block::Plant(_) => use_on_plant(world, held, pos, out),
        Block::Pot(pot) => use_on_pot(world, held, pos, pot, out),
        Block::Tray(tray) => use_on_tray(world, held, pos, tray, boils_away, out),
        _ => false,
    }
}

fn use_on_plant(world: &mut dyn World, held: Held, pos: Pos, out: &mut Vec<Effect>) -> bool {
    let Some((lower_pos, lower)) = plant::root(world, pos) else {
        return false;
    };
    // A bottle on the plant waters its pot: that is where a player aims.
    if held == Held::WaterBottle && water_pot(world, above(lower_pos, -1), out) {
        return true;
    }
    held == Held::Shears && cut(world, lower_pos, lower, out)
}

/// Shears on the plant whose lower segment `lower` is at `lower_pos`: three windows, one leaf
/// each, and the window's flag is what closes it. Ages 6 and 7 rub for resin instead of trimming.
fn cut(world: &mut dyn World, lower_pos: Pos, lower: Plant, out: &mut Vec<Effect>) -> bool {
    let age = lower.age;
    let (spent, rub) = if age == EARLY_AGE {
        (lower.trimmed_early, false)
    } else if LATE_AGES.contains(&age) {
        (lower.trimmed_late, false)
    } else if RUB_AGES.contains(&age) {
        (lower.rubbed, true)
    } else {
        out.push(Effect::Tell("hempdustry.trim.too_young"));
        return false;
    };
    if spent {
        out.push(Effect::Tell(if rub {
            "hempdustry.trim.already_rubbed"
        } else {
            "hempdustry.trim.already"
        }));
        return false;
    }
    // The flag first, then the leaf: the flag is what the loot table reads to take this leaf back
    // off the harvest, so the leaf is only ever handed over together with it.
    let mut marked = lower;
    if age == EARLY_AGE {
        marked.trimmed_early = true;
    } else if rub {
        marked.rubbed = true;
    } else {
        marked.trimmed_late = true;
    }
    world.set(lower_pos, Block::Plant(marked));
    // A trim of a mildewed plant takes the diseased leaves away, and no leaf is handed over.
    let cure = !rub && lower.mildew == Mildew::Mildewed;
    if cure {
        mildew::write(world, lower_pos, Mildew::Clean);
    }
    // A Grow Lamp plant has leaf to spare: a trim of one hands over a second leaf.
    let leaves = match (cure, rub, lower.condition) {
        (true, _, _) => 0,
        (false, false, Condition::GrowLamp) => 2,
        _ => 1,
    };
    if leaves > 0 {
        out.push(Effect::Drop {
            pos: lower_pos,
            item: LEAF,
            count: leaves,
        });
    }
    if rub {
        let one_in = if lower.condition.artificial() {
            CHARAS_ONE_IN_UNDER_LIGHTS
        } else {
            CHARAS_ONE_IN
        };
        let charas = world.next_int(one_in) == 0;
        if charas {
            out.push(Effect::Drop {
                pos: lower_pos,
                item: CHARAS,
                count: 1,
            });
        }
        // Honey is the mod's resin sound, so a rub that pays out sounds unlike one that doesn't.
        let pitch = if charas { 0.7 } else { 0.6 } + world.next_float() * 0.2;
        let (sound, volume) = if charas { (HONEY, 0.8) } else { (PICK, 1.0) };
        out.push(Effect::Sound {
            pos: lower_pos,
            sound,
            volume,
            pitch,
        });
    } else {
        let pitch = 0.8 + world.next_float() * 0.4;
        out.push(Effect::Sound {
            pos: lower_pos,
            sound: PICK,
            volume: 1.0,
            pitch,
        });
    }
    out.push(Effect::DamageHeld);
    true
}

/// Pours a water bottle into the pot at `pos`, if there is one: a dry pot is watered, a wet one
/// overwatered, which may stress a flowering plant in it. Either way the water is poured.
fn water_pot(world: &mut dyn World, pos: Pos, out: &mut Vec<Effect>) -> bool {
    let Block::Pot(pot) = world.block(pos) else {
        return false;
    };
    if pot.watered {
        stress::roll(world, above(pos, 1), stress::OVERWATERED);
    } else {
        world.set(
            pos,
            Block::Pot(Pot {
                watered: true,
                ..pot
            }),
        );
    }
    out.push(Effect::Sound {
        pos,
        sound: "minecraft:item.bottle.empty",
        volume: 1.0,
        pitch: 1.0,
    });
    out.push(Effect::Splash(pos));
    out.push(Effect::EmptyHeld(Cow::Borrowed("minecraft:glass_bottle")));
    true
}

fn use_on_pot(
    world: &mut dyn World,
    held: Held,
    pos: Pos,
    pot: Pot,
    out: &mut Vec<Effect>,
) -> bool {
    match held {
        // A bucket is the tray's tool: taken and ignored, so it doesn't pour into the plant's space.
        Held::WaterBucket => true,
        Held::WaterBottle => water_pot(world, pos, out),
        Held::Fertilizer => feed(world, pos, Medium::Pot(pot), false, out),
        Held::Shears | Held::Other => false,
    }
}

fn use_on_tray(
    world: &mut dyn World,
    held: Held,
    pos: Pos,
    tray: Tray,
    boils_away: bool,
    out: &mut Vec<Effect>,
) -> bool {
    match held {
        // Where water boils away the bucket goes through, so vanilla pours it and hisses it away.
        Held::WaterBucket if boils_away => false,
        Held::WaterBucket if tray.level < Tray::MAX_LEVEL => {
            world.set(
                pos,
                Block::Tray(Tray {
                    level: Tray::MAX_LEVEL,
                    ..tray
                }),
            );
            out.push(Effect::Sound {
                pos,
                sound: "minecraft:item.bucket.empty",
                volume: 1.0,
                pitch: 1.0,
            });
            out.push(Effect::EmptyHeld(Cow::Borrowed("minecraft:bucket")));
            true
        }
        // A full tray refuses the bucket by taking the click and doing nothing.
        Held::WaterBucket => true,
        Held::Fertilizer => feed(world, pos, Medium::Tray(tray), boils_away, out),
        Held::WaterBottle | Held::Shears | Held::Other => false,
    }
}

/// One charge of food from any fertiliser, whatever it was: a pack's fancier fertiliser is an
/// alternative, never an upgrade. A bed that can't take it lets the click through.
fn feed(
    world: &mut dyn World,
    pos: Pos,
    medium: Medium,
    boils_away: bool,
    out: &mut Vec<Effect>,
) -> bool {
    if !medium.takes_food(boils_away) {
        return false;
    }
    world.set(pos, medium.fed_once().block());
    out.push(Effect::ConsumeHeld);
    out.push(Effect::GrowthSparks(pos));
    true
}
