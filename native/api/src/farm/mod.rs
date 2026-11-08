//! Hemp farming: the crops, the beds and the records a plant keeps, as the Java mod plays them.
//!
//! Everything here runs against [`World`] and [`interact::Actor`], never Horizon itself, so the
//! rules are tested on their own (`tests`) and the game's adapter only translates.

pub mod growth;
pub mod harvest;
pub mod interact;
pub mod light;
pub mod medium;
pub mod mildew;
pub mod model;
pub mod plant;
pub mod stress;
pub mod world;

#[cfg(test)]
mod tests;

pub use model::{Block, Condition, Kind, Mildew, Plant, Pos, Pot, Segment, Tray};
pub use world::{Config, World};
