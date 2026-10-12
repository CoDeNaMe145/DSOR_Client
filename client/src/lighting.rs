//! The look of a level, from the 2018 data: its global light, ambient light, fog,
//! bloom, colour grading and vignette, and the lights placed in it
//! (tools/export_ambience.py -> `maps/<map>.ambience.json`, the level's default
//! `_Instance_AmbienceBubble` and its `_Instance_Light` rows).
//!
//! Nebula lights in gamma space and shows the result without tonemapping:
//!   colour = albedo x (sat(N.L) x LightColor x LightIntensity + LightAmbient
//!            + sat(BackLightFactor - N.L) x LightOppositeColor x LightIntensity)
//!            + point lights, each LightColor x LightIntensity x sat(N.L) x sat(1 - d / range)
//! EVIDENCE: shaders_sm30 "lightsources" GlobalLight / PointLight ps_3_0, the
//!   "standard" Solid ps_3_0 (lit colour = light buffer x DiffMap0) and "pe_compose"
//!   (no curve, only saturation, balance, bloom, vignette and fade).
//! Bevy lights linearly and divides the diffuse by pi, so each term becomes its
//! linear value in Bevy's units (see `UNIT`): what the 2018 renderer shows as `v`
//! Bevy shows as `v` too once the camera is not tonemapped.
//!
//! F2 opens a panel that scales each term (Shift: ten steps at once); 1 everywhere
//! is the level's own look. The factors are kept in the browser (localStorage) or,
//! natively, in `look.json` in the working directory.

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraState};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{DirectionalLight, GlobalAmbientLight, PointLight};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::post_process::effect_stack::Vignette;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::ui::RelativeCursorPosition;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use bevy::render::view::{ColorGrading, ColorGradingGlobal};
use serde::{Deserialize, Serialize};

use crate::map::{CurrentMap, CELL};

/// One Nebula light unit (a light buffer value of 1) in Bevy's units: the inverse
/// of the camera's default exposure (EV100 9.7, `Exposure::BLENDER`).
const UNIT: f32 = 1.2 * 831.746; // 1.2 x 2^9.7
/// Bevy's diffuse ambient is `EnvBRDFApprox(albedo, F_AB(1, NdotV))` x ambient:
/// 0.452 x albedo at roughness 1 (Karis' fit); Nebula's is albedo x ambient.
const AMBIENT_BRDF: f32 = 0.452;
/// The light that follows the player (LocalLightColor x LocalLightIntensity):
/// `_Template_RemotePlayer.PlayerLightOffset` (1, 1, -0.5) from the player.
/// UNVERIFIED: its range (the data has none).
const PLAYER_LIGHT_OFFSET: Vec3 = Vec3::new(1.0, 1.0, -0.5);
const PLAYER_LIGHT_RANGE: f32 = 8.0;

/// Gamma value -> linear, per channel (the sRGB curve, also above 1).
fn linear(c: [f32; 3], k: f32) -> LinearRgba {
    let l = Color::srgb(c[0] * k, c[1] * k, c[2] * k).to_linear();
    LinearRgba::rgb(l.red, l.green, l.blue)
}

/// A Nebula point light: gamma colour x intensity, falling off linearly to its
/// range. Bevy's falloff is replaced by Nebula's (`NebulaFalloff`: sat(1 - d/r),
/// ^2.2 as the light is added in gamma space), so the light is its colour x
/// intensity at the centre and nothing at its range, with no inverse-square hot
/// spot near it: matched at half range with bevy's 1/d^2, a lamp lit the wall
/// beside it ~9 times too bright ("trop emissive les lights").
/// E / pi x exposure = linear(colour x intensity) x f: intensity / (4 pi) = pi x UNIT.
pub fn point_light(color: [f32; 3], intensity: f32, range: f32) -> PointLight {
    PointLight {
        color: Color::LinearRgba(linear(color, intensity)),
        intensity: 4.0 * std::f32::consts::PI.powi(2) * UNIT,
        range,
        shadow_maps_enabled: false,
        ..default()
    }
}

/// Bevy's `getDistanceAttenuation` (bevy_pbr::lighting) replaced, once the module
/// is loaded, by Nebula's linear point light falloff. Point and spot lights only.
/// EVIDENCE: the frame's Lights batch (b_empty, default.xml) and docs/fx.md
///   ("Nebula's linear falloff").
fn nebula_falloff(mut shaders: ResMut<Assets<bevy::shader::Shader>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let target = bevy::shader::ShaderImport::Custom("bevy_pbr::lighting".into());
    let Some(id) = shaders.iter().find(|(_, s)| s.import_path == target).map(|(id, _)| id) else { return };
    *done = true;
    let Some(mut shader) = shaders.get_mut(id) else { return };
    let bevy::shader::Source::Wgsl(src) = &shader.source else { return };
    let old = "fn getDistanceAttenuation(distanceSquare: f32, inverseRangeSquared: f32) -> f32 {\n    return getRangeFalloff(distanceSquare, inverseRangeSquared) * 1.0 / max(distanceSquare, 0.0001);\n}";
    if !src.contains(old) {
        warn!("bevy_pbr::lighting: getDistanceAttenuation not as expected; point lights keep bevy's falloff");
        return;
    }
    let new = "fn getDistanceAttenuation(distanceSquare: f32, inverseRangeSquared: f32) -> f32 {\n    // Nebula: linear to the range, in gamma space (crate::lighting).\n    return pow(saturate(1.0 - sqrt(distanceSquare * inverseRangeSquared)), 2.2);\n}";
    let patched = src.replace(old, new);
    shader.source = bevy::shader::Source::Wgsl(patched.into());
    info!("point lights: Nebula's linear falloff");
}

/// An ambience bubble's settings (the fields the client uses; names as in the
/// level database).
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase", default)]
pub struct Bubble {
    pub light_color: [f32; 4],
    pub light_opposite_color: [f32; 4],
    pub light_intensity: f32,
    pub back_light_factor: f32,
    pub light_ambient: [f32; 4],
    pub saturation: f32,
    pub bloom_scale: f32,
    pub bloom_color: [f32; 4],
    pub bright_pass_threshold: f32,
    pub fog_color: [f32; 4],
    pub fog_intensity: f32,
    pub fog_near_dist: f32,
    pub fog_far_dist: f32,
    pub local_light_color: [f32; 4],
    pub local_light_intensity: f32,
    /// Custom player-light range; not present in original ambience assets.
    pub local_light_range: f32,
    pub vignette_intensity: f32,
    pub vignette_size: f32,
    pub vignette_color: [f32; 4],
    /// Seconds to fade into this bubble's look.
    #[serde(rename = "PEFadeTime")]
    pub fade_time: f32,
    /// The light's transform z axis: towards the light.
    #[serde(rename = "dir")]
    pub dir: Option<[f32; 3]>,
}

fn mix4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

impl Bubble {
    /// `self` faded `t` (0..1) of the way to `to`.
    fn mix(&self, to: &Bubble, t: f32) -> Bubble {
        let f = |a: f32, b: f32| a + (b - a) * t;
        let dir = match (self.dir, to.dir) {
            (Some(a), Some(b)) => Some(Vec3::from_array(a).lerp(Vec3::from_array(b), t).normalize_or(Vec3::Y).to_array()),
            (a, b) => b.or(a),
        };
        Bubble {
            light_color: mix4(self.light_color, to.light_color, t),
            light_opposite_color: mix4(self.light_opposite_color, to.light_opposite_color, t),
            light_intensity: f(self.light_intensity, to.light_intensity),
            back_light_factor: f(self.back_light_factor, to.back_light_factor),
            light_ambient: mix4(self.light_ambient, to.light_ambient, t),
            saturation: f(self.saturation, to.saturation),
            bloom_scale: f(self.bloom_scale, to.bloom_scale),
            bloom_color: mix4(self.bloom_color, to.bloom_color, t),
            bright_pass_threshold: f(self.bright_pass_threshold, to.bright_pass_threshold),
            fog_color: mix4(self.fog_color, to.fog_color, t),
            fog_intensity: f(self.fog_intensity, to.fog_intensity),
            fog_near_dist: f(self.fog_near_dist, to.fog_near_dist),
            fog_far_dist: f(self.fog_far_dist, to.fog_far_dist),
            local_light_color: mix4(self.local_light_color, to.local_light_color, t),
            local_light_intensity: f(self.local_light_intensity, to.local_light_intensity),
            local_light_range: f(self.local_light_range, to.local_light_range),
            vignette_intensity: f(self.vignette_intensity, to.vignette_intensity),
            vignette_size: f(self.vignette_size, to.vignette_size),
            vignette_color: mix4(self.vignette_color, to.vignette_color, t),
            fade_time: to.fade_time,
            dir,
        }
    }
}

/// A bubble with a place of its own: inside its volume (a unit box or a sphere
/// of diameter 1 through its transform), the level takes its look, the highest
/// priority winning, faded in over its PEFadeTime.
/// UNVERIFIED: the sphere's unit radius (0.5, as the box's half side).
#[derive(Deserialize, Clone, Debug)]
pub struct LocalBubble {
    #[serde(flatten)]
    pub look: Bubble,
    pub m: Option<[f32; 16]>,
    #[serde(default)]
    pub priority: i32,
    /// The event set row it belongs to; -1 when always there.
    #[serde(default)]
    pub event: i32,
    #[serde(default)]
    pub shape: String,
}

impl LocalBubble {
    fn contains(&self, p: Vec3) -> bool {
        let Some(m) = self.m else { return false };
        let local = Mat4::from_cols_array(&m).inverse().transform_point3(p);
        if self.shape == "Sphere" {
            local.length() <= 0.5
        } else {
            local.abs().max_element() <= 0.5
        }
    }
}

impl Default for Bubble {
    /// Kingshill's (a0200_kingscity), for a level without data.
    fn default() -> Self {
        Self {
            light_color: [0.851, 0.757, 0.682, 1.0],
            light_opposite_color: [0.176, 0.231, 0.349, 1.0],
            light_intensity: 1.3,
            back_light_factor: 0.3,
            light_ambient: [0.855, 0.831, 0.8, 0.98],
            saturation: 1.0,
            bloom_scale: 1.0,
            bloom_color: [1.0, 0.71, 0.502, 1.0],
            bright_pass_threshold: 0.7,
            fog_color: [0.486, 0.745, 0.78, 1.0],
            fog_intensity: 1.0,
            fog_near_dist: 25.0,
            fog_far_dist: 50.0,
            local_light_color: [1.0, 0.71, 0.502, 1.0],
            local_light_intensity: 0.25,
            local_light_range: PLAYER_LIGHT_RANGE,
            vignette_intensity: 1.0,
            vignette_size: 2.0,
            vignette_color: [0.0, 0.0, 0.0, 1.0],
            fade_time: 2.0,
            dir: Some([-0.136, 0.721, -0.679]),
        }
    }
}

/// A light placed in the level (`_Instance_Light`, always-on rows).
#[derive(Deserialize, Clone, Debug)]
pub struct LevelLight {
    /// 0 point, 1 projected spot (not drawn: see crate docs/fx.md).
    pub t: u8,
    pub p: [f32; 3],
    pub color: [f32; 3],
    pub i: f32,
    pub r: f32,
    /// [frequency, intensity] when it flickers.
    pub flicker: Option<[f32; 2]>,
}

#[derive(Asset, TypePath, Deserialize, Debug, Default)]
pub struct Ambience {
    pub global: Option<Bubble>,
    #[serde(default)]
    pub bubbles: Vec<LocalBubble>,
    #[serde(default)]
    pub lights: Vec<LevelLight>,
}

#[derive(Default, TypePath)]
pub struct AmbienceLoader;

impl AssetLoader for AmbienceLoader {
    type Asset = Ambience;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Ambience, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }

    fn extensions(&self) -> &[&str] {
        &["ambience.json"]
    }
}

/// The current level's ambience and what of it has been applied.
#[derive(Resource, Default)]
struct Level {
    map: String,
    ambience: Handle<Ambience>,
    /// The look in force (the data's, or the default until it loads), and the
    /// fade it is in: from, to, which local bubble (None: the level's own), done.
    bubble: Bubble,
    fade: Option<(Bubble, Bubble, f32)>,
    inside: Option<usize>,
    bubble_loaded: bool,
    lights_placed: bool,
}

/// The main directional light (spawned by main.rs): the level's global light.
#[derive(Component)]
pub struct Sun;

/// The global light's back light: from the opposite direction, no shadow.
#[derive(Component)]
struct BackLight;

#[derive(Component)]
struct PlayerLight;

/// A level light: its intensity at factor 1, and its flicker (frequency, depth,
/// phase) when it has one.
#[derive(Component)]
struct LevelLightPower {
    base: f32,
    flicker: Option<(f32, f32, f32)>,
}

/// Scale factors over the level's own look, tunable in game (F2).
#[derive(Resource, Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Lighting {
    pub sun: f32,
    pub back: f32,
    pub ambient: f32,
    pub lights: f32,
    /// Exposure compensation, stops.
    pub exposure: f32,
    pub fog: f32,
    pub bloom: f32,
    pub saturation: f32,
    pub vignette: f32,
    /// The 2018 renderer clips; Bevy's TonyMcMapface rolls highlights off.
    pub tonemapping: bool,
    pub shadows: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            sun: 1.0,
            back: 1.0,
            ambient: 1.0,
            lights: 1.0,
            exposure: 0.0,
            fog: 1.0,
            bloom: 1.0,
            saturation: 1.0,
            vignette: 1.0,
            tonemapping: false,
            shadows: true,
        }
    }
}

/// One tunable line: label, step, read, write.
struct Field {
    label: &'static str,
    step: f32,
    get: fn(&Lighting) -> f32,
    set: fn(&mut Lighting, f32),
}

const FIELDS: &[Field] = &[
    Field { label: "Sun x", step: 0.05, get: |l| l.sun, set: |l, v| l.sun = v.max(0.0) },
    Field { label: "Back light x", step: 0.05, get: |l| l.back, set: |l, v| l.back = v.max(0.0) },
    Field { label: "Ambient x", step: 0.05, get: |l| l.ambient, set: |l, v| l.ambient = v.max(0.0) },
    Field { label: "Level lights x", step: 0.05, get: |l| l.lights, set: |l, v| l.lights = v.max(0.0) },
    Field { label: "Exposure", step: 0.05, get: |l| l.exposure, set: |l, v| l.exposure = v.clamp(-5.0, 5.0) },
    Field { label: "Fog x", step: 0.05, get: |l| l.fog, set: |l, v| l.fog = v.clamp(0.0, 2.0) },
    Field { label: "Bloom x", step: 0.05, get: |l| l.bloom, set: |l, v| l.bloom = v.clamp(0.0, 4.0) },
    Field { label: "Saturation x", step: 0.02, get: |l| l.saturation, set: |l, v| l.saturation = v.clamp(0.0, 3.0) },
    Field { label: "Vignette x", step: 0.05, get: |l| l.vignette, set: |l, v| l.vignette = v.clamp(0.0, 2.0) },
    Field {
        label: "Tonemapping (0/1)",
        step: 1.0,
        get: |l| if l.tonemapping { 1.0 } else { 0.0 },
        set: |l, v| l.tonemapping = v >= 0.5,
    },
    Field {
        label: "Shadows (0/1)",
        step: 1.0,
        get: |l| if l.shadows { 1.0 } else { 0.0 },
        set: |l, v| l.shadows = v >= 0.5,
    },
];

const STORAGE_KEY: &str = "dsor.look";

fn load_saved() -> Option<Lighting> {
    #[cfg(target_arch = "wasm32")]
    {
        let raw = web_sys::window()?.local_storage().ok()??.get_item(STORAGE_KEY).ok()??;
        serde_json::from_str(&raw).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = STORAGE_KEY;
        serde_json::from_slice(&std::fs::read("look.json").ok()?).ok()
    }
}

fn save(l: &Lighting) {
    let Ok(raw) = serde_json::to_string(l) else { return };
    #[cfg(target_arch = "wasm32")]
    if let Some(Ok(Some(store))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = store.set_item(STORAGE_KEY, &raw);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::fs::write("look.json", &raw);
    info!("look: {raw}");
}

#[derive(Component)]
struct Panel;
#[derive(Component)]
struct ValueText(usize);
#[derive(Component)]
struct NumericField(usize);
#[derive(Component)]
struct StepButton(usize, f32);


#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum StudioPage {
    #[default]
    Overview,
    Lighting,
    Regions,
    Atmosphere,
    Presets,
}

#[derive(Component)]
struct StudioPreviewNav(StudioPage);

#[derive(Component)]
struct StudioPreviewPageBody(StudioPage);
#[derive(Component)]
struct StudioPreviewRoot;

