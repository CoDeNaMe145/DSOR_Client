#!/usr/bin/env python3
"""Floating combat texts (damage numbers, "Block!"...), from the 2018 client's data.

Usage: export_combat_text.py <export_win32> <assets> [<DSO_Godot>]

Writes interface/combat_text.json:
  types: CombatTextType name -> {color [r,g,b,a], duration, fade, offset_y [x,y,z],
         offset_x [x,y,z], offset_x_var, dynamic_x, velocity [x,y,z], velocity_var,
         size, label}   (static.db4 _Template_CombatText, one row per type)
  labels: overhead label name -> {dt, y [...], scale [...], alpha [...], outline,
          size}: the label's UI animation (ui/overheadtext.bxml AnimRes
          anims:ui/overheadtext.nax3, AnimGroup = clip, AnimDuration), one key per
          dt seconds -- translation y (UI units, y down), scale, alpha (the scale
          curve's w); its outline (TextEffectSize) and fontSize. Needs DSO_Godot.
  texts: block, critical ("{0:i} !"), immunity, low_health, loot, loot_amount,
         sudden_death (language/<DSOR_LANG, en>/gui.window.xml floatingBlock,
         floatingCritical, floatingDamageImmunity, lowhealthhit, lootReceived,
         lootReceivedAmount, suddenDeath)
  currencies: a loot message's currency type -> its name (the no-auto-use
         currency item's Title, db.noautousecurrency.xml): 0 coins, 1 Andermant,
         3 ancestral knowledge, 5 glyphs of power. EVIDENCE (release261):
         HandleMessageCommand's loot case 0x1401d09ac reads
         currencyItemTemplateIdsNoAutoUse[type], filled at 0x1400b5916 in the
         order vc, rc, hc, gc, -, uc.
EVIDENCE: TemplateManager's combat text getters read these columns by type
  (GetCombatTextColor 0x51212B: +0x10 colour; 0x5121E1 +0x30 x offset; 0x51226A
  +0x40 its variation; 0x5122C9 +0x50 y offset; 0x5123FD +0x60 velocity; 0x512486
  +0x70 its variation; 0x512352 +0x74 size; 0x5123B5 +0x78 template name;
  0x512196 +0xB8 dynamic x offset).
"""
import json, os, re, sqlite3, struct, sys

export, assets = sys.argv[1], sys.argv[2]
db = sqlite3.connect(f"file:{os.path.join(export, 'db', 'static.db4')}?mode=ro", uri=True)


def vec(blob, n):
    return [round(v, 6) for v in struct.unpack("<4f", blob)[:n]]


types = {}
for row in db.execute(
    "select CombatTextType, CombatTextColor, CombatTextDuration, CombatTextFadeOutDuration,"
    " CombatTextOffsetYBase, CombatTextOffsetXBase, CombatTextOffsetXVariation,"
    " CombatTextDynamicXOffsetEnabled, CombatTextVelocity, CombatTextVelocityVariation,"
    " CombatTextSize, CombatTextTemplateName from _Template_CombatText"
):
    name, color, duration, fade, oy, ox, oxv, dyn, vel, velv, size, label = row
    types[name] = {
        "color": vec(color, 4), "duration": duration, "fade": fade,
        "offset_y": vec(oy, 3), "offset_x": vec(ox, 3), "offset_x_var": oxv,
        "dynamic_x": bool(dyn), "velocity": vec(vel, 3), "velocity_var": velv,
        "size": size, "label": label or "",
    }

lang = os.environ.get("DSOR_LANG", "en")


def loca_of(name):
    xml = open(os.path.join(export, "language", lang, name), encoding="utf-8-sig").read()
    return dict(re.findall(r'<item name="([^"]+)"><!\[CDATA\[(.*?)\]\]></item>', xml, re.S))


loca = loca_of("gui.window.xml")
money = loca_of("db.noautousecurrency.xml")
currencies = {str(k): money.get(f"{t}_no_auto_useTitle", t) for k, t in
              ((0, "money_vc"), (1, "money_rc"), (3, "money_gc"), (5, "money_uc"))}
texts = {
    "block": loca.get("floatingBlock", "Block!"),
    "critical": loca.get("floatingCritical", "{0:i}!"),
    "immunity": loca.get("floatingDamageImmunity", "Immune"),
    "low_health": loca.get("lowhealthhit", ""),
    "loot": loca.get("lootReceived", "{0:s}"),
    "loot_amount": loca.get("lootReceivedAmount", "{0:i} {0:s}"),
    "sudden_death": loca.get("suddenDeath", ""),
}
labels = {}
if len(sys.argv) > 3:
    sys.path.insert(0, sys.argv[3])
    from dsoexp import bxml, nax
    doc = bxml.parse(open(os.path.join(export, "ui", "overheadtext.bxml"), "rb").read())
    clips = nax.load(os.path.join(export, "anims", "ui", "overheadtext.nax3"))
    clips = clips if isinstance(clips, list) else clips.clips

    def walk(n):
        a = n.attrs
        if a.get("AnimGroup") is not None and a.get("id"):
            c = clips[int(a["AnimGroup"])]
            dt = c.duration / max(1, c.num_keys)
            kinds = {cv.curve_type: i for i, cv in enumerate(c.curves)}
            tr, sc = kinds.get(nax.TRANSLATION), kinds.get(nax.SCALE)
            keys = range(c.num_keys)
            labels[a["id"]] = {
                "dt": round(dt, 6),
                "y": [round(c.sample(tr, f)[1], 5) for f in keys] if tr is not None else [],
                "scale": [round(c.sample(sc, f)[0], 5) for f in keys] if sc is not None else [],
                "alpha": [round(c.sample(sc, f)[3], 5) for f in keys] if sc is not None else [],
                "outline": float(a.get("TextEffectSize", "0") or 0) if a.get("TextEffect") == "1" else 0.0,
                "size": float(a.get("fontSize", "10") or 10),
            }
        for ch in n.children:
            walk(ch)

    walk(doc if hasattr(doc, "children") else doc.root)
out = os.path.join(assets, "interface", "combat_text.json")
# A fresh conversion runs this before the interface export makes the folder.
os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, "w") as f:
    json.dump({"types": types, "texts": texts, "labels": labels, "currencies": currencies}, f, ensure_ascii=False, separators=(",", ":"))
print({"types": len(types), "labels": sorted(labels), "texts": texts})
