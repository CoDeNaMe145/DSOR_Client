//! Native DLSS setup and camera lifecycle. The browser never compiles this module.
use super::*;
use bevy::anti_alias::dlss::{Dlss, DlssPerfQualityMode, DlssProjectId, DlssSuperResolutionSupported};
use bevy::camera::Hdr;
use bevy::camera::CameraMainTextureUsages;
use bevy::core_pipeline::prepass::MotionVectorPrepass;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::settings::Backends;
use bevy::render::render_resource::TextureUsages;
use bevy::shader::Source;

#[derive(Resource, Default)]
pub(super) struct ShaderCompatibility { ready: u8, failed: bool }

pub(super) fn install_cleanup(app: &mut App) {
    use bevy::render::{Render, RenderApp, RenderSystems};
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(Render, cleanup_motion_vectors.in_set(RenderSystems::Prepare));
    }
}

// Bevy 0.19 leaves this pipeline on the render camera after motion vectors
// are disabled. Its attachment layout would reference the removed texture.
fn cleanup_motion_vectors(
    mut commands: Commands,
    views: Query<Entity, (Without<MotionVectorPrepass>, With<bevy::core_pipeline::prepass::BackgroundMotionVectorsPipelineId>)>,
) {
    use bevy::core_pipeline::prepass::{BackgroundMotionVectorsPipelineId, BackgroundMotionVectorsBindGroup};
    for entity in &views { commands.entity(entity).remove::<(BackgroundMotionVectorsPipelineId, BackgroundMotionVectorsBindGroup)>(); }
}

pub(super) fn configure(app: &mut App) {
    // Stable application ID, generated for DSOR rather than copied from an example.
    app.insert_resource(DlssProjectId("fd5c983a-3316-4a0f-80e5-65af0f8990e2".parse().unwrap()));
    // Enable the target-specific anti-alias backend directly. DefaultPlugins
    // installs DlssPlugin through AntiAliasPlugin, but its early init is ours.
    app.add_plugins(bevy::anti_alias::dlss::DlssInitPlugin);
}

fn supported(vendor: u32, vulkan: bool, runtime_supported: bool) -> bool {
    vendor == 0x10de && vulkan && runtime_supported
}

pub(super) fn detect(app: &mut App) {
    let available = app.world().get_resource::<RenderAdapterInfo>().is_some_and(|adapter|
        supported(adapter.vendor, Backends::from(adapter.backend) == Backends::VULKAN,
            app.world().contains_resource::<DlssSuperResolutionSupported>()));
    if available { app.init_resource::<ShaderCompatibility>(); }
    info!("DLSS / DLAA: {}", if available { "available" } else { "unavailable (requires NVIDIA RTX, Vulkan and the DLSS runtime)" });
}

// Bevy 0.19 builds these textures/clusters at output resolution. Convert only
// projection and cluster coordinates; texture UVs must still address the full
// allocation. The ratios are 1 for DLAA, ordinary cameras and shadow views.
const CLUSTER_FROM: &str = "(frag_coord - bindings::view.viewport.xy) * bindings::lights.cluster_factors.xy";
const CLUSTER_TO: &str = "(frag_coord - bindings::view.main_pass_viewport.xy) * (bindings::view.viewport.zw / bindings::view.main_pass_viewport.zw) * bindings::lights.cluster_factors.xy";
const AO_PROJECT_FROM: &str = "let clip_xy = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - 2.0 * uv.y);";
const AO_PROJECT_TO: &str = "let projected_uv = uv * view.viewport.zw / view.main_pass_viewport.zw;\n    let clip_xy = vec2<f32>(projected_uv.x * 2.0 - 1.0, 1.0 - 2.0 * projected_uv.y);";
const AO_SAMPLE_FROM: &str = "let sample_mul = vec2<f32>(omega.x, -omega.y) * sample_scale;";
const AO_SAMPLE_TO: &str = "let sample_mul = vec2<f32>(omega.x, -omega.y) * sample_scale * (view.main_pass_viewport.zw / view.viewport.zw);";

fn patched(source: &str, replacements: &[(&str, &str)]) -> Option<String> {
    let mut source = source.to_owned();
    for (from, to) in replacements {
        if source.matches(from).count() != 1 { return None; }
        source = source.replace(from, to);
    }
    Some(source)
}

