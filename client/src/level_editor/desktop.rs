//! Native immediate-mode editor shell. No subprocess owns authored state.
mod browser;
mod inspector;
mod lighting;
mod markers;
mod overview;
use super::*;
use bevy_egui::{
    egui, EguiPlugin, EguiPrimaryContextPass, EguiTextureHandle, EguiUserTextures,
    PrimaryEguiContext,
};
use dsor_level_data::{self as data, scene, Project, Result as DataResult, Session};
use serde_json::Value;

#[derive(Clone)]
struct Row {
    key: String,
    kind: String,
    id: String,
    label: String,
    model: String,
    search: String,
    active: bool,
    appearance: Value,
}
#[derive(Resource)]
pub(super) struct State {
    pub(super) project: Project,
    session: Option<Session>,
    maps: Vec<String>,
    map: String,
    map_search: String,
    selected: Option<String>,
    draft: Value,
    original: Value,
    raw: String,
    raw_changed: bool,
    rows: Vec<Row>,
    tab: usize,
    search: String,
    table: String,
    event: String,
    events: data::events::MapEvents,
    catalogs: scene::Catalogs,
    revision: u64,
    publish: bool,
    message: String,
    error: bool,
    assets: Vec<String>,
    asset_search: String,
    asset_folder: String,
    asset_selected: String,
    thumbs: browser::Thumbnails,
    sources_open: bool,
    sources_assets: String,
    sources_db: String,
    sources_overrides: String,
    pending: Option<Action>,
    overview: overview::Overview,
    show_overview: bool,
    show_markers: bool,
    add_open: bool,
    add_kind: String,
    add_json: String,
}
#[derive(Clone)]
enum Action {
    Open(String),
    Save,
    Build,
    Play,
    Undo,
    Redo,
    Select(String),
    Apply,
    Duplicate,
    Delete,
    Revert(String),
    Keep(String),
    AddAsset(String),
    AddLight(bool),
    AddRecord,
    Quit,
    Restart,
}
impl State {
    pub fn from_args() -> DataResult<Self> {
        let root = crate::install_root();
        let mut p = Project::load(&root)?;
        let mut map = None;
        let mut args = std::env::args().skip_while(|a| a != "--editor").skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--assets" => p.assets = PathBuf::from(args.next().ok_or("--assets needs a path")?),
                "--db-root" => {
                    p.db_root = Some(PathBuf::from(args.next().ok_or("--db-root needs a path")?))
                }
                "--overrides" => {
                    p.overrides = PathBuf::from(args.next().ok_or("--overrides needs a path")?)
                }
                "--cache" => p.cache = PathBuf::from(args.next().ok_or("--cache needs a path")?),
                "--help" => {
                    println!("dsor-client --editor [map] [--assets DIR] [--db-root DIR] [--overrides DIR] [--cache DIR]");
                    std::process::exit(0);
                }
                s if !s.starts_with('-') => map = Some(s.to_owned()),
                _ => return Err(format!("Unknown editor option: {a}").into()),
            }
        }
        if let Ok(v) = std::env::var("DSOR_ASSETS") {
            if !std::env::args().any(|a| a == "--assets") {
                p.assets = v.into();
            }
        }
        p = p.checked()?;
        let maps = p.maps();
        let map = map
            .or_else(|| {
                maps.iter()
                    .find(|m| m.as_str() == "a0200_kingscity")
                    .cloned()
            })
            .or_else(|| maps.first().cloned())
            .unwrap_or_default();
        let mut state = Self {
            sources_assets: p.assets.to_string_lossy().into_owned(),
            sources_db: p
                .db_root
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            sources_overrides: p.overrides.to_string_lossy().into_owned(),
            thumbs: browser::Thumbnails::new(&p),
            project: p,
            session: None,
            maps,
            map: map.clone(),
            map_search: String::new(),
            selected: None,
            draft: Value::Null,
            original: Value::Null,
            raw: String::new(),
            raw_changed: false,
            rows: vec![],
            tab: 0,
            search: String::new(),
            table: String::new(),
            event: String::new(),
            events: default(),
            catalogs: default(),
            revision: 0,
            publish: false,
            message: "Choose a map to begin".into(),
            error: false,
            assets: vec![],
            asset_search: String::new(),
            asset_folder: String::new(),
            asset_selected: String::new(),
            sources_open: false,
            pending: None,
            overview: default(),
            show_overview: false,
            show_markers: true,
            add_open: false,
            add_kind: data::GEOMETRY.into(),
            add_json: String::new(),
        };
        state.assets = browser::scan_assets(&state.project);
        if !map.is_empty() {
            if let Err(e) = state.open(&map) {
                state.message = e.to_string();
                state.error = true;
            }
        }
        Ok(state)
    }
    fn open(&mut self, name: &str) -> DataResult<()> {
        let s = self.project.open(name, true)?;
        self.map = name.into();
        self.session = Some(s);
        self.selected = None;
        self.draft = Value::Null;
        self.original = Value::Null;
        self.raw.clear();
        self.raw_changed = false;
        self.event.clear();
        self.catalogs = default();
        self.overview = overview::Overview::load(self.session.as_ref().unwrap());
        self.refresh();
        self.message = format!("Opened {name}");
        Ok(())
    }
    fn refresh(&mut self) {
        let Some(s) = &self.session else { return };
        self.events =
            data::events::MapEvents::new(&s.records, &s.snapshot["manifest"]["event_rules"]);
        let active = self.events.preview_flags(&self.event);
        self.rows.clear();
        for (kind, records) in data::object(&s.records) {
            for (id, v) in data::object(records) {
                let model = if kind == data::GEOMETRY {
                    data::text(&v["model"])
                } else {
                    data::text(&v["Graphics"])
                };
                let label = if kind == data::LIGHT {
                    format!(
                        "{} light · {}",
                        if v["LightType"] == 1 { "Spot" } else { "Point" },
                        &id[id.len().saturating_sub(8)..]
                    )
                } else if kind == data::AMBIENCE && data::flag(&v["PEDefaultEntity"]) {
                    "Environment · sun / ambient / fog".into()
                } else {
                    ["n", "Name", "Id"]
                        .iter()
                        .find_map(|k| v[*k].as_str().filter(|s| !s.is_empty()))
                        .unwrap_or(if model.is_empty() {
                            kind.trim_start_matches("_Instance_")
                        } else {
                            model.rsplit('/').next().unwrap()
                        })
                        .to_owned()
                };
                let appearance = Value::Object(
                    ["CharacterSet", "AnimSet", "StartAnimationState"]
                        .into_iter()
                        .filter_map(|k| {
                            v.get(k)
                                .filter(|v| !data::text(v).is_empty())
                                .map(|v| (k.into(), v.clone()))
                        })
                        .collect(),
                );
                self.rows.push(Row {
                    key: format!("{kind}\t{id}"),
                    kind: kind.clone(),
                    id: id.clone(),
                    search: format!("{label} {model} {id} {v}").to_lowercase(),
                    label,
                    model: model.into(),
                    active: self.events.visible(kind, id, v, &active),
                    appearance,
                });
            }
        }
        self.rows
            .sort_by(|a, b| a.label.cmp(&b.label).then(a.key.cmp(&b.key)));
        self.publish = true;
    }
    fn draft_dirty(&self) -> bool {
        self.raw_changed || self.draft != self.original
    }
    fn apply(&mut self) -> DataResult<()> {
        if !self.draft_dirty() {
            return Ok(());
        }
        let key = self.selected.clone().ok_or("Nothing selected")?;
        let (kind, id) = key.split_once('\t').ok_or("Invalid selection")?;
        let value = if self.raw_changed {
            scene::from_friendly(&serde_json::from_str::<Value>(&self.raw)?)?
        } else {
            self.draft.clone()
        };
        self.session
            .as_mut()
            .ok_or("Open a map first")?
            .update(kind, id, value.clone())?;
        self.draft = value.clone();
        self.original = value;
        self.raw_changed = false;
        self.raw = serde_json::to_string_pretty(&scene::friendly(&self.draft))?;
        self.refresh();
        Ok(())
    }
    fn select(&mut self, key: String) -> DataResult<()> {
        self.apply()?;
        self.selected = Some(key.clone());
        let (k, id) = key.split_once('\t').ok_or("Invalid object key")?;
        let s = self.session.as_ref().ok_or("Open a map first")?;
        self.draft = s.records[k][id].clone();
        if self.draft.is_null() {
            self.draft = s
                .operation(k, id)
                .and_then(|o| o.get("base"))
                .cloned()
                .unwrap_or(Value::Null);
        }
        self.original = self.draft.clone();
        self.raw = serde_json::to_string_pretty(&scene::friendly(&self.draft))?;
        self.raw_changed = false;
        Ok(())
    }
    fn selected_parts(&self) -> DataResult<(String, String)> {
        let (k, id) = self
            .selected
            .as_deref()
            .and_then(|s| s.split_once('\t'))
            .ok_or("Select an object first")?;
        Ok((k.into(), id.into()))
    }
    fn placement(&self, camera: &Transform, editor: &Editor) -> [f32; 3] {
        let direction = camera.forward().as_vec3();
        let ground = (editor.center.y - camera.translation.y) / direction.y;
        let distance = if direction.y < -0.001 && ground > 0. && ground <= 1000. {
            ground
        } else {
            10.
        };
        let mut p = camera.translation + direction * distance;
        if editor.controls.snap > 0. {
            p = (p / editor.controls.snap).round() * editor.controls.snap;
        }
        p.to_array()
    }
    fn execute(
        &mut self,
        action: Action,
        editor: &mut Editor,
        camera: &Transform,
        exit: &mut MessageWriter<AppExit>,
    ) -> DataResult<()> {
        match action {
            Action::Open(name) => {
                self.open(&name)?;
            }
            Action::Select(key) => self.select(key)?,
            Action::Apply => {
                self.apply()?;
            }
            Action::Save => {
                self.apply()?;
                self.session.as_mut().ok_or("Open a map first")?.save()?;
                self.message = "Overrides saved".into();
            }
            Action::Build | Action::Play => {
                self.apply()?;
                let s = self.session.as_mut().ok_or("Open a map first")?;
                s.save()?;
                let report = data::export::build(s, None, None)?;
                self.message = format!(
                    "Built client assets and derived DB4. {}",
                    data::text(&report["note"])
                );
                if matches!(action, Action::Play) {
                    let mut command = std::process::Command::new(std::env::current_exe()?);
                    command
                        .args([&s.name, "--npcs"])
                        .env("DSOR_ASSETS", &s.project.assets)
                        .current_dir(&s.project.root);
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        command.creation_flags(0x08000000);
                    }
                    command.spawn()?;
                }
            }
            Action::Undo | Action::Redo => {
                if let Some(s) = &mut self.session {
                    if matches!(action, Action::Undo) {
                        s.undo();
                    } else {
                        s.redo();
                    }
                }
                self.draft = self.original.clone();
                self.raw_changed = false;
                self.refresh();
                if let Some(key) = self.selected.clone() {
                    self.select(key)?;
                }
            }
            Action::Duplicate => {
                self.apply()?;
                let (k, id) = self.selected_parts()?;
                let id = self.session.as_mut().unwrap().duplicate(&k, &id)?;
                self.refresh();
                self.select(format!("{k}\t{id}"))?;
            }
            Action::Delete => {
                self.apply()?;
                let (k, id) = self.selected_parts()?;
                self.session.as_mut().unwrap().delete(&k, &id)?;
                self.selected = None;
                self.draft = Value::Null;
                self.original = Value::Null;
                self.refresh();
            }
            Action::Revert(ref key) | Action::Keep(ref key) => {
                let (k, id) = key.split_once('\t').ok_or("Invalid key")?;
                let s = self.session.as_mut().ok_or("Open a map first")?;
                if matches!(action, Action::Keep(_)) {
                    s.keep_override(k, id)?;
                } else {
                    s.revert(k, id);
                }
                self.draft = self.original.clone();
                self.raw_changed = false;
                self.refresh();
                self.select(key.clone())?;
            }
            Action::AddAsset(model) => {
                self.apply()?;
                let position = self.placement(camera, editor);
                let id=self.session.as_mut().ok_or("Open a map first")?.add(data::GEOMETRY,json!({"model":model,"p":position,"r":[0.,0.,0.,1.],"s":[1.,1.,1.],"n":"","g":0,"col":true}))?;
                self.refresh();
                self.select(format!("geometry\t{id}"))?;
            }
            Action::AddLight(spot) => {
                self.apply()?;
                let mut position = self.placement(camera, editor);
                position[1] += 3.;
                let s = self.session.as_mut().ok_or("Open a map first")?;
                let value = scene::new_light(s, position, spot)?;
                let id = s.add(data::LIGHT, value)?;
                self.refresh();
                self.select(format!("{}\t{id}", data::LIGHT))?;
            }
            Action::AddRecord => {
                self.apply()?;
                let value = scene::from_friendly(&serde_json::from_str::<Value>(&self.add_json)?)?;
                let id = self
                    .session
                    .as_mut()
                    .ok_or("Open a map first")?
                    .add(&self.add_kind, value)?;
                self.add_open = false;
                self.refresh();
                self.select(format!("{}\t{id}", self.add_kind))?;
            }
            Action::Quit => {
                exit.write(AppExit::Success);
            }
            Action::Restart => {
                let mut p = self.project.clone();
                p.assets = self.sources_assets.clone().into();
                p.db_root = if self.sources_db.trim().is_empty() {
                    None
                } else {
                    Some(self.sources_db.clone().into())
                };
                p.overrides = self.sources_overrides.clone().into();
                let p = p.checked()?;
                p.save_settings()?;
                let mut command = std::process::Command::new(std::env::current_exe()?);
                command
                    .arg("--editor")
                    .arg(&self.map)
                    .current_dir(&p.root)
                    .env_remove("DSOR_ASSETS");
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                command.spawn()?;
                exit.write(AppExit::Success);
            }
        }
        Ok(())
    }
}
pub(super) fn install(app: &mut App, state: State) {
    app.insert_resource(state)
        .init_resource::<ViewportImage>()
        .add_plugins(EguiPlugin::default())
        .insert_resource(bevy_egui::EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        })
        .add_systems(Startup, setup.after(super::setup))
        .add_systems(EguiPrimaryContextPass, draw)
        .add_systems(Update, close_request);
}
#[derive(Resource, Default)]
struct ViewportImage {
    image: Handle<Image>,
    size: UVec2,
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut viewport: ResMut<ViewportImage>,
    view: Single<Entity, With<ViewCamera>>,
) {
    viewport.size = UVec2::new(1600, 960);
    viewport.image = images.add(Image::new_target_texture(
        1600,
        960,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands
        .entity(*view)
        .insert(bevy::camera::RenderTarget::Image(
            viewport.image.clone().into(),
        ));
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        bevy::camera::visibility::RenderLayers::none(),
        Camera {
            order: 20,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.055, 0.075, 0.10)),
            ..default()
        },
    ));
}
fn close_request(
    mut events: MessageReader<bevy::window::WindowCloseRequested>,
    mut state: ResMut<State>,
) {
    if events.read().next().is_some() {
        state.pending = Some(Action::Quit);
    }
}
fn style(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = egui::vec2(8., 7.);
    style.spacing.button_padding = egui::vec2(10., 6.);
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = egui::Color32::from_rgb(27, 30, 36);
    style.visuals.window_fill = egui::Color32::from_rgb(32, 36, 43);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(17, 20, 25);
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(34, 79, 113);
    style.visuals.selection.stroke = egui::Stroke::new(1., egui::Color32::from_rgb(130, 201, 255));
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(43, 48, 57);
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}
fn request(state: &mut State, action: Action, actions: &mut Vec<Action>) {
    if matches!(action, Action::Open(_) | Action::Quit | Action::Restart)
        && (state.draft_dirty() || state.session.as_ref().is_some_and(Session::dirty))
    {
        state.pending = Some(action);
    } else {
        actions.push(action);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut contexts: Query<&mut bevy_egui::EguiContext, With<PrimaryEguiContext>>,
    mut textures: ResMut<EguiUserTextures>,
    mut images: ResMut<Assets<Image>>,
    mut viewport: ResMut<ViewportImage>,
    mut state: ResMut<State>,
    mut editor: ResMut<Editor>,
    mut previews: ResMut<previews::Previews>,
    monitor: Res<camera_preview::Monitor>,
    lighting_state: Res<lighting_preview::State>,
    mut cameras: Query<
        (&mut Camera, &Transform, &mut bevy::camera::RenderTarget),
        With<ViewCamera>,
    >,
    windows: Query<&Window>,
    roots: Query<&Transform, With<ObjectRoot>>,
    mut exit: MessageWriter<AppExit>,
) -> bevy::prelude::Result {
    let mut context = contexts.single_mut()?;
    let ctx = context.get_mut();
    let state = &mut *state;
    let editor = &mut *editor;
    let Ok((mut camera, camera_at, mut target)) = cameras.single_mut() else {
        return Ok(());
    };
    let Ok(window) = windows.single() else {
        return Ok(());
    };
    if state.revision == 0 {
        style(ctx);
    }
    let mut actions = vec![];
    for event in std::mem::take(&mut editor.inbox) {
        match data::text(&event["action"]) {
            "select" => actions.push(Action::Select(data::text(&event["key"]).into())),
            "transform" => {
                let result = state.apply().and_then(|_| {
                    scene::apply_transform(
                        state.session.as_mut().ok_or("No session")?,
                        &event,
                        state.revision,
                    )
                });
                match result {
                    Ok(()) => {
                        state.refresh();
                        if let Some(key) = state.selected.clone() {
                            actions.push(Action::Select(key));
                        }
                    }
                    Err(e) => {
                        state.message = e.to_string();
                        state.error = true;
                        state.publish = true;
                    }
                }
            }
            "save" => actions.push(Action::Save),
            "undo" => actions.push(Action::Undo),
            "redo" => actions.push(Action::Redo),
            "duplicate" => actions.push(Action::Duplicate),
            "delete" => actions.push(Action::Delete),
            _ => {}
        }
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "editor-root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::Panel::top("project-bar").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("DSOR")
                    .strong()
                    .size(20.)
                    .color(egui::Color32::from_rgb(117, 195, 255)),
            );
            ui.label(egui::RichText::new("LEVEL EDITOR").size(11.).weak());
            ui.separator();
            egui::ComboBox::from_id_salt("map-picker")
                .selected_text(&state.map)
                .width(210.)
                .show_ui(ui, |ui| {
                    ui.text_edit_singleline(&mut state.map_search);
                    let filter = state.map_search.to_lowercase();
                    for name in state.maps.iter().filter(|n| n.contains(&filter)) {
                        ui.selectable_value(&mut state.map, name.clone(), name);
                    }
                });
            if ui.button("Open").clicked() {
                request(state, Action::Open(state.map.clone()), &mut actions);
            }
            if ui.button("Sources…").clicked() {
                state.sources_open = true;
            }
            ui.separator();
            if ui
                .button("Save")
                .on_hover_text("Save authored overrides · Ctrl+S")
                .clicked()
            {
                actions.push(Action::Save);
            }
            if ui.button("Undo").clicked() {
                actions.push(Action::Undo);
            }
            if ui.button("Redo").clicked() {
                actions.push(Action::Redo);
            }
            ui.separator();
            if ui
                .button("Build")
                .on_hover_text("Compose client sidecars and a derived DB4")
                .clicked()
            {
                actions.push(Action::Build);
            }
            if ui
                .button(egui::RichText::new("▶ Play").color(egui::Color32::from_rgb(127, 212, 162)))
                .clicked()
            {
                actions.push(Action::Play);
            }
            if state.session.as_ref().is_some_and(Session::dirty) || state.draft_dirty() {
                ui.label(
                    egui::RichText::new("● Unsaved").color(egui::Color32::from_rgb(232, 185, 103)),
                );
            }
        });
    });
    egui::Panel::bottom("status-bar").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&state.message).color(if state.error {
                egui::Color32::from_rgb(255, 135, 130)
            } else {
                egui::Color32::from_rgb(170, 184, 201)
            }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.small(format!(
                    "{} objects  ·  {} ready",
                    editor.placed.len(),
                    editor.placed.values().filter(|p| p.ready).count()
                ));
            });
        });
    });
    egui::Panel::left("content-browser")
        .default_size(310.)
        .resizable(true)
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in ["Objects", "Assets", "Lighting", "Changes"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut state.tab, i, *name);
                }
            });
            ui.separator();
            if state.tab == 2 { lighting::source(ui, &lighting_state); }
            match state.tab {
                1 => browser::assets(
                    ui,
                    state,
                    &mut previews,
                    &mut textures,
                    &mut images,
                    &mut actions,
                ),
                3 => changes(ui, state, &mut actions),
                _ => browser::objects(
                    ui,
                    state,
                    &mut previews,
                    &mut textures,
                    &mut images,
                    &mut actions,
                ),
            }
        });
    egui::Panel::right("details")
        .default_size(345.)
        .min_size(320.)
        .resizable(true)
        .show(&mut root, |ui| {
            lighting::selection(ui, editor);
            inspector::draw(
                ui,
                state,
                &mut previews,
                &mut textures,
                &mut images,
                &mut actions,
            );
        });
    egui::Panel::top("viewport-tools").show(&mut root, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                ("select", "Q  Select"),
                ("translate", "W  Move"),
                ("rotate", "E  Rotate"),
                ("scale", "R  Scale"),
            ] {
                let allowed = mode != "scale"
                    || !state
                        .selected
                        .as_deref()
                        .and_then(|s| s.split_once('\t'))
                        .is_some_and(|(k, _)| scene::is_camera(k));
                ui.add_enabled_ui(allowed, |ui| {
                    ui.selectable_value(&mut editor.controls.mode, mode.into(), label);
                });
            }
            ui.separator();
            ui.label("Snap");
            ui.add(
                egui::DragValue::new(&mut editor.controls.snap)
                    .speed(0.1)
                    .range(0. ..=100.),
            );
            ui.label("Fly");
            ui.add(
                egui::DragValue::new(&mut editor.controls.camera_speed)
                    .speed(1.)
                    .range(0.5..=200.),
            );
            if ui.button("Focus").clicked() {
                editor.controls.focus_serial += 1;
                editor.controls.focus_target = "selection".into();
            }
            if ui.button("Frame map").clicked() {
                editor.controls.focus_serial += 1;
                editor.controls.focus_target = "map".into();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut state.show_overview, false, "Perspective");
            ui.selectable_value(&mut state.show_overview, true, "Map overview");
            ui.separator();
            ui.label("Map event state");
            let before = state.event.clone();
            egui::ComboBox::from_id_salt("map-event")
                .selected_text(data::events::label(&state.event))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.event, String::new(), "None");
                    for flag in &state.events.flags {
                        ui.selectable_value(
                            &mut state.event,
                            flag.clone(),
                            data::events::label(flag),
                        )
                        .on_hover_text(flag);
                    }
                });
            if state.event != before {
                state.refresh();
            }
            ui.checkbox(&mut editor.controls.show_grid, "Grid");
            ui.checkbox(&mut editor.controls.show_helpers, "Helpers");
            ui.checkbox(&mut editor.controls.show_bubbles, "Lighting zones")
                .on_hover_text("Show colored trigger boundaries and labels. The Lighting tab also shows zones.");
            ui.checkbox(&mut state.show_markers, "Markers")
                .on_hover_text("Spawn locations and teleport stones. Click an icon to inspect; double-click to focus.");
            if state
                .session
                .as_ref()
                .is_some_and(|s| s.records[data::CAMERA].is_object())
            {
                if ui
                    .button(egui::RichText::new("▣ Fixed camera").color(egui::Color32::LIGHT_BLUE))
                    .clicked()
                {
                    actions.push(Action::Select("camera\tcharacter-selection".into()));
                }
            }
        });
    });
    let rect = root.available_rect_before_wrap();
    let scale = ctx.pixels_per_point();
    let pos = (Vec2::new(rect.min.x, rect.min.y) * scale).as_uvec2();
    let size = (Vec2::new(rect.width(), rect.height()) * scale)
        .as_uvec2()
        .max(UVec2::ONE);
    let max = UVec2::new(window.physical_width(), window.physical_height());
    if viewport.size != max && max.x > 0 && max.y > 0 {
        if let Some(mut image) = images.get_mut(&viewport.image) {
            *image = Image::new_target_texture(
                max.x,
                max.y,
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                None,
            );
        }
        viewport.size = max;
    }
    if let bevy::camera::RenderTarget::Image(image) = &mut *target {
        image.scale_factor = window.scale_factor();
    }
    if pos.x < max.x && pos.y < max.y {
        camera.viewport = Some(bevy::camera::Viewport {
            physical_position: pos,
            physical_size: size.min(max - pos),
            ..default()
        });
    }
    editor.viewport = Some(Rect::from_corners(
        Vec2::new(rect.min.x, rect.min.y),
        Vec2::new(rect.max.x, rect.max.y),
    ));
    let mut over_marker = false;
    if state.show_overview {
        overview::draw(&mut root, state, &mut actions, editor);
    } else if max.x > 0 && max.y > 0 {
        // One window camera owns compositing. Rendering the scene into an image
        // also clears regions uncovered when inspector panels or monitors move.
        let texture = textures.add_image(EguiTextureHandle::Strong(viewport.image.clone()));
        let uv = egui::Rect::from_min_max(
            egui::pos2(pos.x as f32 / max.x as f32, pos.y as f32 / max.y as f32),
            egui::pos2(
                (pos.x + size.x).min(max.x) as f32 / max.x as f32,
                (pos.y + size.y).min(max.y) as f32 / max.y as f32,
            ),
        );
        root.put(rect, egui::Image::new((texture, rect.size())).uv(uv));
        over_marker |= lighting::labels(&mut root, rect, editor, &lighting_state, &camera, camera_at,
            &roots, window.scale_factor() / scale, &mut actions);
        if state.show_markers {
            over_marker |= markers::draw(
                &mut root,
                rect,
                editor,
                &camera,
                camera_at,
                &roots,
                window.scale_factor() / scale,
                &mut actions,
            );
        }
    }
    if !state.show_overview {
        egui::Area::new("viewport-hint".into())
            .fixed_pos(rect.left_bottom() + egui::vec2(14., -30.))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "RMB + WASD  fly    ·    Q / E  down / up    ·    F  focus",
                    )
                    .small()
                    .color(egui::Color32::from_gray(185)),
                );
            });
    }
    if let Some(spec) = &monitor.state {
        let id = textures.add_image(EguiTextureHandle::Strong(monitor.image.clone()));
        let aspect = data::number(&spec["lens"]["aspect"], 16. / 9.) as f32;
        let width = (rect.width() * 0.34).clamp(180., 360.);
        egui::Window::new("GAME CAMERA")
            .id("game-camera-monitor".into())
            .title_bar(true)
            .resizable(false)
            .collapsible(false)
            .fixed_pos(egui::pos2(rect.right() - width - 18., rect.top() + 12.))
            .show(ctx, |ui| {
                ui.image((id, egui::vec2(width, width / aspect)));
                ui.small(format!(
                    "{:.1}° FOV  ·  {:.2}:1",
                    data::number(&spec["lens"]["fov"], 34.),
                    aspect
                ));
            });
    }
    dialogs(ctx, state, &mut actions);
    // Ctrl shortcuts remain available from the outliner, but text widgets retain
    // editing keys. Native viewport input handles the ordinary transform keys.
    if ctx.text_edit_focused() {
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::CTRL, egui::Key::S) {
                actions.push(Action::Save);
            }
        });
    }
    for action in actions {
        state.error = false;
        if let Err(e) = state.execute(action, editor, camera_at, &mut exit) {
            state.message = e.to_string();
            state.error = true;
        }
    }
    if state.publish {
        if let Some(s) = &state.session {
            state.revision += 1;
            match scene::document(s, &mut state.catalogs, state.revision)
                .and_then(|v| serde_json::from_value::<Scene>(v).map_err(Into::into))
            {
                Ok(scene) => {
                    editor.pending_scene = Some(scene);
                    editor.controls.revision = state.revision;
                    state.publish = false;
                }
                Err(e) => {
                    state.message = e.to_string();
                    state.error = true;
                    state.publish = false;
                }
            }
        }
    }
    if editor.controls.preview_ambience.as_ref().is_some_and(|key| state.selected.as_ref() != Some(key)) {
        editor.controls.preview_ambience = None;
    }
    editor.controls.selected = state.selected.clone();
    editor.controls.show_lights = state.tab == 2;
    let active = state.events.preview_flags(&state.event);
    editor.controls.active_events = state.events.active_sets(&active).into_iter().collect();
    editor.controls.hidden_geometry = state
        .rows
        .iter()
        .filter(|r| r.kind == data::GEOMETRY && !r.active)
        .map(|r| r.key.clone())
        .collect();
    editor.controls.pickable = state
        .rows
        .iter()
        .filter(|r| {
            r.active
                && (state.table.is_empty() || r.kind == state.table)
                && r.search.contains(&state.search.to_lowercase())
        })
        .map(|r| r.key.clone())
        .collect();
    editor.keyboard_blocked =
        ctx.text_edit_focused() || state.pending.is_some() || state.sources_open || state.add_open;
    editor.pointer_blocked = state.show_overview
        || ctx.is_pointer_over_egui()
        || ctx.pointer_latest_pos().is_none_or(|p| !rect.contains(p));
    // A bare root UI is not a panel: ignore its background for hit testing.
    if ctx.pointer_latest_pos().is_some_and(|p| rect.contains(p))
        && !state.show_overview
        && !state.sources_open
        && !state.add_open
        && state.pending.is_none()
    {
        editor.pointer_blocked = ctx
            .layer_id_at(ctx.pointer_latest_pos().unwrap())
            .is_some_and(|l| l.order != egui::Order::Background);
    }
    editor.pointer_blocked |= over_marker;
    Ok(())
}
fn changes(ui: &mut egui::Ui, state: &State, actions: &mut Vec<Action>) {
    ui.heading("Authored changes");
    let Some(s) = &state.session else { return };
    ui.small(format!(
        "{} operations · {} conflicts",
        data::array(&s.patch["operations"]).count(),
        s.conflicts.len()
    ));
    egui::ScrollArea::vertical().show(ui, |ui| {
        for op in data::array(&s.patch["operations"]) {
            let key = format!("{}\t{}", data::text(&op["kind"]), data::text(&op["id"]));
            ui.push_id(&key, |ui| {
                ui.group(|ui| {
                    let conflict = s
                        .conflicts
                        .iter()
                        .find(|c| c["kind"] == op["kind"] && c["id"] == op["id"]);
                    ui.label(format!(
                        "{} · {}",
                        data::text(&op["action"]).to_uppercase(),
                        data::text(&op["kind"]).trim_start_matches("_Instance_")
                    ));
                    ui.small(data::text(&op["id"]));
                    if let Some(c) = conflict {
                        ui.colored_label(egui::Color32::LIGHT_RED, data::text(&c["reason"]));
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Inspect").clicked() {
                            actions.push(Action::Select(key.clone()));
                        }
                        if ui.button("Revert").clicked() {
                            actions.push(Action::Revert(key.clone()));
                        }
                        if conflict.is_some() && ui.button("Keep override").clicked() {
                            actions.push(Action::Keep(key.clone()));
                        }
                    });
                });
            });
        }
    });
}
fn dialogs(ctx: &egui::Context, state: &mut State, actions: &mut Vec<Action>) {
    if state.sources_open {
        egui::Window::new("Project sources")
            .collapsible(false)
            .resizable(true)
            .default_width(580.)
            .show(ctx, |ui| {
                ui.label("Converted assets");
                ui.text_edit_singleline(&mut state.sources_assets);
                ui.label("Original map DB4 directory");
                ui.text_edit_singleline(&mut state.sources_db);
                ui.label("Authored overrides (outside assets)");
                ui.text_edit_singleline(&mut state.sources_overrides);
                ui.small("Applying sources restarts the editor with the selected asset directory.");
                ui.horizontal(|ui| {
                    if ui.button("Apply and restart").clicked() {
                        state.sources_open = false;
                        request(state, Action::Restart, actions);
                    }
                    if ui.button("Cancel").clicked() {
                        state.sources_open = false;
                    }
                });
            });
    }
    if state.add_open {
        egui::Window::new("Add DB4 record")
            .collapsible(false)
            .default_width(550.)
            .show(ctx, |ui| {
                ui.label(&state.add_kind);
                egui::ScrollArea::vertical()
                    .max_height(550.)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut state.add_json)
                                .code_editor()
                                .desired_width(f32::INFINITY),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui.button("Add record").clicked() {
                        actions.push(Action::AddRecord);
                    }
                    if ui.button("Cancel").clicked() {
                        state.add_open = false;
                    }
                });
            });
    }
    if let Some(action) = state.pending.clone() {
        let dirty = state.draft_dirty() || state.session.as_ref().is_some_and(Session::dirty);
        if !dirty {
            state.pending = None;
            actions.push(action);
        } else {
            egui::Modal::new("unsaved-changes".into()).show(ctx, |ui| {
                ui.heading("Save your changes?");
                ui.label("This map has unsaved edits.");
                ui.horizontal(|ui| {
                    if ui.button("Save and continue").clicked() {
                        let result = state
                            .apply()
                            .and_then(|_| state.session.as_mut().ok_or("No map")?.save());
                        match result {
                            Ok(()) => {
                                state.pending = None;
                                actions.push(action.clone());
                            }
                            Err(e) => {
                                state.message = e.to_string();
                                state.error = true;
                            }
                        }
                    }
                    if ui.button("Discard and continue").clicked() {
                        state.pending = None;
                        actions.push(action.clone());
                    }
                    if ui.button("Cancel").clicked() {
                        state.pending = None;
                    }
                });
            });
        }
    }
}
