//! The Hemp Press ([`crate::machine::press`]) wired to Horizon: its block entity in the Java
//! mod's key (`Progress`), its tick (heat read from the block below as the Infuser's, `lit` and
//! `pressing` written back, one item pushed out of the back), its container, menu and screen, and
//! the platen and capstan its renderer screws down onto the packet.
//!
//! The push cooldown isn't saved (as in Java): it lives here by level and position. The platen's
//! stroke is the client's: it starts when a press is first seen `pressing`, and each squeeze
//! lasts the press time, so the phase falls out of the game clock (`strokeTicks`).

use std::sync::PoisonError;

use horizon_client::mod_api::{
    BlockEntitySubmitEvent, BlockEntityTickEvent, Compound, ContainerAcceptsEvent,
    DestroyBlockEvent, Direction, Flow, Host, ItemStack, MenuQuickMoveEvent, MenuSlotRulesEvent,
    ModContainer, NativeScreen, ScreenDrawEvent, StandaloneModelsEvent, Tag,
};

use super::infuser::{At, at, heated_from, is_air};
use super::{GameKitchen, Machines, block_id, scaled};
use crate::machine::decarb::Kitchen;
use crate::machine::press::{
    self, Bench, DATA_HEATED, DATA_PRESS_TIME, DATA_PROGRESS, INPUT_SLOT, OUTPUT_SLOT, PRESS_TIME,
    Pour, Poured, Press, SLOT_COUNT,
};

/// The Hemp Press: its block, its block entity type and its menu type share the id.
pub(crate) const HEMP_PRESS: &str = "hempdustry:hemp_press";
pub(super) const PRESSING: &str = "hempdustry:pressing";
const PROGRESS: &str = "Progress";

/// `HempPressScreenHandler`'s slots, item top-left in the 176 x 166 panel.
const SLOTS: [[f32; 2]; SLOT_COUNT] = [[56.0, 35.0], [116.0, 35.0]];
/// `HempPressScreen`: the panel, and the arrow and flame right of it in the sheet.
const PANEL: [f32; 2] = [176.0, 166.0];
const ARROW: [f32; 4] = [176.0, 0.0, 24.0, 16.0];
const ARROW_AT: [f32; 2] = [79.0, 35.0];
const FLAME: [f32; 4] = [176.0, 16.0, 14.0, 14.0];
const FLAME_AT: [f32; 2] = [84.0, 55.0];
const SHEET: &str = "hempdustry:gui/container/hemp_press";

/// The moving parts, block models no blockstate names.
const PLATEN: &str = "hempdustry:block/hemp_press_platen";
const SCREW: &str = "hempdustry:block/hemp_press_screw";
/// `HempPressBlockEntityRenderer`: pixels the platen travels, the capstan's turns over that
/// travel, and the share of a squeeze spent screwing down and holding.
const TRAVEL: f32 = 4.0;
const TURNS: f32 = 1.5;
const DESCENT: f32 = 0.55;
const HOLD: f32 = 0.9;
/// No tint.
const WHITE: u32 = 0x00FF_FFFF;

impl Bench for GameKitchen<'_> {
    fn result_for(&self, stack: &ItemStack) -> Option<ItemStack> {
        Kitchen::result_for(self, stack)
    }

    fn max_stack(&self, stack: &ItemStack) -> u32 {
        Kitchen::max_stack(self, stack)
    }
}

/// The container behind the press, through its side facing the press.
struct Behind<'a> {
    host: &'a mut dyn Host,
    pos: [i32; 3],
    side: Direction,
}

impl Pour for Behind<'_> {
    fn pour(&mut self, one: &ItemStack) -> Poured {
        if is_air(&self.host.block(self.pos)) {
            return Poured::Nothing;
        }
        match self.host.insert_item(self.pos, self.side, one.clone()) {
            Some(left) if left.count >= one.count => Poured::Full,
            _ => Poured::Moved,
        }
    }
}

/// The block behind the press at `pos` and its side the press pours in by: the press's back is
/// opposite its `facing`, and the neighbour's face against it points the way the press does.
fn behind(host: &dyn Host, pos: [i32; 3]) -> Option<([i32; 3], Direction)> {
    let facing = match host.block_property(pos, "facing")?.as_str() {
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => return None,
    };
    let [dx, dy, dz] = facing.opposite().normal();
    Some(([pos[0] + dx, pos[1] + dy, pos[2] + dz], facing))
}

/// The press as the Java mod saves it; a missing key reads as 0 (`getIntOr`).
fn load(nbt: &Compound) -> Press {
    let progress = match nbt.get(&PROGRESS.into()) {
        Some(Tag::Int(n)) => *n,
        _ => 0,
    };
    Press {
        progress,
        ..Press::default()
    }
}

