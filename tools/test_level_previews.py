"""Preview caches follow asset rebuilds without losing offline thumbnails."""
from pathlib import Path
import tempfile
import unittest

from level_data import GEOMETRY, Project
from level_previews import PreviewCache


class PreviewCacheTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.project = Project(temporary.name)
        self.source = self.project.assets / "props/barrel.glb"
        self.source.parent.mkdir(parents=True)
        self.source.write_bytes(b"original model")

    def test_rebuild_refreshes_preview_and_last_render_survives_missing_assets(self):
        cache = PreviewCache(self.project)
        first = cache.describe(GEOMETRY, {"model": "props/barrel"})
        Path(first["path"]).write_bytes(b"rendered thumbnail")
        cache.remember(first)
        cache.flush()
        self.source.write_bytes(b"rebuilt model with different geometry")
        rebuilt = cache.describe(GEOMETRY, {"model": "props/barrel"})
        self.assertNotEqual(rebuilt["path"], first["path"])
        self.assertEqual(rebuilt["cached"], Path(first["path"]))
        self.source.unlink()
        offline = PreviewCache(self.project).describe(GEOMETRY, {"model": "props/barrel"})
        self.assertFalse(offline["available"])
        self.assertEqual(offline["path"], first["path"])

    def test_instances_share_thumbnails_but_different_outfits_do_not(self):
        cache = PreviewCache(self.project)
        first = cache.describe(GEOMETRY, {"model": "props/barrel", "p": [1, 2, 3]})
        moved = cache.describe(GEOMETRY, {"model": "props/barrel", "p": [9, 8, 7]})
        self.assertEqual(first["path"], moved["path"])
        guard = cache.describe("_Instance_NPC", {"Graphics": "characters/uniskel", "CharacterSet": "guard"})
        merchant = cache.describe("_Instance_NPC", {"Graphics": "characters/uniskel", "CharacterSet": "merchant"})
        self.assertNotEqual(guard["identity"], merchant["identity"])
        self.assertIsNone(cache.describe("_Instance_Light", {}))


if __name__ == "__main__":
    unittest.main()
