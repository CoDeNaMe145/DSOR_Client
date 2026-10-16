# Local 3D level editor

Run `LevelEditor.cmd` on Windows, or:

```sh
cargo run --release -p dsor-client -- --editor a0200_kingscity --assets assets --db-root <server>/data/maps
```

The editor is a native Rust application: Bevy renders the scene, egui draws the
editor panels, and `crates/level-data` owns SQLite DB4 access and persistent edits.
It has no Python or Tk dependency. Build the editor and headless composition tool:

```powershell
powershell -ExecutionPolicy Bypass -File tools/Build-LevelEditor.ps1
```

On other platforms use `cargo build --release -p dsor-client -p dsor-level-data`.
Run `target/release/dsor-client --editor`. The UI and GPU viewport share one native
window and communicate in process on every platform.
The editor opens maximized, with a normal resizable window when restored.

**Sources…** selects the converted assets directory, original map DB4 directory,
and override directory. Use the server's `data/maps` directory to edit the same
gameplay records it uses. This workspace defaults to `.local/dso_server/data/maps`
when available. Settings are local, in `.level-editor/project.json`.

## Editing

- Choose a map and press **Open**. The outliner combines geometry placements with
  all `_Instance_*` tables from its DB4, including NPCs, lights, exits, entries,
  spawn points, sequences, triggers, event sets, and level settings. Empty tables
  remain available in the table selector.
- Filter by table, name, model, or GUID. The default outliner shows spatial
  objects; choose a specific database table to inspect other records.
- The object selector shows rendered model thumbnails and a larger preview of
  the selected object. Previews load for visible rows and are shared between
  instances of the same model. NPC outfits have their own previews.
- The separate **Assets** tab browses all placeable `.glb` models under the
  configured assets folder, including models unused by the current level.
  Search by name or path, filter by folder, and inspect thumbnails and a larger
  preview. **Add to level** or double-click places the selected model
  in the current camera view (on the ground when it is nearby) and selects it.
  Placement is one undoable addition in the override layer; **Focus** frames it.
  **Refresh** rescans after assets are added or rebuilt; changing Sources
  switches the library to that directory. Animation-only support files are
  excluded. Scene filters and event state do not restrict the asset library.
- Click a model in the **3D viewport** or an outliner row to select. The viewport
  uses the client's models, materials, and character renderer. Selection is
  shared with the DB4 inspector and highlighted with a bounding box.
- Hold right mouse to look freely from the camera's current position, with
  WASD to fly and Q/E to descend/ascend; Shift increases speed. Middle-drag pans
  and the wheel moves forward/back. **Fly speed** sets movement speed in metres
  per second. **F** / **Focus** frames the selected object; **Frame map** frames
  the map once, without anchoring subsequent camera movement to it.
- **Grid** and **Helpers** toggle the grid and metadata markers independently.
  Both start off for a clean viewport. Selection handles remain visible.
- **Markers** starts on: green flag icons locate player entries and NPC/object/item
  spawns; blue portal icons locate teleport stones. Icons stay readable as you
  zoom, follow the active map event state, and work without model assets. Hover
  for the type and name, click to inspect the DB4 record, or double-click to focus.
  They are editor overlays and do not appear in the game camera preview.
- **Q** selects without transform handles, **W** moves along world axes,
  **E** rotates around world axes, and **R** scales
  along local axes. Drag a colored axis handle; scale's white center handle
  changes all axes together. Escape cancels the current gesture. Each completed
  gesture creates one undoable edit and updates the inspector immediately.
  Shortcuts work with a selected object in the viewport or outliner and leave
  text fields alone. Holding RMB gives fly controls priority. G/S remain aliases
  for move/scale.
- **Snap** is translation spacing in metres (`0` disables snapping). When enabled,
  rotation snaps to 15 degrees and scale to 0.1 increments. Hold Ctrl while
  dragging to bypass snapping. Ctrl+S/Z/Y/D save, undo, redo, and duplicate;
  Delete removes the selected object.
- Set X/Y/Z and press **Apply**. Y is elevation. The viewport gizmos
  edit geometry or the DB4 transform. Ground coordinates use the same X/Z frame
  as the client and server, without axis conversion.
