"""Lazy object thumbnails shared by the outliner and placement picker."""
from __future__ import annotations

from pathlib import Path
import math
import tkinter as tk
from tkinter import ttk

from level_data import GEOMETRY, atomic_json, digest, read_json
from level_assets import iter_models
from level_cameras import is_camera
from level_lighting import KINDS as LIGHTING_KINDS


class PreviewCache:
    def __init__(self, project):
        self.project = project
        self.directory = project.cache / "previews"
        self.directory.mkdir(parents=True, exist_ok=True)
        # A slow load or GPU readback can fail transiently. Retry once in the
        # next editor session, while avoiding a retry loop within this session.
        for marker in self.directory.glob("*.failed"):
            marker.unlink()
        try:
            self.index = read_json(self.directory / "index.json", {})
        except (OSError, ValueError):
            self.index = {}  # A damaged thumbnail index must not block editing.
        self.dirty = False

    def describe(self, kind, values, key=None):
        model = values.get("model", "") if kind == GEOMETRY else values.get("Graphics") or ""
        if not model:
            return None
        appearance = {field: values.get(field) for field in ("CharacterSet", "AnimSet", "StartAnimationState") if values.get(field)}
        identity = digest({"model": model, "appearance": appearance})
        reference = model.rsplit(":", 1)[-1]
        source = self.project.assets / (reference + ".glb")
        sources = [source]
        if reference in ("characters/uniskel", "characters/uniskel_dwarf"):
            sources = [self.project.assets / reference / name for name in ("outfits.json", "parts.json", "variations.json", "__animations.glb")]
        stamps = []
        for path in sources:
            try:
                stat = path.stat()
                stamps.append((str(path), stat.st_size, stat.st_mtime_ns))
            except FileNotFoundError:
                pass
        previous = self.index.get(identity, {}).get("file")
        cached = self.directory / previous if previous else None
        signature = digest({"renderer": 2, "sources": stamps})[:20]
        target = self.directory / (identity + "-" + signature + ".png")
        if not stamps and cached and cached.is_file():
            target = cached
        return {"identity": identity, "model": model, "key": "\t".join(key) if key else None,
                "path": str(target), "available": bool(stamps), "cached": cached,
                "appearance": appearance}

    def remember(self, item):
        value = {"file": Path(item["path"]).name, "model": item["model"], "appearance": item["appearance"]}
        if self.index.get(item["identity"]) != value:
            self.index[item["identity"]] = value
            self.dirty = True

    def flush(self):
        if self.dirty:
            atomic_json(self.directory / "index.json", self.index)
            self.dirty = False


