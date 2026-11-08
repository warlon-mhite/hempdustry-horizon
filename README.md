# Hempdustry for Horizon

[Hempdustry](https://github.com/warlon-mhite/hempdustry-fabric) for
[Horizon](https://github.com/horizon-rust/horizon), the Rust edition of Minecraft: Java Edition 26.3.

Horizon runs no Java mods, so this is the same mod built a second way: its data and art straight from
the Java repository, and its behaviour written again in Rust.

| Part | What | Status |
|---|---|---|
| Content (`.hmod`) | Every block, item, recipe, loot table, tag, advancement, model, texture, sound and translation | Builds; every block places and draws |
| Native (Rust) | Growing, trimming, smoking, edibles, the machines and their screens | Farming written and unit-tested (`native/`); not yet played in a game |

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

Worlds stay compatible with the Java mod on Minecraft 26.3: the same ids, block states and data.

## The native mod

`native/` is a Horizon trait mod with the same id, `hempdustry`. `native/api/src/farm/` holds the Java
mod's farming rules as plain Rust with no Horizon types (growth, light records, stress, mildew, the
Grow Pot and Hydro Tray, trimming, bone meal, harvesting), tested on their own with `cargo test` in
`native/api`; the rest of the crate wires them to Horizon's hooks. Building it into a game is
`horizon-launcher native-build native --instance <instance> --trust-native-code`, which compiles
Horizon with the mod linked in: trusting native code is the player's decision.

## Licence

Code and data AGPL-3.0-only, art and music CC-BY-NC-SA-4.0, as the Java mod (see its `CREDITS.md`).
`tools/oracle/` derives from Horizon's NeoForge oracle and keeps its MIT notice.
