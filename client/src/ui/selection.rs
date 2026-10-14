//! Original character-selection mesh UI, with a runtime four-row roster.
use super::*;
use crate::selection::{Dialog, Request, Selection};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;

pub struct SelectionUiPlugin;
impl Plugin for SelectionUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, (spawn, input, modal_input, paint, modal).chain()
                .after(crate::net::NetReceive).after(super::follow_screen));
    }
}

#[derive(Resource)]
struct Data { layouts: Vec<Handle<Layout>>, maps: Handle<local_map::MapNames> }
#[derive(Component)] struct Root;
#[derive(Component)] struct ModalRoot;
#[derive(Component)] struct Suspended(Visibility);
#[derive(Component)] struct Piece { button: Option<usize>, state: Option<String>, row: Option<usize>, value: Option<Value> }
#[derive(Clone)] enum Value { Name, Class, Level, Map, SelectedName, Status, Andermant }
#[derive(Clone, Debug, PartialEq)] enum Action { Row(usize), Up, Down, Play, Add, Delete, Left, Right, Rotate, Fullscreen, Quit }
struct Hit { id: String, rect: [f32; 4], k: f32, action: Action }
#[derive(Resource, Default)] struct Controls { hits: Vec<Hit>, hovered: Option<usize>, pressed: Option<usize>, drag_x: Option<f32> }
#[derive(Component, Clone, Copy)] enum ModalAction { Cancel, Confirm, Class, Gender }

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Data { layouts: ["charselectionwindow", "currencybar", "gamebar"].into_iter()
        .map(|n| assets.load(format!("interface/{n}.ui.json"))).collect(), maps: assets.load("interface/maps.names.json") });
    commands.init_resource::<Controls>();
}

fn expanded(layout: &Layout) -> Vec<(Widget, Option<usize>)> {
    let template = layout.widgets.iter().find(|w| w.id.ends_with("/TemplateCharacter"));
    let mut result = Vec::new();
    for w in &layout.widgets {
        // The currency bar contains a dedicated RC-only version for this screen.
        let currency = layout.window == "currencybar" && w.id.starts_with("Canvas/LabelIngamePaymentRCOnly");
        if (w.hidden && !currency) || ["/UnlockRequestButton", "/BuyCharacterButton", "/InstanceSelectionLabel", "/EditButton", "/LabelTwinkIncentive",
            "/SelectedCharacterLevelClass", "/SelectedCharacterMap", "/DeactivatedIconsLabel"].iter().any(|s| w.id.contains(s)) { continue; }
        if layout.window == "currencybar" && !currency { continue; }
        if layout.window == "gamebar" && !w.id.contains("/gamemenu") && !w.id.contains("/fullscreen") { continue; }
        if let Some(t) = template.filter(|t| w.id.starts_with(&t.id)) {
            for row in 0..4 {
                let mut w = w.clone();
                let dy = row as f32 * (t.rect[3] - t.rect[1]);
                w.id = w.id.replacen(&t.id, &format!("{}/Row{row}", t.id), 1);
                w.rect[1] += dy; w.rect[3] += dy;
                if let Some(a) = &mut w.art { for xy in a.pos.chunks_exact_mut(2) { xy[1] += dy; } }
                result.push((w, Some(row)));
            }
        } else { result.push((w.clone(), None)); }
    }
    result
}

fn action(w: &Widget, row: Option<usize>) -> Option<Action> {
    if !matches!(w.kind.as_str(), "Button" | "TextButton") { return None; }
    let leaf = w.id.rsplit('/').next()?;
    Some(match leaf {
        "TemplateCharacterButton" => Action::Row(row?), "ScrollUp" => Action::Up, "ScrollDown" => Action::Down,
        "StartGame" => Action::Play, "CharacterAddButton" => Action::Add, "CharacterDeleteButton" => Action::Delete,
        "RotateCharLeftButton" => Action::Left, "RotateCharRightButton" => Action::Right,
        "DragRotateAreaButton01" => Action::Rotate,
        _ => match w.event.as_str() { "Logout" => Action::Quit, "ToggleFullscreen" => Action::Fullscreen, _ => return None },
    })
}

