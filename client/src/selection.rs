//! Character-service lobby. Preview requests never commit a gameplay character.
use std::collections::HashMap;
use bevy::prelude::*;
use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use dsor_proto::commands::player::{CharacterSelection, SelectionCharacter, SelectionEquipment};
use serde::Deserialize;
use crate::character::{self, CharacterAnim, CharacterDesc, CharacterLibrary, ItemSkins};

pub const MAP: &str = "a0000_char";

#[derive(Clone, Debug)]
pub enum Request {
    Preview(u32), Play(u32), OpenEditor, Back,
    Create(String, u8, u8), Delete(u32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Dialog {
    #[default]
    None,
    Create { name: String, class: u8, gender: u8, waiting: bool },
    Delete { id: u32, name: String, waiting: bool },
}

#[derive(Resource, Default)]
pub struct Selection {
    pub active: bool,
    pub entering: bool,
    pub offline: bool,
    pub characters: Vec<SelectionCharacter>,
    pub selected: Option<u32>,
    pub capacity: u32,
    pub free: u32,
    pub first_row: usize,
    pub angle: f32,
    pub status: String,
    pub dialog: Dialog,
    pub revision: u64,
    pub requests: Vec<Request>,
    pub equipment: HashMap<u32, Vec<SelectionEquipment>>,
    requested: Option<u32>,
    preview_revision: u64,
}

impl Selection {
    pub fn current(&self) -> Option<&SelectionCharacter> {
        self.characters.iter().find(|c| Some(c.character) == self.selected)
    }

    pub fn choose(&mut self, id: u32) {
        if self.entering || self.selected == Some(id) || !self.characters.iter().any(|c| c.character == id) { return; }
        self.selected = Some(id);
        self.reveal_selected();
        self.status.clear();
        self.requested = None;
        self.revision += 1;
        self.preview_revision += 1;
        self.request_preview();
    }

    pub fn step(&mut self, delta: i32) {
        let Some(index) = self.characters.iter().position(|c| Some(c.character) == self.selected) else { return };
        let next = (index as i32 + delta).clamp(0, self.characters.len().saturating_sub(1) as i32) as usize;
        self.choose(self.characters[next].character);
        self.reveal_selected();
    }

    fn reveal_selected(&mut self) {
        if let Some(index) = self.characters.iter().position(|c| Some(c.character) == self.selected) {
            if index < self.first_row { self.first_row = index; }
            if index >= self.first_row + 4 { self.first_row = index - 3; }
        }
    }

    fn request_preview(&mut self) {
        if let Some(id) = self.selected {
            if !self.offline && self.requested != Some(id) {
                self.requests.push(Request::Preview(id));
                self.requested = Some(id);
            }
        }
    }

    pub fn receive(&mut self, reply: CharacterSelection, preferred: Option<u32>) {
        match reply.operation {
            1 => {
                self.active = true;
                // The service repeats its initial roster. Keep the user's selection
                // and any newer preview reply instead of resetting the screen.
                // The codec's historical `free` field is release261's TOTAL
                // character_slots. `capacity` is an unrelated header field.
                let capacity = reply.free.min(100);
                let free = capacity.saturating_sub(reply.characters.len() as u32);
                let changed = self.characters != reply.characters || self.capacity != capacity || self.free != free;
                self.capacity = capacity;
                self.free = free;
                self.characters = reply.characters;
                if self.characters.iter().any(|c| c.character == reply.character) {
                    self.equipment.entry(reply.character).or_insert(reply.equipment);
                }
                let valid = |id: &u32| self.characters.iter().any(|c| c.character == *id);
                let selected = self.selected.filter(valid).or_else(|| preferred.filter(valid))
                    .or_else(|| Some(reply.character).filter(valid)).or_else(|| self.characters.first().map(|c| c.character));
                if changed || self.selected != selected {
                    self.selected = selected;
                    self.entering = false;
                    self.first_row = self.first_row.min(self.characters.len().saturating_sub(4));
                    self.reveal_selected();
                    self.dialog = Dialog::None;
                    self.status.clear();
                    self.requested = None;
                    self.revision += 1;
                    self.preview_revision += 1;
                    self.equipment.retain(|id, _| self.characters.iter().any(|c| c.character == *id));
                }
                self.request_preview();
            }
            9 => {
                if !self.characters.iter().any(|c| c.character == reply.character) { return; }
                if self.equipment.get(&reply.character) != Some(&reply.equipment) {
                    self.equipment.insert(reply.character, reply.equipment);
                    if self.selected == Some(reply.character) {
                        self.preview_revision += 1;
                        self.revision += 1;
                    }
                }
            }
            4 => {
                self.entering = false;
                self.status = format!("This character cannot enter the world (server reason {}).", reply.deny_reason);
                self.revision += 1;
            }
            _ => {}
        }
    }

    pub fn editor(&mut self, defaults: &(String, u8, u8)) {
        if let Dialog::Delete { id, waiting: true, .. } = self.dialog {
            self.characters.retain(|c| c.character != id);
            self.equipment.remove(&id);
            self.selected = self.characters.first().map(|c| c.character);
            self.preview_revision += 1;
        }
        self.active = true;
        self.entering = false;
        self.dialog = Dialog::Create { name: defaults.0.clone(), class: defaults.1.min(3), gender: defaults.2.min(1), waiting: false };
        self.status.clear();
        self.revision += 1;
    }

    pub fn creation_refused(&mut self, reason: u8) {
        if let Dialog::Create { waiting, .. } = &mut self.dialog { *waiting = false; }
        self.status = match reason { 1 => "That name is already taken.", 2 => "Enter a character name.",
            3 => "That name contains invalid characters.", 7 => "The character could not be created. Return to the roster and try again.",
            _ => "The server refused this character. Try another name." }.into();
        self.revision += 1;
    }

    pub fn play(&mut self) {
        if self.offline { self.status = "Offline preview".into(); self.revision += 1; return; }
        if self.entering || self.dialog != Dialog::None { return; }
        if let Some(id) = self.current().map(|c| c.character) {
            self.requests.push(Request::Play(id));
            self.entering = true;
            self.status = "Entering the world...".into();
            self.revision += 1;
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct SelectionCamera { pub pitch: f32, pub yaw: f32, pub offset: [f32; 3], pub fov: f32 }

#[derive(Asset, TypePath, Deserialize)]
pub struct SelectionData {
    pub camera: SelectionCamera,
    pub slots: HashMap<String, Vec<u8>>,
    pub hair: HashMap<String, HashMap<String, String>>,
    pub beard: HashMap<String, HashMap<String, String>>,
}

#[derive(Default, TypePath)]
struct SelectionLoader;
impl AssetLoader for SelectionLoader {
    type Asset = SelectionData;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new(); r.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["selection.json"] }
}

#[derive(Resource)]
struct Scene { data: Handle<SelectionData>, shown: Option<Entity>, pending: Option<Entity>, revision: Option<u64> }

#[derive(Component)]
pub struct Preview;

pub struct SelectionPlugin;
impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>().init_asset::<SelectionData>().init_asset_loader::<SelectionLoader>()
            .add_systems(Startup, setup)
            .add_systems(Update, scene.after(crate::net::NetReceive).before(crate::net::NetSystems));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, replay.after(scene));
    }
}

/// Opt-in native renderer regression, matching the equipment-replay facility.
/// It consumes explicit actions from a local fixture; ordinary sessions never
/// create this resource or send any automatic selection request.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Deserialize)]
struct ReplayStep {
    frame: u32,
    select: Option<u32>,
    rotate: Option<f32>,
    capture: Option<String>,
    #[serde(default)] play: bool,
    #[serde(default)] quit: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Replay { loaded: bool, frame: u32, steps: std::collections::VecDeque<ReplayStep>, quit_at: Option<u32> }

#[cfg(not(target_arch = "wasm32"))]
fn replay(mut commands: Commands, mut state: Local<Replay>, mut selection: ResMut<Selection>, scene: Res<Scene>,
    loading: Res<crate::loading::LoadingScreen>, current: Option<Res<crate::map::CurrentMap>>) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    if !state.loaded {
        state.loaded = true;
        if let Ok(path) = std::env::var("DSOR_SELECTION_REPLAY") {
            match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
                Ok(steps) => state.steps = steps,
                Err(e) => error!("character selection replay: {e}"),
            }
        }
    }
    if state.steps.is_empty() && state.quit_at.is_none() { return; }
    if loading.active() || !current.is_some_and(|m| m.spawned) || (selection.active &&
        (scene.pending.is_some() || scene.revision != Some(selection.preview_revision))) { return; }
    state.frame += 1;
    if state.quit_at.is_some_and(|at| state.frame >= at) { commands.write_message(AppExit::Success); return; }
    while state.steps.front().is_some_and(|s| s.frame <= state.frame) {
        let step = state.steps.pop_front().unwrap();
        if let Some(id) = step.select { selection.choose(id); }
        if let Some(angle) = step.rotate { selection.angle = angle; }
        if step.play { selection.play(); }
        if let Some(path) = step.capture { commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path)); }
        if step.quit { state.quit_at = Some(state.frame + 30); }
        info!("character selection replay frame {}", state.frame);
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>, mut selection: ResMut<Selection>) {
    commands.insert_resource(Scene { data: assets.load("characters/selection.json"), shown: None, pending: None, revision: None });
    // Deterministic server-packet fixture, without authenticating or changing an account.
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(path) = std::env::var("DSOR_SELECTION_PREVIEW") {
        let result = (|| -> Option<()> {
            let packets: Vec<(String, usize)> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            selection.offline = true;
            for (hex, bits) in packets {
                let bytes: Vec<u8> = hex.as_bytes().chunks_exact(2).map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap_or(""), 16)).collect::<Result<_, _>>().ok()?;
                selection.receive(dsor_proto::messages::character_selection(&bytes, bits)?, None);
            }
            commands.insert_resource(crate::map::CurrentMap::new(MAP.into(), assets.load(format!("maps/{MAP}.map.json"))));
            Some(())
        })();
        if result.is_none() { error!("Invalid character-selection preview fixture"); }
    }
}

