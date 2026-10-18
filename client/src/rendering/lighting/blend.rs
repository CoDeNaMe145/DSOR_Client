//! Cross-fade radiance from fixed preset directions using one map-wide shadow map.
//! A small shared view uniform also works with WebGL's one-directional-light limit.
//! All forward materials (including cutouts) keep their own PBR specialization.
use super::{linear, Bubble, Lighting, UNIT};
use std::{any::Any, sync::Arc};
use bevy::prelude::*;
use bevy::material::{key::ErasedMaterialPipelineKey, MaterialProperties};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialVertexShader, MaterialFragmentShader, MeshPipeline,
    MeshPipelineKey, MeshViewBindGroup, PreparedMaterial, PrepassPipeline, PrepassPipelineSpecializer,
    prepare_mesh_view_bind_groups};
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{*, binding_types::uniform_buffer};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::{ShaderDefVal, Source};

const MAX_TERMS: usize = 64;

pub(super) fn direction(value: Option<[f32; 3]>) -> Vec3 {
    Vec3::from_array(value.unwrap_or([-0.136, 0.721, -0.679])).normalize_or(Vec3::Y)
}

#[derive(Clone, Debug, PartialEq)]
struct LightTerm {
    dir: Vec3,
    front: Vec3,
    shadowed: Vec3,
    back: Vec3,
    /// BackLightFactor: the back light covers sat(offset - N.L), not only the
    /// surfaces facing away from the light.
    back_offset: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LightBlend(Vec<LightTerm>, f32);

impl Default for LightBlend {
    fn default() -> Self { Self::from(&Bubble::default()) }
}

impl From<&Bubble> for LightBlend {
    fn from(b: &Bubble) -> Self {
        Self::with_reference(b, b.sun_luminance())
    }
}

impl LightBlend {
    pub(super) fn with_reference(b: &Bubble, reference: f32) -> Self {
        let color = |c: [f32; 4]| {
            let c = linear([c[0], c[1], c[2]], b.light_intensity);
            Vec3::new(c.red, c.green, c.blue)
        };
        let front = color(b.light_color);
        // Shadows take the zone's whole sun, however dim the zone: the original
        // darkens the same way in every zone.
        // EVIDENCE: the running release261_192 client sends shadowDarkeningFactor
        //   0 in Kingshill's throne room and church zones as outdoors
        //   (lighting-work/captures/kingshill-{throne,church,garden}.txt).
        // FAILURE (S's earlier rule, strength x zone sun / outdoor sun, about
        //   0.17 in the throne room): Kingshill's palace_shadowobject, whose
        //   only job is to shade the hall, removed almost nothing; with shadows
        //   off the hall was only 1.6 brighter while its sun added 18.5.
        let _ = reference;
        let strength = b.light_shadow_intensity.clamp(0., 1.);
        // The back light is LightOppositeColor as is: unlike the sun's colour it
        // is not scaled by LightIntensity.
        // EVIDENCE: the running release261_192 client's globalBackLightColor equals
        //   the zone's LightOppositeColor while globalLightColor is LightColor x
        //   LightIntensity (Kingshill, intensity 1.4: 0.298, 0.3373, 0.3647 sent for
        //   both; lighting-work/captures/kingshill-garden.txt).
        let back = linear([b.light_opposite_color[0], b.light_opposite_color[1], b.light_opposite_color[2]], 1.0);
        Self(vec![LightTerm { dir: direction(b.dir), front,
            shadowed: front * strength,
            back: Vec3::new(back.red, back.green, back.blue), back_offset: b.back_light_factor }], reference)
    }
    pub(super) fn mix(&self, to: &Bubble, t: f32) -> Self {
        if t <= 0. { return self.clone(); }
        if t >= 1. { return Self::with_reference(to, self.1); }
        // Retarget from the actual mixture, including every outgoing direction.
        // Repeated doorway crossings never collapse it into a rotating light.
        let mut result = self.clone();
        for term in &mut result.0 {
            term.front *= 1. - t;
            term.shadowed *= 1. - t;
            term.back *= 1. - t;
        }
        let target = Self::with_reference(to, self.1).0.remove(0);
        if let Some(term) = result.0.iter_mut().find(|term| term.dir.distance_squared(target.dir) < 1e-10) {
            term.front += target.front * t;
            term.shadowed += target.shadowed * t;
            term.back += target.back * t;
            term.back_offset += (target.back_offset - term.back_offset) * t;
        } else {
            result.0.push(LightTerm { dir: target.dir, front: target.front * t,
                shadowed: target.shadowed * t, back: target.back * t, back_offset: target.back_offset });
        }
        // Discard only sub-visible residue after many interrupted transitions.
        result.0.retain(|term| term.front.max_element().max(term.back.max_element()) > 1e-7);
        result
    }

