//! Client audio: one device stream, shared PCM assets, bounded voices and loads.
//! All file access goes through AssetServer, including in browser builds.
mod mixer;
mod animation;
mod equipment;
pub(crate) use animation::AnimationAudio;
use crate::{
    network::net::{LocalPlayer, Net},
    world::{map::CurrentMap, props::NetworkProps},
};
use bevy::{
    asset::{io::Reader, AssetLoader, LoadContext, LoadState},
    audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source},
    prelude::*,
};
use mixer::{Command, Control, Mixer, Pcm, VOICES};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
};

const CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_LOADS: usize = 4;
const MAX_PENDING: usize = 128;
const EFFECT_AGE: f64 = 0.25;
const MAP_FADE: f32 = 1.5;

#[derive(Asset, TypePath)]
struct SampleAsset(Arc<Pcm>);
#[derive(Default, TypePath)]
struct SampleLoader;
impl AssetLoader for SampleLoader {
    type Asset = SampleAsset;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(SampleAsset(Arc::new(Pcm::wav(&bytes)?)))
    }
    fn extensions(&self) -> &[&str] {
        &["wav"]
    }
}
#[derive(Deserialize)]
struct Event {
    samples: Vec<String>,
    looping: bool,
    max_voices: usize,
}
#[derive(Deserialize)]
struct Placement {
    cue: String,
    event: i32,
    matrix: [f32; 16],
    layers: String,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    shape: String,
}
impl Placement {
    fn position(&self) -> Vec3 {
        Vec3::from_slice(&self.matrix[12..15])
    }
    fn contains(&self, point: Vec3) -> bool {
        let matrix = Mat4::from_cols_array(&self.matrix);
        if matrix.determinant().abs() < 1e-6 {
            return false;
        }
        let local = matrix.inverse().transform_point3(point);
        if self.shape.eq_ignore_ascii_case("sphere") {
            // Match the unit volumes used by the client's ambience bubbles.
            local.length_squared() <= 0.25
        } else {
            local.abs().max_element() <= 0.5
        }
    }
}
#[derive(Deserialize)]
struct MapAudio {
    music: String,
    ambient: String,
    ambient_3d: String,
    layers: String,
    event_sets: BTreeMap<i32, String>,
    emitters: Vec<Placement>,
    areas: Vec<Placement>,
}
#[derive(Asset, TypePath, Deserialize)]
struct Catalog {
    version: u32,
    events: HashMap<String, Event>,
    samples: HashMap<String, usize>,
    maps: HashMap<String, MapAudio>,
    #[serde(default)]
    animations: animation::Bindings,
}
#[derive(Default, TypePath)]
struct CatalogLoader;
impl AssetLoader for CatalogLoader {
    type Asset = Catalog;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<Catalog, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let catalog: Catalog = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        if catalog.version != 1 {
            return Err(std::io::Error::other("unsupported audio catalog version"));
        }
        Ok(catalog)
    }
    fn extensions(&self) -> &[&str] {
        &["audio.json"]
    }
}
#[derive(Asset, TypePath)]
struct Output {
    receiver: Mutex<Option<Receiver<Command>>>,
    finished: Arc<[AtomicU64; VOICES]>,
}
impl Decodable for Output {
    type Decoder = Mixer;
    fn decoder(&self) -> Mixer {
        // Only the single, permanent AudioPlayer owns this source.
        Mixer::new(
            self.receiver
                .lock()
                .unwrap()
                .take()
                .expect("audio output created twice"),
            self.finished.clone(),
        )
    }
}
impl Source for Mixer {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(mixer::RATE).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Bus {
    Music,
    Ambient,
    Effects,
}
impl Bus {
    fn slots(self) -> std::ops::Range<usize> {
        match self {
            Self::Music => 0..2,
            Self::Ambient => 2..16,
            Self::Effects => 16..VOICES,
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Music => 0,
            Self::Ambient => 1,
            Self::Effects => 2,
        }
    }
}
#[derive(Clone)]
struct Request {
    cue: String,
    key: Option<String>,
    bus: Bus,
    position: Option<Vec3>,
    emitter: Option<Entity>,
    owner: Option<Entity>,
    volume: f32,
    duration: Option<f32>,
    created: Option<f64>,
    variant: u64,
    one_shot: bool,
}
impl Request {
    fn background(&self) -> bool {
        self.key.is_some() && self.position.is_none() && self.bus != Bus::Effects
    }
    fn slots(&self) -> std::ops::Range<usize> {
        // Two outgoing beds can overlap two incoming beds even when every
        // positioned ambience voice is occupied.
        if self.bus == Bus::Ambient {
            if self.background() {
                2..6
            } else {
                6..16
            }
        } else {
            self.bus.slots()
        }
    }
    fn bed(cue: &str, key: String, bus: Bus, position: Option<Vec3>) -> Self {
        Self {
            cue: cue.into(),
            key: Some(key),
            bus,
            position,
            emitter: None,
            owner: None,
            volume: 1.,
            duration: None,
            created: None,
            variant: 0,
            one_shot: false,
        }
    }
}
/// Bounded producer queue. Sequences submit event names, never file paths or
/// AudioPlayers. The audio system owns loading, deduplication and playback.
#[derive(Resource, Default)]
pub(crate) struct AudioRequests {
    pending: VecDeque<Request>,
}
impl AudioRequests {
    fn spatial_one_shot(&mut self, cue: &str, position: Vec3, owner: Entity) {
        if self.pending.len() < MAX_PENDING {
            self.pending.push_back(Request {
                cue: cue.into(), key: None, bus: Bus::Effects, position: Some(position),
                emitter: Some(owner), owner: Some(owner), volume: 0.7, duration: None,
                created: None, variant: 0, one_shot: true,
            });
        }
    }
    pub(crate) fn ui(&mut self, cue: &str) {
        if self.pending.len() < MAX_PENDING {
            self.pending.push_back(Request {
                cue: cue.into(),
                key: None,
                bus: Bus::Effects,
                position: None,
                emitter: None,
                owner: None,
                volume: 0.8,
                duration: None,
                created: None,
                variant: 0,
                one_shot: true,
            });
        }
    }
    pub(crate) fn effect(
        &mut self,
        cue: String,
        position: Vec3,
        emitter: Entity,
        owner: Entity,
        volume: f32,
        duration: f32,
    ) {
        if !cue.is_empty() && volume.is_finite() && self.pending.len() < MAX_PENDING {
            let cue = cue
                .trim()
                .trim_start_matches("event:/")
                .trim_start_matches('/')
                .to_owned();
            self.pending.push_back(Request {
                cue,
                key: None,
                bus: Bus::Effects,
                position: Some(position),
                emitter: Some(emitter),
                owner: Some(owner),
                volume: (volume / 100.).clamp(0., 2.),
                duration: Some(duration.max(0.01)),
                created: None,
                variant: 0,
                one_shot: false,
            });
        }
    }
}
struct Cached {
    handle: Handle<SampleAsset>,
    bytes: usize,
    used: u64,
}
struct Active {
    token: u64,
    request: Request,
    looping: bool,
    until: Option<f64>,
    stopped: bool,
    // Keep outgoing spatial sounds at their last position in the stereo mix
    // while fading, even when the new map moves the listener elsewhere.
    spatial_gain: [f32; 2],
}
#[derive(Default)]
struct Scene {
    map: String,
    wanted: Vec<Request>,
    listener: Vec3,
    right: Vec3,
}
impl Scene {
    fn refresh(
        &mut self,
        current: Option<(&str, bool)>,
        loading: bool,
        login: bool,
        catalog: &Catalog,
        events: &NetworkProps,
        listener: Vec3,
        right: Vec3,
    ) -> bool {
        if current.is_none() && login && !loading {
            let changed = !self.map.is_empty();
            *self = Self::default();
            return changed;
        }
        // A map hand-off can temporarily remove CurrentMap and the player.
        // Keep the audible scene and listener intact throughout that gap.
        let Some((name, true)) = current.filter(|_| !loading) else {
            return false;
        };
        let changed = self.map != name;
        self.map = name.into();
        self.wanted = desired(catalog.maps.get(name), listener, events);
        for request in &mut self.wanted {
            if request.position.is_some() {
                request.key = request.key.as_ref().map(|key| format!("{name}:{key}"));
            }
        }
        self.listener = listener;
        self.right = right;
        changed
    }
}
#[derive(Resource)]
struct AudioState {
    catalog: Handle<Catalog>,
    sender: SyncSender<Command>,
    finished: Arc<[AtomicU64; VOICES]>,
    voices: [Option<Active>; VOICES],
    cache: HashMap<String, Cached>,
    failed: HashSet<String>,
    warned: HashSet<String>,
    recent: HashMap<(String, Option<Entity>), f64>,
    repeats: HashMap<String, (f64, u64)>,
    warm_class: Option<u8>,
    warm: VecDeque<String>,
    scene: Scene,
    token: u64,
    frame: u64,
    reported: bool,
}
impl AudioState {
    fn sample(
        &mut self,
        path: &str,
        catalog: &Catalog,
        server: &AssetServer,
        assets: &mut Assets<SampleAsset>,
    ) -> Option<Arc<Pcm>> {
        if self.failed.contains(path) {
            return None;
        }
        if let Some(entry) = self.cache.get_mut(path) {
            entry.used = self.frame;
            if let Some(sample) = assets.get(&entry.handle) {
                return Some(sample.0.clone());
            }
            if matches!(
                server.get_load_state(entry.handle.id()),
                Some(LoadState::Failed(_))
            ) {
                self.failed.insert(path.into());
                self.cache.remove(path);
            }
            return None;
        }
        let bytes = *catalog.samples.get(path)?;
        if bytes > CACHE_BYTES {
            self.failed.insert(path.into());
            warn!("Audio sample exceeds cache budget: {path}");
            return None;
        }
        if self
            .cache
            .values()
            .filter(|e| assets.get(&e.handle).is_none())
            .count()
            >= MAX_LOADS
        {
            return None;
        }
        while self.cache.values().map(|e| e.bytes).sum::<usize>() + bytes > CACHE_BYTES {
            let oldest = self
                .cache
                .iter()
                .filter(|(_, e)| {
                    assets
                        .get(&e.handle)
                        .is_some_and(|s| Arc::strong_count(&s.0) == 1)
                })
                .min_by_key(|(_, e)| e.used)
                .map(|(p, _)| p.clone());
            let Some(oldest) = oldest else { return None };
            let old = self.cache.remove(&oldest).unwrap();
            assets.remove(old.handle.id());
        }
        self.cache.insert(
            path.into(),
            Cached {
                handle: server.load(path.to_owned()),
                bytes,
                used: self.frame,
            },
        );
        None
    }
    fn start(
        &mut self,
        request: &Request,
        catalog: &Catalog,
        server: &AssetServer,
        assets: &mut Assets<SampleAsset>,
        now: f64,
        listener: Vec3,
        right: Vec3,
        volumes: [f32; 3],
    ) -> bool {
        let Some(event) = catalog.events.get(&request.cue) else {
            if self.warned.insert(request.cue.clone()) {
                debug!("Unresolved audio event: {}", request.cue);
            }
            return true;
        };
        if event.samples.is_empty() {
            return true;
        }
        let gain = gains(request, listener, right, volumes);
        if gain[0].max(gain[1]) < 0.001 {
            return true;
        }
        // UI feedback has no owner or duration to stop an authored loop.
        // Its playback contract is always one shot, regardless of FMOD flags.
        let looping = !request.one_shot
            && (event.looping
                || request.bus == Bus::Music
                || request.key.as_deref() == Some("ambient"));
        if request.key.is_none() && looping {
            if let Some(active) = self.voices.iter_mut().flatten().find(|v| {
                !v.stopped
                    && v.looping
                    && v.request.cue == request.cue
                    && v.request.emitter == request.emitter
                    && v.request.owner == request.owner
            }) {
                active.until = request.duration.map(|d| now + d as f64);
                return true;
            }
        }
        let count = self
            .voices
            .iter()
            .flatten()
            .filter(|v| {
                !v.stopped && v.request.cue == request.cue
                    // Maps sometimes use the same cue in different bed roles.
                    // Allow those roles to overlap during their crossfade;
                    // the four reserved slots still bound their total cost.
                    && !(request.background() && v.request.background()
                        && request.key != v.request.key)
            })
            .count();
        if count >= event.max_voices.clamp(1, 8) {
            return true;
        }
        let key = (request.cue.clone(), request.emitter);
        if request.key.is_none() && self.recent.get(&key).is_some_and(|t| now - t < 0.035) {
            return true;
        }
        let Some(slot) = request.slots().find(|&s| self.voices[s].is_none()) else {
            return true;
        };
        // Stable choice until a request actually starts, so asynchronous loading
        // never cycles through new variants on every retry.
        let choice = ((request
            .created
            .unwrap_or(0.)
            .to_bits()
            .wrapping_mul(0x9e3779b97f4a7c15)
            >> 32)
            ^ request.variant) as usize
            % event.samples.len();
        let path = &event.samples[choice];
        let Some(pcm) = self.sample(path, catalog, server, assets) else {
            return false;
        };
        let token = self.token + 1;
        let fade = if request.bus != Bus::Effects {
            MAP_FADE
        } else {
            0.01
        };
        if self
            .sender
            .try_send(Command::Start {
                slot,
                token,
                pcm,
                looping,
                spatial: request.position.is_some(),
                gain,
                fade,
            })
            .is_err()
        {
            return false;
        }
        self.token = token;
        self.voices[slot] = Some(Active {
            token,
            request: request.clone(),
            looping,
            until: if looping {
                request.duration.map(|d| now + d as f64)
            } else {
                None
            },
            stopped: false,
            spatial_gain: gains(request, listener, right, [1.; 3]),
        });
        debug!(
            "audio voice {slot}: {} -> {path} (loop={looping})",
            request.cue
        );
        if request.key.is_none() {
            self.recent.insert(key, now);
        }
        true
    }

