# Client feature gaps and implementation progress

Updated 2026-10-14. The baseline compares the local release261 Python server in
`.local/dso_server` with this Rust client. Opening a window does not mean its
gameplay service is connected.

## Current work

- [x] Audit server handlers against client networking and UI.
- [x] Verify the existing protocol tests (8 passed).
- [x] Retain authoritative inventory contents and apply incremental updates.
- [x] Populate equipment, bag items, stack counts and ingredient collection slots.
- [x] Render item tooltips using the original XML templates and item data.
- [x] Implement dragging between slots, dropping compatible stacks to merge,
      and Ctrl+left-click stack splitting with server-assigned item IDs.
- [x] Apply server position corrections, including `ForceTransform`.
- [x] Test protocol compatibility, inventory actions, UI behavior and corrections.
- [x] Push inventory work to `origin/release261` (`1fa3082`).
- [x] Resolve equipped models from the server's item, equipment-skin and appearance tables.
- [x] Apply transmog, dyes and equip/unequip changes to local and remote players.
- [x] Finish appearance regression tests and native visual inspection.

World drops, ground-item rendering and pickup are explicitly deferred.

## Feature backlog

| Feature | Client status at audit | Remaining work |
| --- | --- | --- |
| Inventory | Equipment appearance and vitals read from packets; item contents discarded | Current work above; later purchases, bank access and additional item actions |
| Equipped appearance | Direct legacy Skin lookup only; dyes/transmog ignored; remote updates could precede spawn | Implemented and validated below; remaining missing meshes require asset conversion |
| World drops | No ground-item controller | Render, update, remove and pick up drops; deferred |
| Death and revival | Monster deaths handled | Player death state, death dialog, respawn choices and revive updates |
| Position corrections | Local Move ignored; ForceTransform not decoded | Current work above |
| NPC interaction | NPCs render | Dialogue, service selection and encounter requests |
| Quests and props | Quest window navigation; DB4 prop rendering and server visibility/state updates | Log, objectives, accept/turn-in, markers, prop interactions and triggers |
| Vendors and crafting | Panels only | Offers, buy/sell/buyback, repair, salvage, crafting, sockets and customization |
| Talents | Skill book and hotbar assignment work | Allocation, resets, presets, wisdom/group/honor progression |
| Character statistics | Legacy base-stat calculator; stats asset absent locally | Release261 data, equipment and talent contributions, reliable maxima |
| Chat and social | Social tabs only | Chat connection, player search, group/guild rosters, invitations and group finder |
| Travel | Ordinary map exits work; travel stones render | Travel-map requests, unlocked destinations, travel items and home teleport |
| Events and rewards | Panels only | Event data, participation, notifications, daily rewards/challenges, achievements and wanted missions |
| PvP and rankings | Panels only | Flag, queues, acceptance, battleground state/results, player targeting and ranking rows |
| Mounts and cosmetics | No collection controller | Ownership, activation, favorites and pet upgrades |
| Character management | Login auto-selects a character | Roster screen, deletion, full creation controls and in-game logout |

## Server limits

Friend-list modifications are also missing on the server (`services/social.py`).
Ranking supports lifetime honor/PvP results; historical boards return empty
results (`services/ranking.py`). The prop-interaction path still leaves chest
loot unimplemented (`services/map_events.py`). These are not client-only gaps.

## Evidence and validation

- Server command routing: `.local/dso_server/services/dispatch.py`.
- Client packet application: `client/src/network/net.rs::on_command`.
- Client panel actions: `client/src/ui/controls.rs` and `controls/panels.rs`.
- Existing interface implementation: [UI documentation](ui.md).
- Baseline: `cargo test --offline --locked --release -p dsor-proto` passed all
  8 tests. The initial audit was static; the account endpoint was unreachable.

## Implementation results

`client/src/gameplay/inventory.rs` retains item records across complete allocation updates,
removals and NEW-to-OLD replacement maps. The client also decodes the server's
compact item-update layout correctly. `controls/items.rs` supplies icons, counts,
collection placeholders, tooltips and inventory interactions. The exported
catalog contains 15,334 item definitions and 393 live ingredient collection rows.
The original tooltip border and split dialog come from the XML export.

The client sends ordinary move/swap requests, storage-specific merge requests,
and split requests containing the amount **remaining** in the source stack.
Amounts and IDs remain server-authoritative. The ordinary bag exposes all
unlocked slots through eight 28-slot tabs. The premium bag exposes 28 slots
when membership is active. Loose inventory has its own paged, read-only view
and automatic server recovery.

The inspected server supported splitting but lacked manual merge operations
0x0D-0x10. Its local source now validates compatible stacks, limits quantities,
preserves overflow and sale values, rekeys changed items, sends both inventory
updates and persists the account. The portable change, including tests, is
[`tools/server_inventory_merge.patch`](../tools/server_inventory_merge.patch).
For a fresh copy of this server, run from its root:

```powershell
git apply --ignore-space-change D:/dso/tools/server_inventory_merge.patch
```

The patch is already applied to `.local/dso_server`; restart an existing server
process to load it. No live account inventory was changed during this work.

`ForceTransform` (113) now sets the authoritative absolute position and heading,
clears stale movement/cast holds, and updates local or remote players. Local
Move echoes are compared to the original sent record by tick: ordinary delayed
echoes are ignored, while changed coordinates are applied as corrections.
Map/character changes reset reconciliation history.

## Completed validation

The original 3-row/11-page interpretation was corrected after the user supplied
the bag rules: 4 x 7 slots in bags I-IX, membership-gated bag IX, read-only loose
items in bag X, and paging controls only in bag X. Ingredient tier selectors
now render above their background. `MembershipUpdate` (36) is decoded and
disables premium access when the server expires it.

The server already implemented expiry, persistence and automatic restoration,
but did not transmit loose inventory items. Its serializer now projects those
records into the overflow allocation for display while keeping them outside
the server's usable item table. The additional portable patch is
[`tools/server_inventory_overflow.patch`](../tools/server_inventory_overflow.patch),
already applied locally. Apply it from a fresh server root with
`git apply --ignore-space-change D:/dso/tools/server_inventory_overflow.patch`.
The membership/overflow regression suite passes 24 tests, including idle expiry,
reconnect persistence, renewal, and restoring a freed slot in the same request.

