//! The world as the farming rules need it. The game's adapter implements it over Horizon's host;
//! the tests implement it over a map of blocks.

use super::model::{Block, LightSource, Pos};

/// The switches of the Java mod's config the farming rules read (`world.cropGrowthMultiplier`,
/// `world.mildew`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// Multiplies the growth odds of every hemp plant. 1.0 grows as the mod ships.
    pub crop_growth_multiplier: f64,
    /// Powdery mildew on or off. Off, a plant that still has it is cleaned on its next tick.
    pub mildew: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            crop_growth_multiplier: 1.0,
            mildew: true,
        }
    }
}

/// A level the farming rules read and write, and the random they draw from.
pub trait World {
    /// The block at `pos`.
    fn block(&self, pos: Pos) -> Block;

    /// Writes one of our blocks (or air) at `pos`, telling clients and neighbours' shapes
    /// (vanilla's `Block.UPDATE_CLIENTS`). False when nothing changed.
    fn set(&mut self, pos: Pos, block: Block) -> bool;

    /// `Level.getRawBrightness(pos, 0)`: the brighter of block and sky light, 0 to 15.
    fn raw_brightness(&self, pos: Pos) -> u8;

    /// What the block at `pos` counts as for a plant below it (`GrowLight.of`).
    fn light_source(&self, pos: Pos) -> LightSource;

    /// Water at `pos`, as `FluidState.is(FluidTags.WATER)`: a water block, or one holding water.
    fn is_water(&self, pos: Pos) -> bool;

    /// The ground at `pos` is in `#hempdustry:beldia_soil` (sand, red sand, any mod's `#c:sands`).
    fn is_beldia_soil(&self, pos: Pos) -> bool;

    /// Something above `pos` that stops movement, glass included, leaves not: the
    /// `MOTION_BLOCKING_NO_LEAVES` heightmap stands above it.
    fn roofed(&self, pos: Pos) -> bool;

    /// Vanilla's sulfur gas reaches the plant whose lower segment is at `pos`.
    fn gassed(&self, pos: Pos) -> bool;

    /// The crop is in `#hempdustry:mildew_resistant`.
    fn mildew_resistant(&self, kind: super::model::Kind) -> bool;

    /// `RandomSource.nextFloat`: 0 inclusive to 1 exclusive.
    fn next_float(&mut self) -> f32;

    /// `RandomSource.nextInt(bound)`: 0 inclusive to `bound` exclusive.
    fn next_int(&mut self, bound: i32) -> i32;

    /// The config the rules read.
    fn config(&self) -> Config {
        Config::default()
    }
}
