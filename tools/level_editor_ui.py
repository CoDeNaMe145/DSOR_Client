"""Desktop editor shell: compact chrome, resizable docks, focused properties."""
import tkinter as tk
from tkinter import ttk

BG = "#18191c"
PANEL = "#232427"
FIELD = "#191a1d"
TEXT = "#e3e4e8"
MUTED = "#94979f"
BLUE = "#3794ff"


def theme(window):
    window.configure(bg=BG)
    window.option_add("*TCombobox*Listbox.background", FIELD)
    window.option_add("*TCombobox*Listbox.foreground", TEXT)
    window.option_add("*TCombobox*Listbox.selectBackground", "#274f79")
    style = ttk.Style(window)
    style.theme_use("clam")
    style.configure(".", background=PANEL, foreground=TEXT, font=("Segoe UI", 10),
                    fieldbackground=FIELD, bordercolor="#34363b", lightcolor=PANEL, darkcolor=PANEL,
                    troughcolor=BG, focuscolor=PANEL)
    style.configure("TFrame", background=PANEL)
    style.configure("Chrome.TFrame", background=BG)
    style.configure("TLabel", padding=0)
    style.configure("Muted.TLabel", foreground=MUTED)
    style.configure("Title.TLabel", font=("Segoe UI", 14, "bold"))
    style.configure("Section.TLabel", foreground="#bcbfc7", font=("Segoe UI", 9, "bold"), padding=(0, 8))
    style.configure("TButton", background="#303136", borderwidth=0, padding=(10, 6), relief="flat")
    style.map("TButton", background=[("pressed", "#404249"), ("active", "#3b3e44")], foreground=[("disabled", "#62656e")])
    style.configure("Accent.TButton", background="#2469ac", foreground="white")
    style.map("Accent.TButton", background=[("active", "#3188df"), ("pressed", "#1d5892")])
    style.configure("Tool.TButton", background=PANEL, padding=(8, 5))
    style.configure("Active.Tool.TButton", background="#284d76", foreground="#dcebff")
    style.map("Active.Tool.TButton", background=[("active", "#326495")])
    style.configure("TEntry", padding=(6, 6), borderwidth=1, insertcolor=TEXT)
    style.configure("TCombobox", padding=(6, 5), arrowsize=13)
    style.map("TCombobox", fieldbackground=[("readonly", FIELD)], foreground=[("readonly", TEXT)],
              selectbackground=[("readonly", FIELD)], selectforeground=[("readonly", TEXT)])
    style.configure("TCheckbutton", padding=(3, 4))
    style.map("TCheckbutton", background=[("active", PANEL)])
    style.configure("TNotebook", borderwidth=0, tabmargins=0)
    style.configure("TNotebook.Tab", background=BG, foreground=MUTED, padding=(13, 8), borderwidth=0)
    style.map("TNotebook.Tab", background=[("selected", PANEL)], foreground=[("selected", TEXT)])
    style.configure("Treeview", rowheight=30, background=FIELD, fieldbackground=FIELD, borderwidth=0)
    style.configure("Preview.Treeview", rowheight=72)
    style.configure("Outliner.Treeview", rowheight=44)
    style.configure("Treeview.Heading", background="#292b2f", foreground=MUTED, font=("Segoe UI", 9), padding=(6, 7), relief="flat")
    style.map("Treeview", background=[("selected", "#264e78")], foreground=[("selected", "white")])
    style.configure("Vertical.TScrollbar", background="#41434a", arrowcolor=MUTED, borderwidth=0, arrowsize=11)
    style.configure("Horizontal.TScrollbar", background="#41434a", arrowcolor=MUTED, borderwidth=0, arrowsize=11)
    style.configure("TPanedwindow", background=BG)
    style.configure("Sash", sashthickness=5, gripcount=0)


def section(parent, title):
    ttk.Label(parent, text=title.upper(), style="Section.TLabel").pack(fill="x")


