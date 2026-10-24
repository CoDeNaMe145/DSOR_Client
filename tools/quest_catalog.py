#!/usr/bin/env python3
"""The quests' tasks for the client's quest log (interface/quests.catalog.json).

Usage: quest_catalog.py <export_win32 | quests.pbxml | quests.xml> <assets>
                        [--database static.db4] [--godot <DSO_Godot>]

The quests come from the client's ``quests/quests.pbxml``: a KCAP pack of one
BXML document per quest (a ``<Quests>`` around one ``<Quest>``), decoded in
memory with DSO_Godot's dsoexp.bxml (hence --godot). Or from the server's
data/quests.xml (DSOR_Server, read by its gameplay/quest_content.py): the same
pack decoded by its pbxml_to_xml.py, every ``<Quest>`` under one ``<Quests>``,
a ``packEntry`` attribute where an entry's name is not the quest's id. The client
only needs what the server's QuestLogInfo (140) and QuestUpdate (139) leave out:
each quest's level, type, deletability and rewards (experience, gold in copper,
Andermant, the dynamicXP percent, the loot items with their class and rarity), whether
its tasks are staged, and its tasks in index order (their title is the text key
``quest.<quest>.s_<title>`` of the dialogue catalog, their type, their target count
and the indices of their preTasks). A task's target is the amount of the ``has``
guard on the token its own events produce, as the server counts it
(TaskDefinition.counter_guard); without one the task is a single step.
"""
import json
import os
import sqlite3
import struct
import sys
import xml.etree.ElementTree as ET


def read_quests(path, godot=None):
    """Every <Quest>, from quests.xml or the pack (export_win32 or the file itself)."""
    if os.path.isdir(path):
        path = os.path.join(path, "quests", "quests.pbxml")
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"KCAP":
        return ET.fromstring(data).findall("Quest")
    if godot:
        sys.path.insert(0, godot)
    try:
        from dsoexp import bxml
    except ImportError:
        sys.exit("quest_catalog.py: a .pbxml needs --godot <DSO_Godot> (dsoexp.bxml)")

    def element(node):
        e = ET.Element(node.tag, node.attrs)
        e.extend(map(element, node.children))
        return e

    # KCAP: version u32, then (u16 name length, name, u32 size, BXML document) each.
    quests, at = [], 8
    while at < len(data):
        n = struct.unpack_from("<H", data, at)[0]
        size = struct.unpack_from("<I", data, at + 2 + n)[0]
        at += 6 + n
        quests += [element(q) for q in bxml.parse(data[at:at + size]).children if q.tag == "Quest"]
        at += size
    return quests


def task_target(task):
    produced = {r.get("value", "") for event in task.findall("Event") for r in event.findall("Return")}
    guards = [g for g in task.findall("Guard") if g.get("type") == "has"]
    own = [g for g in guards if g.get("value", "") in produced]
    guard = own[0] if own else (guards[0] if guards else None)
    return max(1, int(guard.get("amount", 1))) if guard is not None else 1


def travel_items(database):
    """Travel items by template: the map they lead to (_Template_Item.TravelItemTarget)."""
    if not database:
        return {}
    con = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        return {row[0]: row[1] for row in con.execute(
            "SELECT Id, TravelItemTarget FROM _Template_Item WHERE TravelItemTarget IS NOT NULL AND TravelItemTarget != ''")}
    finally:
        con.close()


def flag(element, name):
    return (element.get(name, "false") or "false").lower() == "true"


# The quest types whose title shows its place in its series, " (n/m)": release261
# ServerQuestManager::LoadQuestsFromXML numbers them 4 to 11, pvp 9 excepted, and
# the title (0x1405AD574) adds the part for those alone.
NUMBERED = {"rowrepeatable", "local", "world", "heroic", "event", "crafting", "craftingrepeatable"}


def parts(quests):
    """Each numbered quest's (questNumber, the series' last number). The series:
    the quests its followups link it to within its questRowNumber.
    INFERRED: the client's total (QuestInfo +0x30) is computed after loading, not
    traced; this gives Full Moon Rising's "(1/5)" as a 2026 capture shows it."""
    by_id = {q.get("id", ""): q for q in quests}
    row = lambda q: (q.get("questRowNumber", ""), q.get("chapterId", ""), q.get("type", ""))
    links = {k: set() for k in by_id}
    for k, q in by_id.items():
        for f in (q.get("followup", "") or "").split(";"):
            if f in by_id and row(by_id[f]) == row(q):
                links[k].add(f)
                links[f].add(k)
    number = lambda k: int(by_id[k].get("questNumber", 0) or 0)
    result = {}
    for k, q in by_id.items():
        if q.get("type", "") not in NUMBERED or number(k) <= 0:
            continue
        seen, todo = {k}, [k]
        while todo:
            for other in links[todo.pop()] - seen:
                seen.add(other)
                todo.append(other)
        result[k] = [number(k), max(number(s) for s in seen)]
    return result


def export_quests(source, assets, database=None, godot=None):
    quests = {}
    every = read_quests(source, godot)
    numbered = parts(every)
    for quest in every:
        tasks = quest.findall("Task")
        index = {task.get("title", ""): n for n, task in enumerate(tasks)}
        rewards = quest.findall("Reward")
        quests[quest.get("id", "")] = {
            "level": int(quest.get("level", 1) or 1),
            "map": quest.get("map", ""),
            "chapter": quest.get("chapterId", ""),
            "deletable": flag(quest, "deletable"),
            "type": quest.get("type", ""),
            # A task with prerequisites shows once they are done (the NPC's quest
            # screen, UI::QuestWidget2 0x14021DEA8).
            "staged": flag(quest, "stagedTasks"),
            # Its place in its series, for its title; a quest not live (enabledLive
            # false) is titled "[INTERNAL] " (0x1405AD574).
            "part": numbered.get(quest.get("id", "")),
            "internal": not flag(quest, "enabledLive"),
            "rewards": {kind: sum(int(r.get("value", 0) or 0) for r in rewards if r.get("type") == kind)
                        for kind in ("xp", "vc", "rc")}
                       # The percent of the player's level the experience is (dynamicXP).
                       | {"dynamic_xp": sum(int(float(r.get("value", 0) or 0)) for r in rewards if r.get("type") == "dynamicXP")},
            # Loot rewards, per class when the quest names one, and their rarity.
            "items": [{"item": r.get("value", ""), "amount": int(r.get("amount", 1) or 1), "class": r.get("class", ""),
                       "rarity": r.get("rarity", "")}
                      for r in rewards if r.get("type") == "loot" and r.get("value")][:16],
            "tasks": [
                {"title": task.get("title", ""), "kind": task.get("tasktype", ""), "target": task_target(task),
                 "pre": [index[t] for t in (task.get("preTasks", "") or "").split(";") if t in index]}
                for task in tasks
            ],
        }
    out = os.path.join(assets, "interface", "quests.catalog.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8") as f:
        json.dump({"quests": quests, "travel_items": travel_items(database)}, f, ensure_ascii=False, separators=(",", ":"))
    return len(quests)


if __name__ == "__main__":
    args = sys.argv[1:]
    option = lambda name: args[args.index(name) + 1] if name in args else None
    print("quests:", export_quests(args[0], args[1], option("--database"), option("--godot")))
