# The original client interface

## The 2018 data

- `export_win32/ui/<window>.bxml`: one window per file, Nebula binary XML.
  A tree of widgets (`Label`, `TextLabel`, `Button`, `SlotButton`, `Slot`,
  `ProgressBar`...), each with a `rect` in fractions of a 1024 x 768 reference
  screen, its anchor to its parent (`HorizontalParentAlignment`,
  `VerticalParentAlignment`; none = top left), its artwork (`texture` +
  `meshGrpIdx`) and text (`font`, `fontSize`, `fontColor`, `Text`).
- `export_win32/meshes/ui/<window>_s_0.nvx2`: the window's artwork as one mesh;
  a widget's `meshGrpIdx` is a group of it, with its own triangles, atlas UVs and
  vertex colours (quads, nine-slices, the globes' gradient polygons).
- `export_win32/textures/ui/*`: the atlases; `textures/icons/*`: item and skill
  icons (`_Template_Skill.IconBrush` in `db/static.db4`).
- `export_win32/language/<lang>/*.xml`: the `gui.*` texts.

DSO_Godot's `export_ui.py` and `dsoexp/bxml.py` decode all of it; this
repository's `tools/export_ui.py` reuses them.

## The export (`tools/export_ui.py <export_win32> <assets> <DSO_Godot> [window ...]`)

Every window by default (`--hud`: the four the HUD needs) into
`<assets>/interface/<window>.ui.json`, plus `skills.icons.json` (skill id ->
icon), `loca_fr.json` / `loca_en.json`, and the atlases and icons as PNG under
`<assets>/textures/`. The current source tree contains 160 windows.

Each widget: its id path, its rect in reference pixels, `k`, its artwork
(triangles, UVs, vertex colours, atlas) and its English text. UI, item and skill
catalogues use the English localization; missing entries keep their source key
or template fallback instead of switching languages. Re-export the assets after
updating the localization code: translations are baked into the JSON files.
`k` is how far the widget follows the screen's extra width: the client scales the
interface by the window height and widens the reference screen; widget sizes are
fixed and only full-screen top-level canvases stretch, so a widget's x is its
1024 x 768 x plus k x (reference width - 1024). The bottom bar (centred) has
k = 0.5, the money (top left) 0, the menu buttons (top right) 1.

Button state subtrees retain `button` (the owner's widget path) and `state`
(`normal`, `mouseover`, `pressed`, `mouseoverpressed`, `disabled`). Button
widgets retain their `type`, `event`, `enabled` and `click_through` fields.
The renderer selects one state at runtime; these states must not be baked into
the mesh as permanently hidden or simultaneously visible artwork.
Button state backgrounds render before the button's common icon/text children;
source XML order alone can put an opaque button face over its icon. Glow children
inside a button state follow that state rather than being suppressed globally.

What the game shows only in other runtime contexts is exported `hidden`:
cooldown covers, glows, the "buy a slot"
offers, the alternative stacks DSO_Godot already spots, the character preview
area (`figur`, a runtime render target), placeholder icons (`icon_dummy`, "?"),
templates outside a window's root (`ROOTS`: the inventory's drag ghosts, the
payment variants of the money display). Template slots the game clones are laid
out (`GRIDS`: the inventory bag, 7 x 4 empty slots, fitted to the area's width).
Readable, lossless copies of the source BXML are also written to
`interface/xml/<window>.xml`. BXML is a binary XML tree, not a compressed text
file; the exporter decodes its node, attribute and string tables.

## The client (`client/src/ui`)

A 2D camera over the 3D one (sharing its target textures: same HDR and MSAA),
orthographic, 768 reference pixels high. Each window is built once into a few
meshes (one per run of widgets sharing an atlas, anchor and visibility gate,
the draw order kept by depth),
drawn by `UiMaterial` (`ui.wgsl`: atlas x vertex colour in gamma space, cut to a
clip rectangle). A resize only moves the per-k parents. Texts are Text2d.

Drawn:

- always: the current `bottombar/Canvas/Label_new` layout: horizontal health and
  resource bars, XP and wisdom bars with level labels, five quick slots plus
  left/right mouse slots, mount and home teleport; the money (`currencybar`), the menu buttons
  (`gamebar`), and the central navigation (`ingamemenu`);
- menus: inventory (I: equipment slots, eight 7 x 4 bags, premium and loose-item bags),
  character sheet (C), current skill/talent window (K: `skilltalentswindow`),
  quest log, travel map, local map, shop, social window, PvP matchmaking,
  leaderboards, events and game menu (Escape). Natively `DSOR_UI_OPEN=inventory,...` opens
  them from the start (screenshots).

Live values: health and resource (`Net::health` / `resource`), level, XP, wallet,
the character sheet's available figures, the quick slot bar (`Net::bar`, wire slots: 0 left
button, 1 right button, 2-6 keys 1-5), the character name. Offline (map viewer)
the map viewer shows resting values (80 % health, 60 % resource, 35 % XP, the mage's
skills). Wisdom's display is restored, but its server progression is not decoded
yet: it shows an unknown level rather than a fabricated value when connected.

## Interaction (`client/src/ui/controls.rs`)

The window controller resolves source events (such as `Close`, `NextPage`,
`InventoryReturn`, `IngredientBag`) and controls identified by widget path
(bag tabs, character tabs and navigation buttons). Unimplemented actions use
the authored disabled state. A click activates on release over the same
control that received the press; dragging off, losing focus, or closing the
window cancels it. The highest visible window owns the pointer at its bounds.
The game menu blocks pointer input to the windows and world below it.

Top-navigation panels (except the location-map overlay) and the game menu can be dragged with the left
mouse button from the top 32 reference pixels of their painted frame (scaled
with the interface). The cursor changes to a hand there; close buttons retain
priority over dragging. Clicking a window brings it forward below the modal
game menu. Artwork, text, gauge clipping and hit regions share the root's
offset. Positions survive closing/reopening during the session. Dragging and
resizing keep at least 64 reference pixels of the title reachable; losing focus
or hiding a window cancels its drag. The HUD bars stay fixed.

Inventory equipment, ingredients, trinkets and mercenary pages are mutually
exclusive. The character sheet likewise selects one of its five tabs. Switching
between the character sheet and skills/talents closes the previous panel.
Escape closes the topmost menu, or opens the game menu when none is open.
The central navigation opens inventory, character sheet and skills; the gamebar
opens the game menu, toggles fullscreen and opens the membership tab.

All eleven central navigation buttons open their corresponding panels; their
pressed state follows actual window visibility, including keyboard toggles and
close buttons. `controls/panels.rs` selects quest, social, skill/talent, shop,
leaderboard and travel-region pages. List/card templates stay hidden until data
is supplied. Checkboxes and filters keep local state. Search and voucher fields
support Unicode text, caret navigation, deletion and Ctrl+A; focused fields
consume game shortcuts. Backend actions such as buying, redeeming or joining a
battle remain disabled until their services are connected.

The top-right bar selects the standard background, fullscreen/menu controls,
and the non-premium badge. Autoplay, membership alternatives and regional
sign-in/season-pass controls are conditional variants, not extra bars to draw
together. Common highlight layers are hidden; authored hover/pressed highlights
are selected by the owning button. Identical nested icon meshes are drawn once.
The skills window's background layers are painted before its controls, and
authored multiline labels wrap within their source rectangles. Hidden legacy
gauges and quick slots cannot contribute artwork, fill bounds or hit regions.

The original `locationmap` is a runtime drawing area with no map bitmap. Its
client implementation (`local_map.rs`) projects the loaded navigation mesh into
a Path of Exile-inspired transparent overlay: muted gold boundary lines, a faint
walkable-area tint and a mint player arrow that follows position and facing.
Terrain and player markers share the scene camera's ground-plane projection,
so map directions match the world view without a mirrored vertical axis.
Shared triangle edges are removed, so the overlay shows terrain boundaries.
Tab cycles from off to a compact upper-left preview, then a larger centered
preview, then off. The navigation map button uses the same cycle. Escape hides
the map; its next opening starts compact. Both sizes, including their captions,
are click-through and have no header or settings controls. A right-aligned
localized map name sits just below the preview's bottom-right corner; names
come from the original English `maps.names` localization, matching the rest of
the current interface. The exporter writes them to `interface/maps.names.json`.
Until the settings window is implemented, the F3 debug menu's "Map transparency"
row adjusts terrain transparency in 5% steps from 0-100% (default 45%), while
the player arrow and map name stay visible.
The setting survives restarts in `map-settings.json` natively or `dsor.map` in
browser localStorage. Re-export the UI to generate the localized name table.
For native captures, use `DSOR_UI_OPEN=locationmap` and optionally
`DSOR_MAP_VIEW=expanded`; the default capture shows the compact map.

Inventory contents now follow the server's item records and slot allocations.
The eight ordinary tabs each address 28 slots (4 rows x 7 columns), accounting
for the server's 224-slot bag. They have no footer arrows or page counter.
Bag IX is the 28-slot premium bag, enabled only by the server's active paid
membership and premium capacity. Expiry disables it and returns its open view to bag I.
Bag X contains loose inventory items waiting for space. Only that bag displays
the paging footer (21 items per overflow page, with the fourth row reserved for
paging). These items can be inspected but cannot be dragged,
split, used, swapped or receive drops. The server automatically moves them into
free inventory slots and sends the new allocation. Bank overflow remains in the
bank's separate recovery queue. Unavailable ordinary cells are dimmed.
Equipment and bag cells show icons and stack counts. Ingredient category and
rarity tabs populate the collection grid from `_Template_IngredientBag`, including
dimmed uncollected items and owned quantities from the ingredient allocation.
The five tier selectors retain their original normal/hover/selected artwork;
the ingredient background is painted first and unused recipe/core overlays stay
hidden, so these selectors remain visible.

`tools/inventory_catalog.py`, called when exporting `inventory`, writes
`inventory.items.json` from the server database and original localizations/icons.
Export `inventory tooltip unstackwindow` together. Hovering an item shows its
name, description, level, enchantments, sockets and quantity in the original
tooltip border. Ctrl+left-click opens the original split dialog: choose the
amount using its input, +/- buttons or slider, then confirm with its button or
Enter. The new stack goes into the first free slot in the same storage.

Drag items between slots to move/swap; drop a compatible stack onto another to
merge up to its maximum. Excess remains in the source slot. Escape, loss of
focus or a stale inventory revision cancels a drag. Splits and merges wait for
the server's replacement IDs and amounts; an empty final mutation packet does
not erase retained item records. Dropping outside inventory cancels the drag.
The split dialog is modal and cannot send movement to the world underneath.
See [implementation progress](client-progress.md) for the required local server
merge patch and remaining inventory services.

For native visual inspection, `DSOR_INVENTORY_PREVIEW=<packet.bin>` replays
InventoryInfo and MembershipUpdate from a binary server message in offline
`--play` mode. It changes no account, and offline inventory mutations stay
disabled. With packet replay enabled, `DSOR_INVENTORY_VIEW=ingredients,3`,
`premium` or `overflow` selects the view for renderer captures without desktop
input. Use `DSOR_UI_OPEN=inventory` to open the panel.

Hotbar skill clicks enter the existing skill casting path, with its ordinary
resource/cooldown validation.

`tools/ui_catalog.py` instantiates the current spell book from
`_Template_SkillTalent` joined to `_Template_SkillUIConfig` and `_Template_Skill`.
Pass `--database <server>/data/db/static.db4` to `tools/export_ui.py` to use the
server's exact release data. Rows/columns come from that table, and wire skill
indices are `rowid - 1`. The four class grids contain 15/18/16/16 live active
spells plus their passive talent cards. Only the local character's grid and
class backdrop are visible. The generic blue overlay and old preset strip are
excluded. Names use `db.skill2.table` / `db.skilltalent2.table` localization.

Drag an available spell onto LMB, RMB or keys 1–5. Dragging an existing hotbar
spell swaps its slot with the target; dropping elsewhere or pressing Escape
cancels. A four-reference-pixel threshold keeps ordinary slot clicks working.
The lock button prevents edits; the bar starts unlocked. Ghost icons and
tooltips do not intercept input. Online availability comes from `SkillBookInfo`
(owned and not hidden), while offline play allows all class spells for testing.
`QuickSlotsInfo` is retained in full, including item slots and inactive bars.
Each completed edit sends `QuickSlots` (0x51) with exactly 17 slots and bar index
0. An incomplete server snapshot cannot be edited. The server persists this
command; the client updates the displayed bar immediately.

The navigation audit selects `leaderboards` (PvP/PvE/event), `skilltalentswindow`
and `questlog`; the older `pvpleaderboards`, `skillwindow2`, `talentwindow`,
`questscreen` and `questscreen2` are not loaded by runtime navigation. Inventory,
character sheet, social, shop, matchmaking and events each use their named
current layout. Ranking categories are cloned from the current templates using
`_Template_LeaderBoardCategory` and `RankPriority`. Only the selected group's
table and the appropriate overall/category header render. Class filters and
category selection work locally; rows, seasonal rewards and pagination await
the ranking service.

Visible artwork and buttons populate `UiRegions` using the same page/state
gates as rendering. Hidden pages cannot intercept input. These regions integrate
with `ui_input` so a UI press remains captured through its release and cannot
move or attack in the world underneath. A left press started in the world keeps
its ownership while held over UI, so crossing a panel does not interrupt walking
or start a window drag. A new press over UI is captured normally. Scroll still
respects the UI under the cursor.

## Missing

- Text uses `assets/fonts/tahoma.ttf`; the original bitmap font and text effects
  are not reproduced.
- Inventory sorting, bank access, slot purchases and additional item actions
  (use, sell, repair, identify, sockets and customization) need their controllers.
  Equipment comparison tooltips remain.
- Character preview render targets and list scrolling are not implemented.
- Server-backed quests, catalogue entries, social rosters, event payloads,
  leaderboard rows and talent allocation are not bound yet; opening their
  panels and switching their UI pages works independently of those services.
- Context-specific dialogs outside the top navigation (crafting, chat, etc.)
  still need their owning gameplay controllers.
