#!/usr/bin/env python3
"""Compatibility entry point. Seasonal map conversion is implemented in Rust."""
import sys
from apply_level_overrides import run

if __name__ == '__main__':
    raise SystemExit(run('events', *sys.argv[1:]))
