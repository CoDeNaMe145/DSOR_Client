"""Applies to a DSO_Godot checkout the fixes the release needs and its
pinned commit (tools/release/dso_godot.rev) lacks.

  patch_dso_godot.py <DSO_Godot>

- dsoexp/resources.py named its Crunch cache files after the source path with
  only the separators replaced: on Windows the drive's colon stayed in the
  name, so every .crn texture failed to cache and was skipped.
"""
import sys
from pathlib import Path

FIXES = {
    "dsoexp/resources.py": [
        ("import os\nimport shutil\n", "import os\nimport hashlib\nimport shutil\n"),
        ('    key = os.path.abspath(src).lstrip(os.sep).replace(os.sep, "__")\n',
         '    key = hashlib.sha256(os.path.abspath(src).encode("utf-8")).hexdigest()\n'),
    ],
}

if __name__ == "__main__":
    root = Path(sys.argv[1])
    for name, edits in FIXES.items():
        path = root / name
        text = path.read_text(encoding="utf-8").replace("\r\n", "\n")
        for old, new in edits:
            if new in text:
                continue
            if text.count(old) != 1:
                sys.exit(f"{path}: the text to fix is not there once: {old!r}")
            text = text.replace(old, new)
        path.write_text(text, encoding="utf-8", newline="\n")
        print(f"patched {path}")
