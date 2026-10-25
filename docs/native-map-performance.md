# Native Kingscity and Swerdfield investigation — 2026-10-16

Scope: `Play.cmd` / `dsor-client.exe`, local converted release-261 assets.
Swerdfield resolves to `a0007_pastures_pve`; the older `a0300_citypastures`
localization entry has no converted map in this checkout.

## Current result

The remaining Kingscity bottleneck was dominated by preparation of hundreds of
small indirect-rendering batches. Sharing identical immutable NPC materials
reduced the measured jobs from 558 to 422; direct drawing bypasses that setup
while retaining CPU visibility, shadows and GPU skinning. Native clients now
default to direct drawing, with `DSOR_FLAGS=indirectdraw` as a startup fallback.
Online NPC visuals also stream by distance; authoritative actor records remain.

In repeated same-executable comparisons, Kingscity's average final-window p50
fell from 10.12 to 6.64 ms and p95 from 12.74 to 8.24 ms. Swerdfield's smaller
change was 5.15 to 4.58 ms p50, with overlapping windows. These are provisional
results with the other game left open, not an unconditional FPS guarantee.
The isolated performance commit passes 142 tests (13 existing fixture tests
ignored); the integrated workspace passed 152 before separating parallel UI
work. Native lifecycle and both out/back routes pass. `Play.cmd` uses the rebuilt optimized client. See
[the batch investigation and final validation](#confirmed-batch-fragmentation-and-direct-draw-comparison)
for executable hashes, raw evidence, scope and remaining limitations.
Earlier stages and their historical measurements follow below.

## Loading changes after the investigation

The original implementation did visibility culling, not asset residency:
it instantiated every placement and whole-model skin, then hid distant cells
and unwanted skins. The changes now separate these decisions:

- Map geometry loads in 24-unit cells within 70 units of the player's focus,
  retains loaded cells out to 90 units, and still draws within the existing
  50-unit range. At most 24 model requests and two cell builds run at a time.
  Models are shared between resident cells; distant geometry, merged meshes,
  projected decals, and unused model handles are released. Material deduplication
  caches are cleared when residency changes so they cannot pin old textures.
  A placement belongs to the cell of its origin, but all three distances are
  measured to where the cell's placements reach. When a map starts, the native
  client reads every model's bounds from its GLB JSON chunk (`<model>.bounds`,
  read before the binary data) and holds the loading screen until they are in.
  Without this, `g0003_tribe_01_hub` lost its ground in the south: the whole
  ground is one placement, about 160 units across, whose origin's cell is at the
  north end (2026-10-25). Browser builds skip the read and keep measuring to the
  cell squares.
- Decorative characters and sequences use the same 70/90-unit buffer. Offline
  NPC placements also stream, with four new actors per system per frame. Live
  network actor identities still follow the server's interest-management
  messages; the later online-residency change also streams their visual rigs.
- Whole-model NPCs, decorations and monsters request a particular character set
  through `model_variant.rs`. Before Bevy loads the GLB, the reader removes
  unselected skin nodes and unreferenced meshes, materials, textures and images.
  It preserves skeletons, attachment nodes, animations and custom dye/layer
  texture indices. Each model/set pair has a separate cached asset identity;
  ordinary relative texture paths remain shared. Missing sets fail explicitly
  rather than loading every skin.

This requires no asset reconversion. The reader still reads the original GLB's
binary data temporarily; it does not upload or retain unrequested geometry as
render assets. Map manifests, navigation, level lights, and the existing
DB4/network-prop rendering path remain available across the map. This is not
a claim that every asset category is now streamed.

Filter validation: 149 converted character sets across 32 models used by the two
maps (plus monster rigs) pass glTF validation after filtering. Focused tests also
cover unique variant cache paths, animation/joint remapping, attachment retention,
and custom texture-index remapping. At this first streaming stage, the client
suite passed 110 tests with 13 existing tests ignored. The historical measurements below describe the
pre-streaming executable.

Native validation of the new optimized executable
(`.local/map-stream-release.exe`, SHA-256
`93a8860d59c457f67c55c6b2c01384b4eefdc8f3dc99c4d8edb587980fc2a393`):

- Kingscity census: 37,556 entities versus 67,946 previously; NPC hierarchies
  25,094 versus 52,529; dynamic skinned-mesh bounds 48 versus 984. A selected
  horse instance has 5 or 7 mesh parts instead of the original 114. The initial
  player relocation retains some cells within the 90-unit hysteresis buffer,
  so this run has 34 geometry chunks and 416 models, rather than the smaller
  70-unit cold-start set.
- Both offline routes moved to another area and returned, exited normally,
  and logged no errors. Screenshots show scenery returning correctly. Kingscity
  went from 34 resident chunks to 42 and back to 32; model handles went
  416 → 418 → 400 and image assets 1,888 → 2,017 → 1,868. Swerdfield returned
  to 30 of 110 chunks and 230 of 339 models. These are residency observations,
  not GPU-memory measurements.
- The isolated live Swerdfield server run exercised movement, spawning and
  animation, including three different skeleton-warrior looks together. Its
  final monster census has 16 meshes, all selected, with no hidden monster
  meshes. Only the existing missing optional `a0000_char` exits/NPC/deco files
  were reported during character selection; no map or variant load failed.

Evidence: `*-stream-audit.*`, `*-stream-route.*`, route screenshots/history,
and `swerdfield-stream-online.*` in `.local/logs/map-fps/`. Census runs overlapped
the other task's compilation and are not FPS benchmarks. Separate 25-second
`stageprofile` timing runs below had no compiler at either endpoint and no
census enabled. Values are the final two five-second windows, comparing the
preserved pre-streaming executable with the new executable at the same pose;
concurrent development means this is not an isolated skin-filter microbenchmark.

| Sample | Previous p50 / p95, ms | Streaming p50 / p95, ms |
|---|---:|---:|
| Kingscity `(155, 1, -54)` | 13.26–13.28 / 16.42–16.63 | 10.80–12.18 / 13.92–14.05 |
| Swerdfield `(114, 0, 82)` | 6.81–7.52 / 8.21–8.87 | 7.46–7.72 / 8.93–9.78 |

The Kingscity sample improves. The short Swerdfield sample does **not** show an
FPS gain; its new p95 is slightly worse despite lower residency. These runs do
not establish a universal FPS improvement. They leave shadow/render work and
traversal hitches as separate performance targets. No frame exceeded 50 ms in
the final two windows of any of these four timing runs.

## Original investigation findings

The strongest measured optimization target at the sampled Kingscity view is
shadow-related work. Large actor hierarchies and unused skin variants are
additional confirmed work to investigate. Sustained low FPS was not reproduced
in the settled Swerdfield samples; its retained geometry and runtime streaming
still warrant attention. These findings do not establish a universal map-wide
bottleneck or a verified optimization gain.

## Workload and methodology

Machine: Intel i7-14700KF, RTX 4070, Vulkan, 64 GB RAM. The initial captures
are 1280 × 720, uncapped (`AutoNoVsync`). Offline player positions are
Kingscity `(155, 1, -54)` and Swerdfield `(114, 0, 82)`, using the normal
follow camera. `--npcs` loads all placed NPCs, in addition to decorations;
this differs from the server's interest-managed population.

The original executable was preserved at `.local/map-fps-original.exe`.
Initial `*-current` timing runs overlapped background compilation (and briefly
one another) and are **excluded from performance conclusions**. Census runs
also have substantial diagnostic overhead and are used only for scene counts.
In particular, `renderaudit` can itself cause periodic frame spikes.

Raw evidence and screenshots: `.local/logs/map-fps/`. Local runners:
`.local/profile-maps.py` and `.local/run-map-fps-online.py`. The latter uses
an isolated loopback server at `127.0.0.5` and separate fixture stores, without
using account credentials or modifying the normal server's player data.

The user confirmed another task was compiling during this investigation. Later
Kingscity comparison runs recorded no `rustc`/link process at their beginning
or end; earlier timings are retained as exploratory evidence only. The first
comparisons also lacked explicit control over background-window throttling.
The final runner uses the existing UI replay to keep the test window logically
focused, with its cursor outside the viewport, without stealing desktop focus.
Use the runner's compiler checks as endpoint observations, not continuous
monitoring or proof that no other workload ran between them.