    pub(super) fn gpu(&self, factors: &Lighting) -> GpuAmbience {
        let mut value = GpuAmbience::default();
        let mut terms: Vec<_> = self.0.iter().collect();
        if terms.len() > MAX_TERMS {
            // A pathological sequence of >64 distinct zones within their fade
            // times: retain the visible contributions, never rotate directions.
            terms.sort_by(|a, b| b.front.max_element().max(b.back.max_element())
                .total_cmp(&a.front.max_element().max(a.back.max_element())));
            warn_once!("Ambience blend exceeds {MAX_TERMS} directions; dropping faintest contributions");
            terms.truncate(MAX_TERMS);
        }
        value.counts = UVec4::new(1, terms.len() as u32, 0, 0);
        let unit = std::f32::consts::PI * UNIT;
        for (out, term) in value.terms.iter_mut().zip(terms) {
            *out = GpuTerm { direction: term.dir.extend(0.),
                front: (term.front * unit * factors.sun).extend(0.),
                shadowed: (term.shadowed * unit * factors.sun).extend(0.),
                back: (term.back * unit * factors.back).extend(term.back_offset) };
        }
        value
    }
}

/// The share of the whole light (sun, ambient and back light) the original
/// keeps in full shadow: its sun pass multiplies the sum by the screen-space
/// shadow buffer's x. The shadow pass writes shadowDarkeningFactor there where
/// shadowed (the running client sends 0 at LightShadowIntensity 1), and the two
/// blur passes (pe_blurshadowhori / vert) each output 1 - 0.8 x the kernel's
/// 0.242 + 0.399 + 0.242 x (1 - x): a fully shadowed 0 becomes
/// 1 - 0.7063^2 = 0.501.
/// EVIDENCE: shaders_sm30 shadow pass (cmp oC0.x, ..., shadowDarkeningFactor,
///   shadowMap), FilterKernel ps (c8 = (1, 0.8), add oC0, -r0, 1) and the
///   captured FilterKernel weights; the sun pass `mul r0.xyz, r0, r2.x`.
///   Kingshill's garden, same stone: shadow / sunlit 0.57 in the original.
/// UNVERIFIED: how LightShadowIntensity other than 1 (0.25..3 in the data) maps
///   to shadowDarkeningFactor; taken as 1 - intensity, clamped.
pub(super) fn shadow_floor(shadow_intensity: f32) -> f32 {
    const BLURRED: f32 = 0.8 * (0.242 + 0.3989 + 0.242);
    let darkening = (1.0 - shadow_intensity).clamp(0.0, 1.0);
    1.0 - BLURRED * BLURRED * (1.0 - darkening)
}

#[derive(Clone, Copy, Default, ShaderType)]
struct GpuTerm { direction: Vec4, front: Vec4, shadowed: Vec4, back: Vec4 }

#[derive(Resource, Clone, ExtractResource, ShaderType)]
pub(super) struct GpuAmbience {
    counts: UVec4,
    terms: [GpuTerm; MAX_TERMS],
    /// Linear rgb; w 1 enables height fog (0 in the inactive buffer).
    height_fog_color: Vec4,
    /// x FogNearHeight, y FogFarHeight.
    fog_heights: Vec4,
    /// Gamma rgb: RimLightColor x RimLightIntensity, Nebula's rimLightColor
    /// (0 in the inactive buffer: no rim).
    rim_color: Vec4,
    /// View-space direction (xyz) and exponent (w), Nebula's rimLightDirAndSpec.
    rim_dir_spec: Vec4,
    /// Gamma rgb FogColor; w 1 enables Nebula's distance fog (0 in the inactive
    /// buffer: Bevy's fog stays).
    fog_color: Vec4,
    /// x FogNearDist, y FogFarDist (camera depth), z the most fog (FogIntensity).
    fog_dist: Vec4,
    /// x 1 sums the sun, back light and ambient in gamma space as the 2018/261
    /// sun pass does; y the terms' unit (PI x UNIT); z the share of the whole
    /// light kept in full shadow; w 1 applies it (the default; DSOR_FLAGS=noshadowfloor).
    gamma_light: Vec4,
    /// The ambient light in that gamma sum (gamma rgb); w 1 squares the
    /// ambient occlusion in gamma (DSOR_FLAGS=aogamma), 2 the same after x 1.3 clamped to 1
    /// (ao13), 0 applies it once in linear, 3 leaves it out, 4 the original's
    /// estimate squared (the default; lighting/pe_ssao.wgsl).
    ambient_gamma: Vec4,
    /// x 1: surfaces write gamma colours to the scene target (ambience.wgsl
    /// encode_scene), blending in gamma as the original; 0 in the inactive buffer.
    output: Vec4,
}
// CONTRACT: the field order above is ambience.wgsl's `Ambience` struct order
// (the GPU reads by position); `uniform_fields_follow_the_wgsl_order` checks it.

impl Default for GpuAmbience {
    fn default() -> Self {
        Self { counts: UVec4::ZERO, terms: [GpuTerm::default(); MAX_TERMS], height_fog_color: Vec4::ZERO, fog_heights: Vec4::ZERO,
            rim_color: Vec4::ZERO, rim_dir_spec: Vec4::ZERO, fog_color: Vec4::ZERO, fog_dist: Vec4::ZERO,
            gamma_light: Vec4::ZERO, ambient_gamma: Vec4::ZERO, output: Vec4::ZERO }
    }
}

impl GpuAmbience {
    /// The bubble's height fog. Like the 2018 shaders, it has no strength of its
    /// own: the colour and the two heights are the whole of it.
    pub(super) fn with_height_fog(mut self, b: &Bubble) -> Self {
        let c = linear([b.height_fog_color[0], b.height_fog_color[1], b.height_fog_color[2]], 1.0);
        self.height_fog_color = Vec4::new(c.red, c.green, c.blue, 1.0);
        self.fog_heights = Vec4::new(b.fog_near_height, b.fog_far_height, 0.0, 0.0);
        self
    }

