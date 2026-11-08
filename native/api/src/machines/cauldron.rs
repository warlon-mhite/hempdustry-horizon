//! The water cauldron's washes ([`crate::machine::cauldron`]) on a player's click: decarboxylated
//! hemp into washed, hemp stems into retted ones. The cauldron's own use lets an item it has no
//! interaction for through, so the click reaches `use_item_on`.

use horizon_client::mod_api::{Flow, Host, UseItemOnEvent};

use crate::farm::interact::Effect;
use crate::host::Hands;
use crate::machine::cauldron::soak_for;

const WATER_CAULDRON: &str = "minecraft:water_cauldron";
const CAULDRON: &str = "minecraft:cauldron";
/// `Block.UPDATE_ALL`, as `setBlockAndUpdate` writes a cauldron's level.
const UPDATE_ALL: i32 = 3;

/// A click with hemp or stems on a water cauldron: as many as the water covers are soaked, the
/// levels that took are spent (the last one leaves an empty cauldron, `lowerFillLevel`), and the
/// soaked items come back where they fit.
pub(crate) fn use_on(host: &mut dyn Host, event: &UseItemOnEvent<'_>) -> Flow {
    let Some(soak) = soak_for(&event.stack.item) else {
        return Flow::Continue;
    };
    if host.block(event.pos) != WATER_CAULDRON {
        return Flow::Continue;
    }
    let levels = host
        .block_property(event.pos, "level")
        .and_then(|level| level.parse::<u32>().ok())
        .unwrap_or(0);
    let Some(soaked) = soak.soak(event.stack.count, levels) else {
        return Flow::Continue;
    };
    let dimension = host.dimension();
    match levels - soaked.levels {
        0 => {
            if let Some(empty) = host.default_state(CAULDRON) {
                host.set_block(dimension, event.pos, empty, UPDATE_ALL);
            }
        }
        left => {
            host.set_block_property(event.pos, "level", &left.to_string());
        }
    }
    let effects = vec![
        Effect::Exchange {
            count: soaked.count,
            into: soak.output,
        },
        Effect::Sound {
            pos: event.pos,
            sound: "minecraft:item.bucket.empty",
            volume: 1.0,
            pitch: 1.0,
        },
    ];
    Hands { host, dimension }.apply(event.stack, effects);
    Flow::Cancel
}
