# DSOR_Client

A Drakensang Online client for the browser (WebAssembly) and the desktop, written
in Rust on Bevy, that plays against the DSO-Reconstruction server
(`experimental`, branch `rust-18`, or the Python `multiplayer-2018`).

The goal is a 1:1 reconstruction of the rel 261 client.

## Layout

| Path | What |
|---|---|
| `crates/raknet` | RakNet 4.035 client protocol in pure Rust, transport-agnostic (no sockets): the same code runs natively over UDP and in the browser over the relay. |
| `crates/proto` | Game protocol: RakNet BitStream codec and the game commands. |
| `crates/relay` | WebSocket <-> UDP relay placed next to the server. Browsers cannot send UDP; the relay carries RakNet datagrams unchanged. |
| `crates/level-data` | Rust level overrides, DB4 composition, event layers, and the headless `dsor-levels` tool. |
| `client` | The game, on Bevy. Builds natively and for `wasm32-unknown-unknown`. |
| `tools` | Asset conversion (wraps DSO_Godot) and helpers. |

## Level editor

Run `LevelEditor.cmd` (or `cargo run --release -p dsor-client -- --editor`) for the native Rust
3D level editor and DB4 inspector. The UI, rendering, and persistence run in one process with no Python dependency. Build it with
`powershell -ExecutionPolicy Bypass -File tools/Build-LevelEditor.ps1`.
Select models in the live viewport; Q selects, W moves, E rotates, and R scales.
Fixed cameras, including `a0000_char`'s character-selection camera, have a
selectable 3D outline and a live preview in the viewport's top-right corner.
The **Lighting** tab edits point/spot lights and the map's sun, ambient light,
fog and bloom, with live scene and camera previews.
Saved edits live in `level-overrides/`, outside
generated assets, and are reapplied by asset conversion. Cached metadata allows
editing when the original assets are unavailable. **Build client + DB4** produces
matching client map files and a derived database for server deployment.
See [the level editor guide](docs/level-editor.md) for source selection, controls,
offline editing, and navigation/collision limitations.

## VFX editor

Run `VfxEditor.cmd` (or `python3 tools/vfx_editor/server.py`) and open
<http://localhost:8091> to edit spell effects: a skill's sequences on a
timeline (effects, lights, shakes, timing), its effect models' particle
emitters (curves for size, colour, opacity, speed...), shader values, animators
and textures. **Open preview** starts the client on the debug map, which replays
the selection within a second of each edit. Edits live in `vfx-overrides/`,
outside the generated assets, and are reapplied by asset conversion
(`tools/vfx_apply.py`). See [the VFX editor guide](docs/vfx-editor.md).

## Game data

