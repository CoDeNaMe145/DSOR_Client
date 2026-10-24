#!/bin/sh
# Convert an unpacked Drakensang Online client into the assets this client
# loads. tools/convert_assets.py does the work (on Windows run it directly:
# no shell needed); this keeps the old command working.
#
#   tools/convert_assets.sh <export_win32> <out_dir> [<DSO_Godot>]
#
# Afterwards point the client at it:  ln -sfn <out_dir> <workspace>/assets
#
# Needs python3 with Pillow and numpy (PYTHON=<python> picks another), cargo
# for the level and texture tools, a C++ compiler and curl for the .crn
# transcoder, git (only when cloning DSO_Godot), and vgmstream-cli for audio
# (on PATH or DSOR_VGMSTREAM=/path/to/vgmstream-cli). Logs go to <out_dir>/logs/.
set -eu
exec "${PYTHON:-python3}" "$(dirname "$0")/convert_assets.py" "$@"