fn value(w: &Widget, row: Option<usize>) -> Option<Value> {
    Some(match w.id.rsplit('/').next()? {
        "CharacterName" if row.is_some() => Value::Name,
        "CharacterClass" if row.is_some() => Value::Class,
        "CharacterLevel" if row.is_some() => Value::Level,
        "Map" if row.is_some() => Value::Map,
        "SelectedCharacterName" => Value::SelectedName,
        "DenyReason" => Value::Status,
        "andermant_display" => Value::Andermant,
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
fn spawn(mut commands: Commands, selection: Res<Selection>, data: Res<Data>, layouts: Res<Assets<Layout>>,
    assets: Res<AssetServer>, screen: Res<Screen>, mut controls: ResMut<Controls>, roots: Query<Entity, With<Root>>,
    mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<UiMaterial>>,
    mut windows: Query<(Entity, &mut Visibility, Option<&Suspended>), With<Window2018>>) {
    for (entity, mut visibility, suspended) in &mut windows {
        if selection.active && suspended.is_none() {
            commands.entity(entity).insert(Suspended(*visibility)); *visibility = Visibility::Hidden;
        } else if !selection.active {
            if let Some(saved) = suspended { *visibility = saved.0; commands.entity(entity).remove::<Suspended>(); }
        }
    }
    if !selection.active {
        for root in &roots { commands.entity(root).despawn(); }
        controls.hits.clear(); controls.pressed = None; controls.hovered = None; controls.drag_x = None;
        return;
    }
    if !roots.is_empty() || data.layouts.iter().any(|h| layouts.get(h).is_none()) { return; }
    let root = commands.spawn((Root, Transform::from_xyz(0.0, 0.0, 400.0), Visibility::Inherited)).id();
    let widgets: Vec<_> = data.layouts.iter().flat_map(|h| expanded(layouts.get(h).unwrap())).collect();
    controls.hits = widgets.iter().filter_map(|(w, row)| action(w, *row).map(|action| Hit { id: w.id.clone(), rect: w.rect, k: w.k, action })).collect();
    let mut parents = HashMap::new();
    for (index, (w, row)) in widgets.iter().enumerate() {
        let button = controls.hits.iter().position(|h| w.id == h.id || w.id.starts_with(&(h.id.clone() + "/")));
        let state = button.and_then(|b| w.id.strip_prefix(&(controls.hits[b].id.clone() + "/")))
            .and_then(|s| s.split('/').next()).filter(|s| matches!(*s, "normal" | "pressed" | "mouseover" | "disabled" | "mouseoverpressed")).map(str::to_owned);
        let z = index as f32 * 0.01;
        let mut parent = |flip: bool, commands: &mut Commands| {
            *parents.entry((w.k.to_bits(), flip)).or_insert_with(|| commands.spawn((Follows { k: w.k, flip },
                follow_transform(&screen, w.k, flip), Visibility::Inherited, ChildOf(root))).id())
        };
        if let Some(art) = &w.art {
            // Payment is decorative here; do not paint all of its authored states.
            if w.id.contains("/PaymentButton/") && !w.id.contains("/PaymentButton/normal") { continue; }
            let mut run = Run::default(); run.push(art, 0.0);
            if !run.idx.is_empty() {
                let p = parent(true, &mut commands);
                commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(materials.add(material(art.tex.as_ref().map(|t| assets.load(t.clone()))))),
                    Transform::from_xyz(0.0, 0.0, z), RenderLayers::layer(LAYER), ChildOf(p), Visibility::Hidden,
                    Piece { button, state: state.clone(), row: *row, value: None }));
            }
        }
        if let Some(label) = &w.text {
            let dynamic = value(w, *row);
            if dynamic.is_none() && !static_text(label) { continue; }
            let [l, t, r, b] = w.rect;
            let x = match label.align[0] { 0 => l, 2 => r, _ => (l + r) * 0.5 };
            let y = match label.align[1] { 0 => t, 2 => b, _ => (t + b) * 0.5 };
            let p = parent(false, &mut commands);
            commands.spawn((Text2d::new(if dynamic.is_some() { String::new() } else { label.text.clone() }),
                TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(label.size.max(1.0) * FONT_SCALE * screen.scale), ..default() },
                TextColor(Color::srgba(label.color[0], label.color[1], label.color[2], label.color[3])),
                Anchor(Vec2::new(match label.align[0] { 0 => -0.5, 2 => 0.5, _ => 0.0 }, match label.align[1] { 0 => 0.5, 2 => -0.5, _ => 0.0 })),
                Transform::from_xyz(x, -y, z + 0.005).with_scale(Vec3::splat(1.0 / screen.scale)),
                // The source's text rectangles stop before the unused incentive icons.
                // Allow long names/map captions across the rest of their roster row.
                bevy::text::TextBounds::new_horizontal((if row.is_some() { 220.0 } else { r - l }) * screen.scale),
                TextLayout::justify(match label.align[0] { 0 => Justify::Left, 2 => Justify::Right, _ => Justify::Center }),
                RenderLayers::layer(LAYER), ChildOf(p), Visibility::Hidden, Piece { button, state, row: *row, value: dynamic }));
        }
    }
}

