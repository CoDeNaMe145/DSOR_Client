# Testing and diagnostics

[Back to the README](../README.md) | [Getting started](getting-started.md)

Run these commands from the repository root. Rendering checks require
the converted game assets described in [Getting started](getting-started.md).

## Automated checks

```sh
cargo test --workspace                       # protocol, relay, RakNet
cargo run -p dsor-client --release -- a0200_kingscity --cam 109,31.1,84.4,121.5,13.4,71.9 \
    --screenshot-to /tmp/kingshill.png        # renders, saves, quits
```

## Environment variables

Useful environment variables natively: `DSOR_ASSETS=<dir>` (asset folder),
`DSOR_LIKE_WEB=1` (one thread, as in the browser: CPU cost per frame),
`DSOR_SHOT_WAIT=<frames>` (settle longer before the screenshot), `DSOR_PANEL=1`
(look panel open), `DSOR_UI_OPEN=inventory,charactersheet` (windows open at start),
`DSOR_CAST=<skill id>,<seconds>` with `--character 1 0` (cast a skill repeatedly;
`DSOR_SHOT_AFTER_CAST=<seconds>,<png>` screenshots it),
`DSOR_NO_DECALS=1`.

## Performance diagnostics

The HUD (top left) shows FPS, CPU milliseconds per frame (main + render), total
entities and mesh candidates across all views, including shadows. Candidates
are counted before GPU culling; they are not on-screen meshes or GPU draw calls.
The browser console logs the same every five seconds. `DSOR_FLAGS=renderaudit`
(`?flag=renderaudit` in the browser) enables a detailed scene census; see
[rendering diagnostics](fx.md#rendering-census).

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
For combat drops near 60 FPS, use `DSOR_FLAGS=combatprofile,stageprofile`.
`combatprofile` includes frame profiling, lowers the individual frame report
threshold to 16.67 ms, and reports five-second mean/max system wall times for
casts, projectiles, sequences, hit reactions, material flashes, corpse removal,
combat text, stats, audio, and the existing movement/particle/occlusion spans.
System times can overlap on worker threads; do not add them into a CPU total.
This is opt-in diagnostic accounting; normal play does not collect these samples.
The older HUD CPU total uses a shared main/render clock and is not reliable for
attributing costs between pipelined native frames.
On Windows, `python tools/profile_native_maps.py kingscity baseline --flags stageprofile`
runs a fixed-view capture and saves logs under `.local/logs/map-fps/`; use
`swerdfield` for the second map. See the [native map investigation](native-map-performance.md)
for findings, limitations and comparison commands. Extra client environment goes
through `--env KEY=VALUE` (the runner drops the caller's `DSOR_*` variables).

To find which system costs a frame, build with the `system-profile` feature and
run with `systemprofile`:

```powershell
cargo build --locked --profile fast -p dsor-client --features system-profile
python tools/profile_native_maps.py kingscity systems --exe target/fast/dsor-client.exe --flags systemprofile
python tools/profile_native_maps.py kingscity ui --exe target/fast/dsor-client.exe --flags systemprofile --env DSOR_SYSTEM_PROFILE=dsor_client::ui
```

Every five seconds the log gives each module's systems' time a frame
(`system profile module: 0.262 ms a frame  62 systems  dsor_client::ui`), then the
45 costliest systems and schedules with their mean and maximum a run
(`DSOR_SYSTEM_PROFILE=<text>` lists every one whose name holds that text instead).
These are wall times from Bevy's `trace` spans: schedules contain their systems,
the render world's systems run inside `bevy_render::run_render_schedule`, and the
spans add a little cost, so compare builds that both have the feature. This is
how the 2026-10-21 interface regression was found in one run
([Per-frame cost](ui.md#per-frame-cost)).

`frameprofile` also logs `ui work: N layouts prepared, N system views, N quest lists
in N frames` every five seconds: the interface's rebuilds. In a still scene they
stay at 0, and the map runner prints a warning when they do not.
`uiaudit` (always on in debug builds) rebuilds the open windows' views once a
second anyway and logs an error if one changed without its inputs changing: a
`Net` field edited without `Net::touch_ui`.
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
