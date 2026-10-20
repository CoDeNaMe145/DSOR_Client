"""Read FMOD event references from the original RIFF banks.

Format research: https://github.com/Masusder/FModBankParser (Apache-2.0).
Only event/timeline/instrument edges are followed: buses are shared by unrelated
events and must never be used to choose samples. DSP/parameter automation is
retained in the original banks, not reconstructed by this reader.
"""
import struct


def chunks(data, start=12, end=None):
    end = len(data) if end is None else end
    while start + 8 <= end:
        tag, size = struct.unpack_from('<4sI', data, start)
        at = start + 8
        if at + size > end:
            raise ValueError('truncated FMOD metadata')
        if tag == b'LIST':
            yield data[at:at + 4], list(chunks(data, at + 4, at + size))
        else:
            yield tag, data[at:at + size]
        start = at + size + size % 2


def flat(tree):
    for tag, data in tree:
        if isinstance(data, list):
            yield from flat(data)
        else:
            yield tag, data


class Reader:
    def __init__(self, data):
        self.data, self.at = data, 0

    def take(self, size):
        result = self.data[self.at:self.at + size]
        self.at += size
        if len(result) != size:
            raise ValueError('truncated FMOD string table')
        return result

    def count(self):
        low, = struct.unpack('<H', self.take(2))
        return (low & 0x7fff) | (struct.unpack('<H', self.take(2))[0] << 15) if low & 0x8000 else low

    def elements(self, size):
        count = self.count() >> 1
        if count:
            self.take(2)
        return [self.take(size) for _ in range(count)]


def strings(data):
    r = Reader(data)
    if r.take(4) != b'\1\0\0\0':
        raise ValueError('unsupported FMOD string table')
    nodes = [struct.unpack('<II', n) for n in r.elements(8)]
    guids = r.elements(16)
    blob = r.take(r.count())
    leaves = [int.from_bytes(r.take(3), 'little') for _ in range(r.count())]
    parents = [int.from_bytes(r.take(3), 'little') for _ in range(r.count())]
    if len(leaves) != len(guids) or len(parents) != len(nodes):
        raise ValueError('invalid FMOD string-tree dimensions')
    result = {}
    for guid, node in zip(guids, leaves):
        parts, visited = [], set()
        while node != 0xffffff:
            if node in visited or node >= len(nodes):
                raise ValueError('invalid FMOD string-tree reference')
            visited.add(node)
            offset = nodes[node][0] & 0xffffff
            if offset != 0xffffff:
                parts.append(blob[offset:blob.index(b'\0', offset)].decode('utf-8'))
            node = parents[node]
        result[guid] = ''.join(reversed(parts))
    return result


BODY = {b'EVNT': b'EVTB', b'TMLN': b'TLNB', b'PMLO': b'PMLB',
        b'WAIT': b'WAIB', b'MUIT': b'MUIB', b'SPIT': b'SPIB', b'EVIT': b'EVIB', b'TRAN': b'TRNB'}


def read_bank(data, samples, graph, counts=None):
    tree = list(chunks(data, 12, struct.unpack_from('<I', data, 4)[0] + 8))
    nodes, waves, events, instruments = graph
    if counts is None:
        counts = []
        for tag, payload in flat(tree):
            if tag == b'SNDH' and payload:
                for at in range(4, len(payload), 8):
                    offset = struct.unpack_from('<I', payload, at)[0]
                    counts.append(struct.unpack_from('<I', data, offset + 8)[0])
    def visit(items):
        for tag, payload in items:
            if isinstance(payload, list):
                body = next((p for t, p in payload if t == BODY.get(tag)), None)
                if body is not None:
                    guid = body[:16]
                    nodes[guid] = b''.join(p for t, p in payload if isinstance(p, bytes)
                                          and (t == BODY[tag] or t in (b'INST', b'PLST', b'TRTL')))
                    if tag == b'EVNT': events[guid] = body
                    inst = next((p for t, p in payload if t == b'INST'), None)
                    if inst and len(inst) >= 28:
                        instruments[guid] = struct.unpack_from('<ffi', inst, 16)
                visit(payload)
            elif tag == b'WAV ':
                bank, index = struct.unpack_from('<II', payload, 18)
                global_index = sum(counts[:bank]) + index
                if bank >= len(counts) or global_index >= len(samples):
                    raise ValueError('FMOD waveform refers outside the extracted bank')
                waves[payload[:16]] = samples[global_index]
    visit(tree)
def resolve_events(graph, names):
    nodes, waves, events, instruments = graph
    keys = set(nodes) | set(waves)
    edges = {guid: {payload[i:i + 16] for i in range(max(0, len(payload) - 15))
                    if payload[i:i + 16] in keys and payload[i:i + 16] != guid}
             for guid, payload in nodes.items()}
    result = {}
    for guid, body in events.items():
        name = names.get(guid, '').removeprefix('event:/')
        if not name: continue
        pending, visited, found = [guid], set(), {}
        looping = False
        while pending:
            key = pending.pop()
            if key in visited: continue
            visited.add(key)
            if key in waves:
                found[waves[key]['path']] = waves[key]
            if key in instruments:
                looping |= instruments[key][2] < 0
            pending.extend(edges.get(key, ()))
        if found:
            polyphony, priority = struct.unpack_from('<ii', body, 80)
            result[name] = {'samples': [s['path'] for s in sorted(found.values(), key=lambda s: s['index'])],
                            'looping': looping, 'max_voices': max(1, min(polyphony if polyphony > 0 else 4, 8)),
                            'priority': max(0, min(priority, 255))}
    return result


def event_samples(data, samples, names):
    graph = ({}, {}, {}, {})
    read_bank(data, samples, graph)
    return resolve_events(graph, names)
