#!/usr/bin/env python3
"""Close the looping clips of the converted models: one more key at the end.

Usage: close_loops.py [--undo] <assets> [glb ...]   (assets/characters/**/*.glb when none
given; --undo reopens every closed clip under <assets>)

A Nebula clip of n keys, key duration dt, that loops (post-infinity loop) lasts
n * dt and interpolates from its last key back to its first over the last dt.
DSO_Godot writes the n keys at 0 .. (n-1) * dt and names the clip "<name>-loop":
played in a loop the last stretch is missing, the pose jumps from the last key
to the first ("ca sacade au moment du loop" -- the run cycle).
This appends, to every sampler of every "-loop" animation, a key at n * dt
holding the first key's value (a rotation flipped to the last one's hemisphere,
so it turns the short way). Idempotent: an animation whose samplers already end
on their first value at n * dt is left alone (the extras flag "dsor_closed").
"""
import glob, json, os, struct, sys

import numpy as np

COMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def read_glb(path):
    b = open(path, "rb").read()
    if b[:4] != b"glTF":
        return None
    n = struct.unpack("<I", b[12:16])[0]
    g = json.loads(b[20:20 + n])
    rest = b[20 + n:]
    blob = b""
    if len(rest) >= 8:
        ln = struct.unpack("<I", rest[:4])[0]
        blob = rest[8:8 + ln]
    return g, bytearray(blob)


def write_glb(path, g, blob):
    js = json.dumps(g, separators=(",", ":")).encode()
    js += b" " * ((4 - len(js) % 4) % 4)
    blob = bytes(blob) + b"\0" * ((4 - len(blob) % 4) % 4)
    total = 12 + 8 + len(js) + 8 + len(blob)
    with open(path, "wb") as f:
        f.write(struct.pack("<III", 0x46546C67, 2, total))
        f.write(struct.pack("<II", len(js), 0x4E4F534A) + js)
        f.write(struct.pack("<II", len(blob), 0x004E4942) + blob)


def read(g, blob, i):
    a = g["accessors"][i]
    bv = g["bufferViews"][a["bufferView"]]
    if a.get("componentType") != 5126 or bv.get("byteStride"):
        return None
    off = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    c = COMP[a["type"]]
    return np.frombuffer(bytes(blob[off:off + a["count"] * c * 4]), dtype="<f4").reshape(a["count"], c)


def append(g, blob, values, kind, minmax=False):
    while len(blob) % 4:
        blob.append(0)
    off = len(blob)
    data = np.ascontiguousarray(values, dtype="<f4")
    blob += data.tobytes()
    g["bufferViews"].append({"buffer": 0, "byteOffset": off, "byteLength": data.nbytes})
    acc = {"bufferView": len(g["bufferViews"]) - 1, "componentType": 5126, "count": len(data), "type": kind}
    if minmax:
        acc["min"] = [float(x) for x in data.min(axis=0)]
        acc["max"] = [float(x) for x in data.max(axis=0)]
    g["accessors"].append(acc)
    return len(g["accessors"]) - 1


def compact(g, blob):
    """Drop the accessors and buffer views nothing refers to any more, and rebuild
    the binary buffer from what is left (the replaced keys would stay otherwise)."""
    used = set()
    for m in g.get("meshes", []):
        for p in m.get("primitives", []):
            used.update(p.get("attributes", {}).values())
            if "indices" in p:
                used.add(p["indices"])
            for t in p.get("targets", []):
                used.update(t.values())
    for sk in g.get("skins", []):
        if "inverseBindMatrices" in sk:
            used.add(sk["inverseBindMatrices"])
    for an in g.get("animations", []):
        for sm in an["samplers"]:
            used.update((sm["input"], sm["output"]))
    acc_map, accessors = {}, []
    for i, a in enumerate(g["accessors"]):
        if i in used:
            acc_map[i] = len(accessors)
            accessors.append(a)
    views_used = {a["bufferView"] for a in accessors if "bufferView" in a}
    views_used |= {im["bufferView"] for im in g.get("images", []) if "bufferView" in im}
    view_map, views, out = {}, [], bytearray()
    for i, bv in enumerate(g["bufferViews"]):
        if i not in views_used:
            continue
        while len(out) % 4:
            out.append(0)
        start = bv.get("byteOffset", 0)
        data = blob[start:start + bv["byteLength"]]
        nv = dict(bv)
        nv["byteOffset"] = len(out)
        out += data
        view_map[i] = len(views)
        views.append(nv)
    for a in accessors:
        if "bufferView" in a:
            a["bufferView"] = view_map[a["bufferView"]]
    for im in g.get("images", []):
        if "bufferView" in im:
            im["bufferView"] = view_map[im["bufferView"]]
    for m in g.get("meshes", []):
        for p in m.get("primitives", []):
            p["attributes"] = {k: acc_map[v] for k, v in p.get("attributes", {}).items()}
            if "indices" in p:
                p["indices"] = acc_map[p["indices"]]
            if "targets" in p:
                p["targets"] = [{k: acc_map[v] for k, v in t.items()} for t in p["targets"]]
    for sk in g.get("skins", []):
        if "inverseBindMatrices" in sk:
            sk["inverseBindMatrices"] = acc_map[sk["inverseBindMatrices"]]
    for an in g.get("animations", []):
        for sm in an["samplers"]:
            sm["input"], sm["output"] = acc_map[sm["input"]], acc_map[sm["output"]]
    g["accessors"], g["bufferViews"] = accessors, views
    return out


