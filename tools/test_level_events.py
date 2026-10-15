"""Authored event state semantics, including the normal-content replacements."""
import unittest
from types import SimpleNamespace

from level_data import GEOMETRY
from level_events import MapEvents
from level_viewport import Viewport
import test_level_editor as fixtures


class MapEventTests(unittest.TestCase):
    def setUp(self):
        self.records = {
            "_Instance_EventSets": {
                "first": {"Id": "38", "EventFlags": "anniversary_event=True;"},
                "second": {"Id": "92", "EventFlags": "christmas_2012=True;"},
                "third": {"Id": "75", "EventFlags": "anniversary_event=False;christmas_2012=False;"},
                "fourth": {"Id": "136", "EventFlags": "anniversary_event=True;christmas_2012=True;blocked=False;"},
            },
            "_Instance_NPC": {str(i): {"EventSetRow": i} for i in (-1, 38, 92, 75, 136, 999)},
            GEOMETRY: {"placement": {"g": 38}},
        }
        self.events = MapEvents(self.records)

    def test_none_shows_negative_gates_and_base_but_not_seasonal_content(self):
        self.assertEqual(self.events.active_sets(), {-1, 75})
        self.assertTrue(self.events.visible(GEOMETRY, {"g": 38}))
        self.assertFalse(self.events.visible("_Instance_NPC", {"EventSetRow": 999}))

    def test_event_uses_id_not_record_order_and_swaps_normal_content(self):
        self.assertEqual(self.events.active_sets("CHRISTMAS_2012"), {-1, 92, 136})
        self.assertEqual(self.events.active_sets("anniversary_event"), {-1, 38, 136})
        self.assertEqual(self.events.active_sets("blocked"), {-1, 75})

    def test_effective_override_rules_and_null_membership(self):
        self.records["_Instance_EventSets"]["second"]["EventFlags"] = "christmas_2012=False;"
        changed = MapEvents(self.records)
        self.assertIn(92, changed.active_sets())
        self.assertNotIn(92, changed.active_sets("christmas_2012"))
        self.assertTrue(changed.visible("_Instance_NPC", {"EventSetRow": None}))

    def test_winter_preset_enables_scenery_and_removes_both_normal_replacements(self):
        self.records['_Instance_EventSets'].update({
            'scenery': {'Id': '91', 'EventFlags': 'christmas_2012_silent=True;'},
            'normal': {'Id': '72', 'EventFlags': 'christmas_2012_silent=False;'},
        })
        self.records['_Instance_Sequence'] = {'reindeer': {'EventSetRow': 91}, 'normal': {'EventSetRow': 72}}
        events = MapEvents(self.records)
        winter = events.preview_flags('christmas_2012')
        self.assertEqual(events.active_sets(winter), {-1, 91, 92, 136})
        self.assertEqual(events.active_sets(events.preview_flags()), {-1, 72, 75})
        self.assertEqual(events.active_sets(events.preview_flags('christmas_2012_silent')), {-1, 75, 91})

    def test_cached_database_keeps_event_rules_without_assets(self):
        fixture = fixtures.LevelOverridesTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        import sqlite3
        with sqlite3.connect(fixture.project.db_root / "test.db4") as db:
            db.execute('CREATE TABLE _Instance_EventSets (Id TEXT PRIMARY KEY, EventFlags TEXT)')
            db.execute('INSERT INTO _Instance_EventSets VALUES (?, ?)', ('92', 'winter=True;'))
            db.execute('UPDATE _Instance_NPC SET EventSetRow=92')
        session = fixture.session()
        fixture.project.db_root = None
        fixture.project.assets = fixture.project.root / "absent"
        cached = fixture.project.open("test")
        self.assertEqual(MapEvents(cached.records).active_sets("winter"), {-1, 92})
        self.assertEqual(MapEvents(cached.records).active_sets(), {-1})
        self.assertFalse(cached.dirty)

    def test_live_sequence_retains_its_event_and_real_effect_tracks(self):
        fixture = fixtures.LevelOverridesTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        session = fixture.session()
        from level_data import float_blob, atomic_json
        session.records["_Instance_Sequence"] = {"fx": {"EventSetRow": 92, "SequenceIsPlaying": 1,
            "SequencerFileName": "winter/snow", "Transform": float_blob([1,0,0,0, 0,1,0,0, 0,0,1,0, 2,3,4,1])}}
        seq = {"length": 25, "tracks": [{"kind": "fx", "graphics": "effects/snow", "start": 0, "end": 25}]}
        atomic_json(fixture.project.assets / "skills/sequences.json", {"snow": seq})
        viewport = Viewport.__new__(Viewport)
        viewport.editor = SimpleNamespace(session=session, project=fixture.project)
        viewport.catalogs = {}
        item = next(item for item in viewport.scene()["objects"] if item["kind"] == "_Instance_Sequence")
        self.assertEqual(item["sequence"], seq)
        self.assertEqual(item["event"], 92)
        self.assertEqual(item["matrix"][12:15], [2,3,4])


if __name__ == "__main__":
    unittest.main()
