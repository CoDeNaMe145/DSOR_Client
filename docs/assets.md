# Converted game assets

The client renders the 2018 client's own data (rel 206.7), converted to glTF 2.0
and PNG by [DSO_Godot](https://github.com/DSO-Reconstruction/DSO_Godot). The
data belongs to the publisher, so it is generated locally and never committed:
`assets/` at the workspace root is gitignored and is normally a symlink to a
folder outside the repository.

## Producing it

```sh
# <export_win32> is the unpacked client tree (models/, meshes/, maps/, textures/, ...):
# game-data/export_win32 once the account screen has downloaded it (docs/game-data.md)
tools/convert_assets.sh game-data/export_win32 ~/Documents/Drakensang/dsor_assets
ln -sfn ~/Documents/Drakensang/dsor_assets assets
```

The script clones DSO_Godot if it is not given one, builds its `.crn`
transcoder, then runs `batch_export.py` (models), `split_character.py` (the two
player skeletons) and `export_maps.py` (maps). Logs and a per-model report land
in `<out>/logs/`. On a 24-core machine the whole run takes a few minutes; the
2018 graphics data comes to about 2.8 GB; decoded audio adds to this.

Natively the client reads `<workspace>/assets` (override with the `DSOR_ASSETS`
environment variable). In the browser it fetches `assets/...` relative to the
page, so the web server must serve the same tree next to `index.html`.

### Audio preparation

The converter runs `tools/export_audio.py` to prepare the original audio and
`tools/export_audio_runtime.py` to bind game events and map audio to those samples.
Install [vgmstream-cli](https://github.com/vgmstream/vgmstream/releases)
(tested with r2117), including its companion libraries. Put it on `PATH`, set
`DSOR_VGMSTREAM` to the executable, or unpack it into `.local/vgmstream/`.
The Windows conversion wrapper uses the same exporter. For an existing asset
pack, run only:

```sh
python tools/export_audio.py <export_win32> <assets> --vgmstream /path/to/vgmstream-cli
python tools/export_audio_runtime.py <export_win32> <assets>
```

Some client exports contain stripped banks with no events or samples (including
the bank used by the lava hound's footsteps). If a complete older export is
available, pass `--fallback-root <older_export_win32>` to `export_audio.py`, or
set `DSOR_AUDIO_FALLBACK_ROOT` when running the full converter. Only completely
empty banks with matching original bank GUIDs are restored; event-only banks
remain intact. The source trees are read-only, and the catalog records each
restored bank's provenance. Use the same fallback on subsequent conversions;
the local Windows wrapper already supplies it.

Every source file under `export_win32/audio` is retained in `audio/source/`,
including event-only banks, `eventbankmapping.txt`, and animation `hotspots.xml`.
Embedded FSB5 samples are decoded to PCM16 WAVs, at their original sample rate
and channel count, under `audio/samples/<original-bank-path>/<sample-index>.wav`.
Each WAV contains one complete pass; decoding adds no repeats or fade-out.
Available sample loop points are retained in the WAV's `smpl` chunk and catalog.
Numbered filenames preserve duplicate variants and avoid unsafe source names.

`audio/catalog.json` contains source-relative file keys, hashes, asset-relative
paths, a `banks` map with each bank's `samples`, and an `event_banks` map from
event names to bank names. Sample entries include the original name, index,
sample rate, channel count, frame count, codec, and decoder loop metadata.
That catalog identifies banks. The runtime exporter additionally follows FMOD
event, timeline, instrument and waveform GUID references across banks, without
following shared buses into unrelated events. It writes `audio/runtime.audio.json`
with sample bindings, loop/polyphony metadata, and every DB4 map's music, ambient
beds, positioned sound objects, music areas, layers and event gates. Cue names
with `event:/` prefixes are normalized. `audio/runtime-report.json` lists map
cues that cannot be resolved; the client skips them. With the older-bank fallback,
the local pack contains 4,619 resolved events across 391 maps, with 14 distinct
unresolved map cues (3,431 events and 41 unresolved map cues before restoration).

The runtime export also reads `audio/hotspots.xml` and NAX3 sound markers. The
marker category is the hotspot index, so silent slots and out-of-order markers
must be retained. Skin and character-set bindings select the original NPC,
creature and mount sounds. Run this stage after model conversion so character
set sidecars are available. Cosmetic sets without their own binding use the
most common binding for the same model and movement clip; explicit silent bindings stay
silent. The current pack has movement bindings for 219 of 252 mount definitions;
the remaining definitions, including test mounts and newer models, lack authored
movement markers or bindings and remain silent. A binding is playable only when
its event resolves to extracted samples: inspect `unresolved_animation_events`
in `audio/runtime-report.json` as well as the animation markers when diagnosing
missing footsteps.

Reruns reuse unchanged banks after checking their WAV headers and sizes; missing
or truncated WAVs are regenerated. `--force` decodes everything again, and
`--workers N` controls parallel decoding (default 4). Missing input, missing
decoder, malformed banks, and incomplete decoding fail the conversion rather
than publishing a successful catalog. Audio stays inside the gitignored asset
tree used by native and web builds.

The client uses one stereo output stream and 64 reusable voices: two music
voices for crossfades, fourteen ambience voices, and forty-eight effect voices.
PCM16 sample buffers are shared across simultaneous casts. A 128 MiB LRU cache
counts pending loads against its budget, allows four concurrent loads, and never
evicts a playing buffer. Current-class spell variants are warmed within a 32 MiB
allowance. Effect requests are capped at 128, expire after 250 ms, and have a
35 ms per-emitter/event cooldown plus the authored event's voice limit (capped
at eight). Saturation drops excess effects; it cannot take music slots. The
mixer resamples to 48 kHz, interpolates samples, fades starts/stops, and limits
the combined output to prevent clipping. Positioned sounds attenuate to silence
at 45 metres and pan relative to the camera, using the player as listener.

Music and ambience continue throughout map loading, including gaps with no map
or player. Destination background samples preload behind the overlay; once the
map is ready, each outgoing bed fades over 1.5 seconds as its replacement starts.
Music also blends into ambience-only destinations. Four ambience slots are
reserved for overlapping background beds, so busy sound objects cannot block a
transition. Enabled music areas override the default track. Inactive or gated
sound objects stay silent, and only the nearest ten audible objects receive voices.
Non-looping ambient cues recur with a 5–11 second gap. Sequence sound tracks
preserve cue names, volumes and loop lifetime, including remote actors; repeated
requests for an existing owned loop reuse its voice. Master, music, ambience,
effects and mute controls live in Settings → Audio and follow Apply/Cancel.
HUD buttons use the original click/close cues as one-shots, overriding authored
loop flags so an ownerless UI voice always finishes and releases its slot.
Animation markers trigger spatial one-shots for NPC actions (including smiths'
hammer strikes), decorative actors, footsteps, creatures and mounts. They follow
the active clip and animation speed, including NPC pose LOD, and skip stale
markers after hitches or teleports. The rider does not duplicate mount sounds.
These sounds share the bounded effects pool and PCM cache; nearby clips warm
their samples before the next marker. Equipment sounds follow confirmed local
inventory slot changes, once per swap, with separate armor, weapon and jewelry
cues. Initial inventory snapshots and unchanged closing packets stay silent.
Browser audio resumes on the first
click or key press.

This is a sample-based player, not an FMOD Studio interpreter: an event's reachable
samples currently serve as variants. Layered instrument timing, parameter-driven
music stems, authoring DSP/reverb and scripted activation of disabled sound
objects are not reconstructed. Original banks and map reverb names are preserved
for extending those behaviours. Music stays in the bounded PCM cache rather than
streaming from disk; oversized samples are skipped with a warning.

To browse the extracted soundtrack, run `python tools/collect_music.py <assets>`.
It groups byte-identical music samples, gives the WAVs descriptive filenames,
and writes `audio/music/music.m3u8` for tracks lasting at least 30 seconds.
`all_music.m3u8` includes shorter cues and jingles; `tracks.csv` lists durations
and source banks. Change the main playlist cutoff with `--min-seconds`.
The collection uses hard links where supported, so it needs no second copy of
the audio; edits to a linked WAV also affect its original sample.

### Block-compressed textures (KTX2)

The last conversion step (`dsor-ktx2`, crates/texture-pack) writes beside every
PNG a model references a `.ktx2`: the game's own DXT1/DXT3/DXT5 blocks and
mipmaps when the PNG is still that DDS decoded, otherwise BC7 with a mip chain
(normal maps rebuilt from bump maps, roughness maps, dye masks), zstd
compressed. `textures/ktx2.list` names the packed PNGs. Textures whose size is
not a multiple of 4 stay PNG only.

The client points a model at the `.ktx2` only when the GPU samples BC (every
native backend, desktop WebGPU and WebGL2); other GPUs keep the PNGs, and
`DSOR_FLAGS=pngtextures` forces them for comparison. Rerun it alone after a
tool rewrites textures (a `.ktx2` older than its PNG is redone):

```sh
cargo run --release -p dsor-texture-pack --bin dsor-ktx2 -- <export_win32> <assets>
```

### Updating model and character data

`tools/restore_pivots.py <export_win32>/models <assets> <DSO_Godot>` restores
the source models' rotation and scale centers after the other model
postprocessors. The full conversion runs it automatically; run it once for
existing assets too. Omitting these centers made the travel stone's pulsing
glow slide away from the stone as it grew. Static corrections are baked into
translations; animated corrections use glTF transform helpers, retaining the
authored animation and exact interpolation. This applies to native, browser
and editor rendering without a client rebuild. The tool is idempotent and
accepts optional model paths (without extensions) for a targeted repair.

`tools/restore_skin_fragments.py <export_win32> <assets> <DSO_Godot>` puts back
the skin fragments DSO_Godot leaves out. Nebula splits a skin whose skeleton
exceeds a 72-joint palette into several primitive groups, and a skin node draws
every group it lists; DSO_Godot exported only group 0. Balor lost his right
wing, arm and leg; Khalys, Sigrismarr, Mhorblik, Sharr Kharab and 34 other
models lost parts or showed an unskinned copy frozen in the bind pose. The full
conversion runs it before the other postprocessors; run it once for existing
assets too (39 models, 90 skin nodes). It is idempotent and accepts optional
model names.

For a production `dso_server` checkout that contains
`gameplay/pathengine_navigation.py`, export navigation with its reader:

```sh
python tools/export_navmesh.py <export_win32> <assets> --server <dso_server>
```

This writes `maps/<map>.nav.bin` in world X/Y/Z coordinates. The original
`EXPERIMENTAL` reader remains supported when `--server` is omitted.

Equipment mappings must use the same static database as the server. For
release261, regenerate them after converting the older graphics pack:

```sh
python tools/equipment_visuals.py <dso_server>/data/db/static.db4 <assets>
# Or regenerate all character tables with the database explicitly selected:
python tools/character_tables.py <export_win32> <assets> --database <dso_server>/data/db/static.db4
```

This resolves item skins, `EquipmentSkinId` references, randomized appearances
and transmog IDs. Inventory exports also regenerate the mappings with their
`--database` selection. The lookup cannot supply geometry absent from the
converted graphics pack; those parts are skipped and base clothing remains.
Equipment dyes require the masks embedded by `tools/embed_character_masks.py`,
which the full conversion script already runs.

Clothing coverage comes from `tables/gfxskintags.bin` (version 2). Regenerate
the part catalogs of an existing asset pack with:

```sh
python tools/skin_coverage.py <export_win32> <assets>
```

`character_tables.py` also performs this step. Each `parts.json` now contains
`parts` (available mesh names) and `tags` (per-mesh `mask` and `layer`). Higher
visible layers hide overlapping regions on lower layers. Male/female meshes
use their own rules, so exposed skin is preserved. Legacy name-only catalogs
still load, but need regeneration to enable coverage. Meshes without metadata
remain visible. This workspace's catalogs have been regenerated.

## What is in it

DB4 travel points, workbenches, chests and other state objects need prop sidecars
in addition to the geometry manifests. Regenerate them for an existing asset pack:

```sh
cargo run --release -p dsor-level-data --bin dsor-levels -- props --assets assets --db-root <dso_server>/data/maps --static-db <dso_server>/data/db/static.db4
```

The full converter runs this export too. `maps/<map>.props.json` retains GUIDs,
authored transforms, quest visibility and event-set declarations;
`props/catalog.props.json` supplies models for server-created props. Editor
builds regenerate the edited map's prop sidecar from its effective DB4 records.
Source databases are read-only inputs. Both native and browser clients load
these files through the asset server.

The client previews the authored default state offline, including quest scenery
and negative-only event sets (Kingshill's decorative workbenches). Online `EventUpdate` switches
event variants, while `PropInfo`/`NewProp`, `PropUpdate`, and `DiscardProp` bind,
update and remove props. Only the selected model state group is drawn. Quest
props with `QuestControlled` and no `ShowAlways` wait for a server announcement
when connected to a server.
This supplies visuals and state synchronization; crafting and travel service
interactions still require their gameplay UI/controller implementations.

| Path | What |
|---|---|
| `<category>/<model>.glb` | one model per client `.n3`: `models/t001_hub/arch_house_01.n3` -> `t001_hub/arch_house_01.glb`. Same sub-path, extension swapped. |
| `<category>/<model>.fx.json` | particle emitter and render-state sidecar of a `.glb`, embedded into it by `tools/embed_emitters.py` |
| `textures/<category>/<name>_color.png` etc. | textures, referenced by the `.glb` files through **relative** URIs (`../textures/t001_hub/mat_wood_01_color.png`). Never move a `.glb` out of the tree. Bump maps become `_bump_nrm.png` tangent-space normal maps, spec maps `_spec_rough.png`. |
| `characters/uniskel/`, `characters/uniskel_dwarf/` | the player characters split up: `__animations.glb` (skeleton + every clip), `parts/*.glb`, `outfits.json` (outfit -> parts), `variations.json` (body-shape variations). |
| `characters/item_skins.json` | release-matched `items` and `appearances` maps to skin part names; generated by `tools/equipment_visuals.py`. |
| `characters/<skeleton>/parts.json` | available parts and authored clothing coverage masks/layers, generated by `tools/skin_coverage.py` or `tools/character_tables.py`. |
| `maps/<map>.map.json` | one placement manifest per client `.map` (format below). |
| `maps/<map>.ambience.json` | the level's global light, post effect (fog, bloom, saturation, vignette) and placed lights (`tools/export_ambience.py`). |
| `audio/source/`, `audio/samples/`, `audio/catalog.json` | original audio banks and metadata, decoded WAV samples, and lookup catalog (`tools/export_audio.py`). |
| `audio/runtime.audio.json`, `audio/runtime-report.json` | game event/sample bindings, audio for every map, and unresolved cue names (`tools/export_audio_runtime.py`). |
| `textures/**/<name>.cube.png` | cube maps, six faces stacked top to bottom (+X -X +Y -Y +Z -Z), `tools/embed_textures.py`. |
| `maps/build_map.gd` | DSO_Godot's Godot-side builder; ignored here. |
| `logs/` | conversion logs and `export_report.json`. |

Every `.glb` has one scene (`Scene0`); the client loads it as
`GltfAssetLabel::Scene(0)`.

## Map manifest (`maps/<map>.map.json`)

```json
{
  "map": "a0200_kingscity",
  "size": [61, 11, 62],                 // the map's grid size, in cells
  "center": [121.5, 13.399, 71.3854],    // bounding-box centre, game frame
  "extents": [122.5, 22.399, 123.5315],  // bounding-box half extents
  "models": ["t001_hub/tile_wall_corner_02", "..."],
  "groups": [{"name": "...", "type": 0, "parent": -1}],
  "instances": [
    {"m": 0, "p": [174.0, 8.0, 6.0], "r": [0.0, 1.0, 0.0, 0.0],
     "s": [1.0, 1.0, 1.0], "n": "", "g": -1, "col": true}
  ],
  "nav_blockers": [[[x, z], "..."]]
}
```

- `models` is the map's list of distinct models, each a path relative to the
  asset root without extension: the model file is `<models[i]>.glb`.
- Each instance places `models[m]` at position `p`, rotation `r` (a unit
  quaternion, **x, y, z, w**) and scale `s`; the transform is
  `T(p) * R(r) * S(s)`, applied to the model's own glTF scene. `n` is the
  instance's name (often empty), `g` its group index (-1 = none), `col` whether
  it collides.
- Instances without graphics (pure collision / logic) are dropped by the export.
- There is **no separate terrain**: the ground of every map is made of tile
  models (`*/tile_ground_*`, `*/tile_wall_*`, `*/tile_ramp_*`) placed like any
  other prop.

The manifest comes from the map's `TMPL` (template) and `INST` (instance)
sections: `templates[instance.templateIndex].gfxResId` names the `.n3`.

## Coordinate frame

The map placements are in the game's map frame, which is also the server's
"description frame" (creature and spawn-point positions in `experimental`'s
`dsor/mapdata.py` are `(x, elevation, y)` in it): Nebula3's right-handed, Y-up
frame, one unit = one metre, ground on X/Z, elevation on Y. glTF and Bevy are
right-handed Y-up too, and DSO_Godot writes the models without any axis
conversion, so a game position is a Bevy position unchanged. The client keeps
that mapping in exactly one place, `client/src/world/map.rs::game_to_bevy`, so any
later correction lands there. Rotations and scales are used as-is.

## Running the map viewer

The character-selection screen uses `maps/a0000_char.map.json` and its ambience,
plus `interface/charselectionwindow.ui.json`; the creation screen
`interface/chargenwindow.ui.json` and `premadepackagewindow.ui.json`.
`character_tables.py` exports `characters/selection.json` with `_Globals.CharGen*`
camera settings, item `SlotType` mappings, character hair/beard tables and the
creation screen's data and English texts (docs/ui.md, "The character screens").
To update an existing pack:

```sh
python tools/export_character_selection.py <export_win32>/db/static.db4 <assets>
```

The texts come from `<export_win32>/language/en`, found beside the database's
`db` folder; give `--language <export_win32>` for a database elsewhere (the
server's).

The source UI's `TemplateCharacter` subtree is cloned into four visible roster
rows at runtime. The original meshes supply the frame, nameplate, rotation
controls and normal/hover/pressed/disabled button artwork.

```sh
cargo run -p dsor-client --release -- a0200_kingscity      # Kingshill (default)
cargo run -p dsor-client --release -- a0001_start_tutorial_dun
cargo run -p dsor-client --release -- a0302_wildforest
# screenshot after loading, then exit:
cargo run -p dsor-client --release -- a0200_kingscity --screenshot
cargo run -p dsor-client --release -- a0200_kingscity --screenshot-to out.png --cam 120,60,130,120,10,70
```

On the web: `index.html?map=a0302_wildforest` (optionally `&cam=x,y,z,tx,ty,tz`).

## Known gaps of the 2018 conversion

See the run's `logs/`. With the 2018 client, every model, map and character
converted without a failure; what is missing is data the client itself does not
ship or features glTF cannot carry:

- 64 models are referenced by some map but absent from the 2018 data (mostly
  `m001_arena/*`, `e003_essence_desert/veg_grass_02`); those placements are
  skipped with a load error in the log.
- A couple of hundred textures referenced by models are absent from the data;
  those materials have no base-colour map.
- A few animation clips and mesh groups referenced by boss death models are
  missing from their `.nax3`/`.nvx2` files.
- What glTF cannot carry is put back by `tools/` after the conversion (step 5 of
  `convert_assets.sh`): Nebula render states and cull modes, emitters, shader
  animators, cube maps and second texture layers, the levels' light and
  post-effect settings. What the client draws of it, and what is still missing,
  is in `docs/fx.md`.
