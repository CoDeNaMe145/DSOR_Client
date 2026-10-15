"""Fixed camera round trips use the same persistent layer as map edits."""
import copy
import math
import unittest
from types import SimpleNamespace

from level_cameras import CAMERA, SELECTION_ID, selection_record, lens
from level_data import Project, atomic_json, read_json, floats, moved, float_blob
from level_export import build
from level_viewport import Viewport, apply_transform
import test_level_editor as fixtures


class FixedCameraTests(unittest.TestCase):
    def setUp(self):
        f = fixtures.LevelOverridesTest()
        f.setUp()
        self.addCleanup(f.doCleanups)
        self.project = f.project
        (f.db_root / 'a0000_char.db4').write_bytes(f.db.read_bytes())
        atomic_json(f.assets / 'maps/a0000_char.map.json', {**f.manifest, 'map': 'a0000_char'})
        self.catalog = {'camera': {'pitch': -3., 'yaw': -0.2, 'offset': [0., 2., -14.5], 'fov': 20.},
                        'slots': {'shirt': [2]}, 'hair': {'mage_male': {'1': 'hair1'}}, 'beard': {}}
        self.catalog_path = f.assets / 'characters/selection.json'
        atomic_json(self.catalog_path, self.catalog)
        self.session = self.project.open('a0000_char')

    def test_original_pose_and_lens_reach_viewport_without_an_authored_edit(self):
        row = self.session.records[CAMERA][SELECTION_ID]
        matrix = floats(row['Transform'])
        self.assertEqual(matrix[12:15], [0., 2., 14.5])
        self.assertAlmostEqual(matrix[9], -math.sin(math.radians(-3)), places=6)
        viewport = Viewport.__new__(Viewport)
        viewport.editor = SimpleNamespace(session=self.session, project=self.project)
        viewport.catalogs = {}
        item = next(o for o in viewport.scene()['objects'] if o['kind'] == CAMERA)
        self.assertEqual(item['camera'], lens(CAMERA, row))
        self.assertEqual(item['camera']['fov'], 20.)
        self.assertEqual(item['model'], '')
        self.assertFalse(self.session.dirty)

    def test_move_rotate_save_build_and_rebuild_preserve_camera_and_other_catalog_data(self):
        s = self.session
        row = s.records[CAMERA][SELECTION_ID]
        s.update(CAMERA, SELECTION_ID, moved(CAMERA, row, [1., 3., 12.]))
        apply_transform(s, {'revision': 1, 'key': CAMERA+'\t'+SELECTION_ID, 'mode': 'rotate', 'axis': 1, 'angle': 0.2}, 1)
        final = copy.deepcopy(s.records[CAMERA][SELECTION_ID])
        s.save()
        build(s)
        catalog = read_json(self.catalog_path)
        self.assertEqual(catalog['camera']['matrix'], floats(final['Transform']))
        self.assertEqual(catalog['slots'], self.catalog['slots'])
        self.assertEqual(catalog['hair'], self.catalog['hair'])
        self.assertEqual(catalog['camera']['_level_override']['base'], self.catalog['camera'])
        reopened = self.project.open('a0000_char')
        self.assertFalse(reopened.conflicts)
        self.assertEqual(reopened.records[CAMERA][SELECTION_ID], final)
        # Catalog provenance also recovers the camera baseline without a cache.
        uncached = Project(self.project.root, self.project.assets, self.project.db_root,
                           self.project.overrides, self.project.cache / 'new-cache').open('a0000_char')
        self.assertFalse(uncached.conflicts)
        self.assertEqual(uncached.records[CAMERA][SELECTION_ID], final)
        # Converter output replaces the catalog. A new unedited FOV flows through.
        rebuilt = copy.deepcopy(self.catalog)
        rebuilt['camera']['fov'] = 24.
        atomic_json(self.catalog_path, rebuilt)
        rebuilt_session = self.project.open('a0000_char')
        self.assertFalse(rebuilt_session.conflicts)
        self.assertEqual(rebuilt_session.records[CAMERA][SELECTION_ID]['Transform'], final['Transform'])
        self.assertEqual(rebuilt_session.records[CAMERA][SELECTION_ID]['FieldOfView'], 24.)

    def test_camera_undo_and_offline_editing_do_not_need_assets_or_db4(self):
        s = self.session
        row = copy.deepcopy(s.records[CAMERA][SELECTION_ID])
        s.update(CAMERA, SELECTION_ID, {**row, 'FieldOfView': 28., 'PreviewAspect': 4/3})
        s.undo()
        self.assertEqual(s.records[CAMERA][SELECTION_ID], row)
        s.redo(); s.save()
        self.project.assets = self.project.root / 'missing'
        self.project.db_root = None
        offline = self.project.open('a0000_char')
        self.assertEqual(offline.sources[CAMERA], 'cached')
        self.assertFalse(offline.conflicts)
        self.assertEqual(offline.records[CAMERA][SELECTION_ID]['FieldOfView'], 28.)
        offline.update(CAMERA, SELECTION_ID, moved(CAMERA, offline.records[CAMERA][SELECTION_ID], [2., 4., 11.]))
        offline.save()
        self.assertEqual(floats(self.project.open('a0000_char').records[CAMERA][SELECTION_ID]['Transform'])[12:15], [2., 4., 11.])

    def test_invalid_lenses_and_scaled_camera_are_rejected(self):
        row = self.session.records[CAMERA][SELECTION_ID]
        for bad in ({'FieldOfView': 0}, {'PreviewAspect': 0}, {'NearClip': 2000}, {'FarClip': -1}):
            with self.assertRaises(ValueError):
                self.session.update(CAMERA, SELECTION_ID, {**row, **bad})
        matrix = floats(row['Transform']);matrix[0:3] = [v*2 for v in matrix[0:3]]
        with self.assertRaisesRegex(ValueError, 'scale'):
            self.session.update(CAMERA, SELECTION_ID, {**row, 'Transform': float_blob(matrix)})
        self.assertFalse(self.session.dirty)

    def test_fixed_selection_camera_cannot_be_added_duplicated_or_deleted(self):
        s = self.session
        with self.assertRaises(ValueError):
            s.add(CAMERA, s.records[CAMERA][SELECTION_ID])
        with self.assertRaises(ValueError):
            s.duplicate(CAMERA, SELECTION_ID)
        with self.assertRaises(ValueError):
            s.delete(CAMERA, SELECTION_ID)
        self.assertFalse(s.dirty)


if __name__ == '__main__':
    unittest.main()
