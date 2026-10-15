# VFX overrides

Edits made with the VFX editor (`tools/vfx_editor`, see `docs/vfx-editor.md`):

- `models/<asset path>.json`: an effect model's edits (node extras, material textures)
- `sequences.json`: edited spell sequences, whole

They are applied over the converted assets by `tools/vfx_apply.py` (the editor does it
as you edit; `tools/convert_assets.sh` does it after a conversion). Commit them to share
an effect; delete a file (or use **Revert to game**) to go back to the game's own data.
