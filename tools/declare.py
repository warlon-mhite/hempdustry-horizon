#!/usr/bin/env python3
"""Write Horizon's block and item declarations for Hempdustry from the game's own facts.

    python3 tools/declare.py FACTS.json VANILLA_ITEMS.json OUT_DIR

FACTS.json is what tools/oracle exports from the Java mod running on Minecraft 26.3 (every block
and item as the game registers it, and vanilla's blocks beside them). VANILLA_ITEMS.json is
Horizon's prepared `items.json` (vanilla's default components). OUT_DIR receives
`data/hempdustry/horizon/block/*.json` and `item/*.json`, the content a Horizon content mod
declares its blocks and items with: a vanilla template, and what differs from it.

The template of each block is chosen here by hand (TEMPLATES): the vanilla block whose behaviour
is closest and whose state properties are all ours, so a world saved by the Java mod reads back
with the same states. Everything else comes from the facts: the state properties the template
lacks, our numbers, sounds, shapes and ticks.
"""
import itertools
import json
import sys
from collections import Counter
from pathlib import Path

NS = "hempdustry"

BONGS = ["bong", "tinted_bong"] + [c + "_bong" for c in (
    "white", "light_gray", "gray", "black", "brown", "red", "orange", "yellow", "lime", "green",
    "cyan", "light_blue", "blue", "purple", "magenta", "pink")]

# Block -> vanilla template. Shape and behaviour class first; numbers, sounds and states come
# from the facts. Machines take a furnace: Horizon opens a mod container's menu only through a
# template that opens one.
TEMPLATES = {
    "hemp_bricks_block": "bricks", "hemp_bricks_stairs": "brick_stairs",
    "hemp_bricks_slab": "brick_slab", "hemp_bricks_wall": "brick_wall",
    "hemp_planks": "oak_planks", "hemp_planks_stairs": "oak_stairs", "hemp_planks_slab": "oak_slab",
    "hemp_planks_button": "oak_button", "hemp_planks_pressure_plate": "oak_pressure_plate",
    "hemp_planks_fence": "oak_fence", "hemp_planks_fence_gate": "oak_fence_gate",
    "hemp_planks_door": "oak_door", "hemp_planks_trapdoor": "oak_trapdoor",
    "hemp_planks_shelf": "oak_shelf",
    "hemp_planks_sign": "oak_sign", "hemp_planks_wall_sign": "oak_wall_sign",
    "hemp_planks_hanging_sign": "oak_hanging_sign", "hemp_planks_wall_hanging_sign": "oak_wall_hanging_sign",
    "hemp_parquet": "oak_planks", "hemp_parquet_stairs": "oak_stairs", "hemp_parquet_slab": "oak_slab",
    "hempcrete_block": "white_concrete", "hempcrete_powder_block": "white_concrete_powder",
    "hemp_bale": "hay_block", "hemp_leaves": "moss_block", "hemp_wool": "white_wool",
    "hemp_carpet": "white_carpet", "hemp_bed": "white_bed",
    "indica_crop": "wheat", "sativa_crop": "wheat", "beldia_crop": "wheat",
    "indica_flower": "allium", "potted_indica_flower": "potted_allium",
    "sativa_flower": "rose_bush", "potted_sativa_flower": "potted_dandelion",
    "decarboxylator": "furnace", "infuser": "furnace", "hemp_press": "furnace",
    "sifting_box": "oak_planks", "grow_lamp": "redstone_lamp", "grow_pot": "terracotta",
    "hydro_tray": "white_glazed_terracotta",
    "hashish_bar": "white_glazed_terracotta", "filtered_hashish_bar": "white_glazed_terracotta",
    "charas_ball": "clay", "space_cake": "cake",
    **{b: "white_glazed_terracotta" for b in BONGS},
}

SIGNS = {
    "hemp_planks_sign": {"wood_type": f"{NS}:hemp", "wall_block": f"{NS}:hemp_planks_wall_sign"},
    "hemp_planks_wall_sign": {"wood_type": f"{NS}:hemp"},
    "hemp_planks_hanging_sign": {"wood_type": f"{NS}:hemp", "wall_block": f"{NS}:hemp_planks_wall_hanging_sign"},
    "hemp_planks_wall_hanging_sign": {"wood_type": f"{NS}:hemp"},
}

# Items whose behaviour lives in the template's id rather than in components.
ITEM_TEMPLATES = {"indica_seeds": "wheat_seeds", "sativa_seeds": "wheat_seeds", "beldia_seeds": "wheat_seeds"}
DEFAULT_ITEM_TEMPLATE = "stick"

# Java behaviour written as an item override (`finishUsingItem`), restated as the vanilla components
# that do the same and merged over the item's own. Hemp milk: vanilla milk's clear, and the bucket back.
ITEM_BEHAVIOUR = {
    "hemp_milk_bucket": {
        "minecraft:consumable": {"on_consume_effects": [{"type": "minecraft:clear_all_effects"}]},
        "minecraft:use_remainder": {"id": "minecraft:bucket"},
    },
}


def parse_state(key):
    return dict(kv.split("=", 1) for kv in key.split(",")) if key else {}


