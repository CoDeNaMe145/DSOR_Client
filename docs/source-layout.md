# Source layout

[Back to the README](../README.md)

The Bevy client lives in `client/src`. Its root contains only `main.rs`, which
parses launch options, configures the application, and installs plugins. Feature
modules live in the folders below.

| Folder | Responsibility |
|---|---|
| [`actors/`](../client/src/actors/) | Player, monster, and NPC visuals; equipment appearance, nameplates, and picking shapes. |
| [`diagnostics/`](../client/src/diagnostics/) | Debug menu, bug dumps, performance HUD, frame profiling, and render audits. |
| [`gameplay/`](../client/src/gameplay/) | Skills, inventory state, character statistics, and NPC dialogue behavior. |
| [`level_editor/`](../client/src/level_editor/) | Native editor, DB4 inspector, scene previews, cameras, and editing controls. |
| [`network/`](../client/src/network/) | Game and chat connections, server command application, and movement reconciliation. |
| [`rendering/`](../client/src/rendering/) | Materials, lighting, particles, decals, visibility, postprocessing, and model preparation. WGSL shaders live in `rendering/shaders/`. |
| [`screens/`](../client/src/screens/) | Login, character-selection state, and the shared loading screen. |
| [`ui/`](../client/src/ui/) | Game interface rendering, window controls, pointer ownership, dialogue and selection views, local map, and combat text. |
| [`world/`](../client/src/world/) | Map manifests and loading, navigation, exits, and authored props. |

Each folder's `mod.rs` declares its modules. Cross-feature references use their
full location, such as `crate::world::map::CurrentMap` or
`crate::network::net::Net`. Existing feature submodules and tests stay beside
their parent module: for example, `rendering/materials.rs` owns
`rendering/materials/npc_shared.rs`.

The separation between `screens` and `ui` follows their existing roles:
`screens/selection.rs` manages the character lobby, while `ui/selection.rs`
renders its controls. Likewise, `gameplay/dialogue.rs` handles approaching an NPC
and requesting options; `ui/dialogue.rs` draws the dialogue window.

The level editor and `screens/login.rs` are native-only modules, guarded by
`#[cfg(not(target_arch = "wasm32"))]`. Keep those guards when adding platform
dependencies. The remaining feature groups are shared with browser builds.

Embedded shader URLs follow their source location. Rendering shaders use
`embedded://dsor_client/rendering/shaders/<name>.wgsl`; the interface shader
remains at `embedded://dsor_client/ui/ui.wgsl`. Update both registration and
loading references when moving an embedded asset.

## Supporting crates

| Crate | Responsibility |
|---|---|
| `crates/raknet` | Transport-independent RakNet protocol. |
| `crates/proto` | Game commands, codecs, sessions, and protocol fixtures. |
| `crates/transport` | Native UDP and browser WebSocket transport. |
| `crates/relay` | WebSocket-to-UDP relay for browser clients. |
| `crates/level-data` | Persistent level overrides, DB4 composition, and the `dsor-levels` tool. |

See [Testing and diagnostics](testing-and-diagnostics.md) for validation commands.