pub fn description(c: &SelectionCharacter, equipment: &[SelectionEquipment], data: &SelectionData, skins: &ItemSkins) -> CharacterDesc {
    let mut worn = Vec::new();
    for e in equipment {
        let parts = skins.appearances.get(&e.slot).or_else(|| skins.items.get(&e.template)).cloned().unwrap_or_default();
        // The wire list has no slot number, so resolve SlotType from the static
        // item table. Its order cannot identify a slot when others are empty.
        for &slot in data.slots.get(&e.template).into_iter().flatten() {
            worn.push(crate::appearance::Equipment { slot, skin_parts: parts.clone(), colour: (e.colour[3] > 0.0).then_some(e.colour) });
        }
    }
    worn = crate::appearance::normalize(worn);
    worn.dedup_by(|a, b| a.slot == b.slot);
    let mut desc = CharacterDesc { class: c.player_class.min(3) as u8, gender: c.gender.min(1) as u8,
        armament: crate::appearance::remote_armament(&worn, ""), equipment: worn, ..default() };
    let key = format!("{}_{}", desc.class_name(), if desc.gender == 1 { "female" } else { "male" });
    for (table, index) in [(&data.hair, c.hair), (&data.beard, c.beard)] {
        if let Some(part) = table.get(&key).and_then(|t| t.get(&index.to_string())) { desc.cosmetics.push(part.clone()); }
    }
    desc
}

