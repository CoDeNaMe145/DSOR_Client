"""Persistent, field-level level overrides. Standard library only; sources are read-only.

The editable layer belongs in level-overrides/, never in the generated assets.
Cached metadata/SQLite backups let an editor session survive missing game files.
"""
from __future__ import annotations

import copy
from contextlib import closing
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sqlite3
import struct
import tempfile
import uuid
from level_cameras import CAMERA

VERSION = 1
GEOMETRY = "geometry"


def source_key(kind):
    return kind if kind in (GEOMETRY, CAMERA) else 'db4'


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def digest(value):
    return hashlib.sha256(canonical(value).encode("utf-8")).hexdigest()


def read_json(path, default=None):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except FileNotFoundError:
        return copy.deepcopy(default)


def atomic_json(path, value):
    path = Path(path)
    payload = json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix=path.name + ".", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def map_name(value):
    if not re.fullmatch(r"[A-Za-z0-9_-]+", value):
        raise ValueError("Map names may contain letters, numbers, underscores and hyphens only")
    return value


def quote(name):
    return '"' + name.replace('"', '""') + '"'


def encode(value):
    return {"$blob": value.hex().upper()} if isinstance(value, bytes) else value


def decode(value):
    if isinstance(value, dict) and set(value) == {"$blob"}:
        return bytes.fromhex(value["$blob"])
    if isinstance(value, (dict, list)):
        raise ValueError("Database values must be scalar values or a {$blob: hex} object")
    if isinstance(value, float) and not math.isfinite(value):
        raise ValueError("Database numbers must be finite")
    return value


