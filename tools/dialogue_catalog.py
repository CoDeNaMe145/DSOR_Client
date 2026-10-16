"""NPC portraits and English conversation text used by the runtime NPC screen."""
import json
import os
import re


def plain_text(value):
    # Nebula's inline colour tags are not HTML and are not understood by Text2d.
    value = re.sub(r"<br\s*/?>", "\n", value, flags=re.I)
    return re.sub(r"<[^>]*>", "", value).strip()


def export_dialogue(database, textures, localization, assets):
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
    texts = {k.lower(): plain_text(v) for k, v in localization.get("en", {}).items()
             if k.startswith(("quest.", "db.npc", "gui.npc", "gui.quest", "gui.dialog"))}
    with open(os.path.join(assets, "interface", "dialogue.catalog.json"), "w", encoding="utf-8") as f:
        json.dump({"portraits": available, "icons": icons, "texts": texts}, f, ensure_ascii=False, separators=(",", ":"))
    return {"portraits": len(available), "texts": len(texts)}
