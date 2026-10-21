"""Server packets that fill the systems' windows for offline screenshots.

Usage: python tools/ui_fixture_packets.py <server> <assets> <out.json> [<static.db4>]

Writes [{frame, name, hex, bits}] for the client's DSOR_PACKET_REPLAY (offline
play only): a wallet, a quest log, achievements, daily challenges, the Wanted
board, running events, the premium offers, leaderboards, the album, the recipe
books and an inventory with socketed equipment, gems and ingredients. Every
packet comes from the local server's own encoders (as tools/*_vectors.py); the
ids come from the client's catalogues (interface/*.json), so the windows show
real texts and icons. Nothing here touches a player database.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

server, assets, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2]), Path(sys.argv[3])
load_package(server)
from dso_server.protocol import player_init_protocol as init
from dso_server.protocol import gameplay_protocol as gp
from dso_server.protocol import ranking_protocol as rk
from dso_server.protocol import wanted_thief_protocol as wt
from dso_server.protocol import pvp_match_protocol as pvp
from dso_server.protocol import inventory_codec as inv
from dso_server.protocol.quest_protocol import quest_log_entry
from dso_server.protocol.reward_protocol import LOCAL_OCP_OFFERS

interface = assets / "interface"
systems = json.loads((interface / "systems.catalog.json").read_text(encoding="utf-8"))
quests = json.loads((interface / "quests.catalog.json").read_text(encoding="utf-8"))["quests"]
items = json.loads((interface / "inventory.items.json").read_text(encoding="utf-8"))["items"]

records = []
def record(name, packet, frame=1):
    data, bits = packet
    records.append(dict(frame=frame, name=name, hex=data.hex(), bits=bits))

def batch(name, command_id, value, frame=1):
    record(name, init.encode_server_command_batch((init.ServerCommand(command_id, value, 1),), include_terminator=True), frame)

# The wallet: Andermant, gold (copper), honour, draken.
batch("wallet", init.CURRENCY_CHANGED_COMMAND, init.CurrencyChanged(1250, 4_573_210, (870, 0, 35), 0, False))

# Three quests with tasks, the second ready to turn in.
chosen = [(k, v) for k, v in sorted(quests.items()) if len(v.get("tasks", [])) >= 2 and v.get("level", 0) >= 5][:3]
entries = []
for n, (quest, data) in enumerate(chosen):
    counters = [0] * 16
    counters[0] = 1
    counters[1] = min(3, data["tasks"][1].get("target", 1))
    entries.append(quest_log_entry(quest, 1 if n == 1 else 0, 0b1 if n != 1 else 0b11, tuple(counters), ""))
batch("quest_log", init.QUEST_LOG_INFO_COMMAND, init.QuestLogInfo(tuple(entries)))

# Achievements of the first category: a few under way, a few done (two with titles).
categories = sorted(systems["achievement_categories"], key=lambda c: (c["priority"], c["id"]))
first = next(c["id"] for c in categories if not c["parent"])
subs = {c["id"] for c in categories if c["parent"] == first} | {first}
mine = sorted(k for k, a in systems["achievements"].items() if a["category"] in subs)[:8]
titled = sorted(k for k, a in systems["achievements"].items() if a["title_text"])[:2]
done = (0, 2026, 18, 21, 4, 9, 0, 7, 10)
values = []
for n, key in enumerate(mine):
    amount = systems["achievements"][key]["amount"]
    values.append(init.Achievement(key, (amount,) + done[1:] if n % 3 == 0 else (max(0, amount // 2), 0, 0, 0, 0, 0, 0, 0, 0)))
values += [init.Achievement(key, (systems["achievements"][key]["amount"],) + done[1:]) for key in titled]
batch("achievements", init.ACHIEVEMENT_INFO_COMMAND, init.AchievementInfo(tuple(values)))

# Four daily challenges: available, accepted, completed, available.
dailies = sorted(k for k, c in systems["challenges"].items() if c["category"] not in ("BonusReward", "Spender") and c["value"] > 0)[:4]
stamp = (2026, 10, 21, 0, 0, 0)
challenges = tuple(init.TimedChallenge(k, k, state, progress, stamp)
                   for k, state, progress in zip(dailies, (0, 2, 3, 0), (0, 1, systems["challenges"][dailies[2]]["value"], 0)))
# The bonuses for the dailies solved: the first reached.
bonus_ids = sorted((k for k, c in systems["challenges"].items() if c["category"] == "BonusReward"), key=lambda k: systems["challenges"][k]["value"])
bonuses = tuple(init.TimedChallenge(k, k, 3 if n == 0 else 2, 1, stamp) for n, k in enumerate(bonus_ids))
batch("challenges", init.TIMED_CHALLENGES_INFO_COMMAND, init.TimedChallengesInfo(0, challenges, bonuses, ()))

# The Wanted board: four missions, the second accepted.
import datetime
record("wanted", wt.encode_wanted_thief_info(wt.WantedThiefInfo(2, 1, (1, 2, 3, 4), (0, 1, 0, 0), None), actor_id=1))

# Three running events with their progress.
events = [k for k in ("halloween_event_01", "christmas_event_01", "wanted_thief_event") if k in systems["events"]]
events += [k for k in sorted(systems["events"]) if systems["events"][k]["teleport"]][:3 - len(events)]
update = tuple(init.EventUpdateEntry(100 + n, key, (True, False, False, False), (2026, 10, 1, 0, 0, 0), (2026, 11, 2, 12, 0, 0), ())
               for n, key in enumerate(events))
record("events", init.encode_event_update(init.EventUpdate(update)))
batch("event_progress", init.EVENT_PROGRESS_INFO_COMMAND, init.EventProgressInfo((init.EventProgressInfoEntry(100, (340, 1000, 0, 0)),)))

# The premium shop's offers.
record("ocp", gp.encode_ocp_offer_batch(gp.OCPOfferBatch(tuple(LOCAL_OCP_OFFERS), 1, (), 1)))

# The leaderboards: ten rows of the overall and of the honour board, our rank.
names = ["Aldric", "Brenna", "Corwin", "Dagny", "Eskil", "Freya", "Gunnar", "Hilde", "Ivar", "Jorun"]
rows = tuple(rk.RankingRow(name, 5000 - 371 * n, n, 40 - n, n, n % 3, 120 - 7 * n, 30 + n) for n, name in enumerate(names))
request = rk.RankingRequest(command_id=rk.HONOR_OVERALL_USER_RANKING_INFO_COMMAND, start=0, count=10, context=0, flags=1, class_id=4, board=0xFF, sort_column=0)
record("overall_board", rk.encode_ranking_page(request, rows, start=0, total=37))
request = rk.RankingRequest(command_id=rk.HONOR_RANKING_INFO_COMMAND, start=0, count=10, context=0, flags=1, class_id=4, board=4, sort_column=0)
record("honor_board", rk.encode_ranking_page(request, rows, start=0, total=37))
request = rk.RankingRequest(command_id=rk.HONOR_LOCAL_USER_RANKING_INFO_COMMAND, start=0xFFFFFFFF, count=0, context=0, flags=0, class_id=4, board=4, sort_column=0)
record("own_rank", rk.encode_local_ranking(request, rows[6]))

# The album: stickers of each page owned, two favourites.
books = {b["id"]: b["category"] for b in systems["stickerbooks"]}
owned = []
for page in range(1, 8):
    owned += sorted(k for k, s in systems["stickers"].items() if books.get(s["book"]) == page)[:5]
stickers = tuple(init.StickerbookEntry(3 if n < 2 else 2, 0, key) for n, key in enumerate(owned))
batch("album", init.STICKERBOOK_INFO_COMMAND, init.StickerbookInfo(2, stickers))

# Recipe books known: every book with recipes whose items are all known.
# An ingredient is "template[:amount]".
template = lambda value: value.split(":")[0]
known = sorted({r["book"] for r in systems["recipes"].values() if r["crafted"] in items and all(template(i) in items for i in r["ingredients"])})[:40]
batch("recipes", init.RECIPE_BOOK_INFO_COMMAND, init.RecipeBookInfo(tuple(init.RecipeBookEntry(b, None) for b in known)))

# An inventory: a worn helmet with three sockets (one gem), carried gems, a
# second helmet, a sword and ingredients of a known recipe.
def item(item_id, template, **fields):
    return inv.ItemInfo(item_id, template_id=template, **fields)
# The offline character is a mage (--play 1 0).
mage = lambda d: d.get("class_name", "") in ("", "mage", "all")
helmet = next(k for k, d in sorted(items.items()) if d.get("slots") == [0] and d.get("item_category") == "Armor" and mage(d) and d.get("required_level", 0) >= 10)
sword = next(k for k, d in sorted(items.items()) if d.get("slots") == [5] and d.get("item_category") == "Weapon" and mage(d))
gems = [g for g in ("gem_ruby_b", "gem_amethyst_b", "gem_onyx_c", "gem_cyanit_a") if g in items]
recipe = next(r for r in systems["recipes"].values() if r["book"] in known)
bag = [item(2001, helmet, field_08=3, socket_count=3, socketed_templates=[gems[0], "", ""]),
       item(2002, helmet, field_08=2), item(2003, sword, field_08=4)]
bag += [item(2010 + n, g, amount=3) for n, g in enumerate(gems[1:])]
bag += [item(2020 + n, template(t), amount=int(t.split(":")[1]) if ":" in t else 1) for n, t in enumerate(recipe["ingredients"])]
worn = item(2000, helmet, field_08=4, socket_count=2, socketed_templates=[gems[1], ""])
# A worn sword too (the transmog desk's right hand).
wielded = item(1999, sword, field_08=3)
bag = [b for b in bag if b.item_id != 2003]
info = inv.InventoryInfo(items=[worn, wielded] + bag, equipment_slots={2000: (0,), 1999: (5,)}, storage_58={it.item_id: n for n, it in enumerate(bag)})
record("inventory", inv.encode_inventory_info(info))

# The Andermant item shop's offers (as the server loads them for a level 40 mage).
from dso_server.protocol import shop_protocol as shops
static = Path(sys.argv[4]) if len(sys.argv) > 4 else None
if static and static.is_file():
    offers = shops.load_shop_offers(static, "global_rc_trader", player_level=40, class_id=1)
    record("item_shop", shops.encode_offer_response_batch(offers, timed_offers_actor_id=1))

# Queued for PvP.
record("queue", pvp.encode_matchmaking_info(pvp.MatchMakingInfo(0, 100, 1, "")))

# The social window: a group (us, the offline "Debug" mage, its leader; one member
# on another map, one offline), a guild, and the players a search found (more
# than the List's 19 rows: it scrolls).
from dso_server.protocol import social_protocol as sp
summary = lambda cid, name, where, cls, level, **k: sp.PlayerSummary(0, 0, cid, 0, name, where, cls, level, **k)
group = (summary(1, "Debug", "a0200_kingscity", 1, 40, role=0), summary(2, "Brenna", "a0302_wildforest", 2, 37, role=1),
         summary(3, "Corwin", "a0200_kingscity", 0, 41, online=False, available=False, group_invitable=False, role=1))
record("group", sp.encode_group_status(sp.GroupStatus(0, 7, 1, 1, False, group, "Debug's group", False, "")))
stamp = (2026, 10, 1, 12, 0, 0)
guild = tuple(sp.GuildMember(cid, 0, name, cls, level, online, online, False, rank, stamp)
              for cid, name, cls, level, online, rank in [(1, "Debug", 1, 40, True, 2), (2, "Brenna", 2, 37, True, 1), (4, "Dagny", 3, 22, True, 0),
                                                          (5, "Eskil", 0, 55, False, 0), (6, "Freya", 1, 18, False, 0), (7, "Gunnar", 3, 61, True, 1)])
record("guild", sp.encode_guild_status(sp.GuildStatus(0, 3, "Knights of Duria", 1, True, 1, len(guild), guild)))
found = [("Brenna", 2, 37), ("Hilde", 0, 12), ("Ivar", 3, 44), ("Jorun", 1, 29), ("Kari", 2, 8), ("Leif", 0, 52), ("Maren", 1, 33),
         ("Njal", 3, 19), ("Oda", 2, 47), ("Pelle", 0, 25), ("Ragna", 1, 60), ("Sigrun", 3, 14), ("Torvald", 0, 38), ("Ulla", 2, 21),
         ("Vidar", 1, 49), ("Yrsa", 3, 31), ("Asger", 0, 5), ("Bodil", 2, 57), ("Dag", 1, 16), ("Embla", 3, 42), ("Fenja", 0, 27), ("Geir", 2, 35)]
record("player_search", sp.encode_player_query_result(1, tuple(summary(100 + n, name, "", cls, level) for n, (name, cls, level) in enumerate(found))))

out.write_text(json.dumps(records, indent=1) + "\n", encoding="utf-8")
print(f"wrote {len(records)} fixture packets to {out}: quests {[q for q, _ in chosen]}, events {events}, helmet {helmet}, gems {gems}, recipe book {recipe['book']}")
