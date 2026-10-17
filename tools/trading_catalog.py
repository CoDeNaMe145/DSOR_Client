"""Presentation metadata for release 261 NPC shops; contents/prices stay on the wire."""

# IngamePaymentData::StringToOfferCategory, also used by shop_protocol.py.
# Category IDs are protocol values, not database row numbers.
CATEGORIES = [
    ('Potions', 'drink'), ('Elixirs', 'elixir'), ('Tonics', 'tonic'), ('Buffs', 'buffs'),
    ('Gems', 'skillgem'), ('One-handed weapons', 'weapon'), ('Two-handed weapons', 'weapon_01'),
    ('Armor', 'armor'), ('Shields', 'shield'), ('Jewelry', 'misc'), ('Glyphs', 'gemupgrade'),
    ('Essences', 'essences'), ('Dyes', 'color'), ('Mounts', 'mount'), ('Packages', 'packages'),
    ('Pets', 'pet'), ('Event', 'event'), ('Gambler', 'gambler'), ('Premium', 'premium'),
    ('Daily premium', 'premium24'), ('Cloaks', 'cloak'), ('Consumable emotes', 'emote_01'),
    ('Emotes', 'emote'), ('Temporary costumes', 'costume_01'), ('Costumes', 'costume'),
    ('Lucky spheres', 'luckysphere_andemant'), ('Draken lucky spheres', 'luckysphere_draken'),
    ('Draken offers', 'luckysphere_gen_draken'), ('Limited offers', 'luckysphere_limited'),
    ('Transfer items', 'misc'), ('Gem upgrades', 'newgems'), ('Recipes', 'recipe'),
    ('Andermant recipes', 'recipe_andemant'), ('Draken recipes', 'recipe_draken'),
    ('Materi recipes', 'recipe_materi'), ('Honor recipes', 'recipe_honor'),
    ('Ingredients', 'ingredients'), ('Andermant ingredients', 'ingredients_andemant'),
    ('Draken ingredients', 'ingredients_draken'), ('Materi ingredients', 'ingredients_materi'),
    ('Honor ingredients', 'ingredients_honor'), ('Travel items', 'travelitem_normal'),
    ('Event travel', 'travelitem_event'), ('Parallel World travel', 'travelitem_pw'),
    ('Appearances', 'color'), ('Season pass', 'seasonpass'), ('Miscellaneous', 'misc'), ('QA', 'all'),
]


def export_trading(database, textures):
    def texture(brush):
        brush = (brush or '').replace('\\', '/')
        return f'textures/{brush}.png' if brush and textures.get(brush) else ''

    return dict(
        categories={str(code): dict(name=name, icon=texture('ui/icon_tab_' + icon))
                    for code, (name, icon) in enumerate(CATEGORIES)},
        offers={oid: dict(icon=texture(icon), class_name=(cls or '').lower())
                for oid, icon, cls in database.execute('SELECT Id, IconBrush, CharClass FROM _Template_Offer')},
        currencies={shop: texture(icon) for shop, icon in database.execute(
            'SELECT Id, CurrencyIcon FROM _Template_ItemCurrency')},
        glyph=texture('ui/icon_currency_glyph'),
    )