def state_key(props, names):
    return ",".join(f"{n}={props[n]}" for n in names)


def close(a, b):
    return json.dumps(a, sort_keys=True) == json.dumps(b, sort_keys=True)


def round_boxes(boxes):
    return [[round(v, 6) for v in box] for box in boxes]


def shape_table(ours, tmpl, own_names, field):
    """A Horizon shape table for `field` (`outline` or `collision`), or None when the template's
    shapes already match ours state for state."""
    by_state = {k: round_boxes(s[field]) for k, s in ours["states"].items()}
    if tmpl is not None:
        same = True
        for k, boxes in by_state.items():
            props = parse_state(k)
            tk = state_key(props, sorted(tmpl["properties"]))
            t = tmpl["states"].get(tk)
            if t is None or not close(round_boxes(t[field]), boxes):
                same = False
                break
        if same:
            return None
    names = sorted(ours["properties"])
    distinct = {json.dumps(b) for b in by_state.values()}
    if len(distinct) == 1:
        return {"default": next(iter(by_state.values()))}
    for size in range(1, len(names) + 1):
        for subset in itertools.combinations(names, size):
            table, ok = {}, True
            for k, boxes in by_state.items():
                props = parse_state(k)
                tup = ",".join(props[n] for n in subset)
                if tup in table and not close(table[tup], boxes):
                    ok = False
                    break
                table[tup] = boxes
            if ok:
                common = Counter(json.dumps(b) for b in table.values()).most_common(1)[0][0]
                values = {t: b for t, b in table.items() if json.dumps(b) != common}
                if any(len(b) > 16 for b in table.values()):
                    raise SystemExit(f"{field}: more than 16 boxes for {ours}")
                return {"properties": list(subset), "values": values, "default": json.loads(common)}
    raise SystemExit("no shape table")


def sound_block(facts, ours_sound, template_id):
    """The vanilla block whose sounds are ours: the template when its own are, else the first
    vanilla block with exactly our sound events and numbers."""
    # The default state's: Horizon's `sounds` names a block, and a block's sounds are its default
    # state's (the decorated pot's other states sound cracked).
    def sound_of(b):
        blk = facts["blocks"][b]
        return blk["states"][blk["default"]]["sound"]
    if close(sound_of(template_id), ours_sound):
        return None
    for bid, b in facts["blocks"].items():
        if bid.startswith("minecraft:") and b["states"] and close(sound_of(bid), ours_sound):
            return bid
    raise SystemExit(f"no vanilla block sounds like {ours_sound}")


def component_patch(ours, base):
    patch = {}
    for k, v in ours.items():
        if k not in base or not close(base[k], v):
            patch[k] = v
    for k in base:
        if k not in ours:
            patch["!" + k] = {}
    return patch


def block_decl(facts, vanilla_items, name):
    bid = f"{NS}:{name}"
    ours = facts["blocks"][bid]
    template = "minecraft:" + TEMPLATES[name]
    tmpl = facts["blocks"][template]
    own = {n: v for n, v in ours["properties"].items() if n not in tmpl["properties"]}
    extra = sorted(set(tmpl["properties"]) - set(ours["properties"]))
    if extra:
        print(f"  {name}: template {template} adds {extra}", file=sys.stderr)
    for n in set(ours["properties"]) & set(tmpl["properties"]):
        if ours["properties"][n] != tmpl["properties"][n]:
            raise SystemExit(f"{name}: {n} is {ours['properties'][n]}, {template} has {tmpl['properties'][n]}")
    decl = {"template": template}
    default = parse_state(ours["default"])
    t_default = parse_state(tmpl["default"])
    if own:
        decl["state_properties"] = {
            n: ({"type": "boolean"} if sorted(vals) == ["false", "true"] else {"type": "enum", "values": vals})
            for n, vals in sorted(own.items())}
        decl["default_state"] = {n: default[n] for n in sorted(own)}
    shared_default = {n: v for n, v in default.items() if n in tmpl["properties"] and t_default.get(n) != v}
    if shared_default:
        print(f"  {name}: default differs from {template} on {shared_default}", file=sys.stderr)

    d = ours["states"][ours["default"]]
    td = tmpl["states"][tmpl["default"]]
    props = {}
    if d["hardness"] != td["hardness"]:
        props["hardness"] = d["hardness"]
    if ours["explosion_resistance"] != tmpl["explosion_resistance"]:
        props["explosion_resistance"] = ours["explosion_resistance"]
    if ours["friction"] != tmpl["friction"]:
        props["friction"] = ours["friction"]
    emissions = {s["emission"] for s in ours["states"].values()}
    t_emissions = {s["emission"] for s in tmpl["states"].values()}
    if len(emissions) == 1 and emissions != t_emissions:
        props["light"] = next(iter(emissions))
    elif len(emissions) > 1 and max(emissions) != max(t_emissions):
        print(f"  {name}: light by state {sorted(emissions)} vs {template} {sorted(t_emissions)}", file=sys.stderr)
    if d["requires_tool"] != td["requires_tool"]:
        props["requires_tool"] = d["requires_tool"]
    if d["map_color_id"] != td["map_color_id"]:
        props["map_color"] = d["map_color_id"]
    snd = sound_block(facts, d["sound"], template)
    if snd:
        props["sounds"] = snd
    occl = {s["can_occlude"] for s in ours["states"].values()}
    t_occl = {s["can_occlude"] for s in tmpl["states"].values()}
    if occl == {False} and t_occl != {False}:
        props["occludes"] = False
    # Whether each of our states ticks as the template state with the same template properties
    # does: comparing the two sets of answers would call wheat (ticks below age 7) and a crop whose
    # lower half always ticks the same, since both sets are {true, false}.
    ticks = {k: s["random_ticks"] for k, s in ours["states"].items()}
    t_names = list(tmpl["properties"])
    t_ticks = {state_key(parse_state(k), t_names): s["random_ticks"] for k, s in tmpl["states"].items()}
    tick_set = set(ticks.values())
    # A template property we lack (the furnace's `lit` under the infuser) stays at its default.
    as_template = lambda k: state_key({**t_default, **parse_state(k)}, t_names)
    if any(t != t_ticks[as_template(k)] for k, t in ticks.items()):
        if len(tick_set) == 1:
            props["random_ticks"] = next(iter(tick_set))
        else:
            for n, vals in own.items():
                on = {parse_state(k)[n] for k, t in ticks.items() if t}
                if all(ticks[k] == (parse_state(k)[n] in on) for k in ticks):
                    props["random_ticks"] = {"property": n, "values": sorted(on)}
                    break
            else:
                print(f"  {name}: random ticks follow no single own property", file=sys.stderr)
    if props:
        decl["properties"] = props
    outline = shape_table(ours, tmpl if not own else None, own, "outline")
    collision = shape_table(ours, tmpl if not own else None, own, "collision")
    if outline is not None:
        decl["shapes"] = outline
    if collision is not None and not (outline is not None and close(collision, outline)):
        decl["collision_shapes"] = collision
    decl.update(SIGNS.get(name, {}))

    item = facts["items"].get(bid)
    if item and item.get("block") == bid:
        base = vanilla_items.get(f"minecraft:{TEMPLATES[name]}", {})
        patch = component_patch(item["components"], base)
        if patch:
            decl["components"] = patch
    else:
        decl["item"] = False
    return decl