- **Details** exposes common object fields with a model preview. **Apply**,
  saving, or selecting another object applies valid field edits.
  The **DB4 / JSON** section exposes every record field. Transforms and known color
  blobs display as `{"$f32": [...]}`; other binary fields retain exact
  `{"$blob": "HEX"}` values. **Apply fields** validates the record. Selecting
  another object or saving also applies valid pending inspector edits.
- **+ Add** opens the searchable asset library with thumbnails and a larger
  selected-model preview, or adds a record in the selected DB4 table. A model
  reference can still be typed when its source asset is unavailable. Fill in
  its template ID and required gameplay values in the inspector. **Duplicate**
  copies a placement or GUID-based DB4 record with a new identity. **Delete**
  records a deletion in the override layer. **Undo/Redo** operate on edits during
  the session; **Revert** removes a record's override, including a deletion.
- **Save** / Ctrl+S saves the authored layer. **Changes**
  lists additions, field changes, deletions, and unresolved targets.

The secondary **Map overview** tab shows placement markers, the cached navigation
outline, and original navigation blockers. Drag markers to move on X/Z;
right-drag pans and the wheel zooms. The 3D viewport updates live from the same
effective records. Models that are unavailable appear as editable wire boxes.
**Play** builds the effective files and opens the normal game client;
close and reopen that gameplay preview after edits.

## Fixed camera preview

The character-selection level shows **Fixed camera** above the viewport.
`a0000_char` has a fixed character-selection camera defined by the original
`_Globals.CharGen*` values in `characters/selection.json`, rather than a camera
row in its map DB4. The editor exposes it under the **camera** table.

Click its camera outline in 3D or use **Fixed camera**. A live monitor
appears in the viewport's top-right corner using the selected camera's position,
rotation, vertical FOV and clipping planes. The outline shows its viewing direction
and frustum. W moves the camera and E rotates it; the monitor updates during the
drag. The free-fly editor camera stays independent. Selecting another object hides
the monitor, and editor handles/grid stay out of the monitor's image.

**FieldOfView**, **NearClip**, and **FarClip** control the lens. **PreviewAspect**
sets the monitor's simulated game-window aspect ratio (16:9 by default); the game
itself uses the actual window dimensions. This previews scene framing, without
the character-selection UI or its dynamically spawned character. Lighting uses
the map's effective lighting with the same sun, ambient, local-light and
post-effect code as the game.

Camera changes use the same override history, Save, Undo, offline metadata cache,
and rebuild composition as other edits. **Build** applies the `a0000_char` camera
to the client catalog while preserving item slots, hair and beard definitions.
Other DB4 cameras keep their edits in the derived map DB4. Maps without a fixed
camera use the game's ordinary player-follow camera when played.

## Lighting

The **Lighting** tab lists placed lights and ambience records from the map DB4.
**+ Point** and **+ Spot** place a new light in view, three metres above the ground.
Select a light in the list or click its marker in 3D. W moves it and E aims a
spotlight. Selected lights show their range sphere or spot cone. Unselected light
markers appear while the Lighting tab is open or **Helpers** is enabled.

The Details dock provides RGB color swatches (with numeric HDR values), intensity,
range, spot cone angle, shadows, flicker, and event-set assignment. Press **Apply**
to preview the edited values together as one undoable change. Saving or selecting
another object also applies valid pending edits. RGB changes preserve the DB4 color's original alpha.
Intensity/range zero disables a light. Spot cone angles are full angles in degrees.
Legacy projected lights with a zero cone remain unrendered; setting a positive
cone produces a standard spotlight. Projected light textures are not supported.

**Select environment** opens the default ambience: sun color/intensity, back-light
color, ambient color, fog, bloom and saturation. E rotates its sun direction.
Under **Color & bloom**, edit `Saturation`, RGB `Balance`, bloom and vignette
settings, then press **Apply**. These values directly control the scene and
camera monitor; saved F2 colour multipliers no longer alter them.
Local ambience volumes also expose enabled state, priority, shape and game fade
time; R scales their volume. Normal lighting follows the editor camera's actual
position inside a volume. Looking toward a volume or selecting it does not activate
it. The client uses the player's feet when a character is present and the camera's
position in the free-camera viewer, retaining its normal volume fade transitions.

