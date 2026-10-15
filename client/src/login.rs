//! Native account screen, rendered by the game's UI camera. Authentication and
//! the RakNet session run in this process; no launcher or child process is used.
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use bevy::clipboard::Clipboard;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{net, LoginGate, Options};

const SETTINGS: &str = "client-auth.json";
const GOLD: Color = Color::srgb(0.78, 0.64, 0.39);
const INK: Color = Color::srgb(0.045, 0.061, 0.08);
const MUTED: Color = Color::srgb(0.57, 0.64, 0.70);

pub struct LoginPlugin;
impl Plugin for LoginPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup.run_if(resource_exists::<LoginGate>))
            .add_systems(Update, (buttons, keyboard, poll, draw, update_text).chain()
                .run_if(resource_exists::<LoginGate>));
    }
}

// Only preferences are saved. Passwords and session tokens stay in memory.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    server: String,
    username: String,
    character: String,
    class: u8,
    gender: u8,
}
impl Default for Preferences {
    fn default() -> Self {
        Self { server: "127.0.0.1:8080".into(), username: String::new(),
            character: String::new(), class: 1, gender: 0 }
    }
}

#[derive(Default)]
struct Input {
    value: String,
    cursor: usize, // UTF-8 byte boundary
    selected: bool,
}
impl Input {
    fn new(value: String) -> Self { Self { cursor: value.len(), value, selected: false } }
    fn insert(&mut self, text: &str) {
        self.erase_selection();
        let text: String = text.chars().filter(|c| !c.is_control())
            .take(256usize.saturating_sub(self.value.chars().count())).collect();
        self.value.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }
    fn erase_selection(&mut self) -> bool {
        if !self.selected { return false; }
        self.value.clear(); self.cursor = 0; self.selected = false; true
    }
    fn left(&self) -> usize { self.value[..self.cursor].char_indices().next_back().map_or(0, |(i, _)| i) }
    fn right(&self) -> usize { self.cursor + self.value[self.cursor..].chars().next().map_or(0, char::len_utf8) }
    fn backspace(&mut self) {
        if !self.erase_selection() && self.cursor > 0 {
            let left = self.left(); self.value.drain(left..self.cursor); self.cursor = left;
        }
    }
    fn delete(&mut self) {
        if !self.erase_selection() && self.cursor < self.value.len() {
            self.value.drain(self.cursor..self.right());
        }
    }
    fn display(&self, secret: bool, caret: bool) -> String {
        let mut chars: Vec<char> = self.value.chars().collect();
        let pos = self.value[..self.cursor].chars().count();
        if secret { chars.fill('*'); }
        if caret { chars.insert(pos, '|'); }
        // Keep the cursor visible even with a pasted, long value.
        let start = pos.saturating_sub(31);
        chars.into_iter().skip(start).take(36).collect()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Page { Login, Register, Settings }
#[derive(Component)] struct Root;
#[derive(Component)] struct Field(usize);
#[derive(Component)] struct FieldText(usize);
#[derive(Component)] struct Status;
#[derive(Component, Clone, Copy)]
enum Action { Field(usize), Play, Register, Settings, Back, Class, Gender, Cancel }
enum Reply { Status(Value), Authenticated(net::NetConfig) }
struct Job { receiver: Mutex<mpsc::Receiver<Result<Reply, String>>>, authentication: bool }

#[derive(Resource)]
struct Form {
    inputs: [Input; 5], // username, password, confirmation, account service, character name
    focus: usize,
    page: Page,
    class: u8,
    gender: u8,
    status: String,
    error: bool,
    redraw: bool,
    job: Option<Job>,
    connecting: Option<Instant>,
    login_override: Option<String>,
    character_id: Option<u32>,
}
impl Form {
    fn busy(&self) -> bool { self.connecting.is_some() || self.job.as_ref().is_some_and(|j| j.authentication) }
    fn fields(&self) -> &[usize] {
        match self.page { Page::Login => &[0, 1], Page::Register => &[0, 1, 2], Page::Settings => &[3, 4] }
    }
    fn message(&mut self, text: impl Into<String>, error: bool) { self.status = text.into(); self.error = error; }
    fn preferences(&self) -> Preferences {
        Preferences { server: self.inputs[3].value.trim().into(), username: self.inputs[0].value.trim().into(),
            character: self.inputs[4].value.trim().into(), class: self.class, gender: self.gender }
    }
    fn save(&mut self) {
        let data = serde_json::to_vec_pretty(&self.preferences()).expect("preferences serialize");
        if std::fs::write(SETTINGS, data).is_err() {
            self.message("Settings could not be saved; you can still sign in.", true);
        }
    }
    fn check_server(&mut self) {
        let address = match account_address(&self.inputs[3].value) {
            Ok(a) => a, Err(e) => { self.message(e, true); return; }
        };
        self.message("Checking account server...", false);
        let (tx, rx) = mpsc::channel();
        self.job = Some(Job { receiver: Mutex::new(rx), authentication: false });
        std::thread::spawn(move || {
            let _ = tx.send(request(&address, "GET", "/api/status", json!(null)).map(Reply::Status));
        });
    }
    fn submit(&mut self) {
        if self.busy() { return; }
        if self.page == Page::Settings {
            match account_address(&self.inputs[3].value) {
                Ok(address) => self.inputs[3] = Input::new(address),
                Err(e) => { self.message(e, true); return; }
            }
            self.save(); self.page = Page::Login; self.focus = 0; self.redraw = true; self.check_server(); return;
        }
        let username = self.inputs[0].value.trim().to_string();
        if username.is_empty() || self.inputs[1].value.is_empty() {
            self.message("Enter your username and password.", true); return;
        }
        let register = self.page == Page::Register;
        if register && self.inputs[1].value != self.inputs[2].value {
            self.message("The passwords do not match.", true); return;
        }
        let address = match account_address(&self.inputs[3].value) {
            Ok(a) => a, Err(e) => { self.message(e, true); return; }
        };
        let password = std::mem::take(&mut self.inputs[1]).value;
        self.inputs[2] = Input::default();
        let mut preferences = self.preferences();
        preferences.server = address.clone();
        let login_override = self.login_override.clone();
        let character_id = self.character_id;
        self.save();
        self.message(if register { "Creating your account..." } else { "Signing in..." }, false);
        let (tx, rx) = mpsc::channel();
        self.job = Some(Job { receiver: Mutex::new(rx), authentication: true });
        self.redraw = true;
        std::thread::spawn(move || {
            let result = (|| {
                let payload = json!({"username": username, "password": password});
                if register { request(&address, "POST", "/api/register", payload.clone())?; }
                let reply = request(&address, "POST", "/api/login", payload)?;
                let config = session_config(&reply, &preferences, login_override, character_id)?;
                // DNS may block, so resolve here rather than in the rendering thread.
                let target = dsor_proto::session::resolve(&config.login, 2190)
                    .ok_or("The game server address could not be resolved.")?;
                Ok(Reply::Authenticated(net::NetConfig { login: target.to_string(), ..config }))
            })();
            let _ = tx.send(result);
        });
    }
}

fn account_address(value: &str) -> Result<String, String> {
    let value = value.trim().strip_prefix("http://").unwrap_or(value.trim()).trim_end_matches('/');
    if value.is_empty() || value.chars().any(char::is_whitespace) || value.contains(['/', '@', '?', '#']) {
        return Err("Enter an account server as host:port (for example 127.0.0.1:8080).".into());
    }
    let address = if value.contains(':') { value.to_string() } else { format!("{value}:8080") };
    let valid = address.rsplit_once(':').is_some_and(|(host, port)|
        !host.is_empty() && port.parse::<u16>().is_ok_and(|p| p > 0));
    if !valid { return Err("The account server needs a port between 1 and 65535.".into()); }
    Ok(address)
}

fn request(address: &str, method: &str, path: &str, payload: Value) -> Result<Value, String> {
    let body = if payload.is_null() { String::new() } else { payload.to_string() };
    let (status, body) = dsor_transport::http_request(address, method, path, &body)
        .map_err(|_| "Cannot reach the account server. Check Settings and try again.".to_string())?;
    let document: Value = serde_json::from_str(&body)
        .map_err(|_| "The account server returned an invalid response.".to_string())?;
    if !(200..300).contains(&status) {
        return Err(document["error"].as_str().unwrap_or("The account server refused the request.")
            .chars().filter(|c| !c.is_control()).take(180).collect());
    }
    Ok(document)
}

fn session_config(reply: &Value, prefs: &Preferences, login: Option<String>, character: Option<u32>) -> Result<net::NetConfig, String> {
    let account = reply["session"]["account_id"].as_u64().filter(|a| *a > 0 && *a <= u32::MAX as u64)
        .ok_or("The server returned an invalid account.")?.to_string();
    let session = reply["session"]["session_id"].as_str().ok_or("The server returned no session.")?;
    if !session.chars().all(|c| c.is_ascii_hexdigit() || c == '-') ||
        dsor_proto::identity::Credentials::parse(&account, session).is_none() {
        return Err("The server returned an invalid session.".into());
    }
    let login = if let Some(login) = login { login } else {
        let fallback = prefs.server.rsplit_once(':').map_or(prefs.server.as_str(), |(h, _)| h);
        let host = reply["launch"]["host"].as_str().filter(|h| !h.is_empty()).unwrap_or(fallback);
        let port = reply["launch"].get("login_port").map_or(Some(2190), Value::as_u64)
            .filter(|p| *p > 0 && *p <= 65535).ok_or("The server returned an invalid login port.")?;
        format!("{host}:{port}")
    };
    let chat = net::chat_target(&reply["launch"], &login);
    Ok(net::NetConfig { login, account, session: session.into(), user: None, password: None,
        account_api: Some(prefs.server.clone()), relay: None, character, chat: Some(chat),
        new_character: Some((if prefs.character.is_empty() { prefs.username.clone() } else { prefs.character.clone() }, prefs.class, prefs.gender)) })
}

fn setup(mut commands: Commands, mut options: ResMut<Options>) {
    let mut prefs: Preferences = std::fs::read(SETTINGS).ok()
        .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut password = String::new();
    let mut login_override = None;
    let mut character_id = None;
    if let Some(config) = options.net.as_mut() {
        login_override = Some(config.login.clone()); character_id = config.character;
        prefs.server = config.account_api.clone().unwrap_or_else(||
            format!("{}:8080", config.login.rsplit_once(':').map_or(config.login.as_str(), |(h, _)| h)));
        if let Some(user) = config.user.take() { prefs.username = user; }
        password = config.password.take().unwrap_or_default();
        if let Some((name, class, gender)) = &config.new_character {
            prefs.character = name.clone(); prefs.class = *class; prefs.gender = *gender;
        }
    }
    let mut form = Form { inputs: [Input::new(prefs.username), Input::new(password), Input::default(),
            Input::new(prefs.server), Input::new(prefs.character)], focus: 0, page: Page::Login,
        class: prefs.class.min(3), gender: prefs.gender.min(1), status: String::new(), error: false,
        redraw: true, job: None, connecting: None, login_override, character_id };
    if !form.inputs[0].value.is_empty() { form.focus = 1; }
    form.check_server();
    commands.insert_resource(form);
}

fn buttons(mut form: ResMut<Form>, interactions: Query<(&Interaction, &Action), Changed<Interaction>>) {
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed { continue; }
        if form.busy() {
            if matches!(action, Action::Cancel) {
                form.job = None;
                if form.connecting.is_some() { form.connecting = Some(Instant::now() - Duration::from_secs(120)); }
                form.redraw = true; form.message("Sign-in cancelled.", false);
            }
            continue;
        }
        match *action {
            Action::Field(i) => { form.focus = i; form.inputs[i].cursor = form.inputs[i].value.len(); }
            Action::Play => form.submit(),
            Action::Register => { form.page = Page::Register; form.focus = 0; form.redraw = true; form.message("Create an account on this server.", false); }
            Action::Settings => { form.page = Page::Settings; form.focus = 3; form.redraw = true; form.message("Choose your server and new character defaults.", false); }
            Action::Back => { form.page = Page::Login; form.focus = 0; form.inputs[2] = Input::default(); form.redraw = true; form.check_server(); }
            Action::Class => { form.class = (form.class + 1) % 4; form.redraw = true; }
            Action::Gender => { form.gender ^= 1; form.redraw = true; }
            Action::Cancel => {}
        }
    }
}

fn keyboard(mut events: MessageReader<KeyboardInput>, keys: Res<ButtonInput<KeyCode>>, mut clipboard: ResMut<Clipboard>, mut form: ResMut<Form>) {
    for event in events.read() {
        if !event.state.is_pressed() || form.busy() { continue; }
        let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if event.key_code == KeyCode::Tab {
            let fields = form.fields();
            let current = fields.iter().position(|i| *i == form.focus).unwrap_or(0);
            form.focus = fields[(current + if shift { fields.len() - 1 } else { 1 }) % fields.len()];
            continue;
        }
        if event.key_code == KeyCode::Enter || event.key_code == KeyCode::NumpadEnter { form.submit(); continue; }
        let focus = form.focus;
        let input = &mut form.inputs[focus];
        if control {
            match event.key_code {
                KeyCode::KeyA => input.selected = true,
                KeyCode::KeyV => if let Some(Ok(text)) = clipboard.fetch_text().poll_result() { input.insert(&text); },
                _ => {}
            }
            continue;
        }
        match event.key_code {
            KeyCode::Backspace => input.backspace(),
            KeyCode::Delete => input.delete(),
            KeyCode::ArrowLeft => { input.cursor = input.left(); input.selected = false; }
            KeyCode::ArrowRight => { input.cursor = input.right(); input.selected = false; }
            KeyCode::Home => { input.cursor = 0; input.selected = false; }
            KeyCode::End => { input.cursor = input.value.len(); input.selected = false; }
            _ => if let Some(text) = &event.text { input.insert(text); }
        }
    }
}

fn poll(world: &mut World) {
    let reply = {
        let form = world.resource::<Form>();
        form.job.as_ref().and_then(|j| match j.receiver.lock().unwrap().try_recv() {
            Ok(r) => Some(r), Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("The sign-in request ended unexpectedly. Try again.".into()))
        })
    };
    if let Some(reply) = reply {
        world.resource_mut::<Form>().job = None;
        match reply {
            Ok(Reply::Status(doc)) => {
                let mut form = world.resource_mut::<Form>();
                if doc["service"] == "dso-account-service" {
                    form.message(format!("{} is online", doc["name"].as_str().unwrap_or("Account server")), false);
                } else { form.message("This address is not a DSO account server. Check Settings.", true); }
            }
            Ok(Reply::Authenticated(config)) => {
                world.resource_mut::<Options>().net = Some(config.clone());
                world.insert_resource(config.clone());
                net::start_session(world, config);
                let mut form = world.resource_mut::<Form>();
                form.connecting = Some(Instant::now()); form.message("Signed in. Loading your characters...", false);
            }
            Err(error) => { let mut form = world.resource_mut::<Form>(); form.message(error, true); form.focus = 1; }
        }
        world.resource_mut::<Form>().redraw = true;
    }
    let Some(start) = world.resource::<Form>().connecting else { return; };
    if world.get_resource::<crate::selection::Selection>().is_some_and(|s| s.active &&
        (!s.characters.is_empty() || s.dialog != crate::selection::Dialog::None)) {
        let roots: Vec<_> = world.query_filtered::<Entity, With<Root>>().iter(world).collect();
        for root in roots { world.entity_mut(root).despawn(); }
        world.remove_resource::<LoginGate>();
        world.resource_mut::<Form>().connecting = None;
        world.resource_mut::<Form>().redraw = false;
        world.resource_mut::<Options>().login_screen = false;
        for mut window in world.query::<&mut Window>().iter_mut(world) { window.title = "DSOR Kitty".into(); }
        info!("in-client login: opened character selection");
        return;
    }
    let failure = world.remove_resource::<net::ConnectionFailure>().map(|e| e.0);
    if failure.is_some() || start.elapsed() > Duration::from_secs(60) {
        world.resource_mut::<crate::loading::LoadingScreen>().cancel();
        world.remove_non_send::<net::Net>();
        world.remove_resource::<net::NetActive>();
        world.remove_resource::<net::NetConfig>();
        world.remove_resource::<crate::map::CurrentMap>();
        world.remove_resource::<crate::nav::CurrentNav>();
        let entities: Vec<Entity> = world.query_filtered::<Entity, Or<(With<crate::map::MapRoot>, With<net::LocalPlayer>, With<net::RemotePlayer>, With<crate::npc::Npc>)>>().iter(world).collect();
        for entity in entities { if let Ok(e) = world.get_entity_mut(entity) { e.despawn(); } }
        let mut form = world.resource_mut::<Form>();
        form.connecting = None; form.redraw = true;
        form.message(failure.unwrap_or_else(|| "Could not enter the world. Check the game server and sign in again.".into()), true);
        return;
    }
    let ready = world.get_resource::<crate::map::CurrentMap>().is_some_and(|m| m.spawned)
        && world.query_filtered::<&crate::character::CharacterAnim, With<net::LocalPlayer>>()
            .iter(world).any(|a| a.is_ready());
    if ready && !world.resource::<crate::loading::LoadingScreen>().active() {
        let roots: Vec<_> = world.query_filtered::<Entity, With<Root>>().iter(world).collect();
        for root in roots { world.entity_mut(root).despawn(); }
        world.remove_resource::<LoginGate>();
        world.resource_mut::<Form>().connecting = None;
        world.resource_mut::<Form>().redraw = false;
        world.resource_mut::<Options>().login_screen = false;
        for mut window in world.query::<&mut Window>().iter_mut(world) { window.title = "DSOR Kitty".into(); }
        info!("in-client login: entered the world");
    }
}

