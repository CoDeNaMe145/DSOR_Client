#!/usr/bin/env python3
"""Decode FMOD audio for the client (bindings are built by export_audio_runtime.py).

Usage: export_audio.py <export_win32> <assets> [--vgmstream <vgmstream-cli>]

Requires vgmstream-cli (https://github.com/vgmstream/vgmstream/releases).
Original banks/mappings live under audio/source; decoded PCM16 WAVs under
audio/samples. audio/catalog.json records source hashes, sample names and loop
metadata. WAVs contain one complete pass, with no added repeats or fades.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import uuid
import wave


SCHEMA_VERSION = 1
LOOSE_AUDIO = {".wav", ".ogg", ".mp3", ".flac", ".opus", ".aiff"}


def riff_chunks(data, start, end):
    """Walk FMOD's RIFF lists, including padding after odd-sized chunks."""
    while start < end:
        if start + 8 > end:
            raise ValueError("truncated RIFF chunk header")
        tag, size = struct.unpack_from("<4sI", data, start)
        at = start + 8
        if at + size > end:
            raise ValueError("truncated RIFF chunk")
        if tag == b"LIST":
            if size < 4:
                raise ValueError("truncated RIFF list")
            yield from riff_chunks(data, at + 4, at + size)
        else:
            yield tag, data[at:at + size]
        start = at + size + size % 2


def fsb_samples(data):
    if len(data) < 28 or data[:4] != b"FSB5":
        raise ValueError("expected an unencrypted FSB5 sample bank")
    version, count, headers, names, audio, _codec = struct.unpack_from("<6I", data, 4)
    if version not in (0, 1):
        raise ValueError(f"unsupported FSB5 version {version}")
    header_size = 60 if version == 1 else 64
    if header_size + headers + names + audio > len(data) or headers < count * 8:
        raise ValueError("truncated FSB5 sample bank")
    return count


def bank_samples(data):
    """Count every embedded FSB, distinguishing empty event banks from errors."""
    if data[:4] == b"FSB5":
        return fsb_samples(data)
    if len(data) < 12 or data[:4] != b"RIFF" or data[8:12] != b"FEV ":
        raise ValueError("expected an FMOD RIFF/FEV bank or FSB5 file")
    end = struct.unpack_from("<I", data, 4)[0] + 8
    if end > len(data):
        raise ValueError("truncated FMOD bank")
    chunks = list(riff_chunks(data, 12, end))
    formats = [chunk for tag, chunk in chunks if tag == b"FMT "]
    sound_headers = [chunk for tag, chunk in chunks if tag == b"SNDH"]
    if len(formats) != 1 or len(formats[0]) < 4 or not sound_headers:
        raise ValueError("FMOD bank is missing FMT/SNDH metadata")
    version = struct.unpack_from("<I", formats[0])[0]
    stride = 4 if version <= 0x28 else 8
    total = 0
    for header in sound_headers:
        if not header:  # Event-only banks still need to be preserved.
            continue
        if len(header) < 4 + stride or (len(header) - 4) % stride:
            raise ValueError("invalid FMOD sample-bank index")
        for at in range(4, len(header), stride):
            offset = struct.unpack_from("<I", header, at)[0]
            size = struct.unpack_from("<I", header, at + 4)[0] if stride == 8 else len(data) - offset
            if offset < 12 or offset + size > len(data):
                raise ValueError("FMOD sample bank points outside the file")
            total += fsb_samples(data[offset:offset + size])
    return total


def empty_bank_identity(data):
    """Recognize stripped banks, not valid event-only/shared-sample banks."""
    if data[:4] != b"RIFF":
        return None
    chunks = list(riff_chunks(data, 12, struct.unpack_from("<I", data, 4)[0] + 8))
    if any(payload for tag, payload in chunks if tag not in (b"FMT ", b"BNKI")):
        return None
    identity = next((payload[:16] for tag, payload in chunks if tag == b"BNKI"), b"")
    return identity if len(identity) == 16 else None


def fallback_bank(path, relative, data, fallback):
    identity = empty_bank_identity(data)
    candidate = fallback / relative
    if identity is None or not candidate.is_file():
        return path, data
    replacement = candidate.read_bytes()
    if not bank_samples(replacement) or replacement[:4] != b"RIFF":
        return path, data
    chunks = dict(riff_chunks(replacement, 12, struct.unpack_from("<I", replacement, 4)[0] + 8))
    if chunks.get(b"BNKI", b"")[:16] != identity:
        return path, data  # A same-named bank from a different FMOD project.
    return candidate, replacement


def read_json(path):
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
        return value if isinstance(value, dict) else {}
    except (OSError, ValueError):
        return {}


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    temporary.replace(path)


