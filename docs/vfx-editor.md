# VFX editor

Edit the game's spell effects and see them live in the client.

```sh
python3 tools/vfx_editor/server.py          # Windows: VfxEditor.cmd
```

then open <http://localhost:8091>. Python 3 (standard library only) and a built
client (`cargo build --release -p dsor-client`) are needed; the converted assets
are read from `assets/` (`--assets <dir>` for another folder).

## Using it

1. **Open preview** starts the client on the debug map, offline, with the class
   chosen beside it. It plays whatever you select, again every *Replay every*
   seconds, and replays within a second of each edit.
2. In the left panel pick a **skill** (its sequences are listed), a **sequence**,
   or an **effect** model. **Play** (or Space) replays the selection; the
   orange dot marks what you have edited; *Edited only* filters on it.
3. A **sequence** is a timeline at 25 frames a second: effect models (fx),
   lights, camera shakes, the caster's animation, sounds, the skill's phase.
   Drag a bar to move it, its ends to change its frames; click a track for its
   values (model, joint, placement, colour...). **+ Effect / + Light / + Shake**
   add a track; *Duplicate* / *Delete track* in the inspector.
   **Edit this effect model** opens the fx track's model.
4. An **effect model** lists its emitters and surfaces:
   - an *emitter*'s values and curves: particles per second, lifetime, spread,
     speed over the emission; size, spin, colour, opacity over a particle's
     life. Drag a curve's points (the two middle ones also move in time) or
     type the values; the dashed line is the game's own curve;
   - a *surface*'s shader values (intensity, emissive, scale...), its animators
     (keyframes over time), its UV scroll speed;
   - each material's **texture**: pick any of the game's effect textures.
   Other values are shown as JSON, editable too.
5. **Undo / Redo** (Ctrl+Z / Ctrl+Y) step through this session's edits;
   **Revert to game** drops every edit of the open sequence or effect.

## Where edits go

Nothing is lost on a reconversion: edits are kept in `vfx-overrides/`
(`models/<asset path>.json`, `sequences.json`; see its README) and applied over
the assets by `tools/vfx_apply.py` -- by the editor as you edit, by
`tools/convert_assets.sh` after a conversion. The converted file keeps a
pristine copy beside it (`<file>.glb.orig`, `sequences.json.orig`), so an edit
taken back restores the game's data exactly. Commit `vfx-overrides/` to share
your edits; the others get them with `python3 tools/vfx_apply.py assets`.

## How the preview works

`DSOR_VFX_PREVIEW=<dir>` (client/src/skills.rs, `vfx_preview`): the editor
writes `<dir>/preview.json` (`.vfx-editor/` in the repository) with what to play
and the files it changed; the client reloads those files, stops what was playing
and plays it again, and answers in `<dir>/status.json`. A skill is cast as a
real cast would be (its landing, ground effects and summons offline included);
a sequence plays on the player; an effect model plays alone 3 m ahead.

## Not yet

- New effects from scratch (for now: edit an existing sequence -- add fx
  tracks with any model -- or an existing model).
- Adding or removing emitters and surfaces in a model; editing meshes.
- A playhead in sync with the client (the timeline's is approximate).
