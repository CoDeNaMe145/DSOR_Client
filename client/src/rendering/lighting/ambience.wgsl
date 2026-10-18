#define_import_path dsor::ambience

#import bevy_pbr::lighting::{LightingInput, ambience_directional_light, LAYER_BASE}
#import bevy_pbr::{mesh_view_bindings, mesh_view_types}
#import bevy_render::color_operations::{linear_rgb_to_srgb, srgb_to_linear_rgb}

struct AmbienceTerm {
    direction: vec4<f32>,
    front: vec4<f32>,
    shadowed: vec4<f32>,
    back: vec4<f32>,
}
struct Ambience {
    counts: vec4<u32>,
    terms: array<AmbienceTerm, 64>,
    // rgb linear colour, w 1 when enabled.
    height_fog_color: vec4<f32>,
    // x FogNearHeight, y FogFarHeight.
    fog_heights: vec4<f32>,
    // Gamma rgb: RimLightColor x RimLightIntensity (0: no rim).
    rim_color: vec4<f32>,
    // View-space direction (xyz), exponent (w).
    rim_dir_spec: vec4<f32>,
    // Authored display-space ambient light (no BRDF/exposure scaling).
    ambient: vec4<f32>,
}
@group(1) @binding(0) var<uniform> ambience: Ambience;

// Diffuse irradiance in Nebula's display space. Sum lights BEFORE multiplying
// by the display-space texture, then convert the result back to Bevy's linear target.
fn apply_ambience(input: ptr<function, LightingInput>, enable_diffuse: bool, shadow: f32) -> vec3<f32> {
    if !enable_diffuse { return vec3(0.0); }
    var result = vec3(0.0);
    for (var i = 0u; i < ambience.counts.y; i += 1u) {
        let term = ambience.terms[i];
        // The shadowed share fades with its source radiance. Dark target zones
        // lose shadows naturally; LightShadowIntensity controls their contrast.
        let front = max(vec3(0.0), term.front.rgb - term.shadowed.rgb * (1.0 - shadow));
        result += front * saturate(dot((*input).layers[LAYER_BASE].N, term.direction.xyz));
        // The back light, as the 2018/261 sun shader adds it: LightOppositeColor
        // x sat(BackLightFactor - N.L), diffuse only. It also reaches the faces
        // still turned a little towards the light (Wild Forest's orange rims).
        let back_weight = saturate(term.back.w - dot((*input).layers[LAYER_BASE].N, term.direction.xyz));
        result += term.back.rgb * back_weight;
    }
    return result;
}

// Keep converted materials' existing specular response separate from diffuse.
// Their source SpecMap/MatSpecularPower are not all retained by the glTF export.
fn ambience_specular(input: ptr<function, LightingInput>, shadow: f32) -> vec3<f32> {
    var result = vec3(0.0);
    for (var i = 0u; i < ambience.counts.y; i += 1u) {
        let term = ambience.terms[i];
        let front = max(vec3(0.0), term.front.rgb - term.shadowed.rgb * (1.0 - shadow));
        result += ambience_directional_light(term.direction.xyz, srgb_to_linear_rgb(front) * 3135.608,
            input, false);
    }
    return result;
}