def setup(editor):
    e, w = editor, editor.window
    w.title("DSOR · Level Editor")
    w.geometry("1600x980")
    w.minsize(1180, 760)
    try:
        w.state("zoomed")
    except tk.TclError:
        try:
            w.attributes("-zoomed", True)
        except tk.TclError:
            w.geometry(f"{w.winfo_screenwidth()}x{w.winfo_screenheight()}+0+0")
    theme(w)

    def button(parent, caption, command, style="TButton", **pack):
        b = ttk.Button(parent, text=caption, style=style, command=lambda: e.action(command))
        b.pack(side="left", padx=2, **pack)
        return b

    header = tk.Frame(w, bg=BG, padx=12, pady=7)
    header.pack(fill="x")
    tk.Label(header, text="D", bg=BLUE, fg="white", font=("Segoe UI", 12, "bold"), padx=8, pady=2).pack(side="left", padx=(0, 10))
    tk.Label(header, text="DSOR", bg=BG, fg=TEXT, font=("Segoe UI", 11, "bold")).pack(side="left", padx=(0, 18))
    for name, actions in [("File", [("Open map", e.open_map), ("Sources…", e.sources), ("Save overrides    Ctrl+S", e.save), ("Build client + DB4", e.build)]),
                          ("Edit", [("Undo    Ctrl+Z", e.undo), ("Redo    Ctrl+Y", e.redo), ("Duplicate", e.duplicate), ("Delete", e.delete), ("Revert override", e.revert)]),
                          ("View", [("Frame map", e.fit), ("Focus selection    F", e.focus)])]:
        menu = tk.Menubutton(header, text=name, bg=BG, fg=MUTED, activebackground=PANEL, activeforeground=TEXT, font=("Segoe UI", 10), padx=9)
        dropdown = tk.Menu(menu, tearoff=False, bg=PANEL, fg=TEXT, activebackground="#264e78", activeforeground="white", borderwidth=0)
        for caption, fn in actions:
            dropdown.add_command(label=caption, command=lambda f=fn: e.action(f))
        menu.configure(menu=dropdown)
        menu.pack(side="left")
    tk.Label(header, text="LEVEL EDITOR", bg=BG, fg="#737781", font=("Segoe UI", 9)).pack(side="right")

    toolbar = ttk.Frame(w, padding=(10, 8))
    toolbar.pack(fill="x", pady=(0, 5))
    ttk.Label(toolbar, text="LEVEL", style="Muted.TLabel").pack(side="left", padx=(3, 8))
    e.map_box = ttk.Combobox(toolbar, textvariable=e.map_var, width=27, values=e.project.maps())
    e.map_box.pack(side="left", padx=(0, 4))
    e.map_box.bind("<Return>", lambda event: e.action(e.open_map))
    button(toolbar, "Open", e.open_map)
    ttk.Separator(toolbar, orient="vertical").pack(side="left", fill="y", padx=12)
    button(toolbar, "Save", e.save)
    button(toolbar, "Build", e.build)
    button(toolbar, "▶  Play", e.preview, "Accent.TButton")
    e.saved_label = ttk.Label(toolbar, foreground="#83c5a4")
    e.saved_label.pack(side="right", padx=10)

    footer = tk.Frame(w, bg=BG, padx=12, pady=6)
    footer.pack(side="bottom", fill="x")
    e.source_label = tk.Label(footer, text="", bg=BG, fg=MUTED, font=("Segoe UI", 9))
    e.source_label.pack(side="right", padx=(12, 0))
    tk.Label(footer, textvariable=e.status, bg=BG, fg=MUTED, anchor="w", font=("Segoe UI", 9)).pack(fill="x")

    panes = ttk.Panedwindow(w, orient="horizontal")
    panes.pack(fill="both", expand=True, padx=6)
    left = ttk.Frame(panes, width=320)
    panes.add(left, weight=0)
    e.content_tabs = ttk.Notebook(left)
    e.content_tabs.pack(fill="both", expand=True)
    objects, assets, lighting, changes = (ttk.Frame(e.content_tabs) for _ in range(4))
    e.content_tabs.add(objects, text="Objects")
    e.content_tabs.add(assets, text="Assets")
    e.content_tabs.add(lighting, text="Lighting")
    e.content_tabs.add(changes, text="Changes")
    from level_assets import AssetBrowser
    e.asset_browser = AssetBrowser(e, assets)
    from level_lighting_ui import LightingPanel
    e.lighting_panel = LightingPanel(e, lighting)
    def content_changed(event):
        if e.content_tabs.select() == str(e.asset_browser.window):
            e.asset_browser.ensure_source()
        if e.viewport and e.session:
            e.viewport.controls()
    e.content_tabs.bind('<<NotebookTabChanged>>', content_changed)
    header = ttk.Frame(objects, padding=(12, 10))
    header.pack(fill="x")
    ttk.Label(header, text="Outliner", font=("Segoe UI", 11, "bold")).pack(side="left")
    button(header, "+ Add", e.add, "Accent.TButton", fill="x")
    filters = ttk.Frame(objects, padding=(10, 0, 10, 8))
    filters.pack(fill="x")
    ttk.Entry(filters, textvariable=e.search_var).pack(fill="x", pady=(0, 5))
    ttk.Label(filters, text="Search objects, models or GUIDs", style="Muted.TLabel", font=("Segoe UI", 9)).pack(anchor="w", pady=(0, 7))
    e.kind_box = ttk.Combobox(filters, textvariable=e.kind_var, state="readonly")
    e.kind_box.pack(fill="x")
    e.kind_box.bind("<<ComboboxSelected>>", lambda event: e.refresh_list())
    ttk.Checkbutton(filters, text="Show inactive event records", variable=e.show_events, command=e.refresh_list).pack(anchor="w", pady=(4, 0))
    actions = ttk.Frame(objects, padding=8)
    actions.pack(side="bottom", fill="x")
    for name, fn in [("Duplicate", e.duplicate), ("Delete", e.delete), ("Revert", e.revert)]:
        button(actions, name, fn, "Tool.TButton", fill="x", expand=True)
    e.object_count = ttk.Label(objects, style="Muted.TLabel", padding=(12, 6))
    e.object_count.pack(side="bottom", fill="x")
    listing = ttk.Frame(objects)
    listing.pack(fill="both", expand=True)
    e.objects = e.tree(listing, ("name", "state"), (185, 62))
    e.objects.configure(show="tree headings", style="Outliner.Treeview", height=4)
    e.objects.column("#0", width=44, minwidth=44, stretch=False)
    e.objects.heading("#0", text="")
    e.objects.heading("name", text="Object")
    e.objects.heading("state", text="Layer")
    e.objects.tag_configure("inactive", foreground="#747781")
    e.objects.bind("<<TreeviewSelect>>", e.select_list)
    e.objects.bind("<Double-1>", lambda event: e.focus())
    ttk.Label(changes, text="Authored changes", font=("Segoe UI", 11, "bold"), padding=(12, 12)).pack(fill="x")
    change_actions = ttk.Frame(changes, padding=8)
    change_actions.pack(side="bottom", fill="x")
    for name, fn in [("Undo", e.undo), ("Redo", e.redo), ("Revert", e.revert)]:
        button(change_actions, name, fn, "Tool.TButton", fill="x", expand=True)
    change_listing = ttk.Frame(changes)
    change_listing.pack(fill="both", expand=True)
    e.changes = e.tree(change_listing, ("name", "state"), (185, 62))
    e.changes.configure(height=4)
    e.changes.heading("name", text="Object")
    e.changes.heading("state", text="Change")
    e.changes.bind("<<TreeviewSelect>>", e.select_change)

    center = ttk.Frame(panes)
    panes.add(center, weight=1)
    eventbar = ttk.Frame(center, padding=(10, 8))
    eventbar.pack(fill="x")
    ttk.Label(eventbar, text="MAP EVENT STATE", style="Muted.TLabel", font=("Segoe UI", 9, "bold")).pack(side="left", padx=(0, 10))
    e.camera_button = ttk.Button(eventbar, text='Camera', style='Tool.TButton', command=lambda: e.action(e.select_camera))
    e.camera_button.pack(side='right', padx=(10, 0))
    e.event_box = ttk.Combobox(eventbar, textvariable=e.event_var, state="readonly", width=38)
    e.event_box.pack(side="left", fill="x", expand=True)
    e.event_box.bind("<<ComboboxSelected>>", lambda event: e.action(e.change_event))
    e.event_note = ttk.Label(center, style="Muted.TLabel", padding=(10, 0, 10, 7), font=("Segoe UI", 9))
    e.event_note.pack(fill="x")
    controls = ttk.Frame(center, padding=(5, 5))
    controls.pack(fill="x")
    e.mode_buttons = {}
    for caption, mode in [("Select  Q", "select"), ("Move  W", "translate"), ("Rotate  E", "rotate"), ("Scale  R", "scale")]:
        e.mode_buttons[mode] = button(controls, caption, lambda m=mode: e.viewport.set_mode(m), "Active.Tool.TButton" if mode == "translate" else "Tool.TButton")
    ttk.Separator(controls, orient="vertical").pack(side="left", fill="y", padx=7)
    button(controls, "Frame map", e.fit, "Tool.TButton")
    button(controls, "Focus  F", e.focus, "Tool.TButton")
    settings = ttk.Frame(center, padding=(9, 2, 9, 7))
    settings.pack(fill="x")
    ttk.Label(settings, text="Snap", style="Muted.TLabel").pack(side="left", padx=(0, 5))
    ttk.Entry(settings, textvariable=e.snap, width=4).pack(side="left")
    ttk.Label(settings, text="Fly speed", style="Muted.TLabel").pack(side="left", padx=(14, 5))
    speed = ttk.Combobox(settings, textvariable=e.camera_speed, values=[2, 5, 15, 30, 60, 120], width=5, state="readonly")
    speed.pack(side="left")
    speed.bind("<<ComboboxSelected>>", lambda event: e.viewport.controls())
    ttk.Checkbutton(settings, text="Grid", variable=e.show_grid, command=lambda: e.viewport.controls()).pack(side="right")
    ttk.Checkbutton(settings, text="Helpers", variable=e.show_helpers, command=lambda: e.viewport.controls()).pack(side="right")
    ttk.Label(center, text="RMB look + WASD fly  ·  Q / E height  ·  Shift boost  ·  MMB pan  ·  Wheel move", style="Muted.TLabel",
              padding=(10, 8), font=("Segoe UI", 9)).pack(side="bottom", fill="x")
    ttk.Label(center, textvariable=e.viewport_status, style="Muted.TLabel", padding=(10, 5), font=("Segoe UI", 9)).pack(side="bottom", fill="x")
    e.views = ttk.Notebook(center)
    e.views.pack(fill="both", expand=True)
    e.viewport_host = tk.Frame(e.views, background="#101318")
    overview = ttk.Frame(e.views)
    e.views.add(e.viewport_host, text="Perspective · Free fly")
    e.views.add(overview, text="Map overview")
    e.canvas = tk.Canvas(overview, background="#191c21", highlightthickness=0)
    e.canvas.pack(fill="both", expand=True)
    e.canvas.bind("<Configure>", lambda event: e.draw())
    e.canvas.bind("<ButtonPress-1>", e.map_press)
    e.canvas.bind("<B1-Motion>", e.map_drag)
    e.canvas.bind("<ButtonRelease-1>", e.map_release)
    e.canvas.bind("<ButtonPress-3>", lambda event: setattr(e, "pan", (event.x, event.y, *e.view[:2])))
    e.canvas.bind("<B3-Motion>", e.pan_map)
    e.canvas.bind("<MouseWheel>", e.zoom)
    e.canvas.bind("<Button-4>", lambda event: e.zoom(event, 1))
    e.canvas.bind("<Button-5>", lambda event: e.zoom(event, -1))

    right = ttk.Frame(panes, width=350, padding=(10, 0))
    panes.add(right, weight=0)
    ttk.Label(right, text="Details", font=("Segoe UI", 11, "bold"), padding=(2, 12)).pack(anchor="w")
    e.selection_name = ttk.Label(right, text="Nothing selected", font=("Segoe UI", 12, "bold"), wraplength=320)
    e.selection_name.pack(fill="x", pady=(2, 3))
    ttk.Label(right, textvariable=e.detail, wraplength=320, style="Muted.TLabel", font=("Segoe UI", 9)).pack(fill="x", pady=(0, 8))
    e.detail_tabs = ttk.Notebook(right)
    e.detail_tabs.pack(fill="both", expand=True)
    properties, raw = ttk.Frame(e.detail_tabs, padding=8), ttk.Frame(e.detail_tabs)
    e.detail_tabs.add(properties, text="Properties")
    e.detail_tabs.add(raw, text="DB4 / JSON")
    preview = ttk.Frame(properties, height=136)
    preview.pack(fill="x", pady=(4, 4))
    preview.pack_propagate(False)
    e.object_preview = ttk.Label(preview, text="Select an object to preview", anchor="center", compound="top")
    e.object_preview.pack(fill="both", expand=True)
    section(properties, "Transform")
    transform = ttk.Frame(properties)
    transform.pack(fill="x")
    for col, (axis, variable, tint) in enumerate(zip("XYZ", e.xyz, ("#e77c79", "#87c99a", "#7ca9e8"))):
        transform.columnconfigure(col, weight=1)
        field = ttk.Frame(transform)
        field.grid(row=0, column=col, sticky="ew", padx=2)
        ttk.Label(field, text=axis, foreground=tint).pack(side="left", padx=(0, 4))
        entry = ttk.Entry(field, textvariable=variable, width=7)
        entry.pack(fill="x", expand=True)
        entry.bind("<Return>", lambda event: e.action(e.apply_position))
    ttk.Button(properties, text="Apply position", command=lambda: e.action(e.apply_position)).pack(fill="x", pady=6)
    section(properties, "Object properties")
    e.property_fields = {}
    e.property_initial = {}
    form_canvas = tk.Canvas(properties, bg=PANEL, highlightthickness=0, height=170)
    form_canvas.pack(fill="both", expand=True)
    e.property_form = ttk.Frame(form_canvas)
    form_id = form_canvas.create_window(0, 0, window=e.property_form, anchor="nw")
    e.property_form.bind("<Configure>", lambda event: form_canvas.configure(scrollregion=form_canvas.bbox("all")))
    form_canvas.bind("<Configure>", lambda event: form_canvas.itemconfigure(form_id, width=event.width))
    form_canvas.bind("<MouseWheel>", lambda event: form_canvas.yview_scroll(-event.delta // 120, "units"))
    # Properties use a vertical scrollbar beside their canvas, within a stable dock.
    property_scroll = ttk.Scrollbar(form_canvas, orient="vertical", command=form_canvas.yview)
    property_scroll.place(relx=1, x=0, y=0, relheight=1, anchor="ne")
    form_canvas.configure(yscrollcommand=property_scroll.set)
    ttk.Button(properties, text="Apply properties", style="Accent.TButton", command=lambda: e.action(e.apply_properties)).pack(fill="x", pady=(8, 4))
    e.event_detail = ttk.Label(properties, wraplength=300, style="Muted.TLabel", font=("Segoe UI", 9))
    e.event_detail.pack(fill="x", pady=6)
    ttk.Label(raw, text="Exact database fields · editable JSON", style="Muted.TLabel", padding=(8, 10)).pack(fill="x")
    editor_frame = ttk.Frame(raw)
    editor_frame.pack(fill="both", expand=True)
    e.inspector = tk.Text(editor_frame, width=35, height=8, wrap="none", undo=True, bg=FIELD, fg=TEXT,
                          insertbackground=BLUE, selectbackground="#264e78", font=("Consolas", 10), borderwidth=0, padx=10, pady=10)
    scroll = ttk.Scrollbar(editor_frame, command=e.inspector.yview)
    e.inspector.configure(yscrollcommand=scroll.set)
    scroll.pack(side="right", fill="y")
    e.inspector.pack(fill="both", expand=True)
    horizontal = ttk.Scrollbar(raw, orient="horizontal", command=e.inspector.xview)
    horizontal.pack(fill="x")
    e.inspector.configure(xscrollcommand=horizontal.set)
    ttk.Button(raw, text="Apply fields", style="Accent.TButton", command=lambda: e.action(e.apply_fields)).pack(fill="x", pady=8)
    e.conflict_button = ttk.Button(right, text="Resolve conflict: keep override", command=lambda: e.action(e.keep_override))
    e.conflict_button.pack(fill="x", pady=8)
    e.conflict_button.pack_forget()
    # Set dock widths after the maximized window has its actual dimensions.
    def size_docks():
        w.update_idletasks()
        total = panes.winfo_width()
        if total < 800:
            w.after(100, size_docks)
            return
        panes.sashpos(0, min(330, int(total * .23)))
        panes.sashpos(1, total - min(370, int(total * .27)))
    w.after(150, size_docks)