The tested optimized executable was copied to `.local/map-fps-release.exe` so
other builds could not replace it mid-comparison. SHA-256:
`5a153114366ad65908348c5fb599702d5e4ecb2d3701f8353abc56a711993f77`.

## Timing results

Ranges below are the two final five-second reporting windows of each short run,
after startup, with `frameprofile,stageprofile` enabled. They are not pooled
percentiles or statistical confidence intervals. There were no frames over
50 ms in those Kingscity windows. Resolution and actor position stayed fixed.

| Kingscity configuration | Frame p50, ms | Frame p95, ms | Mean PostUpdate, ms |
|---|---:|---:|---:|
| Default, `baseline-final` | 9.65–10.38 | 13.05–13.68 | 5.11 |
| `noshadows-repeat` | 7.67–9.00 | 10.71–11.68 | 5.05 |
| `noocclusion-repeat` | 11.37–12.61 | 14.45–15.06 | 3.45 |

Removing shadows reduced the interval between main-schedule frames from about
3.77 ms to 1.86 ms while PostUpdate stayed near 5 ms. This points to the shadow
rendering path as a useful target; that interval includes extraction,
synchronization and waits, so it cannot identify CPU versus GPU shadow cost.

Turning off obstruction rays reduced PostUpdate time, but the overall frame
time did not improve in this comparison. Do not call those raycasts the sole
FPS bottleneck. They currently use up to five mesh raycasts, with all hits
collected before camera-to-player distance filtering; movement into buildings
can be a different workload from this stationary view.

Earlier actor-free, no-animation and no-particle runs overlapped other builds
or had different focus control. They support follow-up experiments, not reliable
speedup percentages. In particular, removing decorations also removes their
effect sequences, so that experiment does not isolate skeleton processing.

An isolated online Swerdfield walk, with real server interest management and
monster updates, settled at p50 **6.99–7.44 ms**, p95 **8.49–9.85 ms** and p99
**9.08–10.97 ms** across the final four reporting windows. There were no frames
over 50 ms in those windows. This covers one entry route, not sustained
large-pack combat or every location. The selection scene logged its three
pre-existing missing optional sidecars (`exits`, `npcs`, `deco`); the game map
loaded without a renderer panic.

A second offline Swerdfield position, `(24.77, 0, 53)`, was checked visually
and timed after compilation stopped (no compiler at either endpoint):

| Swerdfield central view | Frame p50, ms | Frame p95, ms | Mean PostUpdate, ms |
|---|---:|---:|---:|
| Default | 6.30–6.91 | 7.01–7.87 | 2.66 |
| Shadows disabled | 5.14–5.22 | 5.91–6.12 | 2.21 |
| Obstruction checks disabled | 5.10–5.69 | 6.14–6.56 | 1.96 |

Both features have measurable costs at this view, but neither explains a
sustained sub-60-FPS condition there. A higher-resolution session, a different
location, or dense combat can change the result. No claim is made that this
small set of routes reproduces every slowdown the user sees.

Available GPU-pass timestamps in the diagnostic run were small (the largest
reported Kingscity span was bin unpacking at approximately 0.69 ms). **This is
not a complete GPU frame measurement.** Bevy 0.19.1's shadow pass has
`timestamp_writes: None` and no diagnostic recorder span in `view_shadow_pass`.
Do not use the reported GPU passes to rule out a GPU shadow bottleneck. The
GPU diagnostic runs also overlapped compilation and are not FPS benchmarks.

## Scene census

These are CPU visibility candidates, not GPU draw calls or visible pixels.
Main-view and shadow candidates overlap and must not be added together.

| Fixed-position offline capture | Kingscity | Swerdfield |
|---|---:|---:|
| Map placements in manifest | 8,319 | 8,802 |
| Placed NPCs + decorative actors | 119 + 205 | 3 + 30 |
| Total entities after loading | 67,946 | 26,106 |
| Entities in NPC hierarchies | 52,529 | 2,238 |
| Map meshes | 3,215 | 6,034 |
| Map meshes hidden by hierarchy | 2,552 | 5,198 |
| Map main-frustum candidates | 99 | 126 |
| Map shadow candidates | 468 | 773 |
| Map candidates outside main frustum, in another view | 386 | 670 |
| Map triangles in main-frustum candidates | 496,435 | 528,140 |
| Animation targets, whole world | 20,603 | 1,916 |
| Extra body-variation nodes | 14,128 | 414 |
| `DynamicSkinnedMeshBounds` components | 984 | 76 |
| Retained `Mesh2d` entities | 5,950 | 5,900 |
| Hidden / total map cells | 65 / 82 | 90 / 110 |

### Kingscity actor cost

The distance and animation culling systems already hide distant actors and park
bone targets. They do not unload their entity hierarchies. About 77% of entities
in this capture belong to NPCs/decorations, even though only a fraction animate.

Whole-model NPCs retain unselected skins. The horse scene alone accounts for
3,084 entities and 684 meshes: 672 meshes are hidden, none is in the main
frustum, and 12 are shadow candidates. `npc::animate_models` hides unused skin
variants rather than removing their mesh/skinning components.

The installed Bevy 0.19.1 `update_skinned_mesh_bounds` query processes all
`DynamicSkinnedMeshBounds` meshes without a visibility or joint-change filter.
Hiding these variants therefore does not suppress that registered bounds work.
Dressed character parts already remove dynamic bounds and use explicit bounds
in `character::bind`; whole-model NPCs have a different path.

This is a concrete avoidable workload, **not yet a measured millisecond saving**.
A safe optimization must preserve bounds when an actor becomes visible and
retain joints used by the selected skins and attachments.

### Swerdfield geometry cost

The largest retained scene hierarchies include 510 meshes for fence pillars,
506 for stone walls, 286 for tombstones, and 224/210 for two fence variants.
These full scenes bypass `map::flat_surfaces` merging: that path rejects models
with animations, skins, emitters, or projected decals. Static geometry otherwise
merges by material/layout/shadow policy into 24-unit cells.

The captured main view therefore has roughly half a million map triangles
before GPU culling, despite the relatively small number of mesh candidates.
The shadow view includes substantially more map meshes than the main view.
Possible improvements are an intact-state representation for destructible props,
spatially tighter batching, and tighter shadow-caster bounds. Visibility and
shadow correctness need visual regression checks before changing those policies.

### Online population check

The isolated Swerdfield server generated 394 initial monsters and another 126
from the map event. It sends nearby actors to the client, not all 520 at once.
At the entry position, the census retained 1,098 entities / 112 meshes under
monster hierarchies, of which 103 meshes were hidden and four were main-view
candidates. Therefore an offline `--npcs` benchmark does not cover monster
streaming, combat effects, death, or changing equipment/skin state.

## Diagnostic changes

`DSOR_FLAGS=frameprofile,stageprofile,gpuprofile` now separates raw frame
intervals, main-schedule boundaries and supported GPU timestamp spans.
`stageprofile` logs mean/max values for PreUpdate, fixed/update, scene spawning,
PostUpdate, Last and the interval from Last through the next First. That last
interval includes extraction/synchronization/event-loop work and waits.
`gpuprofile` logs the largest twelve GPU spans; nested spans must not be summed.
These diagnostics are opt-in and do not change default rendering or gameplay.

The existing HUD's shared main/render CPU clock is unsuitable for attribution
across pipelined native frames; a render completion can see a later main frame's
start. Render CPU wall time also includes waits and is not a GPU measurement.

## Recommended implementation order

1. **Measure and reduce shadow work.** Add actual shadow GPU timestamps or use
   a graphics capture, then separate CPU caster preparation from shadow-map
   rendering. Preserve off-screen objects whose shadows reach the visible scene.
   Use the current fixed views and moving camera routes as visual regressions.
2. **Remove work for unused skins and dormant actors.** Stop dynamic bounds
   updates on permanently unselected whole-model variants, then reduce retained
   actor hierarchies or stream decorations around the player. Do not remove
   bones still used by selected skins, picking, or effect attachments.
3. **Give static prop states a cheaper representation.** Swerdfield's intact
   destructible props should not require all the general-purpose scene overhead
   indefinitely. Preserve their transition to the destroyed state.
4. **Bound or accelerate obstruction raycasts.** Restrict tests to the camera–
   player segment and accelerate triangle queries. Preserve the five-sample
   full-occlusion behavior. The Kingscity test shows that reducing this CPU work
   alone does not guarantee a faster presented frame.