pub(super) fn prepare_shaders(
    mut shaders: ResMut<Assets<Shader>>,
    compatibility: Option<ResMut<ShaderCompatibility>>,
    mut availability: ResMut<DlssAvailability>,
) {
    let Some(mut compatibility) = compatibility else { return };
    if compatibility.ready == 3 || compatibility.failed { return; }
    for (bit, path, replacements) in [
        (1, "bevy_pbr/render/clustered_forward.wgsl", vec![(CLUSTER_FROM, CLUSTER_TO)]),
        (2, "bevy_pbr/ssao/ssao.wgsl", vec![(AO_PROJECT_FROM, AO_PROJECT_TO), (AO_SAMPLE_FROM, AO_SAMPLE_TO)]),
    ] {
        if compatibility.ready & bit != 0 { continue; }
        let Some(id) = shaders.iter().find_map(|(id, shader)| shader.path.ends_with(path).then_some(id)) else { continue };
        let Some(mut shader) = shaders.get_mut(id) else { continue };
        let Source::Wgsl(source) = &shader.source else { continue };
        let Some(source) = patched(source, &replacements) else {
            error!("DLSS disabled: Bevy shader {path} changed; review the resolution compatibility fix");
            compatibility.failed = true;
            return;
        };
        shader.source = Source::Wgsl(source.into());
        compatibility.ready |= bit;
    }
    if compatibility.ready == 3 { *availability = DlssAvailability::Available; }
}

impl DlssMode {
    fn quality(self) -> DlssPerfQualityMode { match self {
        Self::Off | Self::Dlaa => DlssPerfQualityMode::Dlaa,
        Self::Quality => DlssPerfQualityMode::Quality, Self::Balanced => DlssPerfQualityMode::Balanced,
        Self::Performance => DlssPerfQualityMode::Performance,
    } }
}

#[derive(Component)]
pub(super) struct DlssCameraState {
    added_hdr: bool,
    texture_usages: TextureUsages,
    retiring: bool,
    position: Vec3,
    rotation: Quat,
}

