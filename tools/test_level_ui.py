"""The friendly property form and raw inspector share the authored edit layer."""
import json
import unittest

from level_data import GEOMETRY
from level_editor import Editor
import test_level_editor as fixtures


class Value:
    def __init__(self, value): self.value = value
    def get(self, *args): return self.value
    def delete(self, *args): self.value = ""
    def insert(self, index, value): self.value = value


class PropertyEditTests(unittest.TestCase):
    def setUp(self):
        fixture = fixtures.LevelOverridesTest()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        self.editor = Editor.__new__(Editor)
        e = self.editor
        e.session = fixture.session()
        ident = next(iter(e.session.records[GEOMETRY]))
        e.selected = GEOMETRY, ident
        self.original = e.session.records[GEOMETRY][ident].copy()
        e.inspector_text = json.dumps(self.original)
        e.inspector = Value(e.inspector_text)
        e.property_fields = {"model": Value(self.original["model"])}
        e.property_initial = {"model": self.original["model"]}

    def test_saving_pending_properties_creates_one_undoable_override(self):
        e = self.editor
        e.property_fields["model"].value = "props/rock"
        self.assertTrue(e.pending_inspector())
        e.commit_inspector()
        self.assertFalse(e.pending_inspector())
        self.assertEqual(e.session.patch["operations"][0]["values"], {"model": "props/rock"})
        self.assertEqual(json.loads(e.inspector.get())["model"], "props/rock")
        e.session.undo()
        self.assertEqual(e.session.records[GEOMETRY][e.selected[1]], self.original)

    def test_disagreeing_forms_do_not_silently_overwrite_one_another(self):
        e = self.editor
        e.property_fields["model"].value = "props/rock"
        e.inspector.value = json.dumps({**self.original, "model": "props/bench"})
        with self.assertRaisesRegex(ValueError, "both Properties and JSON"):
            e.commit_inspector()
        self.assertFalse(e.session.dirty)


if __name__ == "__main__":
    unittest.main()
