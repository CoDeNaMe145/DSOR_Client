//! Shared loading overlay for startup asset batches, connection hand-offs and
//! map streaming. The artwork stays resident, so changing maps needs no UI IO.
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use bevy::asset::{LoadState, RecursiveDependencyLoadState, UntypedHandle};
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems, render_resource::PipelineCache};
use bevy::window::PrimaryWindow;

use crate::{actors::character::CharacterAnim, world::map::CurrentMap, network::net::LocalPlayer};

pub(crate) mod readiness;

const SPLASH: &str = "textures/ui/splashart_crunch.png";
const BAR: &str = "textures/ui/statusbar_connenctscreen_new.png";
/// After this long with neither the bar nor its step moving, the screen says
/// the game is still working;
const STILL_WORKING_SECS: f32 = 15.0;
/// after this long it offers to enter the world anyway, or to close the client.
/// FAILURE: a rare stall (a count that never met its total) held Kingshill at
///   98% with a still screen, and players took the game for frozen.
const OFFER_SECS: f32 = 45.0;
/// The status line's words for preparing graphics (its own hint).
const GRAPHICS: &str = "Preparing graphics";
/// The shine's width (percent of the bar) and the seconds one crossing takes.
const SHINE_WIDTH: f32 = 14.0;
const SHINE_SECS: f32 = 1.8;
const BUTTON: Color = Color::srgb(0.14, 0.12, 0.08);
const BUTTON_HOVER: Color = Color::srgb(0.26, 0.21, 0.12);

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
    /// What it is doing now, in the player's words (the line under the caption).
    detail: String,
    /// Seconds on this screen, and since the bar or the detail last moved.
    elapsed: f32,
    stalled: f32,
    /// The world is there to enter by hand (the map spawned, our character in).
    enterable: bool,
}

impl LoadingScreen {
    pub fn active(&self) -> bool { self.work.is_some() }

    pub(crate) fn map_active(&self) -> bool { self.error.is_none() && matches!(self.work, Some(Work::Map(_))) }

    pub(crate) fn fail(&mut self, message: String) { self.error = Some(message); }

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
        if self.error.is_some() { return; }
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

    /// The bar's whole percent, as the caption shows it.
    fn percent(&self) -> u32 { (self.progress * 100.0) as u32 }

    /// Notes the step now under way: a new step or a higher percent is
    /// movement; time without either is a stall.
    fn note(&mut self, detail: String, enterable: bool, moved: bool, dt: f32) {
        self.elapsed += dt;
        if moved || detail != self.detail { self.stalled = 0.0; } else { self.stalled += dt; }
        self.detail = detail;
        self.enterable = enterable;
    }

    /// The line under the status, once a stall is long: that the game still
    /// works, then what can be done.
    fn hint(&self) -> Option<&'static str> {
        if self.error.is_some() || self.progress >= 1.0 || self.stalled < STILL_WORKING_SECS { return None; }
        let offer = self.stalled >= OFFER_SECS;
        Some(match (self.detail.starts_with(GRAPHICS), offer, self.enterable) {
            (true, false, _) => "The first start prepares the graphics for your card. This can take a few minutes.",
            (false, false, _) => "This is taking longer than usual, but the game is still working.",
            (_, true, true) => "Still not finished. You can enter now; anything missing appears as you play.",
            (_, true, false) => "Still not finished. You can keep waiting, or close the client and try again.",
        })
    }

    /// Which buttons show: (Enter anyway, Close client).
    fn offers(&self) -> (bool, bool) {
        let stalled = self.error.is_none() && self.progress < 1.0 && self.stalled >= OFFER_SECS;
        (stalled && self.enterable, stalled || self.error.is_some())
    }
}

/// A readiness stage (readiness::check) in the player's words.
fn step_words(stage: &str) -> &'static str {
    if stage.starts_with("map and class assets") { "Loading your skills and effects" }
    else if stage.starts_with("navigation") || stage.starts_with("ambience") { "Preparing the area" }
    else if stage.starts_with("nearby NPC") { "Placing the people nearby" }
    else if stage.starts_with("scene") { "Loading scenery" }
    else if stage.starts_with("appearance of") { "Waiting for other players' appearances" }
    else if stage.starts_with("appearance") { "Dressing your character" }
    else if stage.starts_with("first animation") { "Preparing animations" }
    else { "Preparing the world" }
}