    /// Nebula's distance fog: clear = clamp((far - depth) / (far - near),
    /// fogColor.w, 1), the colour mixed over the lit one in gamma space, with
    /// depth along the camera's view.
    /// EVIDENCE: shaders_sm30's vertex shaders pass the view-space position
    ///   (modelView x position) and the pixel shaders compute
    ///   (fogDistances.y - z) x 1/(fogDistances.y - fogDistances.x) (preshader),
    ///   max fogColor.w, min 1, then lrp(fogColor, colour). The running
    ///   release261_192 client sends fogColor.w = 1 - FogIntensity (0 in
    ///   Kingshill's global zone, 0.2 in its gate zone at intensity 0.8) and
    ///   fogDistances = (FogNearDist, FogFarDist, ...): measured from the camera,
    ///   not shifted (lighting-work/captures/kingshill-*.txt).
    /// The panel's Fog x scales the most fog, Fog range x the distances.
    pub(super) fn with_fog(mut self, b: &Bubble, factors: &Lighting) -> Self {
        self.fog_color = Vec4::new(b.fog_color[0], b.fog_color[1], b.fog_color[2], 1.0);
        let near = b.fog_near_dist.max(0.0) * factors.fog_range;
        let far = (b.fog_far_dist * factors.fog_range).max(near + 0.1);
        self.fog_dist = Vec4::new(near, far, (b.fog_intensity * factors.fog).clamp(0.0, 1.0), 0.0);
        self
    }