- `cargo test --offline --locked --release -p dsor-client -p dsor-proto -- --include-ignored`:
  **39 client tests and 10 protocol tests passed**, including the exported UI
  interactions, split dialog, drag cancellation, collection placeholders, stack
  replacement IDs, membership/overflow packets and applied corrections. The bag
  layout regression verifies 28-slot boundaries, tier artwork ordering, premium
  gating, overflow-only paging and refusing pointer actions on loose items.
- `python -m unittest discover -s .local/dso_server/tests -p '*inventory*.py'`
  with `.local` on `PYTHONPATH`: **8 tests passed**, covering merge limits,
  quantity/value conservation, allocation/identity changes and rejection paths.
- Native release build succeeded. Its existing unused `Label.bold` warning
  remains; test builds also report two existing Bevy deprecation warnings.
- Six native renderer captures were visually inspected under
  `.local/logs/inventory-layout/`: normal, ingredient tiers I and V, premium,
  expired premium and overflow. They confirm four normal/premium rows, visible
  tier buttons and changing collection quantities, a disabled expired premium
  tab, and paging shown only for loose items. The optimized executable was
  rebuilt successfully. Desktop input/screen-grab automation was unavailable;
  interaction behavior was checked in the UI and server tests rather than
  against a live account.
- All 393 ingredient entries have icons. Exported inventory/tooltip/split
  layouts load. The known missing `assets/characters/stats.json` error remains
  separate from this inventory work.
- The server patch passed an application check against saved original files;
  `git diff --check` passed. Detailed test/build logs are under `.local/logs/`.

## Equipped character appearance

The inventory work was committed and pushed as `1fa3082`. Appearance work now
resolves local equipment from the retained authoritative inventory, including
compact `ItemUpdate` changes. Remote spawn and update packets keep their skin
parts and colour. Updates are coalesced per actor and applied after spawning;
unchanged equipment keeps the current skeleton. Redressing preserves the
logical animation state and unrelated character attachments.

`tools/equipment_visuals.py` follows the server's resolution order: runtime
transmog or appearance ID through `_Template_ItemAppearance` and
`_Template_EquipmentSkin`, then the item's `Skin` or `EquipmentSkinId`. The local
release261 export contains 3,321 item mappings and 2,661 appearance mappings.
The old flat table format remains readable. Inventory UI exports regenerate
this table with their selected database; character exports accept `--database`.
To regenerate only the equipment mappings:

```powershell
python tools/equipment_visuals.py .local/dso_server/data/db/static.db4 assets
```

Visible slots 0-7 select the class skeleton's meshes and female variants where
available. Semicolon-separated graphics are expanded; two-handed meshes are
drawn once. Removing equipment restores base clothing. Missing converted
equipment meshes leave the base clothing available. Dyes clone each mesh's
material and use the existing shader's authored green-channel dye mask, so
another player wearing the same model keeps their own colour.

The installed graphics pack still lacks at least one referenced mesh for 40
wearable item templates (plus 116 non-slotted entries). Updating lookup data
does not create those missing graphics. The local audit details are in
`.local/logs/appearance-assets.json`; matching converted meshes are required
for those skins. The fixture outfits used below have all their meshes and masks.

The remote wire field historically called `armament` is a player-kind byte in
release261 (the server sends 5). Remote poses now use the advertised animation
set, with hands as a fallback on spawn. Rangers use their large-weapon set only
for two-handed equipment; local poses follow authoritative hand allocations.

Validation: **47 client tests and 2 Python export tests passed**. The new
tests cover lookup precedence, cached item records, compact transmog/dye updates,
unchanged-outfit preservation, attachment preservation, gender variants,
two-handed deduplication, ranger and remote weapon poses, dye isolation, and
server-encoded spawn/update/unequip batches. Fixtures are generated by
`tools/appearance_vectors.py`.

The optimized native executable built successfully. Three captures under
`.local/logs/appearance/` were visually inspected: `initial.png`, `changed.png`
and `unequipped.png`. They show the local male and remote female outfits,
independent red/green dyes, a blue local transmog with the remote female model
variant, and restored base clothing after unequipping. Skin diagnostics reported
no dead or foreign joints. Existing missing `characters/stats.json`, optional
debug-map sidecars and a zero-size text-span warning remain unrelated.

Explicit `--cam` coordinates are now respected during offline play, allowing
these close-up captures. `DSOR_INVENTORY_PREVIEW` accepts the appearance fixture
batches as well as ordinary inventory packets. Packet-driven ECS tests exercise
updates on existing entities; captures replay the final fixture states. No live
account or multiplayer session was modified. `git diff --check` passed.

The appearance changes were committed and pushed as `c151885` to
`origin/release261` on 2026-10-14.
World drops remain deferred.

## Smooth equipment transitions

The reported weapon/shield flicker came from rebuilding the complete skeleton
on every outfit change, resetting playback, and starting the incoming animation
at full weight. The current fix retains the skeleton, animation player, graph,
bone attachments and unchanged body/clothing meshes. Replacement parts load
hidden; the old outfit remains visible until its replacement and textures are
ready. Readiness checks the live scene instance, meshes and material textures
(including dye masks). Native testing exposed a Bevy scene whose recursive
dependency status remained `Loading` after its assets were present; the handoff
therefore checks the actual render assets instead of that aggregate status.
Publishing the new outfit also selects its weapon animation set.
`DSOR_CHECK_SKIN=1` also reports pending outfit loads.

Weapon posture changes blend over 220 ms. The incoming clip starts at its
existing weight (zero for a new clip), and outgoing weights decrease while the
total stays at one. A second change during the blend starts from the current
mixed pose. Equivalent movement and one-shot states retain normalized clip
progress across sets; common clips continue without restarting. Ordinary state
changes retain the existing 150 ms blend duration.

- [x] Preserve the live skeleton and defer replacing equipment until it is ready.
- [x] Blend weapon poses with normalized weights and retain animation progress.
- [x] Exercise real animated joint rotations, rapid reversal, shared clips,
      one-shot progress, and retaining the old outfit during loading.
