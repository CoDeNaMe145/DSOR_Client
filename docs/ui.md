# The original client interface

## Native account screen

`client/src/screens/login/view.rs` renders sign-in, registration and connection
settings with the exported game artwork: the chapter-zero castle illustration,
`struktur` panel texture, `deco_elemente_5` window rails and button states,
`deco_elemente` recessed inputs, and the character-selection primary button.
It uses the game's Tahoma font. Game atlas regions are sliced or mirrored at
runtime.

The artwork preloads behind the shared loading screen. Wide windows place the
form beside the castle; narrower windows center the form and scale it to keep
every control visible. Fields have focus and selection feedback, buttons use
hover/pressed artwork, and pending sign-in uses an indeterminate loading bar.
Tab / Shift+Tab move between fields, Enter submits, and Escape returns or cancels.
Show / Hide reveals passwords temporarily; submission and page changes restore
masking. Passwords and tokens remain in memory only.

The footer identifies the project as non-profit and open-source. Its GitHub
button opens `https://github.com/DSO-Reconstruction/DSOR_Client` in the default
browser on release, with a gold glow, lift and brief icon tilt on hover.
The embedded GitHub mark comes from Octicons; its source and MIT license are in
`client/src/screens/login/icons/`.

## Game cursors

`client/src/ui/cursor.rs` uses the original `textures/mousecursor` artwork as
native custom cursors, with the top-left tip as the click hotspot. The gold hand
is shared by login, loading, character selection, menus and ordinary movement.
All styles are resampled from 64×64 to 42×42 before native cursor creation,
making them about 35% smaller while retaining the full artwork and click hotspot.
World hover switches to the sword for enemies, speech bubble for NPCs, shop
cursor for merchants, and gears for loot and exits. UI input and dragging keep
the hand; free-camera cursor visibility still follows the camera controls.

**Settings > Mouse > Classic cursor** selects the original gold arrow
(`mousecursor/arrow.png`). Like the original client's `classiccursor` option,
it keeps the arrow across hover targets, menus and login. It is off by default;
Apply switches immediately and saves `classic_cursor` with the other preferences,
while Cancel keeps the previous style. Reset restores the modern style in the
draft. Existing settings files without the field keep the modern cursors.

`tools/export_ui.py` also converts the complete mousecursor set (including its
colour variants) into `assets/textures/mousecursor/*.png`. Re-run that exporter
for an existing asset folder. Cursor images keep their CPU pixels for winit and
use the same asset paths in native and browser builds.

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
k = 0.5, the money (top left) 0, the menu buttons (top right) 1. A window's root
Canvas lays its children against the whole screen, whatever its own rect: a
child aligned left gets k = 0, right 1, centred 0.5 (the quest tracker at the
right edge, the character sheet and the workbench at the left).
`tools/patch_ui_anchors.py <fresh assets> <assets>` copies only the `k` values
of a fresh export into converted layouts.

Button state subtrees retain `button` (the owner's widget path) and `state`
(`normal`, `mouseover`, `pressed`, `mouseoverpressed`, `disabled`). Button
widgets retain their `type`, `event`, `enabled` and `click_through` fields.
The renderer selects one state at runtime; these states must not be baked into
the mesh as permanently hidden or simultaneously visible artwork.
Button state backgrounds render before the button's common icon/text children;
source XML order alone can put an opaque button face over its icon. Glow children
inside a button state follow that state rather than being suppressed globally.
Within a state, its captions are drawn after the state's own artwork (the
client's `draw_order`: relief shading and frames follow the caption in the
XML). Texts with `TextEffect 1` keep their outline (`text.outline`: size and
colour); the client draws it with four copies of the text behind it, each
with a shadow on the opposite side (eight directions).

What the game shows only in other runtime contexts is exported `hidden`:
cooldown covers, glows, the "buy a slot"
offers, the alternative stacks DSO_Godot already spots, the character preview
area (`figur`, a runtime render target), placeholder icons (`icon_dummy`, "?"),
templates outside a window's root (`ROOTS`: the inventory's drag ghosts, the
money display's other variants). Template slots the game clones are laid
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
Authored labels and runtime UI text use a shared 80% text scale, including item
counts, tooltips, map captions and character-selection dialogs.

Drawn:

- always: the current `bottombar/Canvas/Label_new` layout: horizontal health and
  resource bars, XP and wisdom bars with level labels, five quick slots plus
  left/right mouse slots, mount and home teleport; the money (`currencybar`,
  with its currency bag: see below), the menu buttons
  (`gamebar`), and the central navigation (`ingamemenu`);
- the status effects on the player, above the action bar, and the hovered
  monster's negative ones under its panel (see below);
- menus: inventory (I: equipment slots, eight 7 x 4 bags, premium and loose-item bags),
  character sheet (C), current skill/talent window (S: `skilltalentswindow`),
  stickerbook (W: `collectorsalbum`, titled "Collector's Bag"),
  quest log, travel map, local map, shop, social window, PvP matchmaking,
  leaderboards, events and game menu (Escape). Natively `DSOR_UI_OPEN=inventory,...` opens
  them from the start (screenshots).

Live values: health and resource (`Net::health` / `resource`), level, XP, wallet,
the character sheet's available figures, the quick slot bar (`Net::bar`, wire slots: 0 left
button, 1 right button, 2-6 keys 1-5), the character name. Offline (map viewer)
the map viewer shows resting values (80 % health, 60 % resource, 35 % XP, the mage's
skills). Wisdom starts at level 0, 0% progress and 0 available points, matching
the server default. Both the HUD and wisdom tab use these defaults; server
wisdom progression is not decoded yet. Late-loading layouts receive the current
values without waiting for another stat update.

## Status effects above the action bar

`bottombar`'s `BuffBar` holds two rows of six slots, each with a time label
(`DurN`): `Buffs` for the positive effects, filled from `Slot1` at the left, and
`Debuffs` for the negative ones, `Slot1` at the right edge, filled towards the
middle. `client/src/ui/controls/effects.rs` fills them from the effects the
server runs on the player (`Net::stat_effects`, StatusEffectCommand on our
actor).

The client's database decides what shows and how
(`tools/status_catalog.py` → `interface/status.catalog.json`, written whenever
`export_ui.py` exports `bottombar`, or alone:
`status_catalog.py <export_win32> <assets> <DSO_Godot>`):

- `_Template_StatusEffect.Groups`: `BuffUI` goes on the positive half,
  `DebuffUI` on the negative half; every other effect (boss idles, triggers,
  aura carriers) is never drawn. The 79 marked effects without an icon in the
  client are left out.