fn enabled(action: &Action, selection: &Selection) -> bool {
    if selection.entering || selection.dialog != Dialog::None { return false; }
    match action {
        Action::Row(row) => selection.first_row + row < selection.characters.len(),
        Action::Up => selection.first_row > 0,
        Action::Down => selection.first_row + 4 < selection.characters.len(),
        Action::Play | Action::Left | Action::Right | Action::Rotate => selection.current().is_some(),
        Action::Delete => !selection.offline && selection.current().is_some(),
        Action::Add => !selection.offline && selection.free > 0,
        _ => true,
    }
}

#[allow(clippy::too_many_arguments)]
fn input(mut selection: ResMut<Selection>, mut controls: ResMut<Controls>, mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>, mut scroll: MessageReader<MouseWheel>, time: Res<Time>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>, loading: Res<crate::loading::LoadingScreen>, mut exit: MessageWriter<AppExit>) {
    if !selection.active || loading.active() { scroll.clear(); return; }
    let Ok(mut window) = windows.single_mut() else { return };
    let cursor = window.cursor_position().map(|p| p * (BASE_H / window.height()));
    let width = BASE_H * window.width() / window.height();
    controls.hovered = cursor.and_then(|p| controls.hits.iter().enumerate().rev().filter(|(_, h)| {
        let [l,t,r,b] = h.rect; let x = p.x - h.k * (width - BASE_W);
        enabled(&h.action, &selection) && x >= l && x <= r && p.y >= t && p.y <= b
    // The authored portrait drag rectangle overlaps the rotation arrows.
    // Explicit buttons take priority over that broad background hit area.
    }).min_by_key(|(_, h)| matches!(h.action, Action::Rotate)).map(|(i, _)| i));
    if selection.dialog == Dialog::None && !selection.entering {
        if keys.just_pressed(KeyCode::ArrowUp) { selection.step(-1); }
        if keys.just_pressed(KeyCode::ArrowDown) { selection.step(1); }
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) { selection.play(); }
        let wheel: f32 = scroll.read().map(|w| w.y).sum();
        if wheel != 0.0 && controls.hovered.is_some_and(|i| matches!(controls.hits[i].action, Action::Row(_) | Action::Up | Action::Down)) {
            let max = selection.characters.len().saturating_sub(4);
            selection.first_row = (selection.first_row as i32 - wheel.signum() as i32).clamp(0, max as i32) as usize;
        }
    } else { scroll.clear(); }
    if mouse.just_pressed(MouseButton::Left) { controls.pressed = controls.hovered; controls.drag_x = cursor.map(|p| p.x); }
    if mouse.pressed(MouseButton::Left) {
        if let Some(i) = controls.pressed {
            match controls.hits[i].action {
                Action::Left => selection.angle += time.delta_secs() * 1.6,
                Action::Right => selection.angle -= time.delta_secs() * 1.6,
                Action::Rotate => if let Some(x) = cursor.map(|p| p.x) {
                    if let Some(old) = controls.drag_x { selection.angle += (x - old) * 0.012; }
                    controls.drag_x = Some(x);
                },
                _ => {}
            }
        }
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(i) = controls.pressed.filter(|i| Some(*i) == controls.hovered) {
            let action = controls.hits[i].action.clone();
            if enabled(&action, &selection) {
                match action {
                    Action::Row(row) => { let id = selection.characters[selection.first_row + row].character; selection.choose(id); }
                    Action::Up => selection.first_row = selection.first_row.saturating_sub(1),
                    Action::Down => selection.first_row += 1,
                    Action::Play => selection.play(),
                    Action::Add => {
                        selection.requests.push(Request::OpenEditor);
                        selection.dialog = Dialog::Create { name: String::new(), class: 0, gender: 0, waiting: true };
                        selection.status = "Opening character creation...".into(); selection.revision += 1;
                    }
                    Action::Delete => if let Some(c) = selection.current() {
                        selection.dialog = Dialog::Delete { id: c.character, name: c.name.clone(), waiting: false }; selection.revision += 1;
                    },
                    Action::Fullscreen => { window.mode = if matches!(window.mode, bevy::window::WindowMode::Windowed) {
                        bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
                    } else { bevy::window::WindowMode::Windowed }; }
                    Action::Quit => { exit.write(AppExit::Success); }
                    _ => {}
                }
            }
        }
        controls.pressed = None; controls.drag_x = None;
    }
}