- [x] Cover waiting for base, normal and dye textures before publishing outfits.
- [x] Final client regression run: **51 tests passed**.
- [x] Final optimized native build succeeded.
- [x] Three timed local/remote renderer runs, with 181 captured frames each.

All three final replays completed six equipment updates during capture, including
a rapid reversal, two-handed weapons, empty hands and sword/shield restoration.
Key transition frames were visually inspected. Both characters retained their
skeletons throughout the updates; there were no skin-binding warnings or stalled
outfits. Logs and frames are under `.local/logs/equipment-transition-verified1/`
through `equipment-transition-verified3/`. The final test/build logs are
`.local/logs/equipment-blend-tests-final.log` and
`.local/logs/equipment-blend-build.log`. `git diff --check` passed.

`tools/equipment_transition_preview.py` generates an initial server packet and
six timed equipment updates without opening an account. On the debug map,
`DSOR_INVENTORY_PREVIEW=<output>/initial.bin` supplies the initial outfits and
`DSOR_EQUIPMENT_REPLAY=<output>/sequence.json` schedules updates by frame after
both initial outfits are ready and the loading screen closes. Use `--play 0 0`
and the existing `DSOR_SHOT_BURST` capture option to inspect the transitions.

The transition fix is included in the character-selection and equipment update;
the earlier appearance commit is `c151885`. World drops remain deferred.

## Clothing and body coverage

The default torso item was already replaced when a torso was equipped, but
the client still drew every base body segment. Those segments protruded through
the replacement armor. The new selection step uses the original
`tables/gfxskintags.bin`: each named mesh has a region mask and layer. Only
visible higher layers hide matching lower layers; equal layers coexist.
This applies to local and remote players, including base clothes, boots,
gloves and transmog. The male/female mesh is resolved before its mask is used.
Authored NPC outfits retain their explicit part lists.

Coverage is calculated for the entire desired outfit, then published through
the existing atomic outfit swap. Removing equipment restores the newly exposed
body segments and default clothes. Missing converted equipment cannot hide the
fallback outfit; unknown coverage is left visible conservatively.

- [x] Read and export the original version-2 coverage table.
- [x] Regenerate both local part catalogs (1,714 human and 905 dwarf rules).
- [x] Cull lower layers when assembling player outfits.
- [x] Add regression cases for gender-specific cuts, body restoration,
      unavailable armor, equal layers, hidden occluders and legacy catalogs.
- [x] Two Python exporter tests passed.
- [x] Final client regression run: **54 tests passed**.
- [x] Optimized build and native before/after captures.

The human before/after captures show the protruding base geometry removed on
both local male and remote female characters, while exposed skin remains
visible. The initial scene drops from 34 to 22 rendered character surfaces.
Final human and dwarf replays each captured 181 frames and completed all six
equipment updates. Visually inspected frames cover different torso cuts,
removing boots/gloves, restoring default clothing and re-equipping the outfit.
Both characters retained their skeletons, with no new skin-binding warnings or
recurring outfit waits. The dwarf replay briefly retained the old outfit while
the replacement torso loaded.

Capture logs and frames are under `.local/logs/clothing-coverage-before/`,
`clothing-coverage-after/` and `clothing-coverage-dwarf/`. The final test/build
logs are `.local/logs/clothing-coverage-tests.log` and
`.local/logs/clothing-coverage-build.log`. `git diff --check` passed.

Regenerate older asset packs with `python tools/skin_coverage.py <export_win32>
<assets>`. The full character-table export also includes this step. The native
regression fixture can be generated with
`python tools/equipment_transition_preview.py <server> <output> --clothing`;
it swaps torso models, boots and gloves on local male and remote female players,
then restores the base outfit. The coverage changes are included in the
character-selection and equipment update.
World drops remain deferred.

## Character selection scene

The previous client automatically selected the first roster entry (and created
a default character on empty accounts). The character service now stays open
until the player chooses **Play Now**. The selection screen uses the converted
`charselectionwindow.bxml` geometry, textures, button states and roster-row
template, together with the original `a0000_char` forest scene, fire, fog and
lighting. Camera pitch/yaw, offset and FOV come from `_Globals.CharGen*` in
`static.db4`, exported as `characters/selection.json`.

- [x] Preserve full roster and operation-9 preview equipment replies.
- [x] Request operation 8 when browsing; send operation 3 only on Play Now.
- [x] Clone four XML roster rows, page additional characters, support keyboard
      selection, drag rotation and the original rotation buttons.
- [x] Resolve sparse equipment slots from `_Template_Item.SlotType`, preserving
      transmog, dye, gender variants, coverage culling and class proportions.
- [x] Resolve roster hair/beard indices and use `CharSelectIdle` animations.
- [x] Keep the previous portrait until its replacement outfit is ready.
- [x] Gate gameplay input/HUD until the actual game map loads.
- [x] Add name/class/gender creation and explicit deletion-confirmation dialogs.
- [x] Native rendering: all four classes, both genders, five equipped previews,
      portrait rotation and live-server selection-to-Kingshill handoff.
- [x] Final client regression suite: **67 tests passed**, including XML input,
      hidden-preview animation readiness and preselected roster scrolling.
- [x] Protocol regression suite: **11 tests passed**.
- [x] Python exporter/fixture compilation and `git diff --check`.
- [x] Final optimized executable build and capture check.

The roster's historical codec field `free` is release261's total
`character_slots`; remaining slots are calculated from the account roster.
Repeated initial rosters preserve selection and newer equipment replies; late
preview replies are cached by character ID and cannot switch the active model.
Character creation currently uses a compact dialog; recreating the separate
`chargenwindow.bxml` customization screen is outside this selection-screen work.
Purchasing slots, instance switching and paid appearance editing remain deferred.

`python tools/export_character_selection.py <static.db4> <assets>` regenerates
the camera/cosmetic/slot catalog; `character_tables.py` also runs this export.
`python tools/character_selection_preview.py <server-package> <output.json>`
creates a five-character fixture using the real server packet encoder. Run
`dsor-client a0000_char` with `DSOR_SELECTION_PREVIEW=<output.json>` to exercise
the roster without opening or changing an account.

