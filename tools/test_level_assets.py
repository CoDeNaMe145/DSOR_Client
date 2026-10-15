"""Asset library discovery and placement use project files and persistent edits."""
from types import SimpleNamespace
from unittest.mock import Mock
import unittest

from level_assets import iter_models, placement_position
from level_data import GEOMETRY
from level_editor import Editor
import test_level_editor as fixtures


class AssetLibraryTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.LevelOverridesTest()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)

    def test_inventory_includes_unplaced_nested_models_but_not_support_files(self):
        root = self.fixture.project.assets
        for name in ("new_theme/props/bench.glb", "characters/hero/__animations.glb", "textures/wood.png", "data/maps.json"):
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"test asset")
        found = list(iter_models(root))
        self.assertIn("new_theme/props/bench", found)
        self.assertNotIn("characters/hero/__animations", found)
        self.assertFalse(any(name.startswith(("textures/", "data/")) for name in found))
        self.assertEqual(list(iter_models(root / "missing")), [])

    def test_placement_uses_the_view_ground_or_forward_direction(self):
        self.assertEqual(placement_position({"camera_position": [1, 10, 2], "camera_forward": [0, -0.5, -0.5]}, [40, 0, 50]), [1, 0, -8])
        self.assertEqual(placement_position({"camera_position": [1, 10, 2], "camera_forward": [0, 0, -1]}, [40, 0, 50]), [1, 10, -8])
        self.assertEqual(placement_position({"camera_position": [0, 150, 100], "camera_forward": [0, -0.6, -0.8]}, [40, 0, 50]), [0, 0, -100])
        self.assertEqual(placement_position({}, [40.2, 0, 50.1], 1), [40, 0, 50])

    def test_library_adds_geometry_even_when_scene_filter_is_npcs_and_survives_offline(self):
        e = Editor.__new__(Editor)
        e.session = self.fixture.session()
        e.commit_inspector = Mock()
        e.view = [40, 50, 3]
        e.snap = SimpleNamespace(get=lambda: "1")
        e.viewport = SimpleNamespace(revision=7, last_status={"revision": 7, "camera_position": [1, 10, 2], "camera_forward": [0, -0.5, -0.5]})
        e.kind_var, e.search_var, e.objects, e.window, e.focus = (Mock() for _ in range(5))
        e.kind_var.get.return_value = "NPC"
        def refresh():
            e.rows = [(GEOMETRY, ident) for ident in e.session.records[GEOMETRY]]
        e.refresh = refresh
        ident = e.place_asset("new_theme/props/bench")
        self.assertEqual(e.selected, (GEOMETRY, ident))
        self.assertEqual(e.session.records[GEOMETRY][ident]["p"], [1, 0, -8])
        self.assertEqual(e.session.patch["operations"][0]["action"], "add")
        self.assertEqual(len(e.session.undo_stack), 1)
        e.kind_var.set.assert_called_with("All objects")
        e.session.save()
        self.fixture.project.assets = self.fixture.root / "unavailable"
        self.assertEqual(self.fixture.session().records[GEOMETRY][ident]["model"], "new_theme/props/bench")
        e.session.undo()
        self.assertNotIn(ident, e.session.records[GEOMETRY])


if __name__ == "__main__":
    unittest.main()
