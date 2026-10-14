#!/usr/bin/env python3
"""The levels' decorative sequences: the life of a town that the server never sends.

Usage: export_deco.py <export_win32> <assets>
Run after export_skills.py (skills/sequences.json) and character_tables.py
(characters/anims.json, which this extends).

A level's _Instance_Sequence rows (Kingshill has 413) play a sequence from
sequences/*.pbxml where they stand, looping: guards at their posts, townsfolk
sitting, pigeons, cats and chickens (a CharacterTrackBar each), light beams, rain,
bats (effect tracks). Only rows with EventSetRow -1 are always there; the others
belong to an event (anniversary, Halloween...) and are left out.

Writes:
  maps/<map>.deco.json  {"chars": [{g, outfit, set, m, speed}],
                         "seqs": [{seq, m}]}
      chars  a character to place: its model (characters/uniskel[_dwarf] or a
             whole model), its character set, the anim set playing its clip as
             "Idle", the 4x4 placement (DirectX row-major, 16 floats, the
             sequence's graphics transform applied) and the clip's speed
      seqs   a sequence with effects to play there, looping (its characters
             left to "chars")
  characters/anims.json gains one set per clip, "deco:<clip>" -> {"Idle": clip}
      (the clip as the model names it: uniskel's carry "-loop")
"""
import json, os, sqlite3, struct, sys

sys.path.insert(0, os.path.dirname(__file__))
from fix_materials_io import read_glb  # noqa: E402

root, assets = sys.argv[1], sys.argv[2]
sequences = json.load(open(os.path.join(assets, "skills", "sequences.json")))
anims_path = os.path.join(assets, "characters", "anims.json")
anims = json.load(open(anims_path))

clips_cache = {}


def clips(graphics):
    """The model's animation names (uniskel's are in its __animations.glb)."""
    if graphics not in clips_cache:
        rel = graphics + ("/__animations.glb" if graphics.endswith(("uniskel", "uniskel_dwarf")) else ".glb")
        try:
            doc, _ = read_glb(os.path.join(assets, rel))
            clips_cache[graphics] = {a.get("name", "") for a in doc.get("animations", [])}
        except Exception:  # noqa: BLE001 -- not converted: nothing to play
            clips_cache[graphics] = set()
    return clips_cache[graphics]


def matrix(v):
    return list(struct.unpack("<16f", v)) if isinstance(v, bytes) and len(v) == 64 else None


def local(at):
    """A track's [tx, ty, tz, rx, ry, rz, sx, sy, sz] (degrees) as a row-major 4x4."""
    import math
    tx, ty, tz, rx, ry, rz, sx, sy, sz = at
    if not any((tx, ty, tz, rx, ry, rz)) and (sx, sy, sz) == (1, 1, 1):
        return None
    cx, cy, cz = (math.cos(math.radians(a)) for a in (rx, ry, rz))
    snx, sny, snz = (math.sin(math.radians(a)) for a in (rx, ry, rz))
    # Rotation X then Y then Z, row vectors (DirectX).
    r = [[cy * cz, cy * snz, -sny],
         [snx * sny * cz - cx * snz, snx * sny * snz + cx * cz, snx * cy],
         [cx * sny * cz + snx * snz, cx * sny * snz - snx * cz, cx * cy]]
    return [r[0][0] * sx, r[0][1] * sx, r[0][2] * sx, 0, r[1][0] * sy, r[1][1] * sy, r[1][2] * sy, 0,
            r[2][0] * sz, r[2][1] * sz, r[2][2] * sz, 0, tx, ty, tz, 1]


def mul(a, b):
    """Row-major 4x4 a x b."""
    return [sum(a[i * 4 + k] * b[k * 4 + j] for k in range(4)) for i in range(4) for j in range(4)]


stats = {"maps": 0, "chars": 0, "seqs": 0, "no clip": 0, "no sequence": 0, "event only": 0}
missing = set()
for name in sorted(os.listdir(os.path.join(root, "maps"))):
    if not name.endswith(".db4"):
        continue
    level = name[:-4]
    con = sqlite3.connect(f"file:{os.path.join(root, 'maps', name)}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        rows = con.execute("select Transform, EventSetRow, SequencerFileName, SequenceIsPlaying from _Instance_Sequence").fetchall()
    except sqlite3.OperationalError:
        continue
    doc = {"chars": [], "seqs": []}
    for r in rows:
        if r["EventSetRow"] != -1:
            stats["event only"] += 1
            continue
        m = matrix(r["Transform"])
        seq_name = (r["SequencerFileName"] or "").split("/", 1)[-1]
        seq = sequences.get(seq_name)
        if not m or not seq or not r["SequenceIsPlaying"]:
            stats["no sequence"] += 1
            continue
        effects = False
        for t in seq["tracks"]:
            if t["kind"] != "character":
                effects |= t["kind"] in ("fx", "light")
                continue
            model, _, rest = t["object"].partition("/")
            outfit, _, clip = rest.partition("/")
            graphics = "characters/" + model
            have = clips(graphics)
            name_ = clip + "-loop" if clip + "-loop" in have else clip
            if name_ not in have:
                stats["no clip"] += 1
                missing.add(t["object"])
                continue
            anims.setdefault("deco:" + name_, {"Idle": name_})
            lm = local(t["at"])
            doc["chars"].append({"g": graphics, "outfit": outfit, "set": "deco:" + name_,
                                 "m": [round(x, 4) for x in (mul(lm, m) if lm else m)], "speed": t.get("speed", 1.0)})
            stats["chars"] += 1
        if effects:
            doc["seqs"].append({"seq": seq_name, "m": [round(x, 4) for x in m]})
            stats["seqs"] += 1
    if doc["chars"] or doc["seqs"]:
        with open(os.path.join(assets, "maps", f"{level}.deco.json"), "w") as f:
            json.dump(doc, f, separators=(",", ":"))
        stats["maps"] += 1
with open(anims_path, "w") as f:
    json.dump(anims, f, separators=(",", ":"))
print(stats)
if missing:
    print("no clip:", sorted(missing)[:20])
