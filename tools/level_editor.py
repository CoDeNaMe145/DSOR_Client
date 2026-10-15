#!/usr/bin/env python3
"""Local level editor: python tools/level_editor.py [map] [--assets DIR] [--db-root DIR]."""
from __future__ import annotations

import argparse
import copy
import json
import math
import os
import re
from pathlib import Path
import subprocess
import struct
import sys
import tkinter as tk
from tkinter import filedialog, messagebox, ttk
import uuid

from level_data import (GEOMETRY, Project, atomic_json, decode, encode, float_blob,
                        floats, moved, position, read_json)
from level_export import build
from level_viewport import Viewport
from level_previews import ModelPicker, Previews
from level_events import MapEvents, event_label
from level_assets import placement_position
from level_cameras import CAMERA, is_camera
from level_lighting import KINDS as LIGHTING_KINDS, LIGHT, AMBIENCE, property_value

ROOT = Path(__file__).resolve().parent.parent
BACKGROUND = "#111820"
PANEL = "#1a2430"
TEXT = "#dce6ed"
MUTED = "#92a4b5"
ACCENT = "#69cfb4"


def display_values(values):
    result = copy.deepcopy(values)
    # Exact hex for all other blobs; only known float fields get a friendly view.
    for key, value in result.items():
        if isinstance(value, dict) and "$blob" in value and (key == "Transform" or "Color" in key or key in ("LightAmbient", "Balance", "OverHeadIconOffset")):
            try:
                vector = floats(value)
                if vector and all(math.isfinite(v) for v in vector):
                    result[key] = {"$f32": vector}
            except (TypeError, ValueError):
                pass
    return result


def stored_values(values):
    if not isinstance(values, dict):
        raise ValueError("The inspector must contain a JSON object")
    return {key: float_blob(value["$f32"]) if isinstance(value, dict) and set(value) == {"$f32"} else value
            for key, value in values.items()}


def label(kind, values):
    if kind == LIGHT:
        name = values.get('Name') or values.get('Id')
        suffix = str(values.get('_ID') or values.get('Guid', {}).get('$blob', ''))[:8]
        return name or ('Spot light' if values.get('LightType') == 1 else 'Point light') + (' · ' + suffix if suffix else '')
    if kind == AMBIENCE and values.get('PEDefaultEntity'):
        return 'Environment · Sun & ambient'
    return str(values.get("n") or values.get("Name") or values.get("Id") or values.get("model")
               or values.get("SequencerFileName") or values.get("_ID") or kind.removeprefix("_Instance_"))


def kind_label(kind):
    if kind == CAMERA:
        return 'Fixed cameras'
    return "Geometry" if kind == GEOMETRY else re.sub(r"(?<=[a-z])(?=[A-Z])", " ", kind.removeprefix("_Instance_"))


def color(kind, values):
    if kind == GEOMETRY:
        return "#486862" if "tile_ground" in values.get("model", "") else "#6f8291"
    return {"_Instance_NPC": "#69d5d6", "_Instance_Light": "#eac975", "_Instance_Entry": "#8cb8ff",
            "_Instance_InteractExit": "#be9bfa", "_Instance_SpawnPoint": "#f18f83",
            "_Instance_Monster": "#f18f83", "_Instance_Sequence": "#a9c68a"}.get(kind, "#c4a0b0")