#[allow(clippy::too_many_arguments)]
fn scene(mut commands: Commands, selection: Res<Selection>, mut scene: ResMut<Scene>,
    data: Res<Assets<SelectionData>>, skins: Res<Assets<ItemSkins>>, library: Res<CharacterLibrary>,
    mut previews: Query<(&CharacterAnim, &character::Character, &mut Visibility, &mut Transform), With<Preview>>,
    mut cameras: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<crate::highlight::HighlightCamera>, Without<Preview>)>) {
    if !selection.active {
        for id in [scene.shown.take(), scene.pending.take()].into_iter().flatten() { commands.entity(id).try_despawn(); }
        scene.revision = None;
        return;
    }
    let Some(data) = data.get(&scene.data) else { return };
    let cam = &data.camera;
    for (mut tf, mut projection) in &mut cameras {
        // Nebula stores the camera's offset along forward (-Z); Bevy stores
        // the camera position behind the portrait, looking down -Z.
        let rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw.to_radians(), cam.pitch.to_radians(), 0.0);
        *tf = Transform::from_translation(Vec3::new(cam.offset[0], cam.offset[1], -cam.offset[2])).with_rotation(rotation);
        if let Projection::Perspective(p) = &mut *projection { p.fov = cam.fov.to_radians(); }
    }
    if scene.revision != Some(selection.preview_revision) {
        if let (Some(c), Some(skins)) = (selection.current(), skins.get(&library.item_skins)) {
            if let Some(equipment) = selection.equipment.get(&c.character) {
                if let Some(old) = scene.pending.take() { commands.entity(old).try_despawn(); }
                let entity = character::spawn_character(&mut commands, description(c, equipment, data, skins), Transform::default());
                let mut anim = CharacterAnim::default();
                anim.state = character::AnimState::Named("CharSelectIdle".into());
                commands.entity(entity).insert((Preview, anim, Visibility::Hidden));
                scene.pending = Some(entity);
                scene.revision = Some(selection.preview_revision);
                info!("character preview: {} ({})", c.name, c.character);
            }
        } else if selection.current().is_none() {
            for id in [scene.shown.take(), scene.pending.take()].into_iter().flatten() { commands.entity(id).try_despawn(); }
        }
    }
    if let Some(id) = scene.pending {
        if let Ok((anim, character, mut visibility, _)) = previews.get_mut(id) {
            if anim.is_ready() && character.outfit_ready() {
                *visibility = Visibility::Inherited;
                if let Some(old) = scene.shown.replace(id) { commands.entity(old).try_despawn(); }
                scene.pending = None;
            }
        }
    }
    for (_, _, _, mut tf) in &mut previews { tf.rotation = Quat::from_rotation_y(selection.angle); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn roster() -> CharacterSelection {
        CharacterSelection { operation: 1, character: 1, capacity: 200_000_003, free: 5,
            characters: vec![SelectionCharacter { character: 1, name: "One".into(), ..default() },
                SelectionCharacter { character: 2, name: "Two".into(), ..default() }], ..default() }
    }
    #[test]
    fn browsing_and_duplicate_rosters_never_play_or_overwrite_a_newer_preview() {
        let mut state = Selection::default(); state.receive(roster(), None);
        assert_eq!((state.capacity, state.free), (5, 3));
        assert!(matches!(state.requests.as_slice(), [Request::Preview(1)]));
        state.requests.clear(); state.choose(2);
        let rev = state.preview_revision;
        state.receive(roster(), None);
        assert_eq!(state.selected, Some(2)); assert_eq!(state.preview_revision, rev);
        state.receive(CharacterSelection { operation: 9, character: 1, equipment: vec![SelectionEquipment { template: "late".into(), ..default() }], ..default() }, None);
        assert_eq!(state.preview_revision, rev, "late reply must not replace the chosen character");
        assert!(state.requests.iter().all(|r| matches!(r, Request::Preview(_))));
        state.requests.clear(); state.play(); state.play();
        assert!(matches!(state.requests.as_slice(), [Request::Play(2)]));
    }
    #[test]
    fn roster_refresh_and_rejected_creation_remain_interactive() {
        let mut state = Selection::default(); state.receive(roster(), Some(2));
        assert_eq!(state.selected, Some(2));
        let mut smaller = roster(); smaller.characters.pop(); smaller.free = 4;
        state.receive(smaller, None); assert_eq!(state.selected, Some(1));
        state.editor(&("Name".into(), 3, 1));
        state.creation_refused(1);
        assert!(matches!(state.dialog, Dialog::Create { waiting: false, class: 3, gender: 1, .. }));
        assert!(state.status.contains("already taken"));
    }
    #[test]
    fn preselected_character_and_preview_changes_stay_in_the_visible_rows() {
        let mut reply = roster();
        reply.characters = (1..=5).map(|character| SelectionCharacter { character, ..default() }).collect();
        let mut state = Selection::default(); state.receive(reply, Some(5));
        assert_eq!(state.selected, Some(5)); assert_eq!(state.first_row, 1);
        state.choose(1); assert_eq!(state.first_row, 0);
        state.choose(5); assert_eq!(state.first_row, 1);
    }
    #[test]
    fn preview_resolves_sparse_slots_transmog_dyes_and_two_handed_duplicates() {
        let data: SelectionData = serde_json::from_str(r#"{"camera":{"pitch":-3,"yaw":0,"offset":[0,2,-14.5],"fov":20},"slots":{"torso":[2],"staff":[5,6]},"hair":{"mage_female":{"3":"hair"}},"beard":{}}"#).unwrap();
        let skins: ItemSkins = serde_json::from_str(r#"{"items":{"torso":["coat"],"staff":["staff"]},"appearances":{"skin":["transmog"]}}"#).unwrap();
        let staff = SelectionEquipment { template: "staff".into(), ..default() };
        let desc = description(&SelectionCharacter { player_class: 1, gender: 1, hair: 3, ..default() },
            &[SelectionEquipment { template: "torso".into(), slot: "skin".into(), colour: [1.0, 0.0, 0.0, 1.0], ..default() }, staff.clone(), staff], &data, &skins);
        assert_eq!(desc.equipment.len(), 3); assert_eq!(desc.equipment[0].slot, 2);
        assert_eq!(desc.equipment[0].skin_parts, ["transmog"]);
        assert_eq!(desc.equipment[0].colour, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(desc.armament, 5); assert_eq!(desc.cosmetics, ["hair"]);
    }
}