- `IconBrush` is the image; `IconConfiguration` names its background and frame
  (`_Template_IconConfig`: `statuseffect_buff` / `_debuff` on `frame_status`,
  costumes on `bg_costume`...), drawn in the inventory's order.
- The name is `db.statuseffect.table` `<LocaleId, else Id>Title`.

One icon an effect. Its stack count (the slot's Icon text, outlined) is the
element's occurrence + 1 (`spare`, the server's `field_18`), summed over the
effect's instances and capped at `MaxStackSize`, as the character statistics
count it. An effect keeps its slot while it runs: the server sends every
refresh as a new instance, so the slots follow arrival order, not instance ids.
Past six on a half, the permanent effects (costumes, pets, mounts) give way
first, then the oldest. The time left is `end` minus `Net::server_tick` (both
the server's milliseconds / 40), in the game's short units
(`gui.window` `timeSecondsShort` "{0:s} s", then minutes, `hr`, `d`); an effect
whose end tick is 0 is permanent and shows none.

The bar stays click-through, as authored (no hit regions, not even for its
labels). Hovering an occupied slot opens a card above it: the name, then one
line a modifier, from the client's own texts (`gui.statuseffects.modifier`): a
modifier's `#key` override, else the generic text of its kind and sign
(`damageRelativePlus` "+ {0:s}% damage"). Values come from the element's wire
parameters (`$0`), immutable actor variables or literals, times the stacks;
progression-dependent ones (pet collection bonuses) read as an em dash. The
card adds the stacks and the time left.

### On the hovered monster

The hovered monster's panel (`interactinfo`: name, health bar, level ring) gets
a row of its negative (`DebuffUI`) effects under its bottom plate
(`gothic_panel/LabelBottom`), centred on the health bar: six 22-pixel slots and
their time labels, which `effects::prepare_target` adds to the layout
(`Canvas/Label_bg/TargetEffects`; no original window has such a row). The icons
draw, count and time out as on the bottom bar; the row centres what it shows.
Its positive effects (enrages, auras) are not shown. The running effects of
every other actor in view are kept by `Net::actor_effects` (StatusEffectCommand
on that actor, stamped on each change; dropped when the actor is discarded or
the map changes). The row only shows while the panel does, so its icons cannot
be hovered: moving the pointer onto them leaves the monster.

**Settings > Combat > Status effects on enemies** (on by default, saved as
`monster_effects`; older settings files keep it on) turns the row off. As every
preference, it waits for Apply.

`DSOR_TEST_EFFECTS=<id>[:seconds[:stacks[:p0/p1...]]],...` gives the offline
player these effects (seconds 0: permanent; `#<wire index>` in place of the id
for an effect the bar does not show, such as a Shadow Soul's
`minion_mercenary_*` for the inventory's preview), for captures with
`DSOR_UI_REPLAY`; they also count in the offline character statistics.
`DSOR_TEST_TARGET_EFFECTS` (same format) gives them to each monster when it is
first hovered; with `DSOR_TEST_MONSTER` and `DSOR_TEST_TARGET=1` (the nearest
live monster counts as hovered) no pointer is needed.

## The currency bar and its bag

`currencybar` holds four money displays; `client/src/ui/controls/currency.rs`
draws the one the 2026 client shows in game, `LabelIngamePayment` (the
character screen's is `LabelIngamePaymentRCOnly`, `ui/selection.rs`), whatever
the export marks hidden. It shows the Andermant, the gold, silver and copper,
and one more currency: the one picked in the bag, the Badges of Honor until
then. Its dropdown (that currency) and its bag button open `currencybag` under
the bar: the Glyphs of Power, then every currency of the shops
(`_Template_ItemCurrency`'s items, highest `Priority` first, ties in table
order), each with its icon and whole balance. A click on a row shows that
currency on the bar and saves it at once (`currency` in `game-settings.json`,
as the game keeps `/Settings/CurrencyDisplay`); a click elsewhere, Escape or
the button again close the bag.

Balances: the wallet (NewPlayer +356, CurrencyChanged), the glyphs (NewPlayer
+376, CurrencyChanged's u64) and the keyed currencies (NewPlayer +384,
GameCurrencyChanged); one the server never sent reads 0.

Amounts keep within their places as the game's `GetClampedIntString` does:
the Andermant to 9 digits, the gold to 5, the picked currency to 10; past
them, by thousands with `k`, `M`, `G` (100,000,000 gold reads `100M`). Then the
font of a money label shrinks until its text fits its rect (`ui::Fit`, only
when the text changes). The bag shows whole amounts, in the row's width.
Hovering the Andermant, the coins, the dropdown or a row names the whole
amount ("8883102 Badge of Honor").

The bag's list is `currencies` in `interface/systems.catalog.json`: re-export
the systems catalog (`tools/systems_catalog.py`) for an older conversion.

DEVIATION: the original's fonts shrink no further than the layouts'
FontScalingFactor (0.6); here as far as the place needs. The bag button's glow
when a currency grows and the payment button (the Andermant shop) are not
wired.

## Items received

When the server gives an item (a quest's reward, the daily login chest, a chest
or package opened, event and spender rewards), the item's icon and "Daily Login
Chest received" show under the top navigation: `messagewindow`'s
`ItemMessageLine`, with the icon in its `ItemSlot`.
`client/src/ui/controls/message.rs` draws it as release261's
`UI::MessageWidget` does (addresses in the module):

- The server sends MessageCommand type 2 between the reward's two
  InventoryInfo packets: source 7, the amount, the item's id, and the template
  for a currency that made no item. `gameplay::inventory::Receipt` resolves the
  id on arrival against the carried items, and falls back to the template.
  Sources 0, 1, 3, 4 and 5 name a currency by a fixed template.
- The text is `gui.window` `lootReceived` "{0:s} received", or
  `lootReceivedAmount` "{0:i} {0:s} received" when more than one. The item's
  name and icon layers (background, image, frame) come from the item catalog,
  with a carried item's own look and rarity. A template the catalog does not
  know shows nothing, as in the original.
- Notices queue without limit and show one at a time, each for `_Globals`
  `InfoTextDuration` (4 s). The window is rebuilt only when the shown notice
  changes, and an empty window builds nothing.
- The notice takes no clicks; the world under it stays clickable.

The window's other lines (info, status, system messages) are not wired. Neither
is the original's pause, which puts the shown notice back in the queue while a
UI manager flag (+0x329, not identified) is set. Offline,
`DSOR_PACKET_REPLAY` with the server's receipt packets shows the notices
(docs/client-progress.md, "Items received").

## 3D previews: the inventory's figure and the album's sticker

`client/src/ui/preview.rs` draws the turning 3D views the layouts leave room
for:

- the inventory's `figur` (exported hidden: a runtime render target): the
  player's character, dressed as in the world, playing its `Inventory`
  animation state, on the equipment and trinket pages; on the mercenary page,
  the player's Shadow Soul at rest (`Idle_Hub`) when one runs (its
  `minion_mercenary_*` effect is among the player's status effects), else the
  page's empty placeholder;
- the album's preview box (Categorypage `background_image`, under its
  headline): the sticker picked, as the original shows it -- a costume on the
  player, an emote the player plays again and again, the player riding a
  mount, a pet, a Shadow Soul. A decoration (trail, ground marker, aura,
  wings) only starts effects: the box shows its icon, as do stickers in a
  catalog exported before the looks below.

Hold the left button over the area (its DragRotate buttons) and move
sideways to turn the subject.

Each view is a stage: an image its own camera draws (before the interface,
which samples it), of a subject standing far under the maps on a render layer
only that camera sees (26 to 28), with a studio light of its own. The
inventory's character and its Shadow Soul are separate stages: turning the
pages shows each at once, as it was left. The map's sun,
fog, ambience and post effects never reach it. Its camera carries
`HighlightCamera` so that every scene-camera query (`With<Camera3d>,
Without<HighlightCamera>`) leaves it out. The window draws the image as one of
its widgets, in its drawing order: `prepare_inventory` and the album's prepare
give that widget the stage's texture name (`preview:inventory`,
`preview:mercenary`, `preview:album`), which `spawn_windows` binds to the
image. `ui.wgsl` undoes
the multisampled edges' premultiplied colour, so silhouettes blend cleanly. A
resize remakes the images at the interface's resolution.

A new subject dresses hidden behind the one shown and replaces it once ready,
so picking stickers never flashes an empty box. Once it has posed for a few
frames, the camera frames it from what it draws (its skinned meshes' bounds
through their posed joints, as Bevy culls them; a character's parts carry a
fixed box and a body variation moves its joints off its skin, so neither
serves): its lowest point on the area's floor mark (the inventory's `shadow`
label, the album's payment button; a hovering pet's belly), the whole subject
in view whichever way it turns, with room for an emote's leaps, through a
narrow (10 degree) lens from the front left like the original's
`InventoryCamPosition`.

