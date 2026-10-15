"""3D gesture integration with persistent geometry and DB4 overrides."""
import math
from types import SimpleNamespace
import unittest

import test_level_editor as fixtures
from level_data import GEOMETRY, float_blob, floats
from level_viewport import Viewport, apply_transform, transform_matrix


class ViewportTest(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.LevelOverridesTest()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.session = self.fixture.session()

    def event(self, kind=GEOMETRY, **values):
        ident = next(iter(self.session.records[kind]))
        return {"revision": 3, "key": kind + "\t" + ident, **values}

    def test_translation_preserves_database_basis_and_is_one_undo_step(self):
        kind = "_Instance_NPC"
        ident = next(iter(self.session.records[kind]))
        before = self.session.records[kind][ident]
        matrix = floats(before["Transform"])
        matrix[1] = 0.125  # Keep a non-orthogonal source basis byte-for-byte.
        self.session.update(kind, ident, {**before, "Transform": float_blob(matrix)})
        original = self.session.records[kind][ident]
        count = len(self.session.undo_stack)
        apply_transform(self.session, self.event(kind, mode="translate", position=[41, 42, 43]), 3)
        changed = floats(self.session.records[kind][ident]["Transform"])
        self.assertEqual(changed[:12], matrix[:12])
        self.assertEqual(changed[12:15], [41, 42, 43])
        self.assertEqual(len(self.session.undo_stack), count + 1)
        self.session.undo()
        self.assertEqual(self.session.records[kind][ident], original)

    def test_rotation_changes_orientation_without_moving_database_record(self):
        kind = "_Instance_NPC"
        event = self.event(kind, mode="rotate", axis=1, angle=math.pi / 2)
        _, ident = apply_transform(self.session, event, 3)
        matrix = floats(self.session.records[kind][ident]["Transform"])
        self.assertAlmostEqual(matrix[0], 0, places=6)
        self.assertAlmostEqual(matrix[2], -1, places=6)
        self.assertAlmostEqual(matrix[8], 1, places=6)
        self.assertEqual(matrix[12:15], [10, 20, 30])

    def test_axis_drag_preserves_untouched_geometry_coordinates(self):
        event = self.event(mode="translate", position=[1.0000001, 40, 3.0000001], axis=1)
        _, ident = apply_transform(self.session, event, 3)
        self.assertEqual(self.session.records[GEOMETRY][ident]["p"], [1, 40, 3])

    def test_axis_scale_preserves_other_axes_and_translation(self):
        kind = "_Instance_NPC"
        _, ident = apply_transform(self.session, self.event(kind, mode="scale", factors=[2, 1, 1]), 3)
        matrix = floats(self.session.records[kind][ident]["Transform"])
        self.assertEqual([matrix[0], matrix[5], matrix[10]], [2, 1, 1])
        self.assertEqual(matrix[12:15], [10, 20, 30])

    def test_geometry_rotation_and_scale_use_existing_override_schema(self):
        rotation = [0, math.sin(math.pi / 4), 0, math.cos(math.pi / 4)]
        _, ident = apply_transform(self.session, self.event(mode="rotate", rotation=rotation), 3)
        apply_transform(self.session, self.event(mode="scale", scale=[2, 3, 4]), 3)
        values = self.session.records[GEOMETRY][ident]
        self.assertEqual(values["r"], rotation)
        self.assertEqual(values["s"], [2, 3, 4])
        self.assertEqual(values["p"], [1, 2, 3])
        matrix = transform_matrix(GEOMETRY, values)
        self.assertAlmostEqual(matrix[2], -2)
        self.assertAlmostEqual(matrix[8], 4)
        self.assertEqual(matrix[12:15], [1, 2, 3])
        self.session.save()
        self.assertEqual(self.fixture.session().records[GEOMETRY][ident], values)

    def test_stale_or_invalid_gesture_cannot_mutate_the_map(self):
        event = self.event(mode="translate", position=[20, 30, 40])
        with self.assertRaisesRegex(ValueError, "outdated"):
            apply_transform(self.session, event, 4)
        self.assertFalse(self.session.dirty)
        with self.assertRaises(ValueError):
            apply_transform(self.session, self.event(mode="rotate", rotation=[0, 0, 0, 0]), 3)
        self.assertFalse(self.session.dirty)

    def test_scene_contains_real_model_references_and_database_identity(self):
        viewport = Viewport.__new__(Viewport)
        viewport.editor = SimpleNamespace(session=self.session, project=self.fixture.project)
        viewport.catalogs = {}
        scene = viewport.scene()
        self.assertEqual(scene["protocol"], 1)
        self.assertEqual(scene["map"], "test")
        geometry = next(obj for obj in scene["objects"] if obj["kind"] == GEOMETRY)
        self.assertEqual(geometry["model"], "props/tree")
        self.assertEqual(geometry["matrix"][12:15], [1, 2, 3])
        npc = next(obj for obj in scene["objects"] if obj["kind"] == "_Instance_NPC")
        self.assertIn(npc["key"].split("\t")[1], self.session.records["_Instance_NPC"])
        self.assertEqual(npc["matrix"][12:15], [10, 20, 30])


if __name__ == "__main__":
    unittest.main()
