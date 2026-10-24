#!/usr/bin/env python3
"""Builds the Linux or macOS release (docs/release.md): the client, and the
converter that turns the game data the client downloads into its assets. No
game data goes in it. tools/release/package.ps1 builds the Windows one.

  package.py [--version v0.1.0] [--source <checkout>] [--out dist] [--skip-build]

Leaves <out>/DSOR-<version>-<linux-x64|macos-arm64>/ and its .tar.gz (tar keeps
the programs executable). --source builds another checkout (a release tag's)
with this script. Needs the Rust toolchain, a C++ compiler (c++) and the
internet: Python (python-build-standalone), Pillow and numpy, vgmstream,
DSO_Godot, the Crunch headers and, for Linux, Wine's Tahoma.
"""
import argparse
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path

RELEASE = Path(__file__).resolve().parent
PYTHON_BUILD = "20261021"        # astral-sh/python-build-standalone release
PYTHON_MINOR = "3.12"
VGMSTREAM = "r2117"
WINE = "wine-10.0"               # Wine's Tahoma (LGPL-2.1), for Linux
CRUNCH = "https://raw.githubusercontent.com/BinomialLLC/crunch/master/inc"


def target():
    """(release name suffix, Python triple, vgmstream archive) of this machine."""
    if sys.platform.startswith("linux") and platform.machine() in ("x86_64", "AMD64"):
        return "linux-x64", "x86_64-unknown-linux-gnu", "vgmstream-linux.zip"
    if sys.platform == "darwin" and platform.machine() == "arm64":
        return "macos-arm64", "aarch64-apple-darwin", "vgmstream-mac.zip"
    sys.exit(f"no release for {sys.platform} {platform.machine()} (Linux x86_64 or macOS arm64; Windows: package.ps1)")


def run(what, command, cwd=None):
    print(f"[package] {what}", flush=True)
    subprocess.run([str(c) for c in command], cwd=cwd, check=True)


def fetch(url, path):
    print(f"[package] download {url}", flush=True)
    request = urllib.request.Request(url, headers={"User-Agent": "dsor-release"})
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token and url.startswith("https://api.github.com/"):
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=300) as response, open(path, "wb") as f:
        shutil.copyfileobj(response, f)