    fn replacement_ready(
        &self,
        outgoing: &Request,
        wanted: &[Request],
        catalog: &Catalog,
        volumes: [f32; 3],
    ) -> bool {
        // Fade a bed as its own replacement starts. If the destination has no
        // equivalent (e.g. music -> ambience only), wait for its other beds.
        let same_role = |r: &&Request| {
            if outgoing.bus == Bus::Music {
                r.bus == Bus::Music
            } else {
                r.bus == outgoing.bus && r.key == outgoing.key
            }
        };
        let unavailable = |request: &Request| {
            catalog.events.get(&request.cue).is_none_or(|event| {
                event.samples.first().is_none_or(|path| {
                    self.failed.contains(path) || !catalog.samples.contains_key(path)
                })
            })
        };
        let has_replacement = wanted
            .iter()
            .filter(|r| r.background())
            .any(|r| same_role(&r) && !unavailable(r));
        wanted
            .iter()
            .filter(|r| r.background())
            .filter(|r| !has_replacement || same_role(r))
            .all(|request| {
                if volumes[request.bus.index()] < 0.001
                    || self.voices.iter().flatten().any(|v| {
                        !v.stopped && v.request.key == request.key && v.request.cue == request.cue
                    })
                    || request
                        .key
                        .as_ref()
                        .is_some_and(|key| self.repeats.contains_key(key))
                {
                    return true;
                }
                // Missing/failed optional cues must not keep the old map playing
                // forever. Pending loads, however, keep its background alive.
                unavailable(request)
            })
    }
}

fn gains(request: &Request, listener: Vec3, right: Vec3, volumes: [f32; 3]) -> [f32; 2] {
    let level = request.volume * volumes[request.bus.index()];
    let Some(position) = request.position else {
        return [level; 2];
    };
    let delta = position - listener;
    let distance = delta.length();
    // In game metres: full volume nearby, smoothly silent at 45m.
    let attenuation = (1. - ((distance - 2.) / 43.).clamp(0., 1.)).powi(2);
    let pan = delta
        .try_normalize()
        .map_or(0., |d| d.dot(right))
        .clamp(-1., 1.);
    [
        level * attenuation * ((1. - pan) * 0.5).sqrt(),
        level * attenuation * ((1. + pan) * 0.5).sqrt(),
    ]
}
fn layer_enabled(placement: &Placement, map: &MapAudio) -> bool {
    placement.layers.is_empty()
        || placement
            .layers
            .split(';')
            .any(|l| map.layers.split(';').any(|active| active == l))
}
fn desired(map: Option<&MapAudio>, listener: Vec3, events: &NetworkProps) -> Vec<Request> {
    let Some(map) = map else { return Vec::new() };
    let enabled =
        |p: &&Placement| layer_enabled(p, map) && events.event_enabled(p.event, &map.event_sets);
    let music = map
        .areas
        .iter()
        .filter(enabled)
        .find(|a| a.contains(listener))
        .map_or(map.music.as_str(), |a| a.cue.as_str());
    let mut result = Vec::with_capacity(13);
    if !music.is_empty() {
        result.push(Request::bed(
            music,
            format!("music:{music}"),
            Bus::Music,
            None,
        ));
    }
    for (key, cue) in [("ambient", &map.ambient), ("ambient_3d", &map.ambient_3d)] {
        if !cue.is_empty() {
            result.push(Request::bed(cue, key.into(), Bus::Ambient, None));
        }
    }
    let mut emitters: Vec<_> = map
        .emitters
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.active && enabled(p) && p.position().distance_squared(listener) < 45. * 45.
        })
        .collect();
    emitters.sort_by(|a, b| {
        a.1.position()
            .distance_squared(listener)
            .total_cmp(&b.1.position().distance_squared(listener))
    });
    for (index, p) in emitters.into_iter().take(10) {
        result.push(Request::bed(
            &p.cue,
            format!("emitter:{index}"),
            Bus::Ambient,
            Some(p.position()),
        ));
    }
    result
}

