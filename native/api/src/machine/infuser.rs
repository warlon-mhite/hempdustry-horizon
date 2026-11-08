//! The Infuser (`InfuserBlockEntity`): a hempcrete tub around a cauldron that simmers
//! decarboxylated hemp into one bucket of milk until it is cannabutter.
//!
//! - **Heat comes from below**, not a fuel slot; losing it pauses the batch.
//! - **Milk is poured in by hand** ([`Tub::pour_milk`]), one at a time: one milk, one cannabutter.
//! - **Hemp dissolves gradually**, one item per [`Timings::absorb_interval`] and only until
//!   [`Timings::min`]: the batch is bookkeeping ([`Batch`]), not items in slots.
//! - **The output slot is a preview** of what the batch would yield now; taking it, by any route,
//!   closes the batch. The route is found at the next tick ([`Tub::tick`]'s sweep), so a hopper, a
//!   click and a shift-click are one case.
//! - **Two dials grade it** ([`Quality::of`]): how long it simmered past the minimum, and how pure
//!   the hemp was (washed share minus scorched share).
//! - **A spout pours a finished batch** into whatever is against the block's facing, once it is at
//!   the best grade it can reach ([`Tub::is_at_best_quality`], the one definition of "done").
//!
//! No Horizon host here: what counts as milk or hemp, the butter, and the spout's neighbour come
//! from a [`Game`].

use std::ops::Range;

use horizon_client::mod_api::ItemStack;

/// Retired in 2.0.1: milk used to go in here. Kept so old saves keep their slot numbers; nothing
/// is put in it again, and whatever an old world left there is handed back ([`Ticked::ejected`]).
pub const RETIRED_MILK_SLOT: usize = 0;
/// The two interchangeable hemp slots are contiguous from here.
pub const FIRST_HEMP_SLOT: usize = 1;
pub const HEMP_SLOT_COUNT: usize = 2;
/// The hemp slots: both take any of the three hemps, so a batch can mix them.
pub const HEMP_SLOTS: Range<usize> = FIRST_HEMP_SLOT..FIRST_HEMP_SLOT + HEMP_SLOT_COUNT;
/// The batch's preview.
pub const OUTPUT_SLOT: usize = 3;
/// Retired in 2.0.1 with [`RETIRED_MILK_SLOT`]: where emptied buckets used to come back.
pub const RETIRED_BUCKET_SLOT: usize = 4;
pub const RETIRED_SLOTS: [usize; 2] = [RETIRED_MILK_SLOT, RETIRED_BUCKET_SLOT];
pub const SLOT_COUNT: usize = 5;

/// Most hemp one batch holds: exactly how many absorb intervals fit in the loading window.
pub const BATCH_CAP: i32 = 24;
/// Ticks between the spout's tries at pouring a finished batch: a hopper's cadence.
pub const PUSH_COOLDOWN: i32 = 8;
/// The wait after a try that poured nothing.
pub const PUSH_IDLE_COOLDOWN: i32 = 20;
/// Scorched hemp it takes to add one to a batch's strength, rounded up: a furnace's butter stays
/// tier I however long it simmers.
pub const SCORCHED_PER_STRENGTH: i32 = 4;
/// What the purity data slot reads with no batch: `Short.MIN_VALUE`, since purity itself goes
/// negative with scorched hemp in the batch.
pub const NO_BATCH: i32 = -32_768;

/// The menu's data slots (`ContainerData`), in the Java mod's order.
pub const DATA_PROGRESS: usize = 0;
pub const DATA_HEATED: usize = 1;
pub const DATA_PURITY: usize = 2;
pub const DATA_MIN_TIME: usize = 3;
pub const DATA_FULL_TIME: usize = 4;
pub const DATA_FILLED: usize = 5;
pub const DATA_COUNT: usize = 6;

/// The simmer's two clocks, in ticks (Minecraft's own hour is 1000).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timings {
    /// The earliest a batch can be taken, and the zero of the time dial: six hours.
    pub min: i32,
    /// A full simmer: eighteen hours. Always more than `min`.
    pub full: i32,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            min: 6000,
            full: 18_000,
        }
    }
}

