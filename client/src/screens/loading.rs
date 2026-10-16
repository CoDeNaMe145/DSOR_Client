//! Shared loading overlay for startup asset batches, connection hand-offs and
//! map streaming. The artwork stays resident, so changing maps needs no UI IO.
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use bevy::asset::{LoadState, RecursiveDependencyLoadState, UntypedHandle};
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems, render_resource::PipelineCache};
use bevy::window::PrimaryWindow;

use crate::{actors::character::CharacterAnim, world::map::CurrentMap, network::net::LocalPlayer};

const SPLASH: &str = "textures/ui/splashart_crunch.png";
const BAR: &str = "textures/ui/statusbar_connenctscreen_new.png";

enum Work {
    Assets(Vec<UntypedHandle>),
    Connecting,
    Map(String),
}

#[derive(Resource, Default)]
pub struct LoadingScreen {
    work: Option<Work>,
    label: String,
    progress: f32,
    stable: f32,
    completed: f32,
    error: Option<String>,
}

impl LoadingScreen {
    pub fn active(&self) -> bool { self.work.is_some() }

    /// Future preloaders can submit any typed asset handles via `.untyped()`.
    /// Dependencies and failures are counted too; failed optional assets must
    /// not keep a batch open forever.
    pub fn begin_assets(&mut self, label: impl Into<String>, handles: Vec<UntypedHandle>) {
        self.begin(Work::Assets(handles), label.into());
    }

    pub fn connecting(&mut self) { self.begin(Work::Connecting, "Connecting...".into()); }

    pub fn map(&mut self, name: String) { self.begin(Work::Map(name), "Loading map...".into()); }

    fn begin(&mut self, work: Work, label: String) {
        *self = Self { work: Some(work), label, ..default() };
    }

    pub fn cancel(&mut self) { *self = Self::default(); }

    fn advance(&mut self, fraction: f32, ready: bool, renderer_ready: bool, dt: f32) {
        self.progress = self.progress.max(fraction.clamp(0.0, 0.98));
        if ready && renderer_ready {
            self.stable += dt.min(0.1);
        } else {
            self.stable = 0.0;
            self.completed = 0.0;
        }
        // Allow scene extraction, mesh uploads and shader compilation to settle
        // before exposing the world. Briefly display the completed bar as well.
        if self.stable >= 0.25 {
            self.progress = 1.0;
            self.completed += dt.min(0.1);
            if self.completed >= 0.2 { self.cancel(); }
        }
    }
}

/// Used only on input systems: simulation and asset streaming must keep running.
pub fn gameplay_ready(login: Option<Res<crate::LoginGate>>, loading: Res<LoadingScreen>, selection: Option<Res<crate::screens::selection::Selection>>) -> bool {
    login.is_none() && !loading.active() && !selection.is_some_and(|s| s.active)
}

#[derive(Resource, Clone, Default)]
struct RendererBusy(Arc<AtomicBool>);
#[derive(Resource)]
struct Art { splash: Handle<Image>, bar: Handle<Image>, font: Handle<Font> }
#[derive(Component)] struct Root;
#[derive(Component)] struct Splash;
#[derive(Component)] struct Bar;
#[derive(Component)] struct Fill;
#[derive(Component)] struct Caption;
#[derive(Component)] struct ExitButton;

pub struct LoadingPlugin;
impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        let busy = RendererBusy::default();
        app.insert_resource(busy.clone())
            .init_resource::<LoadingScreen>()
            .add_systems(Startup, setup)
            .add_systems(PreUpdate, track_map.after(crate::network::net::NetReceive))
            .add_systems(PostUpdate, (update_progress, paint, exit_failed).chain().before(bevy::ui::UiSystems::Layout));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(busy).add_systems(Render,
                renderer_busy.after(RenderSystems::Render).before(RenderSystems::Cleanup));
        }
    }
}

fn renderer_busy(cache: Res<PipelineCache>, busy: Res<RendererBusy>) {
    busy.0.store(cache.waiting_pipelines().next().is_some(), Ordering::Relaxed);
}

