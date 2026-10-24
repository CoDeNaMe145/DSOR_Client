"""Runs a conversion script as `python <script> args...` would, with the
script's folder first on sys.path.

  run_tool.py <script.py> [args...]

tools/convert_assets.py starts every script through this: an embedded Python
(the release's, docs/release.md) reads its paths from python3*._pth and adds no
script's folder, while the tools and DSO_Godot's scripts import their neighbours.
"""
import runpy
import sys
from pathlib import Path

if __name__ == "__main__":
    script = Path(sys.argv[1]).resolve()
    sys.argv = [str(script), *sys.argv[2:]]
    sys.path.insert(0, str(script.parent))
    runpy.run_path(str(script), run_name="__main__")