fn local_irradiance(light_id: u32, input: ptr<function, LightingInput>, enable_diffuse: bool, spot: bool) -> vec3<f32> {
    if !enable_diffuse { return vec3(0.0); }
    let light = mesh_view_bindings::clustered_lights.data[light_id];
    let delta = light.position_radius.xyz - (*input).P;
    let distance_square = dot(delta, delta);
    let direction = delta * inverseSqrt(max(distance_square, 1e-8));
    // point_light() sends linear(authored colour * intensity) in pi*UNIT units.
    // Recover its display colour once; distance and N.L are display-space factors.
    let color = linear_rgb_to_srgb(max(light.color_inverse_square_range.rgb / 3135.608, vec3(0.0)));
    var weight = saturate(dot((*input).layers[LAYER_BASE].N, direction))
        * saturate(1.0 - sqrt(distance_square * light.color_inverse_square_range.w));
    if spot {
        var spot_dir = vec3(light.light_custom_data.x, 0.0, light.light_custom_data.y);
        spot_dir.y = sqrt(max(0.0, 1.0 - dot(spot_dir, spot_dir)));
        if (light.flags & mesh_view_types::POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u { spot_dir.y = -spot_dir.y; }
        let cone = saturate(dot(-spot_dir, direction) * light.light_custom_data.z + light.light_custom_data.w);
        weight *= cone * cone;
    }
    return color * weight;
}

fn compose_lighting(albedo: vec3<f32>, irradiance: vec3<f32>, extra_linear: vec3<f32>) -> vec3<f32> {
    // The original LightBuffer is UNORM8 encoded x0.5: its decoded limit is 2.
    let diffuse = linear_rgb_to_srgb(max(albedo, vec3(0.0))) * clamp(irradiance, vec3(0.0), vec3(2.0));
    return srgb_to_linear_rgb(diffuse + linear_rgb_to_srgb(max(extra_linear, vec3(0.0))));
}

fn distance_fog_clear(world_position: vec3<f32>) -> f32 {
    let fog = mesh_view_bindings::fog;
    if fog.mode != mesh_view_types::FOG_MODE_LINEAR { return 1.0; }
    // Original vertex/pixel shaders pass projected forward depth, not radial
    // camera distance. Looking sideways must not add fog at the viewport edges.
    let depth = max(-(mesh_view_bindings::view.view_from_world * vec4(world_position, 1.0)).z, 0.0);
    let clear = (fog.be.y - depth) / max(fog.be.y - fog.be.x, 1e-4);
    // Original shader: min(max(clear, FogColor.w), 1). FogIntensity is the
    // maximum opacity (FogColor.w = 1 - intensity), not a multiplier of distance.
    return clamp(clear, 1.0 - fog.base_color.a, 1.0);
}

fn apply_distance_fog(color: vec4<f32>, world_position: vec3<f32>) -> vec4<f32> {
    let scene = linear_rgb_to_srgb(max(color.rgb, vec3(0.0)));
    let fog = linear_rgb_to_srgb(max(mesh_view_bindings::fog.base_color.rgb, vec3(0.0)));
    return vec4(srgb_to_linear_rgb(mix(fog, scene, distance_fog_clear(world_position))), color.a);
}

fn height_fog_clear(world_position: vec3<f32>) -> f32 {
    let span = ambience.fog_heights.y - ambience.fog_heights.x;
    if ambience.height_fog_color.w == 0.0 || abs(span) < 1e-4 { return 1.0; }
    return saturate((ambience.fog_heights.y - world_position.y) / span);
}

// Nebula's height fog, after distance fog. The 2018/261 vertex shaders compute
// saturate((far - y) / (far - near)) from the world height (heightFogColor.w is
// FogFarHeight, fogDistances.z FogNearHeight), and the pixel shaders lerp from
// heightFogColor to the fogged colour by it: none above near, all of it past far.
fn apply_height_fog(color: vec4<f32>, world_position: vec3<f32>) -> vec4<f32> {
    let span = ambience.fog_heights.y - ambience.fog_heights.x;
    if ambience.height_fog_color.w == 0.0 || abs(span) < 1e-4 {
        return color;
    }
    let clear = height_fog_clear(world_position);
    // In gamma space, as the D3D9 shaders' lrp mixed non-sRGB colours: a linear
    // mix washes partial fog out (about 1.8x brighter at 30% over dark ground).
    let gamma = linear_rgb_to_srgb(max(color.rgb, vec3(0.0)));
    let fog = linear_rgb_to_srgb(ambience.height_fog_color.rgb);
    return vec4(srgb_to_linear_rgb(mix(fog, gamma, clear)), color.a);
}

// The characters' rim light, in gamma space, to add to their lit colour: the
// 2018/261 "character" and "characterfalloff" pixel shaders add
// rimLightColor x pow(max(dot(n_view, rimLightDirAndSpec.xyz), 0), .w) before the
// hit colour and the fogs. Their view-space normals face the camera with +z
// (the normal buffer's decode spans z -0.52..1), as Bevy's view space does.
fn rim_light(world_normal: vec3<f32>, view_from_world: mat4x4<f32>) -> vec3<f32> {
    if ambience.rim_color.w <= 0.0 {
        return vec3(0.0);
    }
    let n = normalize((view_from_world * vec4(world_normal, 0.0)).xyz);
    let rim = pow(max(dot(n, ambience.rim_dir_spec.xyz), 0.0), ambience.rim_dir_spec.w);
    return ambience.rim_color.rgb * rim;
}