#[derive(Component)]
struct StudioPreviewAccentSurface;

#[derive(Component, Clone, Copy)]
struct StudioPreviewOpacityStep(f32);

#[derive(Component, Clone, Copy)]
struct StudioPreviewThemeChoice(StudioPreviewTheme);

#[derive(Component)]
struct StudioPreviewOpacityLabel;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum StudioPreviewTheme {
    #[default]
    Slate,
    Red,
    Blue,
    Green,
}

impl StudioPreviewTheme {
    fn accent(self) -> (f32, f32, f32) {
        match self {
            Self::Slate => (0.82, 0.64, 0.34),
            Self::Red => (0.92, 0.18, 0.24),
            Self::Blue => (0.20, 0.53, 0.98),
            Self::Green => (0.18, 0.76, 0.42),
        }
    }

    fn surface_tint(self) -> ((f32, f32, f32), f32) {
        match self {
            Self::Slate => ((0.055, 0.067, 0.09), 0.0),
            Self::Red => ((0.32, 0.025, 0.045), 0.36),
            Self::Blue => ((0.025, 0.085, 0.32), 0.36),
            Self::Green => ((0.025, 0.20, 0.085), 0.36),
        }
    }
}

#[derive(Resource, Clone, Copy)]
struct StudioPreviewAppearance {
    opacity: f32,
    theme: StudioPreviewTheme,
}

impl Default for StudioPreviewAppearance {
    fn default() -> Self {
        Self {
            opacity: 0.8,
            theme: StudioPreviewTheme::Slate,
        }
    }
}

#[derive(Component)]
struct StudioPreviewDragHandle;

#[derive(Component, Clone, Copy)]
struct StudioPreviewResizeHandle(StudioPreviewResizeEdge);

#[derive(Clone, Copy, Debug)]
enum StudioPreviewResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Default)]
struct StudioPreviewWindowState {
    dragging: bool,
    resizing: bool,
    drag_offset: Vec2,
    resize_start_cursor: Vec2,
    resize_start_size: Vec2,
    resize_start_position: Vec2,
    resize_edge: Option<StudioPreviewResizeEdge>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AmbienceField {
    LightIntensity,
    BackLightFactor,
    DirectionX,
    DirectionY,
    DirectionZ,
    LightColorR,
    LightColorG,
    LightColorB,
    BackColorR,
    BackColorG,
    BackColorB,
    AmbientColorR,
    AmbientColorG,
    AmbientColorB,
    FogColorR,
    FogColorG,
    FogColorB,
    FogIntensity,
    FogNear,
    FogFar,
    PlayerColorR,
    PlayerColorG,
    PlayerColorB,
    PlayerLightIntensity,
    PlayerLightRange,
    Saturation,
    BloomScale,
    BrightPassThreshold,
    VignetteIntensity,
    VignetteSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AmbienceColorTarget {
    Key,
    Back,
    Ambient,
    Fog,
    Player,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AmbienceGroup {
    Lighting,
    Colors,
    Fog,
    PlayerLight,
    PostProcessing,
}

const AMBIENCE_GROUPS: &[AmbienceGroup] = &[
    AmbienceGroup::Lighting,
    AmbienceGroup::Colors,
    AmbienceGroup::Fog,
    AmbienceGroup::PlayerLight,
    AmbienceGroup::PostProcessing,
];

impl AmbienceGroup {
    fn index(self) -> usize {
        match self {
            Self::Lighting => 0,
            Self::Colors => 1,
            Self::Fog => 2,
            Self::PlayerLight => 3,
            Self::PostProcessing => 4,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Lighting => "Lighting & Direction",
            Self::Colors => "Light Colors",
            Self::Fog => "Fog",
            Self::PlayerLight => "Player Light",
            Self::PostProcessing => "Post Processing",
        }
    }

    fn fields(self) -> &'static [AmbienceField] {
        use AmbienceField::*;
        match self {
            Self::Lighting => &[
                LightIntensity, BackLightFactor, DirectionX, DirectionY, DirectionZ,
            ],
            Self::Colors => &[
                LightColorR, LightColorG, LightColorB,
                BackColorR, BackColorG, BackColorB,
                AmbientColorR, AmbientColorG, AmbientColorB,
            ],
            Self::Fog => &[
                FogColorR, FogColorG, FogColorB, FogIntensity, FogNear, FogFar,
            ],
            Self::PlayerLight => &[
                PlayerColorR, PlayerColorG, PlayerColorB,
                PlayerLightIntensity, PlayerLightRange,
            ],
            Self::PostProcessing => &[
                Saturation, BloomScale, BrightPassThreshold,
                VignetteIntensity, VignetteSize,
            ],
        }
    }
}

impl AmbienceColorTarget {
    fn color(self, bubble: &Bubble) -> [f32; 4] {
        match self {
            Self::Key => bubble.light_color,
            Self::Back => bubble.light_opposite_color,
            Self::Ambient => bubble.light_ambient,
            Self::Fog => bubble.fog_color,
            Self::Player => bubble.local_light_color,
        }
    }
}

impl AmbienceField {
    fn label(self) -> &'static str {
        use AmbienceField::*;
        match self {
            LightIntensity => "Key light intensity",
            BackLightFactor => "Back light factor",
            DirectionX => "Light direction X",
            DirectionY => "Light direction Y",
            DirectionZ => "Light direction Z",
            LightColorR => "Key light R",
            LightColorG => "Key light G",
            LightColorB => "Key light B",
            BackColorR => "Back light R",
            BackColorG => "Back light G",
            BackColorB => "Back light B",
            AmbientColorR => "Ambient R",
            AmbientColorG => "Ambient G",
            AmbientColorB => "Ambient B",
            FogColorR => "Fog R",
            FogColorG => "Fog G",
            FogColorB => "Fog B",
            FogIntensity => "Fog intensity",
            FogNear => "Fog near distance",
            FogFar => "Fog far distance",
            PlayerColorR => "Player light R",
            PlayerColorG => "Player light G",
            PlayerColorB => "Player light B",
            PlayerLightIntensity => "Player light intensity",
            PlayerLightRange => "Player light range",
            Saturation => "Saturation",
            BloomScale => "Bloom scale",
            BrightPassThreshold => "Bright-pass threshold",
            VignetteIntensity => "Vignette intensity",
            VignetteSize => "Vignette size",
        }
    }

    fn step(self) -> f32 {
        use AmbienceField::*;
        match self {
            FogNear | FogFar | PlayerLightRange => 0.5,
            Saturation => 0.02,
            _ => 0.05,
        }
    }

    fn color_channel(self) -> Option<(AmbienceColorTarget, usize)> {
        use AmbienceColorTarget::*;
        use AmbienceField::*;
        match self {
            LightColorR => Some((Key, 0)),
            LightColorG => Some((Key, 1)),
            LightColorB => Some((Key, 2)),
            BackColorR => Some((Back, 0)),
            BackColorG => Some((Back, 1)),
            BackColorB => Some((Back, 2)),
            AmbientColorR => Some((Ambient, 0)),
            AmbientColorG => Some((Ambient, 1)),
            AmbientColorB => Some((Ambient, 2)),
            FogColorR => Some((Fog, 0)),
            FogColorG => Some((Fog, 1)),
            FogColorB => Some((Fog, 2)),
            PlayerColorR => Some((Player, 0)),
            PlayerColorG => Some((Player, 1)),
            PlayerColorB => Some((Player, 2)),
            _ => None,
        }
    }

    fn value(self, bubble: &Bubble) -> f32 {
        if let Some((target, channel)) = self.color_channel() {
            return target.color(bubble)[channel];
        }

        use AmbienceField::*;
        match self {
            LightIntensity => bubble.light_intensity,
            BackLightFactor => bubble.back_light_factor,
            DirectionX => bubble.dir.unwrap_or([-0.136, 0.721, -0.679])[0],
            DirectionY => bubble.dir.unwrap_or([-0.136, 0.721, -0.679])[1],
            DirectionZ => bubble.dir.unwrap_or([-0.136, 0.721, -0.679])[2],
            FogIntensity => bubble.fog_intensity,
            FogNear => bubble.fog_near_dist,
            FogFar => bubble.fog_far_dist,
            PlayerLightIntensity => bubble.local_light_intensity,
            PlayerLightRange => bubble.local_light_range,
            Saturation => bubble.saturation,
            BloomScale => bubble.bloom_scale,
            BrightPassThreshold => bubble.bright_pass_threshold,
            VignetteIntensity => bubble.vignette_intensity,
            VignetteSize => bubble.vignette_size,
            _ => unreachable!("color fields are handled above"),
        }
    }
}

#[derive(Component)]
struct AmbienceNumericField(AmbienceField);
#[derive(Component)]
struct AmbienceValueText(AmbienceField);
#[derive(Component, Clone, Copy)]
struct AmbienceStepButton(AmbienceField, f32);
#[derive(Component)]
struct MapAmbienceToggle;
#[derive(Component)]
struct MapAmbienceToggleText;
#[derive(Component)]
struct MapAmbienceSection;
#[derive(Component)]
struct AmbienceTargetLabel;
#[derive(Component, Clone, Copy)]
struct AmbienceTargetStep(i8);
#[derive(Component)]
struct AmbienceGroupToggle(AmbienceGroup);
#[derive(Component)]
struct AmbienceGroupTitle(AmbienceGroup);
#[derive(Component)]
struct AmbienceGroupBody(AmbienceGroup);

#[derive(Resource, Default)]
struct AmbienceNumericEditor {
    field: Option<AmbienceField>,
    draft: String,
    replace_on_next_input: bool,
}

#[derive(Resource, Default)]
struct MapAmbienceUi {
    expanded: bool,
    groups: [bool; 5],
    selected_bubble: Option<usize>,
    map_name: Option<String>,
}

#[derive(Component)]
struct PresetNameField;
#[derive(Component)]
struct PresetNameText;
#[derive(Component)]
struct PresetStatusText;
#[derive(Component)]
struct PresetList;
#[derive(Component)]
struct PresetRow;
#[derive(Component)]
struct PresetChoice(String);

#[derive(Component, Clone, Copy)]
enum PresetAction {
    SaveAs,
    Overwrite,
    Rename,
    Delete,
    ClearSelection,
}

#[derive(Resource, Default)]
struct PresetNameEditor {
    focused: bool,
    draft: String,
    replace_on_next_input: bool,
}

#[derive(Resource, Default)]
struct PresetUiState {
    status: String,
    pending_delete_id: Option<String>,
}

#[derive(Resource, Default)]
pub struct LightingPanelState {
    pub open: bool,
}

#[derive(Resource, Default)]
pub struct LightingInputState {
    pub text_entry_active: bool,
}

#[derive(Resource, Default)]
struct NumericEditor {
    field: Option<usize>,
    draft: String,
    replace_on_next_input: bool,
}

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Ambience>()
            .init_asset_loader::<AmbienceLoader>()
            .init_resource::<Level>()
            .init_resource::<NumericEditor>()
            .init_resource::<PresetNameEditor>()
            .init_resource::<PresetUiState>()
            .init_resource::<LightingPanelState>()
            .init_resource::<LightingInputState>()
            .init_resource::<AmbienceNumericEditor>()
            .init_resource::<MapAmbienceUi>()
            .insert_resource(load_saved().unwrap_or_default())
            .insert_resource(crate::lighting_presets::LightingStudioStore::load())
            .add_systems(PreUpdate, pause_camera_scroll_speed_while_panel_open)
            .add_systems(Update, nebula_falloff)
            .add_systems(
                Update,
                (update_studio_preview_navigation, update_studio_preview_appearance).chain(),
            )
            .add_systems(Update, update_studio_preview_window)
            .add_systems(Update, scroll_studio_preview_pages)
            .add_systems(Update, draw_selected_ambience_area)
            .add_systems(Startup, (spawn_panel, spawn_back_light))
            .add_systems(
                Update,
                (
                    (
    (
        toggle_panel,
        keep_panel_cursor,
        scroll_lighting_panel,
        blur_text_editors_on_ui_click,
        toggle_map_ambience,
        toggle_ambience_group,
        cycle_ambience_target,
        press_buttons,
        edit_numeric_fields,
        edit_ambience_fields,
        step_ambience_fields,
        edit_preset_name,
    ).chain(),
    (
        handle_preset_actions,
        select_preset_from_list,
        update_lighting_input_state,
        pause_free_camera_while_typing,
        refresh_values,
        refresh_ambience_target_label,
        refresh_ambience_values,
        refresh_preset_name,
        refresh_preset_status,
        rebuild_preset_list,
    ).chain(),
).chain().before(crate::net::NetSystems),
                    (follow_level, apply_look, place_level_lights.run_if(resource_exists::<CurrentMap>)).chain(),
                    flicker,
                    player_light,
                ),
            );
    }
}

/// CONTRACT: not on WebGL2. bevy there takes one directional light; with a
///   second one it kept the back light and dropped the sun ("The amount of
///   directional lights of 2 is exceeding the supported limit of 1": no shadows,
///   flat light in the browser).
fn spawn_back_light(mut commands: Commands) {
    if crate::WEBGL2 {
        return;
    }
    commands.spawn((BackLight, DirectionalLight { shadow_maps_enabled: false, ..default() }, Transform::default()));
}

