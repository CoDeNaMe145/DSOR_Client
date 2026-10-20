"""Metadata tests use synthetic fixtures, never proprietary game files."""
import sqlite3
import struct
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

from audio_events import chunks, event_samples, resolve_events
from export_audio_runtime import cue, map_audio, export_runtime
from audio_animation import markers, animation_audio


def guid(value):
    return bytes([value]) * 16


class RuntimeTests(unittest.TestCase):
    def test_animation_marker_indices_survive_time_sorting_and_silent_hotspots(self):
        def clip(name, events, curves=0):
            data = struct.pack('<5H2BH', curves, 0, 86, 1, 40, 0, 1, len(events)) + name.encode().ljust(50, b'\0')
            for slot, key in events:
                data += b'impact'.ljust(47, b'\0') + str(slot).encode().ljust(15, b'\0') + struct.pack('<H', key)
            return data + bytes(curves * 24)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'anims/characters').mkdir(parents=True)
            (root / 'audio').mkdir()
            path = root / 'anims/characters/smith_animations.nax3'
            path.write_bytes(b'NA01' + struct.pack('<II', 2, 0) + clip('forge', [(0, 54), (1, 10)], 3)
                             + clip('run', [(2, 0)]))
            (root / 'audio/hotspots.xml').write_text('<Hotspots><Skin name="smith"><Anim name="forge">'
                '<Hotspot event="event:/sequences/blacksmith"/><Hotspot event=""/></Anim>'
                '<Anim name="run"><Hotspot event=""/><Hotspot event=""/><Hotspot event="event:/hoof"/></Anim></Skin></Hotspots>')
            (root / 'characters').mkdir()
            (root / 'characters/smith.sets.json').write_text('{"smith":{"skins":[]}}')
            result = animation_audio(root, root)
            self.assertEqual(result['models']['characters/smith']['forge'], [{'at': .4, 'slot': 1}, {'at': 2.16, 'slot': 0}])
            self.assertEqual(result['models']['characters/smith']['run'], [{'at': 0., 'slot': 2}])
            self.assertEqual(result['skins']['smith']['forge'], ['sequences/blacksmith', ''])
            self.assertEqual(result['fallbacks']['characters/smith'], {'run': ['', '', 'hoof']})
            path.write_bytes(path.read_bytes()[:-1])
            with self.assertRaisesRegex(ValueError, 'truncated animation event'):
                markers(path)

    def test_missing_map_input_does_not_replace_an_existing_runtime_catalog(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'audio').mkdir()
            runtime = path / 'audio/runtime.audio.json'
            runtime.write_text('existing')
            with self.assertRaisesRegex(ValueError, 'no map databases'):
                export_runtime(path / 'missing', path)
            self.assertEqual(runtime.read_text(), 'existing')

    def test_graph_follows_cross_bank_instruments_without_bus_leakage(self):
        event, timeline, instrument, wave, bus, unrelated = map(guid, range(1, 7))
        body = event + bytes(16) + timeline + bus + bytes(16) + struct.pack('<ii', 3, 64)
        sample = {'path': 'audio/example.wav', 'index': 1}
        # Bus GUID is in the event, but buses are deliberately not graph nodes.
        graph = ({event: body, timeline: timeline + instrument, instrument: instrument + wave},
                 {wave: sample, unrelated: {'path': 'wrong.wav', 'index': 2}},
                 {event: body}, {instrument: (-6., 0., -1)})
        result = resolve_events(graph, {event: 'event:/spell'})['spell']
        self.assertEqual(result['samples'], ['audio/example.wav'])
        self.assertTrue(result['looping'])
        self.assertEqual(result['max_voices'], 3)

    def test_riff_rejects_truncated_chunks(self):
        with self.assertRaisesRegex(ValueError, 'truncated'):
            list(chunks(b'RIFF' + bytes(8) + b'LIST' + struct.pack('<I', 100)))

    def test_map_defaults_positions_event_gates_and_disabled_emitters(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'map.db4'
            with closing(sqlite3.connect(path)) as db, db:
                db.execute('CREATE TABLE _Instance_Levels (DefaultMusic, Default2DAmbient, Default3DAmbient, ReverbPreset, _Layers)')
                db.execute("INSERT INTO _Instance_Levels VALUES ('event:/music/theme', '', '', 'cave', '')")
                db.execute('CREATE TABLE _Instance_EventSets (Id, EventFlags)')
                db.execute("INSERT INTO _Instance_EventSets VALUES ('7','festival=True;')")
                db.execute('CREATE TABLE _Instance_SoundObject (Transform, EventSetRow, _Layers, CueName, IsActive)')
                matrix = struct.pack('<16f', 1,0,0,0,0,1,0,0,0,0,1,0,10,20,30,1)
                db.execute('INSERT INTO _Instance_SoundObject VALUES (?,7,?, ?,0)', (matrix, '', '/environment/fire'))
            result = map_audio(path)
        self.assertEqual(result['music'], 'music/theme')
        self.assertEqual(result['event_sets'], {7: 'festival=True;'})
        self.assertEqual(result['emitters'][0]['matrix'][12:15], [10,20,30])
        self.assertFalse(result['emitters'][0]['active'])
        self.assertEqual(result['emitters'][0]['cue'], 'environment/fire')

    def test_cue_normalization_does_not_guess_legacy_names(self):
        self.assertEqual(cue(' event:/music/theme '), 'music/theme')
        self.assertEqual(cue('missing_typo'), 'missing_typo')
        self.assertEqual(cue(None), '')


if __name__ == '__main__':
    unittest.main()
