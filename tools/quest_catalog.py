#!/usr/bin/env python3
"""The quests' tasks for the client's quest log (interface/quests.catalog.json).

Usage: quest_catalog.py <quests.xml> <assets> [--database static.db4]

``quests.xml`` is the server's decoded ``quests.pbxml`` (DSOR_Server
data/quests.xml, the same file its gameplay/quest_content.py reads). The client
only needs what the server's QuestLogInfo (140) and QuestUpdate (139) leave out:
each quest's level, type, deletability and rewards (experience, gold in copper,
Andermant, the loot items the log shows), and its tasks in index order (their title
is the text key ``quest.<quest>.s_<title>`` of the dialogue catalog, their type
and their target count). A task's target is the amount of the ``has`` guard on
the token its own events produce, as the server counts it
(TaskDefinition.counter_guard); without one the task is a single step.
"""
import json
import os
import sqlite3
import sys
import xml.etree.ElementTree as ET


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


def export_quests(xml_path, assets, database=None):
    quests = {}
    for quest in ET.parse(xml_path).getroot().findall("Quest"):
        quests[quest.get("id", "")] = {
            "level": int(quest.get("level", 1) or 1),
            "map": quest.get("map", ""),
            "chapter": quest.get("chapterId", ""),
            "deletable": (quest.get("deletable", "false") or "false").lower() == "true",
            "type": quest.get("type", ""),
            "rewards": {kind: sum(int(r.get("value", 0) or 0) for r in quest.findall("Reward") if r.get("type") == kind)
                        for kind in ("xp", "vc", "rc")},
            # Loot rewards, per class when the quest names one.
            "items": [{"item": r.get("value", ""), "amount": int(r.get("amount", 1) or 1), "class": r.get("class", "")}
                      for r in quest.findall("Reward") if r.get("type") == "loot" and r.get("value")][:16],
            "tasks": [
                {"title": task.get("title", ""), "kind": task.get("tasktype", ""), "target": task_target(task)}
                for task in quest.findall("Task")
            ],
        }
    out = os.path.join(assets, "interface", "quests.catalog.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8") as f:
        json.dump({"quests": quests, "travel_items": travel_items(database)}, f, ensure_ascii=False, separators=(",", ":"))
    return len(quests)


if __name__ == "__main__":
    args = sys.argv[1:]
    database = args[args.index("--database") + 1] if "--database" in args else None
    print("quests:", export_quests(args[0], args[1], database))
