//! The farming rules' [`World`] over a level hook's [`LevelHost`], which reads blocks only as
//! state ids: enough for the questions those hooks must answer at once (may a seed stand here,
//! does this plant take bone meal), told through [`States`].

use horizon_client::mod_api::LevelHost;

use crate::farm::World;
use crate::farm::model::{Block, Kind, LightSource, Pos};
use crate::states::States;

pub struct LevelWorld<'a> {
    pub level: &'a dyn LevelHost,
    pub states: &'a States,
}

impl World for LevelWorld<'_> {
    fn block(&self, pos: Pos) -> Block {
        self.states.block(self.level.block_state(pos))
    }

    fn set(&mut self, _pos: Pos, _block: Block) -> bool {
        false
    }

    fn raw_brightness(&self, _pos: Pos) -> u8 {
        15
    }

    fn light_source(&self, _pos: Pos) -> LightSource {
        LightSource::NONE
    }

    /// Water blocks only: a waterlogged block or a kelp can't be told from its id here.
    fn is_water(&self, pos: Pos) -> bool {
        self.states.is_water(self.level.block_state(pos))
    }

    fn is_beldia_soil(&self, pos: Pos) -> bool {
        self.states.is_soil(self.level.block_state(pos))
    }

    fn roofed(&self, _pos: Pos) -> bool {
        false
    }

    fn gassed(&self, _pos: Pos) -> bool {
        false
    }

    fn mildew_resistant(&self, _kind: Kind) -> bool {
        false
    }

    fn next_float(&mut self) -> f32 {
        0.5
    }

    fn next_int(&mut self, _bound: i32) -> i32 {
        0
    }
}
