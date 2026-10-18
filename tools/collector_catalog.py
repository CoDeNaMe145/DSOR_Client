"""Materialize the collector category buttons and authored banner artwork."""
from ui_catalog import translated_subtree, localized, quad


def collector_pages(widgets, db, textures, loca):
    template = next(w for w in widgets if w['id'].endswith('/TemplateCategory'))
    parent = template['id'].rsplit('/', 1)[0]
    categories = db.execute('''SELECT Id, StickerbookCategoryId FROM _Template_Stickerbook
        WHERE StickerbookSubcategoryId=-1 ORDER BY StickerbookPriority, rowid''').fetchall()
    added = []
    for order, (name, category) in enumerate(categories):
        target = f'{parent}/AlbumCategory{category}'
        copies = translated_subtree(widgets, template['id'], target, dy=order*34)
        caption = localized(loca, f'db.stickerbookcategory.{name}Title',
                            'Favorites' if category == 0 else name.removeprefix('sb_').title())
        for w in copies:
            w['hidden'] = False
            if w['id'] == target:
                w['event'] = f'AlbumCategory:{category}'
            if w.get('text'):
                # Generated labels use gui.* so the runtime recognizes them as
                # captions, rather than unresolved data-template samples.
                w['text'].update(text=caption, key='gui.client.album.category', size=8.5)
        added.extend(copies)
        if category:
            base = f'Canvas/Collectorsalbum/BackGroundLabel/Inlay/AlbumPage{category}'
            added.append(dict(id=base+'/Title', type='TextLabel', k=.5, rect=[220,110,950,146], hidden=False,
                text=dict(text=caption, key='gui.client.album.category', size=12, color=[.9,.78,.54,1], align=[1,1])))
            texture = 'ui/collectorsalbum_preview_background_crunch'
            if textures.get(texture):
                rect = [660,190,955,530]
                added.append(dict(id=base+'/Artwork', type='Label', k=.5, rect=rect, hidden=False,
                                  art=quad(rect, f'textures/{texture}.png')))
    # This is artwork from the game, independent of any shop or ownership data.
    banner = db.execute('''SELECT TexturePrefix,TextureSuffix FROM _Template_StickerbookBanner
        WHERE IsActive=1 ORDER BY Priority, rowid LIMIT 1''').fetchone()
    for w in widgets:
        if w['id'].endswith('/AdvertisementArtwork') and banner:
            texture = banner[0]+'en'+banner[1]
            if textures.get(texture):
                w['art'] = quad(w['rect'], f'textures/{texture}.png')
                w['hidden'] = False
    return widgets + added
