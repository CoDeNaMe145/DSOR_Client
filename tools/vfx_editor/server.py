#!/usr/bin/env python3
"""VFX editor: edit the game's effects and spell sequences, previewed live.

Usage: python3 tools/vfx_editor/server.py [--assets DIR] [--port 8091]
then open http://localhost:8091 (the page, editor.js, explains itself).

Edits are kept in vfx-overrides/ (see tools/vfx_apply.py) and applied to the
assets at once; the preview client (DSOR_VFX_PREVIEW, client/src/skills.rs)
reloads what changed and plays it again. Python standard library only.
"""
import argparse, json, os, subprocess, sys, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlparse

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.dirname(HERE))
import vfx_apply  # noqa: E402
from fix_materials_io import read_glb  # noqa: E402

CLASSES = ["warrior", "mage", "ranger", "dwarf"]
STATIC = {"/": "index.html", "/editor.js": "editor.js", "/editor.css": "editor.css"}
TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8",
         ".png": "image/png", ".json": "application/json"}


class Editor:
    def __init__(self, assets, overrides, live):
        self.assets, self.overrides, self.live = assets, overrides, live
        self.lock = threading.Lock()
        self.rev = 0
        self.preview = None  # the last {kind, name, every}
        self.client = None
        os.makedirs(live, exist_ok=True)

    # -- reading ----------------------------------------------------------------
    def sequences(self):
        return json.load(open(os.path.join(self.assets, "skills", "sequences.json")))

    def pristine_sequences(self):
        """The conversion's sequences (sequences.json.orig while edits are applied)."""
        path = os.path.join(self.assets, "skills", "sequences.json")
        mark = path + ".vfx"
        applied = os.path.exists(mark) and os.path.getmtime(mark) >= os.path.getmtime(path)
        return json.load(open(path + ".orig")) if applied and os.path.exists(path + ".orig") else self.sequences()

    def seq_edits(self):
        p = os.path.join(self.overrides, "sequences.json")
        return json.load(open(p)) if os.path.exists(p) else {}

    def catalog(self):
        skills = json.load(open(os.path.join(self.assets, "skills", "skills.json")))
        seqs = self.sequences()
        out_skills = []
        for s in skills.values():
            names = set(s.get("execute", {}).values()) | set(s.get("pre", {}).values()) | set(s.get("post", {}).values())
            names |= {s.get("impact", ""), s.get("shifted", "")}
            b = s.get("bullet") or {}
            names |= {b.get("loop", ""), b.get("impact", ""), b.get("death", "")}
            names = sorted(n for n in names if n and n in seqs)
            if s.get("class") or names:
                out_skills.append({"id": s["id"], "class": s.get("class", ""), "type": s.get("type", ""), "seqs": names})
        out_skills.sort(key=lambda s: (s["class"] == "", s["class"], s["id"]))
        effects = set()
        for q in seqs.values():
            for t in q.get("tracks", []):
                if t.get("kind") == "fx" and t.get("graphics"):
                    effects.add(t["graphics"])
        for d in os.listdir(self.assets):
            if d.startswith("effects") and os.path.isdir(os.path.join(self.assets, d)):
                for f in os.listdir(os.path.join(self.assets, d)):
                    if f.endswith(".glb"):
                        effects.add(f"{d}/{f[:-4]}")
        effects = sorted(e for e in effects if os.path.exists(os.path.join(self.assets, e + ".glb")))
        tex_dir = os.path.join(self.assets, "textures", "effects")
        textures = sorted(f"textures/effects/{f}" for f in os.listdir(tex_dir) if f.endswith(".png") and not f.endswith("_nrm.png")) if os.path.isdir(tex_dir) else []
        edited_models = sorted(vfx_apply.model_edits(self.overrides))
        return {"skills": out_skills, "effects": effects, "textures": textures, "classes": CLASSES,
                "edited": {"effects": edited_models, "sequences": sorted(self.seq_edits())},
                "sequence_names": sorted(seqs)}

    def model_doc(self, rel, original=False):
        path = os.path.join(self.assets, rel + ".glb")
        doc, _ = read_glb(path)
        if original and vfx_apply.MARK in doc.get("asset", {}).get("extras", {}) and os.path.exists(path + ".orig"):
            doc, _ = read_glb(path + ".orig")
        return doc

    def effect(self, rel):
        cur, orig = self.model_doc(rel), self.model_doc(rel, original=True)
        base = os.path.dirname(os.path.join(self.assets, rel))

        def texture(doc, m):
            t = m.get("pbrMetallicRoughness", {}).get("baseColorTexture")
            if t is None:
                return None
            uri = doc["images"][doc["textures"][t["index"]]["source"]].get("uri", "")
            return os.path.relpath(os.path.normpath(os.path.join(base, uri)), self.assets).replace(os.sep, "/")

        parents = {c: i for i, n in enumerate(cur.get("nodes", [])) for c in n.get("children", [])}
        onames = {n.get("name"): n for n in orig.get("nodes", [])}
        omats = {m.get("name"): m for m in orig.get("materials", [])}
        nodes = []
        for i, n in enumerate(cur.get("nodes", [])):
            mats = []
            if "mesh" in n:
                for prim in cur["meshes"][n["mesh"]].get("primitives", []):
                    if "material" not in prim:
                        continue
                    m = cur["materials"][prim["material"]]
                    ex = m.get("extras", {})
                    om = omats.get(m.get("name"), m)
                    mats.append({"name": m.get("name"), "shader": ex.get("nebula_shader", ""), "state": ex.get("dsor_state", ""),
                                 "alpha": m.get("alphaMode", "OPAQUE"), "texture": texture(cur, m), "texture_orig": texture(orig, om)})
            extras = n.get("extras", {})
            if not extras and not mats:
                continue
            nodes.append({"name": n.get("name"), "parent": cur["nodes"][parents[i]].get("name") if i in parents else None,
                          "extras": extras, "original": onames.get(n.get("name"), {}).get("extras", {}), "materials": mats})
        anims = [{"name": a.get("name"), "length": max((cur["accessors"][s["input"]].get("max", [0])[0] for s in a.get("samplers", [])), default=0)}
                 for a in cur.get("animations", [])]
        return {"path": rel, "nodes": nodes, "animations": anims, "edited": rel in vfx_apply.model_edits(self.overrides)}

    # -- writing ----------------------------------------------------------------
    def save_effect(self, rel, nodes, materials):
        """nodes: {name: whole extras as edited}; materials: {name: texture}."""
        orig = self.model_doc(rel, original=True)
        onodes = {n.get("name"): n.get("extras", {}) for n in orig.get("nodes", [])}
        edit = {"nodes": {}, "materials": {}}
        for name, extras in nodes.items():
            o = onodes.get(name, {})
            diff = {}
            for k in set(o) | set(extras):
                if k not in extras:
                    diff[k] = None
                elif isinstance(extras[k], dict) and isinstance(o.get(k), dict):
                    d = {kk: vv for kk, vv in extras[k].items() if o[k].get(kk) != vv}
                    if d:
                        diff[k] = d
                elif o.get(k) != extras[k]:
                    diff[k] = extras[k]
            if diff:
                edit["nodes"][name] = diff
        omats = {m.get("name"): m for m in orig.get("materials", [])}
        info = {m["name"]: m for n in self.effect(rel)["nodes"] for m in n["materials"]}
        for name, tex in materials.items():
            if name in omats and tex and tex != info.get(name, {}).get("texture_orig"):
                edit["materials"][name] = {"texture": tex}
        path = os.path.join(self.overrides, "models", rel + ".json")
        os.makedirs(os.path.dirname(path), exist_ok=True)
        if edit["nodes"] or edit["materials"]:
            json.dump({k: v for k, v in edit.items() if v}, open(path, "w"), indent=1, sort_keys=True)
            vfx_apply.apply_model(self.assets, rel, edit)
        else:
            if os.path.exists(path):
                os.remove(path)
            vfx_apply.apply_model(self.assets, rel, None)
        self.bump([rel + ".glb"])

    def save_sequence(self, name, seq):
        edits = self.seq_edits()
        pristine = self.pristine_sequences()
        if seq is None or seq == pristine.get(name):
            edits.pop(name, None)
        else:
            edits[name] = seq
        os.makedirs(self.overrides, exist_ok=True)
        path = os.path.join(self.overrides, "sequences.json")
        if edits:
            json.dump(edits, open(path, "w"), indent=1, sort_keys=True)
        elif os.path.exists(path):
            os.remove(path)
        vfx_apply.apply_sequences(self.assets, edits)
        self.bump(["skills/sequences.json"])

    def bump(self, reload=(), order=None):
        with self.lock:
            if order:
                self.preview = order
            if not self.preview:
                return
            self.rev += 1
            doc = dict(self.preview, rev=self.rev, reload=list(reload))
            tmp = os.path.join(self.live, "preview.json.tmp")
            json.dump(doc, open(tmp, "w"))
            os.replace(tmp, os.path.join(self.live, "preview.json"))

    def status(self):
        p = os.path.join(self.live, "status.json")
        try:
            s = json.load(open(p))
        except (OSError, ValueError):
            s = {}
        s["running"] = self.client is not None and self.client.poll() is None
        s["sent"] = self.rev
        return s

    def launch(self, cls):
        if self.client is not None and self.client.poll() is None:
            self.client.terminate()
        exe = os.path.join(REPO, "target", "release", "dsor-client" + (".exe" if os.name == "nt" else ""))
        if not os.path.exists(exe):
            raise RuntimeError("build the client first: cargo build --release -p dsor-client")
        env = dict(os.environ, DSOR_VFX_PREVIEW=self.live, DSOR_ASSETS=self.assets)
        self.client = subprocess.Popen([exe, "debug", "--play", str(cls), "0"], cwd=REPO, env=env,
                                       stdout=open(os.path.join(self.live, "client.log"), "w"), stderr=subprocess.STDOUT)
        if self.preview:
            self.bump()