impl Timings {
    /// Ticks between one hemp dissolving and the next: `min / BATCH_CAP`, 250 by default.
    #[must_use]
    pub const fn absorb_interval(self) -> i32 {
        let interval = self.min / BATCH_CAP;
        if interval < 1 { 1 } else { interval }
    }

    /// The collectable window, never 0 (the config keeps `full` above `min`).
    #[must_use]
    pub const fn span(self) -> i32 {
        let span = self.full - self.min;
        if span < 1 { 1 } else { span }
    }

    /// The time dial: 0 at `min`, 100 at `full`.
    #[must_use]
    pub fn time_percent(self, progress: i32) -> i32 {
        (progress.saturating_sub(self.min).saturating_mul(100) / self.span()).clamp(0, 100)
    }
}

/// How well a batch was made, apart from how strong it is (`Quality`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Quality {
    /// Pulled early without the prep to make up for it.
    Rough,
    /// A full dose of one dial: all the patience, or all the prep.
    Standard,
    /// Both dials well up.
    Clean,
    /// A full simmer of nothing but washed hemp.
    Perfect,
}

impl Quality {
    /// The score at which a batch is Standard: a full dose of either dial alone.
    pub const STANDARD_SCORE: i32 = 100;
    /// The score at which it is Clean, out of one dial's reach.
    pub const CLEAN_SCORE: i32 = 160;
    /// The lowest purity: nothing but scorched hemp.
    pub const MIN_PURITY: i32 = -100;

    /// The grade of a batch from its two dials: `time + purity`, with Perfect a gate on both
    /// maxed, checked before the score so it can't be rounded into.
    #[must_use]
    pub fn of(time_percent: i32, purity_percent: i32) -> Self {
        let time = time_percent.clamp(0, 100);
        let purity = purity_percent.clamp(Self::MIN_PURITY, 100);
        if time >= 100 && purity >= 100 {
            return Self::Perfect;
        }
        let score = time + purity;
        if score >= Self::CLEAN_SCORE {
            Self::Clean
        } else if score >= Self::STANDARD_SCORE {
            Self::Standard
        } else {
            Self::Rough
        }
    }

    /// The lowest time percent at which a batch of `purity_percent` earns `self`, None when it
    /// never does: [`Self::of`] solved for time, for the screen's marks.
    #[must_use]
    pub fn time_needed(self, purity_percent: i32) -> Option<i32> {
        let purity = purity_percent.clamp(Self::MIN_PURITY, 100);
        let needed = match self {
            Self::Rough => 0,
            Self::Standard => Self::STANDARD_SCORE - purity,
            Self::Clean => Self::CLEAN_SCORE - purity,
            Self::Perfect if purity >= 100 => 100,
            Self::Perfect => return None,
        };
        (needed <= 100).then_some(needed.max(0))
    }

    /// The grade one step up.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Rough => Some(Self::Standard),
            Self::Standard => Some(Self::Clean),
            Self::Clean => Some(Self::Perfect),
            Self::Perfect => None,
        }
    }

    /// Every grade, lowest first.
    pub const ALL: [Self; 4] = [Self::Rough, Self::Standard, Self::Clean, Self::Perfect];

    /// The grade a `hempdustry:quality` value names (`Quality.CODEC`), None for another value.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.name() == name)
    }

    /// `Quality.getTranslationKey`.
    #[must_use]
    pub fn translation_key(self) -> String {
        format!("hempdustry.quality.{}", self.name())
    }

    /// The `hempdustry:quality` component's value.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rough => "rough",
            Self::Standard => "standard",
            Self::Clean => "clean",
            Self::Perfect => "perfect",
        }
    }
}

/// The three hemps the tub takes, in the order it spends them: cheapest first, so a mixed batch
/// keeps as much washed hemp as it can.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hemp {
    /// From a vanilla furnace: counts a quarter, and pulls the grade down.
    Scorched,
    /// Decarboxylated hemp.
    Unwashed,
    /// Washed decarboxylated hemp: what makes Clean and Perfect reachable.
    Washed,
}

