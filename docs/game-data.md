# The game data from Bigpoint's CDN

[Back to the README](../README.md) | [Getting started](getting-started.md) | [Asset reference](assets.md)

The client downloads the original game client's data itself, straight from
Bigpoint's CDN, into `game-data/` at the workspace root. The account screen
shows how far that is in a strip across its top, and signing in to play waits
until every file is on disk. No official launcher is needed. The data belongs
to the publisher: `game-data/` is gitignored and never committed.

The result is the `export_win32/` tree the conversion reads, exactly as the
CDN's tables list it, with the database at `export_win32/db/static.db4`:

```sh
python3 tools/convert_assets.py game-data/export_win32 <out>   # then point assets/ at <out>
```

A [release](release.md) does that itself: once the folder is complete, the
strip goes on to convert it into `assets/` and the client starts again.

## Running it

- **The client**: start it without arguments (or `Play.cmd`). The strip shows
  the stage, the bar, the bytes, the speed and the time left. On a failure it
  says why and offers **Retry**. Play stays locked (the button is dimmed and
  refuses) until the strip says *Game data ready* (in a release, until the
  assets are converted too). Saving the settings is never held up. A complete folder is checked again at every start, which takes
  about a second.
- **The command line**, same code, no window:
  `cargo run --release -p dsor-game-data -- [<folder>] [--downloads N] [--writers N] [--cdn URL]`
  (folder `game-data` by default; it exits 0 once the folder is complete).

| Variable | What |
|---|---|
| `DSOR_GAME_DATA=<folder>` | keep the game data elsewhere (a folder that holds `export_win32/`) |
| `DSOR_GAME_DATA_CDN=<url>` | another CDN or mirror (default `https://heredur.drakensang.com/cdndata`) |
| `DSOR_FLAGS=nogamedata` | no check, no strip, play never waits: for work that does not need the files |
| `DSOR_CONVERTER=<folder>` | a release's converter elsewhere; `DSOR_FLAGS=noconvert` converts nothing ([the release](release.md)) |

Only the account screen checks. The map viewer, `--play` and the direct
`--server ... --sid` start do not.

## What the CDN serves

