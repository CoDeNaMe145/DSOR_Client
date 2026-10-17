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
            (source / "language/en/db.npc.xml").write_text('<item name="guardTitle"><![CDATA[Guard]]></item>'
                '<item name="guardProfession"><![CDATA[Merchant]]></item>'
                '<item name="ProfessionGemTraderFemale"><![CDATA[Jeweler]]></item>')
            fields = "Id,Graphics,CharacterSet,AnimSet,StartAnimationState,LoopStartAnimation,LocationMapIcon,Portrait,NoTalk,ShopId,Profession,OverHeadIconOffset,ProfessionLocaId"
            template = ["guard", "characters/uniskel", "guard", "npc_male", "Idle", 1, "icon", "portrait", 0, "shop", "monger", struct.pack("<4f", 0, 1.5, 0, 0), ""]
            with sqlite3.connect(source / "db/static.db4") as db:
                db.execute(f"create table _Template_NPC({fields})")
                db.execute("insert into _Template_NPC values(?,?,?,?,?,?,?,?,?,?,?,?,?)", template)
            db.close()
            with sqlite3.connect(source / "maps/test.db4") as db:
                db.execute(f"create table _Instance_NPC(Guid,Transform,EventSetRow,{fields})")
                db.execute("create table _Instance_EventSets(Id,EventFlags)")
                db.executemany("insert into _Instance_EventSets values(?,?)", [(1, "winter=True;"), (2, "quest_finished=False;")])
                matrix = struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 10, 0, 20, 1)
                for i, event in enumerate((-1, 1, 2, 99), start=1):
                    own = template.copy()
                    own[9] = "override_shop"
                    own[11] = struct.pack("<4f", 0.5, 2, -0.25, 0)
                    own[12] = "ProfessionGemTraderFemale"
                    db.execute("insert into _Instance_NPC values(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", [bytes([i]) * 16, matrix, event, *own])
            db.close()
            subprocess.run([sys.executable, str(Path(__file__).with_name("export_npcs.py")), str(source), str(assets)], check=True, capture_output=True)
            result = json.loads((assets / "maps/test.npcs.json").read_text())
            entries = list(result.values())
            templates = json.loads((assets / "characters/npc_templates.json").read_text())
            self.assertEqual(templates["guard"]["overhead_offset"], [0, 1.5, 0])
            self.assertEqual(templates["guard"]["profession_title"], "Merchant")
            self.assertTrue(all(r["look"]["profession_title"] == "Jeweler" for r in entries))
            self.assertTrue(all(r["look"]["overhead_offset"] == [0.5, 2, -0.25] for r in entries))
            self.assertTrue(all(r["look"]["profession"] == "monger" for r in entries))
            self.assertEqual([r["event"] for r in entries], [-1, 1, 2, 99])
            self.assertNotIn("event_flags", entries[0])
            self.assertEqual(entries[1]["event_flags"], "winter=True;")
            self.assertEqual(entries[2]["event_flags"], "quest_finished=False;")
            self.assertNotIn("event_flags", entries[3])
            self.assertTrue(all(r["look"]["shop"] == "override_shop" for r in entries))
            self.assertTrue(all(r["m"][12:15] == [10, 0, 20] for r in entries))


if __name__ == "__main__":
    unittest.main()