impl Hemp {
    /// In the order absorption spends them.
    pub const ALL: [Self; 3] = [Self::Scorched, Self::Unwashed, Self::Washed];
}

/// The hemp dissolved into the running batch, by kind. Together at most [`BATCH_CAP`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    pub unwashed: i32,
    pub washed: i32,
    pub scorched: i32,
}

impl Batch {
    /// Hemp items in the batch: what the cap counts and whether there is a batch at all.
    #[must_use]
    pub const fn items(&self) -> i32 {
        self.unwashed + self.washed + self.scorched
    }

    #[must_use]
    pub const fn count(&self, hemp: Hemp) -> i32 {
        match hemp {
            Hemp::Scorched => self.scorched,
            Hemp::Unwashed => self.unwashed,
            Hemp::Washed => self.washed,
        }
    }

    const fn add(&mut self, hemp: Hemp) {
        match hemp {
            Hemp::Scorched => self.scorched += 1,
            Hemp::Unwashed => self.unwashed += 1,
            Hemp::Washed => self.washed += 1,
        }
    }

    /// The butter's `hempdustry:strength`: each decarboxylated hemp counts one, washed or not,
    /// and scorched hemp one per [`SCORCHED_PER_STRENGTH`], rounded up.
    #[must_use]
    pub const fn strength(&self) -> i32 {
        self.unwashed
            + self.washed
            + (self.scorched + SCORCHED_PER_STRENGTH - 1) / SCORCHED_PER_STRENGTH
    }

    /// The purity dial: washed share minus scorched share, floored, -100 to 100; None without a
    /// batch. Floored, so a single unwashed item among a thousand reads 99.
    #[must_use]
    pub const fn purity(&self) -> Option<i32> {
        let total = self.items();
        if total == 0 {
            return None;
        }
        Some(((self.washed - self.scorched) * 100).div_euclid(total))
    }
}

/// What the tub asks of the game, and does through it.
pub trait Game {
    /// Whether `stack` counts as milk (the infusing recipe's `container`).
    fn is_milk(&self, stack: &ItemStack) -> bool;
    /// Which hemp `stack` is, None when the tub doesn't take it.
    fn hemp(&self, stack: &ItemStack) -> Option<Hemp>;
    /// The butter a batch of `strength` and `quality` yields now (the recipe's result, stamped),
    /// None when no recipe names one.
    fn butter(&self, strength: i32, quality: Quality) -> Option<ItemStack>;
    /// Whether `shown` already is that butter: two numbers compared, no stack built.
    fn shows(&self, shown: &ItemStack, strength: i32, quality: Quality) -> bool;
    /// Pours `stack` through the spout into what is against it, all of it or none.
    fn pour(&mut self, stack: &ItemStack) -> Poured;
}

/// What the spout found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poured {
    /// It all went in.
    Moved,
    /// A container that had no room: tried again at the hopper's cadence.
    Full,
    /// Nothing to pour into: tried again after [`PUSH_IDLE_COOLDOWN`].
    Nothing,
}

/// The tub's own state, saved with it (`Progress`, `HaveMilk`, `BatchUnwashed`, `BatchWashed`,
/// `BatchScorched`, `PreviewShown`), and what is not saved (the heat, the spout's cooldown).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tub {
    pub progress: i32,
    /// Something hot is underneath: the screen's flame. Read again every tick.
    pub heated: bool,
    /// One milk, one cannabutter: set by a pour, cleared when the batch closes.
    pub have_milk: bool,
    pub batch: Batch,
    /// The output slot held a preview at the end of the last tick. Saved: a world saved between
    /// a take and the next tick must still close that batch.
    pub preview_shown: bool,
    /// Ticks before the spout tries again. Not saved, and not a change to save.
    pub push_cooldown: i32,
}

/// What a tick did, for the game to act on.
#[derive(Clone, Debug, Default, PartialEq)]
#[must_use]
pub struct Ticked {
    /// The tub's state or slots changed: save it.
    pub changed: bool,
    /// Items an old world left in the retired slots, to pop out of the top of the tub.
    pub ejected: Vec<ItemStack>,
}

