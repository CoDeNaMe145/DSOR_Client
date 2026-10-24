# The Windows release

[Back to the README](../README.md) | [Game data](game-data.md) | [Asset reference](assets.md)

A release is the client plus everything that turns the game data into its
assets, without the workspace: no Rust toolchain, no Visual Studio, no server
checkout, no `target/` (68 GB here). It holds no game data. On first start the
client downloads the game's files from Bigpoint's CDN ([the game data](game-data.md)),
converts them on the player's machine, and starts again on the result.

## Layout

```
DSOR-<version>-windows-x64/
  dsor-client.exe      the client (with DLSS when built with the SDK)
  nvngx_dlss.dll       DLSS's runtime (local builds only, see below)
  README.txt           tools/release/README.txt: what a player needs to know
  licenses/            DSO_Godot's, vgmstream's, NVIDIA's DLSS SDK's
  converter/
    VERSION            the release's version: a new one converts again
    python/            Python's embeddable distribution + Pillow and numpy
    tools/             tools/*.py (no tests): convert_assets.py and its steps
    DSO_Godot/         at tools/release/dso_godot.rev, with patch_dso_godot.py's fixes
    bin/               dsor-levels.exe, dsor-ktx2.exe, crn2dds.exe, vgmstream/
    level-overrides/   the authored level edits the conversion applies
    vfx-overrides/     the VFX editor's edits
  game-data/           made by the client: the CDN's files
  assets/              made by the client: the converted assets
```

The zip is about 100 MB. Once set up the folder holds about 18 GB: 4.8 GB of
game data and 13.3 GB of assets (107,425 files, the October 22 build). A game
update converts into a second copy before the swap, so about 35 GB should be free.

## Where the client keeps its data

`crate::install_root` (client/src/main.rs) decides: an executable with a
`converter/` beside it is a release and uses its own folder, wherever it is
(also unpacked inside the workspace to test it); one inside the workspace it
was compiled in (`target/...`) uses that workspace, as always; any other uses
its own folder. `assets/` (`DSOR_ASSETS`), `game-data/` (`DSOR_GAME_DATA`) and
`converter/` (`DSOR_CONVERTER`) are found there. The level tool does the same
(crates/level-data/src/bin/dsor-levels.rs): in a release's `converter/bin`
(`converter/VERSION` beside it) it reads no workspace's `.level-editor/`, and
the conversion passes it every path.
Settings files (`client-auth.json`, `game-settings.json`, ...) stay in the
working folder, the client's own when started from Explorer.

## The first start

The account screen's strip ([the game data](game-data.md#running-it)) goes on
after the download:

1. **Checking.** `dsor_game_data::needed` (crates/game-data/src/convert.rs)
   reads `assets/conversion.json`, which the converter writes last:

   ```json
   {"converter": "v0.1.0", "build": "6a8b704d...", "static_db": "81a09c5c...", "finished": "..."}
   ```

   Another `converter` (a new release) or `build` (a game update) converts
   everything; only another `static_db` reruns the steps that read it. A
   workspace has no `converter/`, so the check never runs there:
   `DSOR_CONVERTER=<folder>` points at one, `DSOR_FLAGS=noconvert` turns it off.
2. **Converting.** The bundled Python runs
   `converter/tools/convert_assets.py game-data/export_win32 assets --replace --progress --watch-stdin`.
   `--replace` converts into `assets.partial/` and swaps it in only once
   complete, so a stopped conversion never leaves half an asset tree; the next
   start converts again from the beginning. The converter prints
   `@progress <done> <total> <step>`; the strip shows the step, the share done
   and the minutes so far. Play stays locked. The client keeps the converter's
   stdin open: when it quits, however it quits, the converter kills the running
   step's process tree and stops. The game data's lock is held throughout, so a
   second client waits instead of downloading under it.
3. **Starting again.** The client loaded its account screen and every table
   (skills, NPC templates, item skins...) at start, from the old assets or none,
   and Bevy's asset server does not reload them. So once converted it starts a
   new client with the same arguments and exits.

## The server's databases at sign-in