Native regression captures are under `.local/logs/character-selection-verified/`
and `.local/logs/character-selection-online/`. The online run used the real
Python services on a separate loopback address with disposable player stores:
it browsed all five entries, returned to the first, rotated the preview, then
explicitly played the second character into Kingshill. The selection UI and
portrait were removed on handoff; the normal world camera and HUD returned.
No account on the normal server was changed.

`DSOR_SELECTION_REPLAY=<steps.json>` optionally automates this native check.
Steps are an array of objects with a `frame` and any of `select` (character ID),
`rotate` (radians), `capture` (PNG path), `play: true` or `quit: true`. Frames
advance only after the map and current portrait have loaded. Normal sessions
do not replay actions. The input regression separately drives the actual XML
hit regions through pointer selection, keyboard navigation, list scrolling,
resizing, rotation and deletion confirmation. It caught an authored overlap
between the portrait drag area and rotation arrows; explicit buttons now win
that hit test. Preselected characters beyond the first four rows scroll into
view automatically.

Final build and regression logs: `.local/logs/character-selection-build-final.log`,
`character-selection-client-tests-final.log` and `character-selection-protocol-tests.log`.
The rebuilt executable passed both native replays (seven offline captures and
eight live-service captures), including the fifth-row scroll and world handoff.
World drops remain deferred.

## Upstream release261 integration (2026-10-15)

Integrated upstream `release261` through `af0252e`: spell-effect timing and
materials (`7264583`), decorative town characters and sequences (`b4d1266`),
and asset-conversion documentation (`af0252e`). Upstream's literal `main`
remains at `9f3dd83`, already in this fork; the recent work is on `release261`.

Git merged the overlapping README, NPC, monster and exporter changes without
text conflicts. Character selection, inventory behavior, equipment pose
transitions, clothing coverage, English UI and the local-map overlay are
preserved. The merged client regression suite passed all **67 tests**.

Regenerated skills, decorations, monsters, material animations and stats in the
local asset pack. This exposed two exporter compatibility problems, now fixed:
release 261 uses `db.skill2.xml` for skill captions, and Windows' default text
encoding produced unreadable monster JSON or failed to write stats. The stats
exporter accepts both skill-caption filenames; both exporters write UTF-8.
The resulting catalogs load successfully (7,964 monsters and 200 level curves).

The optimized build and live-service replay passed: all five equipped previews,
rotation, paging, and entry into Kingshill with the selected second character.
Kingshill loaded 205 decorative characters and 34 effect sequences. The asset
exporter generated decoration files for 211 maps; it skipped 38 character tracks
whose clips are absent from this local asset pack. Existing optional sidecars
for the selection map remain absent. No new skin-binding warnings or panics
were observed.

Validation logs are under `.local/logs/upstream-release261-merge-*` and
`.local/logs/upstream-merge-*`; final native captures are in
`.local/logs/upstream-merge-online/`. `git diff --check` passed.

## UI shortcuts, defaults and window stacking (2026-10-15)

- [x] Bind the skills/talents window to **S**; focused text fields still consume
      shortcuts.
- [x] Initialize wisdom level, progress and available points to **0** in both
      the HUD and talents window. Replace the XML's sample available-points
      text and initialize labels that load after character values settle.
- [x] Reduce shared UI fonts by **20%**, including runtime item counts/tooltips,
      map captions and character-selection dialogs.
- [x] Keep each menu's artwork, text and runtime items in a separate depth band.
      Newly opened windows and clicked windows come forward as a whole; map,
      modal and tooltip layers retain their intended priority.
- [x] All **68 client tests passed**, including real exported layouts, populated
      inventory rendering, repeated reopening, focused text input, wisdom bars
      and late-loading labels.
- [x] Optimized native build passed. Inspected the skills window and travel map
      over a populated inventory: background artwork, item icons and stack
      labels stay below the front window, and reduced text renders correctly.

Wisdom progression beyond its default still needs server packet decoding.
Validation: `.local/logs/ui-order-tests.log`, `ui-order-build.log` and native
captures under `.local/logs/ui-order/`. `git diff --check` passed.

### Follow-up: contents disappearing after focus changes

The initial overlap captures did not exercise focus changes. Bevy 0.19 retains
the sort key of a transparent `Material2d` mesh when only its transform changes,
while sprites and text sort from their current transforms each frame. Lowering
a window could therefore leave its background drawing over its own contents.

- [x] Mark moved UI meshes for render-order refresh after transform propagation
      and before Bevy checks mesh changes. Stationary windows retain their cache.
- [x] Extend the real-layout regression to check Bevy's pending mesh updates,
      including both raised and lowered windows.
- [x] All **68 client tests passed**, including the retained-order regression.
- [x] Native replay: seven captures, six verified focus changes and a title
      drag from side-by-side windows into overlap. Twenty image-region
      comparisons confirm that exposed labels, icons, equipment slots and stack
      counts remain unchanged (allowing one byte of blend rounding).

Final executable: `target/release/dsor-client.exe`. Logs:
`.local/logs/ui-focus-tests.log`, `ui-focus-build-final.log` and
`ui-focus-pixels.log`; captures, entity snapshots and pointer fixture:
`.local/logs/ui-focus-verified/`. `git diff --check` passed.

## Indoor ambience and player visibility (2026-10-15)

- Adjusted the entrance faces of Kingscity's church and palace ambience boxes
  through persistent level overrides, and applied those transforms to the local
  ambience asset. The approach stays outdoors until the doorway/room entrance.
- Evaluate bubble membership after player movement propagates. Normalize sun
  directions before blending so a large volume does not accelerate the fade.
  Preserve disabled bubble flags in the conversion exporter.
- Cast five camera-to-player rays against map geometry. Obstructions receive a
  local feathered opening with fade/hold timing; floors, geometry behind the
  player, actors and shared materials are preserved. The depth/normal prepass
  matches the cutout, and buildings keep their original shadows.
- All **79 client tests** and **7 lighting/export Python tests** passed. The
  optimized native build passed; eight captures verify approach, entry, interior
  and exit at both locations. Both exits restore all affected material handles,
  and the replay logged no renderer errors or panics. Browser rendering was not
  exercised in this run.

