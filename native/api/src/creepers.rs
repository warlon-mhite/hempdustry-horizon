//! Creepers drawn to hemp, on Horizon: [`creeper`]'s rules as a mob goal every creeper gets at
//! priority 5, after vanilla's own wander (`CreeperEntityMixin`'s `registerGoals` tail), so the
//! wander gets first refusal each tick.
//!
//! A Horizon mob carries no UUID, and its goals are built before its saved data is read, so whether
//! a creeper cares is read when it joins its level (`Host::entity_uuid`), kept by entity id in
//! [`Caring`], and asked once by its goal. Ids are never reused while a world is open.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, PoisonError, RwLock};

use horizon_client::mobs::goals::{self, Cx, Goal};
use horizon_client::mobs::{Mob, MobWorld, Species, riders};
use horizon_client::mod_api::{Host, RandomSource};

use crate::config::Live;
use crate::creeper::{self, Dice, Field, Prowl, TAKEN_RADIUS};
use crate::farm::model::Pos;

pub(crate) const CREEPER: &str = "minecraft:creeper";
const HEMP_CROPS: &str = "hempdustry:hemp_crops";

/// Whether each creeper in the world cares about hemp, by entity id.
#[derive(Clone, Debug, Default)]
pub(crate) struct Caring(Arc<RwLock<HashMap<u32, bool>>>);

impl Caring {
    /// A creeper joined a level: its answer, from its UUID.
    pub(crate) fn joined(&self, host: &mut dyn Host, entity: u32) {
        let Some(uuid) = host.entity(entity).and_then(|e| host.entity_uuid(e)) else {
            return;
        };
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(entity, creeper::cares_about_hemp(uuid));
    }

    /// It left (died, unloaded, changed dimension under a new id).
    pub(crate) fn left(&self, entity: u32) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&entity);
    }

    fn cares(&self, entity: u32) -> Option<bool> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&entity)
            .copied()
    }
}

/// `SeekHempCropGoal`, a `RandomStrollGoal` whose spot is a hemp plant.
#[derive(Clone, Debug)]
pub(crate) struct SeekHemp {
    prowl: Prowl,
    wanted: [f64; 3],
    /// Read from [`Caring`] once the creeper has joined; it never changes after.
    cares: Option<bool>,
    settings: Live,
    caring: Caring,
}

impl SeekHemp {
    pub(crate) fn new(settings: Live, caring: Caring) -> Self {
        Self {
            prowl: Prowl::default(),
            wanted: [0.0; 3],
            cares: None,
            settings,
            caring,
        }
    }

    /// The config's switch is read here rather than when the goal is added, so a reload reaches
    /// creepers that already exist.
    fn switched_on(&self) -> bool {
        self.settings.get().world.creepers_seek_hemp
    }

    fn cares(&mut self, id: u32) -> bool {
        if self.cares.is_none() {
            self.cares = self.caring.cares(id);
        }
        self.cares == Some(true)
    }
}

/// The mob's own random as the rules' dice.
struct Draws<'a, R>(&'a mut R);

impl<R: RandomSource> Dice for Draws<'_, R> {
    fn between(&mut self, min: i32, max: i32) -> i32 {
        self.0.next_int_between_inclusive(min, max).unwrap_or(min)
    }
}

/// The level as a glance reads it.
struct Glance<'a, 'w> {
    w: &'a MobWorld<'w>,
    hemp: Option<&'a HashSet<String>>,
    /// The glancing creeper, which never takes its own field.
    me: u32,
}

impl Field for Glance<'_, '_> {
    fn loaded(&self, x: i32, z: i32) -> bool {
        self.w.world.loaded(x, z)
    }

    fn top(&self, x: i32, z: i32) -> i32 {
        self.w
            .world
            .world_surface(x, z, |state| self.w.facts.is_air(state))
            - 1
    }

    fn is_hemp(&self, [x, y, z]: Pos) -> bool {
        self.hemp.is_some_and(|tag| {
            let state = self.w.world.block(x, y, z);
            tag.contains(&*self.w.facts.get(state).name)
        })
    }

    /// `getEntitiesOfClass(Creeper.class, new AABB(root).inflate(8))`, the glancing one aside.
    fn taken(&self, root: Pos) -> bool {
        let lo = root.map(|v| f64::from(v) - TAKEN_RADIUS);
        let hi = root.map(|v| f64::from(v) + 1.0 + TAKEN_RADIUS);
        self.w.others.iter().any(|other| {
            if other.species != Species::Creeper || other.id == self.me || !other.alive {
                return false;
            }
            let (centre, size) = (other.bounds.center(), other.bounds.size());
            let half = [size.x / 2.0, size.y / 2.0, size.z / 2.0];
            let centre = [centre.x, centre.y, centre.z];
            let min = [0, 1, 2].map(|i| centre[i] - half[i]);
            let max = [0, 1, 2].map(|i| centre[i] + half[i]);
            (0..3).all(|i| min[i] < hi[i] && max[i] > lo[i])
        })
    }
}

/// The block a mob stands in, as `getBlockX/Y/Z`.
fn block_of(m: &Mob) -> Pos {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Mth.floor of a position inside the world"
    )]
    let at = [m.position.x, m.position.y, m.position.z].map(|v| v.floor() as i32);
    at
}

impl Goal for SeekHemp {
    fn flags(&self) -> u8 {
        goals::MOVE
    }

    fn name(&self) -> &'static str {
        "SeekHempCrop"
    }

    /// `SeekHempCropGoal.canUse`, then `RandomStrollGoal.canUse` with its interval and spot.
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        let now = cx.w.game_time;
        if !self.switched_on() || !self.cares(m.id) || self.prowl.resting(now) {
            return false;
        }
        if riders::mob_controlled(m, cx.w) || m.ai.no_action_time >= 100 {
            return false;
        }
        let interval = goals::reduced(self.prowl.interval());
        if m.random.next_int_bound(interval).unwrap_or(1) != 0 {
            return false;
        }
        let glance = Glance {
            w: cx.w,
            hemp: cx.w.block_tags.get(HEMP_CROPS),
            me: m.id,
        };
        let at = block_of(m);
        let Some([x, y, z]) = self
            .prowl
            .next_spot(&glance, &mut Draws(&mut m.random), at, now)
        else {
            return false;
        };
        self.wanted = [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5];
        true
    }

    /// Walking, then lurking where the walk ended. A creeper with a target never gets here: the
    /// attack goal at priority 4 takes the movement from this one.
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if !self.switched_on() || riders::mob_controlled(m, cx.w) {
            return false;
        }
        let walking = !m.ai.nav.is_done();
        self.prowl
            .lurk(walking, &mut Draws(&mut m.random), cx.w.game_time)
    }

    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_move_to(cx.w, self.wanted, creeper::SPEED);
        self.prowl
            .started(&mut Draws(&mut m.random), cx.w.game_time);
    }

    fn stop(&mut self, m: &mut Mob, _cx: &mut Cx) {
        m.nav_stop();
        self.prowl.stopped();
    }

    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}