fn paint(selection: Res<Selection>, controls: Res<Controls>, data: Res<Data>, maps: Res<Assets<local_map::MapNames>>,
    mut pieces: Query<(&Piece, &mut Visibility, Option<&mut Text2d>)>) {
    if !selection.active { return; }
    for (piece, mut visibility, text) in &mut pieces {
        let row = piece.row.and_then(|r| selection.characters.get(selection.first_row + r));
        let shown = if let (Some(button), Some(state)) = (piece.button, &piece.state) {
            let action = &controls.hits[button].action;
            let selected = matches!(action, Action::Row(_)) && row.is_some_and(|c| Some(c.character) == selection.selected);
            let expected = if !enabled(action, &selection) { "disabled" }
                else if controls.pressed == Some(button) || selected { "pressed" }
                else if controls.hovered == Some(button) { "mouseover" } else { "normal" };
            state == expected
        } else { true };
        *visibility = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if let (Some(value), Some(mut text)) = (&piece.value, text) {
            let value = match value {
                Value::Name => row.map(|c| c.name.clone()).unwrap_or_default(),
                Value::Class => row.map(|c| class_name(c.player_class).into()).unwrap_or_default(),
                Value::Level => row.map(|c| format!("Level {}", c.level)).unwrap_or_default(),
                Value::Map => row.map(|c| maps.get(&data.maps).map(|n| n.caption(&c.map_name)).unwrap_or_else(|| c.map_name.clone())).unwrap_or_default(),
                Value::SelectedName => selection.current().map(|c| c.name.clone()).unwrap_or_default(),
                Value::Status => selection.status.clone(),
                Value::Andermant => selection.current().map(|c| c.andermant.to_string()).unwrap_or_default(),
            };
            if **text != value { **text = value; }
        }
    }
}

fn class_name(class: u32) -> &'static str { ["Dragonknight", "Spellweaver", "Ranger", "Steam Mechanicus"].get(class as usize).copied().unwrap_or("Adventurer") }