5. **Repeat at the user's full gameplay workload.** Include their actual display
   resolution, entry/loading transitions and sustained combat. Separate loading
   spikes from settled frame-time tails, and keep compilation/census off.

These were the initial follow-up priorities. The subsequent loading and
entity-residency changes below now also change the production client.

## Reproduce and validate

Build with the normal `cargo build --locked --release -p dsor-client`. Wait for
other builds to finish. On Windows, run these sequentially from the repository:

```powershell
python tools/profile_native_maps.py kingscity baseline --flags stageprofile --seconds 45
python tools/profile_native_maps.py kingscity shadows-off --flags stageprofile,noshadows --seconds 45
python tools/profile_native_maps.py kingscity obstruction-off --flags stageprofile,noocclusion --seconds 45
python tools/profile_native_maps.py swerdfield center --position 24.77 0 53 --flags stageprofile --seconds 45
```

Repeat/reverse the ordering before estimating an improvement. Use the same
executable, resolution and focus policy. Logs, replay inputs and JSON summaries
go under `.local/logs/map-fps/`; summaries include the executable hash and
compiler observations. The runner ends its own offline process at the requested
duration, so its recorded termination code is not evidence of a game crash.
The `--audit` and `--capture` options are for inspecting content, not final
frame-time measurements. Omit `stageprofile`/`gpuprofile` when collecting final
un-instrumented comparison timings (the runner retains lightweight `frameprofile`).

Validation completed: optimized native build, both diagnostic flags exercised
on both maps, sequential fixed-view comparisons, the isolated online Swerdfield
route, Python syntax/help and runner execution, and `git diff --check`. No renderer
panic was observed. Browser behavior and sustained large-pack combat were not tested.

## Interpretation

Map area and total NPC count alone do not explain frame rate. This client pays
for retained hierarchies, skeletal updates, material/mesh preparation, shadow
coverage, geometry in partly visible batches, and pixel work. The Path of Exile
comparison is a useful scalability target, but this investigation does not
measure its engine or establish a like-for-like workload comparison.


## Follow-up: retained entities and skinning work (2026-10-16)

The fixed Kingscity view still held 37,559 entities after nearby map/decor streaming
and requested-only whole-model variants. These are not 37,559 game actors:
there were 137 NPC roots. The principal sources of retained entities/work were:

- Shared dressed-character rigs: 7,653 animated bone entities plus 7,653 scale
  helper entities. Those helpers implement non-inherited Nebula joint scaling;
  the authored variations and animations actually use it. Removing them outright
  would change body shapes and animated poses.
- Equipment scenes: seven entities per typical single-mesh clothing piece,
  including three empty nodes left behind when conversion lifted the skinned
  surface out of its original model branch.
- Clothing skin palettes: 71,489 entries in this view, approximately 24,532 on
  meshes visible to a render view. Each piece carried its entire 68/69-joint rig.
  Scanning all 2,988 human clothing GLBs found 206,172 palette entries but only
  39,422 entries with nonzero vertex influence (80.9% unused). Bevy 0.19.1's
  `bevy_pbr/src/render/skin.rs::extract_joints_for_skin` checks every palette entry
  for each visible skin, and updates/uploads matrices for changed joints.
- Equipment loading also instantiated a private skeleton for each piece, then
  deleted it when binding to the shared character skeleton. This caused transient
  ECS creation/destruction even though the settled census excluded those copies.
- Roughly 6,000 Mesh2d entities belonged to the legacy UI, including unopened
  menus. The original census called these `other` because it only recognized
  Bevy UI `Node` hierarchies, not the legacy window roots.

Implemented changes:

1. `model_compact.rs` compacts each mesh's palette to its nonzero vertex
   influences before Bevy loads it. It remaps JOINTS_0 and the matching inverse
   bind matrices together. Zero-weight lanes remain valid indices; multi-set
   influences and external-buffer models retain their original representation.
   The animated bone hierarchy, animations, materials and geometry remain intact.
2. Converted empty `model` branches are disconnected from character scenes.
   Bone joints, animated nodes, metadata-bearing nodes and named sockets remain.
3. Equipment scene assets remember joint names and discard private skeletons
   before instancing. Newly loaded pieces bind directly to the shared rig;
   an outfit cannot become visible while its skin is still unbound.
4. Unopened menu windows retain lightweight roots and control state. Artwork,
   text, meshes and textures are constructed on first request, then reused on
   subsequent openings. Dialogue continues to rebuild when its contents change.
5. The census identifies legacy UI and reports both total and visible skin
   palette entries, so future regressions can be attributed to actual work.

The GLB source binary is still read temporarily. This is a reduction of loaded
render data and ECS work, not a new compressed/on-disk asset format. Already
opened menu artwork stays cached, and full articulated rigs remain resident for
nearby characters. Map area alone is therefore still not the retained entity
count or the frame cost.


### Separate global prop path

The follow-up census also exposed a second loading path in `props.rs` that
bypassed terrain streaming: Swerdfield retained 11,741 map entities, dominated
by full destructible-prop hierarchies. Stone-wall, fence-pillar and tombstone
scenes alone accounted for 4,003 entities / 1,302 meshes, of which 1,196 meshes
were hidden. These are alternate prop states, not distant terrain batches.

Prop visuals now use the same 70-unit load / 90-unit unload buffer as nearby map
content. Lightweight authored/network records and map markers remain available
across the map. Only resident (model, requested-state) handles are retained, with
at most eight new visual instances per frame and bounded concurrent model loads.
A server state change replaces the visual with the newly requested state;
updates to unloaded props still update their authoritative record. Removal,
GUID rebinding, event activation and map reset continue through the existing
lifecycle rules.

State selection occurs before glTF loading and follows the existing fallback
rules, including cooldown -> inactive and missing colour -> active. Shared
nodes remain, while unselected branches and their mesh/material/texture assets
are excluded. Skinned props retain the original visibility selection as a
conservative fallback for cross-branch joint dependencies. All 420 combinations
of supported states across the maps' 35 prop models passed glTF validation.

Regression checks: 119 client tests passed (13 existing asset-dependent tests
ignored), including direct binding without private bones, palette influence
identity, first-open menu residency, prop radius hysteresis, state selection,
network GUID/discard/update/event behavior, and converted-asset validation.
The fast test executable uses different optimization settings and is never used
for FPS comparisons. The normal optimized release is used for native captures
and timing.


### Native results for the completed change

Windows native, 1280x720, default quality, AutoNoVsync, the same camera/player
poses and offline populations. Control executable SHA-256:
`c5486dfc8ecdc257dda2ee47b50454d519e430cb7b8d761bf5d84420272dbb32`.
Final normal release SHA-256 (also `target/release/dsor-client.exe`):
`a6372b07fae113e6ac27b4147c09d6f4e2cd3fd479795a5ec61e511e09d67a1c`.

| Metric | Kingscity before -> after | Swerdfield before -> after |
|---|---:|---:|
| Total entities | 37,559 -> 27,121 (-28%) | 20,002 -> 5,334 (-73%) |
| Map entities | 4,455 -> 4,256 | 11,741 -> 3,428 |
| NPC entities | 25,094 -> 21,139 | 558 -> 492 |
| Resident prop visuals | 8 -> 3 | 229 -> 56 |
| Mesh assets | 8,951 -> 3,401 | 8,044 -> 2,519 |
| Image assets | 1,888 -> 1,559 | 856 -> 536 |

Kingscity's 1,079 NPC meshes and their 295,898 total triangles are preserved.
NPC skin palette entries fell from 73,260 to 14,097 (-81%). The old palette
count is reconstructed from each original scene's skinned-mesh census and
source skin palette; all those assets have one palette size. The new count is
reported directly by the census. Visible palette entries are approximately
25,100 -> 4,886, with slight animation/view-candidate variation between captures.
The remaining NPC count is dominated by the 7,653 bones and 7,653 required
scale helpers, rather than unused equipment skeletons or empty model branches.

Eight sequential 25-second timing runs alternated/reversed old and new builds.
No compiler was observed before or after any run. Census and live skin checks
were off; both builds used `frameprofile,stageprofile`. The table below averages
the p50/p95 reported by the last two five-second windows in each of two runs;
it is **not** a percentile computed from one pooled frame trace.