The client needs the 2018 game's own files, converted to glTF/PNG by
[DSO_Godot](https://github.com/DSO-Reconstruction/DSO_Godot). They are generated
locally into `assets/` and are never committed (see `tools/convert_assets.sh`).

## Running and testing

### 1. Prerequisites (once)

- Rust (rustup) with the browser target: `rustup target add wasm32-unknown-unknown`
- [trunk](https://trunkrs.dev) for the browser build (`cargo install trunk`, or its
  release binary in `~/.cargo/bin`)
- python3 with Pillow and numpy, a C++ compiler, git (asset conversion)
- the unpacked 2018 client (`DSOClient2018_rel_206-7-32bit/export_win32`)

### 2. Convert the game data (once, ~10 minutes, ~3 GB)

```sh
tools/convert_assets.sh ~/Downloads/DSOClient2018_rel_206-7-32bit/export_win32 \
                        ~/Documents/Drakensang/dsor_assets
ln -sfn ~/Documents/Drakensang/dsor_assets assets
```

This runs DSO_Godot, then every tool of `tools/` in order (render states, cube
maps, ambience, skills, NPCs, exits, interface windows, level decorations). With
`DSOR_SERVER=<DSOR_Server checkout>` it also exports the navigation meshes
(`maps/<map>.nav.bin`, read with the server's PathEngine parser). Without them the
player walks through walls and skills can be aimed anywhere; to add them to
assets converted without: `python3 tools/export_navmesh.py <export_win32> <assets>
--server <DSOR_Server>`.
Details: `docs/assets.md`.

A few tools are not part of `convert_assets.sh` yet; run them afterwards (same
two paths, `<export_win32>` then `<assets>`):

```sh
R=~/Downloads/DSOClient2018_rel_206-7-32bit/export_win32   # or the 261 one, below
O=~/Documents/Drakensang/dsor_assets
G=~/Documents/Drakensang/DSO_Godot
python3 tools/embed_character_masks.py "$R" "$O" "$G"   # character dyes
python3 tools/export_monsters.py "$R" "$O"              # monsters, summons
python3 tools/export_picking.py "$R" "$O"               # click shapes
python3 tools/export_stats.py "$R" "$O"                 # character sheet
python3 tools/export_combat_text.py "$R" "$O" "$G"      # damage numbers
python3 tools/export_debug.py "$O"                      # debug map (F3)
mkdir -p "$O/fonts"                                      # Tahoma, the game's font
cp /System/Library/Fonts/Supplemental/Tahoma.ttf "$O/fonts/tahoma.ttf"
cp "/System/Library/Fonts/Supplemental/Tahoma Bold.ttf" "$O/fonts/tahoma_bold.ttf"
```

(Windows: Tahoma is `C:\Windows\Fonts\tahoma.ttf` / `tahomabd.ttf`.)

#### Release 261 data (branch `release261`)

The 261 client comes as Bigpoint's packed download cache (`dlcache`: `.nb`
bundles and `__toc` files with hash suffixes). The game only downloads what a
character has visited, so that cache misses files (12,960 in ours: skill
animations, meshes, textures -- half-drawn spells, a debug map without ground).

1. Unpack it with DSO_Godot:
   `python3 $G/unpack.py <packed client folder> ~/Documents/Drakensang/client261`
2. Fill the gaps with the 2018 files of the same path (only files the 261 TOCs
   list and the folder lacks; nothing is overwritten):
   ```sh
   cd ~/Documents/Drakensang/client261
   python3 - <<'PY' > /tmp/fill261.txt
   import glob, os
   listed = {l.split("|")[0] for t in glob.glob("*__toc*")
             for l in open(t, encoding="utf-8", errors="replace") if l.count("|") == 2 and "|f|" in l}
   print("\n".join(p for p in sorted(listed) if not os.path.exists(p)))
   PY
   rsync -a --ignore-existing --files-from=/tmp/fill261.txt \
         ~/Downloads/DSOClient2018_rel_206-7-32bit/ .
   ```
   (A file Bigpoint changed since 2018 stays in its 2018 version.)
3. Convert as above with `R=~/Documents/Drakensang/client261/export_win32` and
   another output folder, then run the extra tools, then point `assets` at it.

#### After pulling new code

A tool that changed only takes effect once it is run again over your assets.
Rerunning `convert_assets.sh` always works; to save time, rerun just the tools
the commits touched, for instance:

| Changed | Rerun |
|---|---|
| `tools/export_skills.py` (sequences, statuses) | `export_skills.py`, then `export_deco.py` |
| `tools/export_deco.py` (town guards, townsfolk, animals) | `export_skills.py`, then `export_deco.py` |
| `tools/export_monsters.py` (summons) | `export_monsters.py` |
| DB4 props/travel points | `dsor-levels props --assets assets --db-root <server>/data/maps --static-db <server>/data/db/static.db4` |
| `tools/embed_animators.py` (effect fades, decals) | `embed_animators.py <export_win32>/models <assets>` |
| `tools/restore_pivots.py` (effect positioning) | `restore_pivots.py <export_win32>/models <assets> <DSO_Godot>` after the other model postprocessors |
| `tools/export_stats.py` | `export_stats.py` |
| `vfx-overrides/` (someone's VFX edits) | `python3 tools/vfx_apply.py <assets>` |

`export_deco.py` adds its animation sets to `characters/anims.json`, so it must
run after `export_skills.py` and after any `character_tables.py` run. Any tool
that rewrites an effect model or `skills/sequences.json` drops the VFX edits
from it: run `tools/vfx_apply.py <assets>` last.

### 3. Native client

Running `dsor-client.exe` without arguments opens its built-in login screen.
On Windows, `Play.cmd` opens this same executable. Sign in or create an account,
then choose a character and press **Play Now** to enter the game in the same window. The client uses the
account-service protocol from `dso_custom_client` and passes the returned
account ID and session directly to its game connection.

Native launches use direct 3D and shadow draws to reduce preparation overhead
from small batches. CPU visibility checks, shadows, and GPU instance preprocessing
remain enabled where supported. Set `DSOR_FLAGS=indirectdraw` before launching to
restore the platform's default draw mode; this also applies to `Play.cmd`.
The mode is chosen at startup, so changing it requires restarting the client.

Build and run the complete client, including the login screen:

```sh
cargo build --locked --release -p dsor-client
cargo run --locked --release -p dsor-client
```

The account service defaults to `http://127.0.0.1:8080`. The Rust client connects
to the advertised game host on login port 2190 (or `launch.login_port` when
provided), rather than the retired client's gateway port 80. The service must
be running before signing in. **Settings** configures this address and the name,
class and gender suggested when creating a character. Existing accounts open
the original forest selection scene with their roster and an animated equipment
preview. Click a row or use Up/Down to select; drag the preview or hold its arrow
buttons to rotate it. The list arrows scroll additional characters. Creation and
deletion require their own confirmation. Preferences live in the ignored `client-auth.json`;
passwords and sessions stay in memory. Tab switches fields, Ctrl+V pastes, and
Enter submits the form. Failed authentication and connection errors appear in
the game window and allow another attempt.

The executable also retains the direct game and offline viewer commands:

```sh
cargo run -p dsor-client --release -- a0200_kingscity          # map viewer (free camera)
cargo run -p dsor-client --release -- a0302_wildforest --npcs   # with the level's NPCs
cargo run -p dsor-client --release -- --server 127.0.0.1:2190 --account <id> --sid <session> [--char <id>]
```

| Argument | What |
|---|---|
| `<map>` | the map to show offline (`a0200_kingscity` by default) |
| `--login` / `--offline` | explicitly show the account screen / open the default map viewer |
| `--cam x,y,z,tx,ty,tz` | camera position and look-at point, game frame |
| `--screenshot` / `--screenshot-to <png>` | once loaded, save a screenshot and quit |
| `--no-shadows`, `--npcs` | no sun shadows / place the level's NPCs |
| `--character <class> <gender>` `[--anim <state>]` | a dressed demo character at the map centre (class 0 warrior, 1 mage, 2 ranger, 3 dwarf) |
| `--server <host:port>` `--account <id>` `--sid <session>` `--char <id>` | play online against the login server |

Clicks over the HUD, open game windows, and F2/F3 panels are captured by the
interface until release. They do not issue movement, attacks or exit requests;
scrolling over the interface does not zoom the world camera.

Startup and map transitions use the in-client loading screen with
`splashart_crunch.png` and the frame/fill from `statusbar_connenctscreen_new.png`.
Map progress follows loaded models and textures; gameplay resumes once the
player and renderer are ready. The shared `LoadingScreen::begin_assets` API
accepts asset batches for additional preload stages.

### 4. Browser client

```sh
tools/build_web.sh            # the WebGPU and WebGL2 builds + loader, in client/dist
tools/serve_web.sh            # relay on ws://0.0.0.0:2290 + web server on http://localhost:8080
                              # (builds first if client/dist is missing; --build to rebuild)
```

Then open, offline (map viewer):

```
http://localhost:8080/?map=a0200_kingscity
http://localhost:8080/?map=a0302_wildforest&cam=82,13,220,100,-5,202
```

or online (the game server must be running, e.g. experimental's `run_local_2018.sh`):

```
http://localhost:8080/?server=127.0.0.1:2190&relay=ws://127.0.0.1:2290&account=<id>&sid=<session>
```

| URL parameter | What |
|---|---|
| `map=<name>` | offline map viewer |
| `cam=x,y,z,tx,ty,tz` | camera position and look-at point |
| `npcs` | place the level's NPCs (offline) |
| `noshadows` | no sun shadows |
| `server=host:port`, `relay=ws://...`, `account=`, `sid=`, `char=` | play online (the browser reaches the server through the relay) |
| `gpu=webgpu` / `gpu=webgl2` | force a graphics API (default: WebGPU when the browser offers it) |
| `flag=a,b,...` | diagnostic switches, below |

Flags (`?flag=...` in the browser, `DSOR_FLAGS=...` natively): `nobloom`, `nolights`
(no placed level lights), `noshadows`, `lowshadows` (512 shadow map),
`noparticles`, `nodecals`, `noanim`, `noanimviewcull` (disable NPC visibility-based
animation culling), `nonames`, `nonpcs-shadow`, `depth` (depth
pre-pass on WebGL2: soft particle and water edges), `nodepth` (natively / WebGPU:
without it).

WebGPU needs a secure context: `https://`, or `http://localhost`. A player on
another machine reaching plain `http://` gets the slower WebGL2 build; serve over
HTTPS (and the relay over `wss://`) for others.

### 5. In game

| Key | What |
|---|---|
| left click | walk (online); Shift + left / right button, 1-5: quick slots |
| I, C, S | inventory, character sheet, skills |
| drag an inventory item | move/swap slots; drop onto a compatible stack to merge |
| Ctrl + left click a stack | choose an amount to split into a free inventory slot |
| F2 | the look panel: scale the level's sun, ambient, fog, bloom, vignette...; tonemapping on / off |
| F9 or Ctrl+Shift+D | bug dump: downloads `dsor-dump-<time>.json` (natively: written in the working directory). Also written by itself on a crash or a rendering error. Send it with the bug report. |
| W A S D, Q E, right mouse | free camera (offline viewer) |

### 6. Automated checks

```sh
cargo test --workspace                       # protocol, relay, RakNet
cargo run -p dsor-client --release -- a0200_kingscity --cam 109,31.1,84.4,121.5,13.4,71.9 \
    --screenshot-to /tmp/kingshill.png        # renders, saves, quits
```

Useful environment variables natively: `DSOR_ASSETS=<dir>` (asset folder),
`DSOR_LIKE_WEB=1` (one thread, as in the browser: CPU cost per frame),
`DSOR_SHOT_WAIT=<frames>` (settle longer before the screenshot), `DSOR_PANEL=1`
(look panel open), `DSOR_UI_OPEN=inventory,charactersheet` (windows open at start),
`DSOR_CAST=<skill id>,<seconds>` with `--character 1 0` (cast a skill repeatedly;
`DSOR_SHOT_AFTER_CAST=<seconds>,<png>` screenshots it),
`DSOR_NO_DECALS=1`.

The HUD (top left) shows FPS, CPU milliseconds per frame (main + render), total
entities and mesh candidates across all views, including shadows. Candidates
are counted before GPU culling; they are not on-screen meshes or GPU draw calls.
The browser console logs the same every five seconds. `DSOR_FLAGS=renderaudit`
(`?flag=renderaudit` in the browser) enables a detailed scene census; see
[rendering diagnostics](docs/fx.md#rendering-census).

For intermittent freezes, use `DSOR_FLAGS=frameprofile` (or
`?flag=frameprofile`). It logs raw frames over 50 ms with the map, player position
and loading state; every five seconds it reports p50/p95/p99 and the worst frame.
Slow particle, occlusion, movement, animation and network updates are timed
separately, as are render preparation and submission/presentation. These are CPU
wall times, not GPU timings. F9 includes these log lines in the bug dump.
For comparison runs, `noocclusion` disables the player cutout checks and
`noparticles` disables particle simulation. Enable flags before starting the client.

`stageprofile` adds five-second mean/max timings around main-schedule boundaries,
animation evaluation, transform propagation, extraction and render handoff,
individual render preparation phases, swapchain acquisition, and submission/presentation.
It also reports skin staging-buffer/upload sizes and actual task-pool worker counts.
The nested animation/transform intervals can include parallel work and must not
be added together as exclusive CPU costs.
`between_frames_and_first` includes extraction, synchronization, presentation/event-loop
waits and First; it is not CPU execution time. `gpuprofile` enables Bevy render-pass
diagnostics, including directional/shared shadow passes, and logs the twelve largest
GPU timestamp spans when the backend supports them. These spans can nest, so do not
sum them. Both flags add profiling overhead;
use them to locate costs, then disable them for final A/B frame-time comparisons.
The older HUD CPU total uses a shared main/render clock and is not reliable for
attributing costs between pipelined native frames.
On Windows, `python tools/profile_native_maps.py kingscity baseline --flags stageprofile`
runs a fixed-view capture and saves logs under `.local/logs/map-fps/`; use
`swerdfield` for the second map. See the [native map investigation](docs/native-map-performance.md)
for findings, limitations and comparison commands.
`legacyshadowviews` restores Bevy's unused UI shadow views for same-build regression
comparisons. `allnpcplacements` restores the old offline population with every event
NPC enabled. These are diagnostic overrides; normal play uses rendered 3D shadow
views and the offline viewer respects exported event conditions.
`legacyonlinenpcs` keeps every server NPC's visual loaded for residency comparisons.
Normally only the lightweight actor records remain distant; rigs load within 70 units
and unload beyond 90, while active attached effects keep their rigs resident.
For native scheduler experiments, `DSOR_COMPUTE_THREADS=<positive integer>` overrides
only the compute-worker count. `singlerender` uses a serial outer render schedule;
`singlegraph` uses serial render-graph and per-view schedules. Both retain native
render pipelining. These diagnostic settings do not change the default worker or
executor configuration. Compare them with `stageprofile` disabled too, because its
extra boundary systems add scheduling overhead.
Native clients default to direct 3D/shadow draws, retaining CPU visibility checks,
GPU instance preprocessing and skinning while disabling GPU culling and indirect
draw preparation. `indirectdraw` restores the platform default; it takes precedence
if both draw flags are supplied. Browser defaults are unchanged; `directdraw`
explicitly enables this mode there. `stageprofile` also reports per-view/phase batch
jobs and singleton batches to identify fragmented GPU preparation work.
`legacynpcmaterials` disables sharing of identical immutable NPC part materials for
batch-count comparisons. Normal sharing preserves per-character dyes and animated
materials, and keeps source handles for hot reload.

## Browser build

`tools/build_web.sh` builds the client twice into `client/dist` (WebGPU and
WebGL2) with a loader page that picks WebGPU when the browser offers it;
`tools/serve_web.sh` serves it with the relay. See `docs/fx.md` for the
renderer's state and `docs/ui.md` for the interface.
