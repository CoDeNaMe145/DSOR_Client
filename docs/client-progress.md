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
| Quests and props | Quest window navigation only | Log, objectives, accept/turn-in, markers, prop interactions and triggers |
| Vendors and crafting | Panels only | Offers, buy/sell/buyback, repair, salvage, crafting, sockets and customization |
| Talents | Skill book and hotbar assignment work | Allocation, resets, presets, wisdom/group/honor progression |
| Character statistics | Legacy base-stat calculator; stats asset absent locally | Release261 data, equipment and talent contributions, reliable maxima |
| Chat and social | Social tabs only | Chat connection, player search, group/guild rosters, invitations and group finder |
| Travel | Ordinary map exits work | Travel-map requests, unlocked destinations, travel items and home teleport |
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
- Client packet application: `client/src/net.rs::on_command`.
- Client panel actions: `client/src/ui/controls.rs` and `controls/panels.rs`.
- Existing interface implementation: [UI documentation](ui.md).
- Baseline: `cargo test --offline --locked --release -p dsor-proto` passed all
  8 tests. The initial audit was static; the account endpoint was unreachable.

## Implementation results

`client/src/inventory.rs` retains item records across complete allocation updates,
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
