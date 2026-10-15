"""Ambience export must retain activation state as well as the volume shape."""
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import struct
import subprocess
import sys
import tempfile
import unittest


class AmbienceExportTests(unittest.TestCase):
    def test_disabled_volume_is_not_silently_enabled(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'source/maps').mkdir(parents=True)
            at = struct.pack('<16f', 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 10, 0, 20, 1)
            with closing(sqlite3.connect(root / 'source/maps/test.db4')) as db:
                db.execute('CREATE TABLE _Instance_AmbienceBubble (Transform BLOB, PEDefaultEntity INTEGER, '
                           'AmbienceBubblePriority INTEGER, EventSetRow INTEGER, PEShapeType TEXT, AmbienceBubbleEnabled INTEGER)')
                db.executemany('INSERT INTO _Instance_AmbienceBubble VALUES (?,?,?,?,?,?)',
                               [(at, 1, 0, -1, 'Sphere', 1), (at, 0, 1000, -1, 'Box', 0)])
                db.execute('CREATE TABLE _Instance_Light (EventSetRow INTEGER)')
                db.execute('CREATE TABLE _Instance_GlobalShadow (Transform BLOB)')
                db.commit()
            subprocess.run([sys.executable, str(Path(__file__).with_name('export_ambience.py')),
                            str(root / 'source'), str(root / 'assets')], check=True, capture_output=True)
            document = json.loads((root / 'assets/maps/test.ambience.json').read_text())
            self.assertFalse(document['bubbles'][0]['enabled'])
            self.assertEqual(document['bubbles'][0]['shape'], 'Box')
            self.assertEqual(document['bubbles'][0]['m'][12:15], [10, 0, 20])


if __name__ == '__main__':
    unittest.main()
