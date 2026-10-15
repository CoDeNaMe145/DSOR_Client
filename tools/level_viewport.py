"""Live native 3D viewport for the level editor.

The Bevy renderer owns interaction and temporary drag previews. Python remains
the sole owner of validated edits, history, saves and DB4 exports. One completed
drag is one edit. Revisions reject gestures made against an older scene.
"""
from __future__ import annotations

import copy
import ctypes
import json
import math
import os
from pathlib import Path
import subprocess
import time
import uuid

from level_data import GEOMETRY, atomic_json, digest, float_blob, floats, moved, position, read_json
from level_events import MapEvents
from level_export import track_matrix
from level_cameras import is_camera, lens


def transform_matrix(kind, values):
    if kind != GEOMETRY:
        return floats(values["Transform"])
    x, y, z, w = values["r"]
    sx, sy, sz = values["s"]
    return [(1 - 2 * (y*y + z*z))*sx, 2*(x*y + z*w)*sx, 2*(x*z - y*w)*sx, 0,
            2*(x*y - z*w)*sy, (1 - 2*(x*x + z*z))*sy, 2*(y*z + x*w)*sy, 0,
            2*(x*z + y*w)*sz, 2*(y*z - x*w)*sz, (1 - 2*(x*x + y*y))*sz, 0, *values["p"], 1]


def apply_transform(session, event, revision):
    if event.get("revision") != revision:
        raise ValueError("The scene changed during the gesture; the outdated 3D edit was cancelled")
    kind, ident = event["key"].split("\t", 1)
    values = session.records[kind][ident]
    operation = event["mode"]
    result = copy.deepcopy(values)
    if operation == "translate":
        target = event["position"]
        if event.get("axis") in (0, 1, 2):
            # Only the dragged component changes; do not round untouched JSON
            # coordinates through the renderer's f32 representation.
            target = list(position(kind, values))
            target[event["axis"]] = event["position"][event["axis"]]
        result = moved(kind, values, target)
    elif kind == GEOMETRY:
        if operation == "rotate":
            result["r"] = event["rotation"]
        elif operation == "scale":
            result["s"] = event["scale"]
        else:
            raise ValueError("Unknown transform operation")
    else:
        # Preserve the exact original translation and basis for DB4 records.
        # Rotation applies a world-axis delta; scaling multiplies local axes.
        matrix = floats(values["Transform"])
        if operation == "rotate":
            axis, angle = event["axis"], event["angle"]
            if axis not in (0, 1, 2) or not math.isfinite(angle):
                raise ValueError("Invalid rotation gesture")
            c, s = math.cos(angle), math.sin(angle)
            for offset in (0, 4, 8):
                x, y, z = matrix[offset:offset + 3]
                matrix[offset:offset + 3] = ([x, c*y-s*z, s*y+c*z] if axis == 0 else
                                            [c*x+s*z, y, -s*x+c*z] if axis == 1 else
                                            [c*x-s*y, s*x+c*y, z])
        elif operation == "scale":
            factors = event["factors"]
            if len(factors) != 3 or any(not math.isfinite(v) or v <= 0 for v in factors):
                raise ValueError("Invalid scale gesture")
            for axis in range(3):
                for component in range(3):
                    matrix[axis*4 + component] *= factors[axis]
        else:
            raise ValueError("Unknown transform operation")
        result["Transform"] = float_blob(matrix)
    session.update(kind, ident, result)
    return kind, ident