Signing in may install the account server's `static.db4` and map databases
([the game data](game-data.md#the-account-servers-databases)). The tables made
from them must follow: status effect ids are the database's rows, and the
server names every NPC, prop and exit by its map row's Guid, which each
Bigpoint build gives anew (the October 22 build's maps share none with the
October 20 build's the server plays with). A release converted from the CDN's
current build therefore showed all 68 of Kingshill's NPCs of events not
running: their Guids matched no `npcs.json` entry, so nothing hid them. With a
converter, the sign-in then runs `--database-only` (the card shows *Updating
the game tables*: the steps `steps(..., database_only=True)` of
convert_assets.py list, those that read `static.db4` or `maps/*.db4`), and
starts a new client with the session it just got (`--server <login> --account
<id> --sid <session> --account-api <service>`), which goes straight to the
characters. If that client cannot start, this one plays on with the tables it
has. A sign-in cancelled while the tables were being made lets them finish; the
next sign-in of that client starts a new one too (`game_data::tables_changed`).

The first sign-in to a server of a fresh install downloads its maps (77 MB packed
for 391 maps, 656 MB on disk) and refreshes the tables in about 5 minutes on a
hard disk; later sign-ins only compare the bundle's sha256. A server without the
endpoints (older than 2026-10-25) leaves the CDN's maps in place, and with them
the mismatch.

The database steps leave out `character_tables.py`: the selection screen and
interface steps write its two database tables again, and rerunning it would drop
`export_deco.py`'s sets from `characters/anims.json`.

## Building a release

```powershell
powershell -File tools/release/package.ps1 -Version v0.1.0 -HostPython C:\ProgramData\anaconda3\python.exe
```

builds `dsor-client`, `dsor-levels` and `dsor-ktx2` (`-SkipBuild` takes them
from `CARGO_TARGET_DIR` or `target/release` as they are), compiles `crn2dds.exe`
with MSVC (`/MT /DWIN32`: no runtime to install; its output matches the MinGW
build's byte for byte), downloads Python's embeddable distribution, installs
Pillow and numpy into it with the host Python's pip (wheels for the bundled
Python's version, not the host's), fetches DSO_Godot at the pinned commit
(`-DsoGodot <checkout>` copies a local one instead) and vgmstream, and zips
`dist/DSOR-<version>-windows-x64/`. With the DLSS SDK (`.cargo/config.toml`,
or `DLSS_SDK`) the client has DLSS and the runtime and NVIDIA's license go in;
`-NoDlss` builds without.

`.github/workflows/release.yml` does the same on `windows-latest` without DLSS
(the SDK is not in the repository): a tag `v*` publishes a GitHub release with
the zip, a manual run keeps it as an artifact.

The embeddable Python reads its whole module path from `python312._pth` and
puts no script's folder on it, so the converter starts every script through
`tools/run_tool.py`, which does.

## The converter without a workspace

`tools/convert_assets.py` is the conversion for everyone; `tools/convert_assets.sh`
only runs it. It finds its helpers in this order:

| Helper | Release | Workspace |
|---|---|---|
| `dsor-levels`, `dsor-ktx2` | `converter/bin/` | `cargo run --release` |
| `crn2dds` | `converter/bin/` | `DSO_CRN2DDS`, DSO_Godot's `build/` (built with `c++` when missing) |
| vgmstream | `converter/bin/vgmstream/` | `DSOR_VGMSTREAM`, `.local/vgmstream/`, `PATH` |
| DSO_Godot | `converter/DSO_Godot/` | the third argument, else cloned into `<out>/.DSO_Godot` |
| Tahoma | `C:\Windows\Fonts\tahoma.ttf` (macOS: Supplemental) | the same |

Since the release, the conversion needs no server checkout: navigation meshes
come from the game data's `.tok` files through `tools/navmesh_tok.py` (the same
bytes as the server's reader for all 506 maps, twice as fast), and the quest
catalogue from the game data's own `quests.pbxml` (the server's `quests.xml` is
that pack decoded, from an older build). DSO_Godot looks for `build/crn2dds`
without `.exe`, so on Windows the converter always names the transcoder through
`DSO_CRN2DDS`.
