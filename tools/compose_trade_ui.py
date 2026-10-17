"""Compose the player trade window from the game's own interface art.

The game has no trade between players, so it has no window for it.  This
builds ``interface/tradewindow.ui.json`` (the format tools/export_ui.py
writes) from exported windows:

* unstackwindow ("Split Items"): the frame, its title, the close button, the
  blue button (as "Accept") and the number entry (as the gold entry), the
  frame stretched to hold two offers;
* inventory: a bag cell (TemplateSlot#0_0) for each item place.

Widget ids the client looks for (crate::ui::controls::trade):
``Canvas/Mine/Slot<N>`` and ``Canvas/Theirs/Slot<N>`` (SlotButton, events
``trade_mine:<N>`` / ``trade_theirs:<N>``), ``Canvas/Gold/InputName`` (the gold
entry), ``Canvas/TheirGold`` (their gold), ``Canvas/MineTitle``,
``Canvas/TheirsTitle``, ``Canvas/Status`` (runtime texts), the Accept button
(event ``trade_ready``) and the close button (event ``Close``).

Usage: python3 tools/compose_trade_ui.py <assets>
"""

import copy
import json
import sys
from pathlib import Path

COLUMNS, ROWS = 4, 3
SLOT = 58.0


def load(interface: Path, name: str) -> dict:
    return json.loads((interface / f"{name}.ui.json").read_text(encoding="utf-8"))


def subtree(layout: dict, root: str, skip=()) -> list:
    return [copy.deepcopy(w) for w in layout["widgets"]
            if (w["id"] == root or w["id"].startswith(root + "/"))
            and not any(w["id"] == s or w["id"].startswith(s + "/") for s in skip)]


def move(widgets: list, dx: float, dy: float) -> list:
    for w in widgets:
        l, t, r, b = w["rect"]
        w["rect"] = [l + dx, t + dy, r + dx, b + dy]
        if w.get("art"):
            pos = w["art"]["pos"]
            for i in range(0, len(pos), 2):
                pos[i] += dx
                pos[i + 1] += dy
    return widgets


def stretch(widgets: list, split_x: float, dx: float, split_y: float, dy: float) -> list:
    """Move every point right of split_x by dx and below split_y by dy: a
    nine-sliced frame grows by its middle, its corners keep their size."""
    def sx(x):
        return x + dx if x > split_x else x

    def sy(y):
        return y + dy if y > split_y else y
    for w in widgets:
        l, t, r, b = w["rect"]
        w["rect"] = [sx(l), sy(t), sx(r), sy(b)]
        if w.get("art"):
            pos = w["art"]["pos"]
            for i in range(0, len(pos), 2):
                pos[i] = sx(pos[i])
                pos[i + 1] = sy(pos[i + 1])
    return widgets


def rename(widgets: list, old: str, new: str) -> list:
    for w in widgets:
        w["id"] = new + w["id"][len(old):]
        if w.get("button"):
            w["button"] = new + w["button"][len(old):]
    return widgets


def text(widget_id: str, rect, value: str, key: str, size=10.0, align=(0, 1), color=(0.92, 0.87, 0.76, 1.0)) -> dict:
    return {"id": widget_id, "type": "TextLabel", "k": 0.5, "rect": list(rect), "hidden": False, "click_through": True,
            "text": {"text": value, "key": key, "size": size, "color": list(color), "align": list(align)}}


