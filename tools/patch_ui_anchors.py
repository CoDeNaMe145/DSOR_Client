#!/usr/bin/env python3
"""Copy the anchors (k) of freshly exported layouts into the current ones.

Usage: patch_ui_anchors.py <fresh assets> <assets>

tools/export_ui.py decides each widget's k; a full re-export also rewrites the
other fields from the exporter's current rules. This copies only k, widget by
widget (same id), so an anchoring fix reaches every window without touching
anything else in them.
"""
import json, os, sys

fresh, assets = sys.argv[1], sys.argv[2]
changed = {}
for name in sorted(os.listdir(os.path.join(fresh, "interface"))):
    if not name.endswith(".ui.json"):
        continue
    target = os.path.join(assets, "interface", name)
    if not os.path.isfile(target):
        continue
    new = {w["id"]: w["k"] for w in json.load(open(os.path.join(fresh, "interface", name), encoding="utf-8"))["widgets"]}
    layout = json.load(open(target, encoding="utf-8"))
    count = 0
    for w in layout["widgets"]:
        k = new.get(w["id"])
        if k is not None and k != w["k"]:
            w["k"] = k
            count += 1
    if count:
        with open(target, "w", encoding="utf-8") as f:
            json.dump(layout, f, ensure_ascii=False, separators=(",", ":"))
        changed[name[:-8]] = count
print("anchors patched:", changed)