    /// The light summed in gamma space, as the 2018/261 sun pass does (D3D9, no
    /// sRGB): sun x sat(N.L) + ambient + back x sat(BLF - N.L), the albedo
    /// multiplied by that sum. `ambient` is the ambient light in the terms'
    /// linear scale (before their PI x UNIT).
    /// EVIDENCE: the running release261_192 client sends globalLightColor,
    ///   globalAmbientLightColor and globalBackLightColor as gamma colours (the
    ///   zone's values, lighting-work/captures/kingshill-*.txt); a linear sum of
    ///   the same colours over-saturates tinted ambients (the church's
    ///   (0.18, 0.25, 0.35): blue 4x red in linear, 2x in gamma).
    pub(super) fn with_gamma_light(mut self, switches: &super::RenderSwitches, ambient: Vec3, shadow_intensity: f32) -> Self {
        let floor = if switches.shadow_floor { 1.0 } else { 0.0 };
        self.gamma_light = Vec4::new(if switches.gamma_light { 1.0 } else { 0.0 }, std::f32::consts::PI * UNIT, shadow_floor(shadow_intensity), floor);
        let ao = match switches.ao {
            super::AoMode::Linear => 0.0, super::AoMode::Squared => 1.0, super::AoMode::Clamped => 2.0, super::AoMode::Off => 3.0,
            super::AoMode::Original => 4.0,
        };
        self.ambient_gamma = ambient.max(Vec3::ZERO).powf(1.0 / 2.2).extend(ao);
        self.output = Vec4::new(if switches.gamma_scene { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0);
        self
    }

    /// Leaves the characters' rim light out (the Lighting Menu's switch, or
    /// DSOR_FLAGS=norim), for comparisons.
    pub(super) fn with_rim_enabled(mut self, enabled: bool) -> Self {
        if !enabled { self.rim_color = Vec4::ZERO; }
        self
    }

    /// The characters' rim light. The colour is the bubble's; the engine sets the
    /// direction and exponent itself, the same in every map.
    /// EVIDENCE: the "character" and "characterfalloff" ps_3_0 (shaders_sm30) add
    ///   rimLightColor (c8) x pow(max(dot(n_view, c9.xyz), 0), c9.w) to the lit
    ///   colour, before the hit colour and the fogs. Read from the running
    ///   release261_192 client (D3D9 SetPixelShaderConstantF, Wild Forest):
    ///   c8 = (0.1137, 0.3314, 0.3451, 0.5), its RimLightColor x RimLightIntensity
    ///   0.5; c9 = (0.6963, 0.6963, 0.1741, 4), (4, 4, 1) normalised.
    pub(super) fn with_rim_light(mut self, b: &Bubble) -> Self {
        let c = b.rim_light_color;
        self.rim_color = Vec4::new(c[0], c[1], c[2], 1.0) * b.rim_light_intensity;
        self.rim_dir_spec = Vec3::new(4.0, 4.0, 1.0).normalize().extend(4.0);
        self
    }
}

#[derive(Component, Clone, ExtractComponent)]
pub(super) struct AmbienceView;

pub(super) struct BlendPlugin;
impl Plugin for BlendPlugin {
    fn build(&self, app: &mut App) {
        bevy::shader::load_shader_library!(app, "ambience.wgsl");
        app.init_resource::<GpuAmbience>()
            .add_plugins((ExtractResourcePlugin::<GpuAmbience>::default(), ExtractComponentPlugin::<AmbienceView>::default()))
            .add_systems(Update, prepare_shaders);
        let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
        render.init_resource::<SpecializedMeshPipelines<BlendPipeline>>()
            .init_resource::<SpecializedMeshPipelines<ShadowFacesPipeline>>()
            .init_resource::<BlendBuffers>()
            .add_systems(Render, adapt_materials.after(RenderSystems::PrepareAssets).before(RenderSystems::Specialize))
            .add_systems(Render, prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups).after(prepare_mesh_view_bind_groups));
    }
}

fn blend_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new("ambience blend", &BindGroupLayoutEntries::single(
        ShaderStages::FRAGMENT, uniform_buffer::<GpuAmbience>(false)))
}

