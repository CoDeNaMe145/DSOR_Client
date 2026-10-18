//! Original mousecursor textures, shared by the account screen, lobby and world.
//! Keep the native cursor: winit caches these images and moves them independently
//! of the render frame rate. The artwork's top-left tip is the click hotspot.
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageLoaderSettings;
use bevy::prelude::*;
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow};

use crate::actors::{monsters::Hovered, npc::Npc};
use crate::network::net::Net;
use crate::rendering::highlight::Highlighted;
use crate::screens::{loading::LoadingScreen, selection::Selection};
use crate::world::{drops::ItemHover, exits::Exit};

// Original artwork is 64×64; 42×42 is about 35% smaller.
const CURSOR_SIZE: u32 = 42;

#[derive(Clone, Copy)]
enum Kind { Default, Attack, Talk, Buy, Interact, Classic }

#[derive(Resource)]
struct Cursors([Handle<Image>; 6]);

impl Cursors {
    fn icon(&self, kind: Kind, images: &Assets<Image>) -> Option<CursorIcon> {
        let requested = &self.0[kind as usize];
        // While a contextual image loads (or if it is missing), keep the game's
        // default hand. Never ask winit to retry an unavailable image each frame.
        let handle = if images.contains(requested) { requested } else { &self.0[Kind::Default as usize] };
        images.contains(handle).then(|| CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle: handle.clone(), hotspot: (0, 0), ..default()
        })))
    }
}

pub(super) struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load)
            .add_systems(PostUpdate, (resize, update).chain().after(crate::rendering::highlight::pick));
    }
}

fn load(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(Cursors(["default", "attack", "talk", "buy", "interact", "arrow"].map(|name| {
        server.load_builder().with_settings(|settings: &mut ImageLoaderSettings| {
            // The OS needs CPU pixels even after the renderer has extracted assets.
            settings.asset_usage = RenderAssetUsages::MAIN_WORLD;
        }).load(format!("textures/mousecursor/{name}.png"))
    })));
}