/// A new level: load its ambience.
/// A new level: load its ambience. Then follow the bubble the player (or the
/// camera's focus) stands in.
#[allow(clippy::too_many_arguments)]
fn follow_level(
    current: Option<Res<CurrentMap>>,
    studio: Res<crate::lighting_presets::LightingStudioStore>,
    mut level: ResMut<Level>,
    assets: Res<AssetServer>,
    ambiences: Res<Assets<Ambience>>,
    time: Res<Time>,
    manifests: Res<Assets<crate::map::MapManifest>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    players: Query<&GlobalTransform, With<crate::net::LocalPlayer>>,
) {
    let Some(current) = current else { return };
    if level.map != current.name {
        level.map = current.name.clone();
        level.ambience = assets.load(format!("maps/{}.ambience.json", current.name));
        level.bubble = Bubble::default();
        level.fade = None;
        level.inside = None;
        level.bubble_loaded = false;
        level.lights_placed = false;
    }
    let Some(ambience) = ambiences.get(&level.ambience) else { return };
    let map_overrides = studio.working_map_overrides.get(&current.name);
    let global_source = ambience.global.clone().unwrap_or_default();
    let global = crate::lighting_presets::resolve_bubble(&global_source, map_overrides, None);
    if !level.bubble_loaded {
        level.bubble = global.clone();
        level.bubble_loaded = true;
    }
    if ambience.bubbles.is_empty() && studio.is_changed() {
        let from = level.bubble.clone();
        level.fade = Some((from, global.clone(), 0.0));
    }
    if !ambience.bubbles.is_empty() {
        let ground = manifests.get(&current.manifest).map(|m| m.center[1]).unwrap_or(0.0);
        let player_position = players.iter().next().map(|p| p.translation());
        let focus = cameras
            .iter()
            .next()
            .map(|cam| crate::map::camera_focus(cam, player_position, ground));
        let inside = focus.and_then(|p| {
            ambience
                .bubbles
                .iter()
                .enumerate()
                .filter(|(_, b)| b.event == -1 && b.contains(p))
                .max_by_key(|(_, b)| b.priority)
                .map(|(i, _)| i)
        });

        // Opt-in diagnostic: log only automatic region transitions.
        if std::env::var_os("DSOR_TRACE_AMBIENCE").is_some()
            && inside != level.inside
        {
            let containing: Vec<String> = focus
                .map(|point| {
                    ambience
                        .bubbles
                        .iter()
                        .enumerate()
                        .filter(|(_, bubble)| bubble.event == -1 && bubble.contains(point))
                        .map(|(index, bubble)| {
                            format!(
                                "Area {} (index {}, priority {})",
                                ambience.bubbles
                                    .iter()
                                    .take(index + 1)
                                    .filter(|b| b.event == -1 && b.m.is_some())
                                    .count(),
                                index,
                                bubble.priority,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();

            warn!(
                "DSOR_TRACE_AMBIENCE map={} player_position={:?} focus={:?} previous_area={:?} next_area={:?} containing={:?}",
                current.name,
                player_position,
                focus,
                level.inside.map(|i| i + 1),
                inside.map(|i| i + 1),
                containing,
            );
        }

        if inside != level.inside || studio.is_changed() {
            level.inside = inside;
            let to = if let Some(i) = inside {
                let source_area = &ambience.bubbles[i];
                crate::lighting_presets::resolve_bubble(
                    &source_area.look,
                    map_overrides,
                    Some((i, source_area)),
                )
            } else {
                global.clone()
            };
            let from = level.bubble.clone();
            level.fade = Some((from, to, 0.0));
        }
    }
    // Only while fading: touching `level` marks it changed, which re-applies the look.
    if level.fade.is_some() {
        let Some((from, to, t)) = level.fade.take() else { return };
        let t = t + time.delta_secs() / to.fade_time.max(0.01);
        level.bubble = from.mix(&to, t.min(1.0));
        if t < 1.0 {
            level.fade = Some((from, to, t));
        }
    }
}

#[allow(clippy::type_complexity)]
fn apply_look(
    mut commands: Commands,
    lighting: Res<Lighting>,
    level: Res<Level>,
    mut suns: Query<(&mut DirectionalLight, &mut Transform), (With<Sun>, Without<BackLight>)>,
    mut backs: Query<(&mut DirectionalLight, &mut Transform), (With<BackLight>, Without<Sun>)>,
    mut ambient: ResMut<GlobalAmbientLight>,
    cameras: Query<(Entity, Has<Bloom>), With<Camera3d>>,
    mut fresh: Local<Vec<Entity>>,
) {
    let new_camera = cameras.iter().any(|(e, _)| !fresh.contains(&e));
    if !lighting.is_changed() && !level.is_changed() && !new_camera {
        return;
    }
    fresh.clear();
    fresh.extend(cameras.iter().map(|(e, _)| e));
    let (l, b) = (&*lighting, &level.bubble);
    let rgb = |c: [f32; 4]| [c[0], c[1], c[2]];
    let toward = Vec3::from_array(b.dir.unwrap_or([-0.136, 0.721, -0.679])).normalize_or(Vec3::Y);
    let lit = |c: [f32; 4], k: f32| Color::LinearRgba(linear(rgb(c), k));
    for (mut sun, mut tf) in &mut suns {
        sun.color = lit(b.light_color, b.light_intensity);
        sun.illuminance = std::f32::consts::PI * UNIT * l.sun;
        sun.shadow_maps_enabled = l.shadows && !crate::flag("noshadows");
        *tf = Transform::default().looking_to(-toward, Vec3::Y);
    }
    for (mut back, mut tf) in &mut backs {
        // sat(BackLightFactor - N.L) is approximated by a light from -L.
        back.color = lit(b.light_opposite_color, b.light_intensity);
        back.illuminance = std::f32::consts::PI * UNIT * l.back;
        *tf = Transform::default().looking_to(toward, Vec3::Y);
    }
    let a = linear(rgb(b.light_ambient), 1.0);
    ambient.color = Color::LinearRgba(a);
    ambient.brightness = UNIT / AMBIENT_BRDF * l.ambient;

    // Fog: Nebula's factor is clamp((far - z) / (far - near), FogColor.w, 1).
    // UNVERIFIED: FogIntensity as the fog's opacity.
    let fog = DistanceFog {
        color: Color::srgba(b.fog_color[0], b.fog_color[1], b.fog_color[2], (b.fog_intensity * l.fog).clamp(0.0, 1.0)),
        falloff: FogFalloff::Linear { start: b.fog_near_dist, end: b.fog_far_dist.max(b.fog_near_dist + 0.1) },
        ..default()
    };
    // Bloom: Nebula's bright pass keeps lum(max(2c - threshold, 0)) of the scene,
    // added back blurred x BloomScale (pe_brightpassfilter, pe_compose).
    // Bevy's bloom has no tint (BloomColor) and thresholds linear values.
    let threshold = Color::srgb(b.bright_pass_threshold * 0.5, 0.0, 0.0).to_linear().red;
    let bloom = Bloom {
        intensity: 0.15 * b.bloom_scale * l.bloom,
        prefilter: BloomPrefilter { threshold, threshold_softness: 0.5 },
        composite_mode: BloomCompositeMode::Additive,
        ..Bloom::NATURAL
    };
    // Vignette: lerp(colour, VignetteColor, sat(d^VignetteSize x VignetteIntensity)),
    // d the distance to the screen centre in uv units. Bevy's with smoothness 1 and
    // full edge compensation is lerp(colour, color, (d / radius)^2).
    let vignette = Vignette {
        intensity: if b.vignette_intensity > 0.0 { l.vignette.min(1.0) } else { 0.0 },
        radius: 1.0 / (b.vignette_intensity * l.vignette.max(1.0)).max(1e-3).sqrt(),
        smoothness: 1.0,
        roundness: 1.0,
        edge_compensation: 1.0,
        color: Color::srgb(b.vignette_color[0], b.vignette_color[1], b.vignette_color[2]),
        ..default()
    };
    let grading = ColorGrading {
        global: ColorGradingGlobal { exposure: l.exposure, post_saturation: b.saturation * l.saturation, ..default() },
        ..default()
    };
    let tonemapping = if l.tonemapping { Tonemapping::TonyMcMapface } else { Tonemapping::None };
    for (e, _) in &cameras {
        let mut cam = commands.entity(e);
        cam.insert((fog.clone(), grading.clone(), vignette.clone(), tonemapping));
        // The 2018 client's own bloom (crate::nebula_bloom): from the opaque scene
        // only, as its frame shader does; bevy's took the effects too.
        cam.remove::<Bloom>();
        if crate::flag("nobloom") || b.bloom_scale * l.bloom <= 0.0 {
            cam.remove::<crate::nebula_bloom::NebulaBloom>();
        } else {
            cam.insert(crate::nebula_bloom::NebulaBloom {
                threshold: b.bright_pass_threshold,
                scale: b.bloom_scale * l.bloom,
                color: Vec4::from_array(b.bloom_color),
            });
        }
        let _ = &bloom;
    }
}

/// The level's own lights, each in the culling cell it stands in, so they go out
/// with the cell (crate::map::cull_cells) and with the map.
fn place_level_lights(
    mut commands: Commands,
    mut level: ResMut<Level>,
    mut current: ResMut<CurrentMap>,
    ambiences: Res<Assets<Ambience>>,
    lighting: Res<Lighting>,
) {
    if level.lights_placed || crate::flag("nolights") {
        return;
    }
    let (Some(root), Some(ambience)) = (current.root, ambiences.get(&level.ambience)) else { return };
    let mut placed = 0;
    for (n, light) in ambience.lights.iter().enumerate() {
        // Projected spots (LightType 1) need their texture and shadow: left out.
        if light.t != 0 || light.r <= 0.0 || light.i <= 0.0 {
            continue;
        }
        let at = crate::map::game_to_bevy(light.p);
        let key = ((at.x / CELL).floor() as i32, (at.z / CELL).floor() as i32);
        let cell = *current.cells.entry(key).or_insert_with(|| {
            commands.spawn((Name::new(format!("cell {key:?}")), Transform::default(), Visibility::default(), ChildOf(root))).id()
        });
        let pl = point_light(light.color, light.i, light.r);
        let power = LevelLightPower {
            base: pl.intensity,
            flicker: light.flicker.map(|[frequency, amount]| (frequency, amount.clamp(0.0, 1.0), n as f32 * 1.618)),
        };
        commands.spawn((PointLight { intensity: pl.intensity * lighting.lights, ..pl }, power, Transform::from_translation(at), ChildOf(cell)));
        placed += 1;
    }
    info!("level {}: {placed} lights", level.map);
    level.lights_placed = true;
}

/// Level lights at the panel's factor; flickering ones wander under their base,
/// `depth` deep. UNVERIFIED: the 2018 client's flicker curve.
fn flicker(time: Res<Time>, lighting: Res<Lighting>, mut lights: Query<(&mut PointLight, &LevelLightPower, &InheritedVisibility)>) {
    let t = time.elapsed_secs();
    for (mut l, power, shown) in &mut lights {
        let Some((frequency, depth, phase)) = power.flicker else {
            if lighting.is_changed() {
                l.intensity = power.base * lighting.lights;
            }
            continue;
        };
        if !shown.get() {
            continue;
        }
        let a = t * frequency + phase;
        let wander = 0.5 + 0.3 * a.sin() + 0.2 * (2.3 * a + 1.7).sin();
        l.intensity = power.base * lighting.lights * (1.0 - depth * 0.5 * wander);
    }
}

/// The light that follows the local player (the level's LocalLight).
fn player_light(
    mut commands: Commands,
    level: Res<Level>,
    lighting: Res<Lighting>,
    players: Query<Entity, Added<crate::net::LocalPlayer>>,
    mut lights: Query<&mut PointLight, With<PlayerLight>>,
) {
    let b = &level.bubble;
    let make = || {
        let mut l = point_light([b.local_light_color[0], b.local_light_color[1], b.local_light_color[2]], b.local_light_intensity, b.local_light_range);
        l.intensity *= lighting.lights;
        l
    };
    for p in &players {
        commands.spawn((PlayerLight, make(), Transform::from_translation(PLAYER_LIGHT_OFFSET), ChildOf(p)));
    }
    if level.is_changed() || lighting.is_changed() {
        for mut l in &mut lights {
            *l = make();
        }
    }
}



fn update_studio_preview_appearance(
    opacity_buttons: Query<
        (&Interaction, &StudioPreviewOpacityStep),
        Changed<Interaction>,
    >,
    theme_buttons: Query<
        (&Interaction, &StudioPreviewThemeChoice),
        Changed<Interaction>,
    >,
    mut opacity_labels: Query<&mut Text, With<StudioPreviewOpacityLabel>>,
    roots: Query<Entity, With<StudioPreviewRoot>>,
    children: Query<&Children>,
    nav_components: Query<&StudioPreviewNav>,
    page_bodies: Query<(&StudioPreviewPageBody, &Node)>,
    accent_components: Query<Entity, With<StudioPreviewAccentSurface>>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut appearance: Option<ResMut<StudioPreviewAppearance>>,
    mut original_colors: Local<Vec<(Entity, Color)>>,
) {
    let Some(mut appearance) = appearance else {
        return;
    };

    for (interaction, step) in &opacity_buttons {
        if *interaction == Interaction::Pressed {
            appearance.opacity = (appearance.opacity + step.0).clamp(0.4, 1.0);
        }
    }

    for (interaction, choice) in &theme_buttons {
        if *interaction == Interaction::Pressed {
            appearance.theme = choice.0;
        }
    }

    let opacity_text = format!("{}%", (appearance.opacity * 100.0).round() as u32);
    for mut label in &mut opacity_labels {
        if label.0 != opacity_text {
            label.0 = opacity_text.clone();
        }
    }

    let mut stack: Vec<Entity> = roots.iter().collect();
    let mut entities_to_style = Vec::new();

    while let Some(entity) = stack.pop() {
        entities_to_style.push(entity);
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
    }

    let (tint, mix) = appearance.theme.surface_tint();
    let accent = appearance.theme.accent();

    for entity in entities_to_style {
        let Ok(mut background) = backgrounds.get_mut(entity) else {
            continue;
        };

        let base_color = if let Some((_, color)) = original_colors
            .iter()
            .find(|(saved_entity, _)| *saved_entity == entity)
        {
            *color
        } else {
            let color = background.0;
            original_colors.push((entity, color));
            color
        };

        let base = base_color.to_srgba();

        let (r, g, b) = if let Ok(nav) = nav_components.get(entity) {
            let selected = page_bodies.iter().any(|(body, node)| {
                body.0 == nav.0 && node.display != bevy::ui::Display::None
            });
            let idle = (0.12, 0.14, 0.19);
            if selected {
                (
                    idle.0 + (accent.0 - idle.0) * 0.28,
                    idle.1 + (accent.1 - idle.1) * 0.28,
                    idle.2 + (accent.2 - idle.2) * 0.28,
                )
            } else {
                idle
            }
        } else if accent_components.get(entity).is_ok() {
            accent
        } else {
            (
                base.red + (tint.0 - base.red) * mix,
                base.green + (tint.1 - base.green) * mix,
                base.blue + (tint.2 - base.blue) * mix,
            )
        };

        let styled = Color::srgba(
            r.clamp(0.0, 1.0),
            g.clamp(0.0, 1.0),
            b.clamp(0.0, 1.0),
            (base.alpha * appearance.opacity).clamp(0.0, 1.0),
        );

        if background.0 != styled {
            background.0 = styled;
        }
    }
}

fn update_studio_preview_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    drag_handles: Query<&Interaction, (With<StudioPreviewDragHandle>, Changed<Interaction>)>,
    resize_handles: Query<(&Interaction, &StudioPreviewResizeHandle), Changed<Interaction>>,
    mut panels: Query<&mut Node, With<Panel>>,
    mut state: Local<StudioPreviewWindowState>,
) {
    let drag_started = drag_handles
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);

    let resize_started = resize_handles.iter().find_map(|(interaction, handle)| {
        (*interaction == Interaction::Pressed).then_some(handle.0)
    });

    if !mouse.pressed(MouseButton::Left) {
        state.dragging = false;
        state.resizing = false;
        state.resize_edge = None;
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(mut node) = panels.single_mut() else { return };

    let px = |value: Val, fallback: f32| match value {
        Val::Px(value) => value,
        _ => fallback,
    };

    if drag_started {
        state.dragging = true;
        state.resizing = false;
        state.resize_edge = None;
        state.drag_offset = cursor - Vec2::new(
            px(node.left, 40.0),
            px(node.top, 40.0),
        );
    }

    if let Some(edge) = resize_started {
        state.resizing = true;
        state.dragging = false;
        state.resize_edge = Some(edge);
        state.resize_start_cursor = cursor;
        state.resize_start_position = Vec2::new(
            px(node.left, 40.0),
            px(node.top, 40.0),
        );
        state.resize_start_size = Vec2::new(
            px(node.width, 900.0),
            px(node.height, 650.0),
        );
    }

    if state.dragging {
        let width = px(node.width, 900.0);
        let height = px(node.height, 650.0);

        node.left = Val::Px(
            (cursor.x - state.drag_offset.x)
                .clamp(0.0, (window.width() - width).max(0.0)),
        );
        node.top = Val::Px(
            (cursor.y - state.drag_offset.y)
                .clamp(0.0, (window.height() - height).max(0.0)),
        );
    }

    if state.resizing {
        let Some(edge) = state.resize_edge else { return };
        let delta = cursor - state.resize_start_cursor;
        let start_left = state.resize_start_position.x;
        let start_top = state.resize_start_position.y;
        let start_width = state.resize_start_size.x;
        let start_height = state.resize_start_size.y;

        let fixed_right = start_left + start_width;
        let fixed_bottom = start_top + start_height;

        let change_left = matches!(
            edge,
            StudioPreviewResizeEdge::Left
                | StudioPreviewResizeEdge::TopLeft
                | StudioPreviewResizeEdge::BottomLeft
        );
        let change_right = matches!(
            edge,
            StudioPreviewResizeEdge::Right
                | StudioPreviewResizeEdge::TopRight
                | StudioPreviewResizeEdge::BottomRight
        );
        let change_top = matches!(
            edge,
            StudioPreviewResizeEdge::Top
                | StudioPreviewResizeEdge::TopLeft
                | StudioPreviewResizeEdge::TopRight
        );
        let change_bottom = matches!(
            edge,
            StudioPreviewResizeEdge::Bottom
                | StudioPreviewResizeEdge::BottomLeft
                | StudioPreviewResizeEdge::BottomRight
        );

        let mut left = start_left;
        let mut top = start_top;
        let mut width = start_width;
        let mut height = start_height;

        if change_left {
            let max_width = (fixed_right - 12.0).max(520.0);
            width = (start_width - delta.x).clamp(520.0, max_width);
            left = fixed_right - width;
        } else if change_right {
            let max_width = (window.width() - start_left - 12.0).max(520.0);
            width = (start_width + delta.x).clamp(520.0, max_width);
        }

        if change_top {
            let max_height = (fixed_bottom - 12.0).max(420.0);
            height = (start_height - delta.y).clamp(420.0, max_height);
            top = fixed_bottom - height;
        } else if change_bottom {
            let max_height = (window.height() - start_top - 12.0).max(420.0);
            height = (start_height + delta.y).clamp(420.0, max_height);
        }

        node.width = Val::Px(width);
        node.height = Val::Px(height);
        node.left = Val::Px(left.clamp(0.0, (window.width() - width).max(0.0)));
        node.top = Val::Px(top.clamp(0.0, (window.height() - height).max(0.0)));
    }
}


fn studio_preview_text(
    text: impl Into<String>,
    size: f32,
    color: Color,
) -> (Text, TextFont, TextColor) {
    (
        Text::new(text),
        TextFont {
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}


fn scroll_studio_preview_pages(
    mut mouse_wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut pages: Query<
        (&RelativeCursorPosition, &mut ScrollPosition, &ComputedNode),
        With<StudioPreviewPageBody>,
    >,
) {
    for wheel in mouse_wheel.read() {
        let delta_y = match wheel.unit {
            bevy::input::mouse::MouseScrollUnit::Line => wheel.y * 32.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => wheel.y,
        };

        if delta_y == 0.0 {
            continue;
        }

        for (cursor, mut position, computed) in &mut pages {
            if !cursor.cursor_over {
                continue;
            }

            let max_y = (computed.content_size().y - computed.size().y).max(0.0)
                * computed.inverse_scale_factor;

            position.0.y = (position.0.y - delta_y).clamp(0.0, max_y);
        }
    }
}

fn spawn_studio_preview(commands: &mut Commands, starts_open: bool) {
    commands.insert_resource(StudioPreviewAppearance::default());
    commands.spawn((
        Panel,
        StudioPreviewRoot,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(40.0),
            top: Val::Px(40.0),
            width: Val::Px(900.0),
            height: Val::Px(650.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(12.0),
            padding: UiRect::new(Val::Px(14.0), Val::Px(14.0), Val::Px(0.0), Val::Px(30.0)),
            ..default()
        },
        ScrollPosition::default(),
        RelativeCursorPosition::default(),
        BackgroundColor(Color::srgb(0.055, 0.067, 0.09)),
        if starts_open { Visibility::Inherited } else { Visibility::Hidden },
    ))
    .with_children(|panel| {
        panel.spawn((
            Button,
            StudioPreviewDragHandle,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(58.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|header| {
            header.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(12.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|brand| {
                brand.spawn((
                    Node {
                        width: Val::Px(46.0),
                        height: Val::Px(46.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    StudioPreviewAccentSurface,
                    BackgroundColor(Color::srgb(0.82, 0.64, 0.34)),
                ))
                .with_children(|mark| {
                    mark.spawn((
                        Text::new("D"),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(24.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.07, 0.08, 0.10)),
                    ));
                });

                brand.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|title| {
                    title.spawn((
                        Text::new("LIGHTING MENU"),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(22.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.94, 0.95, 0.97)),
                    ));
                    title.spawn((
                        Text::new("DSOR  /  WORLD ENVIRONMENT EDITOR"),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.59, 0.65, 0.74)),
                    ));
                });
            });

            header.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.20, 0.18, 0.14)),
            ))
            .with_children(|tag| {
                tag.spawn((
                    Text::new("PREVIEW  /  DRAG HEADER TO MOVE"),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.91, 0.73, 0.43)),
                ));
            });
        });

        panel.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
        ));

        panel.spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(12.0),
                ..default()
            },
        ))
        .with_children(|workspace| {
            workspace.spawn((
                Node {
                    width: Val::Px(192.0),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(7.0),
                    padding: UiRect::all(Val::Px(11.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.085, 0.10, 0.135)),
            ))
            .with_children(|sidebar| {
                sidebar.spawn((
                    Text::new("WORKSPACE"),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.58, 0.65, 0.75)),
                ));

                for (page, number, label) in [
                    (StudioPage::Overview, "01", "Overview"),
                    (StudioPage::Lighting, "02", "Lighting"),
                    (StudioPage::Regions, "03", "Regions"),
                    (StudioPage::Atmosphere, "04", "Atmosphere"),
                    (StudioPage::Presets, "05", "Presets"),
                ] {
                    let selected = page == StudioPage::Overview;
                    sidebar.spawn((
                        Button,
                        StudioPreviewNav(page),
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(43.0),
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(10.0),
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(if selected {
                            Color::srgb(0.29, 0.25, 0.18)
                        } else {
                            Color::srgb(0.12, 0.14, 0.19)
                        }),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new(number),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.83, 0.68, 0.43)),
                        ));
                        button.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(14.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.91, 0.93, 0.96)),
                        ));
                    });
                }

                sidebar.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });

                sidebar.spawn((
                    Text::new("F2  /  TOGGLE MENU"),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.57, 0.63, 0.72)),
                ));
            });

            workspace.spawn((
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.075, 0.09, 0.12)),
            ))
            .with_children(|main| {
                // OVERVIEW: a compact dashboard, kept inside its own scrollable page.
                main.spawn((
                    StudioPreviewPageBody(StudioPage::Overview),
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(9.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    RelativeCursorPosition::default(),
                ))
                .with_children(|page| {
                    page.spawn(studio_preview_text(
                        "Environment overview",
                        25.0,
                        Color::srgb(0.94, 0.95, 0.97),
                    ));
                    page.spawn(studio_preview_text(
                        "A starting point for the map's lighting and environment tools.",
                        13.0,
                        Color::srgb(0.64, 0.70, 0.79),
                    ));

                    page.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(9.0),
                            padding: UiRect::all(Val::Px(12.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.095, 0.115, 0.15)),
                    ))
                    .with_children(|appearance| {
                        appearance.spawn(studio_preview_text(
                            "PANEL APPEARANCE",
                            12.0,
                            Color::srgb(0.91, 0.73, 0.43),
                        ));
                        appearance.spawn(studio_preview_text(
                            "Make the menu more transparent or choose a different color theme.",
                            12.0,
                            Color::srgb(0.72, 0.77, 0.84),
                        ));

                        appearance.spawn(Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(7.0),
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn(studio_preview_text(
                                "Opacity",
                                12.0,
                                Color::srgb(0.91, 0.93, 0.96),
                            ));

                            row.spawn((
                                Button,
                                StudioPreviewOpacityStep(-0.1),
                                Node {
                                    width: Val::Px(32.0),
                                    min_height: Val::Px(30.0),
                                    flex_shrink: 0.0,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                            ))
                            .with_children(|button| {
                                button.spawn((
                                    Text::new("-"),
                                    TextFont {
                                        font_size: bevy::text::FontSize::Px(15.0),
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                ));
                            });

                            row.spawn((
                                StudioPreviewOpacityLabel,
                                Text::new("80%"),
                                TextFont {
                                    font_size: bevy::text::FontSize::Px(12.0),
                                    ..default()
                                },
                                TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                Node {
                                    width: Val::Px(60.0),
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                            ));

                            row.spawn((
                                Button,
                                StudioPreviewOpacityStep(0.1),
                                Node {
                                    width: Val::Px(32.0),
                                    min_height: Val::Px(30.0),
                                    flex_shrink: 0.0,
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                            ))
                            .with_children(|button| {
                                button.spawn((
                                    Text::new("+"),
                                    TextFont {
                                        font_size: bevy::text::FontSize::Px(15.0),
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                ));
                            });
                        });

                        for themes in [
                            [
                                (StudioPreviewTheme::Slate, "Slate"),
                                (StudioPreviewTheme::Red, "Red"),
                            ],
                            [
                                (StudioPreviewTheme::Blue, "Blue"),
                                (StudioPreviewTheme::Green, "Green"),
                            ],
                        ] {
                            appearance.spawn(Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(7.0),
                                ..default()
                            })
                            .with_children(|row| {
                                for (theme, label) in themes {
                                    row.spawn((
                                        Button,
                                        StudioPreviewThemeChoice(theme),
                                        Node {
                                            flex_grow: 1.0,
                                            min_width: Val::Px(0.0),
                                            min_height: Val::Px(31.0),
                                            padding: UiRect::axes(
                                                Val::Px(8.0),
                                                Val::Px(5.0),
                                            ),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                    ))
                                    .with_children(|button| {
                                        button.spawn((
                                            Text::new(label),
                                            TextFont {
                                                font_size: bevy::text::FontSize::Px(12.0),
                                                ..default()
                                            },
                                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                        ));
                                    });
                                }
                            });
                        }

                        appearance.spawn(studio_preview_text(
                            "Opacity changes menu surfaces only. Themes recolor the panel and active page highlight, not game lighting or presets.",
                            11.0,
                            Color::srgb(0.64, 0.70, 0.79),
                        ));
                    });

                    for (number, heading, description, status) in [
                        ("01", "GLOBAL LIGHTING", "The overall look of the map.", "LIGHTING"),
                        ("02", "AMBIENCE REGIONS", "Local areas, bounds and transitions.", "REGIONS"),
                        ("03", "PRESETS", "Saved looks and configurations.", "SAVED LOOKS"),
                    ] {
                        page.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                min_height: Val::Px(66.0),
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(11.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::all(Val::Px(10.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
                        ))
                        .with_children(|card| {
                            card.spawn((
                                Node {
                                    width: Val::Px(38.0),
                                    height: Val::Px(38.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.25, 0.22, 0.17)),
                            ))
                            .with_children(|number_box| {
                                number_box.spawn(studio_preview_text(
                                    number,
                                    13.0,
                                    Color::srgb(0.91, 0.73, 0.43),
                                ));
                            });

                            card.spawn(Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                ..default()
                            })
                            .with_children(|details| {
                                details.spawn(studio_preview_text(
                                    heading,
                                    12.0,
                                    Color::srgb(0.91, 0.73, 0.43),
                                ));
                                details.spawn(studio_preview_text(
                                    description,
                                    13.0,
                                    Color::srgb(0.83, 0.86, 0.91),
                                ));
                            });

                            card.spawn(studio_preview_text(
                                status,
                                10.0,
                                Color::srgb(0.61, 0.69, 0.79),
                            ));
                        });
                    }

                    page.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(58.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(5.0),
                            padding: UiRect::all(Val::Px(10.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.095, 0.115, 0.15)),
                    ))
                    .with_children(|note| {
                        note.spawn(studio_preview_text(
                            "DESIGN PREVIEW",
                            11.0,
                            Color::srgb(0.91, 0.73, 0.43),
                        ));
                        note.spawn(studio_preview_text(
                            "All values shown in this prototype are examples. Real controls will be connected after the layout is settled.",
                            12.0,
                            Color::srgb(0.72, 0.77, 0.84),
                        ));
                    });
                });

                // Each following page is a visual mockup, not a live editor yet.
                for page_id in [
                    StudioPage::Lighting,
                    StudioPage::Regions,
                    StudioPage::Atmosphere,
                    StudioPage::Presets,
                ] {
                    main.spawn((
                        StudioPreviewPageBody(page_id),
                        Node {
                            display: bevy::ui::Display::None,
                            width: Val::Percent(100.0),
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(10.0),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition::default(),
                    RelativeCursorPosition::default(),
                    ))
                    .with_children(|page| {
                        match page_id {
                            StudioPage::Overview => {}

                            // LIGHTING
                            StudioPage::Lighting => {
                                page.spawn(studio_preview_text(
                                    "Lighting",
                                    25.0,
                                    Color::srgb(0.94, 0.95, 0.97),
                                ));
                                page.spawn(studio_preview_text(
                                    "Global renderer controls and ambience light settings.",
                                    13.0,
                                    Color::srgb(0.64, 0.70, 0.79),
                                ));

                                page.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(8.0),
                                        padding: UiRect::all(Val::Px(12.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
                                ))
                                .with_children(|card| {
                                    card.spawn(studio_preview_text(
                                        "GLOBAL RENDER CONTROLS",
                                        12.0,
                                        Color::srgb(0.91, 0.73, 0.43),
                                    ));
                                    card.spawn(studio_preview_text(
                                        "Click a value to type; Enter applies; Esc cancels. Shift adjusts by x10.",
                                        11.0,
                                        Color::srgb(0.64, 0.70, 0.79),
                                    ));

                                    for (i, field) in FIELDS.iter().enumerate() {
                                        card.spawn(Node {
                                            width: Val::Percent(100.0),
                                            min_height: Val::Px(30.0),
                                            flex_direction: FlexDirection::Row,
                                            column_gap: Val::Px(5.0),
                                            align_items: AlignItems::Center,
                                            ..default()
                                        })
                                        .with_children(|row| {
                                            row.spawn((
                                                Text::new(field.label),
                                                TextFont {
                                                    font_size: bevy::text::FontSize::Px(12.0),
                                                    ..default()
                                                },
                                                TextColor(Color::srgb(0.83, 0.86, 0.91)),
                                                Node {
                                                    width: Val::Px(140.0),
                                                    ..default()
                                                },
                                            ));

                                            for (sign, label) in [(-1.0, "-"), (1.0, "+")] {
                                                row.spawn((
                                                    Button,
                                                    StepButton(i, sign),
                                                    Node {
                                                        width: Val::Px(24.0),
                                                        min_height: Val::Px(25.0),
                                                        flex_shrink: 0.0,
                                                        justify_content: JustifyContent::Center,
                                                        align_items: AlignItems::Center,
                                                        ..default()
                                                    },
                                                    BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                                ))
                                                .with_children(|button| {
                                                    button.spawn((
                                                        Text::new(label),
                                                        TextFont {
                                                            font_size: bevy::text::FontSize::Px(13.0),
                                                            ..default()
                                                        },
                                                        TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                                    ));
                                                });
                                            }

                                            row.spawn((
                                                Button,
                                                NumericField(i),
                                                Node {
                                                    flex_grow: 1.0,
                                                    min_width: Val::Px(58.0),
                                                    max_width: Val::Px(100.0),
                                                    min_height: Val::Px(25.0),
                                                    flex_shrink: 0.0,
                                                    padding: UiRect::axes(
                                                        Val::Px(5.0),
                                                        Val::Px(3.0),
                                                    ),
                                                    justify_content: JustifyContent::Center,
                                                    align_items: AlignItems::Center,
                                                    ..default()
                                                },
                                                BackgroundColor(Color::srgb(0.07, 0.09, 0.13)),
                                            ))
                                            .with_children(|value| {
                                                value.spawn((
                                                    ValueText(i),
                                                    Text::new(""),
                                                    TextFont {
                                                        font_size: bevy::text::FontSize::Px(12.0),
                                                        ..default()
                                                    },
                                                    TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                                ));
                                            });
                                        });
                                    }
                                });

                                page.spawn(studio_preview_text(
                                    "Region-aware ambience",
                                    18.0,
                                    Color::srgb(0.94, 0.95, 0.97),
                                ));
                                page.spawn(studio_preview_text(
                                    "These fields edit the target selected on Regions. Click a value, type the new number, and press Enter to apply. Esc cancels.",
                                    12.0,
                                    Color::srgb(0.64, 0.70, 0.79),
                                ));

                                for group in [
                                    AmbienceGroup::Lighting,
                                    AmbienceGroup::Colors,
                                    AmbienceGroup::PlayerLight,
                                ] {
                                    page.spawn((
                                        Node {
                                            width: Val::Percent(100.0),
                                            flex_direction: FlexDirection::Column,
                                            row_gap: Val::Px(6.0),
                                            padding: UiRect::all(Val::Px(10.0)),
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
                                    ))
                                    .with_children(|section| {
                                        section.spawn((
                                            Button,
                                            AmbienceGroupToggle(group),
                                            Node {
                                                width: Val::Percent(100.0),
                                                min_height: Val::Px(32.0),
                                                padding: UiRect::axes(
                                                    Val::Px(10.0),
                                                    Val::Px(5.0),
                                                ),
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },
                                            BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                        ))
                                        .with_children(|button| {
                                            button.spawn((
                                                AmbienceGroupTitle(group),
                                                Text::new(format!("{}  ?", group.label())),
                                                TextFont {
                                                    font_size: bevy::text::FontSize::Px(13.0),
                                                    ..default()
                                                },
                                                TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                            ));
                                        });

                                        section.spawn((
                                            AmbienceGroupBody(group),
                                            Node {
                                                width: Val::Percent(100.0),
                                                flex_direction: FlexDirection::Column,
                                                row_gap: Val::Px(7.0),
                                                padding: UiRect::all(Val::Px(5.0)),
                                                ..default()
                                            },
                                            Visibility::Hidden,
                                        ))
                                        .with_children(|body| {
                                            for field in group.fields() {
                                                let field = *field;

                                                body.spawn(Node {
                                                    width: Val::Percent(100.0),
                                                    min_height: Val::Px(30.0),
                                                    flex_direction: FlexDirection::Row,
                                                    column_gap: Val::Px(8.0),
                                                    align_items: AlignItems::Center,
                                                    ..default()
                                                })
                                                .with_children(|row| {
                                                    row.spawn((
                                                        Text::new(field.label()),
                                                        TextFont {
                                                            font_size: bevy::text::FontSize::Px(12.0),
                                                            ..default()
                                                        },
                                                        TextColor(Color::srgb(0.83, 0.86, 0.91)),
                                                        Node {
                                                            flex_grow: 1.0,
                                                            min_width: Val::Px(0.0),
                                                            ..default()
                                                        },
                                                    ));

                                                    row.spawn((
                                                        Button,
                                                        AmbienceStepButton(field, -1.0),
                                                        Node {
                                                            width: Val::Px(27.0),
                                                            min_height: Val::Px(29.0),
                                                            flex_shrink: 0.0,
                                                            justify_content: JustifyContent::Center,
                                                            align_items: AlignItems::Center,
                                                            ..default()
                                                        },
                                                        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                                    ))
                                                    .with_children(|button| {
                                                        button.spawn((
                                                            Text::new("-"),
                                                            TextFont {
                                                                font_size: bevy::text::FontSize::Px(13.0),
                                                                ..default()
                                                            },
                                                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                                        ));
                                                    });

                                                    row.spawn((
                                                        Button,
                                                        AmbienceNumericField(field),
                                                        Node {
                                                            width: Val::Px(100.0),
                                                            min_height: Val::Px(29.0),
                                                            flex_shrink: 0.0,
                                                            padding: UiRect::axes(
                                                                Val::Px(6.0),
                                                                Val::Px(4.0),
                                                            ),
                                                            justify_content: JustifyContent::Center,
                                                            align_items: AlignItems::Center,
                                                            ..default()
                                                        },
                                                        BackgroundColor(Color::srgb(0.07, 0.09, 0.13)),
                                                    ))
                                                    .with_children(|value| {
                                                        value.spawn((
                                                            AmbienceValueText(field),
                                                            Text::new("?"),
                                                            TextFont {
                                                                font_size: bevy::text::FontSize::Px(12.0),
                                                                ..default()
                                                            },
                                                            TextColor(Color::srgb(0.96, 0.80, 0.49)),
                                                        ));
                                                    });

                                                    row.spawn((
                                                        Button,
                                                        AmbienceStepButton(field, 1.0),
                                                        Node {
                                                            width: Val::Px(27.0),
                                                            min_height: Val::Px(29.0),
                                                            flex_shrink: 0.0,
                                                            justify_content: JustifyContent::Center,
                                                            align_items: AlignItems::Center,
                                                            ..default()
                                                        },
                                                        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                                    ))
                                                    .with_children(|button| {
                                                        button.spawn((
                                                            Text::new("+"),
                                                            TextFont {
                                                                font_size: bevy::text::FontSize::Px(13.0),
                                                                ..default()
                                                            },
                                                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                                        ));
                                                    });
                                                });
                                            }
                                        });
                                    });
                                }
                            }

                            StudioPage::Regions => {
                                page.spawn(studio_preview_text(
                                    "Ambience regions",
                                    25.0,
                                    Color::srgb(0.94, 0.95, 0.97),
                                ));
                                page.spawn(studio_preview_text(
                                    "Select the map-wide ambience or cycle through local ambience regions.",
                                    13.0,
                                    Color::srgb(0.64, 0.70, 0.79),
                                ));

                                page.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(10.0),
                                        padding: UiRect::all(Val::Px(14.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
                                ))
                                .with_children(|card| {
                                    card.spawn(studio_preview_text(
                                        "AMBIENCE TARGET",
                                        12.0,
                                        Color::srgb(0.91, 0.73, 0.43),
                                    ));

                                    card.spawn(studio_preview_text(
                                        "Use these controls to choose which ambience the Lighting and Atmosphere settings edit.",
                                        12.0,
                                        Color::srgb(0.72, 0.77, 0.84),
                                    ));

                                    card.spawn(Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Row,
                                        column_gap: Val::Px(8.0),
                                        align_items: AlignItems::Center,
                                        ..default()
                                    })
                                    .with_children(|row| {
                                        row.spawn((
                                            Button,
                                            AmbienceTargetStep(-1),
                                            Node {
                                                width: Val::Px(64.0),
                                                min_height: Val::Px(32.0),
                                                flex_shrink: 0.0,
                                                justify_content: JustifyContent::Center,
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },
                                            BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                        ))
                                        .with_children(|button| {
                                            button.spawn((
                                                Text::new("Prev"),
                                                TextFont {
                                                    font_size: bevy::text::FontSize::Px(13.0),
                                                    ..default()
                                                },
                                                TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                            ));
                                        });

                                        row.spawn((
                                            AmbienceTargetLabel,
                                            Text::new("Target: loading map ambience..."),
                                            TextFont {
                                                font_size: bevy::text::FontSize::Px(13.0),
                                                ..default()
                                            },
                                            TextColor(Color::srgb(0.96, 0.80, 0.49)),
                                            Node {
                                                flex_grow: 1.0,
                                                min_width: Val::Px(0.0),
                                                ..default()
                                            },
                                        ));

                                        row.spawn((
                                            Button,
                                            AmbienceTargetStep(1),
                                            Node {
                                                width: Val::Px(64.0),
                                                min_height: Val::Px(32.0),
                                                flex_shrink: 0.0,
                                                justify_content: JustifyContent::Center,
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },
                                            BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                        ))
                                        .with_children(|button| {
                                            button.spawn((
                                                Text::new("Next"),
                                                TextFont {
                                                    font_size: bevy::text::FontSize::Px(13.0),
                                                    ..default()
                                                },
                                                TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                            ));
                                        });
                                    });

                                    card.spawn(studio_preview_text(
                                        "When a local region is selected, its outline is shown in the world. Edit its overrides from the Lighting and Atmosphere pages.",
                                        12.0,
                                        Color::srgb(0.64, 0.70, 0.79),
                                    ));
                                });
                            }

                            StudioPage::Atmosphere => {
                                page.spawn(studio_preview_text(
                                    "Atmosphere",
                                    25.0,
                                    Color::srgb(0.94, 0.95, 0.97),
                                ));
                                page.spawn(studio_preview_text(
                                    "Fog and post-processing overrides for the target selected on Regions.",
                                    13.0,
                                    Color::srgb(0.64, 0.70, 0.79),
                                ));
                                page.spawn(studio_preview_text(
                                    "Click a value, type the new number, and press Enter to apply. Esc cancels.",
                                    12.0,
                                    Color::srgb(0.64, 0.70, 0.79),
                                ));

                                for group in [
                                    AmbienceGroup::Fog,
                                    AmbienceGroup::PostProcessing,
                                ] {
                                    page.spawn((
                                        Node {
                                            width: Val::Percent(100.0),
                                            flex_direction: FlexDirection::Column,
                                            row_gap: Val::Px(6.0),
                                            padding: UiRect::all(Val::Px(10.0)),
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
                                    ))
                                    .with_children(|section| {
                                        section.spawn((
                                            Button,
                                            AmbienceGroupToggle(group),
                                            Node {
                                                width: Val::Percent(100.0),
                                                min_height: Val::Px(32.0),
                                                padding: UiRect::axes(
                                                    Val::Px(10.0),
                                                    Val::Px(5.0),
                                                ),
                                                align_items: AlignItems::Center,
                                                ..default()
                                            },
                                            BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                        ))
                                        .with_children(|button| {
                                            button.spawn((
                                                AmbienceGroupTitle(group),
                                                Text::new(format!("{}  ?", group.label())),
                                                TextFont {
                                                    font_size: bevy::text::FontSize::Px(13.0),
                                                    ..default()
                                                },
                                                TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                            ));
                                        });

                                        section.spawn((
                                            AmbienceGroupBody(group),
                                            Node {
                                                width: Val::Percent(100.0),
                                                flex_direction: FlexDirection::Column,
                                                row_gap: Val::Px(7.0),
                                                padding: UiRect::all(Val::Px(5.0)),
                                                ..default()
                                            },
                                            Visibility::Hidden,
                                        ))
                                        .with_children(|body| {
                                            for field in group.fields() {
                                                let field = *field;

                                                body.spawn(Node {
                                                    width: Val::Percent(100.0),
                                                    min_height: Val::Px(30.0),
                                                    flex_direction: FlexDirection::Row,
                                                    column_gap: Val::Px(8.0),
                                                    align_items: AlignItems::Center,
                                                    ..default()
                                                })
                                                .with_children(|row| {
                                                    row.spawn((
                                                        Text::new(field.label()),
                                                        TextFont {
                                                            font_size: bevy::text::FontSize::Px(12.0),
                                                            ..default()
                                                        },
                                                        TextColor(Color::srgb(0.83, 0.86, 0.91)),
                                                        Node {
                                                            flex_grow: 1.0,
                                                            min_width: Val::Px(0.0),
                                                            ..default()
                                                        },
                                                    ));

                                                    row.spawn((
                                                        Button,
                                                        AmbienceStepButton(field, -1.0),
                                                        Node {
                                                            width: Val::Px(27.0),
                                                            min_height: Val::Px(29.0),
                                                            flex_shrink: 0.0,
                                                            justify_content: JustifyContent::Center,
                                                            align_items: AlignItems::Center,
                                                            ..default()
                                                        },
                                                        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                                    ))
                                                    .with_children(|button| {
                                                        button.spawn((
                                                            Text::new("-"),
                                                            TextFont {
                                                                font_size: bevy::text::FontSize::Px(13.0),
                                                                ..default()
                                                            },
                                                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                                        ));
                                                    });

                                                    row.spawn((
                                                        Button,
                                                        AmbienceNumericField(field),
                                                        Node {
                                                            width: Val::Px(100.0),
                                                            min_height: Val::Px(29.0),
                                                            flex_shrink: 0.0,
                                                            padding: UiRect::axes(
                                                                Val::Px(6.0),
                                                                Val::Px(4.0),
                                                            ),
                                                            justify_content: JustifyContent::Center,
                                                            align_items: AlignItems::Center,
                                                            ..default()
                                                        },
                                                        BackgroundColor(Color::srgb(0.07, 0.09, 0.13)),
                                                    ))
                                                    .with_children(|value| {
                                                        value.spawn((
                                                            AmbienceValueText(field),
                                                            Text::new("?"),
                                                            TextFont {
                                                                font_size: bevy::text::FontSize::Px(12.0),
                                                                ..default()
                                                            },
                                                            TextColor(Color::srgb(0.96, 0.80, 0.49)),
                                                        ));
                                                    });

                                                    row.spawn((
                                                        Button,
                                                        AmbienceStepButton(field, 1.0),
                                                        Node {
                                                            width: Val::Px(27.0),
                                                            min_height: Val::Px(29.0),
                                                            flex_shrink: 0.0,
                                                            justify_content: JustifyContent::Center,
                                                            align_items: AlignItems::Center,
                                                            ..default()
                                                        },
                                                        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                                    ))
                                                    .with_children(|button| {
                                                        button.spawn((
                                                            Text::new("+"),
                                                            TextFont {
                                                                font_size: bevy::text::FontSize::Px(13.0),
                                                                ..default()
                                                            },
                                                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                                        ));
                                                    });
                                                });
                                            }
                                        });
                                    });
                                }
                            }



                            StudioPage::Presets => {
                                page.spawn(studio_preview_text(
                                    "Lighting presets",
                                    25.0,
                                    Color::srgb(0.94, 0.95, 0.97),
                                ));
                                page.spawn(studio_preview_text(
                                    "Save named looks, load them later, and manage your saved profiles.",
                                    13.0,
                                    Color::srgb(0.64, 0.70, 0.79),
                                ));

                                page.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(10.0),
                                        padding: UiRect::all(Val::Px(12.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
                                ))
                                .with_children(|card| {
                                    card.spawn(studio_preview_text(
                                        "PRESET NAME",
                                        12.0,
                                        Color::srgb(0.91, 0.73, 0.43),
                                    ));

                                    card.spawn((
                                        Button,
                                        PresetNameField,
                                        Node {
                                            width: Val::Percent(100.0),
                                            min_height: Val::Px(36.0),
                                            padding: UiRect::axes(
                                                Val::Px(10.0),
                                                Val::Px(6.0),
                                            ),
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgb(0.07, 0.09, 0.13)),
                                    ))
                                    .with_children(|field| {
                                        field.spawn((
                                            PresetNameText,
                                            Text::new("Click to enter a preset name"),
                                            TextFont {
                                                font_size: bevy::text::FontSize::Px(13.0),
                                                ..default()
                                            },
                                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                        ));
                                    });

                                    card.spawn(studio_preview_text(
                                        "Click the name field, type a name, then choose an action. Text entry uses Enter to finish and Esc to cancel.",
                                        11.0,
                                        Color::srgb(0.64, 0.70, 0.79),
                                    ));

                                    for actions in [
                                        [
                                            (PresetAction::SaveAs, "Save As"),
                                            (PresetAction::Overwrite, "Overwrite"),
                                        ],
                                        [
                                            (PresetAction::Rename, "Rename"),
                                            (PresetAction::Delete, "Delete"),
                                        ],
                                    ] {
                                        card.spawn(Node {
                                            width: Val::Px(254.0),
                                            flex_direction: FlexDirection::Row,
                                            column_gap: Val::Px(6.0),
                                            ..default()
                                        })
                                        .with_children(|row| {
                                            for (action, label) in actions {
                                                row.spawn((
                                                    Button,
                                                    action,
                                                    Node {
                                                        width: Val::Px(124.0),
                                                        min_width: Val::Px(124.0),
                                                        max_width: Val::Px(124.0),
                                                        flex_shrink: 0.0,
                                                        min_height: Val::Px(32.0),
                                                        padding: UiRect::axes(
                                                            Val::Px(5.0),
                                                            Val::Px(5.0),
                                                        ),
                                                        justify_content: JustifyContent::Center,
                                                        align_items: AlignItems::Center,
                                                        ..default()
                                                    },
                                                    BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                                                ))
                                                .with_children(|button| {
                                                    button.spawn((
                                                        Text::new(label),
                                                        TextFont {
                                                            font_size: bevy::text::FontSize::Px(12.0),
                                                            ..default()
                                                        },
                                                        TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                                    ));
                                                });
                                            }
                                        });
                                    }

                                    card.spawn((
                                        Button,
                                        PresetAction::ClearSelection,
                                        Node {
                                            width: Val::Px(254.0),
                                            min_width: Val::Px(254.0),
                                            max_width: Val::Px(254.0),
                                            flex_shrink: 0.0,
                                            min_height: Val::Px(30.0),
                                            padding: UiRect::axes(
                                                Val::Px(8.0),
                                                Val::Px(4.0),
                                            ),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
                                    ))
                                    .with_children(|button| {
                                        button.spawn((
                                            Text::new("Clear Selection"),
                                            TextFont {
                                                font_size: bevy::text::FontSize::Px(12.0),
                                                ..default()
                                            },
                                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                                        ));
                                    });

                                    card.spawn((
                                        PresetStatusText,
                                        Text::new("No preset selected"),
                                        TextFont {
                                            font_size: bevy::text::FontSize::Px(12.0),
                                            ..default()
                                        },
                                        TextColor(Color::srgb(0.96, 0.80, 0.49)),
                                    ));
                                });

                                page.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(8.0),
                                        padding: UiRect::all(Val::Px(12.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
                                ))
                                .with_children(|card| {
                                    card.spawn(studio_preview_text(
                                        "SAVED PRESETS",
                                        12.0,
                                        Color::srgb(0.91, 0.73, 0.43),
                                    ));

                                    card.spawn(studio_preview_text(
                                        "Select a saved preset to load it. The selected preset can then be overwritten, renamed, or deleted.",
                                        11.0,
                                        Color::srgb(0.64, 0.70, 0.79),
                                    ));

                                    card.spawn((
                                        PresetList,
                                        Node {
                                            width: Val::Percent(100.0),
                                            flex_direction: FlexDirection::Column,
                                            row_gap: Val::Px(4.0),
                                            ..default()
                                        },
                                    ));
                                });
                            }
                        }
                    });
                }

            });
        });

        // Transparent resize hitboxes. No visible marks overlap the menu content.
        for (edge, node) in [
            (
                StudioPreviewResizeEdge::Left,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(58.0),
                    bottom: Val::Px(30.0),
                    width: Val::Px(18.0),
                    ..default()
                },
            ),
            (
                StudioPreviewResizeEdge::Right,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    top: Val::Px(58.0),
                    bottom: Val::Px(30.0),
                    width: Val::Px(18.0),
                    ..default()
                },
            ),
            (
                StudioPreviewResizeEdge::Top,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    left: Val::Px(18.0),
                    right: Val::Px(18.0),
                    height: Val::Px(14.0),
                    ..default()
                },
            ),
            (
                StudioPreviewResizeEdge::Bottom,
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(0.0),
                    left: Val::Px(18.0),
                    right: Val::Px(18.0),
                    height: Val::Px(18.0),
                    ..default()
                },
            ),
            (
                StudioPreviewResizeEdge::TopLeft,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    ..default()
                },
            ),
            (
                StudioPreviewResizeEdge::TopRight,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    ..default()
                },
            ),
            (
                StudioPreviewResizeEdge::BottomLeft,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    ..default()
                },
            ),
            (
                StudioPreviewResizeEdge::BottomRight,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    ..default()
                },
            ),
        ] {
            panel.spawn((
                Button,
                StudioPreviewResizeHandle(edge),
                node,
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            ));
        }

    });
}

