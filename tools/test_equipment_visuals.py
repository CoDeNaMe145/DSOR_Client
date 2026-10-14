import sqlite3
import unittest
from equipment_visuals import equipment_visuals


class EquipmentVisualTests(unittest.TestCase):
    def test_server_resolution_precedence_and_multi_part_graphics(self):
        db = sqlite3.connect(':memory:')
        db.executescript('''
            CREATE TABLE _Template_Item(Id, Skin, EquipmentSkinId);
            CREATE TABLE _Template_EquipmentSkin(Id, SkinGraphics);
            CREATE TABLE _Template_ItemAppearance(Id, SkinGraphics);
            INSERT INTO _Template_EquipmentSkin VALUES ('basic', 'helm; helm_fx'), ('override', 'old');
            INSERT INTO _Template_ItemAppearance VALUES ('override', 'new;new'), ('basic', '');
            INSERT INTO _Template_Item VALUES ('direct', 'direct_mesh', 'basic'), ('indirect', '', 'basic'), ('ring', '', '');
        ''')
        data = equipment_visuals(db)
        self.assertEqual(data['items'], {'direct': ['direct_mesh'], 'indirect': ['helm', 'helm_fx']})
        self.assertEqual(data['appearances'], {'basic': ['helm', 'helm_fx'], 'override': ['new']})

    def test_old_database_without_appearance_tables(self):
        db = sqlite3.connect(':memory:')
        db.executescript("CREATE TABLE _Template_Item(Id, Skin); INSERT INTO _Template_Item VALUES ('old', 'mesh');")
        self.assertEqual(equipment_visuals(db), {'items': {'old': ['mesh']}, 'appearances': {}})


if __name__ == '__main__':
    unittest.main()