Executable: `target/release/dsor-client.exe`. Validation:
`.local/logs/indoor-tests-final.log`, `indoor-build-final.log`,
`indoor-runtime-verified.err`; captures in `.local/logs/indoor-captures/`.
See `docs/fx.md` for the cutout behavior and offline replay format.

### Follow-up: reveal only a fully occluded character

Cutouts now require blocked rays to all five body samples: head, torso, lower
body and both sides. Tables, chests and narrow pillars keep their materials when
any sample remains visible. Separate meshes can still jointly hide the whole
character; exposing one sample closes an existing opening with the usual fade.

All **83 client tests passed**, including real-mesh regressions for low furniture,
narrow pillars, combined blockers and restoring the scene when one side clears.
The optimized native executable was rebuilt. Eight Kingscity replay captures
confirm full wall coverage still reveals the character and both porches/exits
have no active cutouts; the palace porch keeps its partially blocking wall intact.
The replay reported no runtime errors.

Validation: `.local/logs/full-occlusion-tests.log`, `full-occlusion-build.log`,
`full-occlusion-runtime.log`; captures in `.local/logs/full-occlusion-captures/`.

## Loading barrier, class preload and render warm-up (2026-10-19)

- Map loading now waits for a scene-readiness barrier besides geometry and
  props: navigation and ambience files, nearby NPC/decor scheduling, every
  spawned scene's live meshes, materials and textures, first animation poses
  and the player's complete outfit. Failed optional scenery settles with the
  asset server's diagnostics; a failed player appearance keeps the error and
  the close button visible.
- The selected class's skill, status, hit and summon effects (including the
  summons' selected skin variants) are planned from the skill, status,
  sequence, monster and outfit catalogs and requested four at a time once the
  initial geometry and props have settled. The handles stay resident for the
  class, now also through a map change: every actor is despawned before the
  next map's player arrives, and the first version dropped the handles in that
  gap and reloaded all of them on every transition (regression test added).
- Behind the opaque overlay, bounded batches of inert mesh/material samples
  (16 proxies and at most two glTF scene descriptions per batch; particle quads
  with the live particle material, decal quads, skinned proxies with
  placeholder joints) are drawn through the real game camera. The render world
  acknowledges uploaded meshes, prepared materials and compiled pipelines over
  two frames before a batch is removed; a failed pipeline or a batch over
  90 seconds fails the loading screen with a message. Nothing plays skills,
  emits particles or triggers audio.
- Progress: 75% map models, 15% class assets, 10% warm-up. `loading readiness:`
  is logged every five seconds with the current stage, `preload:` and
  `render warm-up:` report counts, and `loading screen: ready` closes.
- All **296 client tests passed** (14 ignored for missing converted assets),
  including the preload, warm-up, readiness and loading tests. Two
  pre-existing failures were fixed on the way: the graphics settings test
  still assumed vignette on by default, and the raknet replay test panicked on
  Windows without `HOME` instead of skipping.
- Native validation with the fast build `.local/preload-client.exe`
  (`.local/Build-Preload.ps1`, DLSS feature; `DLSS_SDK`, `VULKAN_SDK` and
  `LIBCLANG_PATH` point into `.local/dlss/`): mage and dwarf in Kingscity and
  the mage on `a0007_pastures_pve` (summons) all reached `loading screen:
  ready` (80/69/80 preloaded assets, 603/267/603 warm-up samples, 4-8 s from
  map start, no renderer errors or panics).
- One smoke surfaced a shader regression from the fog commit: `ambience.wgsl`
  read `mesh_view_bindings::fog`, which Bevy declares only for views carrying
  `DistanceFog`. The hover highlight camera has none and draws the hovered
  actor's own materials, so those variants failed to compile for that view and
  no outline could be drawn. The fog helpers are now guarded by `DISTANCE_FOG`.
  A hover replay (`.local/smoke_hover.py`: cursor on nearby NPCs with
  `renderaudit`) shows the highlight camera active in six audits with zero
  shader errors.

Logs: `.local/logs/preload/finish-1`, `finish-3`, `finish-summon-1`, `fixed-1`
and `hover-1` (`.log`, `.png`, route and UI replay files). The earlier runs
`warrior-0`, `ranger-2`, `dwarf-recheck-3`, `outdoor-summon-1` and
`final-native-1` are in the same directory.

## Dragged item frame and destroying items (2026-10-19)

- A dragged item now carries its frame (`icons_toolkit/frame_slim_*`) over its
  rarity background and icon, as its cell draws it: cells, the drag cursor and
  the new dialog share one helper (`ui::controls::items::sprites`).
- Dropping a carried item (bags or premium bag) out of every window, on the
  world, opens `confirmdialog` in its centred `DeleteItem` variant: the item in
  its slot (with its stack count), "Do you really wish to destroy this item?"
  and its name in its rarity colour. Confirm sends `InventoryCommand` (0x55)
  operation 0 with no arguments; the item stays until the server's
  `InventoryInfo` delta/final pair removes it (DSOR_Server
  `services/inventory_map.py`, the discard branch). Escape, Cancel, or the
  item leaving the bags closes the dialog. Equipped items are not offered:
  the server's discard leaves their look and stats in place. Dropping over a
  window, or while trading, does nothing.
- `DSOR_TEST_DESTROY=1` (offline, with `DSOR_INVENTORY_PREVIEW`) opens the
  dialog on the first carried item for captures.
- All **303 client tests passed** (14 ignored for missing converted assets;
  the 13 asset tests also pass locally), including four new destroy tests.
  Captures: `.local/logs/destroy-dialog/dialog.png` and `inventory.png`.

### Follow-up: DLSS / DLAA in every native build (2026-10-19)