impl Tub {
    /// `InfuserBlockEntity.tick`, in its order: close a batch whose preview was taken, hand back
    /// the retired slots, simmer and dissolve, refresh the preview, pour.
    pub fn tick(
        &mut self,
        slots: &mut [Option<ItemStack>],
        heated: bool,
        timings: Timings,
        game: &mut dyn Game,
    ) -> Ticked {
        let mut ticked = Ticked::default();
        // Not saved, so no change to save: read again from the block below every tick.
        self.heated = heated;
        // Before anything else touches the output slot: a preview gone since the last tick was
        // taken, by whatever route (a click, a shift-click, a hopper).
        if self.preview_shown && slots[OUTPUT_SLOT].is_none() && self.has_batch() {
            self.close_batch();
            ticked.changed = true;
        }
        for slot in RETIRED_SLOTS {
            if let Some(stack) = slots[slot].take() {
                ticked.ejected.push(stack);
                ticked.changed = true;
            }
        }
        if self.can_simmer(slots, timings, game) {
            // Exactly one hemp an interval, only in the loading window, never catching up.
            self.progress += 1;
            if self.progress <= timings.min && self.progress % timings.absorb_interval() == 0 {
                self.absorb_one(slots, game);
            }
            ticked.changed = true;
        }
        ticked.changed |= self.refresh_preview(slots, timings, &*game);
        ticked.changed |= self.push_output(slots, timings, game);
        // Last: what the slot looks like once this tick is done with it.
        let shown = slots[OUTPUT_SLOT].is_some();
        if self.preview_shown != shown {
            self.preview_shown = shown;
            ticked.changed = true;
        }
        ticked
    }

    /// Pours one milk in, if the tub is empty: whether it went in. The caller hands back the
    /// empty container (the milk's craft remainder).
    pub const fn pour_milk(&mut self) -> bool {
        if self.have_milk {
            return false;
        }
        self.have_milk = true;
        true
    }

    /// The tub's block state: milk in it (`filled`, the model), and a batch simmering
    /// (`infusing`, the bubbling).
    #[must_use]
    pub fn state(&self, slots: &[Option<ItemStack>], timings: Timings, game: &dyn Game) -> State {
        State {
            filled: self.have_milk,
            infusing: self.can_simmer(slots, timings, game),
        }
    }

    /// The tub breaks: the preview is thrown away first (spilling returns ingredients, never the
    /// product), and the hemp already dissolved comes back as items. The milk does not: its
    /// bucket came back when it was poured. Leaves no batch, so a second call gives nothing.
    ///
    /// `representative` names each hemp as an item and how many one stack holds: the batch
    /// counts items, not stacks, so the recipe's first match is the only answer there is. A hemp
    /// the recipe no longer names goes with the tub.
    pub fn spill(
        &mut self,
        slots: &mut [Option<ItemStack>],
        representative: &dyn Fn(Hemp) -> Option<(ItemStack, u32)>,
    ) -> Vec<ItemStack> {
        slots[OUTPUT_SLOT] = None;
        let mut out = Vec::new();
        for hemp in Hemp::ALL {
            let Some((one, max)) = representative(hemp) else {
                continue;
            };
            let max = max.max(1);
            let mut left = u32::try_from(self.batch.count(hemp)).unwrap_or(0);
            while left > 0 {
                let mut stack = one.clone();
                stack.count = left.min(max);
                left -= stack.count;
                out.push(stack);
            }
        }
        self.batch = Batch::default();
        self.have_milk = false;
        self.progress = 0;
        self.preview_shown = false;
        out
    }

    /// What a comparator reads: 0 idle, 1 to 14 through the simmer (13 at most: a nearly done
    /// batch never reads as done), 15 once at the best grade it can reach.
    #[must_use]
    pub fn comparator(&self, timings: Timings) -> i32 {
        if self.progress <= 0 {
            return 0;
        }
        if self.is_at_best_quality(timings) {
            return 15;
        }
        1 + (self.progress.saturating_mul(13) / timings.full.max(1)).min(13)
    }