fn text(value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(value), TextFont { font_size: bevy::text::FontSize::Px(size), ..default() }, TextColor(color))
}
fn button(parent: &mut ChildSpawnerCommands, label: &str, action: Action, primary: bool) {
    parent.spawn((Button, action, Node { min_height: px(40), padding: UiRect::axes(px(18), px(10)),
        justify_content: JustifyContent::Center, align_items: AlignItems::Center, flex_grow: 1.0,
        border_radius: BorderRadius::all(px(4)), ..default() },
        BackgroundColor(if primary { GOLD } else { Color::srgb(0.13, 0.17, 0.21) })))
        .with_children(|b| { b.spawn(text(label, 15.0, if primary { INK } else { Color::WHITE })); });
}

fn draw(mut commands: Commands, mut form: ResMut<Form>, roots: Query<Entity, With<Root>>) {
    if !form.redraw { return; }
    form.redraw = false;
    for entity in &roots { commands.entity(entity).despawn(); }
    commands.spawn((Root, Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100),
        flex_direction: FlexDirection::Column, align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        row_gap: px(18), ..default() }, BackgroundColor(INK), GlobalZIndex(10000)))
        .with_children(|root| {
            root.spawn(text("D S O R   K I T T Y", 32.0, GOLD));
            root.spawn((Node { width: px(460), max_width: percent(95), padding: UiRect::all(px(28)),
                flex_direction: FlexDirection::Column, row_gap: px(12), border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)), ..default() },
                BackgroundColor(Color::srgb(0.075, 0.10, 0.13)), BorderColor::all(Color::srgb(0.25, 0.28, 0.30))))
                .with_children(|card| {
                    let title = if form.busy() { "Welcome to Dracania" } else { match form.page {
                        Page::Login => "Sign in", Page::Register => "Create account", Page::Settings => "Settings" } };
                    card.spawn(text(title, 24.0, Color::WHITE));
                    if !form.busy() {
                        for &i in form.fields() {
                            let label = ["Username", "Password", "Confirm password", "Account server (HTTP)", "New character name (optional)"][i];
                            card.spawn(text(label, 13.0, MUTED));
                            card.spawn((Button, Action::Field(i), Field(i), Node { width: percent(100), height: px(42),
                                padding: UiRect::axes(px(12), px(8)), border: UiRect::all(px(1)), align_items: AlignItems::Center,
                                overflow: Overflow::clip(), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(INK), BorderColor::all(MUTED)))
                                .with_children(|f| { f.spawn((FieldText(i), text("", 17.0, Color::WHITE))); });
                        }
                        if form.page == Page::Settings {
                            card.spawn(text("Used only when your account has no character.", 12.0, MUTED));
                            card.spawn(Node { column_gap: px(8), ..default() }).with_children(|row| {
                                button(row, ["Warrior", "Mage", "Ranger", "Dwarf"][form.class as usize], Action::Class, false);
                                button(row, ["Male", "Female"][form.gender as usize], Action::Gender, false);
                            });
                        }
                    }
                    card.spawn((Status, text(&form.status, 14.0, MUTED), Node { min_height: px(40), ..default() }));
                    if form.busy() { button(card, "Cancel", Action::Cancel, false); }
                    else {
                        button(card, match form.page { Page::Login => "PLAY", Page::Register => "CREATE ACCOUNT & PLAY", Page::Settings => "SAVE SETTINGS" }, Action::Play, true);
                        card.spawn(Node { column_gap: px(8), ..default() }).with_children(|row| {
                            if form.page == Page::Login {
                                button(row, "Create account", Action::Register, false);
                                button(row, "Settings", Action::Settings, false);
                            } else { button(row, "Back to sign in", Action::Back, false); }
                        });
                    }
                });
            root.spawn(text("RELEASE 261   /   Tab to switch fields   /   Enter to continue", 12.0, MUTED));
        });
}