| Map | Mean window p50 before -> after | Mean window p95 before -> after |
|---|---:|---:|
| kingscity | 9.41 -> 8.71 ms | 12.12 -> 11.06 ms |
| swerdfield | 5.17 -> 4.92 ms | 6.69 -> 6.28 ms |

These averages improve by about 8%/5% for p50, but the runs overlap materially:
Kingscity's control window medians ranged from 8.11 to 11.18 ms, versus 8.32 to
9.29 ms after. Its first pair improved and its reverse-order pair did not.
Swerdfield's ranges were 4.76-6.00 ms before and 4.66-5.25 ms after. Therefore
there is no claim of a uniform FPS improvement or a resolved frame-stability
problem. Retained entities/assets and unnecessary skinning work are demonstrably
lower; large-pack combat, other resolutions and browser performance still need
separate measurement. All retained timing windows had zero frames over 50 ms.

Both out-and-back native routes exited successfully without renderer, dead-joint
or foreign-joint errors. Kingscity prop visuals/models went 3/3 -> 6/6 -> 3/3;
Swerdfield went 56/15 -> 5/3 -> 54/13. On Swerdfield, mesh assets fell from 2,519
to 1,709 at the distant stop and returned to 2,341, with the slight return
population difference expected from the 70/90 residency buffer. Screenshots of
both return views were inspected. Inventory first-open/reopen was also captured.

An isolated online Swerdfield server exercised actual NewProp, PropUpdate and
DiscardProp packets. The selected wall switched active -> inactive -> active ->
cooldown (inactive fallback) -> active, was discarded, then republished under a
new network ID with the same GUID. Audits confirmed 64 -> 63 -> 64 resident
visuals and 16 -> 17 -> 16 model handles across state changes. Its unused state
handle was released on return to active. Live monster spawning/animation also
ran (nine selected meshes, 283 palette entries, no hidden skin alternatives in
the final capture). The fixture used temporary local stores and was stopped.

Artifacts: `.local/logs/map-fps/*compact*`; `compact-comparison.json` contains the
window values used above. `tools/profile_native_maps.py --ui-route <json>` merges
menu actions into the focus replay; `--check-skins` adds the expensive live joint
ownership diagnostic and must not be used for timing comparisons.


## Kingscity follow-up: unused UI shadow views and source asset states

The live-server Kingscity spawn at `(57.25111, 1.68, -48.35584)` exposed a
different problem from Swerdfield's global prop hierarchy. With the previous
release, 648 of 1,267 resident NPC meshes were reported as shadow candidates.
That was every NPC mesh not hidden by distance/parent visibility; only 146 were
main-camera candidates. There were 5,458 attached NPC animation targets and
9,070 visible skin palette entries. These counts include decorative townspeople.

**The shadow-candidate count was contaminated by an unused UI-camera cascade.**
In the installed Bevy 0.19.1 sources:

- `bevy_light/src/cascade.rs::build_directional_light_cascades` creates cascades
  for every active `Camera`, including the 1280x720 orthographic `Camera2d`.
- `bevy_light/src/lib.rs::check_dir_light_mesh_visibility` uses every cascade
  to mark meshes `ViewVisibility`-visible. It checks mesh/light layers, but does
  not reject views whose camera layers cannot receive that light.
- `bevy_pbr/src/render/light.rs::prepare_lights` actually prepares directional
  shadow views only for `Camera3d`, with matching camera/light layers.

The enormous UI cascade was therefore keeping nearby NPC poses and skin
extraction active without producing a rendered shadow. Earlier census tables
report the union of these CPU candidates; they must not be read as real shadow
draw counts or proof that every offscreen NPC was needed by a rendered view.

`shadow_views.rs` now filters the cascade map after construction and before
directional frusta/visibility are evaluated. It retains active 3D cameras with
matching light layers. Real offscreen shadow casters, portrait views with their
own lights, shadow resolution, and shadow distance are preserved. Existing Bevy
systems remove obsolete per-view visibility lists. `legacyshadowviews` restores
the old behavior for comparisons using the same executable. The census now
records `directional_shadow_views` by light/camera, camera kind/activity, and
per-cascade candidate counts.

Two separate asset-pipeline issues were also found:

1. Character conversion defaulted materials to `doubleSided=true`, and the later
   render-state pass skipped split skeleton parts. The original `character`,
   `characterfalloff`, and `monster` shaders specify default `CullMode=2`.
   Auditing 5,720 source mesh nodes found no overrides; 2,988 human and 1,296
   dwarf part materials need correction. `character_render_states.py` applies
   the verified defaults during conversion, and `model_compact.rs` provides the
   same compatibility repair for existing GLBs. Explicit two-sided settings,
   mixed users, unknown culling modes, and unrelated shaders are preserved.
2. NPC export discarded `EventSetRow` and its conditions. Kingscity's offline
   viewer consequently combined all 119 NPC placements, including seasonal and
   quest alternatives. With no events active, 47 are enabled; the old fixed-view
   70-unit radius contains 46 placements versus 16 enabled by the authored rules.
   Negative-only conditions remain enabled. Python and level-data exporters now
   preserve the metadata, and offline spawning evaluates it. Online spawning
   remains authoritative from the server and may include its baseline unlock
   flags. `allnpcplacements` restores the old offline population for comparisons.
   Existing Kingscity/Swerdfield sidecars were annotated without changing their
   other fields; legacy sidecars elsewhere need regeneration to gain filtering.

The isolated online control has **200 NPC roots: 52 server NPCs and 148 streamed
decorative actors**. Of these, 45 server NPCs and 74 decorations use dressed
characters; the other 81 use whole models. The audit's 120 `Character` components
also include the local player. This population is not all 119 source placements:
the fixture server applies event filtering and baseline town-unlock flags,
initially publishing 51 NPCs. Its player-entry crafting event adds one NPC but
fails to retire the previous event's NPC. In `.local/dso_server`,
`services/map_events.py::_apply_map_event_transition` retires disabled monsters
and props but omits NPCs, while `_spawn_event_npcs` adds the replacement. That
one stale online actor is a separate server lifecycle bug from the missing
offline event metadata fixed above; the fixture server was left unchanged.

`gameplay/local_map_state.py::load_local_map_initialization` sends every enabled
NPC in the map, without a distance-based interest filter. The client retains
their visual hierarchies until a discard packet, unlike its existing offline
NPC/decorative-actor residency. At the online spawn, **19 server NPCs are beyond
70 units** (1,307 bones plus 1,307 scale helpers), including **7 beyond the
90-unit unload radius** (483 bones plus 483 helpers). The count beyond 70 was
18 before the crafting event added its replacement. These are real remaining
residency costs, but are not established as the dominant FPS cause: those distant
actors are already hidden and their animation is parked. Entity counts alone
do not measure their frame cost; the unused UI shadow cascade independently
kept many nearer actors active.

A separate online-residency change should keep each authoritative actor root,
network identity, profile, transform, and map icon stable while loading visual
descendants within 70 units and unloading them beyond 90, with bounded spawns
per frame. Picking, dialogue, replacement/discard packets, and root-based
character/bone lookups must keep working. Active effects that reference bones
must retain their rig; new effects received while a rig is absent must wait for
attachment readiness rather than permanently attaching at a fallback position.
This needs network, effects, and out-and-back interaction tests and is not
implemented in this follow-up. Population evidence is retained in
`.local/logs/map-fps/kingscity-deep-online-control.audit.json` and its matching
`-server.log`; the fixture uses `.local/run-map-fps-online.py` and isolated stores.

Animation export also contains substantial constant data: 82.35% of the human
library's 151,524 channels are constant within a clip. This is not automatically
safe to delete: those constants differ across clips and participate in resets
and blending. No animation tracks or required scale helpers were removed in
this follow-up.

Diagnostic corrections: `noanim` allows selection previews to pose so online
tests can enter the world, and `nonpcs-shadow` now includes decorative actors.
The earlier online noanim run remained in character selection and is excluded.
`stageprofile` includes nested animation/transform wall intervals and render
preparation/submission means; they may include parallel work and must not be
summed as exclusive CPU time. `gpuprofile` now timestamps actual shadow passes
as well as Bevy's other instrumented passes, only when explicitly enabled.

Automated validation: 125 client tests passed, 13 existing fixture-dependent tests
ignored. A fresh process with `DSOR_FLAGS=noanim` also passed the hidden-selection
preview regression. The level-data NPC event sidecar integration test and both
Python exporter/render-state tests passed. Comparing the two updated NPC
sidecars with saved originals confirmed all pre-existing fields are unchanged.

