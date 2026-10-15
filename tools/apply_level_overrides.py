#!/usr/bin/env python3
"""Reapply authored level edits after asset conversion, or export them on demand.

python tools/apply_level_overrides.py --assets assets --db-root <server>/data/maps
Only maps with an override document are processed. Conflicts fail the build and
leave the authored patch intact. No original DB4 is modified.
"""
import argparse
import json
import os
from pathlib import Path
import sys

from level_data import Project, read_json
from level_export import build


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("maps", nargs="*")
    parser.add_argument("--assets", type=Path)
    parser.add_argument("--db-root", type=Path)
    parser.add_argument("--source-root", type=Path, help="Conversion input; maps/ is used if no DB4 source is configured")
    parser.add_argument("--overrides", type=Path)
    parser.add_argument("--cache", type=Path)
    parser.add_argument("--db-out", type=Path, help="Derived DB4 directory; cannot be the original maps directory")
    parser.add_argument("--check", action="store_true", help="Check conflicts without writing client or derived DB4 files")
    args = parser.parse_args()
    config = read_json(root / ".level-editor" / "project.json", {})
    db_root = args.db_root or os.environ.get("DSOR_MAP_DB") or config.get("db_root")
    if not db_root and args.source_root:
        db_root = args.source_root / "maps"
    project = Project(root, args.assets or os.environ.get("DSOR_ASSETS") or config.get("assets"), db_root,
                      args.overrides or os.environ.get("DSOR_LEVEL_OVERRIDES") or config.get("overrides"), args.cache)
    maps = args.maps or sorted(path.stem for path in project.overrides.glob("*.json"))
    failures = []
    for name in maps:
        try:
            session = project.open(name)
            if session.conflicts:
                failures.append({"map": name, "conflicts": session.conflicts})
            elif args.check:
                print(json.dumps({"map": name, "conflicts": [], "sources": session.sources}))
            else:
                print(json.dumps(build(session, db_out=args.db_out)))
        except (OSError, ValueError, KeyError) as exc:
            failures.append({"map": name, "error": str(exc)})
    if failures:
        print(json.dumps({"level_override_errors": failures}, indent=2), file=sys.stderr)
        return 1
    if not maps:
        print("[levels] No authored overrides to apply")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