fn modal_input(mut selection: ResMut<Selection>, mut events: MessageReader<KeyboardInput>,
    interactions: Query<(&Interaction, &ModalAction), Changed<Interaction>>, keys: Res<ButtonInput<KeyCode>>) {
    if !selection.active || selection.dialog == Dialog::None { events.clear(); return; }
    let mut actions: Vec<_> = interactions.iter().filter(|(i, _)| **i == Interaction::Pressed).map(|(_, a)| *a).collect();
    if keys.just_pressed(KeyCode::Escape) { actions.push(ModalAction::Cancel); }
    for event in events.read() {
        if !event.state.is_pressed() { continue; }
        if let Dialog::Create { name, waiting: false, .. } = &mut selection.dialog {
            if event.key_code == KeyCode::Backspace { name.pop(); }
            else if event.key_code == KeyCode::Enter { actions.push(ModalAction::Confirm); }
            else if let Some(text) = &event.text {
                name.extend(text.chars().filter(|c| !c.is_control()).take(32usize.saturating_sub(name.chars().count())));
            }
            selection.revision += 1;
        }
    }
    for action in actions {
        let waiting = matches!(selection.dialog, Dialog::Create { waiting: true, .. } | Dialog::Delete { waiting: true, .. });
        if waiting { continue; }
        match action {
            ModalAction::Cancel => {
                if matches!(selection.dialog, Dialog::Create { .. }) { selection.requests.push(Request::Back); }
                selection.dialog = Dialog::None; selection.status.clear();
            }
            ModalAction::Class => if let Dialog::Create { class, .. } = &mut selection.dialog { *class = (*class + 1) % 4; },
            ModalAction::Gender => if let Dialog::Create { gender, .. } = &mut selection.dialog { *gender ^= 1; },
            ModalAction::Confirm => match selection.dialog.clone() {
                Dialog::Create { name, class, gender, .. } => {
                    let name = name.trim().to_owned();
                    if name.is_empty() { selection.status = "Enter a character name.".into(); }
                    else { selection.requests.push(Request::Create(name.clone(), class, gender));
                        selection.dialog = Dialog::Create { name, class, gender, waiting: true }; selection.status = "Creating character...".into(); }
                }
                Dialog::Delete { id, name, .. } => { selection.requests.push(Request::Delete(id)); selection.dialog = Dialog::Delete { id, name, waiting: true }; selection.status = "Deleting character...".into(); }
                _ => {}
            }
        }
        selection.revision += 1;
    }
}

