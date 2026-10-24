DSOR_Client - a Drakensang Online client (Linux, experimental)
==============================================================

An independent, unofficial client for the DSO-Reconstruction servers. It is not
affiliated with, endorsed by or sponsored by Bigpoint GmbH or the owners of
Drakensang Online.

This download holds no game data. On first start the client downloads the game's
own files from Bigpoint's CDN, the same files the official launcher fetches, and
converts them on your computer into the form it reads.

This Linux build is experimental: it was built on Ubuntu 22.04 (x86_64) and
not yet played on a Linux machine by the developers. Please report what works
and what does not.


Requirements
------------

- x86_64 Linux with glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36...)
- A graphics driver with Vulkan (Mesa or NVIDIA), X11 or Wayland
- The usual desktop libraries: ALSA (libasound2), libudev, libxkbcommon,
  libwayland or libX11. On Ubuntu/Debian:
    sudo apt install libasound2 libudev1 libxkbcommon0 libwayland-client0 libvulkan1
- No DLSS in this build.


Getting started
---------------

1. Unpack the archive somewhere you can write to, with about 35 GB free
   (tar xzf DSOR-...-linux-x64.tar.gz). It keeps about 18 GB; a game update
   converts into a second copy before replacing the first. An SSD makes the
   first start much faster.

2. Start the client from its folder (your settings are kept in the folder you
   start it from):
     cd DSOR-...-linux-x64 && ./dsor-client
   The bar across the top of the window shows:
   - "Downloading the game data": about 2.2 GB (4.8 GB on disk) into
     game-data/. Closing the client pauses it; it resumes on the next start.
   - "Converting the game data": the files become assets/ (about 13 GB).
     This takes a while: about an hour on a desktop computer, more on a
     hard disk or with few cores. Closing the client stops it; it starts
     over on the next start.
   The client then starts again by itself.

3. Open Settings to enter your server's account service (host:port), then
   sign in or create an account and play.

When the game publishes an update, the next start downloads the changes and
converts again. On the first sign-in to a server the client fetches its game
database and maps (about 80 MB) and updates the game tables (a few minutes),
then continues.


What is in this folder
----------------------

dsor-client          the client
converter/           what turns the game data into assets: Python with
                     Pillow and numpy, DSO_Godot, this project's tools,
                     dsor-levels, dsor-ktx2, crn2dds, vgmstream, and Wine's
                     Tahoma font (the game's font is not on Linux)
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