def valid_wav(path, sample):
    try:
        with wave.open(str(path), "rb") as wav:
            return (wav.getcomptype() == "NONE" and wav.getsampwidth() == 2
                    and wav.getnchannels() == sample["channels"]
                    and wav.getframerate() == sample["sample_rate"]
                    and wav.getnframes() == sample["frames"]
                    and path.stat().st_size == sample["bytes"]
                    and sample["bytes"] >= 44 + sample["frames"] * sample["channels"] * 2)
    except (OSError, EOFError, wave.Error, KeyError):
        return False


def sample_path(relative, index):
    # Numeric names avoid unsafe/duplicate stream names and Windows case clashes.
    # Keep .bank/.fsb in the directory name so those containers cannot collide.
    return Path("audio/samples") / relative / f"{index:04d}.wav"


def publish_wav(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    # TemporaryDirectory has an owner-only ACL on Windows. Moving its files
    # preserves that ACL, locking out the user's media player when the exporter
    # runs as a sandbox account. Create a fresh file in the destination directory
    # so it inherits the asset directory's permissions, then replace atomically.
    temporary = destination.with_name(f".{destination.name}.{uuid.uuid4().hex}.tmp")
    try:
        shutil.copyfile(source, temporary)
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)


def export_bank(job, assets, decoder, previous, force):
    source, relative, digest, count = job
    entry = {"source": (Path("audio/source") / relative).as_posix(),
             "sha256": digest, "samples": []}
    if not count:
        return relative.as_posix(), entry, False
    old = previous.get(relative.as_posix(), {})
    samples = old.get("samples", [])
    if (not force and old.get("sha256") == digest and len(samples) == count
            and all(s.get("index") == i and s.get("path") == sample_path(relative, i).as_posix()
                    and valid_wav(assets / sample_path(relative, i), s)
                    for i, s in enumerate(samples, 1))):
        entry["samples"] = samples
        return relative.as_posix(), entry, False

    with tempfile.TemporaryDirectory(prefix=".decode-", dir=assets / "audio") as folder:
        staging = Path(folder)
        result = subprocess.run(
            [decoder, "-i", "-L", "-I", "-s", "1", "-S", "0",
             "-o", str(staging / "?04s.wav"), str(source)],
            capture_output=True, text=True, encoding="utf-8", errors="replace", check=False)
        if result.returncode:
            raise RuntimeError(f"{relative}: vgmstream failed: {result.stderr.strip()}")
        metadata = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
        if len(metadata) != count:
            raise RuntimeError(f"{relative}: decoded {len(metadata)} of {count} samples: {result.stderr.strip()}")
        for index, info in enumerate(metadata, 1):
            stream = info["streamInfo"]
            if stream["index"] != index or stream["total"] != count:
                raise RuntimeError(f"{relative}: unexpected sample index/count from decoder")
            wav = staging / f"{index:04d}.wav"
            sample = {"index": index, "name": stream["name"],
                      "path": sample_path(relative, index).as_posix(),
                      "sample_rate": info["sampleRate"], "channels": info["channels"],
                      "frames": info["numberOfSamples"], "encoding": info["encoding"],
                      "loop": info["loopingInfo"], "bytes": wav.stat().st_size}
            if not valid_wav(wav, sample):
                raise RuntimeError(f"{relative}: invalid or incomplete WAV for sample {index}")
            entry["samples"].append(sample)
        # Publish only after every sample in this bank has passed validation.
        for sample in entry["samples"]:
            destination = assets / sample["path"]
            publish_wav(staging / destination.name, destination)
    return relative.as_posix(), entry, True


def find_decoder(requested):
    executable = requested or os.environ.get("DSOR_VGMSTREAM") or "vgmstream-cli"
    found = shutil.which(executable)
    if found:
        return str(Path(found).resolve())
    if not requested and not os.environ.get("DSOR_VGMSTREAM"):
        local = Path(__file__).resolve().parents[1] / ".local/vgmstream"
        candidate = local / ("vgmstream-cli.exe" if os.name == "nt" else "vgmstream-cli")
        if candidate.is_file():
            return str(candidate)
    raise FileNotFoundError("vgmstream-cli is required to decode FMOD banks. Install it from "
                            "https://github.com/vgmstream/vgmstream/releases and set "
                            "DSOR_VGMSTREAM or pass --vgmstream (include its companion libraries).")


