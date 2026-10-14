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

The appearance changes remain in the working tree after the inventory push.
World drops remain deferred.
