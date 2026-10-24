#!/usr/bin/env python3
"""Restore the skin fragments DSO_Godot's glTF export leaves out.

Usage: restore_skin_fragments.py <export_win32> <assets> <DSO_Godot> [model ...]

Nebula draws a skinned mesh in fragments: a skin node lists (n3 tag SFRG) the
primitive groups of its .nvx2 it draws, each with its own joint palette (at most
72 joints, the vertex shader's matrix budget), so a big skeleton's skin is split.
DSO_Godot exports one primitive group per node, the node's own (0 for every skin
node), with group 0's palette:
- a node listing more groups lost them: Balor's right wing, arm and leg (group 1,
  1791 of its 8082 triangles), parts of Khalys, Sigrismarr, Mhorblik, Sharr
  Kharab, Nefertari, Mortis...;
- a node listing only other groups (Mhorblik's skinned1 = [1, 3], Megasargon's
  skinned1..3) got group 0 and no palette: a copy frozen in the bind pose.
Each such node's mesh is rebuilt as one primitive per listed fragment, written as
DSO_Godot writes a primitive (palette index -> skeleton joint index), with the
node's material and the model's skin. A new primitive carries the attributes the
node's primitive had (tools/fix_materials.py --character-colors drops COLOR_0
from character surfaces), so this can run before or after the other steps. The
binary chunk grows; replaced accessors stay unreferenced. A rebuilt node carries
extras.dsor_fragments: rerunning is a no-op.
EVIDENCE: the converted boss_07_balor weighted 72 of its skin's 127 joints, none
  of them on the right side; its group 0 rebuilt here matches DSO_Godot's
  primitive exactly (positions, UVs, joints, indices; normals within 3e-8).
"""
import json, os, struct, sys

sys.path.insert(0, os.path.dirname(__file__))
from fix_materials_io import read_glb, write_glb  # noqa: E402

root, assets, godot = sys.argv[1], sys.argv[2], sys.argv[3]
only = set(sys.argv[4:])
sys.path.insert(0, godot)
from dsoexp import n3, nvx2  # noqa: E402  DSO_Godot's readers
from dsoexp.resources import Resolver  # noqa: E402

FLOAT, USHORT, UINT = 5126, 5123, 5125
ARRAY_BUFFER, ELEMENT_ARRAY_BUFFER = 34962, 34963
FORMAT = {FLOAT: "f", USHORT: "H", UINT: "I"}
WIDTH = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
resolver = Resolver(root)


class Binary:
    """The glb's binary chunk, appended to."""

    def __init__(self, doc, rest):
        self.doc = doc
        size = struct.unpack_from("<I", rest, 0)[0] if len(rest) >= 8 else 0
        self.data = bytearray(rest[8:8 + size])

    def accessor(self, values, ctype, kind, target, minmax=False):
        self.data += b"\0" * (-len(self.data) % 4)
        flat = [x for v in values for x in (v if WIDTH[kind] > 1 else (v,))]
        raw = struct.pack(f"<{len(flat)}{FORMAT[ctype]}", *flat)
        views = self.doc.setdefault("bufferViews", [])
        views.append({"buffer": 0, "byteOffset": len(self.data), "byteLength": len(raw), "target": target})
        self.data += raw
        acc = {"bufferView": len(views) - 1, "componentType": ctype, "count": len(values), "type": kind}
        if minmax and values:
            cols = list(zip(*values))
            acc["min"], acc["max"] = [min(c) for c in cols], [max(c) for c in cols]
        accessors = self.doc.setdefault("accessors", [])
        accessors.append(acc)
        return len(accessors) - 1

    def rest(self):
        self.data += b"\0" * (-len(self.data) % 4)
        self.doc["buffers"][0]["byteLength"] = len(self.data)
        return struct.pack("<II", len(self.data), 0x004E4942) + bytes(self.data)


def tangents(d, used):
    """DSO_Godot's TANGENT: w is the handedness the binormal gives."""
    out = []
    for v in used:
        t, n = d["tangents"][v], d["normals"][v]
        w = 1.0
        if "binormals" in d:
            b = d["binormals"][v]
            cross = (n[1] * t[2] - n[2] * t[1], n[2] * t[0] - n[0] * t[2], n[0] * t[1] - n[1] * t[0])
            w = -1.0 if sum(c * k for c, k in zip(cross, b)) < 0.0 else 1.0
        out.append((t[0], t[1], t[2], w))
    return out


