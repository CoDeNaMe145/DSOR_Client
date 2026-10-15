"""Persistence/integration regressions for the authored level layer; no game assets needed."""
import copy
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from level_data import GEOMETRY, Project, atomic_json, decode, digest, encode, float_blob, floats, moved, read_json
from level_export import build, client_documents


class LevelOverridesTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.assets = self.root / "assets"
        self.db_root = self.root / "originals"
        self.db_root.mkdir()
        self.db = self.db_root / "test.db4"
        self.manifest = {"map": "test", "center": [0, 0, 0], "extents": [20, 10, 20], "models": ["props/tree", "props/rock"],
                         "groups": [{"name": "original", "type": 0, "parent": -1}], "nav_blockers": [[[0, 0], [1, 0], [1, 1]]],
                         "instances": [{"m": 0, "p": [1, 2, 3], "r": [0, 0, 0, 1], "s": [1, 1, 1], "n": "tree", "g": -1, "col": True},
                                       {"m": 1, "p": [5, 6, 7], "r": [0, 0, 0, 1], "s": [1, 1, 1], "n": "rock", "g": -1, "col": True}]}
        atomic_json(self.assets / "maps/test.map.json", self.manifest)
        self.guid = bytes(range(16))
        self.transform = decode(float_blob([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 10, 20, 30, 1]))
        with closing(sqlite3.connect(self.db)) as connection, connection:
            connection.executescript('''
                CREATE TABLE _Attributes (Name TEXT, Data BLOB);
                INSERT INTO _Attributes VALUES ('original', X'0000FF00');
                CREATE TABLE _Instance_NPC (Guid BLOB PRIMARY KEY, Id TEXT, Transform BLOB,
                    Graphics TEXT, CharacterSet TEXT, AnimSet TEXT, StartAnimationState TEXT,
                    LoopStartAnimation INTEGER, PickingRange REAL, EventSetRow INTEGER);
                CREATE INDEX npc_template ON _Instance_NPC(Id);
                CREATE TABLE _Instance_InteractExit (Guid BLOB PRIMARY KEY, Id TEXT, Transform BLOB,
                    Name TEXT, ExitURL TEXT, EntryName TEXT, Graphics TEXT, PickingRange REAL, EventSetRow INTEGER);
                CREATE TABLE _Instance_Light (Guid BLOB PRIMARY KEY, Transform BLOB, EventSetRow INTEGER,
                    LightType INTEGER, LightColor BLOB, LightIntensity REAL, LightRange REAL,
                    LightFlickerEnable INTEGER, LightFlickerFrequency REAL, LightFlickerIntensity REAL,
                    LightConeAngle REAL, LightCastShadows INTEGER);
            ''')
            connection.execute("INSERT INTO _Instance_NPC VALUES (?,?,?,?,?,?,?,?,?,?)",
                               (self.guid, "guard", self.transform, "characters/guard", "armor", "guard_idle", "Idle", 1, 2.0, -1))
            connection.execute("INSERT INTO _Instance_InteractExit VALUES (?,?,?,?,?,?,?,?,?)",
                               (self.guid, "exit", self.transform, "door", "other_map", "entry", "props/door", 4.0, -1))
            connection.execute("INSERT INTO _Instance_Light VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                               (self.guid, self.transform, -1, 0, decode(float_blob([1, 0.5, 0.2, 1])), 2.0, 5.0, 0, 0, 0, 0, 0))
        self.original_bytes = self.db.read_bytes()
        self.project = Project(self.root, self.assets, self.db_root)

    def session(self):
        return self.project.open("test")

    @staticmethod
    def first(session, kind=GEOMETRY):
        ident = next(iter(session.records[kind]))
        return ident, session.records[kind][ident]

    def test_rebuild_reorders_models_and_placements_without_losing_edits(self):
        session = self.session()
        ident, tree = self.first(session)
        session.update(GEOMETRY, ident, moved(GEOMETRY, tree, [90, 2, 40]))
        session.save()
        build(session)
        rebuilt = copy.deepcopy(self.manifest)
        rebuilt["models"].reverse()
        for row in rebuilt["instances"]:
            row["m"] = 1 - row["m"]
        rebuilt["instances"].reverse()
        atomic_json(self.assets / "maps/test.map.json", rebuilt)
        reopened = self.session()
        self.assertFalse(reopened.conflicts)
        self.assertEqual(reopened.records[GEOMETRY][ident]["p"], [90, 2, 40])
        self.assertEqual(reopened.manifest()["groups"], rebuilt["groups"])
        build(reopened)
        # Repeated builds must not ingest their own output or add duplicates.
        again = self.session()
        self.assertEqual(again.records, reopened.records)
        self.assertEqual(len(again.records[GEOMETRY]), 2)

    def test_cached_editing_without_any_assets_or_original_database(self):
        session = self.session()
        ident, values = self.first(session, "_Instance_NPC")
        session.update("_Instance_NPC", ident, moved("_Instance_NPC", values, [41, 42, 43]))
        session.save()
        self.assets.rename(self.root / "unavailable-assets")
        self.db_root.rename(self.root / "unavailable-databases")
        offline = self.session()
        self.assertEqual(offline.sources, {"geometry": "cached", "db4": "cached", "camera": "unavailable"})
        values = offline.records["_Instance_NPC"][ident]
        offline.update("_Instance_NPC", ident, {**values, "PickingRange": 7.5})
        offline.save()
        report = build(offline)
        with closing(sqlite3.connect(report["database"])) as connection:
            transform, picking = connection.execute("SELECT Transform, PickingRange FROM _Instance_NPC").fetchone()
        self.assertEqual(floats(encode(transform))[12:15], [41, 42, 43])
        self.assertEqual(picking, 7.5)

    def test_patch_only_session_retains_edits_without_a_cache(self):
        session = self.session()
        ident, values = self.first(session)
        session.update(GEOMETRY, ident, moved(GEOMETRY, values, [11, 12, 13]))
        session.save()
        portable = Project(self.root, self.root / "absent", cache=self.root / "new-cache")
        offline = portable.open("test")
        self.assertEqual(offline.records[GEOMETRY][ident]["p"], [11, 12, 13])
        offline.update(GEOMETRY, ident, moved(GEOMETRY, offline.records[GEOMETRY][ident], [21, 22, 23]))
        offline.save()
        with self.assertRaisesRegex(ValueError, "conflicts"):
            build(offline)
        self.assertEqual(self.session().records[GEOMETRY][ident]["p"], [21, 22, 23])

    def test_database_and_client_share_transform_ids_exit_and_light_edits(self):
        session = self.session()
        for kind, field, value in [("_Instance_NPC", "PickingRange", 8.0), ("_Instance_InteractExit", "ExitURL", "new_destination"),
                                   ("_Instance_Light", "LightIntensity", 12.0)]:
            ident, row = self.first(session, kind)
            session.update(kind, ident, {**moved(kind, row, [7, 8, 9]), field: value})
        session.save()
        result = build(session)
        npc = read_json(self.assets / "maps/test.npcs.json")[self.guid.hex().upper()]
        self.assertEqual(npc["m"][12:15], [7, 8, 9])
        self.assertEqual(read_json(self.assets / "maps/test.exits.json")[0]["url"], "new_destination")
        self.assertEqual(read_json(self.assets / "maps/test.ambience.json")["lights"][0]["i"], 12.0)
        with closing(sqlite3.connect(result["database"])) as connection:
            self.assertEqual(connection.execute("SELECT Guid, Transform FROM _Instance_NPC").fetchone(), (self.guid, decode(float_blob(npc["m"]))))
            self.assertEqual(connection.execute("SELECT Data FROM _Attributes").fetchone()[0], bytes.fromhex("0000FF00"))
            self.assertIsNotNone(connection.execute("SELECT name FROM sqlite_master WHERE type='index' AND name='npc_template'").fetchone())
        self.assertEqual(self.db.read_bytes(), self.original_bytes)

    def test_unrelated_upstream_columns_merge_without_becoming_overrides(self):
        session = self.session()
        ident, row = self.first(session, "_Instance_NPC")
        session.update("_Instance_NPC", ident, {**row, "PickingRange": 9.0})
        session.save()
        with closing(sqlite3.connect(self.db)) as connection, connection:
            connection.execute("UPDATE _Instance_NPC SET Graphics='characters/new_guard'")
        reopened = self.session()
        self.assertFalse(reopened.conflicts)
        values = reopened.records["_Instance_NPC"][ident]
        self.assertEqual(values["Graphics"], "characters/new_guard")
        reopened.update("_Instance_NPC", ident, {**values, "PickingRange": 10.0})
        self.assertEqual(reopened.patch["operations"][0]["values"], {"PickingRange": 10.0})

    def test_overlapping_change_blocks_build_until_explicit_resolution(self):
        session = self.session()
        ident, row = self.first(session, "_Instance_NPC")
        session.update("_Instance_NPC", ident, {**row, "PickingRange": 9.0})
        session.save()
        with closing(sqlite3.connect(self.db)) as connection, connection:
            connection.execute("UPDATE _Instance_NPC SET PickingRange=3.0")
        reopened = self.session()
        self.assertEqual(len(reopened.conflicts), 1)
        self.assertIn("PickingRange", reopened.conflicts[0]["reason"])
        before = (self.assets / "maps/test.map.json").read_bytes()
        with self.assertRaises(ValueError):
            build(reopened)
        self.assertEqual((self.assets / "maps/test.map.json").read_bytes(), before)
        reopened.keep_override("_Instance_NPC", ident)
        self.assertFalse(reopened.conflicts)
        self.assertEqual(reopened.records["_Instance_NPC"][ident]["PickingRange"], 9.0)

    def test_added_deleted_and_undo_redo_survive_rebuild(self):
        session = self.session()
        ident, _ = self.first(session, "_Instance_NPC")
        added = session.duplicate("_Instance_NPC", ident)
        self.assertNotEqual(session.records["_Instance_NPC"][added]["Guid"], encode(self.guid))
        session.delete("_Instance_NPC", ident)
        session.undo()
        self.assertIn(ident, session.records["_Instance_NPC"])
        session.redo()
        session.save()
        report = build(self.session())
        with closing(sqlite3.connect(report["database"])) as connection:
            rows = connection.execute("SELECT Guid FROM _Instance_NPC").fetchall()
        self.assertEqual(len(rows), 1)
        self.assertNotEqual(rows[0][0], self.guid)
        session.revert("_Instance_NPC", ident)
        self.assertIn(ident, session.records["_Instance_NPC"])

    def test_reverting_restores_original_generated_map(self):
        session = self.session()
        ident, values = self.first(session)
        session.delete(GEOMETRY, ident)
        session.save()
        build(session)
        reopened = self.session()
        self.assertEqual(len(reopened.records[GEOMETRY]), 1)
        reopened.revert(GEOMETRY, ident)
        reopened.save()
        build(reopened)
        restored = read_json(self.assets / "maps/test.map.json")
        restored.pop("_level_override")
        self.assertEqual(restored, self.manifest)

    def test_upstream_changed_geometry_is_an_unresolved_target(self):
        session = self.session()
        ident, values = self.first(session)
        session.update(GEOMETRY, ident, moved(GEOMETRY, values, [3, 4, 5]))
        session.save()
        rebuilt = copy.deepcopy(self.manifest)
        rebuilt["instances"][0]["p"] = [50, 60, 70]
        atomic_json(self.assets / "maps/test.map.json", rebuilt)
        reopened = self.session()
        self.assertEqual(len(reopened.conflicts), 1)
        self.assertEqual(len(reopened.patch["operations"]), 1)
        with self.assertRaisesRegex(ValueError, "missing identity"):
            reopened.keep_override(GEOMETRY, ident)

    def test_generated_manifest_cannot_become_base_when_cache_is_lost(self):
        session = self.session()
        ident, values = self.first(session)
        session.update(GEOMETRY, ident, moved(GEOMETRY, values, [3, 4, 5]))
        session.save()
        build(session)
        no_cache = Project(self.root, self.assets, self.db_root, cache=self.root / "lost-cache").open("test")
        self.assertEqual(no_cache.sources[GEOMETRY], "unavailable")
        self.assertIsNone(no_cache.snapshot["manifest"])
        self.assertEqual(len(no_cache.conflicts), 1)

    def test_stale_editor_cannot_overwrite_another_editors_save(self):
        one, two = self.session(), self.session()
        ident, values = self.first(one)
        one.delete(GEOMETRY, ident)
        one.save()
        two.update(GEOMETRY, ident, moved(GEOMETRY, values, [3, 4, 5]))
        with self.assertRaisesRegex(ValueError, "another editor"):
            two.save()

    def test_validation_and_original_export_protection(self):
        session = self.session()
        with self.assertRaises(ValueError):
            session.export_db(self.db)
        with self.assertRaises(ValueError):
            Project(self.root, overrides=self.assets / "overrides")
        with self.assertRaises(ValueError):
            self.project.open("../other")
        ident, values = self.first(session)
        for changes in [{"p": [float("nan"), 2, 3]}, {"r": [0, 0, 0, 0]}, {"s": [0, 1, 1]}, {"model": "../escape"}]:
            with self.assertRaises(ValueError):
                session.update(GEOMETRY, ident, {**values, **changes})
        ident, values = self.first(session, "_Instance_NPC")
        with self.assertRaises(ValueError):
            session.update("_Instance_NPC", ident, {**values, "Guid": encode(b"bad")})
        with self.assertRaises(ValueError):
            session.update("_Instance_NPC", ident, {**values, "PickingRange": "wrong type"})

    def test_delete_conflicts_if_upstream_changed_record(self):
        session = self.session()
        ident, _ = self.first(session, "_Instance_NPC")
        session.delete("_Instance_NPC", ident)
        session.save()
        with closing(sqlite3.connect(self.db)) as connection, connection:
            connection.execute("UPDATE _Instance_NPC SET Id='different_npc'")
        reopened = self.session()
        self.assertEqual(len(reopened.conflicts), 1)
        self.assertIn(ident, reopened.records["_Instance_NPC"])

    def test_unchanged_blob_values_survive_friendly_inspector_round_trip(self):
        from level_editor import display_values, stored_values
        session = self.session()
        _, row = self.first(session, "_Instance_NPC")
        self.assertEqual(stored_values(display_values(row)), row)

    def test_missing_table_cannot_silently_drop_an_added_record(self):
        session = self.session()
        ident, _ = self.first(session, "_Instance_NPC")
        session.duplicate("_Instance_NPC", ident)
        session.save()
        with closing(sqlite3.connect(self.db)) as connection, connection:
            connection.execute("DROP TABLE _Instance_NPC")
        reopened = self.session()
        self.assertEqual(len(reopened.conflicts), 1)
        with self.assertRaises(ValueError):
            build(reopened)

    def test_changed_deleted_geometry_remains_an_explicit_conflict(self):
        session = self.session()
        ident, _ = self.first(session)
        session.delete(GEOMETRY, ident)
        session.save()
        rebuilt = copy.deepcopy(self.manifest)
        rebuilt["instances"][0]["p"] = [50, 60, 70]
        atomic_json(self.assets / "maps/test.map.json", rebuilt)
        reopened = self.session()
        self.assertEqual(len(reopened.conflicts), 1)
        self.assertEqual(reopened.patch["operations"][0]["action"], "delete")

    def test_another_session_cannot_change_the_database_under_an_export(self):
        session = self.session()
        with closing(sqlite3.connect(self.db)) as connection, connection:
            connection.execute("UPDATE _Instance_NPC SET PickingRange=3.0")
        self.session()  # Refreshes the shared original cache.
        with self.assertRaisesRegex(ValueError, "another session"):
            session.export_db(self.root / "staging/test.db4")


if __name__ == "__main__":
    unittest.main()
