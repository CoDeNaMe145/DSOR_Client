#!/usr/bin/env python3
"""The character sheet's data (client/src/gameplay/stats.rs), from the 2018 client's own files.

Usage: export_stats.py <export_win32> <assets>

Writes characters/stats.json:
  attrs     _Template_ActorAttributes: name -> [initial, lower, upper]
  levels    _Template_XPLevels: class -> level -> [LevelXP, BaseMana, BaseHP, BaseDamage]
  player    _Template_Player of each class (lower-case class name): RunSpeed, RunSpeedMin,
            SkillSpeedMin, Armor, BlockRating, BlockDamageReduction, CriticalRating,
            CriticalDamageFactor, ClassHitpointsFactor, the regenerations, AnimRunSpeed and
            resist [Physical, Fire, Ice, Lightning, DarkMagic, Poison] (Resistances)
  curves    _Template_DmgLvlCurves: level -> {ArmorDamageReductionValue, BlockChanceFactor,
            CriticalChanceFactor, ResistanceDamageReductionValue, BasePowerFactor,
            BasePowerFactorRelative}
  chance    _Template_DifficultyFactors row 0 ChanceMultiplier
  globals   _Globals Max* reductions and chances, MaxItemLevel, ItemUpgradeProgressionVarA..D
  skills    _Template_Skill: id -> [UseWeaponDPS, DamageModifier, English title, CharClass]
  texts     language/en/gui.window.xml, categories gui.window.charsheet and gui.window.general
  items, enchantments, tiers, item_attrs   for the worn items (SEE: stats.rs)
SEE: the spec of the 2018 computation in client/src/gameplay/stats.rs (addresses there).
"""
import json, os, re, sqlite3, sys
import xml.etree.ElementTree as ET

export, assets = sys.argv[1], sys.argv[2]
db = sqlite3.connect(f"file:{os.path.join(export, 'db', 'static.db4')}?mode=ro", uri=True)
db.row_factory = sqlite3.Row


def rows(sql):
    return [dict(r) for r in db.execute(sql)]


DAMAGE = ["Physical", "Fire", "Ice", "Lightning", "DarkMagic", "Poison"]
out = {}
out["attrs"] = {r["Id"]: [r["InitialValue"], r["LowerLimit"], r["UpperLimit"]] for r in rows("select * from _Template_ActorAttributes")}
out["item_attrs"] = {r["Id"]: [r["InitialValue"], r["LowerLimit"], r["UpperLimit"]] for r in rows("select * from _Template_ItemAttributes")}
levels = {}
for r in rows("select * from _Template_XPLevels"):
    levels.setdefault(r["CharClass"].lower(), {})[str(r["Level"])] = [r["LevelXP"], r["BaseMana"], r["BaseHP"], r["BaseDamage"]]
out["levels"] = levels
player = {}
for r in rows("select * from _Template_Player"):
    cls = r["CharClass"].lower()
    if cls in player:
        continue
    resist = [0.0] * 6
    for part in (r["Resistances"] or "").split(";"):
        if ":" in part:
            k, v = part.split(":", 1)
            if k.strip() in DAMAGE:
                resist[DAMAGE.index(k.strip())] = float(v)
    player[cls] = {k: r[k] for k in ("RunSpeed", "RunSpeedMin", "SkillSpeedMin", "Armor", "BlockRating", "BlockDamageReduction",
                                       "CriticalRating", "CriticalDamageFactor", "ClassHitpointsFactor", "HitPointsRegeneration",
                                       "AbsoluteHitPointsRegeneration", "SkillResourceRegeneration",
                                       "AbsoluteSkillResourceRegeneration", "AnimRunSpeed")}
    player[cls]["resist"] = resist
out["player"] = player
# The 2018 client keeps these per-level factors in _Template_DmgLvlCurves; release 261
# moved them to _Template_Progressions (and dropped BasePowerFactor).
CURVE = ("ArmorDamageReductionValue", "BlockChanceFactor", "CriticalChanceFactor",
         "ResistanceDamageReductionValue", "BasePowerFactor", "BasePowerFactorRelative")
