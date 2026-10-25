"""The Skills & Talents window's data (skilltalentswindow: the Experience,
Wisdom, Group and Honor tabs).

Usage (alone, without re-exporting any window):
  talent_catalog.py <export_win32> <assets> <DSO_Godot> [--database static.db4]
export_ui.py also writes it whenever it exports `skilltalentswindow`.

Writes <assets>/interface/talents.catalog.json and the talents' icons as PNG:
  {"skill_levels": [[level, points granted], ...],          _Template_TalentXPLevelSkill
   "levels": {"Knowledge"|"Group"|"Fame": [[level, xp, points], ...]},
   "skills": {id: talent},       _Template_SkillTalent (skills and skill talents)
   "categories": [category],     _Template_TalentCategories, in table order
   "talents": [talent],          _Template_Talents (Wisdom and Group), table order
   "traits": [trait],            _Template_Trait (Honor), table order
   "focus": [focus],             _Template_FocusTalent (Honor), table order
   "globals": {name: value},     the _Globals the window reads
   "respec": [multiplier, ...],  RespecVCCostMultiplier by character level - 1
   "preset_effects": {wire: tree}, the status effects holding a tree's presets
   "reset_effects": [wire, ...], the status effects changing reset costs
   "texts": {key: text}}         the gui.window texts it shows
A talent: {"class", "tree" (Skill/Knowledge/Group/Fame), "category" (its
CategoryTemplate, or the skill tree's GroupId), "max", "unlock_cost", "cost",
"mult", "unlock" (level), "icon" (texture path), "name", "description",
"description2", "skill" (SkillTemplateId), "effect", "stack_effect",
"tokens"}. Costs per rank are the server's (talent_protocol.py
point_cost_for_rank): rank 1 costs UnlockCosts, rank r >= 2
round(CostsPerLevel * (1 + mult * (r - 2))).

A description's "{n:t:Token}" names a _Template_LocaleToken; "tokens" keeps
what the window needs to show its value at a rank: a talent's actor variable
grows by its stack effect's value with every rank after the first.
"""
import json
import math
import os
import re
import sys

CLASSES = ("warrior", "mage", "ranger", "dwarf", "niwalk")
TREES = ("Skill", "Knowledge", "Group", "Fame")
# _Globals the window and its dialogs read.
GLOBALS = ("VCRespecCostsPerPoint", "TalentRespecCosts", "SkillTalentCooldown", "WisdomTalentCooldown",
           "GroupTalentCooldown", "WisdomTalentCategoryCooldown", "GroupTalentCategoryCooldown",
           "UnlockPreselectionCost")
TEXT_PREFIXES = ("gui.window.skilltalentswindow_", "gui.window.talentwindow_", "gui.window.general_confirmVCwidgetTalentRespecQuestion",
                 "gui.window.general_groupUnlockedInfo", "gui.window.general_honorUnlockedInfo", "gui.window.inventory_unstackwindow",
                 "gui.window.messages_talent", "gui.window.messages_skillTalentRespec", "gui.window.levelup_", "gui.window.general_wisdom",
                 "gui.window.general_bottomBar")


def _text(loca, key, fallback=""):
    # The localization's bullets came through as replacement characters
    # (cp1252 0x95 read as UTF-8).
    return (loca.get("en", {}).get(key) or fallback).replace("�", "•")


def _icon(textures, brush):
    brush = (brush or "").strip().replace("\\", "/")
    return f"textures/{brush}.png" if brush and textures.get(brush) else ""


def _rows(db, sql, *args):
    cursor = db.execute(sql, args)
    keys = [c[0] for c in cursor.description]
    return [dict(zip(keys, row)) for row in cursor]


def _modifiers(program):
    """'Kind:a,b,c;...' -> [(kind, [args])]."""
    out = []
    for part in (program or "").split(";"):
        kind, sep, args = part.strip().partition(":")
        if sep and kind:
            out.append((kind.strip(), [a.strip() for a in args.split(",")]))
    return out