fn resize(cursors: Res<Cursors>, mut images: ResMut<Assets<Image>>) {
    for handle in &cursors.0 {
        let Some(image) = images.get(handle) else { continue };
        if image.size() == UVec2::splat(CURSOR_SIZE) { continue; }
        let Ok(decoded) = image.clone().try_into_dynamic() else { continue };
        // Resample once before winit caches the native cursor. A cursor rect
        // would crop the artwork instead; the top-left hotspot stays at (0, 0).
        *images.get_mut(handle).unwrap() = Image::from_dynamic(
            decoded.thumbnail_exact(CURSOR_SIZE, CURSOR_SIZE),
            true,
            RenderAssetUsages::MAIN_WORLD,
        );
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update(
    mut commands: Commands,
    windows: Query<(Entity, &Window, Option<&CursorIcon>), With<PrimaryWindow>>,
    cursors: Res<Cursors>,
    images: Res<Assets<Image>>,
    ui: Res<super::input::UiInput>,
    login: Option<Res<crate::LoginGate>>,
    loading: Res<LoadingScreen>,
    selection: Option<Res<Selection>>,
    monsters: Res<Hovered>,
    items: Res<ItemHover>,
    highlighted: Res<Highlighted>,
    npcs: Query<&Npc>,
    exits: Query<(), With<Exit>>,
    net: Option<NonSend<Net>>,
    settings: Res<super::Settings>,
) {
    let Ok((entity, window, current)) = windows.single() else { return };
    let world_input = window.focused && window.cursor_position().is_some()
        && login.is_none() && !loading.active() && !selection.is_some_and(|s| s.active)
        && !ui.over_ui && !ui.blocks(MouseButton::Left)
        && !net.as_ref().is_some_and(|n| n.death.dead);
    let mut kind = Kind::Default;
    if world_input {
        if monsters.0.is_some() {
            kind = Kind::Attack;
        } else if items.0.is_some() {
            kind = Kind::Interact;
        } else if let Some((entity, _)) = highlighted.0 {
            if let Ok(npc) = npcs.get(entity) {
                let enabled = !npc.profile.no_talk && net.as_ref().is_none_or(|n|
                    n.npc_dialogue.infos.get(&npc.actor).is_none_or(|info| info.enabled));
                if enabled { kind = if npc.profile.shop.is_empty() { Kind::Talk } else { Kind::Buy }; }
            } else if exits.contains(entity) {
                kind = Kind::Interact;
            }
        }
    }
    // The original classiccursor flag forces Cursor::Arrow (9), bypassing
    // hover variants: rel261 CursorManager at 0x14028dbdf..0x14028dbfd,
    // DrasaCursor::SwitchCursor at 0x1405bd827 selects mousecursor/arrow.
    if settings.current.classic_cursor { kind = Kind::Classic; }
    if let Some(want) = cursors.icon(kind, &images) {
        if current != Some(&want) { commands.entity(entity).insert(want); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        let mut images = Assets::<Image>::default();
        let cursors = Cursors(std::array::from_fn(|_| images.add(Image::default())));
        let mut settings = super::super::Settings::default();
        settings.current = super::super::controls::settings::Preferences::default();
        app.insert_resource(images).insert_resource(cursors).insert_resource(settings)
            .init_resource::<super::super::input::UiInput>()
            .init_resource::<LoadingScreen>().init_resource::<Selection>()
            .init_resource::<Hovered>().init_resource::<ItemHover>().init_resource::<Highlighted>()
            .add_systems(PostUpdate, (resize, update).chain());
        let mut window = Window { focused: true, ..default() };
        window.set_cursor_position(Some(Vec2::new(100.0, 100.0)));
        let entity = app.world_mut().spawn((window, PrimaryWindow)).id();
        (app, entity)
    }

    fn expect(app: &App, window: Entity, kind: Kind) {
        let want = app.world().resource::<Cursors>().icon(kind, app.world().resource::<Assets<Image>>()).unwrap();
        assert_eq!(app.world().get::<CursorIcon>(window), Some(&want));
    }

    #[test]
    fn hover_cursors_yield_to_ui_and_non_gameplay_screens() {
        let (mut app, window) = app();
        app.update();
        expect(&app, window, Kind::Default);
        app.world_mut().resource_mut::<ItemHover>().0 = Some((window, window));
        app.update();
        expect(&app, window, Kind::Interact);
        app.world_mut().resource_mut::<Hovered>().0 = Some((window, 1));
        app.update();
        expect(&app, window, Kind::Attack);
        app.world_mut().resource_mut::<super::super::input::UiInput>().over_ui = true;
        app.update();
        expect(&app, window, Kind::Default);
        app.world_mut().resource_mut::<super::super::input::UiInput>().over_ui = false;
        app.world_mut().insert_resource(crate::LoginGate);
        app.update();
        expect(&app, window, Kind::Default);
        app.world_mut().remove_resource::<crate::LoginGate>();
        app.world_mut().resource_mut::<LoadingScreen>().connecting();
        app.update();
        expect(&app, window, Kind::Default);
        app.world_mut().resource_mut::<LoadingScreen>().cancel();
        app.world_mut().resource_mut::<Selection>().active = true;
        app.update();
        expect(&app, window, Kind::Default);
        app.world_mut().resource_mut::<Selection>().active = false;
        app.update();
        expect(&app, window, Kind::Attack);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        expect(&app, window, Kind::Default);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(None);
        app.update();
        expect(&app, window, Kind::Default);
    }

    #[test]
    fn npc_cursors_only_advertise_available_interactions() {
        let (mut app, window) = app();
        let npc = app.world_mut().spawn(Npc::default()).id();
        app.world_mut().resource_mut::<Highlighted>().0 = Some((npc, [0; 4]));
        app.update();
        expect(&app, window, Kind::Talk);
        app.world_mut().get_mut::<Npc>(npc).unwrap().profile.shop = "merchant".into();
        app.update();
        expect(&app, window, Kind::Buy);
        app.world_mut().get_mut::<Npc>(npc).unwrap().profile.no_talk = true;
        app.update();
        expect(&app, window, Kind::Default);
        app.world_mut().resource_mut::<Highlighted>().0 = None;
        app.update();
        expect(&app, window, Kind::Default);
    }

    #[test]
    fn classic_cursor_overrides_hover_and_switches_back_without_restarting() {
        let (mut app, window) = app();
        app.world_mut().resource_mut::<super::super::Settings>().current.classic_cursor = true;
        app.update();
        expect(&app, window, Kind::Classic);
        app.world_mut().resource_mut::<Hovered>().0 = Some((window, 1));
        app.update();
        expect(&app, window, Kind::Classic);
        app.world_mut().insert_resource(crate::LoginGate);
        app.update();
        expect(&app, window, Kind::Classic);
        app.world_mut().remove_resource::<crate::LoginGate>();
        app.world_mut().resource_mut::<super::super::Settings>().current.classic_cursor = false;
        app.update();
        expect(&app, window, Kind::Attack);
        app.world_mut().resource_mut::<Hovered>().0 = None;
        app.update();
        expect(&app, window, Kind::Default);
    }

    #[test]
    fn unavailable_context_cursor_keeps_game_hand_until_loaded() {
        let (mut app, window) = app();
        let attack = app.world().resource::<Cursors>().0[Kind::Attack as usize].clone();
        let image = app.world_mut().resource_mut::<Assets<Image>>().remove(attack.id()).unwrap();
        app.world_mut().resource_mut::<Hovered>().0 = Some((window, 1));
        app.update();
        expect(&app, window, Kind::Default);
        app.world_mut().resource_mut::<Assets<Image>>().insert(attack.id(), image).unwrap();
        app.update();
        expect(&app, window, Kind::Attack);
    }
}