curves = rows("select * from _Template_DmgLvlCurves")
if curves and "ArmorDamageReductionValue" not in curves[0]:
    curves = rows("select * from _Template_Progressions")
out["curves"] = {str(r["Level"]): {k: r[k] for k in CURVE if k in r} for r in curves}
out["chance"] = next((r["ChanceMultiplier"] for r in rows("select * from _Template_DifficultyFactors") if str(r["Id"]) == "0"), 1.0)
g = rows("select * from _Globals")[0]
out["globals"] = {k: g[k] for k in ("MaxArmorDamageReduction", "MaxBlockChance", "MaxBlockDamageReduction", "MaxCriticalChance",
                                     "MaxResistanceDamageReduction", "MaxItemLevel", "ItemUpgradeProgressionVarA",
                                     "ItemUpgradeProgressionVarB", "ItemUpgradeProgressionVarC", "ItemUpgradeProgressionVarD")}

# Release 261 moved skill captions to db.skill2.xml. Keep the older table for
# 2018 packs and prefer the newer captions when both are present.
titles = {}
for filename in ("db.skill.xml", "db.skill2.xml"):
    skill_path = os.path.join(export, "language", "en", filename)
    if os.path.isfile(skill_path):
        with open(skill_path, encoding="utf-8-sig") as skill_file:
            titles.update(re.findall(r'<item name="([^"]+)Title"><!\[CDATA\[(.*?)\]\]></item>', skill_file.read(), re.S))
# The class too: release261's class skills have bare ids ("angrystrike").
out["skills"] = {r["Id"]: [bool(r["UseWeaponDPS"]), r["DamageModifier"] if r["DamageModifier"] is not None else 1.0,
                           titles.get(r["Id"], r["Id"]), (r["CharClass"] or "").lower()]
                 for r in rows("select Id, UseWeaponDPS, DamageModifier, CharClass from _Template_Skill")}

texts = {}
root = ET.parse(os.path.join(export, "language", "en", "gui.window.xml")).getroot()
for cat in root.iter("category"):
    if cat.get("name") in ("gui.window.charsheet", "gui.window.general"):
        for item in cat.iter("item"):
            texts[item.get("name")] = item.text or ""
# The current character-page code uses these keys, but even the August 2026
# English language pack lacks them. Prefer the client's captions when present;
# older packs still need labels and a percentage format instead of blank rows.
for key, fallback in {
    "finaldamageincreasevalue": "Final Damage Increase:",
    "elementDamageIncreaseSubHeadline": "Elemental Damage Increase:",
    "fireDamageIncrease": "Fire:",
    "iceDamageIncrease": "Ice:",
    "lightningDamageIncrease": "Lightning:",
    "poisonDamageIncrease": "Poison:",
    "lightDamageIncrease": "Andermagic:",
    "riftforcevalue": "Final Damage Reduction:",
    "riftAttributePercentage": "{0:s}%",
}.items():
    if not texts.get(key):
        texts[key] = fallback
out["texts"] = texts

out["items"] = {r["Id"]: {k: r[k] for k in ("ItemCategory", "SlotType", "CharClass", "RequiredLevel", "ItemLevelScalingType",
                                             "SkillDuration", "MinDamage", "MaxDamage", "Armor", "BlockRating",
                                             "BlockDamageReduction", "CriticalRating", "HealthPoints", "Resistance",
                                             "BaseEnchantments", "BaseModifier", "UniqueModifier")}
                for r in rows("select * from _Template_Item")}
out["enchantments"] = {r["Id"]: {k: r.get(k) for k in ("BaseEnchantment", "UseRarityFactor", "ScalingMode", "ScaleReferenceLevel",
                                                    "MaxOriginItemLevel", "Modifiers")}
                       for r in rows("select * from _Template_Enchantment")}
out["tiers"] = {str(r["Tier"]): r for r in rows("select * from _Template_EnchantmentTiers")}

path = os.path.join(assets, "characters", "stats.json")
with open(path, "w", encoding="utf-8") as f:
    json.dump(out, f, ensure_ascii=False, separators=(",", ":"), default=str)
print({k: len(v) if hasattr(v, "__len__") else v for k, v in out.items()}, os.path.getsize(path))