def _variable(effects, effect, name):
    """The value an effect's ActorVariable modifiers give a variable."""
    total = None
    for kind, args in _modifiers((effects.get(effect) or {}).get("StartModifiers")):
        if kind == "ActorVariable" and len(args) >= 3 and args[2] == name:
            try:
                total = (total or 0.0) + float(args[0])
            except ValueError:
                pass
    return total


def _initial(row):
    """An actor variable's starting value, within its limits (the
    talent_*_show variables start at 0 under a lower limit of 0.01)."""
    row = row or {}
    value = row.get("InitialValue")
    value = float(value) if isinstance(value, (int, float)) else 0.0
    lower, upper = row.get("LowerLimit"), row.get("UpperLimit")
    if isinstance(lower, (int, float)) and isinstance(upper, (int, float)) and lower <= upper:
        value = min(max(value, float(lower)), float(upper))
    return value


def _tokens(texts, tokens, effects, variables, preview, talent):
    """The tokens of a talent's texts: {name: {"format", "abs", "base", "step"}}
    (the value at rank r is base + step * (r - 1)) or {"value"} for a fixed one.
    A token the window cannot evaluate is left out (an em dash at run time)."""
    out = {}
    for text in texts:
        for name in re.findall(r"\{\d+:t:([^}]+)\}", text or ""):
            token = tokens.get(name)
            if not token or name in out:
                continue
            entry = {"format": token.get("ValueFormat") or "", "abs": bool(token.get("IsAbsoluteBonus"))}
            kind = token.get("TokenType")
            variable = token.get("Param1Id") or ""
            base = _variable(effects, talent.get("effect"), variable)
            step = _variable(effects, talent.get("stack_effect"), variable)
            if kind in ("TalentActorVariable", "ActorVariable") and (base is not None or step is not None):
                # The talent's own variable: its initial value, what the
                # unlock effect adds, then the stack effect once a rank after
                # the first (WT_all_damage: 75 + 75 per rank).
                entry["base"] = _initial(variables.get(variable)) + (base or 0.0)
                entry["step"] = step or 0.0
            elif kind == "TalentActorVariable":
                if variable not in variables:
                    continue
                entry["value"] = _initial(variables[variable])
            elif kind == "StatusEffect" and (token.get("Param1Attr") or "").strip() == "StartModifier":
                # Param2Attr: the modifier's index in StartModifiers.
                mods = _modifiers((effects.get(token.get("Param1Id")) or {}).get("StartModifiers"))
                try:
                    entry["value"] = float(mods[int(token.get("Param2Attr") or 0)][1][0])
                except (IndexError, ValueError):
                    continue
            else:
                # Durations, ranges, a skill's fields, an immutable variable:
                # the skill previews' resolver (tooltip_catalog.SkillPreview).
                value = preview.value(token, talent.get("skill") or "") if not token.get("LevelModifier") else None
                if not isinstance(value, (int, float)) or not math.isfinite(value):
                    continue
                entry["value"] = float(value)
            out[name] = entry
    return out


