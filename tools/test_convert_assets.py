"""tools/convert_assets.py: the step lists, the stamp, the swap, run_tool.py.

  python -m unittest tools/test_convert_assets.py
"""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import convert_assets  # noqa: E402


def alive(pid):
    if os.name == "nt":
        listed = subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/NH"], capture_output=True, text=True).stdout
        return str(pid) in listed
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


class ConvertAssets(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def run_(self):
        return convert_assets.Run(self.dir / "export_win32", self.dir / "assets", self.dir / "godot", False)

    def test_the_database_steps_are_the_full_conversions_in_its_order(self):
        every = [what for what, _, _ in convert_assets.steps(self.run_())]
        database = [what for what, _, _ in convert_assets.steps(self.run_(), database_only=True)]
        self.assertEqual(len(every), len(set(every)), "step names are unique")
        self.assertEqual(database, [what for what in every if what in database], "same order")
        for step in ("monsters", "combat texts", "skills", "NPCs", "VFX edits", "interface windows", "quests", "level edits", "level props"):
            self.assertIn(step, database)
        # character_tables would drop export_deco's sets from anims.json.
        for step in ("models", "character tables", "decorations", "KTX2 textures"):
            self.assertNotIn(step, database)
        self.assertEqual(every[-1], "the font")

    def test_the_stamp_names_the_build_and_the_database(self):
        root = self.dir / "game-data" / "export_win32"
        (root / "db").mkdir(parents=True)
        (root.parent / "__root.txt").write_text("6a8b704d041044688ebee52da5b48b55\n")
        (root / "db" / "static.db4").write_bytes(b"SQLite format 3\0")
        stamp = convert_assets.stamp(root)
        self.assertEqual(stamp["build"], "6a8b704d041044688ebee52da5b48b55")
        self.assertEqual(stamp["static_db"], hashlib.sha256(b"SQLite format 3\0").hexdigest())
        self.assertTrue(stamp["converter"])

    def test_a_finished_conversion_replaces_the_old_one(self):
        out, partial = self.dir / "assets", self.dir / "assets.partial"
        (out / "maps").mkdir(parents=True)
        (out / "old.txt").write_text("old")
        partial.mkdir()
        (partial / "conversion.json").write_text(json.dumps({"converter": "v1"}))
        convert_assets.replace(partial, out)
        self.assertFalse(partial.exists())
        self.assertFalse((self.dir / "assets.old").exists())
        self.assertTrue((out / "conversion.json").is_file())
        self.assertFalse((out / "old.txt").exists())

    def test_refreshing_the_tables_needs_a_conversion(self):
        root = self.dir / "export_win32"
        (root / "models").mkdir(parents=True)
        (root / "maps").mkdir()
        code = subprocess.call([sys.executable, str(Path(convert_assets.__file__)), str(root), str(self.dir / "none"),
                                "--database-only"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.assertEqual(code, 1)

    def test_closing_stdin_stops_the_conversion_and_its_running_step(self):
        # What the client does by quitting: its end of the pipe closes.
        script = (
            "import os, subprocess, sys, time\n"
            f"sys.path.insert(0, {str(convert_assets.TOOLS)!r})\n"
            "import convert_assets\n"
            "step = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'], start_new_session=os.name != 'nt')\n"
            "convert_assets.RUNNING.append(step)\n"
            "print(step.pid, flush=True)\n"
            "convert_assets.watch_stdin()\n"
            "time.sleep(120)\n")
        converter = subprocess.Popen([sys.executable, "-c", script], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, text=True)
        step = int(converter.stdout.readline())
        converter.stdin.close()
        self.assertEqual(converter.wait(timeout=30), 3)
        self.assertFalse(alive(step), "the running step was killed")

    def test_a_watched_conversion_ends_with_its_own_code(self):
        # The client holds stdin open to the end. A thread blocked reading it
        # once made every finished run die at the interpreter's shutdown
        # ("Fatal Python error: _enter_buffered_busy", exit 139).
        root = self.dir / "export_win32"
        (root / "models").mkdir(parents=True)
        (root / "maps").mkdir()
        converter = subprocess.Popen([sys.executable, str(Path(convert_assets.__file__)), str(root), str(self.dir / "none"),
                                      "--database-only", "--watch-stdin"], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        errors = converter.stderr.read()
        self.assertEqual(converter.wait(timeout=30), 1, errors)
        self.assertNotIn("Fatal Python error", errors)
        self.assertTrue(errors.strip().splitlines()[-1].startswith("error:"), errors)
        converter.stdin.close()

    def test_run_tool_puts_the_scripts_folder_first(self):
        tool = self.dir / "tool"
        tool.mkdir()
        (tool / "neighbour.py").write_text("VALUE = 7\n")
        (tool / "script.py").write_text("import sys, neighbour\nprint(neighbour.VALUE, sys.argv[1:], __name__)\n")
        output = subprocess.check_output([sys.executable, str(Path(convert_assets.TOOLS) / "run_tool.py"),
                                          str(tool / "script.py"), "a", "b"], cwd=self.dir, text=True)
        self.assertEqual(output.strip(), "7 ['a', 'b'] __main__")


if __name__ == "__main__":
    unittest.main()