Timing conditions for this follow-up: another game is running on the same GPU.
The user explicitly requested leaving it open and reporting timings as
provisional. GPU utilization was about 35-36% with this client closed. Native
frame times below therefore describe a contended desktop, not an isolated
hardware benchmark; correctness, per-view census, and same-build comparisons
are the primary evidence. The initial animation interval was approximately
0.43 ms (0.61 ms for the surrounding animation set), transform propagation
approximately 0.87 ms, and render prepare/submit-present wall intervals about
6.16/7.63 ms. These were diagnostic-build observations with the old shadow-view
behavior, not final release speedup measurements or exclusive CPU/GPU costs.

### Native evidence for the Kingscity follow-up

The final normal optimized release is `target/release/dsor-client.exe`, retained
as `.local/map-shadow-final.exe`, SHA-256
`8dd17e85ba3ccc5e82f86ac046b8f62f4939b919951d97d7e28b0a19eee3ca60`.
The following online comparison uses that same executable, the same server
fixture/population, the same plaza pose, and only toggles `legacyshadowviews`.
It isolates the unused-view correction; it does not isolate the material repair
or compare the different online and offline NPC populations.

| Metric at the online plaza | Legacy views | Corrected views |
|---|---:|---:|
| Unused UI shadow-view mesh candidates | 1,675 | 0; view removed |
| Actual 3D shadow-view mesh candidates | 1,162 | 1,162 |
| NPC mesh candidates across all views | 648 | 510 |
| NPC visible skin palette entries | 9,070 | 6,823 |
| Map mesh candidates across all views | 1,037 | 666 |
| All mesh candidates in HUD | 1,733 | 1,220 |
| Active dressed characters in HUD | 62 / 120 | 50 / 120 |
| Retained NPC entities / meshes | 25,401 / 1,267 | 25,401 / 1,267 |

The visible NPC palette reduction is 24.8%. Retained geometry, NPC population,
and real 3D shadow candidates are unchanged. All seven steady audit snapshots
per run report 5,922 versus 5,239 attached NPC animation targets. This counts
`AnimatedBy` components eligible for traversal, not bones sampled every frame;
held animation-LOD frames retain those components.
Both screenshots were inspected; the plaza, characters, clothing, and rendered
shadows remain present. Audit files are `kingscity-shadow-{legacy,fixed}-audit`
under `.local/logs/map-fps`.

Actual directional-shadow GPU spans were approximately 0.265 ms with the unused
view and 0.264 ms without it. The unnecessary view did not render a shadow pass:
its cost was visibility processing and keeping additional meshes/poses active.
In the corrected diagnostic run, animation-target evaluation was about 0.46 ms
(0.61 ms for its surrounding animation set), transform propagation 0.81 ms,
render preparation 5.89 ms, and submit/present 7.44 ms. The latter two are wall
intervals including waits, not exclusive CPU work or GPU duration. Individual
GPU spans included bin unpacking 0.88 ms, transparent 3D 0.37 ms, clustering
0.31 ms, opaque 3D 0.17 ms, and SSAO 0.10 ms. Some profiler spans nest: they
cannot be summed to infer a full GPU frame.

Four 40-second timing runs used the order legacy, corrected, corrected, legacy,
with only lightweight `frameprofile` enabled. No compiler was observed before
or after any run. The last two five-second windows of each run produce:

| Statistic | Legacy views | Corrected views |
|---|---:|---:|
| Mean of window p50 values | 13.98 ms | 13.23 ms |
| Range of window p50 values | 12.89–14.84 ms | 12.64–13.86 ms |
| Mean of window p95 values | 16.82 ms | 16.50 ms |
| Frames above 50 ms in retained windows | 0 | 0 |

These are averages of window percentiles, not pooled frame percentiles. The
mean window median improves about 5.3%, but the ranges overlap, the first pair
regressed, and another game remained active as requested. These provisional
timings do not establish a stable FPS improvement or resolve the whole Kingscity
frame-time problem. Animation evaluation and real shadow rasterization alone
do not explain the remaining frame time; rendering preparation/submission and
its scheduling/presentation waits need finer attribution under controlled load.
Raw timing records are in `shadow-views-comparison.json`.

The online fixture still logs three existing missing optional sidecars for the
character-selection map (`a0000_char.npcs.json`, `.exits.json`, `.deco.json`)
before entering Kingscity. The comparison verifies actual Kingscity HUD/map
entry and does not treat time spent in selection as a city benchmark. Heavy
render census and live joint checks were disabled for timing; their periodic
diagnostic spikes are excluded from performance conclusions.

Both final-release offline out-and-back routes completed with exit code 0,
`DSOR_CHECK_SKIN=1`, no renderer/skin ownership errors, and all three requested
captures saved. Return screenshots for Kingscity and Swerdfield were inspected.
Kingscity's NPC entities changed 15,371 -> 21,129 -> 14,829 across the three
stops; prop visuals/models changed 3/3 -> 6/6 -> 3/3. Swerdfield's total entities
changed 5,334 -> 3,843 -> 4,939. Differences between initial and return residency
are consistent with the 70/90-unit buffer and the different locations traversed.
Both returned to the specified original player position, kept only the actual
3D directional-shadow view, and showed correctly bound/posed characters after
reloading. No compiler was observed at either endpoint. These routes include
expensive per-frame joint checks and census work and are correctness checks,
not FPS comparisons. Artifacts use the `*-shadow-route` prefix.

Two further 40-second diagnostic runs confirmed that `noanim` now enters the
online city (0/120 dressed characters animating), and `nonpcs-shadow` covers
the decorative crowd (17/120 animating, 858 mesh candidates across all views).
The final two noanim window medians were 12.85 and 11.78 ms, with animation-set
and transform wall intervals around 0.32 and 0.42 ms. This is consistent with
animation evaluation explaining only part of the frame, but the diagnostic
runs are not a repeated controlled speedup comparison. In particular,
`nonpcs-shadow` walks tagged NPC hierarchies to catch asynchronously arriving
meshes each frame, adding diagnostic CPU work; its frame time is not an unbiased
measure of shadow-removal benefit. Both native checks reached Kingscity and
reported only the existing selection-sidecar errors described above. Logs and
summaries use `kingscity-shadow-ablations-*` / `shadow-ablations.json`.

### Online visual residency and finer renderer attribution

The subsequent investigation implements the online residency change described
above. Server NPC roots retain actor identity, profile, transform and map marker.
Their recipes contain strings and character descriptions rather than asset
handles. Visuals load nearest-first within 70 units, at most four actors per
frame, and unload beyond 90 units. Eviction releases the character/model handles,
visual children, bone bindings and picking boxes. Re-entry rebuilds these on the
same authoritative root. `legacyonlinenpcs` restores full visual residency for
same-executable comparisons; it does not change server population.

Existing sequences pin resident rigs until their attachments retire. A status
received while a rig is absent does not force a distant load. Its clock and
expiry continue; joint attachments wait for a ready visual, and expired effects
are not replayed on return. Logical character status survives visual eviction.
The diagnostic `nonpcs-shadow` now processes newly added meshes instead of
walking every NPC hierarchy each frame.

The skeleton repair fallback also avoids scanning every cached bone each frame
when no relevant names or animation players were removed. It still handles
partial and delayed scene replacement, nameless cached bones, and the explicit
`DSOR_CHECK_SKIN` diagnostic. This removes redundant work but is not assumed to
explain the whole frame-time gap.

New opt-in render diagnostics measure extraction, renderer handoff, swapchain
acquisition, preparation stages, skin staging/upload, graph recording/submission
and presentation. These are wall intervals and may overlap. In particular,
Vulkan swapchain acquisition includes explicit fence waits on Windows; a long
render interval does not establish equivalent CPU execution or GPU draw time.
`DSOR_COMPUTE_THREADS`, `singlerender` and `singlegraph` allow isolated scheduler
experiments without changing renderer pipelining. Detailed instrumentation adds
many small systems, so scheduler comparisons must also run with only lightweight
`frameprofile` enabled.

The first optimized probe is retained as `.local/map-residency-probe.exe`, SHA-256
`b23417135eec69378102f994d94684bff54c3475b5dbfb2df2e137dbe09ea2c5`.
It predates the first-pose publication correction. Its same-executable census
comparison (`residency-census-*`) confirms:

| Online plaza metric | Full NPC visual residency | Distance residency |
|---|---:|---:|
| NPC roots, including decorative actors | 200 | 200 |
| Resident dressed NPC rigs | 119 | 100 |
| Resident whole-model NPCs | 81 | 81 |
| NPC entities | 25,401 | 21,970 |
| All entities | 32,305 | 28,874 |
| NPC meshes | 1,267 | 1,077 |
| All meshes | 3,683 | 3,493 |
| NPC main-view / shadow candidates | 148 / 510 | 148 / 510 |
| All main-view / shadow candidates | 327 / 1,162 | 327 / 1,162 |

All actor/template/position records match. The 19 deferred actors are server
NPCs; decorative actors already used distance residency. Camera poses/layers,
the single real directional-shadow cascade, 43 resident map chunks and 455
models also match. This removes 3,431 entities and 190 hidden meshes without
reducing visible work. Census runs include expensive periodic audits and are
not frame-time benchmarks. The old HUD animation denominator counts logical
`CharacterAnim` records, including unloaded NPCs, rather than resident rigs.

Detailed profiling of this probe locates the remaining renderer cost in writing
phase buffers (roughly 2.3 ms), preparing bind groups (1.7 ms), recording render
views (5 ms) and submission (2 ms). Swapchain acquisition is about 0.07 ms,
presentation tail about 0.25 ms, skin extraction about 0.48 ms and skin upload
about 0.12 ms. Skin upload is approximately 575 KB/frame in this run. These
wall intervals include scheduler and driver overhead; they are not independent
CPU samples. Source inspection identifies per-batch buffer writes, recreated
bind groups and individual GPU bin-unpacking dispatches as a concrete target.
The `directdraw` comparison bypasses that path while retaining GPU instance
preprocessing and skinning; it also disables GPU culling, so it changes the draw
strategy, not just scheduling.

The first scheduler series with only `frameprofile` used default, four/eight compute
workers, serial outer render scheduling, serial per-view graph scheduling, and
both serial modes. All retained equivalent scene counts. Four workers gave the
largest initial median improvement. A separate four/default/default/four series
had mean final-window p50 values of 9.86 / 11.85 ms and p95 values of
12.41 / 14.93 ms respectively. These are averages of four five-second window
percentiles per mode, not pooled percentiles. The desktop remained contended as
requested, and the first and second series shifted noticeably. One compute
worker regressed to 12.59 / 13.06 ms medians; serializing both renderer schedules
did not add their individual improvements. No default worker/executor change is
justified from this hardware and workload alone. Raw records are in
`residency-probe-comparison.json` and `residency-repeat-comparison.json`.

The isolated native lifecycle fixture (`kingscity-residency-lifecycle-probe`)
publishes a dressed smith and a whole-model gambler, teleports away and back,
then discards and republishes their actor IDs. All identity, visual eviction,
new-child creation, picking rebinding and discard assertions passed. A status
sent while the smith's rig was absent did not force a load. Live skin checks
reported no dead/foreign joints or renderer validation errors; return and
republication captures show posed actors. This checks residency correctness,
not first-frame posing or FPS. The fixture intentionally terminates its client
after completion, so its recorded process exit code is 1.

Actual animation-curve regression tests exposed a separate publication detail:
Bevy consumes animation-graph asset events before their end-of-frame publication.
Starting a clip is insufficient to prove its first pose was evaluated. Newly
streamed animated NPCs now remain pending through two eligible animation passes
with a loaded graph/clip, and keep their targets attached through preparation.
Static whole models do not wait for an animation. The tests check the first
visible frame and re-entry with both possible relative Update orders.

The asset audit also found identical character materials imported independently
from split body/head/equipment GLBs. In the older fixed-shadow capture, 319
resident part-material paths represent 230 normalized exported material
definitions; 104 main-view paths represent 62 definitions. These are GLB JSON
comparisons, not measured runtime unique material handles. Instances of the same file already share material
handles; the duplication is between different part files. These source counts
are evidence of opportunity, not measured batch savings. In particular,
depth-only shadow batch keys can already omit material identity.

`materials/npc_shared.rs` now shares exactly equal, immutable NPC split-part
materials. It compares all reflected base/extension fields and explicitly
compares `cull_mode`, which Bevy excludes from reflection. Unsupported equality
and NaN values fail closed. Player/monster hierarchies, animated surfaces and
runtime-owned dye/effect copies are excluded. Weak cache handles permit unload;
original per-mesh handles preserve hot reload, so this targets batch grouping
rather than immediate material-allocation release. Removal of an unused source
prunes cache entries without rewriting unrelated meshes. `legacynpcmaterials`
restores separate materials for same-executable comparisons.

The current client suite passes 150 tests, with 13 existing fixture-dependent
tests ignored. The added coverage includes real animation-curve publication,
material equality/ownership/hot reload, unused-material removal without render
change ticks, residency hysteresis/budgets, and status expiry while unloaded.

Further source review identifies a narrow potential engine patch if batching
remains costly: Bevy already retains per-batch GPU buffer allocations, but
unconditionally uploads the instance and bin-offset arrays. Their contents need
updates when membership/order, input-uniform indices, bin indices or indirect
offsets change; bone poses alone do not require those uploads. The relevant
mutation methods and buffer fields are private, so a correct focused patch
belongs in the pinned `bevy_render` dependency rather than a parallel client
implementation. Unpacking bind-group caching would separately belong in
`bevy_pbr`, keyed by layout, four buffer IDs and metadata offset, with pruning
for removed views/phases and invalidation for buffer/layout/device replacement.
Bevy alternates phase buffers, so retaining only the previous frame's binding
would miss repeatedly. Neither engine patch is implemented here; the direct-draw
comparison measures the broader path before choosing further work.

### Confirmed batch fragmentation and direct-draw comparison

The optimized batch probe is `.local/map-batch-probe.exe`, SHA-256
`cbbeaeb94f777e46197b8bd555c9b65524929a25a163cfd7174f4da631ce5471`.
It includes the first-pose fix and material sharing; indirect rendering is its
default, and `directdraw` selects the comparison path. All comparisons below
use that same executable. The other game remained open as requested. No Rust
compiler or linker was observed at either endpoint of any timing run.

The settled material census demonstrates actual runtime sharing, beyond the
earlier source-material estimate:

| Kingscity plaza metric | Separate NPC materials | Shared NPC materials |
|---|---:|---:|
| NPC meshes remapped to shared material handles | 0 | 391 |
| Bin-unpacking jobs across views/phases | 558 | 422 |
| Jobs containing one instance | 367 | 277 |
| Alpha-mask jobs, in each of main and prepass | 117 | 80 |
| Shadow jobs | 290 | 228 |
| Instances across phases | 1,700 | 1,700 |

The 136-job reduction is 24.4%. Both captures contain the same 1,077 NPC meshes,
325,377 NPC triangles and 15,119 skin-palette entries, with 148 main-frustum
NPC meshes and 510 shadow candidates. Actor records, resident rigs, split-part
geometry/visibility, map geometry and the player match. Dynamic particles differ
by 12 triangles. Both also have the same eight resident pending NPCs, all
51.886–67.281 units from the player: these are inside the 70-unit preload radius
but outside the 50-unit animation/visibility radius. No pending NPC is within
50 units. The other 19 pending roots have no resident visual and are at least
70 units away. These are expected residency states, not a readiness deadlock.
The raw comparison and its explicit invariants are saved in
`kingscity-material-comparison.json`.

The separate instrumented profile series measured the following final-four-window
weighted wall means. These intervals overlap and must not be added together:

| Renderer interval | Separate materials | Shared materials | Shared + direct draws |
|---|---:|---:|---:|
| Phase-buffer writing | 2.150 ms | 1.652 ms | 0.042 ms |
| Bind-group preparation | 1.583 ms | 1.344 ms | 0.578 ms |
| Render-graph views | 4.450 ms | 4.108 ms | 3.950 ms |
| Render-graph submission | 1.588 ms | 1.370 ms | 0.763 ms |
| Render-graph total | 6.094 ms | 5.533 ms | 4.774 ms |
| Main-thread renderer wait | 7.035 ms | 5.789 ms | 2.886 ms |