    /// The menu's data slots, in the Java mod's order, with the clocks this tub runs on.
    #[must_use]
    pub fn data(&self, timings: Timings) -> [i32; DATA_COUNT] {
        [
            self.progress,
            i32::from(self.heated),
            self.batch.purity().unwrap_or(NO_BATCH),
            timings.min,
            timings.full,
            i32::from(self.have_milk),
        ]
    }

    const fn has_batch(&self) -> bool {
        self.have_milk && self.batch.items() > 0
    }

    /// Something in the output slot to look at yet.
    #[must_use]
    pub const fn is_ready(&self, timings: Timings) -> bool {
        self.progress >= timings.min
    }

    /// The batch has the best grade it will ever reach, so more simmering is time for nothing:
    /// the spout, the comparator and the simmer's end share this. Knowable because the purity
    /// is frozen once the loading window ends.
    #[must_use]
    pub fn is_at_best_quality(&self, timings: Timings) -> bool {
        let Some(purity) = self.batch.purity() else {
            return false;
        };
        self.is_ready(timings)
            && self.has_batch()
            && Quality::of(timings.time_percent(self.progress), purity) == Quality::of(100, purity)
    }

    /// Hot, milk in, hemp to work on (dissolved or still in the slots), and something left to
    /// gain: a batch out of hemp pauses rather than finishing empty.
    fn can_simmer(&self, slots: &[Option<ItemStack>], timings: Timings, game: &dyn Game) -> bool {
        self.heated
            && self.have_milk
            && (self.batch.items() > 0 || available_hemp(slots, game) > 0)
            && self.progress < timings.full
            && !self.is_at_best_quality(timings)
    }

    /// One hemp out of the slots into the batch, the cheapest kind first.
    fn absorb_one(&mut self, slots: &mut [Option<ItemStack>], game: &dyn Game) {
        if self.batch.items() >= BATCH_CAP {
            return;
        }
        for hemp in Hemp::ALL {
            for slot in HEMP_SLOTS {
                let Some(stack) = slots[slot].as_mut() else {
                    continue;
                };
                if game.hemp(stack) != Some(hemp) {
                    continue;
                }
                stack.count -= 1;
                if stack.count == 0 {
                    slots[slot] = None;
                }
                self.batch.add(hemp);
                return;
            }
        }
    }

    /// Keeps the output slot showing the current preview: whether it changed.
    fn refresh_preview(
        &self,
        slots: &mut [Option<ItemStack>],
        timings: Timings,
        game: &dyn Game,
    ) -> bool {
        let wanted = match self.batch.purity() {
            Some(purity) if self.is_ready(timings) && self.has_batch() => Some((
                self.batch.strength(),
                Quality::of(timings.time_percent(self.progress), purity),
            )),
            _ => None,
        };
        let Some((strength, quality)) = wanted else {
            return slots[OUTPUT_SLOT].take().is_some();
        };
        if slots[OUTPUT_SLOT]
            .as_ref()
            .is_some_and(|shown| game.shows(shown, strength, quality))
        {
            return false;
        }
        let butter = game.butter(strength, quality);
        let changed = butter.is_some() || slots[OUTPUT_SLOT].is_some();
        slots[OUTPUT_SLOT] = butter;
        changed
    }

    /// Pours a finished batch out of the spout, closing it as a take does: whether it moved.
    fn push_output(
        &mut self,
        slots: &mut [Option<ItemStack>],
        timings: Timings,
        game: &mut dyn Game,
    ) -> bool {
        if !self.is_at_best_quality(timings) {
            return false;
        }
        let Some(pending) = slots[OUTPUT_SLOT].clone() else {
            return false;
        };
        if self.push_cooldown > 0 {
            self.push_cooldown -= 1;
            return false;
        }
        self.push_cooldown = PUSH_COOLDOWN;
        match game.pour(&pending) {
            Poured::Moved => {
                slots[OUTPUT_SLOT] = None;
                self.close_batch();
                true
            }
            Poured::Full => false,
            Poured::Nothing => {
                self.push_cooldown = PUSH_IDLE_COOLDOWN;
                false
            }
        }
    }