fn update_studio_preview_navigation(
    buttons: Query<(&Interaction, &StudioPreviewNav), Changed<Interaction>>,
    mut nav_styles: Query<(&StudioPreviewNav, &mut BackgroundColor)>,
    mut pages: Query<(&StudioPreviewPageBody, &mut Node)>,
) {
    let selected = buttons.iter().find_map(|(interaction, nav)| {
        (*interaction == Interaction::Pressed).then_some(nav.0)
    });
    let Some(selected) = selected else {
        return;
    };

    for (nav, mut background) in &mut nav_styles {
        background.0 = if nav.0 == selected {
            Color::srgb(0.29, 0.25, 0.18)
        } else {
            Color::srgb(0.12, 0.14, 0.19)
        };
    }
    for (page, mut node) in &mut pages {
        node.display = if page.0 == selected {
            bevy::ui::Display::Flex
        } else {
            bevy::ui::Display::None
        };
    }
}

fn spawn_panel(
    mut commands: Commands,
    store: Res<crate::lighting_presets::LightingStudioStore>,
    mut name_editor: ResMut<PresetNameEditor>,
    mut panel_state: ResMut<LightingPanelState>,
) {
    let starts_open = std::env::var("DSOR_PANEL").is_ok();
    panel_state.open = starts_open;

    if let Some(id) = store.active_preset_id.as_deref() {
        if let Some(preset) = store.presets.get(id) {
            name_editor.draft = preset.name.clone();
        }
    }

    // Optional design-only shell. The existing working F2 menu remains the default.
    if std::env::var_os("DSOR_STUDIO_PREVIEW").is_some() {
        spawn_studio_preview(&mut commands, starts_open);
        return;
    }

    let font = TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() };
    commands
        .spawn((
            Panel,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(8.0),
                top: Val::Px(8.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(8.0)),
                max_height: Val::Percent(95.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            RelativeCursorPosition::default(),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            // DSOR_PANEL=1 (native): open from the start, for screenshots.
            if starts_open { Visibility::Inherited } else { Visibility::Hidden },
        ))
        .with_children(|panel| {
            panel.spawn((Text::new("Look (F2): click a value to type; Enter applies; Esc cancels; Shift: x10"), font.clone(), TextColor(Color::srgb(1.0, 0.85, 0.4))));
            for (i, f) in FIELDS.iter().enumerate() {
                panel
                    .spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() })
                    .with_children(|row| {
                        row.spawn((Text::new(f.label), font.clone(), Node { width: Val::Px(150.0), ..default() }));
                        for (sign, label) in [(-1.0, "-"), (1.0, "+")] {
                            row.spawn((
                                Button,
                                StepButton(i, sign),
                                Node { width: Val::Px(24.0), justify_content: JustifyContent::Center, ..default() },
                                BackgroundColor(Color::srgb(0.25, 0.25, 0.3)),
                            ))
                            .with_children(|b| {
                                b.spawn((Text::new(label), font.clone()));
                            });
                        }
                        row.spawn((
                            Button,
                            NumericField(i),
                            Node {
                                width: Val::Px(78.0),
                                min_height: Val::Px(24.0),
                                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.12, 0.12, 0.15)),
                        ))
                        .with_children(|field| {
                            field.spawn((ValueText(i), Text::new(""), font.clone()));
                        });
                    });
            }

            panel.spawn((
                Button,
                MapAmbienceToggle,
                Node {
                    width: Val::Px(330.0),
                    min_height: Val::Px(27.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.16, 0.22, 0.29)),
            ))
            .with_children(|button| {
                button.spawn((
                    MapAmbienceToggleText,
                    Text::new("Map Ambience  ▸"),
                    font.clone(),
                    TextColor(Color::srgb(0.95, 0.95, 0.95)),
                ));
            });

            panel.spawn((
                MapAmbienceSection,
                Node {
                    width: Val::Px(330.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .with_children(|section| {
                section.spawn((
                    Text::new("Map-wide overrides; untouched values use the original ambience."),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.75, 0.80, 0.85)),
                ));

                section.spawn(Node {
                    width: Val::Px(330.0),
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(4.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Button,
                        AmbienceTargetStep(-1),
                        Node {
                            width: Val::Px(44.0),
                            min_height: Val::Px(27.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.22, 0.22, 0.28)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new("Prev"),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.95, 0.95, 0.95)),
                        ));
                    });

                    row.spawn((
                        AmbienceTargetLabel,
                        Text::new("Target: Global map ambience"),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(Color::srgb(1.0, 0.85, 0.4)),
                        Node {
                            width: Val::Px(232.0),
                            ..default()
                        },
                    ));

                    row.spawn((
                        Button,
                        AmbienceTargetStep(1),
                        Node {
                            width: Val::Px(44.0),
                            min_height: Val::Px(27.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.22, 0.22, 0.28)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new("Next"),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.95, 0.95, 0.95)),
                        ));
                    });
                });

                for group in AMBIENCE_GROUPS {
                    let group = *group;

                    section.spawn((
                        Button,
                        AmbienceGroupToggle(group),
                        Node {
                            width: Val::Px(330.0),
                            min_height: Val::Px(25.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.20, 0.20, 0.25)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            AmbienceGroupTitle(group),
                            Text::new(format!("{}  ▸", group.label())),
                            font.clone(),
                            TextColor(Color::srgb(0.92, 0.92, 0.96)),
                        ));
                    });

                    section.spawn((
                        AmbienceGroupBody(group),
                        Node {
                            width: Val::Px(330.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            padding: UiRect::left(Val::Px(6.0)),
                            ..default()
                        },
                        Visibility::Hidden,
                    ))
                    .with_children(|body| {
                        for field in group.fields() {
                            let field = *field;
                            body.spawn(Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(6.0),
                                align_items: AlignItems::Center,
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn((
                                    Text::new(field.label()),
                                    font.clone(),
                                    TextColor(Color::srgb(0.92, 0.92, 0.92)),
                                    Node {
                                        width: Val::Px(170.0),
                                        ..default()
                                    },
                                ));

                                row.spawn((
                                    Button,
                                    AmbienceNumericField(field),
                                    Node {
                                        width: Val::Px(78.0),
                                        min_height: Val::Px(23.0),
                                        padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.12, 0.12, 0.15)),
                                ))
                                .with_children(|value| {
                                    value.spawn((
                                        AmbienceValueText(field),
                                        Text::new("—"),
                                        font.clone(),
                                    ));
                                });
                            });
                        }
                    });
                }
            });

            panel.spawn((
                Text::new("Lighting presets — click a saved preset to load it"),
                font.clone(),
                TextColor(Color::srgb(1.0, 0.85, 0.4)),
            ));

            panel.spawn((
                Button,
                PresetNameField,
                Node {
                    width: Val::Px(330.0),
                    min_height: Val::Px(27.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.12, 0.12, 0.15)),
            ))
            .with_children(|field| {
                field.spawn((
                    PresetNameText,
                    Text::new("Enter preset name"),
                    font.clone(),
                    TextColor(Color::srgb(0.95, 0.95, 0.95)),
                ));
            });

            panel.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|row| {
                for (action, label) in [
                    (PresetAction::SaveAs, "Save As"),
                    (PresetAction::Overwrite, "Overwrite"),
                    (PresetAction::Rename, "Rename"),
                    (PresetAction::Delete, "Delete"),
                ] {
                    row.spawn((
                        Button,
                        action,
                        Node {
                            width: Val::Px(78.0),
                            min_height: Val::Px(26.0),
                            padding: UiRect::axes(Val::Px(3.0), Val::Px(2.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.22, 0.22, 0.27)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new(label),
                            font.clone(),
                            TextColor(Color::srgb(0.95, 0.95, 0.95)),
                        ));
                    });
                }
            });

            panel.spawn((
                Button,
                PresetAction::ClearSelection,
                Node {
                    width: Val::Px(170.0),
                    min_height: Val::Px(26.0),
                    padding: UiRect::axes(Val::Px(5.0), Val::Px(2.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
            ))
            .with_children(|button| {
                button.spawn((
                    Text::new("Clear Selection"),
                    font.clone(),
                    TextColor(Color::srgb(0.95, 0.95, 0.95)),
                ));
            });

            panel.spawn((
                PresetStatusText,
                Text::new("No preset selected"),
                TextFont {
                    font_size: bevy::text::FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.82, 0.55)),
            ));

            panel.spawn((
                PresetList,
                Node {
                    width: Val::Px(330.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    ..default()
                },
            ));
        });
}