fn save(nbt: &mut Compound, press: &Press) {
    nbt.insert(PROGRESS.into(), Tag::Int(press.progress));
}

/// The block of an event that may name a whole state is a press.
pub(super) fn is_press(block: &str) -> bool {
    block_id(block) == HEMP_PRESS
}

/// How far down the platen is over one squeeze's phase 0..1: screwed down, held, lifted quickly.
fn depth(phase: f32) -> f32 {
    let smooth = |t: f32| t * t * 2.0f32.mul_add(-t, 3.0);
    if phase < DESCENT {
        smooth(phase / DESCENT)
    } else if phase < HOLD {
        1.0
    } else {
        1.0 - smooth((phase - HOLD) / (1.0 - HOLD))
    }
}

impl Machines {
    fn refresh_pressing(&self, host: &dyn Host) {
        super::refresh(host, PRESSING, &self.pressing);
    }

    fn with_bench<R>(&self, f: impl FnOnce(&GameKitchen<'_>) -> R) -> R {
        self.with_recipes(&self.pressing, f)
    }

    fn push_cooldown(&self, at: At) -> i32 {
        self.presses
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&at)
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn press_block_entity() -> (String, Compound) {
        let mut nbt = Compound::new();
        save(&mut nbt, &Press::default());
        (HEMP_PRESS.to_owned(), nbt)
    }

    /// `getSlotsForFace`, as a furnace's: in from above and the sides, out of the bottom. The
    /// bottom is the heat, though, so the output goes out of the back instead.
    pub(super) fn press_container() -> ModContainer {
        let input = vec![INPUT_SLOT];
        ModContainer {
            slots: SLOT_COUNT,
            faces: [
                vec![OUTPUT_SLOT],
                input.clone(),
                input.clone(),
                input.clone(),
                input.clone(),
                input.clone(),
            ],
            inputs: input,
            outputs: vec![OUTPUT_SLOT],
            menu: Some(HEMP_PRESS.to_owned()),
        }
    }

    /// `HempPressBlockEntity.tick`, on the server.
    pub(super) fn tick_press(&self, host: &mut dyn Host, event: &mut BlockEntityTickEvent<'_>) {
        self.refresh_pressing(host);
        let pos = event.pos;
        let here = at(crate::dimension_of(host.dimension()), pos);
        let heated = heated_from(host, [pos[0], pos[1] - 1, pos[2]]);
        let mut press = load(&event.entity.nbt);
        press.push_cooldown = self.push_cooldown(here);
        let mut slots = event.entity.items.clone();
        slots.resize(SLOT_COUNT, None);
        let back = behind(host, pos);
        let press_time = self.settings.get().machine_time(PRESS_TIME);
        let ticked = self.with_bench(|bench| match back {
            Some((pos, side)) => {
                let mut behind = Behind {
                    host: &mut *host,
                    pos,
                    side,
                };
                press.tick(&mut slots, heated, press_time, bench, &mut behind)
            }
            // No facing: nowhere to pour, as a press with nothing behind it.
            None => press.tick(&mut slots, heated, press_time, bench, &mut Nowhere),
        });
        if ticked.changed {
            let entity = &mut *event.entity;
            entity.items = slots;
            save(&mut entity.nbt, &press);
        }
        let data = press.data(press_time);
        if event.entity.data != data {
            event.entity.data = data.to_vec();
        }
        for (key, value) in [("lit", heated), ("pressing", ticked.pressing)] {
            let value = if value { "true" } else { "false" };
            if host.block_property(pos, key).as_deref() != Some(value) {
                host.set_block_property(pos, key, value);
            }
        }
        self.presses
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(here, press.push_cooldown);
    }

    /// A press broken by a player takes its cooldown with it.
    pub(crate) fn press_destroyed(&self, host: &dyn Host, event: &DestroyBlockEvent) {
        if is_press(&host.block(event.pos)) {
            self.presses
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&at(crate::dimension_of(host.dimension()), event.pos));
        }
    }

    /// `Slot.mayPlace`: what presses into something, in the input slot; the output is take-only.
    pub(super) fn press_slot_rules(&self, event: &mut MenuSlotRulesEvent<'_>) {
        event.may_place = self.with_bench(|bench| press::accepts(event.slot, event.stack, bench));
    }

    /// A hopper above or beside the press puts only what it presses (`canPlaceItemThroughFace`).
    pub(super) fn press_accepts(&self, event: &ContainerAcceptsEvent<'_>) -> bool {
        self.with_bench(|bench| press::accepts(event.slot, event.stack, bench))
    }