/// The client doesn't use view environment probes/clustered decals: its cube
/// maps belong to Nebula materials. Group 1 is free, keeping the four bind-group
/// limit on WebGL. Shadow and prepass pipelines retain Bevy's empty group 1.
struct BlendPipeline { pipeline: MaterialPipeline, properties: Arc<MaterialProperties> }
impl SpecializedMeshPipeline for BlendPipeline {
    type Key = ErasedMaterialPipelineKey;
    fn specialize(&self, key: Self::Key, layout: &MeshVertexBufferLayoutRef)
        -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mesh_key: MeshPipelineKey = key.mesh_key.downcast();
        let mut desc = self.pipeline.mesh_pipeline.specialize(mesh_key, layout)?;
        assert!(desc.layout[1].entries.is_empty(), "Ambience view binding conflicts with view environment probes");
        desc.layout[1] = blend_layout();
        desc.layout.insert(3, self.properties.material_layout.as_ref().unwrap().clone());
        let material_group = ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3);
        desc.vertex.shader_defs.push(material_group.clone());
        if let Some(shader) = self.properties.get_shader(MaterialVertexShader) { desc.vertex.shader = shader; }
        if let Some(fragment) = &mut desc.fragment {
            fragment.shader_defs.extend([material_group, "DSOR_AMBIENCE".into()]);
            if let Some(shader) = self.properties.get_shader(MaterialFragmentShader) { fragment.shader = shader; }
        }
        if let Some(specialize) = self.properties.user_specialize {
            specialize(&self.pipeline as &dyn Any, &mut desc, layout, key)?;
        }
        if self.properties.bindless {
            desc.vertex.shader_defs.push("BINDLESS".into());
            if let Some(fragment) = &mut desc.fragment { fragment.shader_defs.push("BINDLESS".into()); }
        }
        Ok(desc)
    }
}

fn specialize(world: &mut World, key: ErasedMaterialPipelineKey, layout: &MeshVertexBufferLayoutRef,
    properties: &Arc<MaterialProperties>) -> Result<CachedRenderPipelineId, SpecializedMeshPipelineError> {
    world.resource_scope(|world, mut cache: Mut<SpecializedMeshPipelines<BlendPipeline>>| {
        let specializer = BlendPipeline { pipeline: MaterialPipeline { mesh_pipeline: world.resource::<MeshPipeline>().clone() },
            properties: properties.clone() };
        cache.specialize(world.resource::<PipelineCache>(), &specializer, key, layout)
    })
}

