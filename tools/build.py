#!/usr/bin/env python3
"""Build Hempdustry for Horizon from the Java mod at the commit upstream.toml pins.

    python3 tools/build.py facts      run the Java mod once and export its facts (needs the checkout
                                      at the pinned commit, clean, and its Gradle toolchain)
    python3 tools/build.py declare    write content/data/hempdustry/horizon/ from those facts
    python3 tools/build.py stage      assemble build/hempdustry/: the pinned assets and data, rewritten
                                      where Horizon can't run them as shipped (item models, placement
                                      modifiers, the infused edible recipes), then content/ over them,
                                      and the manifest
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
    infused = rewrite_infused_recipes(STAGE)
    over = 0
    for src in (ROOT / "content").rglob("*"):
        if src.is_file():
            rel = src.relative_to(ROOT / "content")
            dest = STAGE / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            if is_lang(rel) and dest.exists():
                # Lines only this edition says (how a device packs by hand), added to the Java
                # mod's file rather than standing in for it; a key both have keeps ours.
                merged = json.loads(dest.read_text(encoding="utf-8"))
                merged.update(json.loads(src.read_text(encoding="utf-8")))
                dest.write_text(json.dumps(merged, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            else:
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
          f"{placements} placed features rewritten, {infused[0]} infused recipes as {infused[1]}), "
          f"{over} from content/, in {STAGE}")


def is_lang(rel):
    """`assets/<namespace>/lang/<locale>.json`."""
    parts = rel.parts
    return len(parts) == 4 and parts[0] == "assets" and parts[2] == "lang" and rel.suffix == ".json"


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


# The Java mod's recipe types that carry cannabutter's dose onto what is baked with it, and the
# vanilla type each is written out as.
INFUSED_TYPES = {
    "hempdustry:infused_shaped": "minecraft:crafting_shaped",
    "hempdustry:infused_shapeless": "minecraft:crafting_shapeless",
}
BUTTER = "hempdustry:cannabutter"
# `Quality`'s names, low to high, and `EdibleEffects.MAX_TIER`.
QUALITIES = ["rough", "standard", "clean", "perfect"]
MAX_TIER = 4


def rewrite_infused_recipes(stage_dir):
    """Horizon crafts no recipe type of a mod's, and its crafting hook can't see the grid, so an
    infused recipe (`Infusion.transfer`: the result takes the butter's tier, plus the recipe's
    `offset`, and its quality) becomes one vanilla recipe per result it can make,
    `<recipe>/potency_<n>_<quality>`, its result carrying that dose. The butter's place takes the
    native mod's ingredient kind (`edible/butter.rs`), which accepts only the butter that makes
    that result, so exactly one variant matches any grid. The variants share a `group` (one tile
    in the recipe book), and the advancements that hand out or watch the recipe name every
    variant instead. Returns (recipes, variants)."""
    renamed = {}
    for path in sorted(stage_dir.glob("data/*/recipe/*.json")):
        recipe = json.loads(path.read_text())
        kind = recipe.get("type")
        if kind not in INFUSED_TYPES:
            continue
        namespace, name = path.parts[-3], path.stem
        offset = recipe.pop("offset", 0)
        recipe["type"] = INFUSED_TYPES[kind]
        recipe.setdefault("group", f"{namespace}:{name}")
        slots = recipe["key"] if "key" in recipe else dict(enumerate(recipe["ingredients"]))
        butter_slots = [slot for slot, ingredient in slots.items() if ingredient == BUTTER]
        if len(butter_slots) != 1:
            sys.exit(f"{path.relative_to(stage_dir)}: an infused recipe needs exactly one plain "
                     f"`{BUTTER}` ingredient, found {len(butter_slots)}")
        potencies = sorted({min(max(tier + offset, 1), MAX_TIER) for tier in range(1, MAX_TIER + 1)})
        path.unlink()
        ids = []
        for potency in potencies:
            for quality in QUALITIES:
                variant = json.loads(json.dumps(recipe))
                butter = {"type": BUTTER, "potency": potency, "offset": offset, "quality": quality}
                if "key" in variant:
                    variant["key"][butter_slots[0]] = butter
                else:
                    variant["ingredients"][butter_slots[0]] = butter
                variant["result"].setdefault("components", {}).update(
                    {"hempdustry:potency": potency, "hempdustry:quality": quality})
                variant_name = f"{name}/potency_{potency}_{quality}"
                out = path.parent / f"{variant_name}.json"
                out.parent.mkdir(exist_ok=True)
                out.write_text(json.dumps(variant, indent=2) + "\n")
                ids.append(f"{namespace}:{variant_name}")
        renamed[f"{namespace}:{name}"] = ids

    def rename(value):
        """A recipe id, or a list of them, with each infused recipe as all its variants."""
        names = value if isinstance(value, list) else [value]
        out = [v for n in names for v in renamed.get(n, [n])]
        return out if isinstance(value, list) or len(out) > 1 else out[0]

    for path in sorted(stage_dir.glob("data/*/advancement/**/*.json")):
        advancement = json.loads(path.read_text())
        before = json.dumps(advancement)
        rewards = advancement.get("rewards", {})
        if "recipes" in rewards:
            rewards["recipes"] = rename(rewards["recipes"])
        for name, criterion in list(advancement.get("criteria", {}).items()):
            recipe = criterion.get("conditions", {}).get("recipes")
            if criterion.get("trigger") != "minecraft:recipe_unlocked" or recipe not in renamed:
                continue
            # The criterion names one recipe: one criterion per variant, any of them enough.
            del advancement["criteria"][name]
            names = [f"{name}/{i}" for i in range(len(renamed[recipe]))]
            for variant_name, variant in zip(names, renamed[recipe]):
                advancement["criteria"][variant_name] = {
                    **criterion, "conditions": {**criterion["conditions"], "recipes": variant}}
            advancement["requirements"] = [
                [n for r in group for n in (names if r == name else [r])]
                for group in advancement.get("requirements", [])]
        if json.dumps(advancement) != before:
            path.write_text(json.dumps(advancement, indent=2) + "\n")
    return len(renamed), sum(len(ids) for ids in renamed.values())


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