Direct mode reports zero bin-unpacking jobs and metadata entries for both the
game camera and its directional-shadow view. That means this preparation path
was bypassed, not that the scene has zero render instances. It retains GPU
instance preprocessing and skinning. CPU frustum, visibility, layer, range and
shadow-caster checks remain active; GPU culling and indirect draws are disabled.
The client does not currently enable Bevy's GPU occlusion culling. Its separate
building/player cutout logic is unchanged. Direct shadow-command encoding costs
more (about 0.35 to 0.86 ms in the instrumented series), but the saved preparation
work more than compensates in this scene.

Default/direct captures show the same market actors, clothing, player, buildings
and ground shadows. Every split-part scene's geometry and visibility matches.
Animated decorative pigeons account for one fewer main-view mesh and two fewer
shadow candidates in the direct capture (1,162 to 1,160); dynamic particles
differ by six triangles. These
are explicitly retained in the audit rather than presented as exact pixel parity.

The lightweight timing series disables stage/GPU diagnostics and census capture.
Its Kingscity indirect/direct/direct/indirect order produced mean window p50
10.1225 versus 6.6425 ms, and p95 12.7400 versus 8.2375 ms: approximately 34.4%
and 35.3% lower respectively. Each figure averages the final two five-second
window percentiles from each of two runs, rather than pooling frame samples.
The individual p50 windows are 10.52/11.01/9.61/9.35 ms for indirect and
6.87/6.58/6.54/6.58 ms for direct. The result is provisional under desktop/GPU
contention and does not establish a universal FPS guarantee or eliminate all
loading/driver/desktop spikes. Raw runs are in `batch-timing-comparison.json`;
derived window means are in `batch-timing-means.json`.

Swerdfield's same indirect/direct/direct/indirect sequence produced mean window
p50 5.1450 versus 4.5750 ms and p95 6.7200 versus 6.3475 ms. These smaller
differences have overlapping windows, so the evidence for a speedup is weaker
than Kingscity's. Both modes retained 239 resident models, 1,439 meshes and
490 all-view mesh candidates in the final windows. Total entities varied from
5,921 to 6,082 across this live-server scene. The records are in
`batch-swer-timing-comparison.json`; this is not a deterministic scene census.

A separate legacy/shared/shared/legacy material series retained indirect draws
and gave mean window p50 9.5100 versus 8.9075 ms and p95 12.2500 versus
11.9650 ms. The windows overlap. The material fix's measured batch reduction is
stronger evidence than attributing a precise FPS gain to it under contention.
Do not add these percentages to the direct-draw result: the series ran at
different times, and the direct-draw comparison already uses shared materials.

The production `draw_mode.rs` plugin now selects direct drawing for native
clients at startup. `Play.cmd` and direct executable launches receive the same
default without launcher-specific environment changes. `DSOR_FLAGS=indirectdraw`
restores the platform default and wins if both draw flags are present. Browser
defaults remain unchanged; `directdraw` is an explicit browser override.
Camera requirements are registered before startup/later cameras, and the GPU
support cap also covers point/spot shadow views without enabling unsupported
features. Draw mode is fixed for the process because Bevy does not support live
toggling. Worker counts, executor choices, renderer pipelining, and the backend
retain their previous defaults. The evidence covers these maps on this machine;
much larger instanced scenes and other GPUs may favor indirect drawing and
should be measured before removing the fallback.

The final integrated workspace suite passes 152 tests, with 13 existing fixture-dependent
tests ignored. The added draw-policy tests cover all native/browser override
combinations, camera creation before and after the first update, the shadow
support cap, and fallback preservation without a GPU. Test output is in
`.local/logs/batch-default-tests.log`.

The optimized probe was then checked in direct mode with the first-pose and
sharing changes present. `kingscity-residency-lifecycle-final` passed all 26
checks: stable authoritative actor roots across travel, distant rig/picking
release, new visual children and picking on return, actor discard/republication,
and no forced rig load for an offscreen status. Live skin validation found no
stale or foreign joints. Return, discard and republication captures show posed
fixtures, the active status after return, and its removal with the actor. These
checks establish lifecycle correctness; the first-visible-pose guarantee is
covered by the real-animation regression tests rather than sparse screenshots.

The `kingscity-batch-final-route` and `swerdfield-batch-final-route` runs also
completed their out/back replays and exited normally, with no errors or live
joint-ownership warnings. Both returned to the specified starting positions.
These lifecycle/routes ran while the final source was compiling and enabled
expensive skin/census checks; their timings are deliberately excluded from the
performance comparisons. They used the recorded batch probe with explicit
`directdraw`, not the later production-default executable.

The final optimized executable is `target/release/dsor-client.exe`, also retained
as `.local/map-batch-final.exe`, SHA-256
`a5439dfd04a4771c0e694b77ff27ee7a1f665e74760e218d687ce3f357aa9b4c`.
The normal release build completed successfully in 8m40s. Final online audits on
both maps verify the production default without a `directdraw` flag: the main
3D and shadow views both report direct mode and zero bin-unpacking jobs. Explicit
`indirectdraw` restores indirect camera/shadow views, with 423 jobs in Kingscity
and 120 in Swerdfield in these captures. The minor variation in Kingscity's
shared-material job count between runs is retained in the raw logs. Artifacts
use `draw-default-final` and `draw-default-swer-final`; these audits are not
additional FPS benchmarks. Only the three known selection-map sidecar errors
appear; no new panic or renderer validation error was reported.

Final default/fallback screenshots preserve NPC/player clothing, poses, map
geometry and shadows on both maps. Kingscity retains 1,077 NPC meshes, 325,377
NPC triangles, 148 main-view NPC meshes and 510 NPC shadow candidates in both
captures. Its strict parity report still flags a transient boilertrike idle
effect (three meshes/60 triangles, all outside every view) and 50 particle
triangles; these explain the extra caster-exclusion markers and are retained
as explicit discrepancies. Swerdfield's parity check passes the controlled
invariants: 24 NPC meshes/11,116 triangles, nine main-view NPC meshes and 22 NPC
shadow candidates. Its particle phase differs by 30 triangles; 35 skeleton-warrior
helper entities differ without changing mesh geometry or visibility. These are
qualified visual/geometry checks, not claims of identical world snapshots.
Reports are `kingscity-final-draw-parity.json` and
`swerdfield-final-draw-parity.json`.

One final Kingscity fallback audit recorded a 4,794.20 ms frame. Its main `Last`
interval reached 4,788.758 ms immediately before the census log. In that window,
Update, PostUpdate, renderer handoff, render graph and readback/presentation
maxima remained below 10 ms. The screenshot had completed about 12 seconds
earlier. This localizes the stall to opt-in audit/Last work or a wait there,
where the diagnostic serializes and synchronously writes its JSON, rather than
establishing a GPU-draw or screenshot-readback stall. The underlying file-I/O or
scheduling cause is not proven. The event remains in the report; the final
correctness audits cannot be used to claim stable ordinary gameplay FPS.

Two final 40-second native runs with only lightweight `frameprofile` enabled
checked the production default after that audit stall. The final Kingscity
windows had p50 8.06/8.14 ms, p95 9.51/9.59 ms and maxima 11.23/10.63 ms.
Swerdfield had p50 5.86/5.98 ms, p95 6.44/6.67 ms and maxima 7.18/7.60 ms.
Neither final pair contained a frame over 50 ms. The long audit-stage stall did
not recur; this short check does not prove all possible hitches are eliminated.
No compiler was observed at either endpoint, and only the existing selection
sidecar errors appeared. These are separate production-build smoke checks,
not a fresh controlled speedup comparison; ambient conditions shifted from the
earlier ABBA series. Artifacts use `default-smoke-final` and
`default-smoke-swer-final`.

Before committing, the performance changes were isolated from the parallel
dialogue/UI task in a separate source snapshot. That exact snapshot compiled
and passed 142 client tests, with the same 13 existing fixture-dependent tests
ignored. Both NPC-export and character-render-state Python tests also passed.
Mixed files retain only performance changes and their necessary dependencies;
the UI residency test runs without dialogue resources, and the NPC exporter
test checks an existing loop-state override alongside event metadata. The
native measurements above were taken in the integrated workspace, with each
comparison using one identical executable; their hashes and timing caveats
remain unchanged. Snapshot validation logs are
`.local/logs/performance-push-tests.log` and `performance-push-test-build.log`.

