"""Export equipment graphics using the server's release-matched resolution rules."""
import json
from pathlib import Path


def equipment_visuals(db):
    def rows(table):
        if not db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (table,)).fetchone():
            return []
        cursor = db.execute(f'SELECT * FROM "{table}"')
        columns = [c[0] for c in cursor.description]
        return [dict(zip(columns, row)) for row in cursor]

    def parts(value):
        return list(dict.fromkeys(p.strip() for p in (value or '').split(';') if p.strip()))

    equipment = {r['Id']: parts(r.get('SkinGraphics')) for r in rows('_Template_EquipmentSkin')}
    # Presence resolves ItemAppearance first, then EquipmentSkin, then the item.
    appearances = {key: value for key, value in equipment.items() if value}
    appearances.update({r['Id']: parts(r['SkinGraphics']) for r in rows('_Template_ItemAppearance') if r.get('SkinGraphics')})
    items = {}
    for row in rows('_Template_Item'):
        skin = parts(row.get('Skin')) or equipment.get(row.get('EquipmentSkinId'), [])
        if skin:
            items[row['Id']] = skin
    return dict(items=items, appearances=appearances)


def export_equipment(db, assets):
    data = equipment_visuals(db)
    path = Path(assets) / 'characters' / 'item_skins.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, separators=(',', ':')), encoding='utf-8')
    print(f"item_skins.json: {len(data['items'])} items, {len(data['appearances'])} appearances")


if __name__ == '__main__':
    import argparse
    import sqlite3
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database')
    parser.add_argument('assets')
    args = parser.parse_args()
    with sqlite3.connect(Path(args.database).resolve().as_uri() + '?mode=ro', uri=True) as db:
        export_equipment(db, args.assets)