    /// The batch is collected: the tub empties and the clock starts over. The hemp was spent as
    /// it dissolved, so nothing else is taken.
    const fn close_batch(&mut self) {
        self.have_milk = false;
        self.batch = Batch {
            unwashed: 0,
            washed: 0,
            scorched: 0,
        };
        self.progress = 0;
    }
}

/// The tub's two block-state properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub filled: bool,
    pub infusing: bool,
}

/// Hemp in the slots the batch could still draw on.
fn available_hemp(slots: &[Option<ItemStack>], game: &dyn Game) -> u32 {
    slots[HEMP_SLOTS]
        .iter()
        .flatten()
        .filter(|stack| game.hemp(stack).is_some())
        .map(|stack| stack.count)
        .sum()
}

/// `canPlaceItem`: only the hemp slots take anything, and both take any hemp.
#[must_use]
pub fn accepts(slot: usize, stack: &ItemStack, game: &dyn Game) -> bool {
    HEMP_SLOTS.contains(&slot) && game.hemp(stack).is_some()
}

/// Where a shift-click from the player's inventory sends `stack`: hemp to the hemp slots, else
/// nowhere in the tub.
#[must_use]
pub fn quick_move_slots(stack: &ItemStack, game: &dyn Game) -> Option<Range<usize>> {
    game.hemp(stack).is_some().then_some(HEMP_SLOTS)
}

/// The screen's simmer bar, read from the menu's data slots (`InfuserScreenHandler`): it
/// measures this batch's job, not the clock, so it fills exactly when the spout pours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gauge {
    progress: i32,
    purity: Option<i32>,
    timings: Timings,
}

impl Gauge {
    #[must_use]
    pub fn read(data: &[i32]) -> Self {
        let get = |i: usize| data.get(i).copied().unwrap_or(0);
        let min = get(DATA_MIN_TIME);
        let purity = get(DATA_PURITY);
        Self {
            progress: get(DATA_PROGRESS),
            purity: (purity != NO_BATCH).then_some(purity),
            timings: Timings {
                min,
                full: get(DATA_FULL_TIME).max(min.saturating_add(1)),
            },
        }
    }

    /// The tick this batch reaches its best grade; the full clock without a batch.
    #[must_use]
    pub fn finish(&self) -> i32 {
        let Some(purity) = self.purity else {
            return self.timings.full;
        };
        Quality::of(100, purity)
            .time_needed(purity)
            .map_or(self.timings.full, |needed| {
                self.timings.min + needed * self.timings.span() / 100
            })
    }

    /// How far through its job the batch is, 0 to 1.
    #[must_use]
    pub fn bar(&self) -> f32 {
        fraction(java_float(self.progress), self.finish())
    }

    /// Where the "you may take it now" notch sits along the bar.
    #[must_use]
    pub fn minimum_mark(&self) -> f32 {
        fraction(java_float(self.timings.min), self.finish())
    }

    /// Where along the bar the batch earns its next grade up; None without a batch or a grade
    /// left in reach.
    #[must_use]
    pub fn next_grade_mark(&self) -> Option<f32> {
        let purity = self.purity?;
        let time = self.timings.time_percent(self.progress);
        let mut next = Quality::of(time, purity).next();
        while let Some(grade) = next {
            if let Some(needed) = grade.time_needed(purity) {
                let at = java_float(self.timings.min)
                    + java_float(needed) / 100.0 * java_float(self.timings.span());
                return Some(fraction(at, self.finish()));
            }
            next = grade.next();
        }
        None
    }
}

/// `Mth.clamp(value / (float) of, 0, 1)`.
fn fraction(value: f32, of: i32) -> f32 {
    if of <= 0 {
        return 0.0;
    }
    (value / java_float(of)).clamp(0.0, 1.0)
}

/// Java's `(float)` of an `int`.
#[expect(
    clippy::cast_precision_loss,
    reason = "Java's `(float)` cast rounds as `as` does"
)]
const fn java_float(value: i32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
