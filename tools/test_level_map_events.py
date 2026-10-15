"""Binary event layers, visibility masks and persistent editing of seasonal props."""
import copy
from pathlib import Path
import struct
import unittest

from level_data import GEOMETRY, atomic_json, geometry_records, moved
from level_events import MapEvents
from level_export import build
from level_map_events import with_event_layers
import test_level_editor as fixtures


def pack(fmt, *values):
    return struct.pack('<' + fmt, *values)


def instance(template=0, mapping=0, pos=(1, 2, 3)):
    return (pack('4f4f?', *pos, 1, 0, 0, 0, 1, False)
            + pack('??hhHH', True, True, template, -1, 0, mapping))


class EventLayerTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.LevelOverridesTest()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.project = self.fixture.project
        for row in self.fixture.manifest['instances']:
            for field in ('p', 'r', 's'):
                row[field] = [float(value) for value in row[field]]
        atomic_json(self.project.assets / 'maps/test.map.json', self.fixture.manifest)
        self.map = self.project.db_root / 'test.map'
        strings = ['', 'props/tree', 'effects/snow', 'christmas_2012', 'christmas_2012_silent', 'tree']
        encoded = ('\0'.join(strings) + '\0').encode()
        self.header = b'MOSD' + pack('I', 5)
        # Base row exactly matches the normal exporter's rounding/naming.
        base = instance(mapping=1)
        base = base[:-4] + pack('HH', 5, 1)
        raw = (self.header + b'IPAM' + bytes(54) + b'TRTS' + pack('II', len(strings), len(encoded)) + encoded
               + b'TTES' + pack('I', 0) + b'TEVE' + pack('III', 2, 3, 4)
               + b'LAME' + pack('I', 2) + pack('H', 2) + b'11' + pack('H', 2) + b'10'
               + b'LPMT' + pack('I', 2) + pack('H', 1) + bytes(44) + pack('H', 2) + bytes(44)
               + b'TSNI' + pack('I', 1) + base)
        self.map.write_bytes(raw)
        self.event = self.map.with_name('test_christmas_2012_silent.eve')
        self.event.write_bytes(self.header + b'TSNI' + pack('I', 1) + instance(1, pos=(4, 5, 6)))

    def test_reads_event_template_from_original_map_and_masks_base_geometry(self):
        manifest = with_event_layers(self.fixture.manifest, self.map)
        records = geometry_records(manifest)
        events = MapEvents({GEOMETRY: records}, manifest['event_rules'])
        tree_id = next(k for k, r in records.items() if r['model'] == 'props/tree')
        snow_id = next(k for k, r in records.items() if r['model'] == 'effects/snow')
        self.assertEqual(records[snow_id]['p'], [4, 5, 6])
        self.assertEqual(events.flags, ['christmas_2012_silent'])
        self.assertTrue(events.visible(GEOMETRY, records[tree_id], ident=tree_id))
        self.assertFalse(events.visible(GEOMETRY, records[snow_id]))
        active = events.preview_flags('christmas_2012')
        self.assertEqual(active, {'christmas_2012', 'christmas_2012_silent'})
        self.assertFalse(events.visible(GEOMETRY, records[tree_id], active, tree_id))
        self.assertTrue(events.visible(GEOMETRY, records[snow_id], active, snow_id))

    def test_event_edits_survive_rebuild_offline_and_repeat_export(self):
        session = self.fixture.session()
        ident, snow = next((k, r) for k, r in session.records[GEOMETRY].items() if r.get('event'))
        session.update(GEOMETRY, ident, moved(GEOMETRY, snow, [10, 20, 30]))
        session.save()
        build(session)
        self.assertEqual(len(session.manifest()['instances']), 2)
        self.assertEqual(len(session.manifest()['event_instances']), 1)
        self.assertEqual(self.fixture.session().records, session.records)
        # A converter rebuild with reordered model indexes keeps stable IDs.
        rebuilt = copy.deepcopy(self.fixture.manifest)
        rebuilt['models'].reverse()
        for row in rebuilt['instances']:
            row['m'] = 1 - row['m']
        atomic_json(self.project.assets / 'maps/test.map.json', rebuilt)
        self.map.rename(self.map.with_suffix('.unavailable'))
        self.event.rename(self.event.with_suffix('.unavailable'))
        reopened = self.fixture.session()
        self.assertFalse(reopened.conflicts)
        self.assertEqual(reopened.records[GEOMETRY][ident]['p'], [10, 20, 30])
        self.project.assets = self.project.root / 'absent'
        self.project.db_root = None
        offline = self.fixture.session()
        self.assertEqual(offline.records, reopened.records)
        self.assertFalse(offline.dirty)
        events = MapEvents(offline.records, offline.snapshot['manifest']['event_rules'])
        self.assertFalse(events.visible(GEOMETRY, offline.records[GEOMETRY][ident]))
        self.assertTrue(events.visible(GEOMETRY, offline.records[GEOMETRY][ident], events.preview_flags('christmas_2012')))

    def test_truncated_event_rejected_without_overwriting_cached_baseline(self):
        session = self.fixture.session()
        baseline = session.snapshot_path.read_bytes()
        self.event.write_bytes(self.header + b'TSNI' + pack('I', 1) + bytes(2))
        with self.assertRaisesRegex(ValueError, 'Truncated'):
            self.fixture.session()
        self.assertEqual(session.snapshot_path.read_bytes(), baseline)

    def test_cached_layer_survives_missing_eve_with_parent_map_still_present(self):
        session = self.fixture.session()
        self.event.rename(self.event.with_suffix('.unavailable'))
        self.assertEqual(self.fixture.session().records, session.records)


if __name__ == '__main__':
    unittest.main()