fn setup(mut commands: Commands, assets: Res<AssetServer>, mut loading: ResMut<LoadingScreen>) {
    let art = Art { splash: assets.load(SPLASH), bar: assets.load(BAR), font: assets.load("fonts/tahoma.ttf") };
    loading.begin_assets("Preparing client...", vec![art.splash.clone().untyped(), art.bar.clone().untyped(), art.font.clone().untyped()]);
    commands.spawn((Root, Name::new("Loading screen"), GlobalZIndex(20000),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100),
            overflow: Overflow::clip(), ..default() }, BackgroundColor(Color::BLACK)))
        .with_children(|root| {
            root.spawn((Splash, ImageNode::new(art.splash.clone()), Node { position_type: PositionType::Absolute, ..default() }));
            root.spawn((Node { position_type: PositionType::Absolute, bottom: percent(5), width: percent(100),
                flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(12),
                padding: UiRect::axes(px(16), px(18)), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55))))
                .with_children(|footer| {
                    footer.spawn((Caption, Text::new("Preparing client..."), TextFont { font: art.font.clone().into(), font_size: FontSize::Px(18.0), ..default() },
                        TextColor(Color::srgb(0.96, 0.91, 0.79))));
                    footer.spawn((Bar, Node { position_type: PositionType::Relative, width: px(760), height: px(28), ..default() },
                        ImageNode { image: art.bar.clone(), rect: Some(Rect::new(0.0, 28.0, 953.0, 61.0)), ..default() }))
                        .with_children(|bar| {
                            bar.spawn((Fill, Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: px(0), height: percent(100), ..default() },
                                ImageNode { image: art.bar.clone(), rect: Some(Rect::new(0.0, 0.0, 953.0, 28.0)), ..default() }));
                        });
                    footer.spawn((ExitButton, Button, Visibility::Hidden, Node { display: Display::None, padding: UiRect::axes(px(22), px(8)), ..default() },
                        BackgroundColor(Color::srgb(0.14, 0.12, 0.08))))
                        .with_children(|button| {
                            button.spawn((Text::new("Close client"), TextFont { font: art.font.clone().into(), font_size: FontSize::Px(16.0), ..default() }));
                        });
                });
        });
    commands.insert_resource(art);
}

fn track_map(current: Option<Res<CurrentMap>>, mut loading: ResMut<LoadingScreen>,
    mut seen: Local<Option<(String, AssetId<crate::world::map::MapManifest>)>>) {
    let Some(current) = current else {
        *seen = None;
        if matches!(loading.work, Some(Work::Map(_))) { loading.cancel(); }
        return;
    };
    let key = (current.name.clone(), current.manifest.id());
    if seen.as_ref() != Some(&key) {
        *seen = Some(key);
        loading.map(current.name.clone());
        info!("loading screen: map {}", current.name);
    }
}

fn settled(assets: &AssetServer, handle: &UntypedHandle) -> bool {
    matches!(assets.get_load_state(handle.id()), Some(LoadState::Failed(_))) ||
        matches!(assets.get_recursive_dependency_load_state(handle.id()),
            Some(RecursiveDependencyLoadState::Loaded | RecursiveDependencyLoadState::Failed(_)))
}

#[allow(clippy::too_many_arguments)]
fn update_progress(mut commands: Commands, mut loading: ResMut<LoadingScreen>, current: Option<Res<CurrentMap>>,
    assets: Res<AssetServer>, player: Query<&CharacterAnim, With<LocalPlayer>>,
    online: Option<Res<crate::network::net::NetActive>>, failure: Option<Res<crate::network::net::ConnectionFailure>>,
    art: Res<Art>, busy: Res<RendererBusy>, time: Res<Time<Real>>, props: Option<Res<crate::world::props::PropScene>>,
    selection: Option<Res<crate::screens::selection::Selection>>, previews: Query<(&CharacterAnim, &crate::actors::character::Character), With<crate::screens::selection::Preview>>) {
    if !loading.active() { return; }
    let (fraction, ready) = match loading.work.as_ref().unwrap() {
        Work::Assets(handles) => {
            let done = handles.iter().filter(|h| settled(&assets, h)).count();
            (if handles.is_empty() { 1.0 } else { done as f32 / handles.len() as f32 }, done == handles.len())
        }
        Work::Connecting => (0.0, false),
        Work::Map(name) => {
            let Some(current) = current.as_ref().filter(|m| m.name == *name) else { return; };
            if matches!(assets.get_load_state(&current.manifest), Some(LoadState::Failed(_))) {
                loading.error = Some("Unable to load this map. Check the installed assets.".into());
                commands.insert_resource(crate::network::net::ConnectionFailure("Unable to load the map. Check the installed assets.".into()));
                return;
            }
            let (done, total) = current.progress(&assets);
            let map_ready = current.spawned && done == total && props.as_ref().is_none_or(|p| p.ready(current, &assets));
            let player_ready = if name == crate::screens::selection::MAP {
                selection.as_ref().is_some_and(|s| s.active && (s.characters.is_empty() || previews.iter().any(|(a, c)| a.is_ready() && c.outfit_ready())))
            } else { online.is_none() || player.iter().any(CharacterAnim::is_ready) };
            let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 * 0.9 };
            (fraction + if map_ready && player_ready { 0.1 } else { 0.0 }, map_ready && player_ready)
        }
    };
    if !matches!(loading.work, Some(Work::Assets(_))) {
        if let Some(failure) = failure { loading.error = Some(failure.0.clone()); return; }
    }
    let art_ready = [&art.splash, &art.bar].iter().all(|h| settled(&assets, &(*h).clone().untyped()));
    loading.advance(fraction, ready && art_ready, !busy.0.load(Ordering::Relaxed), time.delta_secs());
}