def close(path):
    got = read_glb(path)
    if got is None:
        return 0
    g, blob = got
    done = 0
    for an in g.get("animations", []):
        if not an.get("name", "").endswith("-loop") or (an.get("extras") or {}).get("dsor_closed"):
            continue
        new_inputs = {}
        changed = False
        # CONTRACT: one closing time for every sampler of the clip, n * dt of its
        #   keyed tracks. A constant track is two keys (first and last time); closed
        #   one dt after ITS last key it reached twice the clip's length, and the
        #   whole run cycle waited on it, frozen ("j'ai 2s ou l'anim est freeze").
        end = None
        for s in an["samplers"]:
            t = read(g, blob, s["input"])
            if t is not None and len(t) >= 3:
                close_at = float(t[-1][0] + (t[1][0] - t[0][0]))
                end = close_at if end is None else max(end, close_at)
        if end is None:
            continue
        for s in an["samplers"]:
            if s.get("interpolation", "LINEAR") != "LINEAR":
                continue
            t = read(g, blob, s["input"])
            v = read(g, blob, s["output"])
            if t is None or v is None or len(t) < 2 or len(v) != len(t):
                continue
            if end <= float(t[-1][0]):
                continue
            first = v[0].copy()
            path_kind = g["accessors"][s["output"]]["type"]
            if path_kind == "VEC4" and float(np.dot(first, v[-1])) < 0:
                first = -first
            if s["input"] not in new_inputs:
                times = np.vstack([t, [[end]]])
                new_inputs[s["input"]] = append(g, blob, times, "SCALAR", minmax=True)
            s["input"] = new_inputs[s["input"]]
            s["output"] = append(g, blob, np.vstack([v, [first]]), path_kind)
            changed = True
        if changed:
            an.setdefault("extras", {})["dsor_closed"] = True
            done += 1
    if done:
        blob = compact(g, blob)
        g["buffers"][0]["byteLength"] = len(blob) + (4 - len(blob) % 4) % 4
        write_glb(path, g, blob)
    return done


def reopen(path):
    """Undo close(): drop the key it appended to each sampler of a closed clip."""
    got = read_glb(path)
    if got is None:
        return 0
    g, blob = got
    done = 0
    for an in g.get("animations", []):
        if not (an.get("extras") or {}).get("dsor_closed"):
            continue
        new_inputs = {}
        for s in an["samplers"]:
            t = read(g, blob, s["input"])
            v = read(g, blob, s["output"])
            if t is None or v is None or len(t) < 3 or len(v) != len(t):
                continue
            if s["input"] not in new_inputs:
                new_inputs[s["input"]] = append(g, blob, t[:-1], "SCALAR", minmax=True)
            s["input"] = new_inputs[s["input"]]
            s["output"] = append(g, blob, v[:-1], g["accessors"][s["output"]]["type"])
        del an["extras"]["dsor_closed"]
        done += 1
    if done:
        blob = compact(g, blob)
        g["buffers"][0]["byteLength"] = len(blob) + (4 - len(blob) % 4) % 4
        write_glb(path, g, blob)
    return done


if __name__ == "__main__":
    # CONTRACT: characters only (skeleton and monster clips, which the client
    #   repeats). An effect model's clip plays ONCE (crate::gameplay::skills
    #   start_fx_animations) whatever its "-loop" name: closed, its tracks ended
    #   back on their first key -- warrior_mightyswing's emitters went back behind
    #   the warrior before emitting and its swoosh swept twice ("les 3 effets ...
    #   a l'oppose", "le FX clignote 2x").
    # --undo: reopen every clip this closed (anywhere under <assets>).
    args = sys.argv[1:]
    undo = "--undo" in args
    args = [a for a in args if a != "--undo"]
    assets = args[0]
    if args[1:]:
        files = args[1:]
    elif undo:
        files = glob.glob(os.path.join(assets, "**", "*.glb"), recursive=True)
    else:
        files = glob.glob(os.path.join(assets, "characters", "**", "*.glb"), recursive=True)
    total = files_changed = 0
    for f in files:
        n = reopen(f) if undo else close(f)
        if n:
            total += n
            files_changed += 1
    print(f"close_loops: {total} clip(s) {'reopened' if undo else 'closed'} in {files_changed} file(s)")