/// The sun's shadow map draws both faces of every caster, as the original's
/// shadow pass must: Kingshill's `t011_kingshill/palace_shadowobject` is a
/// 28 x 32 m plane over the throne room whose 112 faces all point down. The game
/// camera above only sees its back, so it is invisible, and it exists only to
/// shade the hall from the sun above. Culled as Bevy culls back faces in shadow
/// views too, it cast nothing and the hall's floor was sunlit (in-game throne
/// room 57.0 vs the original's 46.7, a large shadow band missing).
/// Only the directional (sun) shadow views, marked UNCLIPPED_DEPTH_ORTHO; the
/// camera's own depth prepass keeps the material's culling.
struct ShadowFacesPipeline { inner: PrepassPipelineSpecializer }
impl SpecializedMeshPipeline for ShadowFacesPipeline {
    type Key = ErasedMaterialPipelineKey;
    fn specialize(&self, key: Self::Key, layout: &MeshVertexBufferLayoutRef)
        -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mesh_key: MeshPipelineKey = key.mesh_key.downcast();
        let mut desc = self.inner.specialize(key, layout)?;
        if mesh_key.contains(MeshPipelineKey::UNCLIPPED_DEPTH_ORTHO) { desc.primitive.cull_mode = None; }
        Ok(desc)
    }
}

fn prepass_specialize(world: &mut World, key: ErasedMaterialPipelineKey, layout: &MeshVertexBufferLayoutRef,
    properties: &Arc<MaterialProperties>) -> Result<CachedRenderPipelineId, SpecializedMeshPipelineError> {
    world.resource_scope(|world, mut cache: Mut<SpecializedMeshPipelines<ShadowFacesPipeline>>| {
        let specializer = ShadowFacesPipeline { inner: PrepassPipelineSpecializer {
            pipeline: world.resource::<PrepassPipeline>().clone(), properties: properties.clone() } };
        cache.specialize(world.resource::<PipelineCache>(), &specializer, key, layout)
    })
}

fn adapt_materials(mut materials: ResMut<ErasedRenderAssets<PreparedMaterial>>) {
    for (_, material) in materials.iter_mut() {
        // Newly prepared properties are unique until specialization shares them.
        if let Some(properties) = Arc::get_mut(&mut material.properties) {
            if properties.base_specialize.is_some() { properties.base_specialize = Some(specialize); }
            if properties.prepass_specialize.is_some() && !crate::flag("shadowcull") {
                properties.prepass_specialize = Some(prepass_specialize);
            }
        }
    }
}

#[derive(Resource, Default)]
struct BlendBuffers { active: UniformBuffer<GpuAmbience>, inactive: UniformBuffer<GpuAmbience>, groups: Option<(BindGroup, BindGroup)> }

fn prepare_bind_groups(device: Res<RenderDevice>, queue: Res<RenderQueue>, cache: Res<PipelineCache>,
    ambience: Res<GpuAmbience>, mut buffers: ResMut<BlendBuffers>, mut views: Query<(&mut MeshViewBindGroup, Has<AmbienceView>)>) {
    if ambience.is_changed() || buffers.groups.is_none() {
        buffers.active.set(ambience.clone());
        buffers.active.write_buffer(&device, &queue);
    }
    if buffers.groups.is_none() {
        buffers.inactive.write_buffer(&device, &queue);
        let layout = cache.get_bind_group_layout(&blend_layout());
        buffers.groups = Some((device.create_bind_group("ambience active", &layout, &BindGroupEntries::single(&buffers.active)),
            device.create_bind_group("ambience inactive", &layout, &BindGroupEntries::single(&buffers.inactive))));
    }
    let (active, inactive) = buffers.groups.as_ref().unwrap();
    for (mut view, enabled) in &mut views { view.binding_array = if enabled { active.clone() } else { inactive.clone() }; }
}

/// Reuse Bevy's current BRDF for each fixed direction, including normal maps,
/// specular and material extensions. Only the contribution blend is custom.
fn light_function(source: &str) -> Option<String> {
    let start = source.find("fn directional_light(")?;
    let end = source[start..].find("\n}")? + start + 2;
    let original = &source[start..end];
    let start_color = original.find("    var texture_sample = 1f;")?;
    let mut function = original[..start_color].to_owned();
    function = function.replace("fn directional_light(", "fn ambience_directional_light(")
        .replace("    light_id: u32,", "    direction: vec3<f32>,\n    light_color: vec3<f32>,")
        .replace("    let light = &view_bindings::lights.directional_lights[light_id];", "")
        .replace("let L = (*light).direction_to_light.xyz;", "let L = direction;");
    if function.contains("(*light)") || !function.contains("let L = direction;") { return None; }
    function.push_str("    return color * light_color;\n}\n");
    Some(format!("{source}\n{function}"))
}

