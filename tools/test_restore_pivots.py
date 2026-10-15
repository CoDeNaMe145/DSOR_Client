import copy
import math
import unittest

from restore_pivots import node_paths, restore, rotate


def point_in_scene(doc, index, point):
    nodes = doc['nodes']
    parents = {c: i for i, node in enumerate(nodes) for c in node.get('children', [])}
    while index is not None:
        node = nodes[index]
        point = [x * s for x, s in zip(point, node.get('scale', [1, 1, 1]))]
        point = rotate(node.get('rotation', [0, 0, 0, 1]), point)
        point = [x + t for x, t in zip(point, node.get('translation', [0, 0, 0]))]
        index = parents.get(index)
    return point


def model():
    return {
        'scenes': [{'nodes': [0]}],
        'nodes': [
            {'name': 'model', 'children': [1]},
            {'name': 'glow', 'children': [2], 'scale': [1.5, 1, 1.5]},
            {'name': 'surface', 'mesh': 0, 'extras': {'dsor_pass': 'Additive'}},
        ],
        'animations': [{
            'channels': [{'target': {'node': 1, 'path': path}, 'sampler': i}
                         for i, path in enumerate(('translation', 'rotation', 'scale'))],
            'samplers': [{'input': 0, 'output': i + 1, 'interpolation': 'LINEAR'} for i in range(3)],
        }],
    }


class PivotTests(unittest.TestCase):
    def assertPoint(self, a, b):
        for x, y in zip(a, b):
            self.assertAlmostEqual(x, y, places=6)

    def test_pulsing_glow_center_stays_on_stone_between_keys(self):
        doc = model()
        pivot = (0.0, 2.0, -1.6)
        original_anim = copy.deepcopy(doc['animations'])
        surface = copy.deepcopy(doc['nodes'][2])
        # The old conversion demonstrably moves the center away from the stone.
        self.assertNotAlmostEqual(point_in_scene(doc, 2, pivot)[2], pivot[2])
        self.assertEqual(restore(doc, {('model', 'glow'): (pivot, pivot)}), 1)
        for frame in range(101):
            scale = 1.2 + 0.6 * frame / 100
            doc['nodes'][1]['scale'] = [scale, 1, scale]
            self.assertPoint(point_in_scene(doc, 2, pivot), pivot)
            # The authored expansion still happens, around that fixed center.
            edge = [pivot[0] + 0.5, pivot[1], pivot[2]]
            self.assertPoint(point_in_scene(doc, 2, edge), [scale * 0.5, 2, -1.6])
        self.assertEqual(doc['animations'], original_anim)
        self.assertEqual(doc['nodes'][2], surface)

    def test_translation_and_rotation_keep_authored_motion(self):
        doc = model()
        pivot = (2, 3, -4)
        restore(doc, {('model', 'glow'): (pivot, pivot)})
        doc['nodes'][1].update(translation=[5, 6, 7], rotation=[0, 0, math.sqrt(0.5), math.sqrt(0.5)], scale=[2, 3, 4])
        self.assertPoint(point_in_scene(doc, 2, pivot), [7, 9, 3])
        self.assertPoint(point_in_scene(doc, 2, [3, 3, -4]), [7, 11, 3])

    def test_different_scale_and_rotation_centers(self):
        doc = model()
        samplers = copy.deepcopy(doc['animations'][0]['samplers'])
        restore(doc, {('model', 'glow'): ((0, 0, 0), (2, 0, 0))})
        targets = {c['target']['path']: c['target']['node'] for c in doc['animations'][0]['channels']}
        self.assertEqual(targets['rotation'], 1)
        self.assertEqual(targets['translation'], 1)
        self.assertNotEqual(targets['scale'], 1)
        doc['nodes'][targets['scale']]['scale'] = [3, 2, 1]
        doc['nodes'][1]['rotation'] = [0, 0, math.sqrt(0.5), math.sqrt(0.5)]
        # Scale around x=2, then rotate about the origin: x=3 -> x=5 -> y=5.
        self.assertPoint(point_in_scene(doc, 2, [3, 0, 0]), [0, 5, 0])
        self.assertEqual(doc['animations'][0]['samplers'], samplers)

    def test_static_pivot_bakes_without_extra_nodes(self):
        doc = model()
        doc.pop('animations')
        doc['nodes'][1].update(translation=[1, 2, 3], rotation=[0, 0, 1, 0], scale=[-2, 3, 1])
        restore(doc, {('model', 'glow'): ((0, 0, 0), (2, 0, 0))})
        self.assertEqual(len(doc['nodes']), 3)
        # x=3 -> scale about x=2 to x=0 -> rotate -> translate.
        self.assertPoint(point_in_scene(doc, 2, [3, 0, 0]), [1, 2, 3])

    def test_nested_pivots_duplicate_names_and_idempotence(self):
        doc = model()
        doc['nodes'][0]['children'].append(3)
        doc['nodes'] += [{'name': 'other', 'children': [4]}, {'name': 'glow', 'children': [5]}, {'name': 'surface', 'mesh': 0}]
        paths_before = node_paths(doc['nodes'])
        pivots = {('model',): ((1, 0, 0), (1, 0, 0)),
                  ('model', 'glow'): ((0, 0, -2), (0, 0, -2))}
        doc['animations'][0]['channels'].append({'target': {'node': 0, 'path': 'scale'}, 'sampler': 2})
        restore(doc, pivots)
        self.assertEqual(node_paths(doc['nodes']), paths_before)
        self.assertPoint(point_in_scene(doc, 2, [0, 0, -2]), [0, 0, -2])
        self.assertEqual(doc['nodes'][4], {'name': 'glow', 'children': [5]})
        self.assertNotEqual(doc['scenes'][0]['nodes'], [0])
        saved = copy.deepcopy(doc)
        self.assertEqual(restore(doc, pivots), 0)
        self.assertEqual(doc, saved)

    def test_sprite_pivot_follows_runtime_rotation(self):
        doc = model()
        doc.pop('animations')
        doc['nodes'][1]['extras'] = {'dsor_sprite': True}
        pivot = (2, 0, 0)
        restore(doc, {('model', 'glow'): (pivot, pivot)})
        doc['nodes'][1]['rotation'] = [0, 0, 1, 0]
        self.assertPoint(point_in_scene(doc, 2, pivot), pivot)

    def test_zero_and_missing_pivots_do_not_change_model(self):
        doc = model()
        before = copy.deepcopy(doc)
        self.assertEqual(restore(doc, {('model', 'glow'): ((0, 0, 0), (0, 0, 0)),
                                       ('missing',): ((1, 2, 3), (1, 2, 3))}), 0)
        self.assertEqual(doc, before)


if __name__ == '__main__':
    unittest.main()
