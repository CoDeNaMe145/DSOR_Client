"""Lighting workspace and typed properties for the existing DB4 edit history."""
import tkinter as tk
from tkinter import colorchooser, ttk

from level_data import floats
from level_lighting import LIGHT, AMBIENCE, COLORS, BOOLEANS, LIGHT_TYPES, new_light


LIGHT_FIELDS = [
    ('Light', [('LightType', 'Type'), ('LightColor', 'Color · RGB'), ('LightIntensity', 'Intensity'),
               ('LightRange', 'Range (m)'), ('LightConeAngle', 'Spot cone (degrees)'), ('LightCastShadows', 'Cast shadows')]),
    ('Animation', [('LightFlickerEnable', 'Flicker'), ('LightFlickerFrequency', 'Flicker frequency'),
                   ('LightFlickerIntensity', 'Flicker depth (0–1)')]),
    ('Visibility', [('EventSetRow', 'Event set ID')]),
]
ENVIRONMENT_FIELDS = [
    ('Sun & ambient', [('LightColor', 'Sun color · RGB'), ('LightIntensity', 'Sun intensity'),
                      ('LightOppositeColor', 'Back light color · RGB'), ('LightAmbient', 'Ambient color · RGB')]),
    ('Fog', [('FogColor', 'Fog color · RGB'), ('FogIntensity', 'Fog intensity'),
             ('FogNearDist', 'Near distance (m)'), ('FogFarDist', 'Far distance (m)')]),
    ('Bloom & color', [('BloomScale', 'Bloom strength'), ('BloomColor', 'Bloom tint · RGB'),
                      ('BrightPassThreshold', 'Bloom threshold'), ('Saturation', 'Saturation')]),
    ('Volume', [('AmbienceBubbleEnabled', 'Enabled'), ('AmbienceBubblePriority', 'Priority'),
                ('PEShapeType', 'Shape'), ('PEFadeTime', 'Game transition time (s)'), ('EventSetRow', 'Event set ID')]),
]


def color_hex(channels):
    return '#' + ''.join(f'{round(max(0, min(1, c))*255):02x}' for c in channels[:3])


def properties(editor, kind, values):
    """Called inside the Details dock; values still commit through its inspector."""
    sections = LIGHT_FIELDS if kind == LIGHT else ENVIRONMENT_FIELDS
    for title, fields in sections:
        if title == 'Volume' and values.get('PEDefaultEntity'):
            continue
        fields = [(key, caption) for key, caption in fields if key in values]
        if not fields:
            continue
        ttk.Label(editor.property_form, text=title.upper(), style='Muted.TLabel', font=('Segoe UI', 9, 'bold')).pack(anchor='w', pady=(12, 5))
        for key, caption in fields:
            frame = ttk.Frame(editor.property_form, padding=(0, 3, 20, 3))
            frame.pack(fill='x')
            value = ', '.join(f'{v:.6g}' for v in floats(values[key])[:3]) if key in COLORS else str(values[key])
            if key == 'LightType':
                value = next((name for name, number in LIGHT_TYPES.items() if number == values[key]), value)
            variable = tk.StringVar(value=value)
            editor.property_fields[key] = variable
            editor.property_initial[key] = value
            if key in BOOLEANS:
                ttk.Checkbutton(frame, text=caption, variable=variable, onvalue='1', offvalue='0',
                                command=lambda: editor.action(editor.apply_properties)).pack(anchor='w')
                continue
            ttk.Label(frame, text=caption, style='Muted.TLabel').pack(anchor='w', pady=(0, 3))
            line = ttk.Frame(frame)
            line.pack(fill='x')
            if key in COLORS:
                swatch = tk.Button(line, width=3, height=1, relief='flat', bd=0, bg=color_hex(floats(values[key])), cursor='hand2')
                swatch.pack(side='left', padx=(0, 6))
                def choose(k=key, var=variable):
                    current = editor.session.records[kind][editor.selected[1]][k]
                    rgb, _ = colorchooser.askcolor(color_hex(floats(current)), title='Choose ' + k, parent=editor.window)
                    if rgb:
                        var.set(', '.join(f'{channel/255:.6g}' for channel in rgb))
                        editor.action(editor.apply_properties)
                swatch.configure(command=choose)
            if key in ('LightType', 'PEShapeType'):
                field = ttk.Combobox(line, textvariable=variable, state='readonly', values=list(LIGHT_TYPES) if key == 'LightType' else ['Box', 'Sphere'])
                field.bind('<<ComboboxSelected>>', lambda event: editor.action(editor.apply_properties))
            else:
                field = ttk.Entry(line, textvariable=variable)
                field.bind('<Return>', lambda event: editor.action(editor.apply_properties))
            field.pack(fill='x', expand=True)
    note = ('W moves the light; E aims a spotlight. Color swatches, checkboxes and Enter apply immediately. '
            'Intensity or range 0 turns a light off. Spot cone 0 keeps a legacy projected light disabled; '
            'set a cone to render a standard spotlight. Projected textures are not rendered.' if kind == LIGHT else
            'E rotates the sun direction. Selecting an ambience volume previews its lighting; '
            'R changes a local volume’s size. Changes apply to both the scene and fixed-camera preview.')
    ttk.Label(editor.property_form, text=note, style='Muted.TLabel', wraplength=280).pack(fill='x', padx=(0, 20), pady=12)