/// Used only on input systems: simulation and asset streaming must keep running.
pub fn gameplay_ready(login: Option<Res<crate::LoginGate>>, loading: Res<LoadingScreen>, selection: Option<Res<crate::screens::selection::Selection>>) -> bool {
    login.is_none() && !loading.active() && !selection.is_some_and(|s| s.active)
}

#[derive(Resource, Clone, Default)]
pub(crate) struct RendererBusy(Arc<AtomicBool>);
#[derive(Resource)]
pub(crate) struct Art { splash: Handle<Image>, bar: Handle<Image>, font: Handle<Font> }
#[derive(Component)] struct Root;
#[derive(Component)] struct Splash;
#[derive(Component)] struct Bar;
#[derive(Component)] struct Fill;
#[derive(Component)] struct Caption;
/// The line under the caption (LoadingScreen::detail and the time taken).
#[derive(Component)] struct Detail;
/// A light crossing the bar while work goes on: the screen is not frozen.
#[derive(Component)] struct Shine;
/// What shows only sometimes: the stall's hint and the two buttons.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Reveal { Hint, Enter, Exit }

pub struct LoadingPlugin;
impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        let busy = RendererBusy::default();
        app.insert_resource(busy.clone())
            .init_resource::<LoadingScreen>()
            .init_resource::<readiness::SceneReadiness>()
            .add_systems(Startup, setup)
            .add_systems(PreUpdate, track_map.after(crate::network::net::NetReceive))
            .add_systems(PostUpdate, readiness::check.after(TransformSystems::Propagate).before(update_progress))
            .add_systems(PostUpdate, (update_progress, paint, paint_status, buttons).chain().before(bevy::ui::UiSystems::Layout))
            .add_plugins(crate::rendering::warmup::WarmupPlugin);
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
                    let font = |size: f32| TextFont { font: art.font.clone().into(), font_size: FontSize::Px(size), ..default() };
                    footer.spawn((Caption, Text::new("Preparing client..."), font(18.0), TextColor(Color::srgb(0.96, 0.91, 0.79))));
                    footer.spawn((Detail, Text::new(""), font(14.0), TextColor(Color::srgb(0.80, 0.76, 0.66))));
                    footer.spawn((Bar, Node { position_type: PositionType::Relative, width: px(760), height: px(28), overflow: Overflow::clip(), ..default() },
                        ImageNode { image: art.bar.clone(), rect: Some(Rect::new(0.0, 28.0, 953.0, 61.0)), ..default() }))
                        .with_children(|bar| {
                            bar.spawn((Fill, Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: px(0), height: percent(100), ..default() },
                                ImageNode { image: art.bar.clone(), rect: Some(Rect::new(0.0, 0.0, 953.0, 28.0)), ..default() }));
                            // A soft gleam inside the track (the art's frame is
                            // the outer quarter above and below).
                            let clear = Color::srgba(1.0, 0.95, 0.8, 0.0);
                            bar.spawn((Shine, Visibility::Hidden, Node { position_type: PositionType::Absolute, left: percent(-SHINE_WIDTH), top: percent(24),
                                width: percent(SHINE_WIDTH), height: percent(52), ..default() },
                                bevy::ui::BackgroundGradient::from(bevy::ui::LinearGradient::to_right(vec![bevy::ui::ColorStop::auto(clear),
                                    bevy::ui::ColorStop::auto(Color::srgba(1.0, 0.95, 0.8, 0.35)), bevy::ui::ColorStop::auto(clear)]))));
                        });
                    footer.spawn((Reveal::Hint, Text::new(""), font(14.0), TextColor(Color::srgb(1.0, 0.85, 0.45)), Visibility::Hidden,
                        Node { display: Display::None, ..default() }));
                    footer.spawn(Node { flex_direction: FlexDirection::Row, column_gap: px(14), ..default() }).with_children(|row| {
                        for (reveal, label) in [(Reveal::Enter, "Enter anyway"), (Reveal::Exit, "Close client")] {
                            row.spawn((reveal, Button, Visibility::Hidden, Node { display: Display::None, padding: UiRect::axes(px(22), px(8)), ..default() },
                                BackgroundColor(BUTTON)))
                                .with_children(|button| { button.spawn((Text::new(label), font(16.0))); });
                        }
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

pub(crate) fn asset_failed(assets: &AssetServer, id: bevy::asset::UntypedAssetId) -> bool {
    matches!(assets.get_load_state(id), Some(LoadState::Failed(_))) ||
        matches!(assets.get_recursive_dependency_load_state(id), Some(RecursiveDependencyLoadState::Failed(_)))
}

pub(crate) fn asset_settled(assets: &AssetServer, id: bevy::asset::UntypedAssetId) -> bool {
    asset_failed(assets, id) ||
        matches!(assets.get_recursive_dependency_load_state(id),
            Some(RecursiveDependencyLoadState::Loaded | RecursiveDependencyLoadState::Failed(_)))
}

fn settled(assets: &AssetServer, handle: &UntypedHandle) -> bool { asset_settled(assets, handle.id()) }

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_progress(mut commands: Commands, mut loading: ResMut<LoadingScreen>, current: Option<Res<CurrentMap>>,
    assets: Res<AssetServer>, player: Query<(&CharacterAnim, &crate::actors::character::Character), With<LocalPlayer>>,
    online: Option<Res<crate::network::net::NetActive>>, failure: Option<Res<crate::network::net::ConnectionFailure>>,
    art: Res<Art>, busy: Res<RendererBusy>, time: Res<Time<Real>>, props: Option<Res<crate::world::props::PropScene>>,
    selection: Option<Res<crate::screens::selection::Selection>>, previews: Query<(&CharacterAnim, &crate::actors::character::Character), With<crate::screens::selection::Preview>>,
    readiness: Res<readiness::SceneReadiness>, warmup: Res<crate::rendering::warmup::Warmup>, mut report_at: Local<f64>) {
    if !loading.active() { return; }
    let renderer_ready = !busy.0.load(Ordering::Relaxed);
    let (fraction, ready, detail, enterable) = match loading.work.as_ref().unwrap() {
        Work::Assets(handles) => {
            let done = handles.iter().filter(|h| settled(&assets, h)).count();
            (if handles.is_empty() { 1.0 } else { done as f32 / handles.len() as f32 }, done == handles.len(),
                format!("Loading game files: {done} of {}", handles.len()), false)
        }
        Work::Connecting => (0.0, false, "Waiting for the server".to_string(), false),
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
            } else { (online.is_none() && player.is_empty()) || player.iter().any(|(a, c)| a.is_ready() && c.outfit_ready()) };
            let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 * 0.75 };
            let ready = map_ready && player_ready && readiness.ready && warmup.ready();
            // The first step not done yet, in the order the bar fills.
            let detail = if !current.spawned || done < total {
                if total == 0 { "Loading the world".to_string() } else { format!("Loading the world: {done} of {total} models") }
            } else if !map_ready { "Placing the world's objects".to_string() }
            else if !readiness.ready { step_words(&readiness.stage).to_string() }
            else if !player_ready { "Dressing your character".to_string() }
            else if !warmup.ready() || !renderer_ready { GRAPHICS.to_string() }
            else { "Almost there".to_string() };
            let enterable = current.spawned && (name == crate::screens::selection::MAP || online.is_none() || !player.is_empty());
            (fraction + readiness.fraction * 0.15 + if warmup.ready() { 0.1 } else { 0.0 }, ready, detail, enterable)
        }
    };
    if !matches!(loading.work, Some(Work::Assets(_))) {
        if let Some(failure) = failure { loading.error = Some(failure.0.clone()); return; }
    }
    let art_ready = [&art.splash, &art.bar].iter().all(|h| settled(&assets, &(*h).clone().untyped()));
    let was_map = loading.map_active();
    if was_map && time.elapsed_secs_f64() - *report_at > 5. {
        *report_at = time.elapsed_secs_f64();
        info!("loading readiness: {}, progress {:.0}%", readiness.stage, fraction * 100.);
    }
    let shown = loading.percent();
    let hinted = loading.hint().is_some();
    loading.advance(fraction, ready && art_ready, renderer_ready, time.delta_secs());
    if was_map && !loading.active() { info!("loading screen: ready (scene, class assets and rendering warm-up complete)"); }
    if !loading.active() { return; }
    let moved = loading.percent() > shown;
    loading.note(detail, enterable, moved, time.delta_secs());
    if !hinted && loading.hint().is_some() {
        warn!("loading screen: no progress for {STILL_WORKING_SECS} s at {}% ({}; readiness: {})",
            loading.percent(), loading.detail, readiness.stage);
    }
}

