#!/usr/bin/env python3
"""Build Hempdustry for Horizon from the Java mod at the commit upstream.toml pins.

    python3 tools/build.py facts      run the Java mod once and export its facts (needs the checkout
                                      at the pinned commit, clean, and its Gradle toolchain)
    python3 tools/build.py declare    write content/data/hempdustry/horizon/ from those facts
    python3 tools/build.py stage      assemble build/hempdustry/: the pinned assets and data, then
                                      content/ over them, and the manifest
    python3 tools/build.py hmod       stage, then pack it with cargo-horizon-mod

The Java repository stays the one source of the mod's data and art: nothing of it is copied into
this repository. What is here is what Horizon needs on top (content/) and the tools that make it.
"""
import json
import os
import shutil
import subprocess
import sys
import tarfile
import io
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "build"
STAGE = BUILD / "hempdustry"
FACTS = BUILD / "facts.json"

# What the Java repository holds for a resource pack and a data pack, at the pinned commit. The
# NeoForge folder brings the biome modifiers, which Horizon reads for a mod's worldgen; Fabric adds
# the same plants in code, which Horizon cannot run.
UPSTREAM_PATHS = [
    "common/src/main/resources/assets",
    "common/src/main/resources/data",
    "common/src/generated/resources/assets",
    "common/src/generated/resources/data",
    "neoforge/src/generated/resources/data",
]


def upstream():
    cfg = tomllib.loads((ROOT / "upstream.toml").read_text())
    clone = os.environ.get("HEMPDUSTRY_JAVA")
    if not clone:
        sys.exit(f"HEMPDUSTRY_JAVA: a clone of {cfg['repository']} (any branch; the build reads commit {cfg['commit']})")
    return cfg, Path(clone).resolve()


def git(repo, *args, **kw):
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, **kw).stdout


def facts():
    cfg, repo = upstream()
    head = git(repo, "rev-parse", "HEAD", text=True).strip()
    if not head.startswith(cfg["commit"]):
        sys.exit(f"{repo} is at {head[:10]}, upstream.toml pins {cfg['commit']}: check it out first")
    if git(repo, "status", "--porcelain", "--untracked-files=no", text=True).strip():
        sys.exit(f"{repo} has uncommitted changes: the facts would not be the pinned commit's")
    run_dir = repo / "fabric" / "build" / "horizon-oracle"
    run_dir.mkdir(parents=True, exist_ok=True)
    (run_dir / "eula.txt").write_text("eula=true\n")
    (run_dir / "server.properties").write_text(
        "server-port=25611\nlevel-type=minecraft\\:flat\nonline-mode=false\nsync-chunk-writes=false\n")
    BUILD.mkdir(exist_ok=True)
    subprocess.run(["./gradlew", "--init-script", str(ROOT / "tools/oracle/oracle.gradle"),
                    ":fabric:runHorizonOracle", f"-Phorizon.oracle.out={FACTS}", "--console=plain"],
                   cwd=repo, check=True)
    print(f"facts: {FACTS}")


def declare():
    resources = os.environ.get("HORIZON_RESOURCES")
    if not resources:
        sys.exit("HORIZON_RESOURCES: the prepared Horizon resources (.local/horizon-client/26.3)")
    subprocess.run([sys.executable, "-I", str(ROOT / "tools/declare.py"), str(FACTS),
                    str(Path(resources) / "items.json"), str(ROOT / "content")], check=True)


def stage():
    cfg, repo = upstream()
    if STAGE.exists():
        shutil.rmtree(STAGE)
    STAGE.mkdir(parents=True)
    tar = git(repo, "archive", "--format=tar", cfg["commit"], "--", *UPSTREAM_PATHS)
    count = 0
    with tarfile.open(fileobj=io.BytesIO(tar)) as t:
        for m in t.getmembers():
            if not m.isfile():
                continue
            rel = next(m.name[len(p) - len(p.split("/")[-1]):] for p in UPSTREAM_PATHS if m.name.startswith(p + "/"))
            dest = STAGE / rel
            if dest.exists():
                sys.exit(f"{rel}: in two upstream folders")
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(t.extractfile(m).read())
            count += 1
    rewritten = rewrite_item_definitions(STAGE)
    placements = rewrite_placements(STAGE)
    over = 0
    for src in (ROOT / "content").rglob("*"):
        if src.is_file():
            dest = STAGE / src.relative_to(ROOT / "content")
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(src, dest)
            over += 1
    (STAGE / "horizon-mod.toml").write_text(
        f'id = "hempdustry"\n'
        f'name = "Hempdustry"\n'
        f'version = "{cfg["version"]}"\n'
        f'description = "{cfg["description"]}"\n'
        f'authors = ["Warlon Mhite"]\n'
        f'api = "0.2"\n'
        f'side = "both"\n'
        f'capabilities = ["registries"]\n')
    print(f"stage: {count} files from {cfg['commit']} ({rewritten} item definitions, "
          f"{placements} placed features rewritten), {over} from content/, in {STAGE}")


