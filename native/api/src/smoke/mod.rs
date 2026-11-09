//! Smoking: the spliff, the pipe, the bongs and the vaporizer, as the Java mod plays a hit.
//!
//! Like [`crate::farm`], nothing here knows Horizon: the data comes in as JSON, the random as a
//! [`hit::Dice`], and a hit comes out as a [`hit::Hit`] plan the game's adapter carries out.

pub mod contents;
pub mod data;
pub mod device;
pub mod hit;
pub mod name;
pub mod pack;
pub mod policy;

#[cfg(test)]
mod tests;

pub use contents::SmokeContents;
pub use data::{Catalog, Category, Effect};
pub use device::{Device, Exhale};
pub use hit::{Dice, Hit, Rules, Smoker, Spent, Worn};
pub use policy::Policy;