fn cover(view: Vec2, image: Vec2) -> Vec2 { image * (view.x / image.x).max(view.y / image.y) }

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn paint(loading: Res<LoadingScreen>, windows: Query<&Window, With<PrimaryWindow>>,
    mut root: Query<&mut Visibility, (With<Root>, Without<Fill>)>,
    mut splash: Query<&mut Node, (With<Splash>, Without<Bar>, Without<Fill>)>,
    mut bars: Query<&mut Node, (With<Bar>, Without<Splash>, Without<Fill>)>,
    mut fills: Query<(&mut Node, &mut ImageNode, &mut Visibility), (With<Fill>, Without<Bar>, Without<Splash>, Without<Root>)>,
    mut text: Query<&mut Text, With<Caption>>) {
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
        let caption = loading.error.clone().unwrap_or_else(|| {
            if loading.progress >= 1.0 { "Ready".into() }
            else if matches!(loading.work, Some(Work::Connecting)) { loading.label.clone() }
            else { format!("{}  {}%", loading.label, loading.percent()) }
        });
        text.set_if_neq(Text(caption));
    }
}

/// The status line (with the time taken, past a few seconds), the stall's
/// hint and buttons, and the shine crossing the bar while work goes on.
/// Texts are written only when they change; the shine moves every frame,
/// as long as this screen shows.
#[allow(clippy::type_complexity)]
fn paint_status(loading: Res<LoadingScreen>, time: Res<Time<Real>>,
    mut texts: Query<(&mut Text, Has<Detail>), Or<(With<Detail>, With<Reveal>)>>,
    mut shown: Query<(&Reveal, &mut Visibility, &mut Node), Without<Shine>>,
    mut shine: Query<(&mut Node, &mut Visibility), (With<Shine>, Without<Reveal>)>) {
    if !loading.active() { return; }
    let detail = if loading.error.is_some() || loading.progress >= 1.0 { String::new() }
        else if loading.elapsed >= 5.0 { format!("{}  ({} s)", loading.detail, loading.elapsed as u32) }
        else { loading.detail.clone() };
    let hint = loading.hint();
    for (mut text, is_detail) in &mut texts {
        let value = if is_detail { detail.clone() } else { hint.unwrap_or_default().to_string() };
        text.set_if_neq(Text(value));
    }
    let (enter, exit) = loading.offers();
    for (reveal, mut visibility, mut node) in &mut shown {
        let on = match reveal { Reveal::Hint => hint.is_some(), Reveal::Enter => enter, Reveal::Exit => exit };
        visibility.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
        let display = if on { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    let working = loading.error.is_none() && loading.progress < 1.0;
    for (mut node, mut visibility) in &mut shine {
        visibility.set_if_neq(if working { Visibility::Inherited } else { Visibility::Hidden });
        if !working { continue; }
        // Eased across, from just off the left end to just off the right.
        let t = (time.elapsed_secs() / SHINE_SECS).fract();
        let eased = t * t * (3.0 - 2.0 * t);
        node.left = percent(-SHINE_WIDTH + eased * (100.0 + SHINE_WIDTH));
    }
}

/// Enter anyway (a stall, the world there) closes this screen; Close client
/// (a stall or a failure) quits. The buttons light up under the pointer.
fn buttons(mut loading: ResMut<LoadingScreen>, mut interactions: Query<(&Reveal, &Interaction, &mut BackgroundColor), Changed<Interaction>>,
    mut exit: MessageWriter<AppExit>) {
    for (reveal, interaction, mut colour) in &mut interactions {
        colour.0 = if *interaction == Interaction::None { BUTTON } else { BUTTON_HOVER };
        if *interaction != Interaction::Pressed { continue; }
        let (enter, close) = loading.offers();
        match reveal {
            Reveal::Enter if enter => {
                warn!("loading screen: entered by the player after {:.0} s without progress at {}% ({})",
                    loading.stalled, loading.percent(), loading.detail);
                loading.cancel();
            }
            Reveal::Exit if close => { exit.write(AppExit::Success); }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_loading_stays_visible_and_stops_map_work_until_reset() {
        let mut loading = LoadingScreen::default();
        loading.map("first".into());
        loading.fail("failed graphics".into());
        for _ in 0..10 { loading.advance(1.0, true, true, 0.1); }
        assert!(loading.active());
        assert!(!loading.map_active());
        assert_eq!(loading.error.as_deref(), Some("failed graphics"));
        loading.map("second".into());
        assert!(loading.map_active());
        assert!(loading.error.is_none());
    }

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
    /// A stall shows that the game still works, then offers a way on; movement
    /// (a higher percent or a new step) puts it all away again.
    #[test]
    fn a_stall_says_the_game_still_works_then_offers_a_way_on() {
        let mut loading = LoadingScreen::default();
        loading.map("a0200_kingscity".into());
        loading.advance(0.5, false, true, 0.1);
        let step = |l: &mut LoadingScreen, detail: &str, secs: f32| for _ in 0..(secs * 10.0) as u32 { l.note(detail.into(), true, false, 0.1); };
        step(&mut loading, "Placing the world's objects", STILL_WORKING_SECS - 1.0);
        assert_eq!((loading.hint(), loading.offers()), (None, (false, false)), "a short wait is only a wait");
        step(&mut loading, "Placing the world's objects", 2.0);
        assert_eq!(loading.hint(), Some("This is taking longer than usual, but the game is still working."));
        assert_eq!(loading.offers(), (false, false));
        step(&mut loading, "Placing the world's objects", OFFER_SECS);
        assert!(loading.hint().unwrap().contains("You can enter now"));
        assert_eq!(loading.offers(), (true, true), "the world is there: enter anyway, or close");
        loading.note("Placing the world's objects".into(), false, false, 0.1);
        assert_eq!(loading.offers(), (false, true), "without a world to enter, only closing is offered");
        loading.note("Dressing your character".into(), true, false, 0.1);
        assert_eq!((loading.hint(), loading.offers()), (None, (false, false)), "a new step is movement");
        step(&mut loading, "Dressing your character", STILL_WORKING_SECS + 1.0);
        loading.note("Dressing your character".into(), true, true, 0.1);
        assert_eq!(loading.hint(), None, "so is a higher percent");
        step(&mut loading, GRAPHICS, STILL_WORKING_SECS + 1.0);
        assert!(loading.hint().unwrap().contains("prepares the graphics"), "shader compiling has its own words");
        loading.fail("Unable to load the map.".into());
        assert_eq!((loading.hint(), loading.offers()), (None, (false, true)), "a failure offers closing only");
        assert!(loading.elapsed > OFFER_SECS);
    }

    /// Enter anyway closes the screen only once it is offered.
    #[test]
    fn enter_anyway_works_only_once_offered() {
        let mut app = App::new();
        app.add_message::<AppExit>().init_resource::<LoadingScreen>().add_systems(Update, buttons);
        let press = |app: &mut App| {
            app.world_mut().spawn((Reveal::Enter, Interaction::Pressed, BackgroundColor(BUTTON)));
            app.update();
        };
        app.world_mut().resource_mut::<LoadingScreen>().map("a0200_kingscity".into());
        press(&mut app);
        assert!(app.world().resource::<LoadingScreen>().active(), "not offered yet: the click does nothing");
        {
            let mut loading = app.world_mut().resource_mut::<LoadingScreen>();
            for _ in 0..(OFFER_SECS as u32 * 10 + 10) { loading.note("Almost there".into(), true, false, 0.1); }
            assert_eq!(loading.offers(), (true, true));
        }
        press(&mut app);
        assert!(!app.world().resource::<LoadingScreen>().active(), "offered: the player enters");
    }

    #[test]
    fn readiness_stages_read_as_player_words() {
        for (stage, words) in [
            ("map and class assets", "Loading your skills and effects"), ("navigation", "Preparing the area"),
            ("ambience", "Preparing the area"), ("nearby NPC/decor scheduling", "Placing the people nearby"),
            ("scene \"maps/a0200.glb\"", "Loading scenery"), ("appearance 12v1 (local, outfit not built)", "Dressing your character"),
            ("appearance of 3 other player(s), first 40v2 \"Kit\" (no animation player)", "Waiting for other players' appearances"),
            ("first animation poses", "Preparing animations"), ("render warm-up", "Preparing the world"),
        ] { assert_eq!(step_words(stage), words, "{stage}"); }
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
