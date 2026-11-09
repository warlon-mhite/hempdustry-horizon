# Hempdustry for Horizon

[Hempdustry](https://github.com/warlon-mhite/hempdustry-fabric) for
[Horizon](https://github.com/horizon-rust/horizon), the Rust edition of Minecraft: Java Edition 26.3.

Horizon runs no Java mods, so this is the same mod built a second way: its data and art straight from
the Java repository, and its behaviour written again in Rust.

| Part | What | Status |
|---|---|---|
| Content (`.hmod`) | Every block, item, recipe, loot table, tag, advancement, model, texture, sound and translation | Builds; every block places and draws |
| Native (Rust) | Growing, trimming, smoking, edibles, the machines and their screens, the config | Farming, smoking, eating, the Decarboxylator, the Infuser, the Hemp Press, the Sifting Box, the water cauldron, the config file and creepers drawn to hemp (`native/`): unit-tested, and played in a game on Horizon `fdc87ae` |

## How it is built

The Java repository stays the one source of the mod's data and art; none of it is copied here.
`upstream.toml` pins the commit the build reads; `HEMPDUSTRY_JAVA` names a local clone of the Java
repository, `HORIZON_RESOURCES` Horizon's prepared resources (`.local/horizon-client/26.3`) and
`CARGO_HORIZON_MOD` the `cargo-horizon-mod` binary.

1. `tools/build.py facts` runs the Java mod once on a dedicated server with a small dev-only exporter
   (`tools/oracle/`, added to the Java checkout's build by an init script, never shipped) and writes
   every block and item exactly as the game registers them.
2. `tools/build.py declare` turns those facts into Horizon's declarations in
   `content/data/hempdustry/horizon/`: each block copies the vanilla block in `tools/declare.py`'s
   table and declares the state properties, numbers, sounds and shapes that differ; each item is a
   component patch. The output is committed, so a build needs no Java.
3. `tools/build.py hmod` assembles the pinned assets and data with `content/` on top and packs
   `build/hempdustry/target/horizon-mod/hempdustry.hmod` with `cargo horizon-mod`.

`content/data/c/` holds the one convention tag the mod's worldgen needs and a loader would
provide: `c:is_desert` (the desert, as NeoForge and Fabric write it), which Beldía's biome tag
names alone. Horizon ships no `c:` tags, so without it no desert grows wild Beldía.

`content/assets/hempdustry/lang/` holds the lines only this edition says (how a device packs by
hand); the build adds them to the Java mod's language files rather than replacing those.

Worlds stay compatible with the Java mod on Minecraft 26.3: the same ids, block states and data.

## The native mod

`native/` is a Horizon trait mod with the same id, `hempdustry`. `native/api/src/farm/` holds the Java
mod's farming rules as plain Rust with no Horizon types (growth, light records, stress, mildew, the
Grow Pot and Hydro Tray, trimming, bone meal, harvesting), and `native/api/src/smoke/` its smoking
rules (strains, devices and the green-out read from the same data files, a hit, the exhale, wear),
`native/api/src/machine/` the machines' and `native/api/src/edible/` the edibles' (tiers, the
come-up, the staggered effects), tested on their own with `cargo test` in `native/api`; the rest of the crate wires them to
Horizon's hooks. `cargo clippy --all-targets` passes clean under the lint table in
`native/api/Cargo.toml` (pedantic, no `unsafe`, no `unwrap` outside tests); a lint broken on
purpose is answered on the spot with `#[expect(…, reason = "…")]`. Building it into a game is
`horizon-launcher native-build native --instance <instance> --trust-native-code`, which compiles
Horizon with the mod linked in: trusting native code is the player's decision.

Played in a game: seeds, growth to two and three blocks and the height a roof leaves, bone meal,
the Grow Pot and Hydro Tray (bottle, bucket, bone meal), both trims and the shears' wear, harvest
from every block and from the bed with Fortune, buds and their spoiling, and the Top Shelf
advancement. Still to come in this part: sulfur gas, Beldía's placement check, the tray's pump,
the grow lamp's ceiling rule and particles.

Smoking, played in a game: a hit from the spliff, the pipe, the bongs (drawn on, as in Java) and the
vaporizer, with each strain's effects at the dose's level and length, Beldía's exhale, the cooldown
they share, the green-out and its lock-out, the charges and wear (Unbreaking counted), the
vaporizer's scorched hemp, and the smoking advancements. The build rewrites the devices' item
definitions so Horizon draws them packed (it reads `has_component` for vanilla's components only).
A loaded smokeable or moon rock is named for its load in its tooltip ("Purple Kush Bong III",
"Mixed Vaporizer"), with "With Hashish" under a spliff or moon rock that carries hash; Horizon has
no item-name hook, so the line above the hotbar keeps the plain name.

Packing, by hand: a pipe, bong or vaporizer used with buds, hash, rosin or a moon rock in the other
hand takes one item per use into its bowl, up to the bowl's dose, so the number of clicks is the
dose. A full bowl, or one already hit, smokes instead. The result is the device the Java mod's
packing recipe makes (same load, every charge, the device's own wear, enchantments and name), so a
stack moves between the two games unchanged; an empty device says how in its tooltip. The crafting
grid can't pack yet (Horizon's crafting hook doesn't see the grid). Still to come: per-strain
tints, the cooldown overlay, Jelly Legs and the standing bong.

Eating, played in a game: a dosed edible (toast, cookie, brownie, dawamesk, bhang) gives nothing at
once; its effects come after a come-up rolled from its quality's window (Perfect 87 to 93 s, Rough
45 s to 4:20), Slowness alone first, then Absorption and Resistance, the munchies and Regeneration,
at the potency and for the length the butter's strength and grade set, through the effect switches
of the config. What is still to come is saved with the player, so a logout doesn't lose it, and a
death does. A space cake keeps its dose when placed; every click on it is a bite (never a candle),
feeding as a cake does, and the seventh takes the last slice. The butter's grade and dose and an
edible's potency and quality show in their tooltips. Still to come: making a dosed edible, since the
infused recipes and the bhang's are custom recipe types that Horizon's crafting hook can't serve
yet (it doesn't see the grid), and the eating statistics.

The Decarboxylator, played in a game: three trays on one fire, each on its own clock, buds and leaf
to decarboxylated hemp by the shipped recipes, fuel from any item that burns in a furnace, hoppers
from above (trays), the sides (fuel) and below (output), only what belongs in each slot, the lit
state, shift-clicks, its own screen with the flame and the three arrows, and its contents spilled
when broken. Worlds keep its block data in the Java mod's keys. Horizon resolves only Minecraft's
own burn-time references, so the native mod writes the mod's fuel times out: hemp burns in a
vanilla furnace too. Still to come here: the oven's crackle and smoke, and an anvil name on its screen.

The Infuser, played in a game: heat from the block below (a campfire, a lit furnace or oven, magma,
lava), milk poured by hand or by a dispenser with the empty handed back, hemp fed by hand or by
hoppers and dissolved one at a time through the loading window, the cannabutter previewed from the
minimum with its strength and grade, the grade climbing with the simmer, the spout pouring a batch
into the container it faces once it can get no better, the comparator reading the batch's
progress, its screen with the bar, the two marks, the milk and the flame, and a broken tub giving
back its dissolved hemp but never the butter. Still to come: its bubbles, steam and simmer sound.

The water cauldron, played in a game: decarboxylated hemp washes and hemp stems ret in it, one
for one, water charged by the amount (a full cauldron washes a stack of 64) and what doesn't fit
in the inventory dropped at the player's feet.

The Hemp Press, played in a game: heated from the block below as the Infuser, kief pressed into
hashish, bubble hash and filtered kief into filtered hashish, filtered hashish into rosin and a
retted stem into eight fibre, one squeeze every 200 ticks, cooling off without heat or room; the
output pushed out of its back one item at a time into whatever container stands there, hoppers
feeding it only what it presses, its screen with the arrow and the flame, and its platen screwed
down onto the packet and lifted again while it presses. Still to come: its steam and creak.

The Sifting Box, played in a game: buds and leaves over a dry screen into nine kief, kief re-sifted
into nine filtered kief, a bucket of water turning it into an ice-water wash that runs only inside a
jacket of ice and at that jacket's rate, into nine bubble hash; the screen settling a second after it
fills, the batch taken with an empty hand, plant matter and powder kept apart with a line on the
action bar saying why, a comparator reading the level. Still to come: the frost while it washes.

The config file, played in a game: `config/hempdustry.json` is the Java mod's own file, read in the
same format, so one file serves both editions. Missing knobs keep their defaults, values out of range
are clamped with a warning in the log, and the file is written back complete with its comments; a
file that can't be read is left as it is and changes nothing. Horizon reads it again whenever it
changes, and `/hempdustry reload` (operators, level 2) does it at once and says whether it took. Every
knob the ported parts use is read live: crop growth and mildew, the effect switches, the machine
speed (the oven and the press) and the Infuser's two times. The client section and the loot section
are kept in the file for the parts still to come.

Creepers drawn to hemp, played in a game: about half of all creepers (by their UUID, so it holds
across saves) now and then wander into a grown, open-sky hemp field, prowl a few steps from plant to
plant, standing still in the crop between them, then lose interest for a minute or three. A field
draws one creeper at a time, a seedling field or a roofed one none. Measured with four creepers in a
pen: those that care spent 11 % of their time in a field covering 5 % of the floor, those that don't
and the same creepers with `world.creepersSeekHemp` off none.

## Licence

Code and data AGPL-3.0-only, art and music CC-BY-NC-SA-4.0, as the Java mod (see its `CREDITS.md`).
`tools/oracle/` derives from Horizon's NeoForge oracle and keeps its MIT notice.
