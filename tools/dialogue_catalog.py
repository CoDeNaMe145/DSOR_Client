"""NPC portraits and English conversation text used by the runtime NPC screen."""
import json
import os
import re


def plain_text(value):
    # Nebula's inline colour tags are not HTML and are not understood by Text2d.
    value = re.sub(r"<br\s*/?>", "\n", value, flags=re.I)
    return re.sub(r"<[^>]*>", "", value).strip()


# Quest texts that colour words (<QuestHighlightStart>Varholm</>, <cFFFFFFFF>...</>):
# kept with their tags for the NPC's quest screen, which draws those words white
# (crate::ui::dialogue::quest). Every other reader takes the plain `texts`.
HIGHLIGHT = re.compile(r"<(QuestHighlightStart|c[0-9A-Fa-f]{8})>")


def marked_text(value):
    return re.sub(r"<br\s*/?>", "\n", value, flags=re.I).strip()


def export_dialogue(database, textures, localization, assets):
    from trading_catalog import export_trading
    portraits = set()
    for (portrait,) in database.execute("SELECT DISTINCT Portrait FROM _Template_NPC"):
        if portrait:
            portraits.add(portrait.replace("\\", "/"))
    portraits.add("npc_portraits/wip")
    available = {p: f"textures/{p}.png" for p in sorted(portraits) if textures.get(p)}
    icons = {}
    for name, brush in {
        "close": "ui/icon_close", "shop": "ui/icon_trademark",
        "quest": "ui/icon_questmark_query", "talk": "ui/icon_questionmark",
    }.items():
        if textures.get(brush):
            icons[name] = f"textures/{brush}.png"
    english = localization.get("en", {})
    texts = {k.lower(): plain_text(v) for k, v in english.items()
             if k.startswith(("quest.", "db.npc", "gui.npc", "gui.quest", "gui.dialog", "gui.window.questscreen"))}
    marked = {k.lower(): marked_text(v) for k, v in english.items() if k.startswith("quest.") and HIGHLIGHT.search(v)}
    with open(os.path.join(assets, "interface", "dialogue.catalog.json"), "w", encoding="utf-8") as f:
        json.dump({"portraits": available, "icons": icons, "texts": texts, "marked": marked,
                   "trading": export_trading(database, textures)}, f, ensure_ascii=False, separators=(",", ":"))
    return {"portraits": len(available), "texts": len(texts), "marked": len(marked)}