def item_decl(facts, vanilla_items, iid, item):
    name = iid.split(":", 1)[1]
    template = "minecraft:" + ITEM_TEMPLATES.get(name, DEFAULT_ITEM_TEMPLATE)
    decl = {"template": template}
    if item.get("block"):
        decl["place_block"] = item["block"]
    components = dict(item["components"])
    for k, v in ITEM_BEHAVIOUR.get(name, {}).items():
        components[k] = {**components.get(k, {}), **v}
    patch = component_patch(components, vanilla_items[template])
    # An item file names these two outside its components (a block file keeps them inside).
    if "minecraft:max_stack_size" in patch:
        decl["max_stack_size"] = patch.pop("minecraft:max_stack_size")
    if "minecraft:rarity" in patch:
        decl["rarity"] = patch.pop("minecraft:rarity")
    # Horizon reads the remainder by item, never from the template.
    if item.get("crafting_remainder"):
        decl["crafting_remainder"] = item["crafting_remainder"]
    if patch:
        decl["components"] = patch
    return decl


def main():
    facts = json.loads(Path(sys.argv[1]).read_text())
    vanilla_items = json.loads(Path(sys.argv[2]).read_text())
    out = Path(sys.argv[3]) / "data" / NS / "horizon"
    ours = sorted(b.split(":", 1)[1] for b in facts["blocks"] if b.startswith(NS + ":"))
    missing = [b for b in ours if b not in TEMPLATES]
    stale = [b for b in TEMPLATES if b not in ours]
    if missing or stale:
        raise SystemExit(f"templates missing for {missing}, for blocks that are gone: {stale}")
    for kind in ("block", "item", "creative_tab"):
        (out / kind).mkdir(parents=True, exist_ok=True)
        for old in (out / kind).glob("*.json"):
            old.unlink()
    for name in ours:
        decl = block_decl(facts, vanilla_items, name)
        (out / "block" / f"{name}.json").write_text(json.dumps(decl, indent=2) + "\n")
    items = 0
    for iid, item in sorted(facts["items"].items()):
        if not iid.startswith(NS + ":"):
            continue
        name = iid.split(":", 1)[1]
        if item.get("block") == iid:
            continue  # declared with its block
        decl = item_decl(facts, vanilla_items, iid, item)
        (out / "item" / f"{name}.json").write_text(json.dumps(decl, indent=2) + "\n")
        items += 1
    tabs = 0
    for tid, tab in sorted(facts.get("creative_tabs", {}).items()):
        name = tid.split(":", 1)[1]
        decl = {"title": tab["title"], "icon": tab["icon"], "items": tab["items"]}
        (out / "creative_tab" / f"{name}.json").write_text(json.dumps(decl, indent=2) + "\n")
        tabs += 1
    print(f"{len(ours)} blocks, {items} items, {tabs} creative tabs")


if __name__ == "__main__":
    main()