fn update_text(form: Res<Form>, time: Res<Time>, mut texts: Query<(&FieldText, &mut Text)>,
    mut fields: Query<(&Field, &mut BorderColor, &mut BackgroundColor)>, mut status: Query<(&mut Text, &mut TextColor), (With<Status>, Without<FieldText>)>) {
    for (field, mut text) in &mut texts {
        **text = form.inputs[field.0].display(field.0 == 1 || field.0 == 2,
            form.focus == field.0 && (time.elapsed_secs() * 2.0) as u32 % 2 == 0);
    }
    for (field, mut border, mut background) in &mut fields {
        *border = BorderColor::all(if form.focus == field.0 { GOLD } else { Color::srgb(0.22, 0.27, 0.32) });
        background.0 = if form.focus == field.0 && form.inputs[field.0].selected { Color::srgb(0.18, 0.24, 0.32) } else { INK };
    }
    for (mut text, mut color) in &mut status {
        **text = form.status.clone();
        color.0 = if form.error { Color::srgb(1.0, 0.52, 0.45) } else { MUTED };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_editing_masks_unicode_and_replaces_selection() {
        let mut input = Input::new("aé猫".into());
        assert_eq!(input.display(true, false), "***");
        input.backspace(); input.cursor = input.left(); input.insert("ß");
        assert_eq!(input.value, "aßé");
        input.delete(); assert_eq!(input.value, "aß");
        input.selected = true; input.insert("new\nvalue");
        assert_eq!(input.value, "newvalue");
        assert!(!input.display(true, true).contains("new"));
    }
    #[test]
    fn server_address_rejects_urls_that_could_inject_http_headers() {
        assert_eq!(account_address("http://localhost:8080/").unwrap(), "localhost:8080");
        assert_eq!(account_address("localhost").unwrap(), "localhost:8080");
        for bad in ["", "https://localhost", "localhost:0", "localhost:65536", "host\r\nHeader:1", "user@host:80"] {
            assert!(account_address(bad).is_err());
        }
    }
    #[test]
    fn account_session_is_passed_to_login_port_not_gateway() {
        let prefs = Preferences { username: "Hero".into(), ..default() };
        let mut reply = json!({"session":{"account_id":777777,"session_id":"01234567-89ab-cdef-0123-456789abcdef"},
            "launch":{"host":"127.0.0.1","gateway_port":80}});
        let config = session_config(&reply, &prefs, None, None).unwrap();
        assert_eq!(config.login, "127.0.0.1:2190"); assert_eq!(config.account, "777777");
        assert_eq!(config.session, reply["session"]["session_id"]);
        assert!(config.password.is_none()); assert!(config.user.is_none());
        reply["launch"]["login_port"] = json!(2300);
        assert_eq!(session_config(&reply, &prefs, None, None).unwrap().login, "127.0.0.1:2300");
        reply["session"]["account_id"] = json!(u64::MAX);
        assert!(session_config(&reply, &prefs, None, None).is_err());
        reply["session"]["account_id"] = json!(777777); reply["session"]["session_id"] = json!("invalid");
        assert!(session_config(&reply, &prefs, None, None).is_err());
    }
}