class Previews:
    def __init__(self, editor):
        self.editor = editor
        self.cache = PreviewCache(editor.project)
        self.images = {}
        self.icons = {}
        self.browsers = []
        self.last_requests = None
        self.closed = False
        self.after = editor.window.after(250, self.poll)

    def icon(self, kind, size=32):
        if kind not in self.icons:
            image = tk.PhotoImage(master=self.editor.window, width=64, height=64)
            image.put("#111820", to=(0, 0, 64, 64))
            color = "#eac975" if kind == "_Instance_Light" else "#69d5d6" if kind == "_Instance_NPC" else "#718798"
            def line(a, b):
                steps = max(abs(a[0]-b[0]), abs(a[1]-b[1]), 1)
                for i in range(steps + 1):
                    x, y = (round(a[j] + (b[j]-a[j])*i/steps) for j in (0, 1))
                    image.put(color, to=(x, y, x+2, y+2))
            edges = [((12, 22), (32, 12)), ((32, 12), (52, 22)), ((52, 22), (32, 32)),
                         ((32, 32), (12, 22)), ((12, 22), (12, 43)), ((32, 32), (32, 53)),
                         ((52, 22), (52, 43)), ((12, 43), (32, 53)), ((32, 53), (52, 43))]
            if is_camera(kind):
                color = "#79baff"
                edges = [((10, 22), (38, 22)), ((38, 22), (38, 44)), ((38, 44), (10, 44)),
                         ((10, 44), (10, 22)), ((38, 29), (52, 21)), ((52, 21), (52, 45)),
                         ((52, 45), (38, 37)), ((19, 22), (19, 17)), ((19, 17), (29, 17)),
                         ((29, 17), (29, 22))]
            elif kind in LIGHTING_KINDS:
                color = '#eac975'
                circle = [(round(32+14*math.cos(i*math.tau/32)), round(27+14*math.sin(i*math.tau/32))) for i in range(33)]
                edges = list(zip(circle, circle[1:])) + [((25, 40), (25, 48)), ((25, 48), (39, 48)),
                                                       ((39, 48), (39, 40)), ((28, 53), (36, 53))]
                for axis in range(8):
                    angle = axis*math.tau/8
                    edges.append(((round(32+20*math.cos(angle)), round(27+20*math.sin(angle))),
                                  (round(32+25*math.cos(angle)), round(27+25*math.sin(angle)))))
            for a, b in edges:
                line(a, b)
            self.icons[kind] = image
        if size == 64:
            return self.icons[kind]
        key = (kind, size)
        if key not in self.icons:
            self.icons[key] = self.icons[kind].subsample(64 // size)
        return self.icons[key]

    def image(self, item, size=64):
        if not item:
            return None
        target = Path(item["path"])
        path = target if target.is_file() else item["cached"]
        if not path or not path.is_file():
            return None
        cache_key = (str(path), size)
        if cache_key not in self.images:
            try:
                source = tk.PhotoImage(master=self.editor.window, file=str(path))
                self.images[cache_key] = source.subsample(max(1, source.width() // size))
            except (tk.TclError, OSError):
                return None  # PNG readback may still be writing; retry next tick.
        if path == target:
            self.cache.remember(item)
        return self.images[cache_key]

    @staticmethod
    def visible(tree):
        if not tree.winfo_ismapped():
            return []
        return list(dict.fromkeys(iid for y in range(0, tree.winfo_height(), 20) if (iid := tree.identify_row(y))))

    def poll(self):
        if self.closed:
            return
        try:
            self.update()
        except (OSError, ValueError, tk.TclError):
            pass  # A rebuild, partially written PNG, or closed picker is retried.
        self.after = self.editor.window.after(250, self.poll)

    def update(self):
        editor = self.editor
        if self.cache.project is not editor.project:
            self.cache.flush()
            self.cache = PreviewCache(editor.project)
            self.last_requests = None
        requests = {}
        def request(kind, values, key=None, size=64):
            item = self.cache.describe(kind, values, key)
            if item and item["available"] and not Path(item["path"]).is_file() and not Path(item["path"]).with_suffix(".failed").exists():
                requests.setdefault(item["identity"], {k: item[k] for k in ("model", "key", "path")})
            return self.image(item, size)

        if editor.session and editor.selected:
            kind, ident = editor.selected
            values = editor.session.records.get(kind, {}).get(ident)
            if values:
                photo = request(kind, values, editor.selected, 128)
                caption = "Live lighting in viewport" if kind in LIGHTING_KINDS else "Live camera view in viewport" if is_camera(kind) else "Preview unavailable"
                editor.object_preview.configure(image=photo or self.icon(kind), text="" if photo else caption, compound="top")
        elif not editor.selected:
            editor.object_preview.configure(image="", text="Select an object to preview")
        if editor.session:
            for iid in self.visible(editor.objects):
                if int(iid) >= len(editor.rows):
                    continue
                kind, ident = editor.rows[int(iid)]
                values = editor.session.records[kind][ident]
                photo = request(kind, values, (kind, ident), 32)
                editor.objects.item(iid, image=photo or self.icon(kind))
        self.browsers = [browser for browser in self.browsers if browser.window.winfo_exists()]
        for browser in self.browsers:
            if not browser.window.winfo_ismapped():
                continue
            selected = browser.reference.get().strip()
            if selected:
                photo = request(GEOMETRY, {"model": selected}, size=getattr(browser, "preview_size", 256))
                browser.preview.configure(image=photo or self.icon(GEOMETRY), text="" if photo else "Preview unavailable / loading", compound="top")
            for iid in self.visible(browser.tree):
                photo = request(GEOMETRY, {"model": browser.rows[int(iid)]}, size=getattr(browser, "thumbnail_size", 64))
                browser.tree.item(iid, image=photo or self.icon(GEOMETRY))
        payload = list(requests.values())
        if editor.viewport and payload != self.last_requests:
            atomic_json(editor.viewport.directory / "previews.json", payload)
            self.last_requests = payload
        self.cache.flush()

    def close(self):
        self.closed = True
        self.editor.window.after_cancel(self.after)
        self.cache.flush()


class ModelPicker:
    """Choose an existing model visually, or type a reference for offline work."""
    def __init__(self, editor):
        self.editor = editor
        self.result = None
        self.window = tk.Toplevel(editor.window)
        self.window.title("Choose object / prefab")
        self.window.geometry("1000x700")
        self.window.minsize(760, 500)
        self.window.transient(editor.window)
        self.search = tk.StringVar()
        self.reference = tk.StringVar()
        self.rows = []
        names = {values["model"] for values in editor.session.records.get(GEOMETRY, {}).values()}
        names.update(row["model"] for row in editor.previews.cache.index.values() if not row.get("appearance"))
        names.update(iter_models(editor.project.assets))
        self.models = sorted(names)
        top = ttk.Frame(self.window, padding=12)
        top.pack(fill="x")
        ttk.Label(top, text="OBJECTS / PREFABS", style="Title.TLabel").pack(anchor="w")
        search = ttk.Entry(top, textvariable=self.search)
        search.pack(fill="x", pady=(10, 4))
        ttk.Label(top, text="Filter by model name or folder", style="Muted.TLabel").pack(anchor="w")
        body = ttk.Frame(self.window, padding=(12, 0))
        body.pack(fill="both", expand=True)
        preview_frame = ttk.Frame(body, width=280)
        preview_frame.pack(side="right", fill="y", padx=(12, 0))
        preview_frame.pack_propagate(False)
        self.preview = ttk.Label(preview_frame, text="Select a model", anchor="center")
        self.preview.pack(fill="x", pady=20)
        ttk.Label(preview_frame, textvariable=self.reference, wraplength=260).pack(fill="x")
        listing = ttk.Frame(body)
        listing.pack(fill="both", expand=True)
        self.tree = editor.tree(listing, ("name",), (420,))
        self.tree.configure(show="tree headings", style="Preview.Treeview", height=4)
        self.tree.column("#0", width=72, stretch=False, minwidth=72)
        self.tree.heading("#0", text="Preview")
        self.tree.heading("name", text="Model reference")
        self.tree.bind("<<TreeviewSelect>>", self.select)
        self.tree.bind("<Double-1>", lambda event: self.accept())
        self.tree.bind("<Return>", lambda event: self.accept())
        bottom = ttk.Frame(self.window, padding=12)
        bottom.pack(fill="x")
        ttk.Label(bottom, text="Model reference (can also be entered without the asset)", style="Muted.TLabel").pack(anchor="w")
        ttk.Entry(bottom, textvariable=self.reference).pack(fill="x", pady=6)
        ttk.Button(bottom, text="Place object", style="Accent.TButton", command=self.accept).pack(side="right")
        ttk.Button(bottom, text="Cancel", command=self.window.destroy).pack(side="right", padx=6)
        self.window.bind("<Escape>", lambda event: self.window.destroy())
        self.search.trace_add("write", lambda *args: self.refresh())
        self.refresh()
        editor.previews.browsers.append(self)
        search.focus_set()
        self.window.grab_set()

    def refresh(self):
        text = self.search.get().casefold().strip()
        self.rows = [model for model in self.models if text in model.casefold()]
        self.tree.delete(*self.tree.get_children())
        for index, model in enumerate(self.rows):
            self.tree.insert("", "end", iid=str(index), values=(model,), image=self.editor.previews.icon(GEOMETRY))

    def select(self, event=None):
        selected = self.tree.selection()
        if selected:
            self.reference.set(self.rows[int(selected[0])])

    def accept(self):
        reference = self.reference.get().strip()
        if reference:
            self.result = reference
            self.window.destroy()

    def choose(self):
        self.editor.window.wait_window(self.window)
        return self.result