## Pastures combat spikes — 2026-10-18

The reported native drop from 150–160 FPS to about 60 FPS was investigated with
repeated spell impacts in `a0007_pastures_pve`. Intermittent 17–35 ms frames were
reproduced. A sustained 60-FPS combat plateau was **not** reproduced, and no
gameplay or renderer behavior change is claimed as a fix.

The clearest located stalls are in presentation synchronization. In
`sustained-durable.log`, the 18:59:39.131 UTC frame took 26.89 ms, while the
previous main schedule took 3.27 ms. Rendering took 26.37 ms, including 23.45 ms
of preparation, with zero pipelines pending. The containing five-second window
recorded a 21.513 ms maximum around swapchain acquisition (normally about
0.05 ms). In `central-fireball.log`, the 19:02:47.720 UTC frame took 33.17 ms
with a 3.46 ms previous main schedule; its window recorded 27.816 ms around
swapchain acquisition. These windows each contain one large render stall.
This locates waiting for the display buffer, not equivalent shader/GPU execution
time. It does not distinguish the driver, compositor, GPU fence, or OS scheduling
as the underlying cause.

Other spikes have a different shape: main and render wall intervals lengthen
together, including transform propagation, bind-group preparation, or graph
recording. A 19.50 ms magic-missile frame also included 7.35 ms in player
occlusion. Those intervals include scheduling waits; they are not proof of a
combat-algorithm bottleneck. Casts, projectile updates, hit handling, material
flashes, damage-text updates, stats, and audio stayed below 1 ms per measured
call in the settled captures. The central iceball capture's largest reported
GPU pass averages were below 1 ms; these are neither worst-frame GPU timings nor
a complete GPU-frame total.

Same-executable captures, excluding the first two five-second windows:

| Capture | Casts during full run | Window p50 range, ms | Window p95 range, ms | Worst settled frame, ms |
|---|---:|---:|---:|---:|
| Entry area, magic missile | 89 | 5.02–5.25 | 5.88–6.68 | 26.89 |
| Central area, iceball | 89 | 4.82–6.36 | 5.96–8.90 | 19.53 |
| Central iceball, particles disabled | 85 | 4.89–6.16 | 5.74–8.43 | 34.87 |
| Central area, fireball | 67 | 5.81–6.20 | 6.60–7.84 | 33.17 |
| Central iceball, without stage/GPU profiling | 89 | 5.19–5.60 | 6.01–7.01 | 24.00 |
| Central idle, without stage/GPU profiling (35 seconds) | 0 | 4.60–4.66 | 5.39–5.60 | 8.00 |

The idle comparison does not select/highlight a monster, so its difference also
includes targeting/render-view work, not just the spell itself.
Disabling particles did not eliminate the stalls. They also occurred without
the extra stage/GPU profiling systems. These short runs do not justify changing
particle quality, combat behavior, renderer backend, or worker defaults.

The runner uses an isolated loopback server at `127.0.0.6` with separate fixture
stores. Monster HP is raised in that server process to sustain repeated impacts;
the shared server code/database and normal player account are untouched. The
central fixture starts at `(24.7711, 0, 53)` after client ground snapping. Captures
use the normal follow camera, saved borderless/fullscreen settings with VSync
off, and UI replay for logical window focus. They do not force desktop foreground
focus or stop other desktop workloads. Screenshots are excluded from these
timed runs. No compiler was observed in follow-up spot checks during these
captures; this was not continuous process monitoring. A new unrelated build was
observed after the final capture ended. The earlier exploratory runs overlapped
compilation and are excluded from this table.
The initial `sustained-profile` fixture did not raise the authoritative health
stat and stopped casting after killing the nearby enemies; it is not a sustained
combat measurement.

Evidence and runners are local under `.local/logs/combat-fps/`,
`.local/profile-combat-online.py`, and `.local/profile-combat-central.py`.
The separate optimized diagnostic executable is
`.local/combat-profile-instrumented.exe`, SHA-256
`ae19c3f3490dda5de2458d96b4ef7e4842c9b52cf17e2662f808f8e21d2dafba`.
It was built with `opt-level=3`, thin LTO, and 256 codegen units against the
existing native dependencies. All comparisons in the table use that executable;
they are not before/after optimization claims. The runner terminates only its
own client after 50 seconds (35 for idle), producing Windows exit code 1 by design. The logs
have no panic; missing trade-window and character-selection sidecars are existing
asset errors unrelated to the settled measurements.

`combatprofile` now logs frames at or above 16.67 ms and five-second system
mean/max timings; the former `frameprofile` threshold was 50 ms and missed
the reported class of hitch. `.local/ProfileCombat.cmd` launches the diagnostic
client against the normal login flow and saves a `live-*.log` in the evidence
directory. A foreground capture of the exact reported fight is still needed to
determine whether the user's full FPS drop shares these presentation stalls.

## Interface regression after the systems' windows (2026-10-21)

After commit `705409a` (the systems' windows on their 2026 layouts, audio,
quests), the user saw about 50–60 FPS online, down from 170–180. The upstream
lighting merge just before it was suspected, but was not the cause. The 10-20
build that already had that lighting, DLSS and `pe_ssao` still ran the Kingscity
benchmark view at about 185 FPS. With the same settings (`game-settings.json`:
DLAA, SSAO high, VSync off) and assets, the `705409a` build ran at about 95 FPS.
Only the fixed/update stage had grown, from 1.2 to 7.5 ms. Rendering,
PostUpdate and GPU spans were unchanged.

A per-system profile (the new `system-profile` build, see
[testing and diagnostics](testing-and-diagnostics.md#performance-diagnostics))
named the systems in one run:

| System (mean a frame) | `705409a` | Fixed |
|---|---:|---:|
| `ui::spawn_windows` | 5.954 ms | 0.011 ms |
| `ui::controls::systems::sync` | 1.002 ms | < 0.003 ms |
| `Update` schedule | 8.320 ms | 1.695 ms |
| All 62 `dsor_client::ui` systems | about 7.2 ms | 0.265 ms |

These are `system-profile` (trace) builds, whose spans add a little cost.

- `spawn_windows` prepared the layout of every closed, never-opened menu every
  frame (`systems::prepare` cloned and filled the large layouts of ten windows),
  then found the window hidden and dropped the result. It now checks visibility
  first; a closed window costs nothing.
- `systems::sync` built all ten system views every frame and compared them, open
  or not. It now builds only the open windows' views, when their inputs change:
  `Net::ui_revision` (bumped by every server command except the actor stream),
  the inventory's revision, `State`'s change ticks (`send` no longer writes it
  every frame), the catalogs loading, the class, and the clock's minute while a
  countdown shows.
- `quests::sync` rebuilt the quest list (text lookups, wrapping, a sort) every
  frame, which costs more with a real quest log. It now rebuilds on the same
  inputs and `QuestUi`'s change ticks.
- `spells::sync` cloned the spell model (with the skill book's map) into all
  windows every frame, marking every `WindowControls` changed. `sheet::rows`
  scanned the character sheet's layout every frame while the sheet was closed.

Interleaved release-build runs, `frameprofile` only, no compiler at either end,
final three five-second windows:

| Executable (SHA-256 prefix) | Window p50, ms | HUD FPS |
|---|---:|---:|
| `705409a` release (`b8c2961c0239`) | 11.67–12.51 | 81–85 |
| Fixed release (`6b23265879e3`) | 5.92–6.16 | 162–163 |

A second interleaved series with `stageprofile` compared the fixed build with
the 10-20 baseline (`4e1ce88b7aef`, fast profile). The baseline measured
4.71–5.21 ms p50 (186–197 FPS) and the fixed build 4.45–4.76 ms (182 FPS).
Fixed/update was 0.92 ms against the baseline's 1.19–1.26 ms. The desktop's
load shifted between the series, so compare rows only within a series. The
fixed build matches the pre-regression frame rate. Every fixed run logged
`ui work: 0 layouts prepared, 0 system views, 0 quest lists`.

The rules and the checks that now guard this are in
[docs/ui.md, "Per-frame cost"](ui.md#per-frame-cost). Logs: `kingscity-regress-*`,
`kingscity-sysprof-*` and `kingscity-ab-*` in `.local/logs/map-fps/`. Online
play was not re-measured. The online gain should be larger than offline, since
a real account's quest log and achievements made the old per-frame rebuilds
costlier.