class Editor:
    def __init__(self, window, project, initial):
        self.window, self.project = window, project
        self.session = None
        self.selected = None
        self.rows = []
        self.changed_rows = []
        self.view = [0.0, 0.0, 2.5]
        self.drag = None
        self.pan = None
        self.inspector_dirty = False
        self.inspector_text = ""
        self.nav_edges = []
        self.viewport = None
        self.viewport_status = tk.StringVar(value="Starting the live 3D viewport…")
        self.map_var = tk.StringVar(value=initial)
        self.kind_var = tk.StringVar(value="All objects")
        self.kind_names = {}
        self.search_var = tk.StringVar()
        self.show_events = tk.BooleanVar(value=False)
        self.event_var = tk.StringVar(value="None")
        self.event_choices = {"None": None}
        self.map_events = MapEvents({})
        self.active_events = {-1}
        self.active_flags = set()
        self.show_helpers = tk.BooleanVar(value=False)
        self.show_grid = tk.BooleanVar(value=False)
        self.camera_speed = tk.StringVar(value="15")
        self.snap = tk.StringVar(value="1")
        self.status = tk.StringVar(value="Open a map to begin")
        self.detail = tk.StringVar(value="Select an object in the map or outliner")
        self.xyz = [tk.StringVar(value="0") for _ in range(3)]
        self.setup()
        self.viewport = Viewport(self, self.viewport_host)
        self.previews = Previews(self)
        self.previews.browsers.append(self.asset_browser)
        window.protocol("WM_DELETE_WINDOW", self.close)
        window.bind("<Control-s>", lambda e: self.action(self.save))
        window.bind("<Control-z>", lambda e: self.action(self.undo) if window.focus_get() != self.inspector else None)
        window.bind("<Control-y>", lambda e: self.action(self.redo) if window.focus_get() != self.inspector else None)
        window.bind("<KeyPress>", self.tool_shortcut, add=True)
        self.search_var.trace_add("write", lambda *args: self.refresh_list())
        self.snap.trace_add("write", lambda *args: self.viewport.controls())
        self.window.after(20, lambda: self.action(self.open_map))

    def setup(self):
        from level_editor_ui import setup
        setup(self)

    def tool_shortcut(self, event):
        # Restrict to scene widgets. Entry/Text/Combobox typing and modal asset
        # pickers retain their keys; native viewport input is handled in Rust.
        if event.widget not in (self.objects, self.changes, self.lighting_panel.tree, self.canvas, self.viewport_host):
            return
        if event.state & (0x4 | 0x8 | 0x20000 | 0x100 | 0x200 | 0x400) or self.drag:
            return
        if not self.selected or not self.is_active(self.selected[0], self.session.records[self.selected[0]][self.selected[1]], self.selected[1]):
            return
        mode = {'q': 'select', 'w': 'translate', 'e': 'rotate', 'r': 'scale', 'g': 'translate', 's': 'scale'}.get(event.keysym.lower())
        if mode:
            self.viewport.set_mode(mode)
            return 'break'

    @staticmethod
    def tree(parent, columns, widths):
        tree = ttk.Treeview(parent, columns=columns, show="headings", selectmode="browse", height=16)
        for column, width in zip(columns, widths):
            tree.column(column, width=width, minwidth=40, stretch=column == "name")
        bar = ttk.Scrollbar(parent, command=tree.yview)
        tree.configure(yscrollcommand=bar.set)
        bar.pack(side="right", fill="y")
        tree.pack(fill="both", expand=True)
        return tree

    def action(self, fn):
        try:
            return fn()
        except Exception as exc:
            self.status.set(str(exc))
            messagebox.showerror("Level editor", str(exc), parent=self.window)

    def needs_session(self):
        if self.session is None:
            raise ValueError("Open a map first")

    def pending_inspector(self):
        return (self.inspector.get("1.0", "end-1c") != self.inspector_text or
                any(var.get() != self.property_initial.get(key) for key, var in self.property_fields.items()))

    def commit_inspector(self):
        if self.selected and self.pending_inspector():
            kind, ident = self.selected
            values = stored_values(json.loads(self.inspector.get("1.0", "end-1c")))
            for key, variable in self.property_fields.items():
                text = variable.get()
                if text == self.property_initial.get(key):
                    continue
                old = self.session.records[kind][ident][key]
                if values.get(key) != old:
                    raise ValueError(f"{key} was edited in both Properties and JSON; apply one version first")
                values[key] = property_value(key, text, old)
            self.session.update(kind, ident, values)
            self.inspector_text = json.dumps(display_values(values), indent=2, ensure_ascii=False)
            self.inspector.delete("1.0", "end")
            self.inspector.insert("1.0", self.inspector_text)
            self.property_initial = {key: var.get() for key, var in self.property_fields.items()}

    def can_leave(self):
        self.commit_inspector()
        if self.session and self.session.dirty:
            answer = messagebox.askyesnocancel("Unsaved overrides", "Save this map's overrides before leaving?", parent=self.window)
            if answer is None:
                return False
            if answer:
                self.session.save()
        return True

    def open_map(self):
        if not self.can_leave():
            return
        session = self.project.open(self.map_var.get().strip())
        self.session, self.selected = session, None
        if self.asset_browser.window.winfo_ismapped():
            self.asset_browser.ensure_source()
        self.event_var.set("None")
        if self.viewport:
            self.viewport.catalogs.clear()
        self.load_navigation()
        self.kind_var.set("All objects")
        self.search_var.set("")
        self.kind_names = {kind_label(kind): kind for kind in dict.fromkeys([GEOMETRY, *sorted(session.snapshot["schemas"]), *session.records])}
        self.kind_box["values"] = ["All objects", *self.kind_names]
        self.map_box["values"] = self.project.maps()
        self.refresh()
        self.window.after(30, self.fit)

    def load_navigation(self):
        """Keep a lightweight walkable outline usable without any model assets."""
        path = self.project.assets / "maps" / (self.session.name + ".nav.bin")
        cache = self.project.cache / "navigation" / (self.session.name + ".json")
        self.nav_edges = read_json(cache, [])
        if not path.is_file():
            return
        raw = path.read_bytes()
        if len(raw) < 4:
            return
        count = struct.unpack_from("<I", raw)[0]
        if len(raw) != 4 + count * 36:
            return
        edges = {}
        for triangle in struct.iter_unpack("<9f", raw[4:]):
            points = [tuple(round(v, 4) for v in triangle[offset:offset + 3]) for offset in (0, 3, 6)]
            for a, b in zip(points, points[1:] + points[:1]):
                key = tuple(sorted((a, b)))
                edges[key] = edges.get(key, 0) + 1
        self.nav_edges = [[a[0], a[2], b[0], b[2]] for (a, b), count in edges.items() if count == 1]
        atomic_json(cache, self.nav_edges)

    def sources(self):
        if not self.can_leave():
            return
        dialog = tk.Toplevel(self.window)
        dialog.title("Level sources")
        dialog.transient(self.window)
        dialog.grab_set()
        variables = [tk.StringVar(value=str(self.project.assets)),
                     tk.StringVar(value=str(self.project.db_root or "")),
                     tk.StringVar(value=str(self.project.overrides))]
        for index, (caption, variable) in enumerate(zip(("Converted assets", "Original DB4 maps directory", "Saved override directory"), variables)):
            ttk.Label(dialog, text=caption).grid(row=index * 2, column=0, sticky="w", padx=12, pady=(12, 3))
            ttk.Entry(dialog, textvariable=variable, width=70).grid(row=index * 2 + 1, column=0, padx=12)
            def browse(var=variable):
                chosen = filedialog.askdirectory(parent=dialog, initialdir=var.get() or self.project.root)
                if chosen:
                    var.set(chosen)
            ttk.Button(dialog, text="Browse…", command=browse).grid(row=index * 2 + 1, column=1, padx=(0, 12))
        ttk.Label(dialog, text="Sources are read-only. Metadata is cached outside assets for offline editing.\n"
                  "Use the server's maps directory to match its gameplay DB4 data.").grid(row=6, column=0, columnspan=2, padx=12, pady=12)
        def accept():
            project = Project(self.project.root, variables[0].get(), variables[1].get() or None, variables[2].get())
            atomic_json(project.cache / "project.json", {"assets": str(project.assets), "db_root": str(project.db_root) if project.db_root else None, "overrides": str(project.overrides)})
            self.project = project
            dialog.destroy()
            self.open_map()
        ttk.Button(dialog, text="Use sources", command=lambda: self.action(accept)).grid(row=7, column=0, columnspan=2, pady=12)

    def refresh(self):
        if not self.session:
            return
        s = self.session
        self.refresh_events()
        cameras = [(kind, ident) for kind, rows in s.records.items() if is_camera(kind) for ident in rows]
        camera_caption = 'Fixed camera · Select' if cameras else 'Fixed camera · source unavailable' if s.name == 'a0000_char' else 'Player-follow camera'
        self.camera_button.configure(text=camera_caption, state='normal' if cameras else 'disabled')
        self.source_label.configure(text=f"Geometry: {s.sources[GEOMETRY]}   ·   DB4: {s.sources['db4']}   ·   {len(s.conflicts)} conflicts")
        self.saved_label.configure(text="Unsaved changes" if s.dirty else "Overrides saved")
        self.window.title(f"DSOR Level Editor — {s.name}" + (" *" if s.dirty else ""))
        self.refresh_list()
        self.changes.delete(*self.changes.get_children())
        self.changed_rows = []
        conflict_keys = {(c["kind"], c["id"]) for c in s.conflicts}
        for op in s.patch["operations"]:
            key = (op["kind"], op["id"])
            self.changed_rows.append(key)
            values = {**op.get("base", {}), **op.get("values", {})}
            self.changes.insert("", "end", iid=str(len(self.changed_rows) - 1),
                                values=(label(op["kind"], values), "CONFLICT" if key in conflict_keys else op["action"]))
        self.show_inspector()
        self.lighting_panel.refresh()
        self.status.set(f"{len(s.patch['operations'])} changes  ·  {len(self.rows):,} records")
        if self.viewport:
            self.viewport.schedule()

    def refresh_list(self):
        if not self.session:
            return
        kind_filter, search = self.kind_names.get(self.kind_var.get(), self.kind_var.get()), self.search_var.get().casefold().strip()
        self.rows = []
        self.objects.delete(*self.objects.get_children())
        operation_keys = {(o["kind"], o["id"]): o["action"] for o in self.session.patch["operations"]}
        for kind, records in self.session.records.items():
            if kind_filter != "All objects" and kind != kind_filter:
                continue
            for ident, values in records.items():
                if kind_filter == "All objects" and position(kind, values) is None:
                    continue
                active = self.is_active(kind, values, ident)
                if not self.show_events.get() and not active:
                    continue
                name = label(kind, values)
                guid = str(values.get("Guid", ""))
                if search and search not in (name + " " + kind + " " + str(values.get("model") or values.get("Graphics") or "") + " " + guid).casefold():
                    continue
                self.rows.append((kind, ident))
                state = operation_keys.get((kind, ident), "base")
                self.objects.insert("", "end", iid=str(len(self.rows) - 1), values=(name, state if active else "Hidden"), image=self.previews.icon(kind), tags=() if active else ("inactive",))
        self.object_count.configure(text=f"{len(self.rows):,} records in outliner")
        self.status.set(f"{len(self.session.patch['operations'])} changes  ·  {len(self.rows):,} records")
        self.draw()
        if self.viewport:
            self.viewport.schedule()

    def is_active(self, kind, values, ident=None):
        if kind == GEOMETRY:
            return self.map_events.visible(kind, values, self.active_flags, ident)
        return MapEvents.reference(kind, values) in self.active_events

    def refresh_events(self):
        self.map_events = MapEvents(self.session.records, (self.session.snapshot['manifest'] or {}).get('event_rules'))
        selected = self.event_choices.get(self.event_var.get())
        self.event_choices = {"None": None, **{event_label(flag): flag for flag in self.map_events.flags}}
        current = next((name for name, flag in self.event_choices.items() if flag == selected), "None")
        self.event_var.set(current)
        self.event_box["values"] = list(self.event_choices)
        self.active_flags = self.map_events.preview_flags(self.event_choices[current])
        self.active_events = self.map_events.active_sets(self.active_flags)
        hidden = sum(not self.is_active(kind, row, ident) for kind, rows in self.session.records.items() for ident, row in rows.items() if position(kind, row) is not None)
        state = 'Base map · no active event' if not self.active_flags else ' + '.join(sorted(self.active_flags))
        self.event_note.configure(text=f"{state}  ·  {hidden:,} inactive objects hidden")

    def change_event(self):
        self.commit_inspector()
        self.refresh_events()
        self.refresh_list()
        self.show_inspector()
        self.lighting_panel.refresh()

    def show_properties(self, kind=None, values=None):
        for child in self.property_form.winfo_children():
            child.destroy()
        self.property_fields = {}
        self.property_initial = {}
        self.selection_name.configure(text=label(kind, values) if values else "Nothing selected")
        rules = self.map_events.describe(kind, values, self.selected[1] if self.selected else None) if values else "Select an object in the viewport or outliner."
        self.event_detail.configure(text=rules if len(rules) <= 180 else rules[:177] + "…")
        if not values:
            return
        if kind in LIGHTING_KINDS:
            from level_lighting_ui import properties
            properties(self, kind, values)
            return
        names = ("n", "model", "event", "Id", "FieldOfView", "PreviewAspect", "NearClip", "FarClip",
                 "CameraVertFieldOfView", "CameraHoriFieldOfView", "CameraAspectRatio", "Name", "Graphics", "CharacterSet", "AnimSet", "StartAnimationState",
                 "EventSetRow", "SequenceIsPlaying", "SequencerFileName", "LightIntensity", "LightRange", "State")
        for key in names:
            if key not in values or isinstance(values[key], (dict, list)):
                continue
            frame = ttk.Frame(self.property_form, padding=(0, 3, 20, 3))
            frame.pack(fill="x")
            caption = {"n": "Name", "model": "Model", "EventSetRow": "Event set ID",
                       "FieldOfView": "Vertical field of view (degrees)",
                       "PreviewAspect": "Preview aspect (width / height)"}.get(key, re.sub(r"(?<=[a-z])(?=[A-Z])", " ", key))
            ttk.Label(frame, text=caption, style="Muted.TLabel").pack(anchor="w", pady=(0, 3))
            variable = tk.StringVar(value="" if values[key] is None else str(values[key]))
            self.property_fields[key] = variable
            self.property_initial[key] = variable.get()
            ttk.Entry(frame, textvariable=variable).pack(fill="x")

    def apply_properties(self):
        self.commit_inspector()
        self.refresh()

    def set_selected(self, key):
        pending = self.pending_inspector()
        self.commit_inspector()
        self.selected = key
        if pending:
            self.refresh()
        else:
            self.show_inspector()
            self.draw()
        if self.viewport:
            self.viewport.schedule()

        self.lighting_panel.reveal()

    def select_list(self, event=None):
        selection = self.objects.selection()
        if selection and int(selection[0]) < len(self.rows):
            self.action(lambda: self.set_selected(self.rows[int(selection[0])]))

    def select_change(self, event=None):
        selection = self.changes.selection()
        if selection and int(selection[0]) < len(self.changed_rows):
            self.action(lambda: self.set_selected(self.changed_rows[int(selection[0])]))

    def selected_values(self):
        self.needs_session()
        if not self.selected:
            raise ValueError("Select a record first")
        kind, ident = self.selected
        values = self.session.records.get(kind, {}).get(ident)
        if values is None:
            raise ValueError("This record is deleted or missing. Use Revert or resolve its conflict")
        return kind, ident, values

    def show_inspector(self):
        text = ""
        self.conflict_button.pack_forget()
        if self.selected:
            kind, ident = self.selected
            values = self.session.records.get(kind, {}).get(ident)
            op = self.session.operation(kind, ident)
            if values is None and op:
                values = {**op.get("base", {}), **op.get("values", {})}
            if values is None:
                self.selected = None
            else:
                text = json.dumps(display_values(values), indent=2, ensure_ascii=False)
                p = position(kind, values)
                for var, v in zip(self.xyz, p or [0, 0, 0]):
                    var.set(f"{v:.6g}")
                extra = ""
                if kind == GEOMETRY:
                    path = self.project.assets / (values["model"].split(":")[-1] + ".glb")
                    extra = "\nModel: available" if path.is_file() else "\nModel: missing — placement remains editable"
                conflict = next((c["reason"] for c in self.session.conflicts if (c["kind"], c["id"]) == self.selected), "")
                if conflict:
                    extra += "\nCONFLICT: " + conflict
                    self.conflict_button.pack(fill="x", pady=8)
                self.detail.set(kind_label(kind) + (" · Inactive in this event state" if not self.is_active(kind, values, ident) else "") + extra)
                self.show_properties(kind, values)
                if is_camera(kind):
                    self.detail.set('Fixed game camera · move / rotate to compose the shot. Preview aspect is adjustable below.'
                                    + ('\nInactive in this event state' if not self.is_active(kind, values, ident) else '') + extra)
                    if self.viewport.mode == 'scale':
                        self.viewport.set_mode('translate')
                elif kind in LIGHTING_KINDS:
                    self.detail.set(('Light · color, intensity, range and shadows' if kind == LIGHT else
                                     'Environment · sun, ambient, fog and bloom') + extra)
        if not self.selected:
            self.detail.set("Select an object in the viewport or outliner")
            self.show_properties()
        self.inspector.delete("1.0", "end")
        self.inspector.insert("1.0", text)
        self.inspector.edit_reset()
        self.inspector_text = text

    def save(self):
        self.needs_session()
        self.commit_inspector()
        self.session.save()
        self.refresh()
        self.status.set("Overrides saved outside generated assets: " + str(self.project.patch_path(self.session.name)))

    def build(self):
        self.save()
        # Re-read the source immediately before building, catching concurrent
        # rebuilds / DB4 edits instead of exporting an old cached baseline.
        self.session.refresh_sources()
        self.session.recompose()
        self.refresh()
        report = build(self.session)
        self.status.set(f"Built {len(report['files'])} client map files. DB4: {report['database'] or 'no database source'}. " + report["note"])
        return report

    def preview(self):
        self.build()
        candidates = [self.project.root / "target" / "release" / "dsor-client.exe",
                      self.project.root / "target" / "release" / "dsor-client",
                      self.project.root / "target" / "debug" / "dsor-client.exe"]
        executable = next((p for p in candidates if p.is_file()), None)
        if executable is None:
            raise ValueError("Build the client first: cargo build --locked --release -p dsor-client")
        env = {**os.environ, "DSOR_ASSETS": str(self.project.assets)}
        args = [str(executable), self.session.name, "--npcs"]
        if self.selected:
            kind, ident, values = self.selected_values()
            p = position(kind, values)
            if p:
                args += ["--cam", ",".join(str(v) for v in [p[0], p[1] + 18, p[2] + 18, *p])]
        subprocess.Popen(args, cwd=self.project.root, env=env)
        self.status.set("Opened the effective map in the 3D client. Close and preview again after further edits.")

    def select_camera(self):
        self.commit_inspector()
        cameras = [(kind, ident) for kind, rows in self.session.records.items() if is_camera(kind) for ident, row in rows.items() if self.is_active(kind, row, ident)]
        if cameras:
            self.kind_var.set('All objects')
            self.search_var.set('')
            self.refresh_list()
            self.set_selected(cameras[0])
            if cameras[0] in self.rows:
                iid = str(self.rows.index(cameras[0]))
                self.objects.selection_set(iid)
                self.objects.see(iid)
            self.focus()

    def apply_fields(self):
        self.commit_inspector()
        self.refresh()

    def apply_position(self):
        self.commit_inspector()
        kind, ident, values = self.selected_values()
        self.session.update(kind, ident, moved(kind, values, [float(var.get()) for var in self.xyz]))
        self.refresh()

    def transform(self, angle, scale):
        self.commit_inspector()
        kind, ident, values = self.selected_values()
        changed = copy.deepcopy(values)
        if kind == GEOMETRY:
            x, y, z, w = changed["r"]
            sine, cosine = math.sin(math.radians(angle) / 2), math.cos(math.radians(angle) / 2)
            changed["r"] = [cosine * x + sine * z, cosine * y + sine * w, cosine * z - sine * x, cosine * w - sine * y]
            length = math.sqrt(sum(v * v for v in changed["r"]))
            changed["r"] = [v / length for v in changed["r"]]
            changed["s"] = [v * scale for v in changed["s"]]
        else:
            matrix = floats(changed["Transform"])
            sine, cosine = math.sin(math.radians(angle)), math.cos(math.radians(angle))
            for offset in (0, 4, 8):
                x, y, z = matrix[offset:offset + 3]
                matrix[offset:offset + 3] = [(cosine * x + sine * z) * scale, y * scale, (cosine * z - sine * x) * scale]
            changed["Transform"] = float_blob(matrix)
        self.session.update(kind, ident, changed)
        self.refresh()

    def add(self):
        self.needs_session()
        self.commit_inspector()
        kind = self.kind_names.get(self.kind_var.get(), self.kind_var.get())
        if kind == LIGHT:
            self.lighting_panel.add()
            return
        if kind in ("All objects", GEOMETRY):
            reference = ModelPicker(self).choose()
            if not reference:
                return
            self.place_asset(reference)
            return
        else:
            schema = self.session.snapshot["schemas"].get(kind)
            if not schema:
                raise ValueError("The DB4 schema is unavailable; restore the source or duplicate a cached record")
            values = {}
            for column in schema["columns"]:
                name, affinity = column["name"], column["type"].upper()
                value = encode(b"") if "BLOB" in affinity else 0 if "INT" in affinity else 0.0 if "REAL" in affinity else ""
                default = column["dflt_value"]
                if default is not None:
                    try:
                        value = json.loads(default)
                    except ValueError:
                        if default.startswith("'") and default.endswith("'"):
                            value = default[1:-1].replace("''", "'")
                if name == "Guid": value = encode(uuid.uuid4().bytes)
                if name in ("EventSetRow", "ParentNodeIndex"): value = -1
                if name == "_Level": value = self.session.name
                if name == "Transform": value = float_blob([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, self.view[0], 0, self.view[1], 1])
                if "BLOB" in affinity and ("Color" in name or name in ("LightAmbient", "Balance")):
                    value = float_blob([1, 1, 1, 1])
                values[name] = value
        ident = self.session.add(kind, values)
        self.selected = kind, ident
        self.refresh()

    def place_asset(self, reference):
        """Both the library and picker author the same persistent geometry layer."""
        self.needs_session()
        self.commit_inspector()
        try:
            snap = float(self.snap.get())
        except ValueError:
            snap = 0
        status = getattr(self.viewport, "last_status", {}) or {}
        if status.get("revision") != getattr(self.viewport, "revision", None):
            status = {}
        location = placement_position(status, [self.view[0], 0, self.view[1]], snap)
        values = {"model": reference.strip(), "p": location, "r": [0, 0, 0, 1],
                  "s": [1, 1, 1], "n": "", "g": -1, "col": True}
        ident = self.session.add(GEOMETRY, values)
        self.selected = GEOMETRY, ident
        self.kind_var.set("All objects")
        self.search_var.set("")
        self.refresh()
        if self.selected in self.rows:
            iid = str(self.rows.index(self.selected))
            self.objects.selection_set(iid)
            self.objects.see(iid)
        # Refresh publishes the new scene first, then focus its newly added root.
        self.window.after_idle(self.focus)
        return ident

    def duplicate(self):
        self.commit_inspector()
        kind, ident, _ = self.selected_values()
        self.selected = kind, self.session.duplicate(kind, ident)
        self.refresh()

    def delete(self):
        self.commit_inspector()
        kind, ident, _ = self.selected_values()
        self.session.delete(kind, ident)
        self.selected = None
        self.refresh()

    def revert(self):
        if self.selected:
            self.session.revert(*self.selected)
            self.refresh()

    def keep_override(self):
        if self.selected:
            self.session.keep_override(*self.selected)
            self.refresh()

    def undo(self):
        if self.session:
            self.session.undo()
            self.refresh()

    def redo(self):
        if self.session:
            self.session.redo()
            self.refresh()

    def screen(self, x, z):
        return ((x - self.view[0]) * self.view[2] + self.canvas.winfo_width() / 2,
                (z - self.view[1]) * self.view[2] + self.canvas.winfo_height() / 2)

    def world(self, x, y):
        return ((x - self.canvas.winfo_width() / 2) / self.view[2] + self.view[0],
                (y - self.canvas.winfo_height() / 2) / self.view[2] + self.view[1])

    def fit(self):
        if not self.session:
            return
        if self.viewport:
            self.viewport.focus("map")
        positions = [p for kind, ident in self.rows if (p := position(kind, self.session.records[kind][ident]))]
        if not positions:
            self.view = [0, 0, 3]
        else:
            low_x, high_x = min(p[0] for p in positions), max(p[0] for p in positions)
            low_z, high_z = min(p[2] for p in positions), max(p[2] for p in positions)
            zoom = min(max(100, self.canvas.winfo_width() - 80) / max(high_x - low_x, 20),
                       max(100, self.canvas.winfo_height() - 80) / max(high_z - low_z, 20))
            self.view = [(low_x + high_x) / 2, (low_z + high_z) / 2, zoom]
        self.draw()

    def focus(self):
        if self.viewport:
            self.viewport.focus("selection")
        if self.selected:
            kind, ident = self.selected
            values = self.session.records.get(kind, {}).get(ident)
            if values and (p := position(kind, values)):
                self.view[:2] = [p[0], p[2]]
                self.view[2] = max(5, self.view[2])
                self.draw()

    def draw(self):
        canvas = self.canvas
        canvas.delete("all")
        width, height = canvas.winfo_width(), canvas.winfo_height()
        desired = 45 / max(0.01, self.view[2])
        magnitude = 10 ** math.floor(math.log10(desired))
        step = next(factor * magnitude for factor in (1, 2, 5, 10) if factor * magnitude >= desired)
        left, top = self.world(0, 0)
        right, bottom = self.world(width, height)
        x = math.floor(left / step) * step
        while x < right:
            sx, _ = self.screen(x, 0)
            canvas.create_line(sx, 0, sx, height, fill="#20303c")
            canvas.create_text(sx + 3, 12, text=f"{x:g}", anchor="w", fill="#536d7f", font=("Segoe UI", 8))
            x += step
        z = math.floor(top / step) * step
        while z < bottom:
            _, sy = self.screen(0, z)
            canvas.create_line(0, sy, width, sy, fill="#20303c")
            z += step
        if not self.session:
            return
        for ax, az, bx, bz in self.nav_edges:
            a, b = self.screen(ax, az), self.screen(bx, bz)
            if max(a[0], b[0]) >= 0 and min(a[0], b[0]) <= width and max(a[1], b[1]) >= 0 and min(a[1], b[1]) <= height:
                canvas.create_line(*a, *b, fill="#3e716f", width=1)
        manifest = self.session.snapshot["manifest"] or {}
        for polygon in manifest.get("nav_blockers", []):
            points = [coordinate for x, z in polygon for coordinate in self.screen(x, z)]
            if len(points) >= 6:
                canvas.create_polygon(points, fill="", outline="#73525a", dash=(3, 4))
        ordered = sorted(enumerate(self.rows), key=lambda item: item[1][0] != GEOMETRY)
        for index, (kind, ident) in ordered:
            values = self.session.records[kind][ident]
            if not self.is_active(kind, values, ident):
                continue
            p = position(kind, values)
            if p is None:
                continue
            if self.drag and self.drag[0] == (kind, ident) and len(self.drag) > 3:
                p = self.drag[3]
            x, y = self.screen(p[0], p[2])
            if not (-25 < x < width + 25 and -25 < y < height + 25):
                continue
            selected = self.selected == (kind, ident)
            tint = "#ffffff" if selected else color(kind, values)
            radius = 6 if selected else 3 if kind == GEOMETRY else 4
            tags = (f"object:{index}",)
            if kind == GEOMETRY:
                canvas.create_rectangle(x - radius, y - radius, x + radius, y + radius, fill=tint if selected else "", outline=tint, tags=tags)
            else:
                canvas.create_oval(x - radius, y - radius, x + radius, y + radius, fill=tint, outline="", tags=tags)
            if selected:
                canvas.create_oval(x - 11, y - 11, x + 11, y + 11, outline=ACCENT, width=2)
                canvas.create_text(x + 15, y - 12, text=label(kind, values)[:55], anchor="w", fill="white", font=("Segoe UI", 10, "bold"))
        canvas.create_text(12, height - 20, text=f"+X →   +Z ↓    Grid {step:g} m", anchor="w", fill=MUTED)

    def map_press(self, event):
        self.canvas.focus_set()
        hits = self.canvas.find_overlapping(event.x - 6, event.y - 6, event.x + 6, event.y + 6)
        for item in reversed(hits):
            for tag in self.canvas.gettags(item):
                if tag.startswith("object:"):
                    key = self.rows[int(tag.split(":")[1])]
                    try:
                        self.set_selected(key)
                        p = position(key[0], self.session.records[key[0]][key[1]])
                        if self.viewport.mode == 'translate':
                            self.drag = (key, (event.x, event.y), p)
                    except Exception as exc:
                        self.action(lambda: (_ for _ in ()).throw(exc))
                    return

    def map_drag(self, event):
        if self.drag:
            key, (x, y), p = self.drag[:3]
            try:
                snap = max(0, float(self.snap.get()))
            except ValueError:
                return
            dx, dz = (event.x - x) / self.view[2], (event.y - y) / self.view[2]
            if abs(event.x - x) + abs(event.y - y) < 3:
                return
            nx, nz = p[0] + dx, p[2] + dz
            if snap:
                nx, nz = round(nx / snap) * snap, round(nz / snap) * snap
            self.drag = (key, (x, y), p, [nx, p[1], nz])
            self.draw()

    def map_release(self, event):
        drag, self.drag = self.drag, None
        if drag and len(drag) > 3:
            key, _, _, p = drag
            def commit():
                self.session.update(*key, moved(key[0], self.session.records[key[0]][key[1]], p))
                self.refresh()
            self.action(commit)

    def pan_map(self, event):
        if self.pan:
            x, y, wx, wz = self.pan
            self.view[:2] = [wx - (event.x - x) / self.view[2], wz - (event.y - y) / self.view[2]]
            self.draw()

    def zoom(self, event, direction=None):
        direction = direction if direction is not None else (1 if event.delta > 0 else -1)
        before = self.world(event.x, event.y)
        self.view[2] = min(100, max(0.02, self.view[2] * (1.2 ** direction)))
        after = self.world(event.x, event.y)
        self.view[0] += before[0] - after[0]
        self.view[1] += before[1] - after[1]
        self.draw()

    def close(self):
        if self.action(self.can_leave):
            self.asset_browser.close()
            self.previews.close()
            if self.viewport:
                self.viewport.close()
            self.window.destroy()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("map", nargs="?", default="a0200_kingscity")
    parser.add_argument("--assets", type=Path)
    parser.add_argument("--db-root", type=Path, help="Directory containing original map .db4 files")
    parser.add_argument("--overrides", type=Path)
    args = parser.parse_args()
    config = read_json(ROOT / ".level-editor" / "project.json", {})
    default_db = ROOT / ".local" / "dso_server" / "data" / "maps"
    project = Project(ROOT, args.assets or os.environ.get("DSOR_ASSETS") or config.get("assets"),
                      args.db_root or os.environ.get("DSOR_MAP_DB") or config.get("db_root") or (default_db if default_db.is_dir() else None),
                      args.overrides or os.environ.get("DSOR_LEVEL_OVERRIDES") or config.get("overrides"))
    window = tk.Tk()
    Editor(window, project, args.map)
    window.mainloop()


if __name__ == "__main__":
    main()