class Handler(BaseHTTPRequestHandler):
    editor = None

    def log_message(self, *a):
        pass

    def send(self, code, body, ctype="application/json"):
        if not isinstance(body, bytes):
            body = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        u = urlparse(self.path)
        q = {k: v[0] for k, v in parse_qs(u.query).items()}
        ed = self.editor
        try:
            if u.path in STATIC:
                f = os.path.join(HERE, STATIC[u.path])
                return self.send(200, open(f, "rb").read(), TYPES[os.path.splitext(f)[1]])
            if u.path.startswith("/asset/"):
                rel = os.path.normpath(unquote(u.path[len("/asset/"):]))
                if rel.startswith("..") or os.path.isabs(rel):
                    return self.send(403, {"error": "outside the assets"})
                f = os.path.join(ed.assets, rel)
                return self.send(200, open(f, "rb").read(), TYPES.get(os.path.splitext(f)[1], "application/octet-stream"))
            if u.path == "/api/catalog":
                return self.send(200, ed.catalog())
            if u.path == "/api/effect":
                return self.send(200, ed.effect(q["path"]))
            if u.path == "/api/sequence":
                cur = ed.sequences().get(q["name"])
                return self.send(200, {"name": q["name"], "seq": cur, "original": ed.pristine_sequences().get(q["name"]),
                                       "edited": q["name"] in ed.seq_edits()})
            if u.path == "/api/status":
                return self.send(200, ed.status())
            return self.send(404, {"error": "not found"})
        except Exception as e:  # noqa: BLE001 -- shown in the page
            return self.send(500, {"error": f"{type(e).__name__}: {e}"})

    def do_POST(self):
        u = urlparse(self.path)
        ed = self.editor
        try:
            body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
            if u.path == "/api/effect":
                ed.save_effect(body["path"], body.get("nodes", {}), body.get("materials", {}))
            elif u.path == "/api/effect/revert":
                ed.save_effect(body["path"], {}, {})
            elif u.path == "/api/sequence":
                ed.save_sequence(body["name"], body.get("seq"))
            elif u.path == "/api/preview":
                ed.bump(order={"kind": body["kind"], "name": body["name"], "every": float(body.get("every", 3))})
            elif u.path == "/api/launch":
                ed.launch(int(body.get("class", 1)))
            else:
                return self.send(404, {"error": "not found"})
            return self.send(200, {"ok": True, "status": ed.status()})
        except Exception as e:  # noqa: BLE001
            return self.send(500, {"error": f"{type(e).__name__}: {e}"})


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--assets", default=os.path.join(REPO, "assets"))
    ap.add_argument("--overrides", default=os.path.join(REPO, "vfx-overrides"))
    ap.add_argument("--port", type=int, default=8091)
    a = ap.parse_args()
    assets = os.path.realpath(a.assets)
    Handler.editor = Editor(assets, a.overrides, os.path.join(REPO, ".vfx-editor"))
    try:
        server = ThreadingHTTPServer(("127.0.0.1", a.port), Handler)
    except OSError:
        sys.exit(f"Port {a.port} is busy: is the editor already open? Otherwise use --port <other>.")
    print(f"VFX editor: http://localhost:{a.port}  (assets {assets})")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        if Handler.editor.client is not None:
            Handler.editor.client.terminate()


if __name__ == "__main__":
    main()
