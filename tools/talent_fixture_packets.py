"""Server packets that fill the Skills & Talents window for offline screenshots.

Usage: python tools/talent_fixture_packets.py <server> <out.json> [--fresh]

Writes [{frame, name, hex, bits}] for the client's DSOR_PACKET_REPLAY (offline
play only, a dwarf: `--play 3 0`): the character at level 100, Wisdom level
300, Group level 200, Honor level 43, and the picks of the user's reference
screenshots (2026-10-25: the Experience tab on preset I, Wisdom on preset II,
Group on preset I, Honor). Every point is spent, as there: 100 Experience
points (Covering Fire's 7 ranks are equipment bonus ranks, not bought), 2865
Wisdom, 2025 Group, 43 Honor. `--fresh`: the same levels and nothing picked
(every point free).

The packets come from the server's own encoders (protocol/player_init_protocol).
Nothing here touches a player database.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

server, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2])
fresh = "--fresh" in sys.argv
load_package(server)
from dso_server.protocol import player_init_protocol as init  # noqa: E402

records = []


def batch(name, command_id, value, frame=1):
    data, bits = init.encode_server_command_batch((init.ServerCommand(command_id, value, 1),), include_terminator=True)
    records.append(dict(frame=frame, name=name, hex=data.hex(), bits=bits))


def picks(ranks):
    return tuple(init.TalentSelectionEntry(rank, 0, talent) for talent, rank in ranks.items() if rank and not fresh)


SKILL = {
    "dwarf_HeavyShot_talent": 10, "dwarf_PositionBombShot_talent": 2, "dwarf_TurretCombat_talent": 10,
    "dwarf_TurretMachineGun_talent": 5, "dwarf_talent_skill_cat_03_dmg": 10, "dwarf_talent_skill_cat_04_cooldown": 10,
    "dwarf_talent_TurretMachineGun_explosion": 1, "dwarf_talent_fire_master": 10, "dwarf_talent_engineer_dwarf": 2,
}
WISDOM = {
    "trait_wisdom_all_hp": 60, "trait_wisdom_all_hpPerSecond": 60, "trait_wisdom_dwarf_resource": 30,
    "trait_wisdom_all_damage": 80, "trait_wisdom_all_critical": 80, "trait_wisdom_all_hpPerKill": 40,
    "trait_wisdom_all_block": 40, "trait_wisdom_all_armor": 80, "trait_wisdom_all_resistance": 80,
    "trait_wisdom_dwarf_towerInRange_Trigger": 1, "trait_wisdom_all_1hand_atk_speed": 30,
    "trait_wisdom_all_2hand_w_damage": 60, "trait_wisdom_all_2hand_atk_speed": 60, "trait_wisdom_all_2hand_kill_trigger": 30,
    "trait_wisdom_all_gold_find": 150, "trait_wisdom_all_dungeon_shop": 1, "trait_wisdom_all_dungeon_workbench": 1,
    "trait_wisdom_all_faster_home_teleport": 15, "trait_wisdom_all_faster_mount_cast": 15, "trait_wisdom_all_faster_hub_movspeed": 15,
}
GROUP = {
    "trait_groupaura_dwarf_lightning_resistance": 50, "trait_groupaura_dwarf_resource_restore": 4,
    "trait_groupaura_dwarf_cooldown_reduction": 21, "trait_groupaura_all_progression_wisdom": 5,
    "trait_groupaura_all_progression_shards": 5, "trait_groupaura_all_essentials_rc": 5,
    "trait_groupaura_all_essentials_realm_fragments": 5,
}
FAME = {
    "fame_focus_talent_01_honorbonus": 25, "fame_focus_talent_02_playerdmgbonus": 3,
    "fame_focus_talent_03_playerdmgresistence": 8, "fame_focus_talent_04_playerstundefense": 7,
}

# The roots: each Wisdom and Group category's ranks together (services/talents.py
# _talent_category_roots).
CATEGORY = {
    "trait_wisdom_base": ("trait_wisdom_all_hp", "trait_wisdom_all_hpPerSecond", "trait_wisdom_dwarf_resource"),
    "trait_wisdom_attack": ("trait_wisdom_all_damage", "trait_wisdom_all_critical", "trait_wisdom_all_hpPerKill"),
    "trait_wisdom_defense": ("trait_wisdom_all_block", "trait_wisdom_all_armor", "trait_wisdom_all_resistance"),
    "trait_wisdom_combat": ("trait_wisdom_dwarf_towerInRange_Trigger",),
    "trait_wisdom_oneHand": ("trait_wisdom_all_1hand_atk_speed",),
    "trait_wisdom_twoHand": ("trait_wisdom_all_2hand_w_damage", "trait_wisdom_all_2hand_atk_speed", "trait_wisdom_all_2hand_kill_trigger"),
    "trait_wisdom_economy": ("trait_wisdom_all_gold_find", "trait_wisdom_all_dungeon_shop", "trait_wisdom_all_dungeon_workbench"),
    "trait_wisdom_ability": ("trait_wisdom_all_faster_home_teleport", "trait_wisdom_all_faster_mount_cast", "trait_wisdom_all_faster_hub_movspeed"),
    "trait_group_aura": ("trait_groupaura_dwarf_lightning_resistance", "trait_groupaura_dwarf_resource_restore", "trait_groupaura_dwarf_cooldown_reduction"),
    "trait_group_progression": ("trait_groupaura_all_progression_wisdom", "trait_groupaura_all_progression_shards"),
    "trait_group_essentials": ("trait_groupaura_all_essentials_rc", "trait_groupaura_all_essentials_realm_fragments"),
}


def roots(ranks):
    if fresh:
        return ()
    return tuple(init.TalentSelectionEntry(sum(ranks.get(t, 0) for t in talents), 0, category)
                 for category, talents in CATEGORY.items() if any(t in ranks for t in talents))


# Level 100 (the XP table's top), the trees' progression: Wisdom's and Group's
# highest levels, Honor level 43 and about half of its way to 44
# (_Template_TalentXPLevel: Honor level 43 from 3437525 XP, 44 from 4067525).
batch("xp", init.XP_CHANGED_COMMAND, init.XPChanged(100_000_000, 100, 99_000_000, 100_000_000, False))
batch("talent_points", init.TALENT_CATEGORY_POINTS_INFO_COMMAND,
      init.TalentCategoryPointsInfo(0xFF, (100_000_000, 3_622_960, 1_924_643, 3_750_000)))
# 50000 gold and 50000 Andermant: a reset or a preset can be paid.
batch("wallet", init.CURRENCY_CHANGED_COMMAND, init.CurrencyChanged(50_000, 500_000_000, (0, 0, 0), 0, False))
batch("unlocked_presets", init.UNLOCK_SKILL_TALENT_PRESELECTION_INFO_COMMAND, init.UnlockSkillTalentPreselectionInfo(0))
batch("talents", init.TALENT_SELECTION_INFO_COMMAND, init.TalentSelectionInfo(0, (
    init.TalentCategorySelection(0, picks(SKILL), ()),
    init.TalentCategorySelection(1, picks(WISDOM), roots(WISDOM)),
    init.TalentCategorySelection(0, picks(GROUP), roots(GROUP)),
    init.TalentCategorySelection(0, picks(FAME), ()),
)))
out.write_text(json.dumps(records, indent=1), encoding="utf-8")
print(f"{len(records)} packets -> {out}")