pub fn lighting_text_input_inactive(input: Option<Res<LightingInputState>>) -> bool {
    !input.is_some_and(|state| state.text_entry_active)
}

fn update_lighting_input_state(
    numeric: Res<NumericEditor>,
    ambience: Res<AmbienceNumericEditor>,
    name: Res<PresetNameEditor>,
    mut input: ResMut<LightingInputState>,
) {
    let active = numeric.field.is_some() || ambience.field.is_some() || name.focused;
    if input.text_entry_active != active {
        input.text_entry_active = active;
    }
}

fn pause_free_camera_while_typing(
    input: Res<LightingInputState>,
    mut saved_enabled: Local<Option<bool>>,
    mut cameras: Query<&mut FreeCameraState>,
) {
    for mut camera in &mut cameras {
        if input.text_entry_active {
            if saved_enabled.is_none() {
                *saved_enabled = Some(camera.enabled);
            }
            camera.enabled = false;
            camera.velocity = Vec3::ZERO;
        } else if let Some(enabled) = saved_enabled.take() {
            camera.enabled = enabled;
            camera.velocity = Vec3::ZERO;
        }
    }
}

fn keep_panel_cursor(
    panel_state: Res<LightingPanelState>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if panel_state.open {
        for mut cursor in &mut cursors {
            cursor.visible = true;
        }
    }
}