Enable **Lighting zones**, open the **Lighting** tab, or enable **Helpers** to show
colored box/sphere boundaries and matching zone labels. The labels identify the
active zone; disabled zones are gray. Hover a label for priority, sun/ambient/fog
swatches, intensity, fog range and fade time, or click it to inspect the zone.
The Lighting tab also names the current lighting source.

Select an ambience record and enable **Preview selected lighting** in Details to
apply its full look to the scene and fixed-camera monitor from outside the zone.
The source and zone label explicitly indicate this preview override. Turn the
toggle off or select another object to return to camera-position lighting.
The fixed-camera monitor shares the editor's current lighting; selecting a fixed
camera does not change which zone is active.

Both the scene and fixed-camera monitor use the game lighting code and current
`look.json` factors. Browser thumbnails keep their independent studio lighting.
Event-bound lights follow **Map event state** in the editor. The generated
ambience file retains their event references; the game's existing lighting loader
still activates only always-on lights, pending runtime map-event integration.

Lighting changes use the same Save, Undo/Redo, offline cache and rebuild overrides
as other DB4 edits. **Build** writes matching `ambience.json` and derived DB4
records without changing the source database. Duplicate/Delete/Revert also apply
to lights. Navigation does not need rebuilding for lighting-only edits.

## Map event state

The viewport's **Map event state** selector starts at **None** each time a map
is opened. Choices come from flags referenced by that map's effective DB4
records, including edited event definitions; they also work from the metadata
cache with the source files absent. Winter festival, Anniversary, Halloween,
and other named events use their original flag IDs, shown in the choice tooltips.
**Winter festival** previews `christmas_2012` together with
`christmas_2012_silent`, its accompanying scenery. **Winter scenery only**
previews the silent layer independently.

`EventSetRow` refers to `_Instance_EventSets.Id`, **not** SQLite rowid or table
order. `-1` (and null) means unconditional. `EventFlags` contains semicolon
separated declarations: positive `=True` flags are alternative activations,
and an active `=False` declaration vetoes the set. Thus **None** also shows
negative-only sets, and choosing Winter both adds winter content and removes
normal content declared with `christmas_2012=False`. Unknown set references
remain hidden. Geometry's `g` is a placement group, not an event reference.
This matches `event_set_enabled` in the local server's
`gameplay/local_map_state.py`.

Seasonal geometry and particle models also live in `<map>_<event>.eve` files,
using the parent `.map` string/template tables. These are read from the configured
DB4 directory, or from `event_instances` in the converted manifest. The conversion
pipeline now exports these layers and the `.map` EMAL masks, whose zero bits hide
base placements when the corresponding event is active. Kingshill's winter layers
include snow emitters, snow heaps, presents, and Christmas trees. Their placements
are editable geometry records, retain their event membership on export, and use
the same persistent override layer and offline metadata cache as other objects.
They are kept separate from unconditional geometry in exported manifests.

State filtering applies to meshes, NPCs, exits, lights, sequence effects and
decorative characters, picking, and the overview. **Show inactive event
records** exposes gray outliner rows for inspection without showing them in
the scene. Event membership and declarations remain editable in the inspector.
The viewport uses the converted sequence catalog to play effect/light tracks
under each authored sequence's transform, so state changes hide their children
as well. Missing visual assets still leave the DB4 record editable.

The selector previews an event state (including the Winter companion flag);
it does not simulate quest triggers,
event schedules, or a running server. Switching it does not author a map edit,
change exported records, or activate events in the normal **Play** client.

## What gets saved

| Path | Role |
|---|---|
| `level-overrides/<map>.json` | Versioned, reviewable authored operations. Commit these files. |
| `.level-editor/maps/<map>.json` | Cached original placement metadata, DB4 schemas, and records. |
| `.level-editor/db4/<map>.db4` | Original SQLite backup, including metadata, indexes, and untouched tables. |
| `.level-editor/navigation/<map>.json` | Optional cached outline for the map view. |
| `.level-editor/build/maps/<map>.db4` | Derived DB4 containing the same effective records as client exports. |
| `.level-editor/build/<map>.report.json` | Build output paths and navigation rebuild status. |
| `.level-editor/live/<session>/` | Temporary viewport scene, events, status, and renderer log. |
| `.level-editor/previews/` | Rendered model thumbnails, kept across asset rebuilds and available offline. |