class LightingPanel:
    def __init__(self, editor, parent):
        self.editor, self.parent = editor, parent
        self.rows = []
        header = ttk.Frame(parent, padding=12)
        header.pack(fill='x')
        ttk.Label(header, text='Lighting', font=('Segoe UI', 12, 'bold')).pack(anchor='w')
        ttk.Label(header, text='Map lights and environment', style='Muted.TLabel').pack(anchor='w', pady=(4, 10))
        actions = ttk.Frame(header)
        actions.pack(fill='x')
        for caption, spot in [('+ Point', False), ('+ Spot', True)]:
            ttk.Button(actions, text=caption, style='Accent.TButton', command=lambda s=spot: editor.action(lambda: self.add(s))).pack(side='left', expand=True, fill='x', padx=2)
        ttk.Button(header, text='Select environment', style='Tool.TButton', command=lambda: editor.action(self.environment)).pack(fill='x', pady=(8, 0))
        self.note = ttk.Label(parent, style='Muted.TLabel', padding=(12, 8), wraplength=280)
        self.note.pack(side='bottom', fill='x')
        footer = ttk.Frame(parent, padding=8)
        footer.pack(side='bottom', fill='x')
        for text, action in [('Duplicate', editor.duplicate), ('Delete', editor.delete), ('Revert', editor.revert)]:
            ttk.Button(footer, text=text, style='Tool.TButton', command=lambda f=action: editor.action(f)).pack(side='left', expand=True, fill='x')
        frame = ttk.Frame(parent)
        frame.pack(fill='both', expand=True)
        self.tree = editor.tree(frame, ('name', 'state'), (185, 62))
        self.tree.configure(style='Outliner.Treeview', show='tree headings', height=4)
        self.tree.column('#0', width=48, minwidth=48, stretch=False)
        self.tree.heading('name', text='Light / environment')
        self.tree.heading('state', text='State')
        self.tree.tag_configure('inactive', foreground='#747781')
        self.tree.bind('<<TreeviewSelect>>', self.select)
        self.tree.bind('<Double-1>', lambda event: editor.focus())

    def refresh(self):
        from level_editor import label
        e = self.editor
        self.tree.delete(*self.tree.get_children())
        self.rows = []
        for kind in (AMBIENCE, LIGHT):
            for ident, row in e.session.records.get(kind, {}).items():
                self.rows.append((kind, ident))
                active = e.is_active(kind, row, ident)
                state = 'Hidden' if not active else 'Global' if row.get('PEDefaultEntity') else 'Base'
                if kind == LIGHT and (row.get('LightIntensity', 0) <= 0 or row.get('LightRange', 0) <= 0 or
                                      (row.get('LightType') == 1 and row.get('LightConeAngle', 0) <= 0)):
                    state = 'Off' if active else state
                self.tree.insert('', 'end', iid=str(len(self.rows)-1), values=(label(kind, row), state),
                                 image=e.previews.icon(kind), tags=() if active else ('inactive',))
        self.note.configure(text=f"{len(e.session.records.get(LIGHT, {}))} lights · {len(e.session.records.get(AMBIENCE, {}))} ambience records\nEvent visibility follows Map event state.")
        self.reveal()

    def reveal(self):
        if self.editor.selected in self.rows:
            iid = str(self.rows.index(self.editor.selected))
            if self.tree.selection() != (iid,):
                self.tree.selection_set(iid)
                self.tree.see(iid)
        elif self.tree.selection():
            self.tree.selection_remove(*self.tree.selection())

    def select(self, event=None):
        selection = self.tree.selection()
        if selection and int(selection[0]) < len(self.rows):
            key = self.rows[int(selection[0])]
            if key != self.editor.selected:
                self.editor.action(lambda: self.editor.set_selected(key))

    def environment(self):
        e = self.editor
        e.needs_session()
        rows = e.session.records.get(AMBIENCE, {})
        ident = next((i for i, row in rows.items() if row.get('PEDefaultEntity')), next(iter(rows), None))
        if ident is None:
            raise ValueError('No environment record is available. Restore the map DB4 or its metadata cache.')
        e.set_selected((AMBIENCE, ident))
        self.reveal()

    def add(self, spot=False):
        from level_assets import placement_position
        e = self.editor
        e.needs_session()
        e.commit_inspector()
        status = getattr(e.viewport, 'last_status', {}) or {}
        if status.get('revision') != e.viewport.revision:
            status = {}
        location = placement_position(status, [e.view[0], 0, e.view[1]], 0)
        location[1] += 3
        e.selected = (LIGHT, e.session.add(LIGHT, new_light(e.session, location, spot)))
        e.refresh()
        e.focus()