def export_talents(db, textures, loca, assets):
    effects = {r["Id"]: r for r in _rows(db, "SELECT Id, StartModifiers, MaxStackSize FROM _Template_StatusEffect")}
    tokens = {r["Id"]: r for r in _rows(db, "SELECT * FROM _Template_LocaleToken")}
    variables = {r["Id"]: r for r in _rows(db, "SELECT * FROM _Template_ActorVariables")}
    from tooltip_catalog import SkillPreview
    preview = SkillPreview(db)

    def talent(row, tree, name_key, description_key, description2_key=None):
        t = {
            "class": row.get("CharClass") or "",
            "tree": tree,
            "category": row.get("CategoryTemplate") or row.get("GroupId") or "",
            "max": max(1, int(row.get("MaxLevel") or 1)),
            "unlock_cost": max(0, int(row.get("UnlockCosts") or 0)),
            "cost": max(0, int(row.get("CostsPerLevel") or 0)),
            "mult": float(row.get("CostsPerLevelMultiplier") or 0.0),
            "unlock": max(0, int(row.get("UnlockLevel") or 0)),
            "icon": _icon(textures, row.get("Icon")),
            "name": _text(loca, name_key, row["Id"]),
            "description": _text(loca, description_key),
            "effect": row.get("BaseStatusEffectId") or "",
            "stack_effect": row.get("StackStatusEffectId") or "",
        }
        if description2_key:
            t["description2"] = _text(loca, description2_key)
        t["tokens"] = _tokens([t["description"], t.get("description2", "")], tokens, effects, variables, preview, t)
        return t

    # A skill is learnt at its own UnlockLevel (_Template_Skill) and upgraded
    # from its talent's (one level later).
    learnt = {r["Id"]: int(r["UnlockLevel"] or 0) for r in _rows(db, "SELECT Id, UnlockLevel FROM _Template_Skill")}
    skills = {}
    for row in _rows(db, "SELECT * FROM _Template_SkillTalent"):
        tid, sid = row["Id"], row.get("SkillTemplateId") or ""
        is_skill = row.get("Category") == "Skill"
        name = (_text(loca, f"db.skill2.table_{sid}Title") or sid) if is_skill else _text(loca, f"db.skilltalent2.table_{tid}Title", tid)
        t = talent(row, "Skill", "", f"db.skilltalent2.table_{tid}Description", f"db.skilltalent2.table_{tid}Description2")
        t.update(name=name, kind=row.get("Category") or "", skill=sid)
        if is_skill:
            t["learn"] = learnt.get(sid, 0)
        skills[tid] = t

    categories = []
    for row in _rows(db, "SELECT rowid, * FROM _Template_TalentCategories ORDER BY rowid"):
        categories.append({"id": row["Id"], "tree": row["TalentCategory"], "column": int(row.get("Column") or 0),
                           "max": int(row.get("MaxLevel") or 0),
                           "title": _text(loca, f"db.talentcategory.table_{row['Id']}Title", row["Id"])})

    talents = []
    for row in _rows(db, "SELECT rowid, * FROM _Template_Talents ORDER BY rowid"):
        t = talent(row, row["TalentCategory"], f"db.talents.table_{row['Id']}Title",
                   f"db.talents.table_{row['Id']}Description", f"db.talents.table_{row['Id']}Description2")
        t["id"] = row["Id"]
        talents.append(t)

    traits = []
    for row in _rows(db, "SELECT rowid, * FROM _Template_Trait ORDER BY rowid"):
        prefix = row["Id"].split("_", 1)[0]
        effect = row.get("StatusEffectId") or ""
        traits.append({"id": row["Id"], "class": prefix if prefix in CLASSES else "", "tree": row.get("TalentCategory") or "Fame",
                       # One rank, at its UnlockCosts (talent_protocol.py definition).
                       "icon": _icon(textures, row.get("Icon")), "max": 1, "unlock_cost": int(row.get("UnlockCosts") or 0),
                       "cost": int(row.get("UnlockCosts") or 0), "unlock": int(row.get("UnlockLevel") or 0), "effect": effect,
                       "name": _text(loca, f"db.trait.table_{row['Id']}Title", row["Id"]),
                       "description": _text(loca, f"db.trait.table_{row['Id']}Description")})

    focus = []
    for row in _rows(db, "SELECT rowid, * FROM _Template_FocusTalent ORDER BY rowid"):
        effect = row.get("StatusEffectId") or ""
        # A point a rank, 25 ranks, no level (release261 0x140a61f30 hard-codes
        # them; the server reads its effect's MaxStackSize, 25 too).
        t = {"id": row["Id"], "tree": row.get("TalentCategory") or "Fame", "icon": _icon(textures, row.get("Icon")),
             "effect": effect, "max": 25,
             "unlock_cost": 1, "cost": 1, "unlock": 0,
             "name": _text(loca, f"db.talents.table_{row['Id']}Title", row["Id"]),
             "description": _text(loca, f"db.talents.table_{row['Id']}Description")}
        # "Each point ... by {0:s}%. Current total bonus: {1:s}%": the effect's
        # first modifier value, per point.
        mods = _modifiers((effects.get(effect) or {}).get("StartModifiers"))
        try:
            t["per_point"] = float(mods[0][1][0])
        except (IndexError, ValueError):
            t["per_point"] = 0.0
        focus.append(t)

    levels = {}
    for tree in TREES[1:]:
        levels[tree] = [[int(r["Level"]), int(r["XP"]), int(r["TalentPointsGranted"])]
                        for r in _rows(db, "SELECT Level, XP, TalentPointsGranted FROM _Template_TalentXPLevel WHERE TalentCategory = ? ORDER BY Level", tree)]
    skill_levels = [[int(r["Level"]), int(r["TalentPointsGranted"])]
                    for r in _rows(db, "SELECT Level, TalentPointsGranted FROM _Template_TalentXPLevelSkill ORDER BY Level")]

    row = _rows(db, "SELECT * FROM _Globals")[0]
    globals_ = {name: row[name] for name in GLOBALS if name in row}
    # A reset's gold: VCRespecCostsPerPoint x RespecVCCostMultiplier[level] x
    # points spent (release261 0x1409d0960), by character level.
    respec = [float(r["RespecVCCostMultiplier"] or 0) for r in
              _rows(db, "SELECT Level, RespecVCCostMultiplier FROM _Template_DynamicLevelCosts ORDER BY Level")]
    # The effects that hold a tree's presets (Groups SkillPreset, KnowledgePreset,
    # GroupPreset) and the one that changes reset costs, by wire index (rowid - 1).
    preset_effects = {}
    for r in _rows(db, "SELECT rowid - 1 AS wire, Groups FROM _Template_StatusEffect"):
        groups = (r["Groups"] or "").split(";")
        for tree, group in (("Skill", "SkillPreset"), ("Knowledge", "KnowledgePreset"), ("Group", "GroupPreset")):
            if group in groups:
                preset_effects[str(r["wire"])] = tree
    reset_effects = [int(r["wire"]) for r in _rows(db, "SELECT rowid - 1 AS wire, StartModifiers FROM _Template_StatusEffect")
                     if "TalentRespecCost:" in (r["StartModifiers"] or "")]

    table = loca.get("en", {})
    texts = {key: _text(loca, key) for key in sorted(table) if key.startswith(TEXT_PREFIXES)}
    # The window's own preset and reset icons.
    for brush in ("ui/button_latin_number_1", "ui/button_latin_number_5", "ui/button_general_icons", "ui/talentwindow_leveltree"):
        textures.get(brush)

    catalog = {"skill_levels": skill_levels, "levels": levels, "skills": skills, "categories": categories,
               "talents": talents, "traits": traits, "focus": focus, "globals": globals_, "respec": respec,
               "preset_effects": preset_effects, "reset_effects": reset_effects, "texts": texts}
    path = os.path.join(assets, "interface", "talents.catalog.json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(catalog, f, ensure_ascii=False, indent=0)
    return f"{len(skills)} skill tree entries, {len(talents)} talents in {len(categories)} categories, {len(traits)} traits, {len(focus)} focus talents"


if __name__ == "__main__":
    import sqlite3
    root, assets, godot = sys.argv[1], sys.argv[2], sys.argv[3]
    database = sys.argv[sys.argv.index("--database") + 1] if "--database" in sys.argv else os.path.join(root, "db", "static.db4")
    # The icons are Crunch textures (export_ui.py does the same).
    crunch = os.path.abspath(os.path.join(godot, "build", "crn2dds.exe"))
    if os.name == "nt" and os.path.isfile(crunch):
        os.environ.setdefault("DSO_CRN2DDS", crunch)
    sys.path.insert(0, godot)
    import export_ui as gx  # noqa: E402  DSO_Godot's exporter
    from dsoexp.resources import Resolver  # noqa: E402
    db = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    tex = gx.Textures(Resolver(root), os.path.join(assets, "textures"), "textures")
    print("talents:", export_talents(db, tex, gx.load_loca(root, ["en"]), assets))
