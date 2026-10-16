#!/usr/bin/env python3
"""Restore the authored face culling of converted character surfaces.

Usage: character_render_states.py <assets> [glb ...]

DSO_Godot's GLB.add_material defaults double_sided=True. The shipped
shaders_sm30 character, characterfalloff and monster effects all declare
CullMode <CullMode> = 2. D3DCULL_CW removes back faces for the counter-clockwise
front winding retained by this conversion (the same mapping used by
embed_animators.py). The old render-state pass skipped split uniskel parts,
and default shader parameters were absent from N3 per-node overrides.

Only these verified shader names are repaired. Explicit per-node CullMode=1
stays two-sided; mixed users remain two-sided. CullMode=3/unknown values and
unrelated materials are left unchanged. Original character N3 audit: 5,720
surfaces use these defaults, with no CullMode override. Metadata records the
resolved state so already-converted assets and the runtime compatibility path
in client/src/model_compact.rs use the same decision.
"""
import argparse
from pathlib import Path

from fix_materials_io import read_glb, write_glb


SHADERS = {"shd:character", "shd:characterfalloff", "shd:monster"}


def restore(doc):
    materials = doc.get("materials", [])
    modes = [set() for _ in materials]
    for node in doc.get("nodes", []):
        if "mesh" not in node:
            continue
        for primitive in doc["meshes"][node["mesh"]].get("primitives", []):
            index = primitive.get("material")
            if index is None:
                continue
            extras = materials[index].get("extras", {})
            if extras.get("nebula_shader") not in SHADERS:
                continue
            mode = node.get("extras", {}).get("dsor_ints", {}).get(
                "CullMode", extras.get("dsor_cull_mode", 2))
            modes[index].add(mode)
    changed = 0
    for material, values in zip(materials, modes):
        if not values or not values <= {1, 2}:
            continue
        double_sided = 1 in values
        changed += material.get("doubleSided", False) != double_sided
        material["doubleSided"] = double_sided
        material.setdefault("extras", {})["dsor_cull_mode"] = 1 if double_sided else 2
    return changed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("assets", type=Path)
    parser.add_argument("models", nargs="*", type=Path)
    args = parser.parse_args()
    files = args.models or sorted((args.assets / "characters").rglob("*.glb"))
    changed = 0
    for path in files:
        doc, binary = read_glb(path)
        count = restore(doc)
        if count:
            write_glb(path, doc, binary)
            changed += count
    print({"character materials repaired": changed, "models checked": len(files)})


if __name__ == "__main__":
    main()
