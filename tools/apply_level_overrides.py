#!/usr/bin/env python3
"""Compatibility entry point for older conversion wrappers; all logic is Rust.

New integrations should invoke dsor-levels apply directly. Build it with
Build-LevelEditor.ps1 or cargo build --release -p dsor-level-data.
"""
from pathlib import Path
import os
import subprocess
import sys


def run(*args):
    root = Path(__file__).resolve().parent.parent
    suffix = '.exe' if os.name == 'nt' else ''
    binary = root / 'target' / 'release' / ('dsor-levels' + suffix)
    if not binary.is_file():
        raise SystemExit('Build the Rust level tool first: cargo build --release -p dsor-level-data')
    return subprocess.call([str(binary), *args], cwd=root)


if __name__ == '__main__':
    raise SystemExit(run('apply', *sys.argv[1:]))