class Viewport:
    def __init__(self, editor, host):
        self.editor, self.host = editor, host
        self.directory = editor.project.cache / "live" / str(uuid.uuid4())
        self.directory.mkdir(parents=True)
        (self.directory / "events").mkdir()
        self.revision = 0
        self.scene_digest = None
        self.controls_digest = None
        self.focus_serial = 0
        self.focus_target = "map"
        self.mode = "translate"
        self.process = None
        self.log = None
        self.handle = None
        self.closed = False
        self.source_assets = None
        self.catalogs = {}
        self.pending = None
        self.window_api = None
        self.host_proc = None
        self.previous_host_proc = None
        host.bind("<Configure>", lambda event: self.resize())
        editor.window.after(100, self.poll)

    def catalog(self, relative, fallback):
        key = str(self.editor.project.assets / relative)
        if key not in self.catalogs:
            self.catalogs[key] = read_json(key, fallback)
        return self.catalogs[key]

    def scene(self):
        session = self.editor.session
        objects = []
        for kind, rows in session.records.items():
            for ident, values in rows.items():
                if position(kind, values) is None:
                    continue
                model = values.get("model", "") if kind == GEOMETRY else values.get("Graphics") or ""
                item = {"key": kind + "\t" + ident, "kind": kind,
                        "label": str(values.get("n") or values.get("Name") or values.get("Id") or model or kind),
                        "matrix": transform_matrix(kind, values), "model": model,
                        "event": MapEvents.reference(kind, values)}
                if is_camera(kind):
                    item['camera'] = lens(kind, values)
                    item['model'] = ''
                if model in ("characters/uniskel", "characters/uniskel_dwarf"):
                    skeleton = model.split("/")[-1]
                    outfit_name = values.get("CharacterSet", "")
                    outfits = self.catalog(f"characters/{skeleton}/outfits.json", {"outfits": []})
                    outfit = next((row for row in outfits["outfits"] if row["name"] == outfit_name), None)
                    if outfit:
                        variations = self.catalog(f"characters/{skeleton}/variations.json", {"variations": {}})
                        item["character"] = {"skeleton": skeleton, "parts": outfit["parts"],
                                             "anim_set": values.get("AnimSet") or "",
                                             "state": values.get("StartAnimationState") or "Idle",
                                             "variation": variations["variations"].get(outfit.get("variation"), [])}
                if kind == "_Instance_Light":
                    try:
                        from level_lighting import light
                        item['light'] = light(values)
                        item['model'] = ''
                        item['label'] = ('Spot light' if item['light']['t'] == 1 else 'Point light') + ' · ' + ident[-8:]
                    except (KeyError, ValueError, TypeError):
                        pass
                if kind == '_Instance_AmbienceBubble':
                    from level_lighting import bubble
                    item['ambience'] = bubble(values)
                    item['default_ambience'] = bool(values.get('PEDefaultEntity'))
                    item['model'] = ''
                if kind == "_Instance_Sequence" and values.get("SequenceIsPlaying"):
                    name = (values.get("SequencerFileName") or "").split("/", 1)[-1]
                    sequence = self.catalog("skills/sequences.json", {}).get(name)
                    if sequence:
                        item["sequence"] = sequence
                        item["decorations"] = []
                        for track in sequence.get("tracks", []):
                            if track.get("kind") != "character":
                                continue
                            model, _, rest = track.get("object", "").partition("/")
                            outfit_name, _, clip = rest.partition("/")
                            decoration = {"model": "characters/" + model, "matrix": track_matrix(track["at"]), "character": None}
                            if model in ("uniskel", "uniskel_dwarf"):
                                outfits = self.catalog(f"characters/{model}/outfits.json", {"outfits": []})
                                outfit = next((row for row in outfits["outfits"] if row["name"] == outfit_name), None)
                                if outfit:
                                    variations = self.catalog(f"characters/{model}/variations.json", {"variations": {}})
                                    anims = self.catalog("characters/anims.json", {})
                                    clip = clip + "-loop" if "deco:" + clip + "-loop" in anims else clip
                                    decoration["character"] = {"skeleton": model, "parts": outfit["parts"], "anim_set": "deco:" + clip,
                                                               "state": "Idle", "variation": variations["variations"].get(outfit.get("variation"), [])}
                            item["decorations"].append(decoration)
                objects.append(item)
        manifest = session.snapshot["manifest"] or {}
        return {"protocol": 1, "map": session.name, "center": manifest.get("center", [0, 0, 0]),
                "extents": manifest.get("extents", [30, 20, 30]), "objects": objects,
                'ambience': self.catalog(f'maps/{session.name}.ambience.json', {}).get('global')}

    def publish(self, force=False):
        if self.closed or self.editor.session is None:
            return
        scene = self.scene()
        fingerprint = digest(scene)
        if force or fingerprint != self.scene_digest:
            self.revision += 1
            scene["revision"] = self.revision
            atomic_json(self.directory / "scene.json", scene)
            self.scene_digest = fingerprint
        self.controls()
        self.start()

    def schedule(self):
        if not self.closed and self.pending is None:
            self.pending = self.editor.window.after_idle(self.flush)

    def flush(self):
        self.pending = None
        try:
            self.publish()
        except (OSError, ValueError, KeyError) as exc:
            self.editor.viewport_status.set(str(exc))

    def controls(self):
        try:
            snap = max(0, float(self.editor.snap.get()))
            if not math.isfinite(snap):
                snap = 0
        except ValueError:
            snap = 0
        selected = "\t".join(self.editor.selected) if self.editor.selected else None
        payload = {"revision": self.revision, "selected": selected, "mode": self.mode, "snap": snap,
                   "focus_serial": self.focus_serial, "focus_target": self.focus_target,
                   "pickable": ["\t".join(key) for key in self.editor.rows if self.editor.is_active(key[0], self.editor.session.records[key[0]][key[1]], key[1])],
                   "hidden_geometry": [GEOMETRY + '\t' + ident for ident, row in self.editor.session.records.get(GEOMETRY, {}).items()
                                       if not self.editor.is_active(GEOMETRY, row, ident)],
                   "active_events": sorted(self.editor.active_events),
                   "show_helpers": self.editor.show_helpers.get(), "show_grid": self.editor.show_grid.get(),
                   'show_lights': self.editor.content_tabs.select() == str(self.editor.lighting_panel.parent),
                   "camera_speed": float(self.editor.camera_speed.get()), "quit": self.closed}
        fingerprint = digest(payload)
        if fingerprint != self.controls_digest:
            atomic_json(self.directory / "controls.json", payload)
            self.controls_digest = fingerprint

    def focus(self, target):
        self.focus_serial += 1
        self.focus_target = target
        self.controls()

    def set_mode(self, mode):
        if mode not in ('select', 'translate', 'rotate', 'scale'):
            return
        if mode == 'scale' and self.editor.selected and is_camera(self.editor.selected[0]):
            return
        self.mode = mode
        for name, button in self.editor.mode_buttons.items():
            button.configure(style="Active.Tool.TButton" if name == mode else "Tool.TButton")
        self.editor.viewport_status.set('Select · click an object; W move · E rotate · R scale' if mode == 'select'
                                       else f"{mode.title()} · drag an axis handle; Esc cancels")
        self.controls()

    def start(self):
        if self.process is not None:
            if self.source_assets == self.editor.project.assets:
                return
            self.stop_process()
        root = self.editor.project.root
        executable = next((root / relative for relative in (
            "target/release/dsor-client.exe", "target/release/dsor-client", "target/debug/dsor-client.exe", "target/debug/dsor-client")
            if (root / relative).is_file()), None)
        sources = [root / "client/src/level_editor.rs", root / 'client/src/lighting.rs', root / 'client/src/camera_view.rs',
                   *(root / "client/src/level_editor").glob("*.rs")]
        if executable is None or any(source.is_file() and executable.stat().st_mtime < source.stat().st_mtime for source in sources):
            self.editor.viewport_status.set("Build the 3D editor renderer: powershell -File tools/Build-LevelEditor.ps1 (or cargo build --release -p dsor-client)")
            return
        self.log = (self.directory / "viewport.log").open("w", encoding="utf-8")
        self.process = subprocess.Popen([str(executable), "--level-editor", str(self.directory)], cwd=root,
                                        env={**os.environ, "DSOR_ASSETS": str(self.editor.project.assets)},
                                        stdout=self.log, stderr=subprocess.STDOUT,
                                        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.source_assets = self.editor.project.assets
        self.editor.viewport_status.set("Loading 3D scene…")

    def embed(self):
        if os.name != "nt" or self.handle or not self.process:
            return
        from ctypes import wintypes as w
        api = ctypes.WinDLL("user32", use_last_error=True)
        api.FindWindowW.argtypes, api.FindWindowW.restype = [w.LPCWSTR, w.LPCWSTR], w.HWND
        api.GetWindowThreadProcessId.argtypes = [w.HWND, ctypes.POINTER(w.DWORD)]
        api.SetParent.argtypes, api.SetParent.restype = [w.HWND, w.HWND], w.HWND
        api.GetWindowLongW.argtypes, api.GetWindowLongW.restype = [w.HWND, ctypes.c_int], w.LONG
        api.SetWindowLongW.argtypes = [w.HWND, ctypes.c_int, w.LONG]
        api.SetWindowPos.argtypes = [w.HWND, w.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, w.UINT]
        api.SetFocus.argtypes, api.SetFocus.restype = [w.HWND], w.HWND
        api.SendMessageW.argtypes, api.SendMessageW.restype = [w.HWND, w.UINT, w.WPARAM, w.LPARAM], ctypes.c_ssize_t
        handle = api.FindWindowW(None, "DSOR Level Viewport " + self.directory.name)
        if not handle:
            return
        pid = w.DWORD()
        api.GetWindowThreadProcessId(handle, ctypes.byref(pid))
        if pid.value != self.process.pid:
            return
        # Remove the top-level frame and host the native GPU surface in Tk.
        style = api.GetWindowLongW(handle, -16)
        api.SetWindowLongW(handle, -16, (style & ~0x80CF0000) | 0x50000000)
        ctypes.set_last_error(0)
        api.SetParent(handle, self.host.winfo_id())
        if ctypes.get_last_error():
            self.editor.viewport_status.set("3D viewport opened in a separate window (embedding unavailable)")
            return
        self.handle, self.window_api = handle, api
        self.install_focus_bridge()
        self.resize()

    def install_focus_bridge(self):
        """Focus the native child before its first mouse press is delivered.

        Winit normally owns a top-level window. After reparenting it does not
        receive top-level activation, and its mouse handler does not focus a
        child window. Windows sends WM_PARENTNOTIFY to our host before the
        child's mouse press, so bridge activation there without polling input.
        """
        if self.host_proc:
            return
        from ctypes import wintypes as w
        api = self.window_api
        setter = getattr(api, "SetWindowLongPtrW", api.SetWindowLongW)
        setter.argtypes, setter.restype = [w.HWND, ctypes.c_int, ctypes.c_void_p], ctypes.c_void_p
        api.CallWindowProcW.argtypes = [ctypes.c_void_p, w.HWND, w.UINT, w.WPARAM, w.LPARAM]
        api.CallWindowProcW.restype = ctypes.c_ssize_t
        callback = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, w.HWND, w.UINT, w.WPARAM, w.LPARAM)

        def procedure(hwnd, message, wparam, lparam):
            if message == 0x0210 and (wparam & 0xffff) in (0x0201, 0x0204, 0x0207) and self.handle:
                api.SetFocus(self.handle)
                api.SendMessageW(self.handle, 0x0086, 1, 0)  # WM_NCACTIVATE
            return api.CallWindowProcW(self.previous_host_proc, hwnd, message, wparam, lparam)

        self.host_proc = callback(procedure)
        self.previous_host_proc = setter(self.host.winfo_id(), -4, ctypes.cast(self.host_proc, ctypes.c_void_p))

    def remove_focus_bridge(self):
        if self.host_proc and self.previous_host_proc:
            setter = getattr(self.window_api, "SetWindowLongPtrW", self.window_api.SetWindowLongW)
            setter(self.host.winfo_id(), -4, self.previous_host_proc)
        self.host_proc = self.previous_host_proc = None

    def resize(self):
        if self.handle and self.window_api:
            self.window_api.SetWindowPos(self.handle, None, 0, 0, max(1, self.host.winfo_width()),
                                         max(1, self.host.winfo_height()), 0x0034)

    def poll(self):
        if self.closed:
            return
        try:
            self.embed()
            for path in sorted((self.directory / "events").glob("*.json")):
                event = read_json(path)
                path.unlink()
                try:
                    self.receive(event)
                except (ValueError, KeyError, TypeError) as exc:
                    self.editor.viewport_status.set(str(exc))
                    self.publish(force=True)
            try:
                status = read_json(self.directory / "status.json")
            except PermissionError:
                # Windows can briefly deny reads while the renderer replaces
                # its informational status file. Try again on the next poll.
                status = None
            if status and status.get("revision") == self.revision and status != getattr(self, "last_status", None):
                self.last_status = status
                if "visible" in status:
                    self.editor.viewport_status.set(f"{status['visible']:,} visible  ·  {status['ready']:,} / {status['total']:,} loaded"
                                                    + (f"  ·  {status['missing']:,} missing models" if status.get("missing") else ""))
                else:
                    self.editor.viewport_status.set(status.get("message", "3D viewport ready"))
            if self.process and self.process.poll() is not None:
                self.editor.viewport_status.set(f"3D viewport stopped (code {self.process.returncode}). Log: {self.directory / 'viewport.log'}")
        except (OSError, ValueError) as exc:
            self.editor.viewport_status.set(str(exc))
        self.editor.window.after(100, self.poll)

    def receive(self, event):
        if event.get("revision") != self.revision:
            raise ValueError("The scene changed; the outdated 3D gesture was cancelled")
        action = event["action"]
        if action == "select":
            key = tuple(event["key"].split("\t", 1))
            if key[1] in self.editor.session.records.get(key[0], {}):
                self.editor.set_selected(key)
                if key in self.editor.rows:
                    iid = str(self.editor.rows.index(key))
                    self.editor.objects.selection_set(iid)
                    self.editor.objects.see(iid)
        elif action == "transform":
            if self.editor.pending_inspector():
                raise ValueError("Apply or discard pending inspector text before dragging in 3D")
            self.editor.selected = apply_transform(self.editor.session, event, self.revision)
            self.editor.refresh()
            self.publish()
        elif action == "mode":
            self.set_mode(event["mode"])
        elif action in ("save", "undo", "redo", "delete", "duplicate"):
            getattr(self.editor, action)()
            self.publish()

    def stop_process(self):
        self.remove_focus_bridge()
        if self.process and self.process.poll() is None:
            atomic_json(self.directory / "stop.json", {"stop": True})
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=3)
        if self.log:
            self.log.close()
        self.process, self.log, self.handle = None, None, None
        stop = self.directory / "stop.json"
        if stop.exists():
            stop.unlink()

    def close(self):
        self.closed = True
        if self.pending:
            self.editor.window.after_cancel(self.pending)
        self.stop_process()
