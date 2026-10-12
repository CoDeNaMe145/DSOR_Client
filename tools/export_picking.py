#!/usr/bin/env python3
"""The boxes the 2018 client picks actors by, from its picking bundles.

Usage: export_picking.py <export_win32> <assets>

Writes characters/picking.json: Graphics ("characters/<model>") -> [{joint,
pos, rot, size}], one per rigid body of the model's physics composite: an
oriented box on that joint (pos and rot in the joint's space, rot x y z w,
size its full edges). A sphere is written as the box around it.

EVIDENCE: export_win32/picking/{monster,npc}.pkg: "KCIP", u32 1, then per model
  u16 name length, name, u32 length, a <Physics> document (Composite >
  RigidBody name="<joint>_Picking" > BoxShape pos rot size).
  ActorPickingProperty::SetupScaledPickingShapes (0x4B894B) strips "_Picking"
  from each rigid body's name and binds the body to that joint of the model,
  scaled by the joint's scale (the lengths of its matrix's rows); the bundle is
  PickingManager::LoadPickingBundle's "export:picking/%s.pkg" (0x58E1DD).
  Monsters picked by the capsule instead (CapsuleRadius 0.3 for the boar):
  only a sliver of the body took the cursor ("la zone ou je passe la souris
  fait pas tout l'actor").
"""
import json, os, re, struct, sys
import xml.etree.ElementTree as ET

import numpy as np


def quat_mul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]


def quat_rotate(q, v):
    x, y, z, w = q
    u = np.array([x, y, z])
    v = np.array(v)
    return (2 * np.dot(u, v) * u + (w * w - np.dot(u, u)) * v + 2 * w * np.cross(u, v)).tolist()


def floats(s):
    return [float(x) for x in s.split()]


def bundle(path):
    b = open(path, "rb").read()
    assert b[:4] == b"KCIP", path
    o, out = 8, {}
    while o < len(b):
        n = struct.unpack_from("<H", b, o)[0]
        o += 2
        name = b[o:o + n].decode()
        o += n
        ln = struct.unpack_from("<I", b, o)[0]
        o += 4
        doc = ET.fromstring(b[o:o + ln].decode())
        o += ln
        boxes = []
        for body in doc.iter("RigidBody"):
            joint = re.sub(r"_Picking$", "", body.get("name", ""))
            bp, br = floats(body.get("pos", "0 0 0")), floats(body.get("rot", "0 0 0 1"))
            for shape in body:
                sp, sr = floats(shape.get("pos", "0 0 0")), floats(shape.get("rot", "0 0 0 1"))
                if shape.tag == "BoxShape":
                    size = floats(shape.get("size"))
                elif shape.tag == "SphereShape":
                    size = [2 * float(shape.get("radius"))] * 3
                else:
                    continue
                pos = (np.array(bp) + quat_rotate(br, sp)).tolist()
                boxes.append({
                    "joint": joint,
                    "pos": [round(x, 4) for x in pos],
                    "rot": [round(x, 4) for x in quat_mul(br, sr)],
                    "size": size,
                })
        if boxes:
            out[name] = boxes
    return out


if __name__ == "__main__":
    export, assets = sys.argv[1], sys.argv[2]
    shapes = {}
    for f in ("monster.pkg", "npc.pkg"):
        shapes.update(bundle(os.path.join(export, "picking", f)))
    with open(os.path.join(assets, "characters", "picking.json"), "w") as f:
        json.dump(shapes, f, separators=(",", ":"))
    print(f"export_picking: {len(shapes)} model(s), {sum(len(v) for v in shapes.values())} box(es)")
