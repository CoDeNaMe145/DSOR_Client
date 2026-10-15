#!/usr/bin/env python3
"""Apply the VFX editor's edits (vfx-overrides/) to the converted assets.

Usage: vfx_apply.py <assets> [<overrides dir>]        (default: vfx-overrides)

The edits live outside the assets, so a new conversion does not lose them (run
this last; convert_assets.sh does) and they can be shared through git:

  vfx-overrides/models/<asset path>.json   an effect model's edits:
      {"nodes":     {"<node name>": {"<extras key>": value, ...}},
       "materials": {"<material name>": {"texture": "<asset path of a png>",
                                          "extras": {...}}}}
      an extras value replaces the converted one, except dicts (dsor_emitter,
      dsor_shader, dsor_vector...), merged key by key; null removes the key
  vfx-overrides/sequences.json             {"<sequence name>": <whole sequence>}
      (the skills/sequences.json format: length, repeat, tracks)

The converted files themselves are edited (as every tool of tools/ does), from a
pristine copy kept beside them (<file>.orig): applying twice, or after an edit
was taken back, always starts from the conversion's output. A file the
conversion rewrote has no "dsor_vfx" mark, so its copy is renewed first.
"""
import json, os, shutil, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fix_materials_io import read_glb, write_glb  # noqa: E402

MARK = "dsor_vfx"


def merge(base, edit):
    """base's extras with edit applied (see the module doc)."""
    out = dict(base)
    for k, v in edit.items():
        if v is None:
            out.pop(k, None)
        elif isinstance(v, dict) and isinstance(out.get(k), dict):
            out[k] = {**out[k], **v}
        else:
            out[k] = v
    return out


def pristine(path, marked):
    """The conversion's own file: its .orig copy, renewed when the file is new."""
    orig = path + ".orig"
    if not marked or not os.path.exists(orig):
        shutil.copyfile(path, orig)
    return orig


def apply_model(assets, rel, edit):
    """Rewrite <assets>/<rel>.glb from its pristine copy with `edit` (None: none)."""
    path = os.path.join(assets, rel + ".glb")
    if not os.path.exists(path):
        return False
    doc, _ = read_glb(path)
    marked = MARK in doc.get("asset", {}).get("extras", {})
    if edit is None and not marked:
        return False
    doc, rest = read_glb(pristine(path, marked))
    if edit:
        nodes = {n.get("name"): n for n in doc.get("nodes", [])}
        for name, e in edit.get("nodes", {}).items():
            if name in nodes:
                nodes[name]["extras"] = merge(nodes[name].get("extras", {}), e)
        materials = {m.get("name"): m for m in doc.get("materials", [])}
        for name, e in edit.get("materials", {}).items():
            m = materials.get(name)
            if m is None:
                continue
            if e.get("extras"):
                m["extras"] = merge(m.get("extras", {}), e["extras"])
            if e.get("texture"):
                uri = os.path.relpath(os.path.join(assets, e["texture"]), os.path.dirname(path)).replace(os.sep, "/")
                images = doc.setdefault("images", [])
                textures = doc.setdefault("textures", [])
                image = next((i for i, im in enumerate(images) if im.get("uri") == uri), None)
                if image is None:
                    images.append({"uri": uri})
                    image = len(images) - 1
                texture = next((i for i, t in enumerate(textures) if t.get("source") == image), None)
                if texture is None:
                    textures.append({"source": image, "sampler": 0} if doc.get("samplers") else {"source": image})
                    texture = len(textures) - 1
                m.setdefault("pbrMetallicRoughness", {})["baseColorTexture"] = {"index": texture}
        doc.setdefault("asset", {}).setdefault("extras", {})[MARK] = True
    write_glb(path, doc, rest)
    return True


def apply_sequences(assets, edits):
    path = os.path.join(assets, "skills", "sequences.json")
    if not os.path.exists(path):
        return 0
    # The client reads every entry as a sequence: the mark is a file beside it,
    # older than sequences.json once export_skills.py has rewritten it.
    mark = path + ".vfx"
    marked = os.path.exists(mark) and os.path.getmtime(mark) >= os.path.getmtime(path)
    if not edits and not marked:
        return 0
    seqs = json.load(open(pristine(path, marked)))
    seqs.update(edits)
    with open(path, "w") as f:
        json.dump(seqs, f, separators=(",", ":"))
    if edits:
        with open(mark, "w") as f:
            json.dump(sorted(edits), f)
    elif os.path.exists(mark):
        os.remove(mark)
    return len(edits)


def model_edits(overrides):
    root = os.path.join(overrides, "models")
    out = {}
    for dirpath, _dirs, files in os.walk(root):
        for name in files:
            if name.endswith(".json"):
                full = os.path.join(dirpath, name)
                out[os.path.relpath(full, root)[:-5].replace(os.sep, "/")] = json.load(open(full))
    return out


def apply_all(assets, overrides):
    """Every edit, and every file an edit was taken back from, rewritten."""
    edits = model_edits(overrides)
    done = 0
    for rel, edit in edits.items():
        done += apply_model(assets, rel, edit)
    # Models whose edit was deleted: back to their conversion's output.
    for dirpath, _dirs, files in os.walk(assets):
        for name in files:
            if name.endswith(".glb.orig"):
                rel = os.path.relpath(os.path.join(dirpath, name), assets)[: -len(".glb.orig")].replace(os.sep, "/")
                if rel not in edits:
                    apply_model(assets, rel, None)
    seq_path = os.path.join(overrides, "sequences.json")
    seqs = apply_sequences(assets, json.load(open(seq_path)) if os.path.exists(seq_path) else {})
    return {"models": done, "sequences": seqs}


if __name__ == "__main__":
    assets = sys.argv[1]
    overrides = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "vfx-overrides")
    print(apply_all(assets, overrides))