# The placement modifiers the Java mod registers in code, and the vanilla one that stands in for
# each. Horizon can't run ours and drops a whole placed feature over one type it doesn't know.
CODE_PLACEMENTS = {
    # Each wild Beldía, not just its patch's centre, in `#hempdustry:beldia_gen`. Vanilla's biome
    # filter at the end of the chain asks the same of each plant's own spot (the biomes that list
    # the feature are the tag's), through the biome zoom's jitter that ours avoids: at a desert's
    # very edge a plant may land a block over the border, or miss a block inside it.
    "hempdustry:in_beldia_biome": "minecraft:biome",
}


def rewrite_placements(stage_dir):
    """Each placement modifier of ours in a placed feature becomes the vanilla one that stands in
    for it (`CODE_PLACEMENTS`). Returns how many files changed."""
    changed = 0
    for path in sorted(stage_dir.glob("data/*/worldgen/placed_feature/*.json")):
        feature = json.loads(path.read_text())
        before = json.dumps(feature)
        for modifier in feature.get("placement", []):
            if modifier.get("type") in CODE_PLACEMENTS:
                modifier["type"] = CODE_PLACEMENTS[modifier["type"]]
        unknown = [m["type"] for m in feature.get("placement", [])
                   if m.get("type", "").split(":", 1)[0] != "minecraft"]
        if unknown:
            sys.exit(f"{path.relative_to(stage_dir)}: placement modifiers Horizon can't run: {unknown}")
        if json.dumps(feature) != before:
            path.write_text(json.dumps(feature, indent=2) + "\n")
            changed += 1
    return changed


# The data components the Java mod registers in code; the native mod registers them on Horizon.
CODE_COMPONENTS = {"smoke_contents", "potency", "charges", "cooldown_until", "strength", "quality"}


def rewrite_item_definitions(stage_dir):
    """Horizon reads a `minecraft:has_component` condition only for vanilla's components and refuses
    the whole item definition otherwise. Each one on a component of ours becomes the custom
    property `hempdustry:has/<component>`, which Horizon asks the native mod (its
    `item_model_property` hook) and which answers it the same way. Returns how many files changed."""
    def walk(node):
        if isinstance(node, list):
            return [walk(n) for n in node]
        if not isinstance(node, dict):
            return node
        node = {k: walk(v) for k, v in node.items()}
        component = node.get("component", "")
        if (node.get("type") == "minecraft:condition"
                and node.get("property") == "minecraft:has_component"
                and component.startswith("hempdustry:")
                and component.split(":", 1)[1] in CODE_COMPONENTS):
            node["property"] = "hempdustry:has/" + component.split(":", 1)[1]
            node.pop("component")
            node.pop("ignore_default", None)
        return node

    changed = 0
    for path in sorted(stage_dir.glob("assets/*/items/*.json")):
        before = json.loads(path.read_text())
        after = walk(before)
        if after != before:
            path.write_text(json.dumps(after, indent=2) + "\n")
            changed += 1
    return changed


def hmod():
    stage()
    tool = os.environ.get("CARGO_HORIZON_MOD", "cargo-horizon-mod")
    subprocess.run([tool, "horizon-mod", "build"], cwd=STAGE, check=True)


if __name__ == "__main__":
    steps = {"facts": facts, "declare": declare, "stage": stage, "hmod": hmod}
    if len(sys.argv) != 2 or sys.argv[1] not in steps:
        sys.exit(__doc__)
    steps[sys.argv[1]]()