pub(super) fn apply(
    mut commands: Commands,
    graphics: Res<GraphicsSettings>,
    availability: Res<DlssAvailability>,
    map: Option<Res<crate::world::map::CurrentMap>>,
    mut cameras: Query<(Entity, &GlobalTransform, Has<Hdr>, &mut CameraMainTextureUsages, Option<&mut Dlss>, Option<&mut DlssCameraState>, Has<crate::rendering::nebula_bloom::NebulaPostProcess>), (With<GameGraphics>, With<Camera3d>)>,
) {
    let active = availability.active(graphics.dlss);
    for (entity, transform, hdr, mut usages, dlss, state, nebula_post) in &mut cameras {
        let mut camera = commands.entity(entity);
        if active {
            let position = transform.translation();
            let rotation = transform.rotation();
            if let Some(mut dlss) = dlss {
                let quality = graphics.dlss.quality();
                if dlss.perf_quality_mode != quality {
                    dlss.perf_quality_mode = quality;
                    dlss.reset = true;
                }
                if let Some(mut state) = state {
                    // Ordinary movement accumulates history; map changes and camera
                    // cuts must discard it. Bevy also recreates history on resize.
                    if position.distance_squared(state.position) > 25.0
                        || rotation.angle_between(state.rotation) > 0.5
                        || map.as_ref().is_some_and(|map| map.is_changed()) {
                        dlss.reset = true;
                    }
                    state.position = position;
                    state.rotation = rotation;
                }
            } else {
                let added_hdr = state.as_ref().map_or(!hdr, |state| state.added_hdr);
                let texture_usages = state.as_ref().map_or(usages.0, |state| state.texture_usages);
                // The overlay must share the same main texture key. Set this in
                // the main world too, where its camera can copy the usage flags.
                usages.0 |= TextureUsages::STORAGE_BINDING;
                camera.insert((Dlss::<bevy::anti_alias::dlss::DlssSuperResolutionFeature> {
                    perf_quality_mode: graphics.dlss.quality(), reset: true, ..default()
                }, DlssCameraState { added_hdr, texture_usages, retiring: false, position, rotation }));
            }
        } else if let Some(mut state) = state {
            if !state.retiring {
                camera.remove::<(Dlss, TemporalJitter, MipBias, MotionVectorPrepass)>();
                // extract_dlss only queries HDR cameras. Leave Hdr for one extraction
                // so it removes its render-world context and resolution override.
                state.retiring = true;
            } else {
                // The level look can arrive after DLSS starts. Its composition
                // needs HDR independently of DLSS to retain the 0..2 range.
                if state.added_hdr && !nebula_post { camera.remove::<Hdr>(); }
                usages.0 = state.texture_usages;
                camera.remove::<DlssCameraState>();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_nvidia_vulkan_and_runtime_confirmation() {
        assert!(supported(0x10de, true, true));
        assert!(!supported(0x1002, true, true));
        assert!(!supported(0x10de, false, true));
        assert!(!supported(0x10de, true, false));
    }

    #[test]
    fn modes_and_disable_clean_up_temporal_state() {
        let mut app = App::new();
        app.init_resource::<GraphicsSettings>().init_resource::<DirectionalLightShadowMap>()
            .insert_resource(DlssAvailability::Available).add_systems(Update, (apply_cameras, apply).chain());
        let camera = app.world_mut().spawn((Camera3d::default(), GameGraphics)).id();
        let original_usages = app.world().get::<CameraMainTextureUsages>(camera).unwrap().0;
        app.world_mut().resource_mut::<GraphicsSettings>().dlss = DlssMode::Dlaa;
        app.update();
        assert_eq!(app.world().get::<Dlss>(camera).unwrap().perf_quality_mode, DlssPerfQualityMode::Dlaa);
        assert!(app.world().get::<Fxaa>(camera).is_none());
        assert!(app.world().get::<MotionVectorPrepass>(camera).is_some());
        assert!(app.world().get::<CameraMainTextureUsages>(camera).unwrap().0.contains(TextureUsages::STORAGE_BINDING));
        app.world_mut().get_mut::<Dlss>(camera).unwrap().reset = false;
        app.world_mut().resource_mut::<GraphicsSettings>().sharpening = 35;
        app.update();
        assert!(!app.world().get::<Dlss>(camera).unwrap().reset, "unrelated settings retain temporal history");
        app.world_mut().entity_mut(camera).insert(GlobalTransform::from_translation(Vec3::X * 10.0));
        app.update();
        assert!(app.world().get::<Dlss>(camera).unwrap().reset, "camera cuts discard history");
        app.world_mut().resource_mut::<GraphicsSettings>().dlss = DlssMode::Quality;
        app.update();
        assert_eq!(app.world().get::<Dlss>(camera).unwrap().perf_quality_mode, DlssPerfQualityMode::Quality);
        app.world_mut().resource_mut::<GraphicsSettings>().dlss = DlssMode::Off;
        app.update();
        assert!(app.world().get::<Dlss>(camera).is_none());
        assert!(app.world().get::<TemporalJitter>(camera).is_none());
        assert!(app.world().get::<MotionVectorPrepass>(camera).is_none());
        assert!(app.world().get::<Hdr>(camera).is_some());
        assert!(app.world().get::<Fxaa>(camera).is_some());
        app.update();
        assert!(app.world().get::<Hdr>(camera).is_none());
        assert!(app.world().get::<DlssCameraState>(camera).is_none());
        assert_eq!(app.world().get::<CameraMainTextureUsages>(camera).unwrap().0, original_usages);
    }

    #[test]
    fn disabling_dlss_preserves_hdr_when_the_level_look_arrives_later() {
        use crate::rendering::nebula_bloom::NebulaPostProcess;
        let mut app = App::new();
        app.insert_resource(GraphicsSettings { dlss: DlssMode::Dlaa, ..default() })
            .insert_resource(DlssAvailability::Available).add_systems(Update, apply);
        let camera = app.world_mut().spawn((Camera3d::default(), GameGraphics)).id();
        app.update();
        assert!(app.world().get::<DlssCameraState>(camera).unwrap().added_hdr);
        // Map lighting may become available after the upscaler has started.
        app.world_mut().entity_mut(camera).insert(NebulaPostProcess {
            threshold: 1., scale: 0., color: Vec4::ONE, saturation: 1., balance: Vec4::ONE,
            vignette_color: Vec4::ZERO, vignette_intensity: 0., vignette_size: 2., death_fade: 0.,
        });
        app.world_mut().resource_mut::<GraphicsSettings>().dlss = DlssMode::Off;
        app.update();
        app.update();
        assert!(app.world().get::<Dlss>(camera).is_none());
        assert!(app.world().get::<DlssCameraState>(camera).is_none());
        assert!(app.world().get::<Hdr>(camera).is_some(), "composition must retain highlights after DLSS cleanup");
    }
}
