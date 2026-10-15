"""A project asset library, independent of the current scene's placements."""
from pathlib import Path
import math
import tkinter as tk
from tkinter import ttk

from level_data import GEOMETRY


def iter_models(folder):
    """Placeable models use the same relative references as map manifests."""
    folder = Path(folder)
    for path in folder.rglob("*.glb"):
        if path.is_file() and not path.name.startswith("__"):
            yield path.relative_to(folder).as_posix()[:-4]


def placement_position(status, fallback, snap=0):
    """Place within the current view, on the ground when it is nearby."""
    result = list(fallback)
    eye, direction = status.get("camera_position"), status.get("camera_forward")
    if (isinstance(eye, list) and isinstance(direction, list) and len(eye) == len(direction) == 3
            and all(isinstance(v, (int, float)) and math.isfinite(v) for v in eye + direction)):
        distance = 10.0
        if direction[1] < -0.001:
            ground = (fallback[1] - eye[1]) / direction[1]
            if 0 < ground <= 1000:
                distance = ground
        result = [p + d * distance for p, d in zip(eye, direction)]
    if snap > 0 and math.isfinite(snap):
        result = [round(v / snap) * snap for v in result]
    return result


class AssetBrowser:
    """Docked library with its own search, folder filter and model preview."""
    preview_size = 128
    thumbnail_size = 32

    def __init__(self, editor, parent):
        self.editor = editor
        self.window = parent
        self.search = tk.StringVar()
        self.folder = tk.StringVar(value="All folders")
        self.reference = tk.StringVar()
        self.models, self.rows = [], []
        self.source = None
        self.scan_job = self.filter_job = self.render_job = None
        self.scanning = False
        toolbar = ttk.Frame(parent, padding=(10, 10, 10, 6))
        toolbar.pack(fill="x")
        ttk.Label(toolbar, text="Project assets", font=("Segoe UI", 11, "bold")).pack(side="left")
        self.reload_button = ttk.Button(toolbar, text="Refresh", style="Tool.TButton", command=self.reload)
        self.reload_button.pack(side="right")
        filters = ttk.Frame(parent, padding=(10, 0, 10, 8))
        filters.pack(fill="x")
        ttk.Entry(filters, textvariable=self.search).pack(fill="x", pady=(0, 5))
        ttk.Label(filters, text="Search all asset names and folders", style="Muted.TLabel", font=("Segoe UI", 9)).pack(anchor="w", pady=(0, 7))
        self.folder_box = ttk.Combobox(filters, textvariable=self.folder, state="readonly", values=["All folders"])
        self.folder_box.pack(fill="x")
        self.folder_box.bind("<<ComboboxSelected>>", lambda event: self.refresh())
        # Pack the footer first so placement controls stay visible on smaller screens.
        footer = ttk.Frame(parent, padding=10)
        footer.pack(side="bottom", fill="x")
        preview_frame = ttk.Frame(footer, height=136)
        preview_frame.pack(fill="x")
        preview_frame.pack_propagate(False)
        self.preview = ttk.Label(preview_frame, text="Select an asset to preview", anchor="center", compound="top")
        self.preview.pack(fill="both", expand=True)
        ttk.Label(footer, textvariable=self.reference, style="Muted.TLabel", wraplength=280).pack(fill="x", pady=6)
        self.place_button = ttk.Button(footer, text="+  Add to level", style="Accent.TButton", state="disabled",
                                       command=lambda: editor.action(self.place))
        self.place_button.pack(fill="x")
        ttk.Label(footer, text="Places in view · double-click to add", style="Muted.TLabel", font=("Segoe UI", 9)).pack(anchor="w", pady=(6, 0))
        self.count = ttk.Label(parent, text="Open this tab to browse assets", style="Muted.TLabel", padding=(10, 6), wraplength=290)
        self.count.pack(side="bottom", fill="x")
        listing = ttk.Frame(parent)
        listing.pack(fill="both", expand=True)
        self.tree = editor.tree(listing, ("name",), (220,))
        self.tree.configure(show="tree headings", style="Outliner.Treeview", height=3)
        self.tree.column("#0", width=44, minwidth=44, stretch=False)
        self.tree.heading("#0", text="")
        self.tree.heading("name", text="Asset")
        self.tree.bind("<<TreeviewSelect>>", self.select)
        self.tree.bind("<Double-1>", lambda event: editor.action(self.place))
        self.tree.bind("<Return>", lambda event: editor.action(self.place))
        self.search.trace_add("write", self.filter_later)
        parent.bind("<Map>", lambda event: self.ensure_source())

    def cancel(self, name):
        job = getattr(self, name)
        if job is not None:
            self.window.after_cancel(job)
            setattr(self, name, None)

    def ensure_source(self):
        if self.source != self.editor.project.assets:
            self.reload()
        self.update_placement()

    def reload(self):
        for name in ("scan_job", "filter_job", "render_job"):
            self.cancel(name)
        self.source = self.editor.project.assets
        self.models, self.rows = [], []
        self.tree.delete(*self.tree.get_children())
        self.reference.set("")
        self.preview.configure(image="", text="Select an asset to preview")
        self.place_button.configure(state="disabled")
        self.scanning = True
        self.reload_button.configure(state="disabled")
        self.count.configure(text="Scanning assets…")
        self.scan = iter_models(self.source)
        self.scan_job = self.window.after(1, self.scan_step)

    def scan_step(self):
        self.scan_job = None
        try:
            for _ in range(400):
                self.models.append(next(self.scan))
        except StopIteration:
            self.models.sort(key=str.casefold)
            self.scanning = False
            self.reload_button.configure(state="normal")
            folders = sorted({model.rpartition("/")[0] for model in self.models if "/" in model}, key=str.casefold)
            self.folder_box["values"] = ["All folders", *folders]
            if self.folder.get() not in self.folder_box["values"]:
                self.folder.set("All folders")
            self.refresh()
            return
        except OSError as exc:
            self.scanning = False
            self.reload_button.configure(state="normal")
            self.count.configure(text=f"Could not read assets: {exc}")
            return
        self.count.configure(text=f"Scanning assets… {len(self.models):,} models")
        self.scan_job = self.window.after(1, self.scan_step)

    def filter_later(self, *args):
        self.cancel("filter_job")
        self.filter_job = self.window.after(120, self.refresh)

    def refresh(self):
        self.cancel("filter_job")
        self.cancel("render_job")
        if self.source != self.editor.project.assets:
            self.reload()
            return
        if self.scanning:
            return
        text, folder = self.search.get().casefold().strip(), self.folder.get()
        selected = self.reference.get()
        self.rows = [model for model in self.models if text in model.casefold()
                     and (folder == "All folders" or model.startswith(folder + "/"))]
        self.tree.delete(*self.tree.get_children())
        if selected not in self.rows:
            self.reference.set("")
            self.preview.configure(image="", text="Select an asset to preview")
        self.update_placement()
        self.count.configure(text=f"{len(self.rows):,} of {len(self.models):,} assets" if self.models else
                             "No placeable models found. Check File → Sources." if self.source.is_dir() else
                             "Assets folder unavailable. Check File → Sources.")
        self.render_step(0)

    def render_step(self, start):
        self.render_job = None
        for index in range(start, min(start + 250, len(self.rows))):
            model = self.rows[index]
            self.tree.insert("", "end", iid=str(index), values=(model,), image=self.editor.previews.icon(GEOMETRY))
            if model == self.reference.get():
                self.tree.selection_set(str(index))
        if start + 250 < len(self.rows):
            self.render_job = self.window.after(1, lambda: self.render_step(start + 250))

    def select(self, event=None):
        selected = self.tree.selection()
        if selected and int(selected[0]) < len(self.rows):
            self.reference.set(self.rows[int(selected[0])])
        self.update_placement()

    def update_placement(self):
        self.place_button.configure(state="normal" if self.reference.get() and self.editor.session and not self.scanning else "disabled")

    def place(self):
        reference = self.reference.get()
        if not reference or self.scanning:
            return
        if self.source != self.editor.project.assets:
            self.reload()
            return
        if not (self.source / (reference + ".glb")).is_file():
            self.reload()
            raise ValueError("This asset is no longer available. The asset library has been refreshed.")
        self.editor.place_asset(reference)

    def close(self):
        for name in ("scan_job", "filter_job", "render_job"):
            self.cancel(name)
