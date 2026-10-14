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

World drops, ground-item rendering and pickup are explicitly deferred.

## Feature backlog

| Feature | Client status at audit | Remaining work |
| --- | --- | --- |
| Inventory | Equipment appearance and vitals read from packets; item contents discarded | Current work above; later purchases, bank access and additional item actions |
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
