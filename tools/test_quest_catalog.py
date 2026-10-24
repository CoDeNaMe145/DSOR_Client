import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

NIL = 0x7FFFFFFF
GODOT = Path(os.environ.get("DSO_GODOT") or Path(__file__).resolve().parents[1] / ".local" / "DSO_Godot")


def bxml(root):
    """One BXML ("LMXB") document, nodes in document order over one string pool."""
    strings, nodes, attrs = {}, [], []
    s = lambda v: strings.setdefault(v, len(strings))

    def add(element, parent):
        i = len(nodes)
        nodes.append(None)
        first = len(attrs)
        attrs.extend((s(k), s(v)) for k, v in element.attrib.items())
        nodes[i] = [s(element.tag), NIL, NIL, parent, first, len(element.attrib)]
        kids = [add(c, i) for c in element]
        if kids:
            nodes[i][1] = kids[0]
        for a, b in zip(kids, kids[1:]):
            nodes[a][2] = b
        return i

    add(root, NIL)
    return (b"LMXB" + struct.pack("<3I", len(attrs), len(nodes), len(strings))
            + b"".join(struct.pack("<2I", *a) for a in attrs) + b"".join(struct.pack("<6I", *n) for n in nodes)
            + b"".join(k.encode() + b"\0" for k in strings))


def quest(qid, number, **extra):
    q = ET.Element("Quest", {"id": qid, "type": "world", "level": "7", "chapterId": "c", "questRowNumber": "1",
                             "questNumber": str(number), "enabledLive": "true", "stagedTasks": "true", **extra})
    for kind, value in (("xp", "20"), ("vc", "5"), ("dynamicXP", "12.5")):
        ET.SubElement(q, "Reward", {"type": kind, "value": value})
    ET.SubElement(q, "Reward", {"type": "loot", "value": "sword", "amount": "2", "class": "mage", "rarity": "Rare"})
    kill = ET.SubElement(q, "Task", {"title": "kill", "tasktype": "KillTask"})
    ET.SubElement(kill, "Guard", {"type": "has", "value": "kill_count", "amount": "3"})
    ET.SubElement(ET.SubElement(kill, "Event", {"type": "kill", "value": "wolf"}), "Return", {"value": "kill_count"})
    ET.SubElement(q, "Task", {"title": "talk", "tasktype": "CompleteTask", "preTasks": "kill"})
    return q


class QuestCatalogTests(unittest.TestCase):
    @unittest.skipUnless((GODOT / "dsoexp" / "bxml.py").exists(), "needs DSO_Godot (DSO_GODOT=<checkout>)")
    def test_pack_and_server_xml_give_the_same_catalog(self):
        # The second pack entry's name is not its quest's id, as in 7 of the client's.
        entries = [("q-1", quest("q-1", 1, followup="q-2")), ("q-two", quest("q-2", 2))]
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "export_win32" / "quests").mkdir(parents=True)
            pack = b"KCAP" + struct.pack("<I", 1)
            for name, q in entries:
                doc = ET.Element("Quests")
                doc.append(q)
                blob = bxml(doc)
                pack += struct.pack("<H", len(name)) + name.encode() + struct.pack("<I", len(blob)) + blob
            (tmp / "export_win32" / "quests" / "quests.pbxml").write_bytes(pack)
            merged = ET.Element("Quests")
            for name, q in entries:
                q = ET.fromstring(ET.tostring(q))
                if q.get("id") != name:
                    q.set("packEntry", name)
                merged.append(q)
            ET.ElementTree(merged).write(tmp / "quests.xml", encoding="utf-8", xml_declaration=True)

            tool = Path(__file__).with_name("quest_catalog.py")
            catalogs = []
            for source in (tmp / "quests.xml", tmp / "export_win32", tmp / "export_win32" / "quests" / "quests.pbxml"):
                out = tmp / ("out%d" % len(catalogs))
                subprocess.run([sys.executable, str(tool), str(source), str(out), "--godot", str(GODOT)],
                               check=True, capture_output=True)
                catalogs.append(json.loads((out / "interface" / "quests.catalog.json").read_text(encoding="utf-8")))
        self.assertEqual(catalogs[0], catalogs[1])
        self.assertEqual(catalogs[0], catalogs[2])
        first = catalogs[0]["quests"]["q-1"]
        self.assertEqual(sorted(catalogs[0]["quests"]), ["q-1", "q-2"])
        self.assertEqual(first["part"], [1, 2])
        self.assertEqual(first["rewards"], {"xp": 20, "vc": 5, "rc": 0, "dynamic_xp": 12})
        self.assertEqual(first["items"], [{"item": "sword", "amount": 2, "class": "mage", "rarity": "Rare"}])
        self.assertEqual([(t["title"], t["target"], t["pre"]) for t in first["tasks"]],
                         [("kill", 3, []), ("talk", 1, [0])])


if __name__ == "__main__":
    unittest.main()
