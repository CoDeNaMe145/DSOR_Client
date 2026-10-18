"""Audio banks must export every variant and never accept partial decoding."""
from contextlib import redirect_stdout
import io
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import wave

from export_audio import bank_samples, export_audio, find_decoder, publish_wav


def pcm_fsb(count=2):
    # Tiny, valid PCM16 FSB5 with duplicate/path-like names. Sample data blocks
    # are aligned to 32 bytes, independently of the variable-length name table.
    headers = b"".join(struct.pack("<Q", (16 << 34) | (i << 7) | (8 << 1)) for i in range(count))
    names = b"".join(struct.pack("<I", count * 4 + i * 13) for i in range(count))
    names += b"../duplicate\0" * count
    audio = b"\x01\x00" * 16 * count
    header = b"FSB5" + struct.pack("<6I", 1, count, len(headers), len(names), len(audio), 2)
    return header.ljust(60, b"\0") + headers + names + audio


def chunk(tag, data):
    return tag + struct.pack("<I", len(data)) + data + b"\0" * (len(data) % 2)


def bank(*fsbs):
    fmt = chunk(b"FMT ", struct.pack("<II", 0x58, 0x56))
    # Odd-sized chunk before SNDH catches lost RIFF padding/alignment.
    prefix = b"PROJ" + chunk(b"JUNK", b"x")
    placeholder = b"\0" * (4 + 8 * len(fsbs)) if fsbs else b""
    offset = 12 + len(fmt) + len(chunk(b"LIST", prefix + chunk(b"SNDH", placeholder))) + 8
    entries = []
    for fsb in fsbs:
        entries.append(struct.pack("<II", offset, len(fsb)))
        offset += len(fsb)
    index = struct.pack("<I", 0x80003) + b"".join(entries) if fsbs else b""
    body = b"FEV " + fmt + chunk(b"LIST", prefix + chunk(b"SNDH", index))
    if fsbs:
        body += chunk(b"SND ", b"".join(fsbs))
    return b"RIFF" + struct.pack("<I", len(body)) + body


class BankIndexTests(unittest.TestCase):
    def test_counts_all_embedded_banks_and_odd_chunk_padding(self):
        self.assertEqual(bank_samples(bank(pcm_fsb(2), pcm_fsb(3))), 5)

    def test_event_only_is_distinct_from_truncated_or_unknown(self):
        self.assertEqual(bank_samples(bank()), 0)
        for data in (bank(pcm_fsb())[:-1], b"unknown", pcm_fsb()[:-1]):
            with self.subTest(data=data[:12]), self.assertRaises(ValueError):
                bank_samples(data)

    def test_rejects_out_of_bounds_sample_bank(self):
        data = bytearray(bank(pcm_fsb()))
        struct.pack_into("<I", data, data.index(b"SNDH") + 12, len(data) + 1)
        with self.assertRaises(ValueError):
            bank_samples(data)


class AudioExportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "input"
        self.assets = Path(self.temp.name) / "assets"
        self.source = self.root / "audio/fmodstudioexport/Desktop"
        self.source.mkdir(parents=True)
        (self.source / "empty.bank").write_bytes(bank())
        (self.source.parent / "eventbankmapping.txt").write_text(
            "skill/attack;skills\nskill/attack;variants\n", encoding="utf-8")

    def export(self):
        with redirect_stdout(io.StringIO()):
            return export_audio(self.root, self.assets)

    def require_decoder(self):
        try:
            find_decoder(None)
        except FileNotFoundError as error:
            self.skipTest(str(error))

    def test_metadata_only_pack_needs_no_decoder_and_retains_event_mapping(self):
        with patch("export_audio.find_decoder", side_effect=AssertionError("no decoder needed")):
            catalog = self.export()
        self.assertEqual(catalog["event_banks"]["skill/attack"], ["skills", "variants"])
        entry = catalog["banks"]["fmodstudioexport/Desktop/empty.bank"]
        self.assertEqual(entry["samples"], [])
        self.assertEqual((self.assets / entry["source"]).read_bytes(), bank())

    def test_real_decoder_preserves_duplicate_names_and_repairs_missing_output(self):
        self.require_decoder()
        (self.source / "skills.fsb").write_bytes(pcm_fsb())
        catalog = self.export()
        samples = catalog["banks"]["fmodstudioexport/Desktop/skills.fsb"]["samples"]
        self.assertEqual([s["name"] for s in samples], ["../duplicate", "../duplicate"])
        self.assertNotEqual(samples[0]["path"], samples[1]["path"])
        for sample in samples:
            path = self.assets / sample["path"]
            self.assertTrue(path.resolve().is_relative_to(self.assets.resolve()))
            with wave.open(str(path), "rb") as wav:
                self.assertEqual(wav.getframerate(), 44100)
                self.assertEqual(wav.getnframes(), 16)
                self.assertEqual(wav.readframes(16), b"\x01\x00" * 16)
        original_manifest = (self.assets / "audio/catalog.json").read_bytes()
        with patch("export_audio.subprocess.run", side_effect=AssertionError("unchanged audio decoded")):
            self.export()
        self.assertEqual((self.assets / "audio/catalog.json").read_bytes(), original_manifest)
        missing = self.assets / samples[1]["path"]
        missing.unlink()
        self.export()
        self.assertTrue(missing.is_file())

    def test_partial_decoder_success_does_not_publish_catalog(self):
        (self.source / "skills.fsb").write_bytes(pcm_fsb())
        with patch("export_audio.find_decoder", return_value="decoder"), patch(
                "export_audio.subprocess.run", return_value=subprocess.CompletedProcess([], 0, "{}\n", "bad subsong")):
            with self.assertRaisesRegex(RuntimeError, "decoded 1 of 2"):
                self.export()
        self.assertFalse((self.assets / "audio/catalog.json").exists())
        self.assertEqual(list((self.assets / "audio").glob(".decode-*")), [])

    def test_loop_points_are_retained_without_repeats_or_fades(self):
        self.require_decoder()
        frames = 176400
        header = b"FSB5" + struct.pack("<6I", 1, 1, 20, 0, frames * 2, 2)
        sample = struct.pack("<QIII", (frames << 34) | (8 << 1) | 1,
                             (3 << 25) | (8 << 1), 44100, 88200 - 1)
        (self.source / "loop.fsb").write_bytes(header.ljust(60, b"\0") + sample + b"\x01\x00" * frames)
        entry = self.export()["banks"]["fmodstudioexport/Desktop/loop.fsb"]["samples"][0]
        self.assertEqual(entry["frames"], frames)
        self.assertEqual(entry["loop"], {"start": 44100, "end": 88200})
        wav = self.assets / entry["path"]
        with wave.open(str(wav), "rb") as decoded:
            self.assertEqual(decoded.getnframes(), frames)
        self.assertIn(b"smpl", wav.read_bytes()[:112])

    def test_missing_input_and_overlapping_output_fail_before_writes(self):
        with self.assertRaises(FileNotFoundError):
            export_audio(self.root / "missing", self.assets)
        with self.assertRaises(ValueError):
            export_audio(self.root, self.root)
        self.assertFalse(self.assets.exists())

    @unittest.skipUnless(os.name == "nt", "Windows ACL regression")
    def test_published_wav_inherits_output_permissions_not_private_staging_acl(self):
        self.assets.mkdir()
        env = dict(os.environ, DSOR_AUDIO_TEST_DIRECTORY=str(self.assets))
        setup = """
$ErrorActionPreference = 'Stop'
$acl = Get-Acl -LiteralPath $env:DSOR_AUDIO_TEST_DIRECTORY
$sid = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-545')
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
    $sid, 'ReadAndExecute', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
$acl.AddAccessRule($rule)
Set-Acl -LiteralPath $env:DSOR_AUDIO_TEST_DIRECTORY -AclObject $acl
"""
        subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", setup],
                       env=env, check=True, capture_output=True, timeout=30)
        with tempfile.TemporaryDirectory(dir=self.assets) as private:
            source = Path(private) / "source.wav"
            source.write_bytes(b"sample bytes")
            destination = self.assets / "sample.wav"
            # Reproduce the old publication path, then replace the locked file.
            locked = Path(private) / "locked.wav"
            locked.write_bytes(b"old bytes")
            locked.replace(destination)
            check = """
$ErrorActionPreference = 'Stop'
$path = Join-Path $env:DSOR_AUDIO_TEST_DIRECTORY 'sample.wav'
$rules = (Get-Acl -LiteralPath $path).GetAccessRules(
    $true, $true, [System.Security.Principal.SecurityIdentifier])
@($rules | Where-Object { $_.IdentityReference.Value -eq 'S-1-5-32-545' -and
    $_.IsInherited -and $_.AccessControlType -eq 'Allow' }).Count
"""
            def inherited_user_rules():
                result = subprocess.run(
                    ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", check],
                    env=env, check=True, capture_output=True, text=True, timeout=30)
                return int(result.stdout.strip())
            self.assertEqual(inherited_user_rules(), 0)
            publish_wav(source, destination)
            self.assertGreater(inherited_user_rules(), 0)
            self.assertEqual(destination.read_bytes(), source.read_bytes())
            self.assertFalse(list(self.assets.glob("*.tmp")))


if __name__ == "__main__":
    unittest.main()
