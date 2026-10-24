DSOR_Client - a Drakensang Online client (macOS, experimental)
==============================================================

An independent, unofficial client for the DSO-Reconstruction servers. It is not
affiliated with, endorsed by or sponsored by Bigpoint GmbH or the owners of
Drakensang Online.

This download holds no game data. On first start the client downloads the game's
own files from Bigpoint's CDN, the same files the official launcher fetches, and
converts them on your computer into the form it reads.

This macOS build is experimental: it was built for Apple Silicon (M1 and later)
and not yet played on a Mac by the developers. It is not signed by Apple.
Please report what works and what does not.


Getting started
---------------

1. Unpack the archive somewhere you can write to, with about 35 GB free. It
   keeps about 18 GB; a game update converts into a second copy before
   replacing the first.

2. The build is not signed, so macOS refuses to start it as downloaded. In
   Terminal, clear the download flag on the whole folder once:
     xattr -dr com.apple.quarantine DSOR-...-macos-arm64

3. Start the client from its folder in Terminal (your settings are kept in the
   folder you start it from):
     cd DSOR-...-macos-arm64 && ./dsor-client
   The bar across the top of the window shows:
   - "Downloading the game data": about 2.2 GB (4.8 GB on disk) into
     game-data/. Closing the client pauses it; it resumes on the next start.
   - "Converting the game data": the files become assets/ (about 13 GB).
     This takes a while: about an hour. Closing the client stops it; it
     starts over on the next start.
   The client then starts again by itself.

4. Open Settings to enter your server's account service (host:port), then
   sign in or create an account and play.

When the game publishes an update, the next start downloads the changes and
converts again. On the first sign-in to a server the client fetches its game
database and maps (about 80 MB) and updates the game tables (a few minutes),
then continues.


What is in this folder
----------------------

dsor-client          the client (Apple Silicon; no DLSS on macOS)
converter/           what turns the game data into assets: Python with
                     Pillow and numpy, DSO_Godot, this project's tools,
                     dsor-levels, dsor-ktx2, crn2dds and vgmstream
licenses/            the licenses of the parts above
game-data/           made on first start: the game's files from the CDN
assets/              made on first start: the converted assets
client-auth.json, game-settings.json, ...   your settings

To start again from scratch, delete assets/ (and game-data/ to download again).


If something goes wrong
-----------------------

- The bar says why and offers Retry. The conversion's logs are in
  assets.partial/logs/ (a first conversion) or assets/logs/.
- The terminal shows the client's log; F9 writes a bug report
  (dsor-dump-<time>.json) in the folder you started it from.
- DSOR_FLAGS=noconvert starts without converting; DSOR_GAME_DATA=<folder>
  keeps the game data elsewhere; DSOR_ASSETS=<folder> reads the assets from
  elsewhere.

Source, documentation and issues: https://github.com/DSO-Reconstruction/DSOR_Client