The cache is gitignored and stays outside `assets/`. Original `.map` and DB4
files are read-only inputs. Building replaces generated client sidecars, never
the original source database. DB4 exports reject original and cached-source
paths as destinations.

An update stores the original record and only changed fields. DB4 identities use
the table's primary key (usually `Guid`). Geometry identities use the original
model reference and placement content, including a duplicate occurrence number,
so reordering the generated model table or instance list does not retarget an
edit. A changed or removed upstream placement produces an explicit unresolved
target; the editor does not guess which object should receive the edit.

## Rebuilds and synchronization

**Build** saves, checks conflicts, and generates the map
manifest, NPC placements/looks, exits, DB4 props/travel points, ambience/lights, decorative sequences,
and derived DB4 from one composed set of records. Other DB4 tables are preserved
in the database even when the current client has no renderer for them.

The tracked `tools/convert_assets.sh` runs override application after conversion.
The local Windows `.local/convert_assets.py` wrapper does the same in this
workspace. After running individual exporters or using another conversion
pipeline, finish with:

```sh
cargo run --release -p dsor-level-data --bin dsor-levels -- apply --assets assets --db-root <original-maps-directory>
```

Without explicit arguments, the tool uses the editor's source settings.
`DSOR_MAP_DB`, `DSOR_ASSETS`, and `DSOR_LEVEL_OVERRIDES` also select paths; command
line arguments take precedence. The full converter supplies its input tree as a
fallback when no DB4 directory is configured. Only maps with override files are
processed. `--check` checks conflicts without publishing client or DB4 outputs;
it may refresh the metadata cache. Pass map names to process only those maps.

To generate DB4 files in a separate staging directory:

```sh
cargo run --release -p dsor-level-data --bin dsor-levels -- apply a0200_kingscity --db-out <staging-directory>
```

To use gameplay edits online, deploy those derived DB4 files into the server's
runtime map directory and restart/reload the affected server maps. Keep the
original DB4 source directory separate from deployment output. The editor does
not replace a running server's databases. Client-only changes do not move
server-controlled actors until the corresponding DB4 is deployed.

Geometry edits flag `navigation_rebuild_required` in the build report. This
editor does **not** regenerate binary `.map`/`.col` files, bake PathEngine
navigation, or repair gameplay references such as an exit targeting a missing
entry. Moving terrain or obstacles requires the corresponding collision and
navigation authoring workflow before using that geometry for online gameplay.
Decorative sequence exports also need the converted sequence and animation
catalogs; missing required definitions block a build while saves remain usable.

## Missing assets and conflicts

Once a map has been opened, removing the entire assets tree and source DB4
directory still leaves its cached placements, records, schema, and original
database available for editing and rebuilding data files. Missing models remain
editable 3D wire boxes and overview markers. Copy `.level-editor/` alongside the overrides for full offline
editing on another machine; it contains original game metadata and databases,
so keep it out of source control.

Even without that cache, an override document retains its additions, original
edited records, new values, and deletions. Those edited records remain visible
and editable. A complete map/DB4 build waits for a baseline to become available;
unknown surrounding data is never synthesized. Generated manifests carry a
provenance marker so an already edited map cannot become its own original after
a cache loss.

When an upstream rebuild changes a field you edited, the editor shows a conflict
and blocks export. Unrelated DB4 field changes merge automatically. In the
**Changes / conflicts** tab, select the record and either **Revert** to accept
the new base, or **Resolve conflict: keep my override** to explicitly rebase it.
Missing geometry identities require reverting and reapplying the change to the
intended placement. Save files use atomic replacement and reject a stale session
if another editor has saved the same map.

## Validation

```sh
cargo test -p dsor-level-data
```

The Rust tests cover rebuild/reordering stability, cached and patch-only editing,
SQLite WAL backups, blob/index preservation, undo/redo, conflict detection, stale
saves, original-file protection, gesture revisions and untouched float precision.
They also cover fixed-camera lenses, light edits, event-set semantics, binary
seasonal placements, base visibility masks, and missing/truncated event files.

The original Python UI and data implementation have been removed. Two tiny Python
entry points remain for older conversion scripts; they invoke `dsor-levels` and
contain no editing or composition logic. The editor and current shell conversion
integration invoke Rust directly.
