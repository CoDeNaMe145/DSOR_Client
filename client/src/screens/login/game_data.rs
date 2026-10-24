//! The game data strip across the top of the account screen. The original
//! client's files come from Bigpoint's CDN into `game-data/` (crates/game-data,
//! docs/game-data.md); the strip shows how far that is, and signing in to play
//! waits until every file is on disk.
//!
//! `DSOR_GAME_DATA=<folder>` keeps the files elsewhere (on an SSD the unpacking
//! is many times faster than on a hard disk), `DSOR_GAME_DATA_CDN=<url>` asks
//! another CDN, and `DSOR_FLAGS=nogamedata` skips the check and the strip.
//!
//! Per-frame cost: the strip is repainted only when the run's progress moved,
//! four times a second at most while bytes stream in (`UiWork::game_data_bars`);
//! a finished run leaves it alone. While files download the screen is held to
//! 60 frames a second, leaving the processor to the unpacking.
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use bevy::sprite::{BorderRect, TextureSlicer};
use bevy::winit::{UpdateMode, WinitSettings};
use dsor_game_data::{Cdn, Options, Progress, Stage, Status, DEFAULT_CDN};

use super::view::{ERROR, GOLD, IVORY, MUTED};
use super::Page;

const BAR: &str = "textures/ui/statusbar_connenctscreen_new.png";
const FONT: &str = "fonts/tahoma.ttf";
/// Four repaints a second at most while a stage goes on.
const REPAINT: f64 = 0.25;

pub(super) struct GameDataPlugin;

impl Plugin for GameDataPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (start, spawn).run_if(resource_exists::<crate::LoginGate>))
            .add_systems(Update, (retry, paint, pace).chain().run_if(resource_exists::<crate::LoginGate>))
            .add_systems(Update, close.run_if(resource_removed::<crate::LoginGate>));
    }
}

#[derive(Resource)]
pub(super) struct GameData {
    root: PathBuf,
    /// Off with `DSOR_FLAGS=nogamedata`: no check, no strip, play never waits.
    enabled: bool,
    progress: Arc<Progress>,
    task: Option<std::thread::JoinHandle<()>>,
    /// Every file is on disk, or the check is off: signing in may play.
    ready: bool,
    /// The window's update modes from before a download held the screen back.
    paced: Option<WinitSettings>,
}

impl GameData {
    fn new() -> Self {
        let root = std::env::var_os("DSOR_GAME_DATA").map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../game-data")));
        let enabled = !crate::flag("nogamedata");
        Self { root, enabled, progress: Arc::default(), task: None, ready: !enabled, paced: None }
    }

    fn start(&mut self) {
        if !self.enabled || self.running() {
            return;
        }
        let progress = Arc::new(Progress::default());
        self.progress = progress.clone();
        let options = Options::new(&self.root);
        let cdn = std::env::var("DSOR_GAME_DATA_CDN").unwrap_or_else(|_| DEFAULT_CDN.to_string());
        info!("game data: {} from {cdn}", self.root.display());
        self.task = std::thread::Builder::new().name("game data".into()).spawn(move || {
            let cdn = Cdn::new(&cdn);
            // Another client checking the same folder (both just started): wait for it.
            for _ in 0..120 {
                match std::panic::catch_unwind(AssertUnwindSafe(|| dsor_game_data::run(&options, &cdn, &progress))) {
                    Ok(Err(dsor_game_data::Error::Busy)) => std::thread::sleep(Duration::from_secs(1)),
                    Ok(_) => return,
                    Err(_) => return progress.abandon("The download stopped unexpectedly. Retry."),
                }
            }
        }).map_err(|e| warn!("game data: no thread for the download: {e}")).ok();
    }

    fn running(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    pub(super) fn ready(&self) -> bool {
        self.ready
    }

    /// The folder the account server's static database goes in, when the check is on.
    pub(super) fn folder(&self) -> Option<PathBuf> {
        self.enabled.then(|| self.root.clone())
    }
}

/// Why signing in cannot play yet: the game data is not all on disk. Saving
/// the settings is never held up.
pub(super) fn refusal(page: Page, data: Option<&GameData>) -> Option<&'static str> {
    (page != Page::Settings && data.is_some_and(|d| !d.ready))
        .then_some("The game data is not complete yet. You can play once the bar at the top is full.")
}

#[derive(Component)]
struct Strip;
#[derive(Component)]
struct Fill;
#[derive(Component)]
struct Headline;
#[derive(Component)]
struct Detail;
#[derive(Component)]
struct RetryButton;

fn start(mut commands: Commands) {
    let mut data = GameData::new();
    data.start();
    commands.insert_resource(data);
}

