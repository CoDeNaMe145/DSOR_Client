# DSOR_Client

A Drakensang Online client for the browser (WebAssembly) and the desktop, written
in Rust on Bevy, that plays against the DSO-Reconstruction server
(`experimental`, branch `rust-18`, or the Python `multiplayer-2018`).

The goal is a 1:1 reconstruction of the rel 261 client.

## Quick start

Follow [Getting started](docs/getting-started.md) to install prerequisites and
convert the game's data into `assets/` (generated locally and never committed).
Then build and launch the native client:

```sh
cargo build --locked --release -p dsor-client
cargo run --locked --release -p dsor-client
```

On Windows, `Play.cmd` launches the built client. Signing in requires a running
account service (default: `http://127.0.0.1:8080`). The guide also covers
[offline viewing](docs/getting-started.md#3-native-client),
[browser builds](docs/getting-started.md#4-browser-client), and
[in-game controls](docs/getting-started.md#5-in-game).

## Editors

- **[Level editor](docs/level-editor.md):** run `LevelEditor.cmd` or
  `cargo run --release -p dsor-client -- --editor` to edit maps, DB4 records,
  cameras, and lighting. Saved edits live in `level-overrides/`.
- **[VFX editor](docs/vfx-editor.md):** run `VfxEditor.cmd` or
  `python3 tools/vfx_editor/server.py`, then open <http://localhost:8091> to edit
  spell effects with a live preview. Saved edits live in `vfx-overrides/`.

## Layout

| Path | Purpose |
|---|---|
| `client` | Bevy game client for native and WebAssembly builds. |
| `crates/raknet` | Transport-independent RakNet 4.035 client protocol. |
| `crates/proto` | Game commands and RakNet BitStream codec. |
| `crates/relay` | WebSocket-to-UDP relay for browser clients; carries datagrams unchanged. |
| `crates/level-data` | Level overrides, DB4 composition, event layers, and the headless `dsor-levels` tool. |
| `tools` | Asset conversion via DSO_Godot and development helpers. |

## Documentation

- [Source layout](docs/source-layout.md): where client features and supporting crates live.
- [Getting started](docs/getting-started.md): prerequisites, asset conversion, native and browser clients, controls.
- [Testing and diagnostics](docs/testing-and-diagnostics.md): automated checks, environment variables, profiling.
- [Asset reference](docs/assets.md): converted data formats and conversion details.
- [Effects and rendering](docs/fx.md): shaders, lighting, particles, and rendering diagnostics.
- [Interface](docs/ui.md): UI assets, implementation, and interactions.
- [Native map performance](docs/native-map-performance.md): investigations and comparison commands.
- [Client progress](docs/client-progress.md): feature gaps and implementation results.
