#!/usr/bin/env python3
"""Collect extracted music with descriptive WAV names and VLC playlists.

Usage: collect_music.py <assets> [--min-seconds 30] [--out <folder>]

Reads audio/catalog.json produced by export_audio.py. Music-labelled banks or
samples are included, with byte-identical WAVs grouped together. Short cues
remain available separately. Hard links avoid duplicating the decoded audio;
copying is used when the destination filesystem cannot link the files.
"""
import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import uuid


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def publish(source, destination, digest):
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_file() and sha256(destination) == digest:
        return "reused"
    temporary = destination.with_name(f".{destination.name}.{uuid.uuid4().hex}.tmp")
    try:
        try:
            os.link(source, temporary)
            method = "linked"
        except OSError:
            shutil.copyfile(source, temporary)
            method = "copied"
        temporary.replace(destination)
        return method
    finally:
        temporary.unlink(missing_ok=True)


def playlist(path, tracks):
    lines = ["#EXTM3U"]
    for track in tracks:
        title = track["name"].replace("\n", " ").replace("\r", " ")
        lines.extend([f"#EXTINF:{round(track['seconds'])},{title}", track["path"]])
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def collect_music(assets, output, min_seconds):
    catalog = json.loads((assets / "audio/catalog.json").read_text(encoding="utf-8"))
    unique, source_count = {}, 0
    for bank, entry in sorted(catalog["banks"].items()):
        for sample in entry["samples"]:
            name = sample["name"] or Path(bank).stem
            if "music" not in (bank + " " + name).lower():
                continue
            source_count += 1
            source = (assets / sample["path"]).resolve()
            if not source.is_relative_to(assets):
                raise ValueError(f"sample path is outside the asset tree: {sample['path']}")
            if source.stat().st_size != sample["bytes"]:
                raise ValueError(f"sample size does not match the catalog: {sample['path']}")
            digest = sha256(source)
            if digest not in unique:
                unique[digest] = {
                    "name": name, "seconds": sample["frames"] / sample["sample_rate"],
                    "sample_rate": sample["sample_rate"], "channels": sample["channels"],
                    "frames": sample["frames"], "bytes": sample["bytes"],
                    "sha256": digest, "sources": [],
                }
            unique[digest]["sources"].append({
                "bank": bank, "index": sample["index"],
                "name": sample["name"], "path": sample["path"],
            })
    if not unique:
        raise ValueError("no music-labelled banks or samples found; run export_audio.py first")

    tracks = sorted(unique.values(), key=lambda track: (track["name"].lower(), track["sha256"]))
    output.mkdir(parents=True, exist_ok=True)
    methods = {"linked": 0, "copied": 0, "reused": 0}
    for index, track in enumerate(tracks, 1):
        category = "tracks" if track["seconds"] >= min_seconds else "short_cues"
        name = re.sub(r"[^A-Za-z0-9_-]+", "_", track["name"]).strip("_")[:120] or "music"
        track["path"] = f"{category}/{index:03d}_{name}.wav"
        track["category"] = category
        source = assets / track["sources"][0]["path"]
        method = publish(source, output / track["path"], track["sha256"])
        methods[method] += 1

    long_tracks = [track for track in tracks if track["category"] == "tracks"]
    short_cues = [track for track in tracks if track["category"] == "short_cues"]
    playlist(output / "music.m3u8", long_tracks)
    playlist(output / "short_cues.m3u8", short_cues)
    playlist(output / "all_music.m3u8", long_tracks + short_cues)
    document = {"version": 1, "min_track_seconds": min_seconds,
                "source_samples": source_count, "unique_samples": len(tracks), "tracks": tracks}
    (output / "catalog.json").write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    with (output / "tracks.csv").open("w", encoding="utf-8-sig", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(["name", "duration", "seconds", "category", "path", "source_banks"])
        for track in long_tracks + short_cues:
            seconds = round(track["seconds"])
            writer.writerow([track["name"], f"{seconds // 60}:{seconds % 60:02d}",
                             round(track["seconds"], 3), track["category"], track["path"],
                             "; ".join(sorted({s["bank"] for s in track["sources"]}))])
    (output / "README.txt").write_text(
        f"Extracted game music\n\n"
        f"Open music.m3u8 in VLC for {len(long_tracks)} tracks of at least {min_seconds:g} seconds.\n"
        f"Open all_music.m3u8 to include the {len(short_cues)} shorter music cues and jingles.\n"
        f"tracks.csv lists names and durations; catalog.json preserves every source bank reference.\n\n"
        f"Collected {source_count} music-labelled samples as {len(tracks)} distinct WAV files.\n"
        f"These are the original music segments/layers, without added looping or event mixing.\n"
        f"Files use hard links where supported. Editing a linked WAV also changes its source sample.\n",
        encoding="utf-8")
    print(f"[music] {source_count} music samples -> {len(tracks)} unique WAVs")
    print(f"[music] {len(long_tracks)} tracks >= {min_seconds:g}s "
          f"({sum(t['seconds'] for t in long_tracks) / 60:.1f} minutes), {len(short_cues)} short cues")
    print(f"[music] {methods} -> {output}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("assets", type=Path)
    parser.add_argument("--out", type=Path, help="default: <assets>/audio/music")
    parser.add_argument("--min-seconds", type=float, default=30,
                        help="minimum duration in the main playlist; shorter cues are kept separately")
    args = parser.parse_args()
    if not 0 <= args.min_seconds < float("inf"):
        parser.error("--min-seconds must be a finite nonnegative number")
    assets = args.assets.resolve()
    output = (args.out or assets / "audio/music").resolve()
    if output == assets / "audio" or any(
            output.is_relative_to(assets / "audio" / name) for name in ("samples", "source")):
        parser.error("music output must be separate from the source audio and sample directories")
    try:
        collect_music(assets, output, args.min_seconds)
    except (OSError, ValueError) as error:
        parser.exit(1, f"music collection failed: {error}\n")


if __name__ == "__main__":
    main()