fn toggle_panel(
    keys: Res<ButtonInput<KeyCode>>,
    mut panel: Query<&mut Visibility, With<Panel>>,
    mut editor: ResMut<NumericEditor>,
    mut name_editor: ResMut<PresetNameEditor>,
    mut panel_state: ResMut<LightingPanelState>,
    mut ambience_editor: ResMut<AmbienceNumericEditor>,
) {
    if keys.just_pressed(KeyCode::F2) {
        for mut v in &mut panel {
            *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
            panel_state.open = *v != Visibility::Hidden;
            if *v == Visibility::Hidden {
                editor.field = None;
                editor.draft.clear();
                editor.replace_on_next_input = false;
                name_editor.focused = false;
                name_editor.replace_on_next_input = false;
                ambience_editor.field = None;
                ambience_editor.draft.clear();
                ambience_editor.replace_on_next_input = false;
            }
        }
    }
}

fn press_buttons(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<(&Interaction, &StepButton), Changed<Interaction>>,
    mut lighting: ResMut<Lighting>,
    mut editor: ResMut<NumericEditor>,
    mut name_editor: ResMut<PresetNameEditor>,
    mut ambience_editor: ResMut<AmbienceNumericEditor>,
) {
    let mul = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 10.0 } else { 1.0 };
    let mut changed = false;
    for (interaction, StepButton(i, sign)) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let f = &FIELDS[*i];
        let v = (f.get)(&lighting) + f.step * sign * mul;
        (f.set)(&mut lighting, v);
        changed = true;
    }
    if changed {
        editor.field = None;
        editor.draft.clear();
        editor.replace_on_next_input = false;
        name_editor.focused = false;
        name_editor.replace_on_next_input = false;
        ambience_editor.field = None;
        ambience_editor.draft.clear();
        ambience_editor.replace_on_next_input = false;
        save(&lighting);
    }
}

fn format_edit_value(value: f32, step: f32) -> String {
    if step >= 1.0 {
        return format!("{value:.0}");
    }

    let mut result = format!("{value:.5}");
    while result.contains('.') && result.ends_with('0') {
        result.pop();
    }
    if result.ends_with('.') {
        result.pop();
    }
    result
}

fn edit_numeric_fields(
    mut events: MessageReader<KeyboardInput>,
    fields: Query<(&Interaction, &NumericField), Changed<Interaction>>,
    mut lighting: ResMut<Lighting>,
    mut editor: ResMut<NumericEditor>,
    mut name_editor: ResMut<PresetNameEditor>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    for (interaction, NumericField(index)) in &fields {
        if *interaction == Interaction::Pressed {
            name_editor.focused = false;
            name_editor.replace_on_next_input = false;
            editor.field = Some(*index);
            editor.draft = format_edit_value(
                (FIELDS[*index].get)(&lighting),
                FIELDS[*index].step,
            );
            editor.replace_on_next_input = true;
        }
    }

    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let Some(index) = editor.field else {
            continue;
        };

        match &event.logical_key {
            Key::Enter => {
                if let Ok(value) = editor.draft.trim().parse::<f32>() {
                    if value.is_finite() {
                        (FIELDS[index].set)(&mut lighting, value);
                        save(&lighting);
                        editor.field = None;
                        editor.draft.clear();
                    }
                }
            }
            Key::Escape => {
                editor.field = None;
                editor.draft.clear();
            }
            Key::Backspace => {
                editor.draft.pop();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars)
                if chars.as_str().eq_ignore_ascii_case("a")
                    && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) =>
            {
                editor.draft.clear();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars) => {
                let allowed = chars.chars().all(|c| {
                    c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')
                });
                if allowed {
                    if editor.replace_on_next_input {
                        editor.draft.clear();
                        editor.replace_on_next_input = false;
                    }
                    if editor.draft.len() + chars.len() <= 24 {
                        editor.draft.push_str(chars);
                    }
                }
            }
            _ => {}
        }
    }

}

fn refresh_values(
    lighting: Res<Lighting>,
    editor: Res<NumericEditor>,
    mut texts: Query<(&ValueText, &mut Text)>,
) {
    if !lighting.is_changed() && !editor.is_changed() {
        return;
    }
    for (ValueText(i), mut t) in &mut texts {
        if editor.field == Some(*i) {
            t.0 = format!("{}_", editor.draft);
        } else {
            let v = (FIELDS[*i].get)(&lighting);
            t.0 = format_edit_value(v, FIELDS[*i].step);
        }
    }
}

// Convert the live F2 controls to and from a persistent preset snapshot.
fn controls_snapshot(lighting: &Lighting) -> crate::lighting_presets::LightingControls {
    crate::lighting_presets::LightingControls {
        sun: lighting.sun,
        back: lighting.back,
        ambient: lighting.ambient,
        lights: lighting.lights,
        exposure: lighting.exposure,
        fog: lighting.fog,
        bloom: lighting.bloom,
        saturation: lighting.saturation,
        vignette: lighting.vignette,
        tonemapping: lighting.tonemapping,
        shadows: lighting.shadows,
    }
}

fn apply_controls(
    lighting: &mut Lighting,
    controls: &crate::lighting_presets::LightingControls,
) {
    lighting.sun = controls.sun;
    lighting.back = controls.back;
    lighting.ambient = controls.ambient;
    lighting.lights = controls.lights;
    lighting.exposure = controls.exposure;
    lighting.fog = controls.fog;
    lighting.bloom = controls.bloom;
    lighting.saturation = controls.saturation;
    lighting.vignette = controls.vignette;
    lighting.tonemapping = controls.tonemapping;
    lighting.shadows = controls.shadows;
}

fn edit_preset_name(
    mut events: MessageReader<KeyboardInput>,
    fields: Query<(&Interaction, &PresetNameField), Changed<Interaction>>,
    store: Res<crate::lighting_presets::LightingStudioStore>,
    mut editor: ResMut<PresetNameEditor>,
    mut numeric: ResMut<NumericEditor>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    for (interaction, _) in &fields {
        if *interaction == Interaction::Pressed {
            numeric.field = None;
            numeric.draft.clear();
            numeric.replace_on_next_input = false;
            editor.focused = true;
            editor.replace_on_next_input = true;
        }
    }

    for event in events.read() {
        if event.state != ButtonState::Pressed || !editor.focused {
            continue;
        }

        match &event.logical_key {
            Key::Enter => {
                editor.focused = false;
                editor.replace_on_next_input = false;
            }
            Key::Escape => {
                editor.focused = false;
                editor.replace_on_next_input = false;
                editor.draft = store
                    .active_preset_id
                    .as_deref()
                    .and_then(|id| store.presets.get(id))
                    .map(|preset| preset.name.clone())
                    .unwrap_or_default();
            }
            Key::Backspace => {
                editor.draft.pop();
                editor.replace_on_next_input = false;
            }
            Key::Space => {
                if editor.replace_on_next_input {
                    editor.draft.clear();
                    editor.replace_on_next_input = false;
                }
                if editor.draft.chars().count() < 64 {
                    editor.draft.push(' ');
                }
            }
            Key::Character(chars)
                if chars.as_str().eq_ignore_ascii_case("a")
                    && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) =>
            {
                editor.draft.clear();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars) => {
                if chars.chars().any(char::is_control) {
                    continue;
                }
                if editor.replace_on_next_input {
                    editor.draft.clear();
                    editor.replace_on_next_input = false;
                }
                if editor.draft.chars().count().saturating_add(chars.chars().count()) <= 64 {
                    editor.draft.push_str(chars);
                }
            }
            _ => {}
        }
    }

}