fn cover(view: Vec2, image: Vec2) -> Vec2 { image * (view.x / image.x).max(view.y / image.y) }

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn paint(loading: Res<LoadingScreen>, windows: Query<&Window, With<PrimaryWindow>>,
    mut root: Query<&mut Visibility, (With<Root>, Without<ExitButton>, Without<Fill>)>,
    mut splash: Query<&mut Node, (With<Splash>, Without<Bar>, Without<Fill>, Without<ExitButton>)>,
    mut bars: Query<&mut Node, (With<Bar>, Without<Splash>, Without<Fill>, Without<ExitButton>)>,
    mut fills: Query<(&mut Node, &mut ImageNode, &mut Visibility), (With<Fill>, Without<Bar>, Without<Splash>, Without<ExitButton>, Without<Root>)>,
    mut text: Query<&mut Text, With<Caption>>,
    mut exit: Query<(&mut Visibility, &mut Node), (With<ExitButton>, Without<Root>, Without<Fill>, Without<Splash>, Without<Bar>)>) {
    for mut visibility in &mut root { *visibility = if loading.active() { Visibility::Inherited } else { Visibility::Hidden }; }
    if !loading.active() { return; }
    let Ok(window) = windows.single() else { return; };
    let view = Vec2::new(window.width(), window.height());
    let size = cover(view, Vec2::new(2048.0, 1024.0));
    for mut node in &mut splash {
        node.width = px(size.x); node.height = px(size.y);
        node.left = px((view.x - size.x) * 0.5); node.top = px((view.y - size.y) * 0.5);
    }
    let width = (view.x * 0.72).min(953.0);
    for mut node in &mut bars { node.width = px(width); node.height = px(width * 33.0 / 953.0); }
    for (mut node, mut image, mut visibility) in &mut fills {
        node.width = px(width * loading.progress);
        image.rect = Some(Rect::new(0.0, 0.0, 953.0 * loading.progress.max(0.001), 28.0));
        *visibility = if loading.progress > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
    }
    for mut text in &mut text {
        **text = loading.error.clone().unwrap_or_else(|| {
            if loading.progress >= 1.0 { "Ready".into() }
            else if matches!(loading.work, Some(Work::Connecting)) { loading.label.clone() }
            else { format!("{}  {}%", loading.label, (loading.progress * 100.0) as u32) }
        });
    }
    for (mut visibility, mut node) in &mut exit {
        *visibility = if loading.error.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        node.display = if loading.error.is_some() { Display::Flex } else { Display::None };
    }
}

fn exit_failed(loading: Res<LoadingScreen>, interactions: Query<&Interaction, (With<ExitButton>, Changed<Interaction>)>, mut exit: MessageWriter<AppExit>) {
    if loading.error.is_some() && interactions.iter().any(|i| *i == Interaction::Pressed) { exit.write(AppExit::Success); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_waits_for_map_and_renderer_then_resets_for_next_map() {
        let mut loading = LoadingScreen::default();
        loading.begin(Work::Map("first".into()), "Loading map...".into());
        for _ in 0..20 { loading.advance(1.0, true, false, 0.1); }
        assert!(loading.active()); assert!(loading.progress < 1.0);
        for _ in 0..10 { loading.advance(1.0, false, true, 0.1); }
        assert!(loading.active());
        for _ in 0..5 { loading.advance(1.0, true, true, 0.1); }
        assert!(!loading.active());
        loading.begin(Work::Map("second".into()), "Loading map...".into());
        assert_eq!(loading.progress, 0.0); assert!(loading.active());
    }
    #[test]
    fn splash_covers_wide_and_tall_windows_without_stretching() {
        for view in [Vec2::new(1280.0, 720.0), Vec2::new(2560.0, 1080.0), Vec2::new(800.0, 1000.0)] {
            let size = cover(view, Vec2::new(2048.0, 1024.0));
            assert!(size.x >= view.x && size.y >= view.y);
            assert_eq!(size.x / size.y, 2.0);
        }
    }
}
