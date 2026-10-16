import json
from pathlib import Path
import sqlite3
import struct
import subprocess
import sys
import tempfile
import unittest


class NpcExportTests(unittest.TestCase):
    def test_event_rules_and_look_overrides_survive_export(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, assets = root / "source", root / "assets"
            for path in (source / "db", source / "maps", source / "language/en", assets / "characters", assets / "maps"):
                path.mkdir(parents=True)
            (source / "language/en/db.npc.xml").write_text('<item name="guardTitle"><![CDATA[Guard]]></item>')
            fields = "Id,Graphics,CharacterSet,AnimSet,StartAnimationState,LoopStartAnimation,LocationMapIcon"
            template = ["guard", "characters/uniskel", "guard", "npc_male", "Idle", 1, "icon"]
            with sqlite3.connect(source / "db/static.db4") as db:
                db.execute(f"create table _Template_NPC({fields})")
                db.execute("insert into _Template_NPC values(?,?,?,?,?,?,?)", template)
            db.close()
            with sqlite3.connect(source / "maps/test.db4") as db:
                db.execute(f"create table _Instance_NPC(Guid,Transform,EventSetRow,{fields})")
                db.execute("create table _Instance_EventSets(Id,EventFlags)")
                db.executemany("insert into _Instance_EventSets values(?,?)", [(1, "winter=True;"), (2, "quest_finished=False;")])
                matrix = struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 10, 0, 20, 1)
                for i, event in enumerate((-1, 1, 2, 99), start=1):
                    own = template.copy()
                    own[5] = 0
                    db.execute("insert into _Instance_NPC values(?,?,?,?,?,?,?,?,?,?)", [bytes([i]) * 16, matrix, event, *own])
            db.close()
            subprocess.run([sys.executable, str(Path(__file__).with_name("export_npcs.py")), str(source), str(assets)], check=True, capture_output=True)
            result = json.loads((assets / "maps/test.npcs.json").read_text())
            entries = list(result.values())
            self.assertEqual([r["event"] for r in entries], [-1, 1, 2, 99])
            self.assertNotIn("event_flags", entries[0])
            self.assertEqual(entries[1]["event_flags"], "winter=True;")
            self.assertEqual(entries[2]["event_flags"], "quest_finished=False;")
            self.assertNotIn("event_flags", entries[3])
            self.assertTrue(all(r["look"]["loop"] is False for r in entries))
            self.assertTrue(all(r["m"][12:15] == [10, 0, 20] for r in entries))


if __name__ == "__main__":
    unittest.main()