fn persist_presets(
    store: &mut crate::lighting_presets::LightingStudioStore,
    ui: &mut PresetUiState,
) {
    if let Err(error) = store.save() {
        warn!("lighting presets: could not persist changes: {error}");
        ui.status = format!("Could not save lighting-presets.json: {error}");
    }
}

fn handle_preset_actions(
    buttons: Query<(&Interaction, &PresetAction), Changed<Interaction>>,
    lighting: Res<Lighting>,
    mut store: ResMut<crate::lighting_presets::LightingStudioStore>,
    mut name_editor: ResMut<PresetNameEditor>,
    mut numeric_editor: ResMut<NumericEditor>,
    mut ui: ResMut<PresetUiState>,
) {
    let controls = controls_snapshot(&lighting);

    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }

        name_editor.focused = false;
        name_editor.replace_on_next_input = false;
        numeric_editor.field = None;
        numeric_editor.draft.clear();
        numeric_editor.replace_on_next_input = false;

        match action {
            PresetAction::SaveAs => {
                ui.pending_delete_id = None;
                let requested_name = name_editor.draft.trim().to_string();

                match store.save_as(&requested_name, &controls) {
                    Ok(id) => {
                        let saved_name = store
                            .presets
                            .get(&id)
                            .map(|preset| preset.name.clone())
                            .unwrap_or(requested_name);

                        name_editor.draft = saved_name.clone();
                        ui.status = format!("Saved new preset '{saved_name}'.");
                        persist_presets(&mut *store, &mut *ui);
                    }
                    Err(error) => {
                        ui.status = format!("Save As failed: {error}");
                    }
                }
            }

            PresetAction::Overwrite => {
                ui.pending_delete_id = None;
                let Some(id) = store.active_preset_id.clone() else {
                    ui.status = "Select a preset before overwriting.".to_string();
                    continue;
                };
                let preset_name = store
                    .presets
                    .get(&id)
                    .map(|preset| preset.name.clone())
                    .unwrap_or_else(|| id.clone());

                match store.overwrite_active(&controls) {
                    Ok(()) => {
                        ui.status = format!("Overwrote '{preset_name}'.");
                        persist_presets(&mut *store, &mut *ui);
                    }
                    Err(error) => {
                        ui.status = format!("Overwrite failed: {error}");
                    }
                }
            }

            PresetAction::Rename => {
                ui.pending_delete_id = None;
                let Some(id) = store.active_preset_id.clone() else {
                    ui.status = "Select a preset before renaming.".to_string();
                    continue;
                };
                let new_name = name_editor.draft.trim().to_string();

                match store.rename_preset(&id, &new_name) {
                    Ok(()) => {
                        name_editor.draft = new_name.clone();
                        ui.status = format!("Renamed active preset to '{new_name}'.");
                        persist_presets(&mut *store, &mut *ui);
                    }
                    Err(error) => {
                        ui.status = format!("Rename failed: {error}");
                    }
                }
            }

            PresetAction::ClearSelection => {
                store.active_preset_id = None;
                ui.pending_delete_id = None;
                name_editor.draft.clear();
                ui.status = "Selection cleared. Current lighting and working overrides were kept.".to_string();
                persist_presets(&mut *store, &mut *ui);
            }

            PresetAction::Delete => {
                let Some(id) = store.active_preset_id.clone() else {
                    ui.pending_delete_id = None;
                    ui.status = "Select a preset before deleting.".to_string();
                    continue;
                };

                let preset_name = store
                    .presets
                    .get(&id)
                    .map(|preset| preset.name.clone())
                    .unwrap_or_else(|| id.clone());

                if ui.pending_delete_id.as_deref() == Some(id.as_str()) {
                    if store.delete_preset(&id) {
                        name_editor.draft.clear();
                        ui.pending_delete_id = None;
                        ui.status = format!("Deleted '{preset_name}'. Working settings were kept.");
                        persist_presets(&mut *store, &mut *ui);
                    } else {
                        ui.pending_delete_id = None;
                        ui.status = "The selected preset no longer exists.".to_string();
                    }
                } else {
                    ui.pending_delete_id = Some(id);
                    ui.status = format!(
                        "Click Delete again to confirm deleting '{preset_name}'."
                    );
                }
            }
        }
    }
}

fn select_preset_from_list(
    choices: Query<(&Interaction, &PresetChoice), Changed<Interaction>>,
    mut store: ResMut<crate::lighting_presets::LightingStudioStore>,
    mut lighting: ResMut<Lighting>,
    mut name_editor: ResMut<PresetNameEditor>,
    mut numeric_editor: ResMut<NumericEditor>,
    mut ui: ResMut<PresetUiState>,
) {
    for (interaction, choice) in &choices {
        if *interaction != Interaction::Pressed {
            continue;
        }

        let Some(preset) = store.select_preset(&choice.0) else {
            ui.status = "That preset could not be found.".to_string();
            continue;
        };

        let selected_name = preset.name.clone();
        apply_controls(&mut *lighting, &preset.controls);
        save(&*lighting);

        name_editor.draft = selected_name.clone();
        name_editor.focused = false;
        name_editor.replace_on_next_input = false;
        numeric_editor.field = None;
        numeric_editor.draft.clear();
        numeric_editor.replace_on_next_input = false;
        ui.pending_delete_id = None;
        ui.status = format!("Loaded '{selected_name}'.");

        persist_presets(&mut *store, &mut *ui);
    }
}

fn refresh_preset_name(
    editor: Res<PresetNameEditor>,
    mut texts: Query<&mut Text, With<PresetNameText>>,
) {
    if !editor.is_changed() {
        return;
    }

    let displayed = if editor.focused {
        format!("{}_", editor.draft)
    } else if editor.draft.is_empty() {
        "Click to enter a preset name".to_string()
    } else {
        editor.draft.clone()
    };

    for mut text in &mut texts {
        text.0 = displayed.clone();
    }
}

fn refresh_preset_status(
    store: Res<crate::lighting_presets::LightingStudioStore>,
    lighting: Res<Lighting>,
    ui: Res<PresetUiState>,
    mut texts: Query<&mut Text, With<PresetStatusText>>,
) {
    if !store.is_changed() && !lighting.is_changed() && !ui.is_changed() {
        return;
    }

    let controls = controls_snapshot(&lighting);
    let heading = if let Some(id) = store.active_preset_id.as_deref() {
        if let Some(preset) = store.presets.get(id) {
            if store.active_preset_is_modified(&controls) {
                format!("Active: {}  * UNSAVED CHANGES", preset.name)
            } else {
                format!("Active: {}", preset.name)
            }
        } else {
            "No preset selected".to_string()
        }
    } else {
        "No preset selected".to_string()
    };

    let displayed = if ui.status.is_empty() {
        heading
    } else {
        format!("{heading}\n{}", ui.status)
    };

    for mut text in &mut texts {
        text.0 = displayed.clone();
    }
}

fn rebuild_preset_list(
    mut commands: Commands,
    store: Res<crate::lighting_presets::LightingStudioStore>,
    lists: Query<Entity, With<PresetList>>,
    rows: Query<Entity, With<PresetRow>>,
) {
    if !store.is_changed() {
        return;
    }

    for row in &rows {
        commands.entity(row).despawn();
    }

    let font = TextFont {
        font_size: bevy::text::FontSize::Px(13.0),
        ..default()
    };

    for parent in &lists {
        commands.entity(parent).with_children(|list| {
            for (id, preset) in &store.presets {
                let active = store.active_preset_id.as_deref() == Some(id.as_str());
                let label = if active {
                    format!("✓  {}", preset.name)
                } else {
                    preset.name.clone()
                };
                let background = if active {
                    Color::srgb(0.30, 0.24, 0.12)
                } else {
                    Color::srgb(0.17, 0.17, 0.21)
                };

                list.spawn((
                    Button,
                    PresetChoice(id.clone()),
                    PresetRow,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(24.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(background),
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new(label),
                        font.clone(),
                        TextColor(Color::srgb(0.95, 0.95, 0.95)),
                    ));
                });
            }
        });
    }
}
// Resolve the current map's source global ambience plus its working overrides.
fn resolved_map_global(
    current: Option<&CurrentMap>,
    level: &Level,
    ambiences: &Assets<Ambience>,
    store: &crate::lighting_presets::LightingStudioStore,
) -> Option<Bubble> {
    let current = current?;
    if level.map != current.name {
        return None;
    }
    let ambience = ambiences.get(&level.ambience)?;
    let source = ambience.global.clone().unwrap_or_default();
    Some(crate::lighting_presets::resolve_bubble(
        &source,
        store.working_map_overrides.get(&current.name),
        None,
    ))
}


// Resolve the selected ambience target. None means the map-global bubble.
fn resolved_map_target(
    current: Option<&CurrentMap>,
    level: &Level,
    ambiences: &Assets<Ambience>,
    store: &crate::lighting_presets::LightingStudioStore,
    selected_bubble: Option<usize>,
) -> Option<Bubble> {
    let Some(index) = selected_bubble else {
        return resolved_map_global(current, level, ambiences, store);
    };

    let current = current?;
    if level.map != current.name {
        return None;
    }

    let ambience = ambiences.get(&level.ambience)?;
    let source_area = ambience.bubbles.get(index)?;

    // Only always-active bubbles with a transform can currently be selected:
    // these are the local bubbles that follow_level actually evaluates.
    if source_area.event != -1 || source_area.m.is_none() {
        return None;
    }

    Some(crate::lighting_presets::resolve_bubble(
        &source_area.look,
        store.working_map_overrides.get(&current.name),
        Some((index, source_area)),
    ))
}
fn set_global_ambience_override(
    store: &mut crate::lighting_presets::LightingStudioStore,
    map_name: &str,
    bubble: &Bubble,
    field: AmbienceField,
    value: f32,
) {
    let map = store.working_map_overrides
        .entry(map_name.to_string())
        .or_default();
    let overrides = &mut map.global;

    if let Some((target, channel)) = field.color_channel() {
        let mut color = target.color(bubble);
        color[channel] = value.clamp(0.0, 4.0);
        match target {
            AmbienceColorTarget::Key => overrides.light_color = Some(color),
            AmbienceColorTarget::Back => overrides.light_opposite_color = Some(color),
            AmbienceColorTarget::Ambient => overrides.light_ambient = Some(color),
            AmbienceColorTarget::Fog => overrides.fog_color = Some(color),
            AmbienceColorTarget::Player => overrides.local_light_color = Some(color),
        }
        return;
    }

    match field {
        AmbienceField::LightIntensity => {
            overrides.light_intensity = Some(value.clamp(0.0, 20.0));
        }
        AmbienceField::BackLightFactor => {
            overrides.back_light_factor = Some(value.clamp(-5.0, 10.0));
        }
        AmbienceField::DirectionX | AmbienceField::DirectionY | AmbienceField::DirectionZ => {
            let mut direction = bubble.dir.unwrap_or([-0.136, 0.721, -0.679]);
            let axis = match field {
                AmbienceField::DirectionX => 0,
                AmbienceField::DirectionY => 1,
                _ => 2,
            };
            direction[axis] = value.clamp(-1.0, 1.0);
            overrides.dir = Some(direction);
        }
        AmbienceField::FogIntensity => {
            overrides.fog_intensity = Some(value.clamp(0.0, 1.0));
        }
        AmbienceField::FogNear => {
            let max_near = (bubble.fog_far_dist - 0.1).max(0.0);
            overrides.fog_near_dist = Some(value.clamp(0.0, max_near));
        }
        AmbienceField::FogFar => {
            overrides.fog_far_dist = Some(
                value.max(bubble.fog_near_dist + 0.1).min(100_000.0)
            );
        }
        AmbienceField::PlayerLightIntensity => {
            overrides.local_light_intensity = Some(value.clamp(0.0, 100.0));
        }
        AmbienceField::PlayerLightRange => {
            overrides.local_light_range = Some(value.clamp(0.1, 1000.0));
        }
        AmbienceField::Saturation => {
            overrides.saturation = Some(value.clamp(0.0, 4.0));
        }
        AmbienceField::BloomScale => {
            overrides.bloom_scale = Some(value.clamp(0.0, 10.0));
        }
        AmbienceField::BrightPassThreshold => {
            overrides.bright_pass_threshold = Some(value.clamp(0.0, 4.0));
        }
        AmbienceField::VignetteIntensity => {
            overrides.vignette_intensity = Some(value.clamp(0.0, 10.0));
        }
        AmbienceField::VignetteSize => {
            overrides.vignette_size = Some(value.clamp(0.0, 10.0));
        }
        _ => unreachable!("color fields are handled above"),
    }
}


// Write one field into an existing BubbleOverrides record.
fn set_bubble_override_value(
    overrides: &mut crate::lighting_presets::BubbleOverrides,
    bubble: &Bubble,
    field: AmbienceField,
    value: f32,
) {
    if let Some((target, channel)) = field.color_channel() {
        let mut color = target.color(bubble);
        color[channel] = value.clamp(0.0, 4.0);
        match target {
            AmbienceColorTarget::Key => overrides.light_color = Some(color),
            AmbienceColorTarget::Back => overrides.light_opposite_color = Some(color),
            AmbienceColorTarget::Ambient => overrides.light_ambient = Some(color),
            AmbienceColorTarget::Fog => overrides.fog_color = Some(color),
            AmbienceColorTarget::Player => overrides.local_light_color = Some(color),
        }
        return;
    }

    match field {
        AmbienceField::LightIntensity => {
            overrides.light_intensity = Some(value.clamp(0.0, 20.0));
        }
        AmbienceField::BackLightFactor => {
            overrides.back_light_factor = Some(value.clamp(-5.0, 10.0));
        }
        AmbienceField::DirectionX
        | AmbienceField::DirectionY
        | AmbienceField::DirectionZ => {
            let mut direction = bubble.dir.unwrap_or([-0.136, 0.721, -0.679]);
            let axis = match field {
                AmbienceField::DirectionX => 0,
                AmbienceField::DirectionY => 1,
                _ => 2,
            };
            direction[axis] = value.clamp(-1.0, 1.0);
            overrides.dir = Some(direction);
        }
        AmbienceField::FogIntensity => {
            overrides.fog_intensity = Some(value.clamp(0.0, 1.0));
        }
        AmbienceField::FogNear => {
            let max_near = (bubble.fog_far_dist - 0.1).max(0.0);
            overrides.fog_near_dist = Some(value.clamp(0.0, max_near));
        }
        AmbienceField::FogFar => {
            overrides.fog_far_dist = Some(
                value.max(bubble.fog_near_dist + 0.1).min(100_000.0)
            );
        }
        AmbienceField::PlayerLightIntensity => {
            overrides.local_light_intensity = Some(value.clamp(0.0, 100.0));
        }
        AmbienceField::PlayerLightRange => {
            overrides.local_light_range = Some(value.clamp(0.1, 1000.0));
        }
        AmbienceField::Saturation => {
            overrides.saturation = Some(value.clamp(0.0, 4.0));
        }
        AmbienceField::BloomScale => {
            overrides.bloom_scale = Some(value.clamp(0.0, 10.0));
        }
        AmbienceField::BrightPassThreshold => {
            overrides.bright_pass_threshold = Some(value.clamp(0.0, 4.0));
        }
        AmbienceField::VignetteIntensity => {
            overrides.vignette_intensity = Some(value.clamp(0.0, 10.0));
        }
        AmbienceField::VignetteSize => {
            overrides.vignette_size = Some(value.clamp(0.0, 10.0));
        }
        _ => unreachable!("color fields are handled above"),
    }
}

