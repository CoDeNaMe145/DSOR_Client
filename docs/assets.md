# Converted game assets

The client renders the 2018 client's own data (rel 206.7), converted to glTF 2.0
and PNG by [DSO_Godot](https://github.com/DSO-Reconstruction/DSO_Godot). The
data belongs to the publisher, so it is generated locally and never committed:
`assets/` at the workspace root is gitignored and is normally a symlink to a
folder outside the repository.

## Producing it

```sh
# <export_win32> is the unpacked client tree (models/, meshes/, maps/, textures/, ...)
tools/convert_assets.sh ~/Downloads/DSOClient2018_rel_206-7-32bit/export_win32 \
                        ~/Documents/Drakensang/dsor_assets
ln -sfn ~/Documents/Drakensang/dsor_assets assets
```

The script clones DSO_Godot if it is not given one, builds its `.crn`
transcoder, then runs `batch_export.py` (models), `split_character.py` (the two
player skeletons) and `export_maps.py` (maps). Logs and a per-model report land
in `<out>/logs/`. On a 24-core machine the whole run takes a few minutes; the
2018 data comes to about 2.8 GB.

Natively the client reads `<workspace>/assets` (override with the `DSOR_ASSETS`
environment variable). In the browser it fetches `assets/...` relative to the
page, so the web server must serve the same tree next to `index.html`.

`tools/restore_pivots.py <export_win32>/models <assets> <DSO_Godot>` restores
the source models' rotation and scale centers after the other model
postprocessors. The full conversion runs it automatically; run it once for
existing assets too. Omitting these centers made the travel stone's pulsing
glow slide away from the stone as it grew. Static corrections are baked into
translations; animated corrections use glTF transform helpers, retaining the
authored animation and exact interpolation. This applies to native, browser
and editor rendering without a client rebuild. The tool is idempotent and
accepts optional model paths (without extensions) for a targeted repair.

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
that mapping in exactly one place, `client/src/map.rs::game_to_bevy`, so any
later correction lands there. Rotations and scales are used as-is.

## Running the map viewer

The character-selection screen uses `maps/a0000_char.map.json` and its ambience,
plus `interface/charselectionwindow.ui.json`. `character_tables.py` now exports
`characters/selection.json` with `_Globals.CharGen*` camera settings, item
`SlotType` mappings, and character hair/beard tables. To update an existing pack:

```sh
python tools/export_character_selection.py <export_win32>/db/static.db4 <assets>
```

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
