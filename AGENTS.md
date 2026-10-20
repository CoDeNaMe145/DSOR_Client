# Notes for coding agents

Read [docs/](docs/) for the area you change; these rules apply everywhere.

## Frame-time rules

- **The interface does no work on a frame where nothing it shows changed.** Never
  rebuild a view, layout or list every frame to compare it with the last one;
  rebuild when an input changes (`Net::ui_revision`, `inventory.revision`, the
  change ticks of the resources read). Never prepare or fill a closed window.
  Write resources only when the value changes. Count rebuild work in
  `ui::UiWork` and test that idle frames leave it at 0. Details and patterns:
  [docs/ui.md, "Per-frame cost"](docs/ui.md#per-frame-cost).
- Code that edits a `Net` field a window shows outside `net::on_command` calls
  `Net::touch_ui()`.
- After a change that adds per-frame systems, measure:
  `python tools/profile_native_maps.py kingscity <name>` (the `ui work:` line
  must stay at 0 in a still scene), and for a breakdown the `system-profile`
  build ([docs/testing-and-diagnostics.md](docs/testing-and-diagnostics.md#performance-diagnostics)).

Why: on 2026-10-21 one commit of new windows cut Kingscity from about 180 to
95 FPS (50–60 online) by preparing every closed menu and rebuilding every
window's view each frame.
