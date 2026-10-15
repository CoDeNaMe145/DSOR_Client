#!/usr/bin/env python3
"""Compatibility entry point: DB4 prop placement/catalog export is implemented in Rust."""
import sys
from apply_level_overrides import run

if __name__ == '__main__':
    if len(sys.argv) != 3:
        raise SystemExit('usage: export_props.py <export_win32> <assets>')
    raise SystemExit(run('props', '--source-root', sys.argv[1], '--assets', sys.argv[2]))