fn spawn(mut commands: Commands, assets: Res<AssetServer>) {
    if crate::flag("nogamedata") {
        return;
    }
    // A fresh machine has no converted assets yet: the strip is then drawn in
    // plain colours with Bevy's own font, and still reads.
    let root = PathBuf::from(crate::asset_root());
    let font = Path::exists(&root.join(FONT)).then(|| assets.load::<Font>(FONT));
    let bar = Path::exists(&root.join(BAR)).then(|| assets.load::<Image>(BAR));
    strip(&mut commands, font, bar);
}

fn strip(commands: &mut Commands, font: Option<Handle<Font>>, bar: Option<Handle<Image>>) {
    let text = |value: &str, size: f32, color: Color| {
        (Text::new(value), TextFont { font: font.clone().map(Into::into).unwrap_or_default(), font_size: FontSize::Px(size), ..default() },
            TextColor(color), TextShadow { offset: Vec2::new(0.0, 1.0), color: Color::BLACK })
    };
    let art = |rect: Rect| bar.clone().map(|image| ImageNode { image, rect: Some(rect), visual_box: bevy::ui::VisualBox::BorderBox,
        image_mode: NodeImageMode::Sliced(TextureSlicer { border: BorderRect::all(9.0), max_corner_scale: 1.0, ..default() }), ..default() });
    commands.spawn((Strip, Name::new("Game data"), GlobalZIndex(10001), Node {
        position_type: PositionType::Absolute, top: px(0), left: px(0), width: percent(100), height: px(42),
        padding: UiRect::axes(px(18), px(0)), column_gap: px(16), align_items: AlignItems::Center,
        border: UiRect::bottom(px(1)), ..default()
    }, BackgroundColor(Color::srgba(0.012, 0.014, 0.018, 0.86)), BorderColor::all(GOLD.with_alpha(0.35))))
        .with_children(|strip| {
            strip.spawn((Headline, text("Game data", 14.0, GOLD), Node { flex_shrink: 0.0, ..default() }));
            // The card's progress bar, as the original draws it (view::draw).
            let mut track = strip.spawn((Node { flex_grow: 1.0, min_width: px(80), height: px(22), overflow: Overflow::clip(), ..default() },
                BackgroundColor(if bar.is_some() { Color::NONE } else { Color::srgba(0.0, 0.0, 0.0, 0.55) })));
            if let Some(image) = art(Rect::new(0.0, 28.0, 953.0, 61.0)) {
                track.insert(image);
            }
            track.with_children(|track| {
                let mut fill = track.spawn((Fill, Node { position_type: PositionType::Absolute, left: px(0), top: px(4), height: px(14), width: percent(0), ..default() },
                    BackgroundColor(if bar.is_some() { Color::NONE } else { GOLD })));
                if let Some(image) = art(Rect::new(0.0, 0.0, 953.0, 28.0)) {
                    fill.insert(image);
                }
            });
            strip.spawn((Detail, text("", 13.0, MUTED), Node { flex_shrink: 0.0, max_width: percent(55), ..default() }));
            strip.spawn((RetryButton, Button, Node { display: Display::None, flex_shrink: 0.0, padding: UiRect::axes(px(10), px(4)),
                border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)), BorderColor::all(GOLD.with_alpha(0.5))))
                .with_children(|button| {
                    button.spawn(text("Retry", 13.0, GOLD));
                });
        });
}

/// What the strip last showed.
#[derive(Default)]
struct Shown {
    revision: Option<u64>,
    stage: Stage,
    at: f64,
}

#[allow(clippy::type_complexity)]
fn paint(mut data: ResMut<GameData>, time: Res<Time<Real>>, mut shown: Local<Shown>, mut work: ResMut<crate::ui::UiWork>,
    mut fills: Query<&mut Node, (With<Fill>, Without<RetryButton>)>,
    mut headlines: Query<&mut Text, (With<Headline>, Without<Detail>)>,
    mut details: Query<(&mut Text, &mut TextColor), (With<Detail>, Without<Headline>)>,
    mut retries: Query<&mut Node, (With<RetryButton>, Without<Fill>)>) {
    let revision = data.progress.revision();
    if !data.enabled || shown.revision == Some(revision) {
        return;
    }
    let status = data.progress.status();
    let now = time.elapsed_secs_f64();
    if shown.revision.is_some() && status.stage == shown.stage && now - shown.at < REPAINT {
        return;
    }
    *shown = Shown { revision: Some(revision), stage: status.stage, at: now };
    work.game_data_bars += 1;
    let ready = status.stage == Stage::Complete;
    if data.ready != ready {
        data.ready = ready;
        info!("game data: {}", if ready { "complete, play unlocked" } else { "incomplete, play locked" });
    }
    for mut node in &mut fills {
        node.width = percent(status.fraction() * 100.0);
    }
    for mut text in &mut headlines {
        set(&mut text, status.headline());
    }
    for (mut text, mut color) in &mut details {
        set(&mut text, &status.detail());
        let tone = tone(&status);
        if color.0 != tone {
            color.0 = tone;
        }
    }
    // Again: after a failure, or to look for an update a run offline could not.
    let retry = if status.stage == Stage::Failed || status.offline { Display::Flex } else { Display::None };
    for mut node in &mut retries {
        if node.display != retry {
            node.display = retry;
        }
    }
}