def main(assets: str) -> None:
    interface = Path(assets) / "interface"
    split = load(interface, "unstackwindow")
    inventory = load(interface, "inventory")

    # The frame, grown by its middle: 352 x 163 -> 572 x 405, then centred.
    base = "Canvas/Label_blur"
    frame = subtree(split, base, skip=(
        f"{base}/Label_background/SplitButton", f"{base}/Label_background/InputValueBackground",
        f"{base}/Label_background/DifficultySlider", f"{base}/Label_background/ButtonRight",
        f"{base}/Label_background/ButtonLeft"))
    canvas = copy.deepcopy(split["widgets"][0])
    grow_x, grow_y = 220.0, 242.0
    stretch([canvas] + frame, 820.0, grow_x, 470.0, grow_y)
    l, t, r, b = canvas["rect"]
    # Left of the inventory (inventory.ui.json's bag starts at x 593), which opens
    # with it.
    dx, dy = 20.0 - l, 384.0 - (t + b) / 2
    move([canvas] + frame, dx, dy)
    left, top = canvas["rect"][0], canvas["rect"][1]
    for w in frame:
        # The title bar's own label is a runtime value (it showed the name).
        if w["id"].endswith("/TextLabel_title"):
            w.pop("text", None)
        if w["id"].endswith("/PackageDescriptionTitle") and w.get("text"):
            w["text"]["text"], w["text"]["key"] = "Trade", "gui.window.trade_title"
    widgets = [canvas] + frame

    # Two offers of COLUMNS x ROWS bag cells.
    cell = subtree(inventory, "Canvas/PositioningLabel/Label_texture/unten/Label_bg/icon_area/TemplateSlot#0_0")
    cell_root = cell[0]["id"]
    cl, ct = cell[0]["rect"][0], cell[0]["rect"][1]
    grid_top = top + 82.0
    for side, x0 in (("Mine", left + 38.0), ("Theirs", left + 38.0 + COLUMNS * SLOT + 30.0)):
        widgets.append(text(f"Canvas/{side}Title", (x0, grid_top - 26.0, x0 + COLUMNS * SLOT, grid_top - 6.0),
                            "", "", size=11.0))
        for n in range(COLUMNS * ROWS):
            x, y = x0 + (n % COLUMNS) * SLOT, grid_top + (n // COLUMNS) * SLOT
            slot = move(rename(copy.deepcopy(cell), cell_root, f"Canvas/{side}/Slot{n}"), x - cl, y - ct)
            slot[0]["type"] = "SlotButton"
            slot[0]["event"] = f"trade_{side.lower()}:{n}"
            slot[0]["click_through"] = False
            widgets.extend(slot)
    mine_x = left + 38.0
    theirs_x = mine_x + COLUMNS * SLOT + 30.0
    gold_top = grid_top + ROWS * SLOT + 12.0

    # Gold: my entry (the split window's number entry), their amount.
    entry = subtree(split, f"{base}/Label_background/InputValueBackground", skip=(
        f"{base}/Label_background/InputValueBackground/TextLabel_Stack",))
    entry_root = entry[0]["id"]
    el, et, er, eb = entry[0]["rect"]
    stretch(entry, (el + er) / 2, 60.0, 1e9, 0.0)
    move(rename(entry, entry_root, "Canvas/Gold"), mine_x + 60.0 - el, gold_top - et)
    widgets.append(text("Canvas/GoldLabel", (mine_x, gold_top + 2.0, mine_x + 56.0, gold_top + 24.0), "Gold", "gui.window.trade_gold"))
    widgets.extend(entry)
    widgets.append(text("Canvas/TheirGoldLabel", (theirs_x, gold_top + 2.0, theirs_x + 56.0, gold_top + 24.0), "Gold", "gui.window.trade_gold"))
    widgets.append(text("Canvas/TheirGold", (theirs_x + 60.0, gold_top + 2.0, theirs_x + 180.0, gold_top + 24.0), "", ""))

    # Accept: the split window's blue button, at the bottom right.
    button = subtree(split, f"{base}/Label_background/SplitButton")
    button_root = button[0]["id"]
    bl, bt, br, bb = button[0]["rect"]
    stretch(button, (bl + br) / 2, 30.0, 1e9, 0.0)
    width = br + 30.0 - bl
    move(rename(button, button_root, "Canvas/Accept"), canvas["rect"][2] - 40.0 - width - bl, canvas["rect"][3] - 52.0 - bt)
    button[0]["event"] = "trade_ready"
    for w in button:
        if w.get("text"):
            w["text"]["text"], w["text"]["key"] = "Accept", "gui.window.trade_accept"
            l2, t2, r2, b2 = w["rect"]
            w["rect"] = [l2 - 15.0, t2, r2 + 15.0, b2]
    widgets.extend(button)
    status_y = canvas["rect"][3] - 50.0
    widgets.append(text("Canvas/Status", (mine_x, status_y, mine_x + 300.0, status_y + 24.0), "", ""))

    out = {"window": "tradewindow", "base": split["base"], "widgets": widgets}
    (interface / "tradewindow.ui.json").write_text(json.dumps(out, separators=(",", ":")), encoding="utf-8")
    print(f"tradewindow: {len(widgets)} widgets, frame {[round(v) for v in canvas['rect']]}")


if __name__ == "__main__":
    main(sys.argv[1])
