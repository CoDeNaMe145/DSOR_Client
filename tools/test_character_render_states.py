import copy
import unittest

from character_render_states import restore


class CharacterCullingTests(unittest.TestCase):
    def test_authored_defaults_overrides_and_unrelated_materials(self):
        doc = {
            "materials": [
                {"doubleSided": True, "extras": {"nebula_shader": shader}}
                for shader in ("shd:character", "shd:characterfalloff", "shd:monster", "shd:standard")
            ],
            "meshes": [{"primitives": [{"material": i}]} for i in range(4)],
            "nodes": [{"mesh": 0}, {"mesh": 1, "extras": {"dsor_ints": {"CullMode": 1}}},
                      {"mesh": 2, "extras": {"dsor_ints": {"CullMode": 3}}}, {"mesh": 3}],
        }
        self.assertEqual(restore(doc), 1)
        self.assertEqual([m["doubleSided"] for m in doc["materials"]], [False, True, True, True])
        once = copy.deepcopy(doc)
        self.assertEqual(restore(doc), 0)
        self.assertEqual(doc, once)
        doc["nodes"].append({"mesh": 0, "extras": {"dsor_ints": {"CullMode": 1}}})
        restore(doc)
        self.assertTrue(doc["materials"][0]["doubleSided"])


if __name__ == "__main__":
    unittest.main()