pub(crate) struct GameAudioPlugin;
impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SampleAsset>()
            .init_asset_loader::<SampleLoader>()
            .init_asset::<Catalog>()
            .init_asset_loader::<CatalogLoader>()
            .add_audio_source::<Output>()
            .init_resource::<AudioRequests>()
            .add_systems(Startup, setup)
            .add_systems(
                PostUpdate,
                (animation::emit, equipment::emit, update).chain()
                    .after(bevy::app::AnimationSystems)
                    .after(bevy::transform::TransformSystems::Propagate),
            );
    }
}
fn setup(mut commands: Commands, server: Res<AssetServer>, mut outputs: ResMut<Assets<Output>>) {
    let (sender, receiver) = sync_channel(128);
    let finished = Arc::new(std::array::from_fn(|_| AtomicU64::new(0)));
    let source = outputs.add(Output {
        receiver: Mutex::new(Some(receiver)),
        finished: finished.clone(),
    });
    commands.spawn((
        Name::new("Game audio mixer"),
        AudioPlayer(source),
        PlaybackSettings::ONCE,
    ));
    commands.insert_resource(AudioState {
        catalog: server.load("audio/runtime.audio.json"),
        sender,
        finished,
        voices: std::array::from_fn(|_| None),
        cache: HashMap::new(),
        failed: HashSet::new(),
        warned: HashSet::new(),
        recent: HashMap::new(),
        repeats: HashMap::new(),
        warm_class: None,
        warm: VecDeque::new(),
        scene: Scene::default(),
        token: 0,
        frame: 0,
        reported: false,
    });
}
fn update(
    mut state: ResMut<AudioState>,
    mut requests: ResMut<AudioRequests>,
    time: Res<Time>,
    server: Res<AssetServer>,
    catalogs: Res<Assets<Catalog>>,
    mut samples: ResMut<Assets<SampleAsset>>,
    current: Option<Res<CurrentMap>>,
    net: Option<NonSend<Net>>,
    settings: Option<Res<crate::ui::Settings>>,
    transforms: Query<&GlobalTransform>,
    players: Query<&GlobalTransform, With<LocalPlayer>>,
    exists: Query<()>,
    cameras: Query<
        &GlobalTransform,
        (
            With<Camera3d>,
            Without<crate::rendering::highlight::HighlightCamera>,
        ),
    >,
    characters: Query<&crate::actors::character::Character, With<LocalPlayer>>,
    loading: Option<Res<crate::screens::loading::LoadingScreen>>,
    login: Option<Res<crate::LoginGate>>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("audio::update");
    let now = time.elapsed_secs_f64();
    state.frame += 1;
    let Some(catalog) = catalogs.get(&state.catalog) else {
        requests.pending.clear();
        return;
    };
    if !state.reported {
        info!(
            "Audio ready: {} events, {} maps; 64 voices, 128 MiB PCM cache",
            catalog.events.len(),
            catalog.maps.len()
        );
        state.reported = true;
        for cue in ["ui/general/ui_click", "ui/general/ui_close_screen"] {
            if let Some(event) = catalog.events.get(cue) {
                state.warm.extend(event.samples.iter().cloned());
            }
        }
    }
    let failed: Vec<_> = state
        .cache
        .iter()
        .filter(|(_, entry)| {
            samples.get(&entry.handle).is_none()
                && matches!(
                    server.get_load_state(entry.handle.id()),
                    Some(LoadState::Failed(_))
                )
        })
        .map(|(path, _)| path.clone())
        .collect();
    for path in failed {
        state.cache.remove(&path);
        state.failed.insert(path);
    }
    let camera = cameras.iter().next();
    let listener = players
        .iter()
        .next()
        .or(camera)
        .map_or(Vec3::ZERO, |t| t.translation());
    let right = camera.map_or(Vec3::X, |t| *t.right());
    let volumes = settings.as_ref().map_or([0.48, 0.48, 0.64], |s| {
        let p = &s.current;
        let master = if p.audio_muted {
            0.
        } else {
            p.master_volume.min(100) as f32 / 100.
        };
        [p.music_volume, p.ambient_volume, p.effects_volume]
            .map(|v| master * v.min(100) as f32 / 100.)
    });
    let loading = loading.is_some_and(|screen| screen.active());
    let offline_events = NetworkProps::default();
    let events = net.as_ref().map_or(&offline_events, |n| &n.props);
    let changed = state.scene.refresh(
        current.as_ref().map(|m| (m.name.as_str(), m.spawned)),
        loading,
        login.is_some(),
        catalog,
        events,
        listener,
        right,
    );
    if changed {
        state.recent.clear();
        state.repeats.clear();
    }
    if changed || loading {
        requests.pending.retain(|r| r.owner.is_none());
        for voice in state.voices.iter_mut().flatten() {
            if voice.request.bus == Bus::Effects && voice.request.owner.is_some() {
                voice.stopped = true;
            }
        }
    }
    let listener = state.scene.listener;
    let right = state.scene.right;
    let wanted = state.scene.wanted.clone();
    // Prefetch destination beds during the loading overlay, without replacing
    // the audible scene or evaluating its zones at the old player's position.
    if let Some(map) = current.as_ref().and_then(|m| catalog.maps.get(&m.name)) {
        for cue in [&map.music, &map.ambient, &map.ambient_3d] {
            if let Some(path) = catalog.events.get(cue).and_then(|e| e.samples.first()) {
                state.sample(path, catalog, &server, &mut samples);
            }
        }
    }
    for slot in 0..VOICES {
        if state.voices[slot]
            .as_ref()
            .is_some_and(|v| state.finished[slot].load(Ordering::Acquire) == v.token)
        {
            let voice = state.voices[slot].take().unwrap();
            if !voice.looping
                && !voice.stopped
                && wanted
                    .iter()
                    .any(|r| r.key == voice.request.key && r.cue == voice.request.cue)
            {
                if let Some(key) = voice.request.key {
                    let variant = voice.request.variant.wrapping_add(1);
                    state
                        .repeats
                        .insert(key, (now + 5. + (variant % 7) as f64, variant));
                }
            }
        }
    }
    // Queue incoming voices before deciding whether to release outgoing beds.
    // Both fades then reach the mixer together, with no frame of silence.
    for request in &wanted {
        if state
            .voices
            .iter()
            .flatten()
            .any(|v| !v.stopped && v.request.key == request.key && v.request.cue == request.cue)
        {
            continue;
        }
        let mut request = request.clone();
        if let Some((when, variant)) = request.key.as_ref().and_then(|key| state.repeats.get(key)) {
            if now < *when {
                continue;
            }
            request.variant = *variant;
        }
        state.start(
            &request,
            catalog,
            &server,
            &mut samples,
            now,
            listener,
            right,
            volumes,
        );
    }
    let retire: [bool; VOICES] = std::array::from_fn(|slot| {
        state.voices[slot].as_ref().is_some_and(|v| {
            !v.request.background()
                || state.replacement_ready(&v.request, &wanted, catalog, volumes)
        })
    });
    let mut controls = Box::new([Control::default(); VOICES]);
    for (slot, entry) in state.voices.iter_mut().enumerate() {
        let Some(v) = entry else { continue };
        if let Some(key) = &v.request.key {
            if !wanted
                .iter()
                .any(|r| r.key.as_ref() == Some(key) && r.cue == v.request.cue)
                && retire[slot]
            {
                v.stopped = true;
            }
        }
        if v.looping
            && (v.until.is_some_and(|end| now >= end)
                || v.request.owner.is_some_and(|e| !exists.contains(e)))
        {
            v.stopped = true;
        }
        if let Some(transform) = v.request.emitter.and_then(|e| transforms.get(e).ok()) {
            v.request.position = Some(transform.translation());
        }
        if !v.stopped {
            v.spatial_gain = gains(&v.request, listener, right, [1.; 3]);
        }
        controls[slot] = Control {
            token: v.token,
            gain: v.spatial_gain.map(|g| g * volumes[v.request.bus.index()]),
            stop: v.stopped,
            fade: if v.request.bus != Bus::Effects {
                MAP_FADE
            } else {
                0.15
            },
        };
    }
    let _ = state.sender.try_send(Command::Controls(controls));
    state.recent.retain(|_, t| now - *t < 1.);
    let pending = requests.pending.len();
    for _ in 0..pending {
        let mut request = requests.pending.pop_front().unwrap();
        let created = *request.created.get_or_insert(now);
        if now - created > EFFECT_AGE {
            continue;
        }
        if !state.start(
            &request,
            catalog,
            &server,
            &mut samples,
            now,
            listener,
            right,
            volumes,
        ) {
            requests.pending.push_back(request);
        }
    }
    // Warm this class's spell variants once, behind audible requests. The same
    // bounded cache and four-load limit apply; no warm job pins an asset.
    let class = characters.iter().next().map(|c| c.desc.class);
    if state.warm_class != class {
        state.warm_class = class;
        state.warm.clear();
        if let Some(class) = class {
            let name = ["warrior", "mage", "ranger", "dwarf"]
                .get(class as usize)
                .unwrap_or(&"warrior");
            let prefix = format!("archetype/{name}/");
            let mut paths: Vec<_> = catalog
                .events
                .iter()
                .filter(|(n, _)| n.starts_with(&prefix))
                .flat_map(|(_, e)| e.samples.iter().cloned())
                .collect();
            paths.sort();
            paths.dedup();
            let mut budget = 32 * 1024 * 1024;
            for path in paths {
                let size = catalog.samples.get(&path).copied().unwrap_or(0);
                if size > 0 && size <= budget {
                    budget -= size;
                    state.warm.push_back(path);
                }
            }
        }
    }
    if volumes[2] > 0. {
        for _ in 0..MAX_LOADS {
            let Some(path) = state.warm.front().cloned() else {
                break;
            };
            if state
                .sample(&path, catalog, &server, &mut samples)
                .is_some()
                || state.failed.contains(&path)
            {
                state.warm.pop_front();
            } else if state.cache.contains_key(&path) {
                state.warm.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_feedback_ignores_authored_loop_flags_finishes_and_reuses_its_slot() {
        let (mut app, receiver, _, _) = transition_app();
        let catalog = app.world().resource::<AudioState>().catalog.clone();
        app.world_mut()
            .resource_mut::<Assets<Catalog>>()
            .get_mut(&catalog)
            .unwrap()
            .events
            .insert(
                "ui/click".into(),
                Event {
                    samples: vec!["resident".into()],
                    looping: true,
                    max_voices: 1,
                },
            );
        app.world_mut()
            .resource_mut::<AudioRequests>()
            .ui("ui/click");
        app.update();
        let state = app.world().resource::<AudioState>();
        let voice = state.voices[16].as_ref().unwrap();
        assert!(!voice.looping);
        let token = voice.token;
        let finished = state.finished.clone();
        let mut mixer = Mixer::new(receiver, finished.clone());
        // The real mixer must reach the end of the PCM and release the voice,
        // even though its source event is authored with an infinite loop.
        mixer.by_ref().take(4096).for_each(drop);
        assert_eq!(finished[16].load(Ordering::Acquire), token);
        app.update();
        assert!(app.world().resource::<AudioState>().voices[16].is_none());
        app.world_mut().resource_mut::<AudioState>().recent.clear();
        app.world_mut()
            .resource_mut::<AudioRequests>()
            .ui("ui/click");
        app.update();
        let state = app.world().resource::<AudioState>();
        assert!(state.voices[16]
            .as_ref()
            .is_some_and(|v| v.token > token && !v.looping));
        assert!(
            state.voices[0].as_ref().is_some_and(|v| v.looping),
            "map music still loops"
        );
    }

    fn transition_app() -> (App, Receiver<Command>, Handle<SampleAsset>, Arc<Pcm>) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<SampleAsset>()
            .init_asset::<Catalog>()
            .init_resource::<AudioRequests>()
            .init_resource::<crate::screens::loading::LoadingScreen>()
            .add_systems(Update, update);
        let pcm = Arc::new(Pcm {
            data: vec![100; 480].into(),
            channels: 1,
            rate: 48000,
            loop_frames: None,
        });
        let mut assets = app.world_mut().resource_mut::<Assets<SampleAsset>>();
        let resident = assets.add(SampleAsset(pcm.clone()));
        // A reserved handle stays pending until the test explicitly supplies
        // decoded PCM, independent of disk speed and asynchronous task timing.
        let pending = assets.reserve_handle();
        let mut catalog = Catalog {
            animations: default(),
            version: 1,
            events: HashMap::new(),
            maps: HashMap::new(),
            samples: HashMap::from([("resident".into(), 960), ("pending".into(), 960)]),
        };
        for (name, path) in [("old", "resident"), ("new", "pending")] {
            let mut map = MapAudio {
                music: format!("{name}/music"),
                ambient: format!("{name}/ambient"),
                ambient_3d: format!("{name}/ambient_3d"),
                layers: String::new(),
                event_sets: BTreeMap::new(),
                emitters: Vec::new(),
                areas: Vec::new(),
            };
            for n in 0..12 {
                map.emitters.push(Placement {
                    cue: format!("{name}/object{n}"),
                    event: -1,
                    matrix: Mat4::from_translation(Vec3::X * (n as f32 + 1.)).to_cols_array(),
                    layers: String::new(),
                    active: true,
                    shape: String::new(),
                });
            }
            for cue in [&map.music, &map.ambient, &map.ambient_3d]
                .into_iter()
                .chain(map.emitters.iter().map(|p| &p.cue))
            {
                catalog.events.insert(
                    cue.clone(),
                    Event {
                        samples: vec![path.into()],
                        looping: true,
                        max_voices: 1,
                    },
                );
            }
            catalog.maps.insert(name.into(), map);
        }
        let catalog = app
            .world_mut()
            .resource_mut::<Assets<Catalog>>()
            .add(catalog);
        let (sender, receiver) = sync_channel(128);
        app.insert_resource(AudioState {
            catalog,
            sender,
            finished: Arc::new(std::array::from_fn(|_| AtomicU64::new(0))),
            voices: std::array::from_fn(|_| None),
            cache: HashMap::from([
                (
                    "resident".into(),
                    Cached {
                        handle: resident,
                        bytes: 960,
                        used: 0,
                    },
                ),
                (
                    "pending".into(),
                    Cached {
                        handle: pending.clone(),
                        bytes: 960,
                        used: 0,
                    },
                ),
            ]),
            failed: HashSet::new(),
            warned: HashSet::new(),
            recent: HashMap::new(),
            repeats: HashMap::new(),
            warm_class: None,
            warm: VecDeque::new(),
            scene: Scene::default(),
            token: 0,
            frame: 0,
            reported: true,
        });
        set_map(&mut app, "old", true);
        (app, receiver, pending, pcm)
    }

    fn set_map(app: &mut App, name: &str, spawned: bool) {
        let mut map = CurrentMap::new(name.into(), Handle::default());
        map.spawned = spawned;
        app.insert_resource(map);
    }

    #[test]
    fn loading_keeps_audio_until_replacements_can_crossfade_even_with_full_pools() {
        let (mut app, receiver, pending, pcm) = transition_app();
        let player = app
            .world_mut()
            .spawn((LocalPlayer::default(), GlobalTransform::IDENTITY))
            .id();
        app.update();
        receiver.try_iter().for_each(drop);
        let state = app.world().resource::<AudioState>();
        assert_eq!(state.voices.iter().flatten().count(), 13);
        assert!(state.voices[6..16].iter().all(Option::is_some));
        let old_token = state.token;
        let old_pan = state.voices[6].as_ref().unwrap().spatial_gain;

        app.world_mut()
            .resource_mut::<crate::screens::loading::LoadingScreen>()
            .connecting();
        app.world_mut().remove_resource::<CurrentMap>();
        *app.world_mut().get_mut::<GlobalTransform>(player).unwrap() =
            GlobalTransform::from_translation(Vec3::splat(2000.));
        for _ in 0..20 {
            app.update();
        }
        assert!(app
            .world()
            .resource::<AudioState>()
            .voices
            .iter()
            .flatten()
            .all(|v| !v.stopped));
        assert_eq!(
            app.world().resource::<AudioState>().scene.listener,
            Vec3::ZERO
        );
        // Gaps between overlay jobs must also preserve the outgoing scene.
        app.world_mut()
            .resource_mut::<crate::screens::loading::LoadingScreen>()
            .cancel();
        app.update();
        set_map(&mut app, "new", false);
        app.world_mut()
            .resource_mut::<crate::screens::loading::LoadingScreen>()
            .map("new".into());
        app.update();
        app.world_mut().resource_mut::<CurrentMap>().root = Some(Entity::from_bits(99));
        app.world_mut().resource_mut::<CurrentMap>().spawned = true;
        app.update();
        assert_eq!(app.world().resource::<AudioState>().token, old_token);
        assert!(app
            .world()
            .resource::<AudioState>()
            .voices
            .iter()
            .flatten()
            .all(|v| !v.stopped));

        // The world is ready, but the incoming WAVs still aren't decoded.
        app.world_mut()
            .resource_mut::<crate::screens::loading::LoadingScreen>()
            .cancel();
        app.update();
        let state = app.world().resource::<AudioState>();
        assert!(state
            .voices
            .iter()
            .flatten()
            .filter(|v| v.request.background())
            .all(|v| !v.stopped));
        assert!(state.voices[6..16].iter().flatten().all(|v| v.stopped));
        assert_eq!(state.voices[6].as_ref().unwrap().spatial_gain, old_pan);
        receiver.try_iter().for_each(drop);

        app.world_mut()
            .resource_mut::<Assets<SampleAsset>>()
            .insert(pending.id(), SampleAsset(pcm))
            .unwrap();
        app.update();
        let state = app.world().resource::<AudioState>();
        assert_eq!(state.voices[..6].iter().flatten().count(), 6);
        for v in state.voices[..6].iter().flatten() {
            assert_eq!(v.stopped, v.request.cue.starts_with("old/"));
        }
        let commands: Vec<_> = receiver.try_iter().collect();
        assert_eq!(
            commands
                .iter()
                .filter(|c| matches!(c, Command::Start { fade, .. } if *fade == MAP_FADE))
                .count(),
            3
        );
        assert!(commands.iter().any(|c| matches!(c, Command::Controls(controls)
            if controls[0].stop && controls[0].fade == MAP_FADE && controls[2].stop && controls[2].fade == MAP_FADE)));

        // A third map change while all crossfade slots are occupied waits for
        // released slots without killing the currently audible replacements.
        set_map(&mut app, "old", true);
        app.update();
        let state = app.world().resource::<AudioState>();
        assert!(state
            .voices
            .iter()
            .flatten()
            .filter(|v| v.request.cue.starts_with("new/"))
            .all(|v| !v.stopped));
        for (slot, v) in state.voices.iter().enumerate() {
            if let Some(v) = v.as_ref().filter(|v| v.stopped) {
                state.finished[slot].store(v.token, Ordering::Release);
            }
        }
        app.update();
        let state = app.world().resource::<AudioState>();
        assert!(state
            .voices
            .iter()
            .flatten()
            .filter(|v| v.request.cue.starts_with("new/"))
            .all(|v| v.stopped));
        assert!(state.voices[0]
            .as_ref()
            .is_some_and(|v| v.request.cue == "old/music" && !v.stopped));
        let token = state.token;
        app.world_mut().resource_mut::<CurrentMap>().root = Some(Entity::from_bits(100));
        app.update();
        assert_eq!(app.world().resource::<AudioState>().token, token);
    }

    #[test]
    fn music_blends_into_ambience_only_maps_and_failed_cues_do_not_hold_it_forever() {
        let (mut app, receiver, pending, pcm) = transition_app();
        let handle = app.world().resource::<AudioState>().catalog.clone();
        app.world_mut()
            .resource_mut::<Assets<Catalog>>()
            .get_mut(&handle)
            .unwrap()
            .maps
            .get_mut("new")
            .unwrap()
            .music
            .clear();
        app.update();
        set_map(&mut app, "new", true);
        app.update();
        assert!(
            !app.world().resource::<AudioState>().voices[0]
                .as_ref()
                .unwrap()
                .stopped
        );
        receiver.try_iter().for_each(drop);
        app.world_mut()
            .resource_mut::<Assets<SampleAsset>>()
            .insert(pending.id(), SampleAsset(pcm))
            .unwrap();
        app.update();
        assert!(
            app.world().resource::<AudioState>().voices[0]
                .as_ref()
                .unwrap()
                .stopped
        );
        assert!(app
            .world()
            .resource::<AudioState>()
            .voices
            .iter()
            .flatten()
            .any(|v| v.request.cue == "new/ambient" && !v.stopped));

        let (mut app, _receiver, _, _) = transition_app();
        app.update();
        set_map(&mut app, "new", true);
        app.update();
        assert!(
            !app.world().resource::<AudioState>().voices[0]
                .as_ref()
                .unwrap()
                .stopped
        );
        app.world_mut()
            .resource_mut::<AudioState>()
            .failed
            .insert("pending".into());
        app.update();
        assert!(app
            .world()
            .resource::<AudioState>()
            .voices
            .iter()
            .flatten()
            .all(|v| v.stopped));
    }

    #[test]
    fn shared_ambience_roles_can_overlap_and_missing_music_waits_for_ambience() {
        let (mut app, _receiver, pending, pcm) = transition_app();
        let handle = app.world().resource::<AudioState>().catalog.clone();
        {
            let mut catalogs = app.world_mut().resource_mut::<Assets<Catalog>>();
            let mut catalog = catalogs.get_mut(&handle).unwrap();
            let map = catalog.maps.get_mut("new").unwrap();
            map.music = "missing/music".into();
            map.ambient = "old/ambient_3d".into();
        }
        app.update();
        set_map(&mut app, "new", true);
        app.update();
        let state = app.world().resource::<AudioState>();
        // This authored cue permits one instance, but two different bed roles
        // need a brief overlap to transfer it without a silent gap.
        assert_eq!(
            state
                .voices
                .iter()
                .flatten()
                .filter(|v| v.request.cue == "old/ambient_3d")
                .count(),
            2
        );
        assert!(!state.voices[0].as_ref().unwrap().stopped);
        app.world_mut()
            .resource_mut::<Assets<SampleAsset>>()
            .insert(pending.id(), SampleAsset(pcm))
            .unwrap();
        app.update();
        let state = app.world().resource::<AudioState>();
        assert!(state.voices[0].as_ref().unwrap().stopped);
        assert!(state
            .voices
            .iter()
            .flatten()
            .any(|v| v.request.cue == "new/ambient_3d" && !v.stopped));
    }

    #[test]
    fn spell_storm_has_bounded_queue_and_distance_culling() {
        let mut requests = AudioRequests::default();
        let id = Entity::from_bits(1);
        for _ in 0..10000 {
            requests.effect("spell".into(), Vec3::ZERO, id, id, 100., 1.);
        }
        assert_eq!(requests.pending.len(), MAX_PENDING);
        let r = requests.pending.front().unwrap();
        assert_eq!(gains(r, Vec3::X * 46., Vec3::X, [1.; 3]), [0.; 2]);
        let gain = gains(r, -Vec3::X, Vec3::X, [1.; 3]);
        assert!(gain[1] > gain[0]);
    }
    #[test]
    fn zone_shapes_use_authored_transform() {
        let mut p = Placement {
            cue: String::new(),
            event: -1,
            matrix: Mat4::from_scale_rotation_translation(
                Vec3::splat(10.),
                Quat::IDENTITY,
                Vec3::X * 20.,
            )
            .to_cols_array(),
            layers: String::new(),
            active: true,
            shape: "sphere".into(),
        };
        assert!(p.contains(Vec3::X * 24.));
        assert!(!p.contains(Vec3::X * 26.));
        p.shape = "box".into();
        assert!(p.contains(Vec3::X * 24.));
        assert!(!p.contains(Vec3::X * 26.));
    }
    #[test]
    fn polyphony_caps_keep_music_and_ambience_slots_free() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<SampleAsset>();
        let server = app.world().resource::<AssetServer>();
        let mut assets = Assets::<SampleAsset>::default();
        let pcm = Arc::new(Pcm {
            data: vec![100; 480].into(),
            channels: 1,
            rate: 48000,
            loop_frames: None,
        });
        let handle = assets.add(SampleAsset(pcm.clone()));
        let (sender, _receiver) = sync_channel(128);
        let mut state = AudioState {
            catalog: Handle::default(),
            sender,
            finished: Arc::new(std::array::from_fn(|_| AtomicU64::new(0))),
            voices: std::array::from_fn(|_| None),
            cache: HashMap::from([(
                "sample".into(),
                Cached {
                    handle,
                    bytes: 960,
                    used: 0,
                },
            )]),
            failed: HashSet::new(),
            warned: HashSet::new(),
            recent: HashMap::new(),
            repeats: HashMap::new(),
            warm_class: None,
            warm: VecDeque::new(),
            scene: Scene::default(),
            token: 0,
            frame: 0,
            reported: false,
        };
        let mut catalog = Catalog {
            animations: default(),
            version: 1,
            events: HashMap::new(),
            samples: HashMap::from([("sample".into(), 960)]),
            maps: HashMap::new(),
        };
        for n in 0..100 {
            catalog.events.insert(
                format!("spell{n}"),
                Event {
                    samples: vec!["sample".into()],
                    looping: false,
                    max_voices: 3,
                },
            );
        }
        let mut request = Request::bed("spell0", "test".into(), Bus::Effects, None);
        request.key = None;
        for n in 0..1000 {
            request.cue = format!("spell{}", n % 100);
            state.start(
                &request,
                &catalog,
                server,
                &mut assets,
                n as f64,
                Vec3::ZERO,
                Vec3::X,
                [1.; 3],
            );
        }
        assert_eq!(state.voices.iter().flatten().count(), 48);
        assert!(state.voices[..16].iter().all(Option::is_none));
        for bus in [Bus::Music, Bus::Ambient] {
            request.bus = bus;
            request.key = Some(format!("bed:{}", bus.index()));
            request.cue = "spell99".into();
            assert!(state.start(
                &request,
                &catalog,
                server,
                &mut assets,
                1001.,
                Vec3::ZERO,
                Vec3::X,
                [1.; 3]
            ));
        }
        assert_eq!(state.voices.iter().flatten().count(), 50);
        // One PCM allocation is referenced by every queued voice.
        assert_eq!(Arc::strong_count(&pcm), 52);
        assert_eq!(state.cache.len(), 1);
        // All resident bytes are pinned by queued/playing voices: a new load
        // must wait, rather than exceeding the budget or evicting live PCM.
        state.cache.get_mut("sample").unwrap().bytes = CACHE_BYTES;
        catalog.samples.insert("other.wav".into(), 960);
        assert!(state
            .sample("other.wav", &catalog, server, &mut assets)
            .is_none());
        assert_eq!(state.cache.len(), 1);
        assert!(assets.get(&state.cache["sample"].handle).is_some());
    }
}