def export_audio(root, assets, requested=None, workers=4, force=False, fallback_root=None):
    source = (root / "audio").resolve()
    assets = assets.resolve()
    if not source.is_dir():
        raise FileNotFoundError(f"audio directory is missing: {source}")
    fallback = (fallback_root / "audio").resolve() if fallback_root else None
    if fallback and not fallback.is_dir():
        raise FileNotFoundError(f"fallback audio directory is missing: {fallback}")
    output = (assets / "audio").resolve()
    if any(output == tree or output.is_relative_to(tree) or tree.is_relative_to(output)
           for tree in (source, fallback) if tree is not None):
        raise ValueError("source audio and output audio directories must be separate")
    files = sorted(p for p in source.rglob("*") if p.is_file())
    if not files:
        raise ValueError(f"audio directory is empty: {source}")
    jobs, originals, loose, selected = [], {}, [], {}
    for path in files:
        relative = path.relative_to(source)
        data = path.read_bytes()
        primary_digest = hashlib.sha256(data).hexdigest()
        if fallback and path.suffix.lower() == ".bank" and not bank_samples(data):
            path, data = fallback_bank(path, relative, data, fallback)
        digest = hashlib.sha256(data).hexdigest()
        selected[relative.as_posix()] = path
        originals[relative.as_posix()] = {"path": (Path("audio/source") / relative).as_posix(),
                                         "sha256": digest, "bytes": len(data)}
        if digest != primary_digest:
            originals[relative.as_posix()]["fallback"] = {"root": str(fallback_root.resolve()),
                                                         "primary_sha256": primary_digest}
        if path.suffix.lower() in (".bank", ".fsb"):
            try:
                count = bank_samples(data)
            except ValueError as error:
                raise ValueError(f"{relative}: {error}") from error
            jobs.append((path, relative, digest, count))
        elif path.suffix.lower() in LOOSE_AUDIO:
            loose.append(originals[relative.as_posix()]["path"])
    decoder = find_decoder(requested) if any(job[3] for job in jobs) else None
    print(f"[audio] {len(files)} source files, {len(jobs)} banks, "
          f"{sum(job[3] for job in jobs)} samples", flush=True)
    restored = sum("fallback" in entry for entry in originals.values())
    if restored:
        print(f"[audio] restored {restored} stripped banks from {fallback}", flush=True)
    output.mkdir(parents=True, exist_ok=True)
    previous = read_json(output / "catalog.json")
    previous_banks = previous.get("banks", {}) if previous.get("version") == SCHEMA_VERSION else {}
    # Preserve all authoring metadata, including banks without embedded samples.
    for relative, path in selected.items():
        entry = originals[relative]
        destination = assets / entry["path"]
        if not destination.is_file() or hashlib.sha256(destination.read_bytes()).hexdigest() != entry["sha256"]:
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, destination)
    banks, decoded = {}, 0
    with ThreadPoolExecutor(max_workers=workers) as pool:
        futures = [pool.submit(export_bank, job, assets, decoder, previous_banks, force) for job in jobs]
        for index, future in enumerate(futures, 1):
            relative, entry, changed = future.result()
            banks[relative] = entry
            decoded += changed
            if index % 100 == 0 or index == len(jobs):
                print(f"[audio] {index}/{len(jobs)} banks prepared ({decoded} decoded)", flush=True)
    events = {}
    mapping = source / "fmodstudioexport/eventbankmapping.txt"
    if mapping.is_file():
        for line in mapping.read_text(encoding="utf-8-sig").splitlines():
            if not line.strip():
                continue
            event, bank = line.rsplit(";", 1)
            events.setdefault(event, [])
            if bank not in events[event]:
                events[event].append(bank)
    catalog = {"version": SCHEMA_VERSION, "files": originals, "banks": banks,
               "event_banks": events, "loose_audio": loose}
    write_json(output / "catalog.json", catalog)
    print(f"[audio] ready: {sum(len(b['samples']) for b in banks.values())} WAVs, "
          f"{len(events)} event mappings -> {output}", flush=True)
    return catalog


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, help="unpacked export_win32 tree")
    parser.add_argument("assets", type=Path, help="client asset output directory")
    parser.add_argument("--vgmstream", help="vgmstream-cli executable (or DSOR_VGMSTREAM)")
    parser.add_argument("--workers", type=int, default=4, help="parallel bank decoders (default: 4)")
    parser.add_argument("--force", action="store_true", help="decode unchanged banks again")
    parser.add_argument("--fallback-root", type=Path, default=os.environ.get("DSOR_AUDIO_FALLBACK_ROOT"),
                        help="older export_win32 tree to restore stripped banks with matching GUIDs "
                             "(or DSOR_AUDIO_FALLBACK_ROOT)")
    args = parser.parse_args()
    if args.workers < 1:
        parser.error("--workers must be positive")
    try:
        export_audio(args.root, args.assets, args.vgmstream, args.workers, args.force, args.fallback_root)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"audio export failed: {error}\n")


if __name__ == "__main__":
    main()