fn blend_function(source: &str) -> Option<String> {
    let start = source.find("        var light_contrib = lighting::directional_light(")?;
    let end_anchor = "        direct_light += light_contrib * shadow;";
    let end = source[start..].find(end_anchor)? + start + end_anchor.len();
    let original = &source[start..end];
    let mut patched = source.replace(original, &format!("#ifdef DSOR_AMBIENCE\n        if ambience.counts.x != 0u && i == 0u {{\n            direct_light += apply_ambience(&lighting_input, enable_diffuse, shadow, dsor_occlusion, dsor_point_gamma);\n        }} else {{\n{original}\n        }}\n#else\n{original}\n#endif"));
    // Transmission uses the same radiance blend on the opposite material lobe.
    let start = patched.find("        let transmitted_light_contrib =\n            lighting::directional_light(")?;
    let anchor = "        transmitted_light += transmitted_light_contrib * transmitted_shadow;";
    let end = patched[start..].find(anchor)? + start + anchor.len();
    let original = patched[start..end].to_owned();
    patched = patched.replace(&original, &format!("#ifdef DSOR_AMBIENCE\n        if ambience.counts.x != 0u && i == 0u {{\n            transmitted_light += apply_ambience(&transmissive_lighting_input, enable_diffuse, transmitted_shadow, dsor_occlusion, vec3(0.0));\n        }} else {{\n{original}\n        }}\n#else\n{original}\n#endif"));
    // With the gamma light sum, point lights join it as the original's point
    // light pass adds into its light buffer (ambience.wgsl nebula_point_light).
    let points = "    // Point lights (direct)\n";
    if !patched.contains(points) { return None; }
    // The original's AO estimate (AoMode::Original) as its pass wrote it: Bevy's
    // diffuse_occlusion is its multi-bounce approximation of the SSAO texture.
    let raw_ao = "#ifdef SCREEN_SPACE_AMBIENT_OCCLUSION\n    if ambience.ambient_gamma.w > 3.5 {\n        dsor_occlusion = vec3(textureLoad(view_bindings::screen_space_ambient_occlusion_texture, vec2<i32>(in.frag_coord.xy), 0i).r);\n    }\n#endif\n";
    patched = patched.replace(points, &format!("#ifdef DSOR_AMBIENCE\n    var dsor_point_gamma = vec3<f32>(0.0);\n    var dsor_occlusion = diffuse_occlusion;\n{raw_ao}#endif\n{points}"));
    let point = "        let light_contrib = lighting::point_light(light_id, &lighting_input, enable_diffuse, true);\n        direct_light += light_contrib * shadow;";
    if patched.matches(point).count() != 1 { return None; }
    patched = patched.replace(point, &format!("#ifdef DSOR_AMBIENCE\n        if ambience.gamma_light.x != 0.0 {{\n            if enable_diffuse {{ dsor_point_gamma += nebula_point_light(light_id, &lighting_input); }}\n        }} else {{\n{point}\n        }}\n#else\n{point}\n#endif"));
    let directional = "    // directional lights (direct)\n    let n_directional_lights = view_bindings::lights.n_directional_lights;\n";
    if patched.matches(directional).count() != 1 { return None; }
    patched = patched.replace(directional, &format!("{directional}#ifdef DSOR_AMBIENCE\n    if ambience.gamma_light.x != 0.0 && (ambience.counts.x == 0u || n_directional_lights == 0u) {{\n        direct_light += apply_points_gamma(&lighting_input, dsor_point_gamma);\n    }}\n#endif\n"));
    // Nebula's distance fog replaces Bevy's on views with the level's look.
    let fog_call = "        output_color = apply_fog(\n            view_bindings::fog,\n            output_color,\n            pbr_input.world_position.xyz,\n            view_bindings::view.world_position.xyz,\n            pbr_input.frag_coord.xy,\n        );";
    if !patched.contains(fog_call) { return None; }
    patched = patched.replace(fog_call, &format!("#ifdef DSOR_AMBIENCE\n        if ambience.fog_color.w != 0.0 {{\n            output_color = apply_nebula_fog(output_color, pbr_input.world_position.xyz);\n        }} else {{\n{fog_call}\n        }}\n#else\n{fog_call}\n#endif"));
    // Height fog follows distance fog on the same fogged materials, as the 2018
    // pixel shaders lerp heightFogColor over the distance-fogged colour.
    let fog_end = "            pbr_input.frag_coord.xy,\n        );\n        }\n#else\n        output_color = apply_fog(\n            view_bindings::fog,\n            output_color,\n            pbr_input.world_position.xyz,\n            view_bindings::view.world_position.xyz,\n            pbr_input.frag_coord.xy,\n        );\n#endif\n    }\n#endif  // DISTANCE_FOG";
    if !patched.contains(fog_end) { return None; }
    patched = patched.replace(fog_end, &format!("{fog_end_body}\n#ifdef DSOR_AMBIENCE\n        output_color = apply_height_fog(output_color, pbr_input.world_position.xyz);\n#endif\n    }}\n#endif  // DISTANCE_FOG", fog_end_body = &fog_end[..fog_end.len() - "\n    }\n#endif  // DISTANCE_FOG".len()]));
    // Every material's last step (lit, unlit, decals, particles, our surfaces):
    // the gamma scene target (ambience.wgsl encode_scene), after the fogs and
    // before the premultiplication, so blending mixes the shown colours.
    let premultiply = "#ifdef PREMULTIPLY_ALPHA\n    output_color = premultiply_alpha(pbr_input.material.flags, output_color);\n#endif\n    return output_color;";
    if patched.matches(premultiply).count() != 1 { return None; }
    patched = patched.replace(premultiply, &format!("#ifdef DSOR_AMBIENCE\n    output_color = encode_scene(output_color);\n#endif\n{premultiply}"));
    Some(format!("#ifdef DSOR_AMBIENCE\n#import dsor::ambience::{{ambience, apply_ambience, apply_height_fog, apply_nebula_fog, apply_points_gamma, encode_scene, nebula_point_light}}\n#endif\n{patched}"))
}

fn prepare_shaders(mut shaders: ResMut<Assets<Shader>>, mut done: Local<bool>) {
    if *done { return; }
    let find = |name: &str| shaders.iter().find_map(|(id, s)| {
        (s.import_path == bevy::shader::ShaderImport::Custom(name.into())).then_some(id)
    });
    let (Some(lighting), Some(pbr)) = (find("bevy_pbr::lighting"), find("bevy_pbr::pbr_functions")) else { return };
    for (id, patch) in [(lighting, light_function as fn(&str) -> Option<String>), (pbr, blend_function)] {
        let mut shader = shaders.get_mut(id).unwrap();
        let Source::Wgsl(source) = &shader.source else { panic!("Ambience requires Bevy's WGSL lighting shaders") };
        let patched = Shader::from_wgsl(patch(source).expect("Bevy lighting shader changed; update ambience blend adapter"), shader.path.clone());
        // Refresh the dependency metadata too: pbr_functions now imports our
        // module. Preserve the original shader's definitions and strong handles.
        shader.source = patched.source;
        shader.imports = patched.imports;
    }
    *done = true;
    info!("ambience: fixed shadow projection and radiance cross-fades");
}

#[cfg(test)]
#[path = "blend_tests.rs"]
mod tests;
