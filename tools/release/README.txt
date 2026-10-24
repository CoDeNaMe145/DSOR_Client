DSOR_Client - a Drakensang Online client
========================================

An independent, unofficial client for the DSO-Reconstruction servers. It is not
affiliated with, endorsed by or sponsored by Bigpoint GmbH or the owners of
Drakensang Online.

This download holds no game data. On first start the client downloads the game's
own files from Bigpoint's CDN, the same files the official launcher fetches, and
converts them on your computer into the form it reads.


Getting started
---------------

1. Unzip this folder somewhere you can write to, with about 35 GB free:
   not under "Program Files". It keeps about 18 GB; a game update converts
   into a second copy before replacing the first. An SSD makes the first
   start much faster.

2. Start dsor-client.exe. The bar across the top of the window shows:
   - "Downloading the game data": about 2.2 GB (4.8 GB on disk) into
     game-data\. Closing the client pauses it; it resumes on the next start.
   - "Converting the game data": the files become assets\ (about 13 GB).
     This takes a while: about an hour on a desktop computer, more on a
     hard disk or with few cores. Closing the client stops it; it starts
     over on the next start.
   The client then starts again by itself.

3. Open Settings to enter your server's account service (host:port), then
   sign in or create an account and play.

When the game publishes an update, the next start downloads the changes and
converts again. When your server uses its own game database, the first sign-in
fetches it and updates the game tables (a minute or two), then continues.


What is in this folder
----------------------

dsor-client.exe      the client
nvngx_dlss.dll       NVIDIA DLSS (used on RTX cards only), when present
converter\           what turns the game data into assets: Python with
                     Pillow and numpy, DSO_Godot, this project's tools,
                     dsor-levels, dsor-ktx2, crn2dds and vgmstream
licenses\            the licenses of the parts above
game-data\           made on first start: the game's files from the CDN
assets\              made on first start: the converted assets
client-auth.json, game-settings.json, ...   your settings

To start again from scratch, delete assets\ (and game-data\ to download again).


If something goes wrong
-----------------------

- The bar says why and offers Retry. The conversion's logs are in
  assets.partial\logs\ (a first conversion) or assets\logs\.
- F9 writes a bug report (dsor-dump-<time>.json) beside the client.
- DSOR_FLAGS=noconvert starts without converting; DSOR_GAME_DATA=<folder>
  keeps the game data elsewhere; DSOR_ASSETS=<folder> reads the assets from
  elsewhere.

Source, documentation and issues: https://github.com/DSO-Reconstruction/DSOR_Client