fn modal(mut commands: Commands, selection: Res<Selection>, assets: Res<AssetServer>, roots: Query<Entity, With<ModalRoot>>,
    mut last: Local<Option<(bool, u64)>>) {
    let key = (selection.active, selection.revision);
    if *last == Some(key) { return; } *last = Some(key);
    for root in &roots { commands.entity(root).despawn(); }
    if !selection.active || selection.dialog == Dialog::None { return; }
    let font: Handle<Font> = assets.load("fonts/tahoma.ttf");
    let color = Color::srgb(0.90, 0.78, 0.54);
    let (title, body, confirm, waiting) = match &selection.dialog {
        Dialog::Create { name, waiting, .. } => ("Create character", format!("Name: {name}|"), "Create", *waiting),
        Dialog::Delete { name, waiting, .. } => ("Delete character", format!("Permanently delete {name}?\nThis also deletes their items and progress."), "Delete", *waiting),
        _ => return,
    };
    commands.spawn((ModalRoot, Node { width: percent(100), height: percent(100), align_items: AlignItems::Center,
        justify_content: JustifyContent::Center, position_type: PositionType::Absolute, ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)), GlobalZIndex(800))).with_children(|root| {
        root.spawn((Node { width: px(430), padding: UiRect::all(px(28)), row_gap: px(18), flex_direction: FlexDirection::Column,
            border: UiRect::all(px(2)), ..default() }, BackgroundColor(Color::srgb(0.055, 0.065, 0.065)), BorderColor::all(color))).with_children(|panel| {
            for (text, size) in [(title.to_owned(), 23.0), (body, 17.0)] {
                panel.spawn((Text::new(text), TextFont { font: font.clone().into(), font_size: bevy::text::FontSize::Px(size), ..default() }, TextColor(color)));
            }
            let mut button = |label: String, action: ModalAction| {
                panel.spawn((Button, action, Node { padding: UiRect::all(px(10)), justify_content: JustifyContent::Center, ..default() },
                    BackgroundColor(Color::srgb(0.20, 0.15, 0.08)))).with_children(|p| {
                    p.spawn((Text::new(label), TextFont { font: font.clone().into(), font_size: bevy::text::FontSize::Px(17.0), ..default() }, TextColor(color)));
                });
            };
            if let Dialog::Create { class, gender, .. } = selection.dialog {
                button(class_name(class as u32).into(), ModalAction::Class);
                button(if gender == 0 { "Male" } else { "Female" }.into(), ModalAction::Gender);
            }
            button(if waiting { "Please wait..." } else { confirm }.into(), ModalAction::Confirm);
            button("Cancel".into(), ModalAction::Cancel);
            if !selection.status.is_empty() { panel.spawn((Text::new(&selection.status), TextFont { font: font.into(), font_size: bevy::text::FontSize::Px(16.0), ..default() }, TextColor(color))); }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_proto::commands::player::{CharacterSelection, SelectionCharacter};

    #[test]
    #[ignore = "requires locally converted character selection XML assets"]
    fn roster_pointer_keyboard_resize_rotation_and_delete_confirmation() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface/charselectionwindow.ui.json");
        let layout: Layout = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let hits = expanded(&layout).iter().filter_map(|(w, row)| action(w, *row).map(|action|
            Hit { id: w.id.clone(), rect: w.rect, k: w.k, action })).collect();
        let mut state = Selection::default();
        state.receive(CharacterSelection { operation: 1, character: 1, free: 6,
            characters: (1..=5).map(|id| SelectionCharacter { character: id, name: format!("Hero{id}"), ..default() }).collect(), ..default() }, None);
        state.requests.clear();
        let mut app = App::new();
        app.insert_resource(state).insert_resource(Controls { hits, ..default() })
            .init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time>().init_resource::<crate::loading::LoadingScreen>()
            .add_message::<MouseWheel>().add_message::<AppExit>().add_message::<KeyboardInput>()
            .add_systems(Update, (input, modal_input).chain());
        let primary = app.world_mut().spawn((Window { focused: true, resolution: (1280, 720).into(), ..default() }, PrimaryWindow)).id();
        let click = |app: &mut App, action: Action| {
            let (rect, k) = { let c = app.world().resource::<Controls>(); let h = c.hits.iter().find(|h| h.action == action).unwrap(); (h.rect, h.k) };
            let mut window = app.world_mut().get_mut::<Window>(primary).unwrap();
            let width = BASE_H * window.width() / window.height(); let scale = window.height() / BASE_H;
            let [l,t,r,b] = rect;
            window.set_cursor_position(Some(Vec2::new((l+r)*0.5+k*(width-BASE_W),(t+b)*0.5)*scale));
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left); app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left); app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        };
        click(&mut app, Action::Row(1)); assert_eq!(app.world().resource::<Selection>().selected, Some(2));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown); app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        assert_eq!(app.world().resource::<Selection>().selected, Some(3));
        click(&mut app, Action::Down); assert_eq!(app.world().resource::<Selection>().first_row, 1);
        app.world_mut().get_mut::<Window>(primary).unwrap().resolution.set(1920.0, 1080.0);
        click(&mut app, Action::Row(3)); assert_eq!(app.world().resource::<Selection>().selected, Some(5));
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        click(&mut app, Action::Left); assert!(app.world().resource::<Selection>().angle > 0.1);
        assert!(app.world().resource::<Selection>().requests.iter().all(|r| matches!(r, Request::Preview(_))));
        app.world_mut().resource_mut::<Selection>().requests.clear();
        click(&mut app, Action::Delete);
        assert!(matches!(app.world().resource::<Selection>().dialog, Dialog::Delete { id: 5, .. }));
        assert!(app.world().resource::<Selection>().requests.is_empty());
        let cancel = app.world_mut().spawn((Interaction::Pressed, ModalAction::Cancel)).id(); app.update(); app.world_mut().despawn(cancel);
        assert_eq!(app.world().resource::<Selection>().dialog, Dialog::None);
        assert!(app.world().resource::<Selection>().requests.is_empty());
        click(&mut app, Action::Delete);
        app.world_mut().spawn((Interaction::Pressed, ModalAction::Confirm)); app.update();
        assert!(matches!(app.world().resource::<Selection>().requests.as_slice(), [Request::Delete(5)]));
    }

    #[test]
    fn four_rows_keep_authored_geometry_and_hide_alternative_buttons() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface/charselectionwindow.ui.json");
        if !path.exists() { return; }
        let layout: Layout = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let widgets = expanded(&layout);
        let rows: Vec<_> = widgets.iter().filter(|(w, r)| matches!(action(w, *r), Some(Action::Row(_)))).collect();
        assert_eq!(rows.len(), 4);
        for pair in rows.windows(2) { assert!((pair[0].0.rect[3] - pair[1].0.rect[1]).abs() < 0.1); }
        assert!(!widgets.iter().any(|(w, _)| w.id.contains("UnlockRequestButton") || w.id.contains("BuyCharacterButton") || w.id.contains("EditButton")));
    }
}