The launched executables (`target/release` and the live client's `fast` build)
had been rebuilt without the opt-in `dlss` Cargo feature, so Settings showed
"NVIDIA DLSS / DLAA: Not in this build". The feature is now a default feature
of the client crate; its Windows/Linux-only dependency keeps browser builds
unaffected. A tracked `.cargo/config.toml` supplies `DLSS_SDK`, `VULKAN_SDK`
and `LIBCLANG_PATH` as paths relative to the workspace (`.local/dlss/`), with
environment variables taking precedence, so a plain `cargo build` includes
DLSS. CI builds the client with `--no-default-features`. The local build
scripts copy `nvngx_dlss.dll` beside their executables.

Verified: a type-check with no SDK variables in the shell; the release build
(9 m 38 s, thin LTO) and the live client build both compile
`rendering/graphics/dlss.rs` and carry the NGX symbols; the release executable
logs `DLSS / DLAA: available` on the RTX 4070 and reaches `loading screen:
ready` (`.local/logs/preload/release-dlss-1.log`); all **306 client tests
passed** (14 ignored). Every native build now initialises Vulkan; build with
`--no-default-features` for another backend.

## Server-to-client wiring: merge, vendors, party, guild, quests, travel, talents, logout, currency, traps, systems (2026-10-20)

The release261 server (DSOR_Server, `.local/dso_server`) already handles most of
what the client never sent; this pass wires the client to it, with every layout
checked byte for byte against the server's own codecs
(`tools/vendor_vectors.py`, `social_vectors.py`, `misc_vectors.py`,
`systems_vectors.py` write `crates/proto/tests/data/*_release261.json`; the
`crates/proto/tests/*_release261.rs` tests decode, round-trip and encode them).

- **Two server trees.** The live server (`python run.py`, UDP 2190) runs from
  `D:\_dso_retired\dso-server\dso_server`, which has the trade service and the
  2026-10-19 combat changes but not the stack merge; the git clone
  `.local/dso_server` has the merge (ops 0x0D-0x10) but had no trade. The trade
  service was ported into the clone as new files (`protocol/trade_protocol.py`,
  `services/trade.py`, its test) plus the dispatch and mixin lines; a
  handler-level merge test was added (`tests/test_inventory_merge.py`,
  `MergeHandlerTests`). The clone's suite: **1344 passed, 3 xfailed**. The
  retired tree was left untouched: restart the live server from the clone
  (`.local\Run-Online.cmd`) to get the merge, or port those files.
- **Stack merge** needs no client change: the client's InventoryCommand
  (`u32 item, u8 op, u32 count, args, u32, u32`) is what the server decodes.
- **Vendors.** `inventory::Transaction` is now the server's layout (counted
  entries of `bool, str offer, u32 item, u32, u32 amount`, then `str shop`,
  `bool`): `Transaction::buy/sell/restore`. The trading screen's offer cards are
  buttons (one copy per click), the buy-back cards restore, and a right click on
  a carried stack while a shop that buys is open sells it (the server's buyback
  list comes back as an Offer that keeps the shop). Layouts for the rest of a
  purchase's reply: QuickbarInfo 82, TimedOffersInfo 130, GameCurrencyChanged
  310 (kept in `Net.game_currencies`), the tier-spender progress 314.
- **Party.** `social::GroupMember` follows the server's PlayerSummary (role byte,
  note string); `GroupStatus` is kept in `Net.group` and drawn as party frames
  (`ui/controls/party.rs`, the `groupwindow` template: a row per member with
  name, level, leader mark and a health gauge, `Gauge::Group(n)`, from the
  actor's last known vitals; members elsewhere are greyed). Leave and found
  (socialscreen's buttons) send GroupLeave 160 / GroupFoundation 152; the
  foundation response, GroupingFailed 163 and the invitation answers reach the
  chat. The server auto-creates a group on an invite, so founding is optional.
- **Guild.** A kind-3 invitation opens the confirm dialog and answers with
  GuildInvitationResponse 171 (0 joins, 1 declines — the server refuses the
  notification codes 1/2). `player::GuildStatus` decodes the members and the
  IgnoredStatus 179 that follows; found/leave send GuildFoundation 168 (the name
  typed in the social window's search field) / GuildLeave 173.
- **Quests.** The log keeps per-task counters (`gameplay/quests.rs`:
  QuestLogInfo's finished mask and counters, QuestUpdate kinds 0/1); the quest
  log window lists the active quests with level, state and a cancel button
  (QuestCommand operation 2, only for quests.xml-deletable quests) and shows the
  open quest's text and objectives with "x/y"; the tracker
  (`activequestwindow`) follows one quest with previous/next. Task titles come
  from `interface/quests.catalog.json` (`tools/quest_catalog.py`, from the
  server's quests.xml, in `convert_assets.sh` with DSOR_SERVER) and the
  dialogue catalog's `quest.<quest>.s_<task>` texts. Delivery options (NPCInfo
  kind 3) appear in the NPC menu and send Deliver 104; a left click on a prop
  the server announced walks to it and sends PropInteraction 57
  (`world/props.rs`). QuestTriggerSignal is not sent: the server detects
  trigger volumes from movement itself.
- **Travel.** `player::Travel` is the native `str source, str map, str entry,
  bool`; the travel map's destination buttons send it (an empty source and
  entry is the selection the server reads first; it requires the map
  discovered). A right click on a travel item sends its use (0x13) then
  UseTravelItem 121 with the item's map (`travel_items` of the quest catalog,
  from `_Template_Item.TravelItemTarget`). Event portals (event-set exits) are
  still not drawn.
- **Talents.** SelectTalent 279 carries the rank delta; a talent card buys one
  rank, the preset buttons send EnableTalentPreselection 281; reset commands
  286/287 and the points/preset info 282/285 have layouts, the selection and
  points are kept in `Net`. The window does not draw the ranks yet.
- **Logout / switch character** (gamemenu) sends LogoutCommand 99; the
  countdown (PlayerLogoutInfo 327) is told in the chat. The server has no other
  logout than the character screen. Delete character was already wired.
- **Currency.** Badges of honour, draken and glyphs reach the currency bar
  (honor_display, event_display, cr_display); HonorPointsChanged 127 updates
  the honour; keyed currencies (materi, event coins...) are kept.
- **Dwarf traps.** NewTrap/TrapTripped/DiscardTrap drive a looping model per
  trap template (`gameplay/skills.rs trap_visuals`: dwarf_mine* ->
  effects_dwarf/dwarf_tripmine, the ranger trap, the fox trap) and the
  `dwarf_tripmine_explode` sequence on a trip.
- **Items.** A right click identifies an unidentified carried item (operation
  0x12). Layouts (not yet in a window): Salvage 49, Crafting 92 and its result
  93, RecipeBookInfo 311, GemRemovalResponse 86, ApplyItemCustomization 322 and
  ItemCustomizationInfo 320, the stickerbook's 303/304/305/307/308 (the album's
  entries are kept in `Net.stickerbook`).
- **Not wired, decodable:** events (ActiveEvents 222, SetEventParticipation
  120, the progress/participation infos), dailies (131/148/149), the Wanted
  Thief (356-359), titles (SetTitle 277), the rankings (requests 235/240/242/
  244, pages 236/241/243/245), PvP (the queue, battleground and match commands;
  `JoinMatchQueue` is now `u32 tick, i8 mode, i8 kind`), the premium shop's OCP
  (267/271/313; `OcpOffer` gained its `featured` bit). Their answers are kept in
  `Net` and the match-making ones are told in the chat; their windows are
  unchanged.
- **Trade** is the server-side port above; the client's trade window matched
  the server's `trade_protocol.py` ids (0x0F00-0x0F05, 0x0F10) already.

Verified: the protocol crate's **35 tests passed** (five new vector suites: 3
vendor, 4 social, 2 misc, 4 systems, over 82 server-generated vectors); all
**321 client tests passed** (14 ignored for missing converted assets), with
new tests for the party frames, the quest log and tracker, the merchant's
card actions and the quest entries; the server clone's suite: 1344 passed.
Not tried in the running game: the live server runs from the retired tree
(see above), so the merge, trade and the new windows await a restart from the
clone and an online session.

