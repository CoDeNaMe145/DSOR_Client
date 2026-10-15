"""Fixed game cameras, including the CharGen camera stored outside map DB4s."""
import math

CAMERA = 'camera'
SELECTION_ID = 'character-selection'
DB_CAMERAS = {'_Instance_Camera', '_Instance_CutsceneCamera', '_Instance_SequencerCamera'}


def is_camera(kind):
    return kind == CAMERA or kind in DB_CAMERAS


def selection_record(camera):
    from level_data import float_blob
    pitch, yaw = math.radians(camera['pitch']), math.radians(camera['yaw'])
    sx, cx, sy, cy = math.sin(pitch), math.cos(pitch), math.sin(yaw), math.cos(yaw)
    x, y, z = camera['offset']
    matrix = camera.get('matrix') or [cy, 0, -sy, 0, sy*sx, cx, cy*sx, 0,
                                     sy*cx, -sx, cy*cx, 0, x, y, -z, 1]
    return {'Id': 'Character selection camera', 'Transform': float_blob(matrix),
            'FieldOfView': camera['fov'], 'PreviewAspect': 16 / 9,
            'NearClip': camera.get('near', 0.1), 'FarClip': camera.get('far', 1000.0)}


def refresh_source(session):
    from level_data import read_json
    if session.name != 'a0000_char':
        return
    data = read_json(session.project.assets / 'characters/selection.json')
    if not data:
        return
    camera = data['camera']
    # A composed catalog carries its original camera, so it cannot become its
    # own baseline after a build or when the metadata cache is removed.
    camera = camera.get('_level_override', {}).get('base', camera)
    session.snapshot['selection_camera'] = camera
    session.snapshot['records'][CAMERA] = {SELECTION_ID: selection_record(camera)}
    session.sources[CAMERA] = 'available'


def lens(kind, row):
    if not is_camera(kind):
        return None
    if kind == CAMERA:
        return {'fov': row['FieldOfView'], 'aspect': row['PreviewAspect'], 'near': row['NearClip'], 'far': row['FarClip']}
    fov = float(row.get('CameraVertFieldOfView') or 0)
    if 0 < fov <= math.pi:
        fov = math.degrees(fov)
    return {'fov': fov or 34.0, 'aspect': float(row.get('CameraAspectRatio') or 16/9), 'near': 0.1, 'far': 1000.0}


def validate(row):
    from level_data import floats
    spec = lens(CAMERA, row)
    if not all(isinstance(v, (int, float)) and math.isfinite(v) for v in spec.values()):
        raise ValueError('Camera lens settings must be finite numbers')
    if not (1 <= spec['fov'] <= 170 and 0.25 <= spec['aspect'] <= 4 and 0 < spec['near'] < spec['far']):
        raise ValueError('Camera needs FOV 1–170°, aspect 0.25–4, and 0 < near clip < far clip')
    matrix = floats(row['Transform'])
    axes = [matrix[i:i+3] for i in (0, 4, 8)]
    if any(abs(sum(v*v for v in a)-1) > 0.002 for a in axes) or any(abs(sum(x*y for x,y in zip(axes[i],axes[j]))) > 0.002 for i,j in ((0,1),(0,2),(1,2))):
        raise ValueError('Cameras can move and rotate; their transform must not contain scale or shear')


def selection_export(session):
    """Patch the existing catalog without touching slots, hair or beard tables."""
    from level_data import read_json, floats
    data = read_json(session.project.assets / 'characters/selection.json')
    if data is None:
        raise ValueError('Restore characters/selection.json before building the fixed camera; camera edits are saved offline')
    base = session.snapshot.get('selection_camera')
    row = session.records.get(CAMERA, {}).get(SELECTION_ID)
    if base is None or row is None:
        raise ValueError('The character selection camera baseline is unavailable')
    validate(row)
    data['camera'] = {**base, 'matrix': floats(row['Transform']), 'fov': row['FieldOfView'],
                      'near': row['NearClip'], 'far': row['FarClip'], '_level_override': {'base': base}}
    return data