def floats(value):
    """Decode a typed DB4 blob without confusing GUIDs with float vectors."""
    raw = decode(value)
    if not isinstance(raw, bytes) or len(raw) % 4:
        raise ValueError("Expected a float32 blob")
    return list(struct.unpack("<%df" % (len(raw) // 4), raw))


def float_blob(values):
    values = [float(v) for v in values]
    if not all(math.isfinite(v) for v in values):
        raise ValueError("Transform values must be finite")
    return encode(struct.pack("<%df" % len(values), *values))


def position(kind, values):
    try:
        p = values["p"] if kind == GEOMETRY else floats(values["Transform"])[12:15]
        return p if len(p) == 3 and all(math.isfinite(v) for v in p) else None
    except (ValueError, KeyError, TypeError):
        return None


def moved(kind, values, xyz):
    result = copy.deepcopy(values)
    if len(xyz) != 3 or not all(math.isfinite(float(v)) for v in xyz):
        raise ValueError("Position needs three finite numbers")
    if kind == GEOMETRY:
        result["p"] = [float(v) for v in xyz]
    else:
        matrix = floats(result["Transform"])
        if len(matrix) != 16:
            raise ValueError("Transform must contain 16 float32 values")
        matrix[12:15] = xyz
        result["Transform"] = float_blob(matrix)
    return result


def geometry_records(manifest):
    records, counts = {}, {}
    for row in [*manifest.get("instances", []), *manifest.get("event_instances", [])]:
        values = copy.deepcopy(row)
        values["model"] = manifest["models"][values.pop("m")]
        fingerprint = digest(values)
        occurrence = counts.get(fingerprint, 0)
        counts[fingerprint] = occurrence + 1
        records[f"placement:{fingerprint}:{occurrence}"] = values
    return records


def manifest_from(base, records, name):
    manifest = copy.deepcopy(base or {"map": name, "center": [0, 0, 0], "extents": [50, 20, 50]})
    models = list(manifest.get("models", []))
    indexes = {model: i for i, model in enumerate(models)}
    manifest["instances"] = []
    if "event_instances" in manifest:
        manifest["event_instances"] = []
    for values in records.values():
        row = copy.deepcopy(values)
        model = row.pop("model")
        if model not in indexes:
            indexes[model] = len(models)
            models.append(model)
        row["m"] = indexes[model]
        manifest.setdefault("event_instances" if row.get("event") else "instances", []).append(row)
    manifest["models"] = models
    return manifest


def database_snapshot(path):
    schemas, records = {}, {}
    with closing(sqlite3.connect(Path(path).resolve().as_uri() + "?mode=ro", uri=True)) as connection:
        connection.row_factory = sqlite3.Row
        names = [row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
        for name in names:
            if not name.startswith("_Instance_"):
                continue
            columns = [dict(row) for row in connection.execute(f"PRAGMA table_info({quote(name)})")]
            keys = [col["name"] for col in sorted(columns, key=lambda col: col["pk"]) if col["pk"]]
            schemas[name] = {"columns": columns, "keys": keys}
            rows, counts = {}, {}
            for row in connection.execute(f"SELECT * FROM {quote(name)}"):
                values = {key: encode(row[key]) for key in row.keys()}
                identity = {key: values[key] for key in keys} if keys else values
                fingerprint = digest(identity)
                occurrence = counts.get(fingerprint, 0)
                counts[fingerprint] = occurrence + 1
                rows[f"row:{fingerprint}:{occurrence}"] = values
            records[name] = rows
    return schemas, records


def empty_patch(name):
    return {"version": VERSION, "map": map_name(name), "operations": []}


def validate_patch(patch, name):
    if patch.get("version") != VERSION or patch.get("map") != name:
        raise ValueError("Unsupported override version or mismatched map name")
    seen = set()
    for op in patch["operations"]:
        key = (op["kind"], op["id"])
        if key in seen or op["action"] not in ("add", "update", "delete"):
            raise ValueError("Duplicate or invalid override operation")
        seen.add(key)
        if op["kind"] not in (GEOMETRY, CAMERA) and not op["kind"].startswith("_Instance_"):
            raise ValueError("Only geometry and map instance tables can be edited")
        if op["action"] != "add" and not isinstance(op.get("base"), dict):
            raise ValueError("Updates and deletions need their original record")
        if op["action"] != "delete" and not isinstance(op.get("values"), dict):
            raise ValueError("Override values must be an object")


def compose(base, patch, offline=False):
    """Three-way, field-level composition; never silently replace upstream edits.

    Unavailable targets remain in the patch and are visible as conflicts. Offline
    mode may use saved original records to show/edit placeholders, but cannot be
    exported as a complete map without a baseline.
    """
    result = copy.deepcopy(base)
    conflicts = []
    for op in patch["operations"]:
        kind, ident = op["kind"], op["id"]
        rows = result.setdefault(kind, {})
        current = rows.get(ident)
        problem = None
        if kind != GEOMETRY and kind not in base:
            conflicts.append({"kind": kind, "id": ident, "reason": "Original DB4 table is unavailable or was removed"})
            if op["action"] == "add":
                rows[ident] = copy.deepcopy(op["values"])
            continue
        if op["action"] == "add":
            if current is not None and current != op["values"]:
                problem = "Added identity already exists in the base"
            else:
                rows[ident] = copy.deepcopy(op["values"])
        elif op["action"] == "delete":
            if current is None and kind == GEOMETRY:
                problem = "Deleted placement is missing or its original identity changed"
            elif current is not None and current != op["base"]:
                problem = "Deleted record changed in the base"
            else:
                rows.pop(ident, None)
        else:
            if current is None and offline:
                current = copy.deepcopy(op["base"])
                rows[ident] = current
            if current is None:
                problem = "Original record is missing or its placement identity changed"
            else:
                changed = [key for key, value in op["values"].items()
                           if key not in current or (current[key] != op["base"].get(key) and current[key] != value)]
                if changed:
                    problem = "Base changed the edited fields: " + ", ".join(changed)
                else:
                    current.update(copy.deepcopy(op["values"]))
        if problem:
            conflicts.append({"kind": kind, "id": ident, "reason": problem})
    return result, conflicts


class Project:
    def __init__(self, root, assets=None, db_root=None, overrides=None, cache=None):
        self.root = Path(root).resolve()
        self.assets = Path(assets or self.root / "assets").resolve()
        self.db_root = Path(db_root).resolve() if db_root else None
        self.overrides = Path(overrides or self.root / "level-overrides").resolve()
        self.cache = Path(cache or self.root / ".level-editor").resolve()
        if self.overrides == self.assets or self.assets in self.overrides.parents:
            raise ValueError("Overrides must be stored outside generated assets")
        if self.cache == self.assets or self.assets in self.cache.parents:
            raise ValueError("Metadata cache must be stored outside generated assets")

    def maps(self):
        names = set()
        for folder, suffix in [(self.assets / "maps", ".map.json"), (self.overrides, ".json"),
                               (self.cache / "maps", ".json"), (self.db_root, ".db4")]:
            if folder and folder.is_dir():
                names.update(p.name[:-len(suffix)] for p in folder.glob("*" + suffix))
        return sorted(name for name in names if re.fullmatch(r"[A-Za-z0-9_-]+", name))

    def patch_path(self, name):
        return self.overrides / (map_name(name) + ".json")

    def open(self, name, refresh=True):
        return Session(self, map_name(name), refresh)


class Session:
    def __init__(self, project, name, refresh=True):
        self.project, self.name = project, name
        self.patch = read_json(project.patch_path(name), empty_patch(name))
        validate_patch(self.patch, name)
        self.saved = digest(self.patch)
        self.undo_stack, self.redo_stack = [], []
        self.snapshot_path = project.cache / "maps" / (name + ".json")
        self.snapshot = read_json(self.snapshot_path, {"version": VERSION, "map": name,
                                  "manifest": None, "schemas": {}, "records": {}})
        self.sources = {CAMERA: 'cached' if self.snapshot['records'].get(CAMERA) else 'unavailable',
                        "geometry": "cached" if self.snapshot["manifest"] else "unavailable",
                        "db4": "cached" if self.snapshot["schemas"] else "unavailable"}
        if refresh:
            self.refresh_sources()
        self.recompose()

    def refresh_sources(self):
        p = self.project
        manifest = read_json(p.assets / "maps" / (self.name + ".map.json"))
        # Provenance travels with the generated file. Even losing the cache must
        # never cause an already composed map to become its own original.
        if manifest is not None:
            provenance = manifest.get("_level_override")
            if provenance is None:
                from level_map_events import with_event_layers
                enriched = with_event_layers(manifest, p.db_root / (self.name + ".map"), self.snapshot["manifest"]) if p.db_root else None
                if enriched is not None:
                    manifest = enriched
                elif "event_instances" not in manifest and self.snapshot["manifest"]:
                    # Asset-only rebuilds must not erase cached event sources.
                    cached = self.snapshot["manifest"]
                    event_rows = geometry_records({**cached, "instances": []})
                    combined = {**geometry_records(manifest), **event_rows}
                    manifest = manifest_from(manifest, combined, self.name)
                    manifest["event_rules"] = cached.get("event_rules", {})
                self.snapshot["manifest"] = manifest
                self.snapshot["records"][GEOMETRY] = geometry_records(manifest)
                self.sources[GEOMETRY] = "available"
            elif self.snapshot["manifest"] and digest(self.snapshot["manifest"]) == provenance.get("base_sha256"):
                self.sources[GEOMETRY] = "cached"
            else:
                self.snapshot["manifest"] = None
                self.snapshot["records"].pop(GEOMETRY, None)
                self.sources[GEOMETRY] = "unavailable"
        db = p.db_root / (self.name + ".db4") if p.db_root else None
        if db and db.is_file():
            backup = p.cache / "db4" / db.name
            backup.parent.mkdir(parents=True, exist_ok=True)
            fd, temporary = tempfile.mkstemp(dir=backup.parent, suffix=".db4")
            os.close(fd)
            # SQLite backup includes committed WAL content and all original
            # tables, indexes and BLOB bytes, including uneditable metadata.
            try:
                with closing(sqlite3.connect(db.as_uri() + "?mode=ro", uri=True)) as source:
                    with closing(sqlite3.connect(temporary)) as target:
                        source.backup(target)
                schemas, rows = database_snapshot(temporary)
                os.replace(temporary, backup)
            finally:
                if os.path.exists(temporary):
                    os.unlink(temporary)
            self.snapshot["schemas"] = schemas
            cameras = self.snapshot['records'].get(CAMERA)
            self.snapshot["records"] = {GEOMETRY: self.snapshot["records"].get(GEOMETRY, {}), **rows}
            if cameras is not None:
                self.snapshot['records'][CAMERA] = cameras
            self.sources["db4"] = "available"
        from level_cameras import refresh_source
        refresh_source(self)
        atomic_json(self.snapshot_path, self.snapshot)

    def recompose(self):
        self.records, self.conflicts = compose(self.snapshot["records"], self.patch)
        # Patch-only sessions still show authored rows when the cache is absent.
        for op in self.patch["operations"]:
            missing_source = self.sources[source_key(op['kind'])] == "unavailable"
            if missing_source and op["action"] == "update":
                self.records.setdefault(op["kind"], {}).setdefault(op["id"], {**op["base"], **op["values"]})

    @property
    def dirty(self):
        return digest(self.patch) != self.saved

    def checkpoint(self):
        self.undo_stack.append(copy.deepcopy(self.patch))
        self.undo_stack = self.undo_stack[-100:]
        self.redo_stack.clear()

    def undo(self):
        if self.undo_stack:
            self.redo_stack.append(self.patch)
            self.patch = self.undo_stack.pop()
            self.recompose()

    def redo(self):
        if self.redo_stack:
            self.undo_stack.append(self.patch)
            self.patch = self.redo_stack.pop()
            self.recompose()

    def operation(self, kind, ident):
        return next((op for op in self.patch["operations"] if (op["kind"], op["id"]) == (kind, ident)), None)

    def validate_values(self, kind, values, original=None):
        canonical(values)  # Reject NaN and unsupported JSON types before saving.
        from level_lighting import validate as validate_lighting
        validate_lighting(kind, values, original)
        if kind == CAMERA:
            from level_cameras import validate
            validate(values)
        if kind == GEOMETRY:
            if "event" in values and (not isinstance(values['event'], str) or any(c in values['event'] for c in ';=\n\r')):
                raise ValueError("Event must be a single event flag name")
            if not isinstance(values.get("model"), str) or not values["model"].strip():
                raise ValueError("A placement needs a model reference (the model file may be missing)")
            for key, size in (("p", 3), ("r", 4), ("s", 3)):
                vector = values.get(key, [])
                if len(vector) != size or not all(isinstance(v, (int, float)) and math.isfinite(v) for v in vector):
                    raise ValueError(f"{key} requires {size} finite numbers")
            norm = sum(v * v for v in values["r"])
            if abs(norm - 1) > 0.002:
                raise ValueError("Rotation must be a unit quaternion (x, y, z, w)")
            if any(abs(v) < 1e-7 for v in values["s"]):
                raise ValueError("Scale cannot be zero")
            rel = values["model"].split(":")[-1].replace("\\", "/")
            if rel.startswith("/") or ".." in rel.split("/"):
                raise ValueError("Model references must stay inside the asset tree")
            return
        schema = self.snapshot["schemas"].get(kind)
        if schema:
            columns = {col["name"]: col for col in schema["columns"]}
            if set(values) != set(columns):
                raise ValueError("Database records must preserve all schema columns")
            for key, value in values.items():
                raw, col = decode(value), columns[key]
                if raw is None:
                    if col["notnull"] or col["pk"]:
                        raise ValueError(f"{key} cannot be null")
                    continue
                affinity = col["type"].upper()
                expected = bytes if "BLOB" in affinity else int if "INT" in affinity else (int, float) if any(t in affinity for t in ("REAL", "FLOA", "DOUB")) else str
                if not isinstance(raw, expected):
                    raise ValueError(f"{key} must match its {affinity} column type")
            if original and any(values[k] != original[k] for k in schema["keys"]):
                raise ValueError("Primary keys are stable identities; duplicate the record to create a new one")
        else:
            for value in values.values():
                decode(value)
        if "Guid" in values and (not isinstance(decode(values["Guid"]), bytes) or len(decode(values["Guid"])) != 16):
            raise ValueError("Guid must be a 16-byte blob")
        if original and "Guid" in values and values["Guid"] != original["Guid"]:
            raise ValueError("Guid is a stable identity; duplicate the record to create a new one")
        if values.get("Transform") is not None:
            matrix = floats(values["Transform"])
            if len(matrix) != 16 or not all(math.isfinite(v) for v in matrix):
                raise ValueError("Transform must contain 16 finite float32 values")
        for key in ("LightColor", "LightOppositeColor", "LightAmbient", "Balance", "BloomColor", "FogColor",
                    "HeightFogColor", "VignetteColor", "LocalLightColor", "RimLightColor", "PrimaryColor", "SecondaryColor"):
            if isinstance(values.get(key), dict):
                vector = floats(values[key])
                if len(vector) != 4 or not all(math.isfinite(v) for v in vector):
                    raise ValueError(f"{key} must contain four finite float32 values")

    def update(self, kind, ident, values):
        old = self.records[kind][ident]
        self.validate_values(kind, values, old)
        if values == old:
            return
        op = self.operation(kind, ident)
        original = self.snapshot["records"].get(kind, {}).get(ident) or (op.get("base") if op else None)
        if any(c["kind"] == kind and c["id"] == ident for c in self.conflicts) and self.sources[source_key(kind)] != "unavailable":
            raise ValueError("Resolve this record's conflict before editing it")
        if original is not None and set(values) != set(original):
            raise ValueError("Preserve the record's original fields")
        self.checkpoint()
        if op and op["action"] == "add":
            op["values"] = copy.deepcopy(values)
        else:
            delta = {k: v for k, v in values.items() if v != original[k]}
            self.patch["operations"] = [o for o in self.patch["operations"] if (o["kind"], o["id"]) != (kind, ident)]
            if delta:
                self.patch["operations"].append({"kind": kind, "id": ident, "action": "update", "base": original, "values": delta})
        self.recompose()

    def add(self, kind, values):
        if kind == CAMERA:
            raise ValueError('The character-selection level uses one fixed camera. Edit its existing camera instead.')
        self.validate_values(kind, values)
        if kind != GEOMETRY:
            keys = self.snapshot["schemas"].get(kind, {}).get("keys", [])
            if keys and any(all(row[k] == values[k] for k in keys) for row in self.records.get(kind, {}).values()):
                raise ValueError("A record with this primary key already exists")
        self.checkpoint()
        ident = "added:" + str(uuid.uuid4())
        self.patch["operations"].append({"kind": kind, "id": ident, "action": "add", "values": copy.deepcopy(values)})
        self.recompose()
        return ident

    def duplicate(self, kind, ident):
        if kind == CAMERA:
            raise ValueError('The character-selection level uses one fixed camera. Edit its existing camera instead.')
        values = copy.deepcopy(self.records[kind][ident])
        if kind != GEOMETRY:
            keys = self.snapshot["schemas"].get(kind, {}).get("keys", [])
            if keys != ["Guid"]:
                raise ValueError("Duplicate is available for records identified by Guid")
            values["Guid"] = encode(uuid.uuid4().bytes)
        return self.add(kind, values)

    def delete(self, kind, ident):
        if kind == CAMERA:
            raise ValueError('This level requires its fixed camera. Move it or revert its overrides instead.')
        op = self.operation(kind, ident)
        original = self.snapshot["records"].get(kind, {}).get(ident) or (op.get("base") if op else None)
        self.checkpoint()
        self.patch["operations"] = [o for o in self.patch["operations"] if (o["kind"], o["id"]) != (kind, ident)]
        if not op or op["action"] != "add":
            self.patch["operations"].append({"kind": kind, "id": ident, "action": "delete", "base": original})
        self.recompose()

    def revert(self, kind, ident):
        self.checkpoint()
        self.patch["operations"] = [o for o in self.patch["operations"] if (o["kind"], o["id"]) != (kind, ident)]
        self.recompose()

    def keep_override(self, kind, ident):
        """Explicit conflict resolution, only when the original identity exists."""
        op = self.operation(kind, ident)
        base = self.snapshot["records"].get(kind, {}).get(ident)
        if not op or op["action"] == "add" or base is None:
            raise ValueError("Cannot rebase a missing identity; revert and reapply to the intended record")
        self.checkpoint()
        op["base"] = copy.deepcopy(base)
        self.recompose()

    def save(self):
        path = self.project.patch_path(self.name)
        disk = read_json(path, empty_patch(self.name))
        if digest(disk) != self.saved:
            raise ValueError("Overrides changed in another editor. Reopen before saving")
        validate_patch(self.patch, self.name)
        atomic_json(path, self.patch)
        self.saved = digest(self.patch)

    def manifest(self):
        return manifest_from(self.snapshot["manifest"], self.records.get(GEOMETRY, {}), self.name)

    def export_db(self, destination):
        """Materialize a derived DB4, never overwrite a source or cached baseline."""
        destination = Path(destination).resolve()
        source = self.project.cache / "db4" / (self.name + ".db4")
        original = self.project.db_root / (self.name + ".db4") if self.project.db_root else None
        if destination in (source, original):
            raise ValueError("Export destination cannot be an original or cached source DB4")
        if self.conflicts:
            raise ValueError("Resolve override conflicts before exporting")
        if not source.is_file():
            raise ValueError("No DB4 baseline is available; save the overrides and export when it returns")
        schemas, records = database_snapshot(source)
        expected = {kind: rows for kind, rows in self.snapshot["records"].items() if kind not in (GEOMETRY, CAMERA)}
        if schemas != self.snapshot["schemas"] or records != expected:
            raise ValueError("The cached DB4 changed in another session; reopen the map before exporting")
        destination.parent.mkdir(parents=True, exist_ok=True)
        fd, temporary = tempfile.mkstemp(dir=destination.parent, suffix=".db4")
        os.close(fd)
        try:
            with closing(sqlite3.connect(source.as_uri() + "?mode=ro", uri=True)) as base:
                with closing(sqlite3.connect(temporary)) as target, target:
                    base.backup(target)
                    for kind, schema in self.snapshot["schemas"].items():
                        relevant = [op for op in self.patch["operations"] if op["kind"] == kind]
                        for op in relevant:
                            if op["action"] == "add":
                                values = op["values"]
                                self.validate_values(kind, values)
                                keys = list(values)
                                target.execute(f"INSERT INTO {quote(kind)} ({','.join(map(quote, keys))}) VALUES ({','.join('?' for _ in keys)})",
                                               [decode(values[k]) for k in keys])
                                continue
                            original_row = self.snapshot["records"][kind].get(op["id"])
                            if original_row is None and op["action"] == "delete":
                                continue
                            keys = schema["keys"] or list(original_row)
                            where = " AND ".join(f"{quote(k)} IS ?" for k in keys)
                            args = [decode(original_row[k]) for k in keys]
                            if target.execute(f"SELECT COUNT(*) FROM {quote(kind)} WHERE {where}", args).fetchone()[0] != 1:
                                raise ValueError(f"Ambiguous DB4 identity in {kind}; export aborted")
                            if op["action"] == "delete":
                                target.execute(f"DELETE FROM {quote(kind)} WHERE {where}", args)
                            else:
                                values = op["values"]
                                self.validate_values(kind, {**original_row, **values}, original_row)
                                assignments = ",".join(f"{quote(k)}=?" for k in values)
                                target.execute(f"UPDATE {quote(kind)} SET {assignments} WHERE {where}", [decode(v) for v in values.values()] + args)
                    if target.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
                        raise ValueError("Generated database failed SQLite integrity_check")
            os.replace(temporary, destination)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
        return destination