The CDN holds **one build, the current one**: the October 20 build's key
(`f910a2e8...`) answered 404 once the October 22 build (`6a8b704d...`)
replaced it. A download is always the build that is live. The server's
database must still match the client's data (status effect ids follow its
rows, see [getting started](getting-started.md#after-pulling-new-code)), so
when the CDN moves ahead of the server, the converted assets can drift from it.
`game-data/__root.txt` names the build on disk.

Everything is checked from one key down (the official launcher hands the game
the same root: `-rooturl httpnz://alicdn-prod-774.drakensang.com/cdndata
-rootkey <key>`):

- `__root.txt`: the build key, which is the md5 of the main table of contents.
- Every file, table and bundle is at `<path>.<md5>.nz`, where md5 is that of
  the stored blob: `__ZN` (or `ZN__`), the unpacked size (u32, little-endian),
  then a zlib stream. The parent table lists that md5.
- The main table `__toc.<key>.nz` lists every folder: `path|d|md5`, 2,843 of
  them. Each folder's table `<folder>/__toc.<md5>.nz` lists its files with full
  paths: `path|f|md5`. Beside every table, `__toc_human_readable.txt` is the
  same listing unpacked. The downloader reads the `.nz` tables, because their
  md5 checks them and their address never changes meaning.
- `bundles/required` (10) and `bundles/optional` (634) list the NB3 bundles
  (`.nb`, `__ZN`-packed like everything else). Unpacked, a bundle is
  `_B3NHB3N`; the member count, 24 (the name table), the offsets of the records
  and of the data (u32 each); the names (u16 length, UTF-8); then a 44-byte
  record per member: its name's offset in the name table, the md5 of its
  stored blob (32 hex digits), its size, its offset from the data offset. The
  members are `__ZN` blobs themselves, and their md5 is the one the folder
  tables list. The format is
  [simo8902's drakensang-nb3-bundle-extractor](https://github.com/simo8902/drakensang-nb3-bundle-extractor)'s
  (WTFPL), reimplemented in `crates/game-data/src/format.rs`.
- The 644 bundles hold all 164,026 `export_win32` files: 2.21 GB to download,
  4.75 GB unpacked. Each file is also on the CDN on its own.
- **The database's `global/` folder.** One member is not where the tables put
  it: bundle633 holds `global/export_win32/db/static.db4`, which the tables list
  as `export_win32/db/static.db4` with the same md5. The downloader drops a
  leading `global/` before `export_win32/` and places a member only where the
  tables list its md5, so the whole tree stays inside `export_win32/`.
  Extractors that keep member names as they are write the database to
  `global/export_win32/db/` instead.
- 13 paths contain spaces (`%20` in URLs) and 12 contain parentheses. No two
  paths differ only in case. Some files share an md5 (identical content under
  several names), so the downloader keys everything by path.
- Not downloaded: the `/applet` files (the game's executables and DLLs, LZMA
  `_2ZN` `.nz2` blobs), which are not data. The `.pbxml` packs (`sequences/`,
  `quests/`) stay packed: the extractor also unpacks them into loose `.bxml`
  files, but our tools read the packs.

## The account server's static database

A server can play with another `static.db4` than the CDN's. The live one has
its own rows (the admin buffs of `tools/install_admin_effects.py`) on top of
another build's. Status effects travel by row (wire id = rowid - 1), so the
client's copy must be the server's, byte for byte. The account service
publishes its database (dso_server `services/content.py`, its
`docs/OPERATIONS.md`), and signing in on the account screen fetches it:

1. Right after `/api/login`, `GET /api/content` gives the server's sha256 and
   size. A server without the endpoint, or one that publishes nothing, ends it
   there.
2. If `game-data/export_win32/db/static.db4` has another sha256,
   `POST /api/content/static-db` (with the session; signed-in players only, a
   few times per address and quarter hour) brings the server's, packed as the
   CDN packs files (7.5 MB for 68.8 MB).
3. Its size, sha256 and SQLite header are checked, then it replaces the folder's
   copy in one step. `.dsor/overrides.txt` records it (`sha256 \t size \t
   source \t path`), so the CDN check keeps it. If the file is lost, the check
   brings back the CDN's version and the next sign-in brings the server's
   again. A later sign-in with the copy already in place costs one request.

The account card says *Checking the server's game database...* meanwhile. A
failure is logged (`game data: the server's static database: ...`) and play
goes on: the client reads no `static.db4` while it runs. The conversion is
what reads the database (status effects, skills, items, equipment), so after a
new one arrives, rerun the generators over `game-data/export_win32`
(`tools/convert_assets.py ... --database-only` reruns just those; a release
does it at sign-in, see [the release](release.md#the-servers-database-at-sign-in)).
Only the dsor client asks these endpoints: the official client, its launcher
and everything they call are unchanged.

Measured on 2026-10-24 against the live tree's account service: the first
`/api/content` hashed and packed the server's database in 0.6 s. The sign-in
that followed replaced the CDN's copy (`4e4b9038...`) with the server's
(`81a09c5c...`) in 0.17 s, and the next sign-in found it matching without a
download. A wrong session got 401 and nothing was written.

## How a run works

`crates/game-data` (`dsor_game_data::run`) brings a folder to the CDN's build:

1. **Connecting**: fetch `__root.txt`.
2. **Listing**: read the main table and every folder's (2,844 tables, cached by
   md5 in `.dsor/tocs/`, so an update only fetches the tables that changed). If
   `__root.txt` on disk already names this build, the manifest is the list and
   no table is read.
3. **Checking**: a file counts as present when the manifest has it at the
   tables' md5 and its size on disk matches. One directory listing per folder
   gives every size (no file is opened).
4. **Downloading**: up to 2,000 missing files are fetched one by one. More than
   that unpacks bundles: those not unpacked completely at their md5 (a fresh
   folder, an interrupted run, a new build) and those that held a file now
   missing (the manifest records each file's bundle). Single files then fill
   in whatever is left. Every blob is checked against its md5 before it is
   used, and every member against its own md5 before it is written. A download
   is retried three times, after a 1, 2 and 4 second pause.
5. **Complete**: files that an older build had and this one dropped are
   removed (only files the downloader wrote, at the size it wrote them). Then
   the manifest is rewritten and `__root.txt` written last.

Without the CDN, the build that `__root.txt` names is checked instead, and play
is allowed if it is complete (the strip says *offline*). A second client on the
same folder is refused while the first runs (`.dsor/lock`).

The downloader's own files are in `game-data/.dsor/`:

| File | What |
|---|---|
| `manifest.txt` | a line per file written: `md5 \t size \t bundle-md5 or - \t path`; the last line of a path counts |
| `bundles.txt` | the bundles unpacked completely: `md5 \t path` |
| `tocs/<md5>` | the tables of contents read |
| `overrides.txt` | files kept in another version than the tables': the account server's `static.db4` (`sha256 \t size \t source \t path`) |
| `tmp/` | tables being written |
| `lock` | held by the running download |

Lines are appended as work completes and count only once their newline is on
disk. Files are written in place in one go, and a file counts only once its
manifest line follows it. So a run cut short (closing the client, a crash, a
lost connection) loses at most the files being written, and the next run
resumes from there.

## Speed

Bundles stream in over 16 connections while 8 threads unpack and write the
ones already in, with a bounded queue between them, so neither the network nor
the disk waits for the other. The bundle sizes, for the bar, are fetched beside
the downloads. Tables and single files use 32 connections. Folders are made
before the first write, and only the members a folder lacks are hashed and
unpacked. While the download runs, the account screen draws at most 60 frames
a second (twice a second behind other windows).

Measured on 2026-10-24 on this workstation (about 10 MB/s down, Defender
real-time scanning on):

| | Time |
|---|---|
| First install on the HDD volume (2.21 GB, 164,026 files) | 5 min 42 s: tables 19 s, then download and unpack at 7.0 MB/s on average |
| An interrupted install resumed on the NVMe drive (2.13 GB left, 626 bundles) | 3 min 47 s, about 10 MB/s: the connection was the limit |
| Every later start (build key, all files checked) | 1.0 s |

The tables are 2,844 small requests of about 190 ms each (a 100 ms round trip
to heredur). 32 at once read 170 a second, and 64 read no more, so a first
listing takes about 15 s; later runs read them from the cache. The connection
pool keeps every connection open: ureq's default of 10 made most requests
start a new TLS handshake. Twelve bundles at once came at 8.3 to 8.5 MB/s from
heredur and 10.0 MB/s from `alicdn-prod-774.drakensang.com`, the Alibaba CDN
the official launcher points the game at (same build, edge caches in Germany).
Point `DSOR_GAME_DATA_CDN` there to try it.

Creating the files dominates on a hard disk. One thread created a file in
4.5 ms there. 8 threads reached 1,550 files a second and 16 no more, while a
temporary file renamed into place managed 135 a second at any thread count.
The same writes on the NVMe drive ran at 4,700 files a second on one thread
and 6,400 on eight. Put the game data on an SSD (`DSOR_GAME_DATA`) and the
unpacking stops being the limit.

Inflating costs little: 690 MB/s for a bundle and 460 MB/s for its members
with flate2's default backend. zlib-rs is 27% faster on members, under a second
over the whole build, so it is not worth switching the backend the workspace's
image crates share.
