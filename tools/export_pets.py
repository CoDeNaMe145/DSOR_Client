#!/usr/bin/env python3
"""Pets: the companions a pet status effect puts at its holder's side.

Usage: export_pets.py <export_win32> <assets> [--database static.db4]

Writes characters/pets.json:
  effects  status effect wire index (_Template_StatusEffect row - 1) -> the pet
           its StartModifiers name ("Pet:PetMonster,<id>[,respawn]#~")
  pets     _Template_PetMonster Id -> {graphics, set, anim_set, run_speed,
           anim_run_speed, dye, summon, death, alt_death, idle_alt, permanent,
           despawn_s, spawn_s}

The server only sends the status effect (StatusEffectCommand 82, a remote
player's status snapshot); the client makes the pet itself (2018/2026 client
Properties::PetProperty::CreatePet). Sequence names are bare: the client looks
SummonSequence up under spawn/, DeathSequence under death/ and
AlternativeIdleSequence under misc/ (PetSequenceProperty), and
PermanentSequence carries its folder ("statuseffect/..."), but
skills/sequences.json keys every sequence by its bare name.
spawn_s: the length of the anim set's "Spawn" clip, for a SummonSequence with
no animation track of its own (10 of 178 pets): the model still rises
(StartAnimationState "Spawn;Idle"). Needs characters/monster_anims.json
(export_monsters.py) and the models.
set: two pets name a skin as their CharacterSet (pet_pixie_golden_01 in the
pixie's skin list pet_pixie_golden, pet_little_bloodmage_01 in
pet_little_bloodmage). The client's ApplySkinList (0x14067bb48) then finds no
list and logs it; what the character shows next was not traced. Here they get
the list holding that skin: the look the template names (the client loader
refuses a model whose set is not in <model>.sets.json).
Not in the client's data: statuseffect/pet_fire_slime_S8 (pet_fire_slime_S8's
PermanentSequence; no sequence pack or sequences.freg lists it).

Use the server's static database (--database): wire indices are its row order.
"""
import json, os, re, sqlite3, struct, sys

sys.path.insert(0, os.path.dirname(__file__))
from fix_materials_io import read_glb  # noqa: E402

export, assets = sys.argv[1], sys.argv[2]
database = sys.argv[sys.argv.index("--database") + 1] if "--database" in sys.argv else os.path.join(export, "db", "static.db4")
db = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
db.row_factory = sqlite3.Row


def bare(name):
    return (name or "").rsplit("/", 1)[-1]


def colour(blob):
    return list(struct.unpack("<4f", blob[:16])) if blob and len(blob) >= 16 else [1.0, 1.0, 1.0, 0.0]


effects = {}
for r in db.execute("select rowid, StartModifiers from _Template_StatusEffect order by rowid"):
    for args in re.findall(r"(?:^|;)Pet:([^;#]*)", r["StartModifiers"] or ""):
        parts = [p.strip() for p in args.split(",")]
        if len(parts) >= 2 and parts[0] == "PetMonster" and parts[1]:
            effects[r["rowid"] - 1] = parts[1]

anims_path = os.path.join(assets, "characters", "monster_anims.json")
anims = json.load(open(anims_path)) if os.path.exists(anims_path) else {}
clip_cache = {}


def clip_seconds(graphics, clip):
    if graphics not in clip_cache:
        lengths = {}
        try:
            doc, _ = read_glb(os.path.join(assets, graphics + ".glb"))
            for a in doc.get("animations", []):
                lengths[a.get("name", "")] = max(doc["accessors"][sm["input"]]["max"][0] for sm in a["samplers"])
        except Exception:  # noqa: BLE001 -- no model: no rise
            pass
        clip_cache[graphics] = lengths
    lengths = clip_cache[graphics]
    return lengths.get(clip) or lengths.get(clip + "-loop") or 0.0


sequences_path = os.path.join(assets, "skills", "sequences.json")
sequences = json.load(open(sequences_path)) if os.path.exists(sequences_path) else {}
sets_cache = {}


def character_set(graphics, name):
    """`name`, or the model's skin list holding a skin of that name."""
    if graphics not in sets_cache:
        path = os.path.join(assets, graphics + ".sets.json")
        sets_cache[graphics] = json.load(open(path)) if os.path.exists(path) else None
    sets = sets_cache[graphics]
    if not name or sets is None or name in sets:
        return name
    return next((list_name for list_name, s in sets.items() if name in s.get("skins", [])), name)


pets = {}
resolved = {}
for r in db.execute("select * from _Template_PetMonster"):
    graphics = r["Graphics"] or ""
    skin_list = character_set(graphics, r["CharacterSet"] or "")
    if skin_list != (r["CharacterSet"] or ""):
        resolved[r["Id"]] = f"{r['CharacterSet']} -> {skin_list}"
    pet = {
        "graphics": graphics,
        "set": skin_list,
        "anim_set": r["AnimSet"] or "",
        # RunSpeed: how fast it follows; AnimRunSpeed: the speed its run clip
        # was made for (2.0 for the slow ones).
        "run_speed": r["RunSpeed"] or 0.0,
        "anim_run_speed": r["AnimRunSpeed"] or 0.0,
        "dye": [colour(r["PrimaryColor"]), colour(r["SecondaryColor"])],
        "summon": bare(r["SummonSequence"]),
        "death": bare(r["DeathSequence"]),
        # Its despawn once it revived its master (the phoenixes).
        "alt_death": bare(r["AlternativeDeathSequence"]),
        "idle_alt": bare(r["AlternativeIdleSequence"]),
        "permanent": bare(r["PermanentSequence"]),
        # Seconds from its removal until it is gone (PetProperty::RemovedPet).
        "despawn_s": r["DespawnTime"] or 0.0,
    }
    summon = sequences.get(pet["summon"])
    if summon is None or not any(t.get("kind") == "anim" for t in summon.get("tracks", [])):
        clip = anims.get(pet["anim_set"], {}).get("Spawn")
        if clip:
            pet["spawn_s"] = round(clip_seconds(pet["graphics"], clip), 3)
    pets[r["Id"]] = pet

missing = sorted({p for p in effects.values() if p not in pets})
os.makedirs(os.path.join(assets, "characters"), exist_ok=True)
with open(os.path.join(assets, "characters", "pets.json"), "w") as f:
    json.dump({"effects": effects, "pets": pets}, f, separators=(",", ":"))
unresolved = sorted(f"{k}: {p['set']}" for k, p in pets.items()
                    if p["set"] and (sets_cache.get(p["graphics"]) or {}).get(p["set"]) is None)
lost = sorted(f"{k}: {p[c]}" for k, p in pets.items() for c in ("summon", "death", "alt_death", "idle_alt", "permanent")
              if p[c] and sequences and p[c] not in sequences)
print({"pet effects": len(effects), "pets": len(pets), "unknown pets": missing,
       "spawn clips": sum(1 for p in pets.values() if p.get("spawn_s")),
       "skin lists from a skin": resolved, "no such skin list": unresolved, "no such sequence": lost})