    /// `quickMoveStack` from the player's inventory: what presses to the input slot. The other
    /// way, and anything the press doesn't take, is the game's routing.
    pub(super) fn press_quick_move(
        &self,
        host: &dyn Host,
        event: &mut MenuQuickMoveEvent<'_>,
    ) -> Flow {
        if event.slot < event.container_slots {
            return Flow::Continue;
        }
        self.refresh_pressing(host);
        let moved = self.with_bench(|bench| {
            if Bench::result_for(bench, event.stack).is_none() {
                return false;
            }
            let max = Bench::max_stack(bench, event.stack);
            let mut slots = event.entity.items.clone();
            slots.resize(SLOT_COUNT, None);
            let moved =
                crate::machine::move_into(&mut slots, INPUT_SLOT..INPUT_SLOT + 1, event.stack, max);
            if moved {
                event.entity.items = slots;
            }
            moved
        });
        if moved { Flow::Cancel } else { Flow::Continue }
    }

    pub(super) fn press_screen() -> NativeScreen {
        NativeScreen {
            width: PANEL[0],
            height: PANEL[1],
            slots: SLOTS.iter().copied().map(Some).collect(),
            inventory_y: 84.0,
            inventory_x: 8.0,
            own_labels: false,
            own_background: true,
        }
    }

    /// `HempPressScreen`: the panel, the arrow filling with the squeeze, the flame while heated.
    pub(super) fn press_screen_draw(host: &dyn Host, event: &mut ScreenDrawEvent<'_>) {
        let [ox, oy] = event.origin;
        let panel = [0.0, 0.0, PANEL[0], PANEL[1]];
        if !host.sprite(event.batch, SHEET, panel, [ox, oy, PANEL[0], PANEL[1]]) {
            return;
        }
        let data = |i: usize| event.data.get(i).copied().unwrap_or(0);
        let press_time = data(DATA_PRESS_TIME).max(1);
        let filled = scaled(data(DATA_PROGRESS), press_time, ARROW[2]);
        if filled > 0.0 {
            let src = [ARROW[0], ARROW[1], filled, ARROW[3]];
            let dst = [ox + ARROW_AT[0], oy + ARROW_AT[1], filled, ARROW[3]];
            host.sprite(event.batch, SHEET, src, dst);
        }
        if data(DATA_HEATED) != 0 {
            let dst = [ox + FLAME_AT[0], oy + FLAME_AT[1], FLAME[2], FLAME[3]];
            host.sprite(event.batch, SHEET, FLAME, dst);
        }
    }

    pub(crate) fn standalone_models(event: &mut StandaloneModelsEvent<'_>) {
        event.models.extend([PLATEN.to_owned(), SCREW.to_owned()]);
    }

    /// `HempPressBlockEntityRenderer.submit`: the platen screwed down by the capstan while
    /// `pressing`, at rest under the crossbeam otherwise. Both parts are square and centred, so
    /// `facing` changes nothing about them.
    pub(crate) fn press_submit(&self, host: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
        if !is_press(event.block) {
            return;
        }
        let draw = &mut *event.draw;
        let pressing = host.block_state_property(event.state, "pressing") == Some("true");
        let ticks = {
            let mut strokes = self.strokes.lock().unwrap_or_else(PoisonError::into_inner);
            if pressing {
                let start = *strokes.entry(event.pos).or_insert(draw.game_time);
                let since = (draw.game_time - start).rem_euclid(i64::from(PRESS_TIME));
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "less than a press time of ticks"
                )]
                let since = since as f32;
                Some(since + draw.partial)
            } else {
                strokes.remove(&event.pos);
                None
            }
        };
        #[expect(clippy::cast_precision_loss, reason = "the press time is 200 ticks")]
        let depth = ticks.map_or(0.0, |t| depth(t / PRESS_TIME as f32));
        draw.push();
        draw.translate([0.0, -depth * TRAVEL / 16.0, 0.0]);
        host.draw_model(draw, PLATEN, WHITE);
        // A right-hand thread drives down turning clockwise seen from above.
        draw.rotate_about([0.0, 1.0, 0.0], -depth * TURNS * 360.0, [0.5, 0.0, 0.5]);
        host.draw_model(draw, SCREW, WHITE);
        draw.pop();
    }
}

/// No container behind the press.
struct Nowhere;

impl Pour for Nowhere {
    fn pour(&mut self, _one: &ItemStack) -> Poured {
        Poured::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::depth;

    #[test]
    fn the_platen_screws_down_holds_and_lifts() {
        assert!(depth(0.0).abs() < 1e-6);
        assert!((depth(0.55) - 1.0).abs() < 1e-6);
        assert!((depth(0.8) - 1.0).abs() < 1e-6);
        assert!(depth(0.999) < 0.01);
        assert!(depth(0.3) > 0.0 && depth(0.3) < 1.0);
    }
}
