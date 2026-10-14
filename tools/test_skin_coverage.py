from pathlib import Path
import json
import struct
import tempfile
import unittest

from skin_coverage import export_part_lists, read_skin_tags


def table(rows):
    data = bytearray(b'GTSG' + struct.pack('<II', 2, len(rows)))
    for name, mask, layer in rows:
        name = name.encode('utf8')
        data += struct.pack('<H', len(name)) + name + struct.pack('<IH', mask, layer)
    return bytes(data)


class SkinCoverageTests(unittest.TestCase):
    def test_masks_and_layers_survive_export_for_available_meshes(self):
        rows = [('warrior_male_upperbody', 0x40, 0), ('warrior_torso_01', 0x1003c0, 1),
                ('warrior_torso_01_female', 0x100780, 1), ('costume', 0x1fffff, 3)]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'tables').mkdir()
            (root / 'tables/gfxskintags.bin').write_bytes(table(rows))
            parts = root / 'assets/characters/uniskel/parts'
            parts.mkdir(parents=True)
            for name in ['warrior_torso_01', 'warrior_torso_01_female', 'unlisted_weapon']:
                (parts / (name + '.glb')).touch()
            export_part_lists(root, root / 'assets')
            result = json.loads((parts.parent / 'parts.json').read_text())
            self.assertEqual(len(result['parts']), 3)
            self.assertEqual(result['tags'], {
                'warrior_torso_01': dict(mask=0x1003c0, layer=1),
                'warrior_torso_01_female': dict(mask=0x100780, layer=1)})
        self.assertEqual(read_skin_tags(table(rows))['costume']['layer'], 3)

    def test_rejects_bad_headers_truncation_duplicates_and_trailing_bytes(self):
        valid = table([('body', 0x40, 0)])
        for data in [b'', b'BAD!' + valid[4:], valid[:4] + struct.pack('<I', 1) + valid[8:],
                     valid[:-1], valid + b'x', table([('body', 1, 0), ('body', 2, 0)])]:
            with self.subTest(data=data), self.assertRaises(ValueError):
                read_skin_tags(data)


if __name__ == '__main__':
    unittest.main()
