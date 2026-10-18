//! Player rendering preferences, separate from the map's authored lighting.
use bevy::anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, fxaa::Fxaa, smaa::{Smaa, SmaaPreset}};
use bevy::light::{DirectionalLightShadowMap, ShadowFilteringMethod};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(all(feature = "dlss", any(target_os = "windows", target_os = "linux")))]
mod dlss;

/// Call before DefaultPlugins, including in the standalone level editor.
pub(crate) fn configure_native_dlss(_app: &mut App) {
    #[cfg(all(feature = "dlss", any(target_os = "windows", target_os = "linux")))]
    dlss::configure(_app);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DlssMode { #[default] Off, Dlaa, Quality, Balanced, Performance }

impl DlssMode {
    fn next(self) -> Self { match self { Self::Off => Self::Dlaa, Self::Dlaa => Self::Quality,
        Self::Quality => Self::Balanced, Self::Balanced => Self::Performance, Self::Performance => Self::Off } }
    fn label(self) -> &'static str { match self { Self::Off => "Off", Self::Dlaa => "DLAA (native resolution)",
        Self::Quality => "DLSS - Quality", Self::Balanced => "DLSS - Balanced", Self::Performance => "DLSS - Performance" } }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DlssAvailability { Available, DesktopOnly, NotCompiled, Unsupported }

impl Default for DlssAvailability {
    fn default() -> Self {
        if !cfg!(any(target_os = "windows", target_os = "linux")) { Self::DesktopOnly }
        else if cfg!(feature = "dlss") { Self::Unsupported }
        else { Self::NotCompiled }
    }
}

impl DlssAvailability {
    fn active(self, mode: DlssMode) -> bool { self == Self::Available && mode != DlssMode::Off }
    fn label(self, mode: DlssMode) -> &'static str { match self {
        Self::Available => mode.label(), Self::DesktopOnly => "Windows / Linux only",
        Self::NotCompiled => "Not in this build", Self::Unsupported => "Unavailable on this device",
    } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AntiAliasing { #[default] Auto, Off, Fxaa, SmaaLow, SmaaHigh }

impl AntiAliasing {
    fn next(self) -> Self {
        match self { Self::Auto => Self::Off, Self::Off => Self::Fxaa, Self::Fxaa => Self::SmaaLow,
            Self::SmaaLow => Self::SmaaHigh, Self::SmaaHigh => Self::Auto }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Auto if cfg!(target_arch = "wasm32") => "Default",
            Self::Auto => "Default (FXAA)", Self::Off => "Off", Self::Fxaa => "FXAA",
            Self::SmaaLow => "SMAA - Low", Self::SmaaHigh => "SMAA - High",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Quality { Off, Low, Medium, High }

impl Quality {
    fn next(self) -> Self { match self { Self::Off => Self::Low, Self::Low => Self::Medium, Self::Medium => Self::High, Self::High => Self::Off } }
    fn label(self) -> &'static str { match self { Self::Off => "Off", Self::Low => "Low", Self::Medium => "Medium", Self::High => "High" } }
    fn shadow_size(self) -> usize { match self { Self::Off | Self::Low => 512, Self::Medium => 1024, Self::High => 2048 } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShadowFilter { #[default] Auto, Fast, Soft }

impl ShadowFilter {
    fn next(self) -> Self { match self { Self::Auto => Self::Fast, Self::Fast => Self::Soft, Self::Soft => Self::Auto } }
    fn label(self) -> &'static str { match self { Self::Auto => "Default", Self::Fast => "Fast", Self::Soft => "Soft" } }
}

#[derive(Resource, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GraphicsSettings {
    pub anti_aliasing: AntiAliasing,
    pub dlss: DlssMode,
    pub shadows: Quality,
    pub shadow_filter: ShadowFilter,
    pub ambient_occlusion: Quality,
    pub sharpening: u8,
    pub bloom: bool,
    pub vignette: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self { anti_aliasing: AntiAliasing::Auto, dlss: DlssMode::Off,
            shadows: if crate::WEBGL2 { Quality::Medium } else { Quality::High },
            shadow_filter: ShadowFilter::Auto, ambient_occlusion: Quality::Medium,
            sharpening: 0, bloom: true, vignette: false }
    }
}

impl GraphicsSettings {
    pub(crate) fn rows(&self, dlss: DlssAvailability) -> Vec<(&'static str, &'static str, String)> {
        let on = |v| if v { "On" } else { "Off" }.to_owned();
        vec![
            (if dlss.active(self.dlss) { "unavailable" } else { "anti_aliasing" }, "Anti-aliasing",
                if dlss.active(self.dlss) { "Handled by DLSS / DLAA" } else { self.anti_aliasing.label() }.into()),
            ("shadows", "Sun shadow quality", self.shadows.label().into()),
            (if crate::WEBGL2 { "unavailable" } else { "shadow_filter" }, "Shadow filtering",
                if crate::WEBGL2 { "Fast (WebGL2)" } else { self.shadow_filter.label() }.into()),
            (if cfg!(target_arch = "wasm32") { "unavailable" } else { "ambient_occlusion" }, "Ambient occlusion (SSAO)",
                if cfg!(target_arch = "wasm32") { "Desktop only" } else { self.ambient_occlusion.label() }.into()),
            ("sharpening", "Sharpening (CAS)", format!("{}%", self.sharpening)),
            ("bloom", "Bloom", on(self.bloom)),
            ("vignette", "Vignette", on(self.vignette)),
            (if dlss == DlssAvailability::Available { "dlss" } else { "unavailable" }, "NVIDIA DLSS / DLAA", dlss.label(self.dlss).into()),
            ("unavailable", "FSR / XeSS upscaling", "Not supported".into()),
        ]
    }

    pub(crate) fn action(&mut self, id: &str, dlss: DlssAvailability) {
        match id {
            "anti_aliasing" if !dlss.active(self.dlss) => self.anti_aliasing = self.anti_aliasing.next(),
            "dlss" if dlss == DlssAvailability::Available => self.dlss = self.dlss.next(),
            "shadows" => self.shadows = self.shadows.next(),
            "shadow_filter" if !crate::WEBGL2 => self.shadow_filter = self.shadow_filter.next(),
            "ambient_occlusion" if !cfg!(target_arch = "wasm32") => self.ambient_occlusion = self.ambient_occlusion.next(),
            "bloom" => self.bloom ^= true,
            "vignette" => self.vignette ^= true,
            _ => {}
        }
    }
}

/// Only the game view uses player quality settings; editor/outline cameras retain theirs.
#[derive(Component)]
pub(crate) struct GameGraphics;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ApplyGraphics;

pub(crate) struct GraphicsPlugin;
impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GraphicsSettings>()
            .init_resource::<DlssAvailability>()
            .add_systems(PostUpdate, (sync_preferences, apply_cameras).chain().in_set(ApplyGraphics));
        #[cfg(all(feature = "dlss", any(target_os = "windows", target_os = "linux")))]
        {
            app.add_systems(PreUpdate, dlss::prepare_shaders)
                .add_systems(PostUpdate, dlss::apply.in_set(ApplyGraphics).after(apply_cameras).after(bevy::transform::TransformSystems::Propagate));
            dlss::install_cleanup(app);
        }
    }
    fn finish(&self, _app: &mut App) {
        #[cfg(all(feature = "dlss", any(target_os = "windows", target_os = "linux")))]
        dlss::detect(_app);
    }
}

fn sync_preferences(mut settings: ResMut<crate::ui::Settings>, mut graphics: ResMut<GraphicsSettings>, dlss: Res<DlssAvailability>) {
    if settings.dlss_availability != *dlss { settings.dlss_availability = *dlss; }
    if *graphics != settings.current.graphics { *graphics = settings.current.graphics.clone(); }
}

fn apply_cameras(
    mut commands: Commands,
    graphics: Res<GraphicsSettings>,
    dlss: Res<DlssAvailability>,
    mut shadows: ResMut<DirectionalLightShadowMap>,
    cameras: Query<(Entity, Ref<GameGraphics>), With<Camera3d>>,
) {
    if graphics.is_changed() {
        shadows.size = if crate::flag("lowshadows") { 512 } else { graphics.shadows.shadow_size() };
    }
    for (entity, marker) in &cameras {
        if !graphics.is_changed() && !dlss.is_changed() && !marker.is_added() { continue; }
        let mut camera = commands.entity(entity);
        camera.remove::<(Fxaa, Smaa, ContrastAdaptiveSharpening)>();
        // SSAO and the WebGL depth prepass require single-sample depth. Spatial
        // AA choices stay compatible with both; Auto preserves the browser default.
        let browser_msaa = cfg!(target_arch = "wasm32") && graphics.anti_aliasing == AntiAliasing::Auto
            && !(crate::WEBGL2 && crate::depth_prepass());
        camera.insert(if browser_msaa { Msaa::Sample4 } else { Msaa::Off });
        match if dlss.active(graphics.dlss) { AntiAliasing::Off } else { graphics.anti_aliasing } {
            AntiAliasing::Auto if !cfg!(target_arch = "wasm32") => { camera.insert(Fxaa::default()); }
            AntiAliasing::Fxaa => { camera.insert(Fxaa::default()); }
            AntiAliasing::SmaaLow => { camera.insert(Smaa { preset: SmaaPreset::Low }); }
            AntiAliasing::SmaaHigh => { camera.insert(Smaa { preset: SmaaPreset::High }); }
            _ => {}
        }
        if graphics.sharpening > 0 {
            camera.insert(ContrastAdaptiveSharpening { sharpening_strength: graphics.sharpening.min(100) as f32 / 100., ..default() });
        }
        camera.insert(if crate::WEBGL2 || graphics.shadow_filter == ShadowFilter::Fast {
            ShadowFilteringMethod::Hardware2x2
        } else { ShadowFilteringMethod::Gaussian });
        #[cfg(not(target_arch = "wasm32"))]
        {
            use bevy::core_pipeline::prepass::NormalPrepass;
            use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel as AoQuality};
            if graphics.ambient_occlusion == Quality::Off || crate::flag("noao") {
                camera.remove::<(ScreenSpaceAmbientOcclusion, NormalPrepass)>();
            } else {
                camera.insert(ScreenSpaceAmbientOcclusion { quality_level: match graphics.ambient_occlusion {
                    Quality::Low => AoQuality::Low, Quality::High => AoQuality::High, _ => AoQuality::Medium,
                }, ..default() });
            }
        }
    }
}

#[cfg(test)]
mod tests;