def executable(path):
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def python_archive(triple, temp):
    """The install-only CPython of PYTHON_BUILD for this triple."""
    listing = temp / "python-build.json"
    fetch(f"https://api.github.com/repos/astral-sh/python-build-standalone/releases/tags/{PYTHON_BUILD}", listing)
    pattern = re.compile(rf"cpython-{re.escape(PYTHON_MINOR)}\.\d+\+{PYTHON_BUILD}-{triple}-install_only\.tar\.gz")
    names = [a["name"] for a in json.loads(listing.read_text())["assets"] if pattern.fullmatch(a["name"])]
    if not names:
        sys.exit(f"python-build-standalone {PYTHON_BUILD} has no CPython {PYTHON_MINOR} for {triple}")
    archive = temp / names[0]
    fetch(f"https://github.com/astral-sh/python-build-standalone/releases/download/{PYTHON_BUILD}/{names[0]}", archive)
    return archive


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--version")
    p.add_argument("--source", type=Path, default=RELEASE.parent.parent)
    p.add_argument("--out", type=Path, default=Path("dist"))
    p.add_argument("--skip-build", action="store_true")
    a = p.parse_args()
    suffix, triple, vgmstream = target()
    source = a.source.resolve()
    version = a.version or subprocess.run(["git", "describe", "--tags", "--always", "--dirty"], cwd=source,
                                          capture_output=True, text=True).stdout.strip() or "dev"
    cargo_target = Path(os.environ.get("CARGO_TARGET_DIR") or source / "target")
    if not a.skip_build:
        # Without DLSS: its SDK is Windows/Linux-only and not in the repository.
        run("build the client", ["cargo", "build", "--locked", "--release", "-p", "dsor-client", "--no-default-features"], cwd=source)
        run("build the level tool", ["cargo", "build", "--locked", "--release", "-p", "dsor-level-data", "--bin", "dsor-levels"], cwd=source)
        run("build the texture packer", ["cargo", "build", "--locked", "--release", "-p", "dsor-texture-pack", "--bin", "dsor-ktx2"], cwd=source)

    name = f"DSOR-{version}-{suffix}"
    out = a.out.resolve()
    stage, temp = out / name, out / f"{name}.tmp"
    for folder in (stage, temp):
        shutil.rmtree(folder, ignore_errors=True)
    converter, licenses = stage / "converter", stage / "licenses"
    bin_dir = converter / "bin"
    for folder in (temp, bin_dir, licenses, converter / "tools"):
        folder.mkdir(parents=True)

    # The client and its notes.
    shutil.copy2(cargo_target / "release" / "dsor-client", stage / "dsor-client")
    shutil.copy2(RELEASE / f"README-{suffix.split('-')[0]}.txt", stage / "README.txt")

    # The converter: tools/convert_assets.py and its tools, the authored edits it applies.
    (converter / "VERSION").write_text(f"{version}\n", encoding="utf-8")
    for script in sorted((source / "tools").glob("*.py")):
        if not script.name.startswith("test_"):
            shutil.copy2(script, converter / "tools" / script.name)
    for folder in ("level-overrides", "vfx-overrides"):
        shutil.copytree(source / folder, converter / folder)
    for tool in ("dsor-levels", "dsor-ktx2"):
        shutil.copy2(cargo_target / "release" / tool, bin_dir / tool)

    # DSO_Godot at the pinned commit, with the fixes it lacks.
    rev = (source / "tools" / "release" / "dso_godot.rev").read_text().strip()
    fetch(f"https://github.com/DSO-Reconstruction/DSO_Godot/archive/{rev}.zip", temp / "godot.zip")
    with zipfile.ZipFile(temp / "godot.zip") as z:
        z.extractall(temp)
    godot = converter / "DSO_Godot"
    shutil.move(str(temp / f"DSO_Godot-{rev}"), godot)
    run("patch DSO_Godot", [sys.executable, source / "tools" / "release" / "patch_dso_godot.py", godot])
    shutil.copy2(godot / "LICENSE", licenses / "DSO_Godot-LICENSE.txt")

    # The Crunch transcoder (crn_decomp.h: public domain). macOS has no
    # malloc_usable_size: its malloc_size answers the same.
    for header in ("crn_decomp.h", "crnlib.h"):
        fetch(f"{CRUNCH}/{header}", temp / header)
    # g++ wants -fpermissive for the 2012 header (DSO_Godot tools/build_crn2dds.sh).
    flags = ["-include", "malloc/malloc.h", "-Dmalloc_usable_size=malloc_size"] if sys.platform == "darwin" \
        else ["-fpermissive", "-static-libstdc++", "-static-libgcc"]
    run("build crn2dds", ["c++", "-O2", "-w", "-fno-strict-aliasing", "-DNDEBUG", *flags, "-I", temp,
                          godot / "tools" / "crn2dds.cpp", "-o", bin_dir / "crn2dds"])

    # vgmstream-cli, for the audio.
    fetch(f"https://github.com/vgmstream/vgmstream/releases/download/{VGMSTREAM}/{vgmstream}", temp / "vgmstream.zip")
    with zipfile.ZipFile(temp / "vgmstream.zip") as z:
        z.extractall(bin_dir / "vgmstream")
    fetch(f"https://raw.githubusercontent.com/vgmstream/vgmstream/{VGMSTREAM}/COPYING", licenses / "vgmstream-COPYING.txt")

    # Python (python-build-standalone) with Pillow and numpy from its own pip.
    with tarfile.open(python_archive(triple, temp)) as t:
        t.extractall(converter, filter="tar")
    python = converter / "python" / "bin" / "python3"
    run("install Pillow and numpy", [python, "-m", "pip", "install", "--disable-pip-version-check",
                                    "--no-warn-script-location", "--no-cache-dir", "Pillow", "numpy"])
    run("check the bundled Python", [python, "-c",
        "import PIL, numpy, sqlite3, multiprocessing, concurrent.futures; print('python ok', PIL.__version__, numpy.__version__)"])

    # Tahoma, the game's font: Linux has none, so Wine's (LGPL-2.1). A converter
    # older than 2026-10-25 (v0.1.0's) does not look for it: it is told to.
    if suffix.startswith("linux"):
        driver = converter / "tools" / "convert_assets.py"
        text = driver.read_text(encoding="utf-8")
        if 'HOME / "fonts" / "tahoma.ttf"' not in text:
            system = 'Path("/usr/share/fonts/truetype/msttcorefonts/tahoma.ttf")'
            if text.count(system + "):") != 1:
                sys.exit(f"{driver}: cannot add the bundled font")
            driver.write_text(text.replace(system + "):", system + ', HOME / "fonts" / "tahoma.ttf"):'), encoding="utf-8")
        (converter / "fonts").mkdir()
        fetch(f"https://gitlab.winehq.org/wine/wine/-/raw/{WINE}/fonts/tahoma.ttf", converter / "fonts" / "tahoma.ttf")
        fetch(f"https://gitlab.winehq.org/wine/wine/-/raw/{WINE}/LICENSE", licenses / "Wine-Tahoma-LICENSE.txt")
        fetch(f"https://gitlab.winehq.org/wine/wine/-/raw/{WINE}/COPYING.LIB", licenses / "Wine-COPYING.LIB.txt")

    for program in (stage / "dsor-client", *bin_dir.iterdir(), bin_dir / "vgmstream" / "vgmstream-cli"):
        if program.is_file():
            executable(program)
    shutil.rmtree(temp)
    archive = out / f"{name}.tar.gz"
    print(f"[package] archive {archive}", flush=True)
    with tarfile.open(archive, "w:gz") as t:
        t.add(stage, arcname=name)
    print(f"[package] done: {archive} ({archive.stat().st_size / 1e6:.1f} MB)", flush=True)


if __name__ == "__main__":
    main()
