#!/usr/bin/env python3
"""Effect models: each animated node's clip starts where Nebula starts it.

Usage: fx_clip_starts.py <export_win32> <assets> <DSO_Godot> [model ...]
       (every model under <assets>/effects* when none given; a model is its path
       under assets without .glb, e.g. effects_playerskills/warrior_mightyswing_fast)

A model's animator node (n3 MANI) animates nodes from clips of one .nax3; a clip
starts at its startKeyIndex x keyDuration on the model's clock, not at 0.
EVIDENCE: AnimatorNodeInstance::Animate (0x7F7F60) samples a transform section
  at time - clip+0x4 x clip+0x10 (FUN_00812fc5's 4th argument): AnimClip's
  startKeyIndex and keyDuration in Nebula3's layout.
  UNVERIFIED: the clip layout is Nebula3's, not read from this build.
  warrior_mightyswing_fast: emitter_swirl and emitter_smoke clips start at key 3
  (3 x 40 ms = 0.12 s, when their particles start: StartDelay 0.12), the swoosh
  at 0. DSO_Godot writes every clip from 0: the emitters swept the swing before
  emitting and every particle came out where they stopped ("les 3 particules au
  meme endroit a la fin alors qu'ils devraient etre le long du swoosh").
This shifts the samplers of each animated node by its clip's start (keys before
it hold the first value, as a clip's constant pre-infinity). Idempotent: the
animation's extras record "dsor_clip_starts".
"""
import glob, os, sys

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))
from close_loops import read_glb, write_glb, read, append, compact  # noqa: E402

export, assets, godot = sys.argv[1], sys.argv[2], sys.argv[3]
sys.path.insert(0, godot)
from dsoexp import n3, nax  # noqa: E402


def starts_of(model):
    """{node name: start seconds} from the model's animator sections."""
    path = os.path.join(export, "models", model + ".n3")
    if not os.path.isfile(path):
        return {}
    out = {}
    clips_of = {}
    for nd in n3.load(path).nodes:
        for sec in nd.anim_sections or []:
            res, group, target = sec.get("anim_resource"), sec.get("anim_group"), sec.get("target_node")
            if not res or group is None or not target or sec.get("node_type") != 4:
                continue
            nax_path = os.path.join(export, "anims", res.split(":", 1)[-1])
            if nax_path not in clips_of:
                try:
                    c = nax.load(nax_path, load_keys=False)
                    clips_of[nax_path] = c if isinstance(c, list) else c.clips
                except Exception:
                    clips_of[nax_path] = []
            clips = clips_of[nax_path]
            if 0 <= int(group) < len(clips):
                c = clips[int(group)]
                start = c.start_key * c.key_duration / nax.TICKS_PER_SEC
                if start > 0:
                    out[target.rsplit("/", 1)[-1]] = start
    return out


def shift(model):
    glb = os.path.join(assets, model + ".glb")
    starts = starts_of(model)
    if not starts or not os.path.isfile(glb):
        return 0
    got = read_glb(glb)
    if got is None:
        return 0
    g, blob = got
    done = 0
    for an in g.get("animations", []):
        if (an.get("extras") or {}).get("dsor_clip_starts"):
            continue
        new_inputs = {}
        changed = False
        for ch in an["channels"]:
            name = g["nodes"][ch["target"]["node"]].get("name")
            start = starts.get(name)
            if not start:
                continue
            s = an["samplers"][ch["sampler"]]
            t = read(g, blob, s["input"])
            if t is None:
                continue
            if s["input"] not in new_inputs:
                new_inputs[s["input"]] = append(g, blob, t + start, "SCALAR", minmax=True)
            s["input"] = new_inputs[s["input"]]
            changed = True
        if changed:
            an.setdefault("extras", {})["dsor_clip_starts"] = {k: round(v, 4) for k, v in starts.items()}
            done += 1
    if done:
        blob = compact(g, blob)
        g["buffers"][0]["byteLength"] = len(blob) + (4 - len(blob) % 4) % 4
        write_glb(glb, g, blob)
    return done


if __name__ == "__main__":
    models = sys.argv[4:] or [
        os.path.relpath(f, assets)[:-4]
        for d in glob.glob(os.path.join(assets, "effects*"))
        for f in glob.glob(os.path.join(d, "**", "*.glb"), recursive=True)
    ]
    total = changed = 0
    for m in models:
        n = shift(m)
        if n:
            total += n
            changed += 1
    print(f"fx_clip_starts: {total} animation(s) shifted in {changed} model(s)")