def primitive(binary, mesh, d, group, palette, names, material):
    """One fragment as DSO_Godot's export_model._mesh writes a primitive."""
    g = mesh.groups[group]
    indices = mesh.indices[g.first_triangle * 3:(g.first_triangle + g.num_triangles) * 3]
    used = sorted(set(indices))
    remap = {v: i for i, v in enumerate(used)}
    attrs = {"POSITION": binary.accessor([d["positions"][v] for v in used], FLOAT, "VEC3", ARRAY_BUFFER, minmax=True)}
    for name, key, kind in (("NORMAL", "normals", "VEC3"), ("TEXCOORD_0", "uv0", "VEC2"),
                            ("TEXCOORD_1", "uv1", "VEC2"), ("COLOR_0", "colors", "VEC4")):
        if name in names and key in d:
            attrs[name] = binary.accessor([d[key][v] for v in used], FLOAT, kind, ARRAY_BUFFER)
    if "TANGENT" in names and "tangents" in d and "normals" in d:
        attrs["TANGENT"] = binary.accessor(tangents(d, used), FLOAT, "VEC4", ARRAY_BUFFER)
    # glTF wants joint 0 wherever the weight is 0.
    joints = [tuple(palette[k] if (w > 0.0 and k < len(palette)) else 0 for k, w in zip(d["joints"][v], d["weights"][v]))
              for v in used]
    attrs["JOINTS_0"] = binary.accessor(joints, USHORT, "VEC4", ARRAY_BUFFER)
    attrs["WEIGHTS_0"] = binary.accessor([tuple(float(x) for x in d["weights"][v]) for v in used], FLOAT, "VEC4", ARRAY_BUFFER)
    ctype = USHORT if len(used) < 65536 else UINT
    index = binary.accessor([remap[i] for i in indices], ctype, "SCALAR", ELEMENT_ARRAY_BUFFER)
    return {"attributes": attrs, "indices": index, "material": material}, g.num_triangles


def restore(n3_path, glb_path, fx_path, stats):
    model = n3.load(n3_path)
    nodes = [nd for nd in model.nodes if nd.skin_fragments]
    exported = lambda nd: [pg for pg, _ in nd.skin_fragments] == [max(0, nd.prim_group)]  # noqa: E731
    if all(exported(nd) for nd in nodes):
        return
    doc, rest = read_glb(glb_path)
    # The glTF node of each n3 node: the sidecar names both, in the same order.
    targets = {}
    for e in json.load(open(fx_path)).get("emitters", []):
        if e.get("gltf_node") is not None:
            targets.setdefault((e.get("node"), e.get("mesh")), []).append(e["gltf_node"])
    seen = {}
    skins = {n["skin"] for n in doc["nodes"] if "skin" in n}
    binary, changed = Binary(doc, rest), False
    for nd in nodes:
        key = (nd.name, nd.mesh)
        k = seen[key] = seen.get(key, -1) + 1
        if exported(nd):
            continue
        found = targets.get(key, [])
        if k >= len(found):
            print(f"{glb_path}: {nd.name} ({nd.mesh}): no glTF node")
            stats["skipped"] += 1
            continue
        node = doc["nodes"][found[k]]
        groups = [pg for pg, _ in nd.skin_fragments]
        if node.get("extras", {}).get("dsor_fragments") == groups:
            continue
        if "mesh" not in node:
            print(f"{glb_path}: {nd.name} ({nd.mesh}): glTF node without a mesh")
            stats["skipped"] += 1
            continue
        skin = node.get("skin")
        if skin is None:
            if len(skins) != 1:
                print(f"{glb_path}: {nd.name}: which skin of {sorted(skins)}?")
                stats["skipped"] += 1
                continue
            skin = next(iter(skins))
        joints = len(doc["skins"][skin]["joints"])
        if any(j >= joints for _, palette in nd.skin_fragments for j in palette):
            print(f"{glb_path}: {nd.name}: palette outside the skin's {joints} joints")
            stats["skipped"] += 1
            continue
        path = resolver.path(nd.mesh)
        mesh = nvx2.load(path)
        d = nvx2.decode(mesh)
        if "joints" not in d or "weights" not in d or any(pg >= len(mesh.groups) for pg in groups):
            print(f"{glb_path}: {nd.name}: {os.path.basename(path)} has no such skinned groups")
            stats["skipped"] += 1
            continue
        old = doc["meshes"][node["mesh"]]
        template = old["primitives"][0]
        names = set(template["attributes"])
        prims = []
        for pg, palette in nd.skin_fragments:
            prim, tris = primitive(binary, mesh, d, pg, palette, names, template.get("material"))
            prims.append(prim)
            stats["triangles"] += tris
        shared = sum(1 for n in doc["nodes"] if n.get("mesh") == node["mesh"]) > 1
        if shared:
            doc["meshes"].append({"name": f"{os.path.basename(path)[:-5]}_g{'_'.join(map(str, groups))}", "primitives": prims})
            node["mesh"] = len(doc["meshes"]) - 1
        else:
            old["primitives"] = prims
        node["skin"] = skin
        node.setdefault("extras", {})["dsor_fragments"] = groups
        stats["nodes"] += 1
        changed = True
    if changed:
        write_glb(glb_path, doc, binary.rest())
        stats["models"] += 1


stats = {"models": 0, "nodes": 0, "triangles": 0, "skipped": 0}
models = os.path.join(root, "models")
for dirpath, _dirs, files in os.walk(models):
    for name in files:
        if not name.endswith(".n3"):
            continue
        n3_path = os.path.join(dirpath, name)
        rel = os.path.relpath(n3_path, models).replace("\\", "/")[:-3]
        if only and rel not in only and os.path.basename(rel) not in only:
            continue
        with open(n3_path, "rb") as f:
            if b"GRFS" not in f.read():  # SFRG, stored reversed
                continue
        glb_path = os.path.join(assets, rel + ".glb")
        fx_path = os.path.join(assets, rel + ".fx.json")
        if os.path.exists(glb_path) and os.path.exists(fx_path):
            restore(n3_path, glb_path, fx_path, stats)
print("restore_skin_fragments:", stats)
