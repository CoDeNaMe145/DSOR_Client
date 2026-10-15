"""Lighting UI edits, generated assets and DB4 exports share one durable layer."""
import json
import sqlite3
import unittest
from contextlib import closing
from types import SimpleNamespace

from level_data import decode, float_blob, floats, read_json, moved
from level_lighting import LIGHT, AMBIENCE, new_light, property_value
from level_export import build, client_documents
from level_viewport import Viewport
from level_editor import Editor, display_values
from test_level_ui import Value
import test_level_editor as fixtures


class LightingTests(unittest.TestCase):
    def setUp(self):
        f = fixtures.LevelOverridesTest()
        f.setUp()
        self.addCleanup(f.doCleanups)
        self.f = f
        self.project = f.project
        with closing(sqlite3.connect(f.db)) as db, db:
            db.execute('''CREATE TABLE _Instance_AmbienceBubble (Guid BLOB PRIMARY KEY, Transform BLOB,
                PEDefaultEntity INTEGER, LightColor BLOB, LightAmbient BLOB, LightIntensity REAL,
                FogColor BLOB, FogNearDist REAL, FogFarDist REAL, FogIntensity REAL, BloomScale REAL,
                BloomColor BLOB, BrightPassThreshold REAL, Saturation REAL, AmbienceBubbleEnabled INTEGER,
                AmbienceBubblePriority INTEGER, PEShapeType TEXT, PEFadeTime REAL, EventSetRow INTEGER)''')
            white = decode(float_blob([1, 1, 1, 1]))
            db.execute('INSERT INTO _Instance_AmbienceBubble VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)',
                       (f.guid, f.transform, 1, white, white, 1., white, 10., 50., 1., 1., white, .7, 1., 1, 0, 'Box', 1., -1))
        self.source_bytes = f.db.read_bytes()
        self.session = self.project.open('test')
        self.ident = next(iter(self.session.records[LIGHT]))

    def test_color_controls_preserve_alpha_and_use_one_undo_step(self):
        s = self.session
        row = s.records[LIGHT][self.ident]
        e = Editor.__new__(Editor)
        e.session, e.selected = s, (LIGHT, self.ident)
        e.inspector_text = json.dumps(display_values(row))
        e.inspector = Value(e.inspector_text)
        e.property_fields = {'LightColor': Value('0.1, 0.2, 0.9'), 'LightIntensity': Value('3.5')}
        e.property_initial = {'LightColor': '1, 0.5, 0.2', 'LightIntensity': '2.0'}
        e.commit_inspector()
        result = s.records[LIGHT][self.ident]
        self.assertEqual(floats(result['LightColor'])[3], floats(row['LightColor'])[3])
        self.assertEqual(result['LightIntensity'], 3.5)
        self.assertEqual(set(s.patch['operations'][0]['values']), {'LightColor', 'LightIntensity'})
        s.undo()
        self.assertEqual(s.records[LIGHT][self.ident], row)
        s.redo()
        self.assertEqual(s.records[LIGHT][self.ident], result)

    def test_light_and_environment_edits_match_viewport_and_exported_db4(self):
        s = self.session
        row = s.records[LIGHT][self.ident]
        updated = {**moved(LIGHT, row, [1., 4., 8.]), 'LightColor': float_blob([.2, .6, 1., .7]),
                   'LightType': 1, 'LightConeAngle': 50., 'LightCastShadows': 1,
                   'LightRange': 12., 'LightFlickerEnable': 1, 'LightFlickerFrequency': 3., 'LightFlickerIntensity': .4}
        s.update(LIGHT, self.ident, updated)
        env_id = next(iter(s.records[AMBIENCE]))
        env = s.records[AMBIENCE][env_id]
        s.update(AMBIENCE, env_id, {**env, 'LightAmbient': float_blob([.1, .2, .3, 1.]), 'BloomScale': .4, 'FogFarDist': 80.})
        s.save()
        viewport = Viewport.__new__(Viewport)
        viewport.editor = SimpleNamespace(session=s, project=self.project)
        viewport.catalogs = {}
        scene = viewport.scene()
        item = next(o for o in scene['objects'] if o['kind'] == LIGHT)
        report = build(s)
        document = read_json(self.project.assets / 'maps/test.ambience.json')
        self.assertEqual(item['light'], document['lights'][0])
        self.assertEqual(document['global']['BloomScale'], .4)
        self.assertTrue(next(o for o in scene['objects'] if o['kind'] == AMBIENCE)['default_ambience'])
        with closing(sqlite3.connect(report['database'])) as db:
            color, cone, shadows = db.execute('SELECT LightColor,LightConeAngle,LightCastShadows FROM _Instance_Light').fetchone()
            self.assertEqual(color, decode(updated['LightColor']))
            self.assertEqual((cone, shadows), (50., 1))
        self.assertEqual(self.f.db.read_bytes(), self.source_bytes)
        self.assertEqual(self.project.open('test').records, s.records)

    def test_offline_creation_duplication_and_delete_survive_rebuild(self):
        s = self.session
        s.add(LIGHT, new_light(s, [4., 3., 2.], spot=True))
        s.save()
        self.project.assets = self.project.root / 'absent-assets'
        self.project.db_root = None
        offline = self.project.open('test')
        ident = offline.add(LIGHT, new_light(offline, [1., 2., 3.]))
        duplicate = offline.duplicate(LIGHT, ident)
        self.assertNotEqual(offline.records[LIGHT][ident]['Guid'], offline.records[LIGHT][duplicate]['Guid'])
        offline.delete(LIGHT, duplicate)
        offline.save()
        self.project.assets = self.f.assets
        self.project.db_root = self.f.db_root
        rebuilt = self.project.open('test')
        self.assertFalse(rebuilt.conflicts)
        self.assertEqual(len(rebuilt.records[LIGHT]), 3)
        build(rebuilt)
        self.assertEqual(len(read_json(self.f.assets / 'maps/test.ambience.json')['lights']), 3)

    def test_event_lights_stay_in_the_export_with_their_event_reference(self):
        s = self.session
        row = s.records[LIGHT][self.ident]
        s.update(LIGHT, self.ident, {**row, 'EventSetRow': 29})
        self.assertEqual(client_documents(s)['ambience.json']['lights'][0]['event'], 29)

    def test_invalid_lighting_values_do_not_create_history(self):
        s = self.session
        row = s.records[LIGHT][self.ident]
        for patch in [{'LightRange': -1.}, {'LightIntensity': float('nan')}, {'LightConeAngle': 180.},
                      {'LightColor': float_blob([-1, 0, 0, 1])}, {'LightFlickerIntensity': 2.}, {'LightType': 9}]:
            with self.assertRaises(ValueError):
                s.update(LIGHT, self.ident, {**row, **patch})
        env_id = next(iter(s.records[AMBIENCE]))
        with self.assertRaises(ValueError):
            s.update(AMBIENCE, env_id, {**s.records[AMBIENCE][env_id], 'FogFarDist': 5.})
        self.assertFalse(s.dirty)
        self.assertFalse(s.undo_stack)

    def test_spot_creation_uses_a_downward_unit_transform_and_fresh_identity(self):
        row = new_light(self.session, [1, 2, 3], spot=True)
        self.assertEqual(row['LightType'], 1)
        self.assertEqual(row['LightConeAngle'], 45.)
        self.assertEqual(floats(row['Transform'])[8:11], [0, 1, 0])
        self.assertEqual(set(row), set(self.session.records[LIGHT][self.ident]))
        self.assertEqual(property_value('LightType', 'Spot / projected', 0), 1)


if __name__ == '__main__':
    unittest.main()