fn set(text: &mut Text, value: &str) {
    if text.0 != value {
        text.0 = value.to_string();
    }
}

fn tone(status: &Status) -> Color {
    match status.stage {
        Stage::Failed => ERROR,
        Stage::Complete => IVORY,
        _ => MUTED,
    }
}

fn retry(interactions: Query<&Interaction, (With<RetryButton>, Changed<Interaction>)>, mut data: ResMut<GameData>) {
    if interactions.iter().any(|i| *i == Interaction::Pressed) && !data.running() {
        data.start();
    }
}

/// While files download, the screen is drawn at most 60 times a second (twice
/// a second behind other windows), so the unpacking gets the processor.
fn pace(mut data: ResMut<GameData>, settings: Option<ResMut<WinitSettings>>) {
    let Some(mut settings) = settings else { return };
    let busy = data.running() && !data.ready;
    if busy && data.paced.is_none() {
        data.paced = Some(settings.clone());
        *settings = WinitSettings { focused_mode: UpdateMode::reactive(Duration::from_secs_f64(1.0 / 60.0)),
            unfocused_mode: UpdateMode::reactive_low_power(Duration::from_millis(500)) };
    } else if !busy && data.paced.is_some() {
        *settings = data.paced.take().unwrap_or_default();
    }
}

fn close(mut commands: Commands, strips: Query<Entity, With<Strip>>, data: Option<ResMut<GameData>>, settings: Option<ResMut<WinitSettings>>) {
    for strip in &strips {
        commands.entity(strip).despawn();
    }
    if let (Some(mut data), Some(mut settings)) = (data, settings) {
        if let Some(paced) = data.paced.take() {
            *settings = paced;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_game_data::{Error, Md5, Source};

    /// A CDN that cannot be reached.
    struct Offline;

    impl Source for Offline {
        fn root_key(&self) -> Result<Md5, Error> {
            Err(Error::Network("offline".into()))
        }

        fn fetch(&self, path: &str, _: Md5, _: &mut dyn FnMut(u64)) -> Result<Vec<u8>, Error> {
            Err(Error::NotFound(path.into()))
        }

        fn size(&self, path: &str, _: Md5) -> Result<u64, Error> {
            Err(Error::NotFound(path.into()))
        }
    }

    fn app() -> (App, Arc<Progress>, PathBuf) {
        let folder = std::env::temp_dir().join(format!("dsor-login-game-data-{}", std::process::id()));
        let progress = Arc::new(Progress::default());
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<crate::ui::UiWork>()
            .insert_resource(GameData { root: folder.clone(), enabled: true, progress: progress.clone(), task: None, ready: false, paced: None })
            .add_systems(Update, paint);
        strip(&mut app.world_mut().commands(), None, None);
        (app, progress, folder)
    }

    /// docs/ui.md, "Per-frame cost": the strip is painted when the run moves,
    /// never on an idle frame.
    #[test]
    fn the_strip_is_repainted_only_when_the_download_moves() {
        let (mut app, progress, folder) = app();
        let painted = |app: &App| app.world().resource::<crate::ui::UiWork>().game_data_bars;
        app.update();
        assert_eq!(painted(&app), 1);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(painted(&app), 1, "idle frames paint nothing");
        let _ = dsor_game_data::run(&Options::new(&folder), &Offline, &progress);
        app.update();
        assert_eq!(painted(&app), 2, "the run ended");
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(painted(&app), 2);
        let detail = app.world_mut().query_filtered::<&Text, With<Detail>>().single(app.world()).unwrap().0.clone();
        assert!(detail.contains("cannot be reached"), "{detail}");
        let retry = app.world_mut().query_filtered::<&Node, With<RetryButton>>().single(app.world()).unwrap().display;
        assert_eq!(retry, Display::Flex);
        assert!(!app.world().resource::<GameData>().ready());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn signing_in_waits_for_the_game_data_but_saving_settings_does_not() {
        let mut data = GameData { root: PathBuf::new(), enabled: true, progress: Arc::default(), task: None, ready: false, paced: None };
        assert!(refusal(Page::Login, Some(&data)).is_some());
        assert!(refusal(Page::Register, Some(&data)).is_some());
        assert_eq!(refusal(Page::Settings, Some(&data)), None);
        data.ready = true;
        assert_eq!(refusal(Page::Login, Some(&data)), None);
        assert_eq!(refusal(Page::Login, None), None, "no check: nothing to wait for");
    }
}