// Record an edit against this bubble's identity, not its display label alone.
fn set_area_ambience_override(
    store: &mut crate::lighting_presets::LightingStudioStore,
    map_name: &str,
    bubble_index: usize,
    source_area: &LocalBubble,
    resolved_bubble: &Bubble,
    field: AmbienceField,
    value: f32,
) {
    let map = store
        .working_map_overrides
        .entry(map_name.to_string())
        .or_default();

    let existing_index = map.areas.iter().position(|candidate| {
        candidate.bubble_index == bubble_index
            && candidate.event == source_area.event
            && candidate.priority == source_area.priority
            && candidate.shape == source_area.shape
            && candidate.transform == source_area.m
    });

    let area_index = if let Some(index) = existing_index {
        index
    } else {
        map.areas.push(crate::lighting_presets::AreaLightingOverride {
            bubble_index,
            event: source_area.event,
            priority: source_area.priority,
            shape: source_area.shape.clone(),
            transform: source_area.m,
            values: crate::lighting_presets::BubbleOverrides::default(),
        });
        map.areas.len() - 1
    };

    set_bubble_override_value(
        &mut map.areas[area_index].values,
        resolved_bubble,
        field,
        value,
    );
}
fn blur_text_editors_on_ui_click(
    buttons: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    mut numeric: ResMut<NumericEditor>,
    mut ambience: ResMut<AmbienceNumericEditor>,
    mut name: ResMut<PresetNameEditor>,
) {
    if !buttons.iter().any(|interaction| *interaction == Interaction::Pressed) {
        return;
    }

    numeric.field = None;
    numeric.draft.clear();
    numeric.replace_on_next_input = false;
    ambience.field = None;
    ambience.draft.clear();
    ambience.replace_on_next_input = false;
    name.focused = false;
    name.replace_on_next_input = false;
}

fn toggle_map_ambience(
    buttons: Query<&Interaction, (Changed<Interaction>, With<MapAmbienceToggle>)>,
    mut state: ResMut<MapAmbienceUi>,
    mut sections: Query<&mut Visibility, With<MapAmbienceSection>>,
    mut labels: Query<&mut Text, With<MapAmbienceToggleText>>,
) {
    let mut toggled = false;
    for interaction in &buttons {
        if *interaction == Interaction::Pressed {
            state.expanded = !state.expanded;
            toggled = true;
        }
    }
    if !toggled {
        return;
    }

    for mut visibility in &mut sections {
        *visibility = if state.expanded {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for mut text in &mut labels {
        text.0 = format!(
            "Map Ambience  {}",
            if state.expanded { "▾" } else { "▸" }
        );
    }
}

fn toggle_ambience_group(
    buttons: Query<(&Interaction, &AmbienceGroupToggle), Changed<Interaction>>,
    mut state: ResMut<MapAmbienceUi>,
    mut bodies: Query<(&AmbienceGroupBody, &mut Visibility)>,
    mut titles: Query<(&AmbienceGroupTitle, &mut Text)>,
) {
    let mut toggled = false;
    for (interaction, group) in &buttons {
        if *interaction == Interaction::Pressed {
            let index = group.0.index();
            state.groups[index] = !state.groups[index];
            toggled = true;
        }
    }
    if !toggled {
        return;
    }

    for (group, mut visibility) in &mut bodies {
        *visibility = if state.groups[group.0.index()] {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (group, mut text) in &mut titles {
        text.0 = format!(
            "{}  {}",
            group.0.label(),
            if state.groups[group.0.index()] { "▾" } else { "▸" }
        );
    }
}


// Cycle through the map-global ambience and supported local ambience bubbles.
fn cycle_ambience_target(
    buttons: Query<(&Interaction, &AmbienceTargetStep), Changed<Interaction>>,
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    mut map_ui: ResMut<MapAmbienceUi>,
    mut editor: ResMut<AmbienceNumericEditor>,
    mut ui: ResMut<PresetUiState>,
) {
    let steps: Vec<i8> = buttons
        .iter()
        .filter_map(|(interaction, step)| {
            (*interaction == Interaction::Pressed).then_some(step.0)
        })
        .collect();

    if steps.is_empty() {
        return;
    }

    let Some(current) = current.as_deref() else {
        ui.status = "Load a map before selecting an ambience target.".to_string();
        return;
    };

    let Some(ambience) = ambiences.get(&level.ambience) else {
        ui.status = "Map ambience is still loading.".to_string();
        return;
    };

    let targets: Vec<Option<usize>> = std::iter::once(None)
        .chain(
            ambience
                .bubbles
                .iter()
                .enumerate()
                .filter(|(_, area)| area.event == -1 && area.m.is_some())
                .map(|(index, _)| Some(index)),
        )
        .collect();

    for step in steps {
        let current_position = targets
            .iter()
            .position(|target| *target == map_ui.selected_bubble)
            .unwrap_or(0);

        let next_position = (current_position as isize + step as isize)
            .rem_euclid(targets.len() as isize) as usize;

        map_ui.selected_bubble = targets[next_position];

        editor.field = None;
        editor.draft.clear();
        editor.replace_on_next_input = false;

        ui.pending_delete_id = None;
        ui.status = match map_ui.selected_bubble {
            None => format!("Selected map-global ambience for {}.", current.name),
            Some(index) => format!(
                "Selected area #{} for {}. Its override applies when that area is active.",
                index + 1,
                current.name,
            ),
        };
    }
}

// Keep the selected target label accurate when maps or ambience assets change.
fn refresh_ambience_target_label(
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    mut map_ui: ResMut<MapAmbienceUi>,
    mut labels: Query<&mut Text, With<AmbienceTargetLabel>>,
) {
    let map_name = current.as_ref().map(|map| map.name.clone());

    if map_ui.map_name != map_name {
        map_ui.map_name = map_name.clone();
        map_ui.selected_bubble = None;
    }

    let label = match current.as_deref() {
        None => "Target: no map loaded".to_string(),
        Some(current) if level.map != current.name => {
            "Target: loading map...".to_string()
        }
        Some(current) => match ambiences.get(&level.ambience) {
            None => "Target: loading map ambience...".to_string(),
            Some(ambience) => {
                let area_count = ambience
                    .bubbles
                    .iter()
                    .filter(|area| area.event == -1 && area.m.is_some())
                    .count();

                match map_ui.selected_bubble {
                    None => format!(
                        "Target: Global ({} editable areas)",
                        area_count
                    ),
                    Some(index) => match ambience.bubbles.get(index) {
                        Some(area) if area.event == -1 && area.m.is_some() => {
                            format!(
                                "Target: Area #{} | {} | priority {} | event {}",
                                index + 1,
                                area.shape,
                                area.priority,
                                area.event,
                            )
                        }
                        _ => {
                            map_ui.selected_bubble = None;
                            format!(
                                "Target: Global ({} editable areas)",
                                area_count
                            )
                        }
                    },
                }
            }
        },
    };

    for mut text in &mut labels {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}
fn step_ambience_fields(
    buttons: Query<(&Interaction, &AmbienceStepButton), Changed<Interaction>>,
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    map_ui: Res<MapAmbienceUi>,
    mut store: ResMut<crate::lighting_presets::LightingStudioStore>,
    mut ui: ResMut<PresetUiState>,
) {
    let steps: Vec<(AmbienceField, f32)> = buttons
        .iter()
        .filter_map(|(interaction, step)| {
            (*interaction == Interaction::Pressed).then_some((step.0, step.1))
        })
        .collect();

    if steps.is_empty() {
        return;
    }

    let Some(current) = current.as_deref() else {
        ui.status = "Load a map before editing ambience.".to_string();
        return;
    };

    if level.map != current.name {
        ui.status = "Map ambience is still loading.".to_string();
        return;
    }

    let map_name = current.name.clone();
    let Some(bubble) = resolved_map_target(
        Some(current),
        &level,
        &ambiences,
        &store,
        map_ui.selected_bubble,
    ) else {
        ui.status = "Map ambience is still loading.".to_string();
        return;
    };

    let mut applied = false;

    for (field, direction) in steps {
        let value = field.value(&bubble) + field.step() * direction;

        if let Some(index) = map_ui.selected_bubble {
            let Some(ambience) = ambiences.get(&level.ambience) else {
                ui.status = "Map ambience is still loading.".to_string();
                continue;
            };

            let Some(source_area) = ambience.bubbles.get(index) else {
                ui.status = "The selected ambience area no longer exists.".to_string();
                continue;
            };

            if source_area.event != -1 || source_area.m.is_none() {
                ui.status = "The selected area is not an editable active bubble.".to_string();
                continue;
            }

            set_area_ambience_override(
                &mut *store,
                &map_name,
                index,
                source_area,
                &bubble,
                field,
                value,
            );
        } else {
            set_global_ambience_override(
                &mut *store,
                &map_name,
                &bubble,
                field,
                value,
            );
        }

        ui.status = format!("Adjusted {}.", field.label());
        applied = true;
    }

    if applied {
        persist_presets(&mut *store, &mut *ui);
    }
}

fn edit_ambience_fields(
    mut events: MessageReader<KeyboardInput>,
    fields: Query<(&Interaction, &AmbienceNumericField), Changed<Interaction>>,
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    map_ui: Res<MapAmbienceUi>,
    mut store: ResMut<crate::lighting_presets::LightingStudioStore>,
    mut editor: ResMut<AmbienceNumericEditor>,
    mut numeric: ResMut<NumericEditor>,
    mut name_editor: ResMut<PresetNameEditor>,
    keys: Res<ButtonInput<KeyCode>>,
    mut ui: ResMut<PresetUiState>,
) {
    let map_name = current.as_ref().map(|map| map.name.clone());
    let bubble = resolved_map_target(current.as_deref(), &level, &ambiences, &store, map_ui.selected_bubble);

    for (interaction, AmbienceNumericField(field)) in &fields {
        if *interaction != Interaction::Pressed {
            continue;
        }

        let Some(bubble) = bubble.as_ref() else {
            ui.status = "Map ambience is still loading.".to_string();
            continue;
        };

        numeric.field = None;
        numeric.draft.clear();
        numeric.replace_on_next_input = false;
        name_editor.focused = false;
        name_editor.replace_on_next_input = false;

        editor.field = Some(*field);
        editor.draft = format_edit_value(field.value(bubble), field.step());
        editor.replace_on_next_input = true;
    }

    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let Some(field) = editor.field else {
            continue;
        };

        match &event.logical_key {
            Key::Enter => {
                let Ok(value) = editor.draft.trim().parse::<f32>() else {
                    continue;
                };
                if !value.is_finite() {
                    continue;
                }

                let (Some(map_name), Some(bubble)) = (map_name.as_deref(), bubble.as_ref()) else {
                    ui.status = "Map ambience is still loading.".to_string();
                    continue;
                };

                if let Some(index) = map_ui.selected_bubble {
                    let Some(ambience) = ambiences.get(&level.ambience) else {
                        ui.status = "Map ambience is still loading.".to_string();
                        continue;
                    };
                    let Some(source_area) = ambience.bubbles.get(index) else {
                        ui.status = "The selected ambience area no longer exists.".to_string();
                        continue;
                    };

                    if source_area.event != -1 || source_area.m.is_none() {
                        ui.status = "The selected area is not an editable active bubble.".to_string();
                        continue;
                    }

                    set_area_ambience_override(
                        &mut store,
                        map_name,
                        index,
                        source_area,
                        bubble,
                        field,
                        value,
                    );

                    ui.status = format!(
                        "Updated {} for area #{} in {}. It applies when that area is active.",
                        field.label(),
                        index + 1,
                        map_name,
                    );
                } else {
                    set_global_ambience_override(
                        &mut store,
                        map_name,
                        bubble,
                        field,
                        value,
                    );

                    ui.status = format!(
                        "Updated {} for {}.",
                        field.label(),
                        map_name,
                    );
                }
                persist_presets(&mut *store, &mut *ui);

                editor.field = None;
                editor.draft.clear();
                editor.replace_on_next_input = false;
            }
            Key::Escape => {
                editor.field = None;
                editor.draft.clear();
                editor.replace_on_next_input = false;
            }
            Key::Backspace => {
                editor.draft.pop();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars)
                if chars.as_str().eq_ignore_ascii_case("a")
                    && (keys.pressed(KeyCode::ControlLeft)
                        || keys.pressed(KeyCode::ControlRight)) =>
            {
                editor.draft.clear();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars) => {
                let allowed = chars.chars().all(|c| {
                    c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')
                });
                if allowed {
                    if editor.replace_on_next_input {
                        editor.draft.clear();
                        editor.replace_on_next_input = false;
                    }
                    if editor.draft.len() + chars.len() <= 24 {
                        editor.draft.push_str(chars);
                    }
                }
            }
            _ => {}
        }
    }
}

fn refresh_ambience_values(
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    store: Res<crate::lighting_presets::LightingStudioStore>,
    editor: Res<AmbienceNumericEditor>,
    map_ui: Res<MapAmbienceUi>,
    mut texts: Query<(&AmbienceValueText, &mut Text)>,
) {
    let map_changed = current.as_ref().is_some_and(|map| map.is_changed());
    if !store.is_changed()
        && !level.is_changed()
        && !ambiences.is_changed()
        && !editor.is_changed()
        && !map_ui.is_changed()
        && !map_changed
    {
        return;
    }

    let bubble = resolved_map_target(current.as_deref(), &level, &ambiences, &store, map_ui.selected_bubble);
    for (AmbienceValueText(field), mut text) in &mut texts {
        if editor.field == Some(*field) {
            text.0 = format!("{}_", editor.draft);
        } else if let Some(bubble) = bubble.as_ref() {
            text.0 = format_edit_value(field.value(bubble), field.step());
        } else {
            text.0 = "—".to_string();
        }
    }
}

// Scroll over any visible part of the F2 panel, including text and empty space.
fn scroll_lighting_panel(
    mut wheel_events: MessageReader<MouseWheel>,
    mut panels: Query<(&RelativeCursorPosition, &mut ScrollPosition, &ComputedNode), With<Panel>>,
) {
    for wheel in wheel_events.read() {
        let delta = match wheel.unit {
            MouseScrollUnit::Line => wheel.y * 20.0,
            MouseScrollUnit::Pixel => wheel.y,
        };

        if delta == 0.0 {
            continue;
        }

        for (cursor, mut position, node) in &mut panels {
            if !cursor.cursor_over {
                continue;
            }

            let max_scroll = (node.content_size.y - node.size.y).max(0.0)
                * node.inverse_scale_factor;

            position.y = (position.y - delta).clamp(0.0, max_scroll);
        }
    }
}


// Prevent the free-camera speed from changing while the F2 panel is open.
// The original setting is restored when the panel closes.
fn pause_camera_scroll_speed_while_panel_open(
    panel: Res<LightingPanelState>,
    mut saved_factor: Local<Option<f32>>,
    mut cameras: Query<&mut FreeCamera>,
) {
    for mut camera in &mut cameras {
        if panel.open {
            if saved_factor.is_none() {
                *saved_factor = Some(camera.scroll_factor);
            }
            camera.scroll_factor = 0.0;
        } else if let Some(original_factor) = saved_factor.take() {
            camera.scroll_factor = original_factor;
        }
    }
}


// Highlight the selected local ambience region in the 3D world.
fn draw_selected_ambience_area(
    panel: Res<LightingPanelState>,
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    map_ui: Res<MapAmbienceUi>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    let preview_mode = std::env::var_os("DSOR_STUDIO_PREVIEW").is_some();
    if !panel.open || (!map_ui.expanded && !preview_mode) {
        return;
    }

    let Some(index) = map_ui.selected_bubble else {
        return;
    };
    let Some(current) = current.as_deref() else {
        return;
    };
    if level.map != current.name {
        return;
    }

    let Some(ambience) = ambiences.get(&level.ambience) else {
        return;
    };
    let Some(area) = ambience.bubbles.get(index) else {
        return;
    };
    if area.event != -1 {
        return;
    }
    let Some(matrix) = area.m else {
        return;
    };

    // This is the same local-to-world matrix used by LocalBubble::contains.
    let transform = Transform::from_matrix(Mat4::from_cols_array(&matrix));
    let marker_color = Color::srgb(1.0, 0.12, 0.85);
    let center = transform.translation;

    if area.shape == "Sphere" {
        // The source bubble's local sphere has a radius of 0.5.
        // Use the largest scale as a visual guide for non-uniform scales.
        let radius = 0.5 * transform.scale.abs().max_element();
        gizmos.sphere(
            Isometry3d::new(center, transform.rotation),
            radius,
            marker_color,
        );
    } else {
        // A transformed unit cube matches the box containment volume.
        gizmos.cube(transform, marker_color);
    }

    gizmos.cross(center, 0.5, Color::srgb(1.0, 0.85, 0.15));

    // Point from the camera to the selected area's center so it can be
    // located even when the volume is not currently in the camera view.
    if let Some(camera) = cameras.iter().next() {
        let origin = camera.translation();
        if origin.distance(center) > 1.0 {
            gizmos.line(origin, center, Color::srgb(1.0, 0.5, 0.1));
        }
    }
}
