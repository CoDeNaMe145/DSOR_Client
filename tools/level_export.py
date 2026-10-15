"""Build client map sidecars and a derived DB4 from the same effective records."""
from __future__ import annotations

import math
from pathlib import Path

from level_data import GEOMETRY, atomic_json, decode, digest, floats, read_json, source_key
from level_cameras import CAMERA, selection_export
from level_lighting import document as lighting_document

def matrix(row):
    try:
        values = floats(row["Transform"])
        return values if len(values) == 16 else None
    except (KeyError, TypeError, ValueError):
        return None


def track_matrix(at):
    tx, ty, tz, rx, ry, rz, sx, sy, sz = at
    cx, cy, cz = (math.cos(math.radians(a)) for a in (rx, ry, rz))
    ax, ay, az = (math.sin(math.radians(a)) for a in (rx, ry, rz))
    return [cy * cz * sx, cy * az * sx, -ay * sx, 0,
            (ax * ay * cz - cx * az) * sy, (ax * ay * az + cx * cz) * sy, ax * cy * sy, 0,
            (cx * ay * cz + ax * az) * sz, (cx * ay * az - ax * cz) * sz, cx * cy * sz, 0,
            tx, ty, tz, 1]


def client_documents(session):
    """DB4 -> the existing Rust loaders' formats; no alternate runtime schema."""
    rows = session.records
    assets = session.project.assets
    documents = {}
    table = lambda name: rows.get("_Instance_" + name, {}).values()
    has = lambda name: "_Instance_" + name in session.snapshot["schemas"]
    if session.snapshot["manifest"] is not None or rows.get(GEOMETRY):
        documents["map.json"] = session.manifest()
    if has("NPC"):
        templates = read_json(assets / "characters" / "npc_templates.json", {})
        npcs = {}
        for row in table("NPC"):
            at = matrix(row)
            if at is None:
                continue
            template = templates.get(row.get("Id"), {})
            look = {key: row.get(column, template.get(key, fallback)) or fallback for key, column, fallback in (
                ("graphics", "Graphics", ""), ("outfit", "CharacterSet", ""), ("anim_set", "AnimSet", ""),
                ("state", "StartAnimationState", "Idle"))}
            look.update(loop=bool(row.get("LoopStartAnimation")), title=template.get("title", ""))
            guid = decode(row["Guid"]).hex().upper()
            npcs[guid] = {"id": row.get("Id", ""), "m": at, "look": look}
        documents["npcs.json"] = npcs
    if has("InteractExit"):
        exits = []
        for row in table("InteractExit"):
            at = matrix(row)
            if at is None or not row.get("ExitURL") or not row.get("Graphics"):
                continue
            exits.append({"id": row.get("Id", ""), "name": row.get("Name") or "",
                          "url": row["ExitURL"], "entry": row.get("EntryName") or "",
                          "graphics": row["Graphics"], "range": row.get("PickingRange") or 0,
                          "event": row.get("EventSetRow", -1), "m": at})
        documents["exits.json"] = exits
    if has("AmbienceBubble") or has("Light"):
        documents["ambience.json"] = lighting_document(session)
    if has("Sequence"):
        sequences = read_json(assets / "skills" / "sequences.json")
        anims = read_json(assets / "characters" / "anims.json")
        changed = {op["id"] for op in session.patch["operations"] if op["kind"] == "_Instance_Sequence"}
        if sequences is None or anims is None:
            if changed:
                raise ValueError("Sequence export needs skills/sequences.json and characters/anims.json; the overrides can still be saved")
        else:
            deco = {"chars": [], "seqs": []}
            for ident, row in rows.get("_Instance_Sequence", {}).items():
                at = matrix(row)
                if at is None or row.get("EventSetRow", -1) != -1 or not row.get("SequenceIsPlaying"):
                    continue
                name = (row.get("SequencerFileName") or "").split("/", 1)[-1]
                sequence = sequences.get(name)
                if sequence is None:
                    if ident in changed:
                        raise ValueError(f"Missing sequence definition: {name}")
                    continue
                effect = False
                for track in sequence["tracks"]:
                    if track["kind"] != "character":
                        effect |= track["kind"] in ("fx", "light")
                        continue
                    model, _, rest = track["object"].partition("/")
                    outfit, _, clip = rest.partition("/")
                    clip = clip + "-loop" if "deco:" + clip + "-loop" in anims else clip
                    anim_set = "deco:" + clip
                    if anim_set not in anims:
                        if ident in changed:
                            raise ValueError(f"Animation set {anim_set} is unavailable; run export_deco.py with the required model assets")
                        continue
                    local = track_matrix(track["at"])
                    transform = [sum(local[i * 4 + k] * at[k * 4 + j] for k in range(4)) for i in range(4) for j in range(4)]
                    deco["chars"].append({"g": "characters/" + model, "outfit": outfit, "set": anim_set,
                                          "m": transform, "speed": track.get("speed", 1)})
                if effect:
                    deco["seqs"].append({"seq": name, "m": at})
            documents["deco.json"] = deco
    return documents


def build(session, assets=None, db_out=None):
    if session.conflicts:
        raise ValueError(f"Resolve {len(session.conflicts)} override conflicts before building")
    for op in session.patch["operations"]:
        if session.sources[source_key(op['kind'])] == "unavailable":
            raise ValueError("A complete baseline is unavailable. Overrides are saved; restore the source or metadata cache before building")
        if op["action"] != "delete":
            values = session.records[op["kind"]][op["id"]]
            original = session.snapshot["records"].get(op["kind"], {}).get(op["id"])
            session.validate_values(op["kind"], values, original)
    # Prepare/validate every document before changing client files.
    documents = client_documents(session)
    camera_catalog = selection_export(session) if session.records.get(CAMERA) else None
    if "map.json" in documents:
        documents["map.json"]["_level_override"] = {
            "version": 1, "base_sha256": digest(session.snapshot["manifest"]),
            "patch_sha256": digest(session.patch),
        }
    target = Path(assets or session.project.assets).resolve()
    db_target = Path(db_out or session.project.cache / "build" / "maps") / (session.name + ".db4")
    database = None
    if session.snapshot["schemas"]:
        database = session.export_db(db_target)
    for suffix, document in documents.items():
        atomic_json(target / "maps" / (session.name + "." + suffix), document)
    if camera_catalog is not None:
        atomic_json(target / 'characters/selection.json', camera_catalog)
    geometry_changed = any(op["kind"] == GEOMETRY for op in session.patch["operations"])
    report = {"map": session.name, "files": list(documents), "database": str(database) if database else None,
              "navigation_rebuild_required": geometry_changed,
              "note": "Geometry edits do not regenerate PathEngine navigation or .col collision files." if geometry_changed else ""}
    if camera_catalog is not None:
        report['files'].append('characters/selection.json')
    atomic_json(session.project.cache / "build" / (session.name + ".report.json"), report)
    return report