What a sticker shows comes from `tools/systems_catalog.py`: each sticker's
`looks`, one per item, give the item's class (`CharClass`; the player's
class's item is shown), the status effect it starts (`StatusEffectId`, wire
index `rowid - 1`) and what that effect shows: a skin part (`SkinModifier`,
worn on a slot of its own so its mask culls what it covers), a mount (the cast
effect's ride, `ActorStatusEffect` in its DoneModifiers, then `Mounted:Mount`),
a pet (`Pet:PetMonster`), a Shadow Soul's monster (`ShadowSoul`) or an emote's
animation (`StartAnimation`). Models come from `characters/mounts.json`,
`pets.json` and `monster_templates.json`. Re-export the systems catalog to add
the looks.

DEVIATION: the original frames each sticker with its own camera
(`_Template_StickerbookItem` CameraPosition, CameraCharactorHeight,
CameraAngleOfView) and lights the stages with `_Globals`' InventoryLight
colours; here every subject is framed from its joints and lit alike.
Emote props (the Stormball flags) and decorations' effects are particles,
drawn outside the actors' hierarchy, so the previews leave them out.

A closed window's stage costs nothing: its camera is off and its subject is
parked (its animation stops). `preview::choose` compares a few revisions each
frame (the windows' pages, the player's look, the views, the player's effects)
and builds a subject only when one moved (`UiWork::previews`).

## The character screens: roster and creation

`client/src/ui/selection.rs` draws the roster (`charselectionwindow`, with
`confirmdialog` to delete) and the creation screen (`chargenwindow` beside
`premadepackagewindow`) from their original layouts, with the currency bar's
RC-only display and the game bar's fullscreen and menu buttons. It has its
own small renderer, not the menu windows'. `client/src/screens/selection.rs`
keeps their state, takes the server's replies and stands the 3D figure.
Their behaviour follows the 2026 client's `CharacterSelectionWidget`,
`CharacterGenerationWidget` and their managers, decompiled in the live server
tree's `data/diagnostics/charselection-widget.annotated.txt` and
`chargen-widget.annotated.txt` (the code comments cite their addresses).

Each window draws its still artwork, then its buttons, then its texts, each
in authored order. In authored order the crest of the creation screen's frame
covered its Create button and caption, and the class's tinted background
covered the class name and description. This also explains the exceptions
listed above (captions over relief art, translucent backdrops under texts).
INFERRED: from these layouts; the game's renderer was not traced, and the menu
windows still draw in authored order.

### The roster

- A row shows the name, the class (`gui.window.chargen_charClass_<class>`, a
  woman's with `.f`: "Steam Mechanica"), "Level {0:i}"
  (`charselection_characterLevel`) and the map's caption, plus the mentor bonus
  icons from the character's two flags (honor: flag B, wisdom: flag A, both).
- **Create new character** shows while a slot is free (the roster's slot
  count, CharacterSelection +0x68). With every slot used, **Buy** takes its
  place until `_Globals.MaxCharactersPerAccount` (5). From then on the add
  button shows again, disabled. DEVIATION: buying a slot is not offered, so
  Buy stays disabled, as when the account is short of Andermant.
- **Delete** opens the confirmation: `confirmdialog`'s centred dialog with
  "You really want to delete your character, "{name}"?" and its tick and cross.
  The tick deletes the character (op 6). It leaves the list at once and the
  first character is chosen. The cross or Escape cancel.
- The DenyReason line shows only the server's refusal to enter the world
  (CharacterSelection op 4): 1 `badCharacter`, 2 `badAccountData`,
  3 `badCharacterData`, 4 `noCharacterData`, 5 `internalError`, otherwise
  `unknownError`. Nothing else is written there.
- The currency bar shows the last character's Andermant.
- Sounds: choosing a character plays `ui_char_gen_choose_<class>` and
  `ui_char_gen_vo_<class>_<gender>`; Play Now plays
  `ui_char_gen_generate_<class>_<gender>`; Add plays `ui_switch_screen`;
  Delete plays `ui_cancel` and `ui_open_screen`; the dialog plays
  `ui_confirm` or `ui_cancel`.
- ADDITION: the original's roster has no keys. Here the arrows choose a
  character and Enter plays.

### The creation screen

- **Opening.** The roster's add button asks the server for its editor
  (CharacterGeneration op 1). The roster goes at once; the creation screen
  shows on the server's answer (op 2), which carries the first class, gender,
  hair and beard. The server also opens it by itself for an account without
  characters.
- **Classes.** The four class buttons are a check group, captioned with the
  class name for the gender shown. Clicking one, even the class already shown,
  or switching the gender does four things:
  - sets that class and gender's default hair and beard (warrior 18/8,
    mage 2/1, ranger 7/10, dwarf 35/35; women 45, 3, 16 and 33, with no beard);
  - keeps the ethnicity;
  - turns the figure to the front;
  - plays the showcase from its start.
- **Rows.** Gender (both arrows switch it), Ethnicity (body style 0 to 2), Hair
  and Beard. The Beard row shows for men only; a woman's beard is 16.
  - The choices are the `_Template_CharacterHair` and `_Template_CharacterBeard`
    rows flagged for that class and gender, in `Index` order.
  - The arrows wrap round, and are never disabled.
  - A value that is none of the choices stays.
- **The name.**
  - The field waits for a click. It then takes the keyboard, with its caret;
    Enter, Escape or a click elsewhere let go. "Enter name" shows while it is
    empty and not being typed into.
  - It takes up to `CharacterNameMaxLength` (16) letters of the language: A-Z,
    a-z, 0-9, accented Latin, Greek and Cyrillic. It never takes spaces,
    punctuation, `<` or `>`.
  - Taking the keyboard hides the last refusal, unless it was a figure's
    (deny reason 8).
- **Create** sends op 3 at once, with no other check: class, gender, hair,
  beard, body style, stature 0, the name, an identity matrix. It sends one
  request at a time.
  - Op 4 refuses the character. The deny reason's text shows in red:
    1 `existingName`, 2 `emptyName` (with the minimum, 3), 3 `notAlphaNumeric`,
    4 `badName`, 5 `badHairStyle`, 6 `badBeardStyle`, 7 `badBodyStyle`,
    8 `badBodyStature`, 9 `alreadyGenerated`, 10 `insufficientSlots`,
    11 `internalError`, 12 `classLocked`, otherwise `unknownError`. Reasons 10
    to 12 have no English text; the key shows.
  - Op 5 makes the character: the screen goes, `ui_char_gen_generate_*`
    plays and the hand-off to the world follows.
- **Back** asks for the roster (CharacterSelection op 2). The roster shows on
  the server's answer, and an account without characters gets the editor
  back. The button is off when the editor's first bool says so.
- **The class description** is `chardesc_<class>` then `charslogan_<class>`,
  wrapped into the list's nine lines. The wheel scrolls it, and the handle
  follows. The class's emblem and tinted background show above it.
- **The premium start panel** is shown as the original shows it without
  offers: its "Premium Packages" headline, an empty list and its clear button,
  which does nothing. Its own Create button stays hidden, and so does the
  editor's price tag.
- **The currency bar** shows the editor's value (op 2, +0x90).
- **The figure** wears the class's `CharGenItems` with its `ArmamentState`. It
  plays `CharSelectStart`, then `CharSelectIdle`, with the class and gender's
  `CharGenStartSequence` (again and again if it repeats: the mage's ball and
  light) and `CharGenIdleSequence` looping. Another hair, beard or ethnicity
  keeps the idle.
- **Sounds:** a class plays `ui_char_gen_choose_<class>` and
  `ui_char_gen_vo_<class>_<gender>`; the gender plays `ui_char_gen_switch_gender`
  and the voice; the other arrows play `ui_char_gen_switch`; the name field
  plays `ui_click`; Back plays `ui_switch_screen`.

### Looks in the world and in the previews

A player's body style picks its body set: `<class>_<gender>_body_00`, `_asian_`
or `_black_` (1, 2). Its proportions stay `var_<class>_<gender>_<stature>`
whatever the ethnicity. NewPlayer and NewRemotePlayer carry class, gender,
hair, beard, body style and stature (`parts`). The world draws the hair and
beard of those styles, with no gender check, and the selection screens give
women no beard. The inventory's figure keeps all of them.

### Data and checks

`tools/export_character_selection.py <static.db4> <assets> [--language
<export_win32>]` (also run by `character_tables.py`) adds `creation` to
`characters/selection.json`:

- the name bounds and the most characters an account has;
- each class's `CharGenItems` and armament;
- each class and gender's hair and beard Index lists and showcase sequences;
- the English `gui.window.chargen_*` and `charselection_*` texts and the
  package panel's headline.

Without it (an older export), the creation screen shows keys.

Offline checks:

- `python tools/character_selection_preview.py <server> <out.json> [--editor]
  [--empty] [--refuse N]` writes server packets that open the editor (and
  refuse a name).
- `DSOR_SELECTION_PREVIEW=<out.json>` and `DSOR_SELECTION_REPLAY=<steps.json>`
  replay them. Steps: `class`, `step: ["hair", 1]`, `name`, `create`,
  `refuse`, `back`, `delete`, `confirm`, and the roster's `select`, `rotate`,
  `play`, `capture`.
- `tools/chargen_vectors.py` checks CharacterGeneration both ways against the
  server's codec (`crates/proto/tests/chargen_release261.rs`).

DEVIATION, not done:

- Changing an existing character: the roster's EditButton, CharacterGeneration
  ops 8, 9 and 10, the SaveButton. The server offers no edits.
- Buying a slot and premium start packages.
- Instance selection.
- These screens' tooltips.
- The preview items' own weapon effects.
- A hair, beard or ethnicity click during `CharSelectStart` cuts it short. The
  figure is remade; the original updates it in place.

## Per-frame cost

**Rule: a frame in which nothing the interface shows has changed does no
interface work beyond a few cheap checks.** The whole interface (every
`dsor_client::ui` system) costs about 0.2 ms a frame at the Kingscity
benchmark view, and must stay well under 0.5 ms. No single UI system should
take more than 0.05 ms in a still scene. A closed window costs nothing.

What this means for new interface code:

- **Never rebuild a view, a layout or a list every frame to compare it with the
  last one.** Build it when an input changes. Server state lives in one big
  `Net`, so Bevy cannot see what changed in it. Key the rebuild on
  `Net::ui_revision` instead: `on_command` bumps it for every server command
  except the actor stream (`actor_stream`: movement, combat, effects, vicinity).
  Also key it on `inventory.revision`, on the change ticks of the UI resources
  it reads (`State`, `QuestUi`) and on which catalogs have loaded.
  `controls::systems::sync` and `controls::quests::sync` are the patterns.
- **Code that edits a field a window shows outside `on_command`** (a click, a
  fixture, a map change) calls `Net::touch_ui()`.
- **Write a resource only when the value changes** (`if x != new { x = new }`).
  Bevy marks a resource changed on every mutable access, so an unconditional
  `state.opened = open` each frame makes every reader of its change ticks
  rebuild every frame.
- **Do not prepare, build or fill a closed window.** `spawn_windows` checks
  visibility before preparing a layout. A view needed only by its window is
  built when the window opens (`systems::sync` only builds the open windows'
  views).
- **Do not copy shared state into every window every frame.** Hand it over when
  it changes, and to windows rebuilt since (`spells::sync`).
- **Count rebuild work in `UiWork`**, and add a test that idle frames leave the
  counters alone (see `closed_windows_are_not_prepared_every_frame`,
  `views_are_built_only_for_open_windows_and_only_when_something_changed`,
  `the_quest_list_is_rebuilt_only_when_something_changed`,
  `the_status_rows_are_reassigned_only_when_what_they_show_changes`,
  `subjects_are_built_when_what_they_show_changes_and_never_on_idle_frames`,
  `the_strip_is_repainted_only_when_the_download_moves`).
- **Progress from another thread** (the account screen's game data strip)
  compares the run's revision counter with the one painted, and repaints at
  most four times a second while it moves (`screens::login::game_data::paint`).
- **Something that counts down** (the status effects' time labels) computes
  when it next reads differently and does nothing until then
  (`controls::effects::sync`).

These checks catch a regression:

- the `UiWork` unit tests above and `only_commands_the_windows_show_move_the_ui_revision`;
- `DSOR_FLAGS=frameprofile` logs `ui work: …` every five seconds (0 in a still
  scene), and `tools/profile_native_maps.py` warns when an idle run rebuilds;
- debug builds (or `DSOR_FLAGS=uiaudit`) rebuild the open views once a second and
  log an error if one changed without an input change (a missed `touch_ui`);
- the `system-profile` build lists every UI system's cost
  ([Testing and diagnostics](testing-and-diagnostics.md#performance-diagnostics)).

FAILURE (2026-10-21): the systems' windows commit (`705409a`) dropped Kingscity
from about 180 to 95 FPS offline, and to 50–60 online. `spawn_windows` prepared
the layouts of every closed, never-opened menu each frame and then dropped them
(5.95 ms a frame). `systems::sync` rebuilt and compared all ten system views
each frame, open or not (1.0 ms a frame offline, more with a real account's
achievements). `quests::sync` rebuilt the quest list each frame, and
`spells::sync` cloned the spell model into all windows each frame. Details:
[native map performance](native-map-performance.md#interface-regression-after-the-systems-windows-2026-10-21).

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
priority over dragging. Opening or clicking a window brings it forward below
the modal game menu. Every menu occupies a separate 20-unit depth band, with
authored artwork/text bounded to its first 8 units and runtime inventory items
at 12 units. Whole windows stay together when overlapped; repeated focus changes
do not drift into modal or tooltip layers. The local map stays below menus.
Bevy 0.19 caches transparent mesh sort keys independently of their transforms;
after hierarchy propagation, moved UI meshes are marked dirty so their retained
order follows the same depth as text and sprites. Stationary meshes stay cached.
Artwork, text, gauge clipping and hit regions share the root's
offset. Positions survive closing/reopening during the session. Dragging and
resizing keep at least 64 reference pixels of the title reachable; losing focus
or hiding a window cancels its drag. The HUD bars stay fixed.

Native focus/overlap regression: `DSOR_UI_REPLAY=<steps.json>` accepts an array
of steps with `frame`, optional `point: [x, y]`, `left: true/false`, `capture`
(PNG path) and `quit: true`. Points use the centered 1024x768 reference layout.
Frames advance during gameplay; pointer steps use the normal click/drag handler.
Each capture also writes a `.ui.json` snapshot of window contents, transforms
and visibility for rendering diagnostics. Input stays inside the replay and
does not warp the desktop cursor.
Combine with `DSOR_UI_OPEN` and `DSOR_INVENTORY_PREVIEW` for populated panels.

Inventory equipment, ingredients, trinkets and mercenary pages are mutually
exclusive. The character sheet likewise selects one of its five tabs. Switching
between the character sheet and skills/talents closes the previous panel.
The character sheet preserves the authored opacity of the shades over its
textured backing. Its scrolling statistic rows and subsection headings use
visible copies of the atlas backgrounds; the source templates remain hidden.
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

The navigation button with the legacy `travelmap` widget ID opens the stickerbook.
Its favorites page is shown initially; data-dependent categories, offers and item
effects remain hidden. In free flight (the map viewer or F3's Free look mode),
WASD and Q/E move the camera without toggling UI windows. Pointer navigation and
other window shortcuts remain available. Free look also blocks character clicks
and skill casts; disabling it restores character follow and the previous zoom.
Close F3 to fly; hold right mouse to look and Shift to move faster. Scene streaming,
actor visibility and fog follow the detached view without relocating the character.

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
a transparent overlay, seen from straight above and turned with the camera's
heading (never its pitch): white boundary lines and captions, a faint neutral
walkable-area tint and the original green player dot. Friendly players use blue
dots; enemies use red dots, including hostile players after faction updates.
NPCs and props use their authored `LocationMapIcon`, including the distinct
normal, golden and cursed amphoras. Quest offers, active tasks and turn-ins
override NPC icons, with turn-ins taking priority; clearing the quest restores
the NPC's own icon. The player indicator is 20 reference pixels and actor dots
are 18; service and quest symbols retain their authored size. Marker positions
follow their world entities. Dead monsters lose map indicators and hover info
as soon as their death is received, including deaths received while loading.
NPCs also carry the original additive, camera-facing `t000_general/status_*`
models above them: profession symbols (including the glowing merchant bag),
`!` for available quests and `?` for turn-ins or active interaction targets.
An unfinished quest does not show a turn-in at its giver. World and map markers
share quest priority and colors; completing a quest restores the service symbol.
The models supply their authored height, with DB4 `OverHeadIconOffset` applied
from the NPC sidecars. World markers follow NPC visibility and visual streaming,
are click-through, and disappear with their actor. Re-run `export_npcs.py` to
include the offsets in older asset packs (missing offsets default to zero).
Hovering an NPC also shows a compact top-center name/profession panel below the
navigation, with the original `interactinfo/LabelGradient` fading side edges,
using the same picked actor as its outline. `ProfessionLocaId` is exported as
the localized `profession_title` (for example, Garnet / Jeweler). The panel is
click-through, scales with the HUD, and clears over UI, during loading, or when
the NPC leaves the world. Re-export NPC sidecars to add these localized labels.
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
The Settings window and F3 debug menu expose a draggable "Map transparency"
slider from 0-100% (default 45%), while
all markers and the map name stay visible.
The setting survives restarts in `map-settings.json` natively or `dsor.map` in
browser localStorage. Re-export the UI, NPCs and props to generate the localized
name table, `icon_map_*` textures and authored marker assignments.
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
Equipment and bag cells show icons and stack counts. Worn pieces also show their
item level in the original's small dark box (`UpgradeButtonTemplate_Green`),
centred 6.4 x 4.8 reference pixels in from the slot's top corner on its column's
outer side: top right for helmet to boots and the off hand, top left for the
weapon, weapon mod, cloak, rings and amulet. The belt, trinkets, mercenary
pieces and essences show none, as in the original
(`UI::InventoryWidget::UpdateItemUpgradeButton`). Its tooltip and click (the
item upgrade window) are not wired. Ingredient category and
rarity tabs populate the collection grid from `_Template_IngredientBag`, including
dimmed uncollected items and owned quantities from the ingredient allocation.
The five tier selectors retain their original normal/hover/selected artwork;
the ingredient background is painted first and unused recipe/core overlays stay
hidden, so these selectors remain visible.

`tools/inventory_catalog.py`, called when exporting `inventory`, writes
`inventory.items.json` from the server database and original localizations/icons.
Export `inventory tooltip unstackwindow` together. Hovering an item shows its
rarity-colored name, category, level, base values, gem icons, unique effects,
customization, melting value and selling coins using the original XML templates.
Hold **Y** for the wider advanced card, or enable **Always show advanced tooltips**
under Settings > General and choose Apply. The saved preference covers inventory
and action-bar item cards. Advanced cards show base rolls, gem values, enchantment
percentages and their contributions. `item_tooltip_values.py` exports numeric
modifier ranges, progressions and rarity/tier limits. The tooltip materializes
base rolls using the native dSFMT stream; wire enchantment fractions are quality
rolls, not absolute stats. Character bonuses (including attack/movement speed,
regeneration and PvP bonuses) remain separate from item-local totals and appear
in both views. Scoped resistance bonuses retain their element. Scripted unique
effects use their authored modifier descriptions alongside the numeric values,
and unrecognized rolls are labeled in advanced
view. Ctrl+left-click opens the original split dialog: choose the
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
and `questlog`; the older `pvpleaderboards`, `skillwindow2`, `talentwindow` and
`questscreen` are not loaded by runtime navigation. `questscreen2` is the current
NPC conversation view, opened by the dialogue controller rather than navigation.
Inventory, character sheet, social, shop, matchmaking and events each use their named
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

## Chat

The chat panel and channel dropdown stay anchored to the screen's left edge at
every aspect ratio and starts compact on login. V and the mode button switch between expanded controls and
a compact preview. Compact mode shows only the expand button and the four newest
message rows; the text lets clicks pass through to the world. Enter expands the
chat and focuses its entry. Expanded scrollback is preserved while the compact
preview continues to show recent messages.

The entry's channel button shows only the current channel's icon, including its
hover and pressed states. Tabs and dropdown choices use their disabled artwork
until the chat server confirms membership (or login, for whispers). Leaving a
group or guild disables that channel and selects an available channel instead;
disconnecting disables all channel choices.

## Social window

`socialscreen` (F) is a systems window (`controls/systems/rosters.rs`): its
lists are the authored row templates cloned per player, rebuilt when the server
says something (`Net::ui_revision`) or a sort or scroll changes.

- Opening it sends GroupList (type 0), GuildList and a PlayerQuery of kind 1:
  the players on our map instance (the server leaves us out, at most 50). Enter,
  in the search field or on its button, searches the name typed among every
  character the server knows (kind 0; offline ones greyed); with an empty field
  it lists the map again. Update repeats the last search and asks for the group
  and the guild.
- Search rows follow the game's `CreatePlayerListElement`: online or offline
  icon (`icon_status_*`), name, class icon (`icon_member_*`), level and the
  group status icon. The server sends no group membership for other players
  (role 0xFF, its third flag set to "online"), so the icon marks our own group's
  members and rows with a group role.
- Group rows (GroupStatus): the leader on its template (star, blue name), the
  member's map name, offline members greyed. Leave group needs a group, Found
  group (group finder page) none.
- Guild rows (GuildStatus): rank (Member, Officer, Guild master). The
  foundation panel shows only outside a guild, Leave guild only inside one.
- Ignore lists IgnoredStatus, which this server always sends empty. The Invite
  tab (the friend-invite campaign) is disabled; Friends and the group finder
  stay empty.
- Each list shows 19 rows and scrolls with the wheel and its own arrows; the
  column heads sort every list (up, down, off; the head shows the arrow). A row
  opens the player menu (whisper, add friend, invite to group, ignore, trade).
- `MenuBackground`, the brown panels behind the column heads and the bottom
  controls, follows the window's contents in the layout; it is drawn before
  them, its border line after. `struktur_hell.png`, the rows' tiled texture
  (u from -1 to 3.5), is sampled repeating.

`tools/ui_fixture_packets.py` adds a group, a guild and a 22-player search for
offline captures (`DSOR_PACKET_REPLAY`, `DSOR_UI_OPEN=socialscreen`).

## NPC dialogue

Left-clicking a talkable NPC follows a route on the navigation mesh and stops about
1.8 world units from its feet. The character faces the NPC, idles, and sends
`Encounter` and `NpcRequest` once upon arrival. Routes go around stalls and walls,
and choose another stopping point if the NPC's near side is blocked. The original `npcscreen` layout
provides the portrait, expandable nameplate and curved buttons with their hover
and pressed states. It sits where the layout puts it, against the screen's
left edge, whatever the screen's width and the NPC's position. Its atlas repeats: the later buttons' left-side tails
use UV coordinates above 1. `NPCInfo` supplies the current quest and talk options;
task-target map markers do not produce duplicate conversation buttons. Additional
options are paged.

### The quest screen (`questscreen2`)

A quest or a conversation opens `questscreen2` beside the portrait, as
release261's `UI::QuestWidget2` fills it (`client/src/ui/dialogue/quest.rs`,
addresses there). Every part stays where the layout puts it; a 2026 capture
of Thabo's "Full Moon Rising (1/5)" offer matches. The 2018 `questscreen`
layout is never loaded by the 2026 client.

- **Bubble.** The headline is the quest's title, with "[INTERNAL] " for a quest
  that isn't live and " (n/m)" for its place in its series. A conversation has
  no headline. The text (`quest.<id>` offer, progress or complete, or
  `quest.<dialog>`) comes in the player's gender (`.f` keys), with the
  player's name for `{0:s}`. It is wrapped to the label's width, as Tahoma
  measures it, into at least three lines. The frame grows round them. Words
  tagged `<QuestHighlightStart>` or `<cAARRGGBB>` are white (the dialogue
  catalog's `marked` texts).
- **Buttons.** They show icons, no captions, under the last line. Back is alone
  while a quest is in progress. An offer, a completion and a conversation have
  Back and Accept. Their tooltips name them: "Accept quest", "Complete quest",
  "End conversation", "Back".
- **Tasks.** Quests only: "Quests", then each task's text (its `{0:i}` the
  task's amount), a row a line from the List's top. The check mark is on each
  task's first line: empty in an offer, ticked from the quest log's task bits
  in progress and completion. With `stagedTasks` a task shows once its
  `preTasks` are done.
- **Rewards.** Quests only, stacked from the panel's top:
  1. "Reward";
  2. the Andermant and coins on the template for the largest coin;
  3. the experience ("XP: n", or "XP (level dependent): n", the dynamicXP
     percent of the level's span);
  4. the items four to a row, unused slots hidden, with the reward's rarity
     background. Items for another class are left out.
- **Back** ends the conversation, as the original's does. **Accept** sends
  `Quest` (offer: also `ui/quests/ui_quest_accept`), **Continue** sends `Talk`.
  Each also sends `NpcRequest`, then returns to the NPC's menu while it offers
  something else, else closes.
- The quest screen is drawn under the NPC screen, so its backdrop shades the
  world, not the portrait. The backdrop is click-through, and so are the
  parts no one sees (the reward frame, the task separators).
- The view is rebuilt only when its inputs change: the conversation,
  `Net::ui_revision`, the catalogs, the player's class or gender.

Data: `tools/quest_catalog.py` exports `staged`, each task's `pre`, `part`,
`internal`, `dynamic_xp` and the items' `rarity`. `tools/dialogue_catalog.py`
adds `marked` and the `gui.window.questscreen` texts. Re-export both for an
older conversion.

Offline captures: `--npcs` places the map's NPCs; `DSOR_FLAGS=allnpcplacements`
adds event NPCs (Thabo). The `DSOR_UI_REPLAY` step `npc` with `npc_quest`,
`npc_quest_state` and `npc_quest_done` (the finished tasks' bits) offers a
quest in that state.

DEVIATION: no fades (the original fades in the bubble over 0.2 s, the buttons
0.1 s later, the tasks and rewards 0.3 s later). A task list longer than its
List is cut, not scrolled. The experience is not multiplied by the player's
quest bonus. A full quest log's "maxQuestCountReached" message is not shown;
the server decides. A delivery still runs straight from the menu (the
original shows it on this screen, "Complete transaction"). INFERRED: the
series total, which the client computes after loading (not traced).

The merchant's **View selection** requests its current offers and opens
`tradingscreen` with the original Trader header, buy/buyback controls, category
icon tabs, two-column item cards, currency icons and usable-items checkbox.
The release 261 Offer decoder retains all eight flags, icon configuration,
48 category codes, discount metadata and payment items. Tabs follow the server's
category order, with a fallback to the returned offers when the list is absent.
Filtering by category/class/level happens before pagination; changing the tab or
filter resets the page. Twelve ordinary cards fit per page; mixed-currency prices
can use taller cards and fewer rows. Prices use the received values and the local
server's veteran-discount rounding. Item-currency icons come from the shop's
`_Template_ItemCurrency`, and offer icon/configuration overrides take precedence
over the item template. Buyback-only updates preserve the shop selection and
refresh its returned item list. Buying, selling and restoring items still need
the transaction controller; buyback prices are not supplied by these snapshots.
The top-navigation `shopwindow` remains the separate global shop. Other NPC
services and delivery/payment choices are not implemented by this controller.

Close or Escape dismisses the interaction. A new ground click, attack, loss of
focus, death, map change or removed NPC cancels it. A blocked approach does not
open a remote conversation; holding the original NPC click cannot change into
ground walking after arrival. Decorative `NoTalk` actors remain noninteractive.

Re-export `export_npcs.py`, then `export_ui.py ... npcscreen questscreen2 tradingscreen` to
include portrait/interaction metadata, NPC portrait textures, option icons and
`interface/dialogue.catalog.json`. Exporting `tradingscreen` also rebuilds the
item catalogue and this catalogue, including category icons and shop currency/offer metadata from
`tools/trading_catalog.py`. The catalogue uses the original English
localizations and strips Nebula's inline colour tags. Offline `--play --npcs`
also supports approaching NPCs. The native UI replay accepts an `npc` template
ID to point at its actual torso; offline-only `player: [x,y,z]`, `npc_quest`,
`npc_quest_state` (0 active, 1 ready, 2 closed, 3 canceled), and
`npc_offers: [item-template, ...]` provide repeatable placement and menu fixtures
without changing an account. `left_point` addresses left-anchored UI controls.
`npc_offer_packet` replays a binary server Offer message through the real decoder
for the selected NPC in offline mode, including categories, prices and buyback.

## Missing

- Text uses `assets/fonts/tahoma.ttf`; the original bitmap font is not
  reproduced (the outline effect is).
- Inventory sorting, bank access, slot purchases and additional item actions
  (use, sell, repair, identify, sockets and customization) need their controllers.
  Equipment comparison tooltips remain.
- List scrolling is not implemented. The transmog desk still shows the skin's
  icon where the game turns the character wearing it; the album and the
  inventory turn theirs (see "3D previews" above).
- The quest log's pages, the album, the events, the leaderboards, the PvP
  queue, the premium shop and the NPC desks (salvage, the `gemdeskwb` gem desk,
  workbench, transmog) are bound on their 2026 layouts
  (`client/src/ui/controls/systems.rs`, see docs/client-progress.md "The
  systems' windows" and "The systems' windows on their 2026 layouts"), and so
  is the social window; talent allocation is not drawn yet. Lists scroll with the
  wheel; their scroll handles follow but cannot be dragged. The transmog desk's
  eye toggles have no server command and are hidden.


## Settings, shortcuts and hover descriptions

Escape > Settings opens the original settings frame with General, Graphics,
Audio, Hotkeys, Mouse and Combat tabs. Preferences are edited as a draft:
Apply saves and activates them, Cancel/Escape discards them, and Reset restores
defaults in the draft. Native settings live in `game-settings.json`; browser
settings use local storage (`dsor.settings`). Failed saves keep the draft open.
Graphics spans two pages (Previous/Next): fullscreen, vertical sync, anti-aliasing
(Default, Off, FXAA, SMAA Low/High), sun shadow quality (Off/Low/Medium/High),
shadow filtering (Default/Fast/Soft), desktop SSAO (Off/Low/Medium/High), a CAS
sharpening slider, bloom, vignette, and map transparency. Apply updates rendering
without restarting. Default AA retains desktop FXAA and browser MSAA (disabled
when WebGL2's optional depth prepass is enabled). Shadow maps use 512/1024/2048
texels; WebGL2 keeps fast hardware shadow filtering. Browser SSAO is unavailable.
Bloom and vignette switches gate the authored map effects without replacing
their colors or strengths. Diagnostic `noao`, `noshadows`, `lowshadows` and
`nobloom` flags still take precedence. Native builds with the optional `dlss`
feature offer Off, DLAA (native resolution), and DLSS Quality/Balanced/Performance.
The control requires an NVIDIA RTX GPU, Vulkan, and a working DLSS runtime;
unsupported devices and builds show a read-only availability explanation.
DLSS/DLAA replaces spatial AA while enabled. The saved spatial AA choice remains
the fallback when DLSS is disabled or the same preferences are loaded on an
unsupported device. CAS remains available. FSR/XeSS is not integrated.
SMAA/FXAA smooth edges at the current resolution, and CAS sharpens; these options
do not upscale. See [native DLSS setup](getting-started.md#optional-native-dlss--dlaa).
Audio provides mute and draggable master, music, ambience and effects sliders.
All percentage settings use sliders with exact percentage labels. Click the
track to jump to a value or drag the thumb; dragging stays captured outside the
track and clamps at its endpoints. Settings sliders edit the draft until Apply.
The debug map-transparency and lighting-panel opacity controls use the same
drag behavior and apply immediately. UI click and close sounds always play once,
even when their source event metadata describes an infinite loop.

The Hotkeys tab supports modifier combinations, Delete to clear a binding and
Escape to cancel capture. Assigning an occupied key clears its old action and
identifies that action. Bindings drive gameplay, HUD captions and tooltips from
one source. The modal blocks movement/casting and other window shortcuts while
editing. Existing mouse attacks remain available alongside keyboard alternatives.
Window keys: I inventory, C character, S skills, W collector's album, Q quests,
M world map, Tab map overlay, F social, E events, L rankings, J battlegrounds.
The bottom-left consumable is on G. A settings file saved before these defaults
(`keymap` below 2) has its consumable moved from E to G, unless G is taken; a
new window key another action already uses stays unbound.

General also has "Show the Return Link button" and "Show the Return Check-in
button" (on by default): the game bar's comeback event buttons
(`ExclsiveactiveCodeBtn`, `RecalledSignInBtn`, no server behind them) hide on
Apply.

The full seventeen-entry server quickbar is retained: slots 0-7 are attacks,
8-12 collection favourites, and 13-16 consumables. The HUD uses the original
mount and companion sockets. Arrows switch the active server-provided
bar; edits preserve every untouched entry and other bars. Kind-1 entries resolve
carried item instances or template names, and kind-2 collection entries send
`UseStickerBookItem`. Essence sockets independently follow equipment slots 35
(primary) and 34 (backup); switching uses the normal authoritative inventory
operation. Icons fill their authored sockets and stack amounts refresh from
inventory updates.

Hover cards use the original gold tooltip frame for navigation, settings,
quickbar items, collection entries, essences, skills, gauges and currencies.
Icon-only controls use compact, centered hints sized to their text, with yellow
hotkey captions. The Escape menu, labeled buttons, settings rows and text entries
do not repeat their visible captions in hover cards.
Spell cards use an icon/title header, effect descriptions, blue per-level
bonuses, separators, target, resource/essence costs and cooldown. Re-export
`skilltalentswindow` to resolve static locale tokens from the skill, status-effect
and actor-variable tables (`tooltip_catalog.py`). Values are labeled as base
skill values; actor-dependent formulas remain an em dash with a legend rather
than deleting the whole effect. Active talent/buff damage is not yet evaluated.

All hover cards anchor to their control's rectangle, including moved windows,
quickbar slots, gauges and currency labels. Placement flips to a free side and
clamps within the viewport; oversized cards scale as a whole without clipping
their contents. Mouse motion within the control does not move its card. Only
the dragged item's icon follows the pointer. Text is measured and wrapped before
the border is sized; the dark fill starts at the frame's opaque vertices, with
three extra reference pixels inset vertically to cover the rail texture's
transparent strip. Item cards use a darker, 98.5% opaque fill. Hover cards do not
capture clicks; the General setting also controls inventory hover descriptions.

### Equipment-derived character statistics

`gameplay/stats/calculation.rs` evaluates the server's layered stat arithmetic:
seeded base enchantments, concrete rolled enchantments, progression/tier scaling,
socketed gems/jewels/runes, equipment set thresholds, actor variables and active
status instances (including stacks and numeric parameters). Item-relative bonuses
are applied before weapon and actor bonuses. The secondary essence slot is inert.
Conditional procs contribute only when the server sends their active status.

The cache refreshes on inventory, class, level, status or data changes. Local
movement and run animation use calculated movement speed; cast locks, sequence
animations and release timing use the server's attack-speed breakpoints and each
skill's `ScaleSkillTiming`. Damage, health/resource maxima, armor/resistances,
critical/block values and regeneration feed the character sheet. Combat outcomes
and live health/resource updates remain authoritative on the server. Rift and
per-element damage-increase rows remain zero because the server stat calculator
does not calculate them; this change does not implement new combat procs.

Export from the same DB4 revision used by the server:

```sh
python tools/export_stats.py <export_win32> assets --database .local/dso_server/data/db/static.db4
python tools/export_skills.py <export_win32> assets --database .local/dso_server/data/db/static.db4
python tools/stat_parity_vectors.py .local/dso_server assets client/tests/fixtures/character_stats.json
```

The fixture compares the Rust evaluator with server-generated loadouts, equipment
and socket removal, changed item seeds, pet removal and all four bare classes.
The expected values allow for f32 values on the wire. Regenerate it deliberately
when the server's stat rules or static database change.

The Collector's Bag category buttons are generated from `_Template_Stickerbook`.
They switch pages and retain the authored preview art; the first page uses the
active localized `_Template_StickerbookBanner`. Ownership grids and collection
actions still require stickerbook runtime integration. Map destination captions
use smaller text rasterized at the actual screen scale instead of magnifying a
low-resolution glyph atlas.