## The systems' windows (2026-10-20)

The windows the previous pass left unwired draw their data now, each an
authored template cloned per entry (`client/src/ui/controls/template.rs`:
`clone_at`, `set_text`, `icon_layers`, `selected`, `dim`, `without`; the party
frames and the quest log use the same helpers). `controls/systems.rs` builds a
`Views` resource every frame from `Net` and the systems catalogue
(`assets/interface/systems.catalog.json`, `tools/systems_catalog.py` in
`convert_assets.sh`: achievements and their categories, events, daily
challenges, Wanted missions, the album's books and stickers, recipes, skins,
socket prices, texts resolved from the language files) and rebuilds only the
windows whose view changed. A rebuilt window keeps its pages, toggles and texts
(`WindowControls::carry`; before, a rebuilt window kept its *old* buttons). The
clones' buttons carry `Sys:<kind>:<arg>` events, parsed into
`systems::Request`, queued by `interact` and sent by `systems::send`, which
also asks the server what an opened window needs (OcpOfferRequest,
ActiveEvents, WantedThiefInfoRequest, the ranking requests).

- **Leaderboards.** The PvP inlay lists the overall board (first category tab,
  OverallRankingRequest 240) or the honour board (HonorRankingRequest 235,
  board 4 — the only one the server records; class tabs filter), ten rows a
  page (the scroll buttons page, ScrollToMe asks for the player's page); our
  own rank (242/244) feeds the matchmaking window's standing.
- **PvP queue** (matchmakingwindow): the text tells the queue's state; Join
  battle joins the open tab's mode (JoinMatchQueue 206: 1v1, 5v5), leaves it
  (AbortMatchQueue 209) or accepts the match found (JoinBattlegroundResponse
  211).
- **Premium shop**: the server's Andermant offers (OcpOfferResponse 270) as
  cards on the deals and offers pages; Buy sends OcpBooking 271 (product key,
  method "external").
- **Events**: a tab per running event (EventUpdate 221), the open one's title
  and text, the time left (ends in `date`), its progress (224), a teleport to
  its area (the catalogue's TeleportTarget, Travel 93).
- **Collector's album**: a grid per category page (the front page lists the
  favourites), owned stickers bright and usable (UseStickerBookItem 309),
  the others faded with Add when a carried item fills them (307), favourite
  (305) and upgrade (308) buttons.
- **NPC desks** open from the NPC menu by profession (jeweller: gem desk;
  smith: salvage and workbench; crafting: workbench; tailor: transmog) and take
  a right-clicked inventory item (`items.rs` → `systems::pick`): the salvage
  desk melts up to nine picks (Salvage 49); the gem desk shows the item's
  sockets, sets a right-clicked gem into the first free one (InventoryCommand
  0x11 [item, socket]), takes one out (0x14 [socket]) and buys the next socket
  (0x15, priced from `_Template_SocketExtensionCosts`); the workbench lists the
  known books' recipes by the crafted item's category and crafts the open one
  with the carried ingredients (Crafting 92, gold or Andermant); the transmog
  desk lists the skins fitting the worn item picked for our class and applies
  the chosen one (ApplyItemCustomization 322; slot type 12 for the cloak).
- **Quest log pages**: achievements (categories with the open one's
  sub-categories, rows with progress and points from AchievementInfo 276, the
  earned title), dailies (four cards: Play accepts, Shuffle replaces,
  DailyChallengeSelection 148), the Wanted board (four cards: accept 358,
  refresh 359, the bottom button refreshes all; "not unlocked" until 357 answers).
- **Titles** (charactersheet): the titles our completed achievements unlock as
  rows (SetTitle 277 kind 1), the radio buttons for none (0) and the PvP rank
  (2).

Verified: **336 client tests passed** (14 ignored), with unit tests for every
window's `prepare` and for the request parsing; the systems catalogue
round-trips through its loader. Not tried online (the live server runs from the
retired tree); a release build with `DSOR_UI_OPEN` renders the empty windows.

## The systems' windows on their 2026 layouts (2026-10-20)

The previous pass drew the systems' windows, but most of them looked wrong:
backgrounds over their rows, artwork cut from the wrong part of its texture,
buttons that did nothing, edge panels not anchored, and two windows built on
legacy or invented layouts. Each window now follows its 2026 layout and every
button it shows either acts or shows its disabled face.

- **Anchoring.** A window's root Canvas lays its children against the whole
  screen (left `k` 0, right 1, centre 0.5). `tools/export_ui.py` exports that
  and `tools/patch_ui_anchors.py <fresh assets> <assets>` copied only the new
  `k` values into the converted layouts (35 windows: the quest tracker sits at
  the right edge again, the character sheet and workbench at the left).
- **Text outlines.** Labels with `TextEffect 1` (about 4,000 of 7,500) carry an
  outline in the export; the client drew none, so gold captions on light
  buttons were unreadable. Each outlined label now has four copies behind it
  in the outline's colour, each with a `Text2dShadow` on the opposite side:
  eight directions at the outline's radius (`ui::outline_copies`);
  `ui::follow_outlines` keeps them on the label's text, size, wrap width and
  alpha.
- **Draw order.** A button state's caption is drawn over the state's own
  artwork (its relief shading and frame follow the caption in the layouts):
  `ui::draw_order`. The quest log's achievement rows and overview put their
  backgrounds under their content (the game's controller does the same).
- **Quest log and tracker.** Objectives show an empty or ticked box
  (`icon_checkmark_empty` / `icon_checkmark`); long objectives wrap (34
  characters in the tracker, 60 in the log) and the tracker grows with them.
- **Achievements.** Rows show their icon, title, text and points; the
  overview shows each category's title, progress bar and count, scrolls to the
  points entry, and the recent achievements' panel shows again.
- **Daily challenges and Wanted.** The cards sit in their slots
  (`TemplateSlot0..3`, the template's left edge at each), the challenge
  artwork keeps the authored crop, the shuffle shows its price (40 Andermant,
  `_Globals.DailyAchievementReshuffleCosts`), the bonus bar shows the seven
  bonus rewards with how many dailies each needs and a tick once reached, and
  the next one. The Wanted bar shows the time to the board's reset (midnight
  UTC) and the refresh price; the privileged poster shows its count and opens
  the membership page.
- **Events.** The bar shows a page of rewards at a time (their page in
  `EventRewardItems "[page/]threshold:item:amount"`; the unpaged form was
  dropped before), evenly spaced with the fill rising between thresholds, the
  Andermant rewards as marks; Prev/Next turn the pages. The useful offers
  (`EventOffer`, the Andermant shop's) fill their slots and buy on a click; the
  collectible's icon sits by the count.
- **Collector's album.** Rebuilt on the authored `Categorypage` and category
  list: categories with their books under the open one, the front page with
  its banners (counters switch them), the new stickers and the favourites;
  a book's page with its text, a 6 x 4 grid, the preview (name, origin,
  background, favourite flag, its offer) and the book's progress with its
  rewards (`StickerbookRewardItems`). A click previews a sticker, a second
  click uses it. The exporter no longer adds the invented `AlbumPage` /
  `AlbumCategory` widgets (`tools/collector_catalog.py` is gone).
- **Transmog desk.** The 2026 customization window: skins and dyes tabs, the
  eight slots (the worn item, a reset once it has a skin or dye; a click picks
  the slot), a 5 x 4 skin grid (effect, shop, owned and worn flags) or the dye
  grid, owned and shop filters, the collection bar, prices as the server
  reckons them (`_Template_DynamicLevelCosts`; a carried dye is free) and
  Apply (disabled until something would change). The eye toggles have no
  server command and are hidden.
- **Gem desk.** `gemdeskwb` (ten sockets) replaces the legacy `gemdesk`;
  "Open Workbench" opens the workbench.
- **Workbench.** The authored tabs (their icons), the category tree with the
  known and the locked recipe books, the filters, the book's text and usable
  items, its recipe's ingredients with their amounts (`"template:amount"`;
  before, the amount suffix stopped every craft), result, prices, the bulk
  amount and Craft (gold or Andermant, disabled without the ingredients). The
  crafting animations' smoke no longer covers the slots.
- **Shop and membership.** Offer cards show their amount; the membership
  page's shop buttons open the shop.
- **Catalogue.** `tools/systems_catalog.py` adds the daily and Wanted figures,
  event pages, currency rewards, collectibles and offers, album rewards,
  origins, backgrounds, offers and banners, the skins' details, the dyes and
  the level price factors.
- **Fixtures.** `tools/ui_fixture_packets.py` writes the server's packets for
  offline screenshots (`DSOR_PACKET_REPLAY`), now with bonus challenges,
  amount-bearing recipes and a wielded sword; `DSOR_UI_REPLAY` steps can
  right-click (`"right": true`).
- **Locked recipe books** show how to get them: their text with the unlock
  item named (the book's title for `{0:s:itemname}`) and the origin box
  (`_Template_RecipeBook.LockedIcon`, "Quest reward", "Recipe"...).

Verified: **341 client tests passed** (15 ignored), with tests for every
rebuilt window and for `draw_order`'s inputs; offline captures of the quest
log (quests, achievements overview and rows, dailies, Wanted), the events
(two events, a second bar page), the album (front page, a category, a
preview, a book), the transmog desk (skins, a chosen skin, dyes), the gem desk
and the workbench (tabs, categories, a locked book) from the fixture packets.
Not tried online. Known gaps: the fixture's worn helmet has no converted icon
(its slot shows empty), the replay's right click lands after the inventory
reads it (the desk pick is covered by the unit tests only), scroll handles
cannot be dragged, and the first client run
after a build sometimes holds the loading screen at 75 % of the effect
preload (unrelated to these windows; the next run loads).

### Follow-up: the quest tracker no longer blocks the game bar (2026-10-21)

Once anchored to the right edge, the quest tracker covered the game bar's
buttons above it (Premium, autoplay, fullscreen, the game menu): none could be
hovered or clicked. Its `LabelFull` / `LabelCompact` containers are invisible
(no texture, alpha 0) yet span x 720..1024 from the top of the screen
(y 0..325), and the tracker (depth 46) is drawn over the game bar (30), so
those empty rectangles took the pointer. Invisible artwork now leaves the
pointer to what lies under it in the tracker, as it already did in the chat
windows (`ui::passes_pointer`); the visible frame (from y 71) still takes it.
Test: `the_quest_tracker_leaves_the_game_bar_above_it_to_the_pointer` (fails
without the fix); 103 UI tests passed with the converted assets.
