#define_import_path dsor::ambience

#import bevy_pbr::lighting::{LightingInput, ambience_directional_light, LAYER_BASE}
#import bevy_pbr::view_transformations::position_world_to_view
#import bevy_pbr::mesh_view_bindings as view_bindings

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
    // Gamma rgb FogColor; w 1 enables Nebula's distance fog.
    fog_color: vec4<f32>,
    // x FogNearDist, y FogFarDist (camera depth), z the most fog (FogIntensity).
    fog_dist: vec4<f32>,
    // x 1 = sum the light in gamma space; y the terms' unit (PI x UNIT);
    // z the share of the whole light kept in full shadow; w 1 applies it.
    gamma_light: vec4<f32>,
    // The ambient light in that gamma sum (gamma rgb); w 1 squares the AO in
    // gamma (DSOR_FLAGS=aogamma), 2 x 1.3 clamped then squared (ao13), 0 once in linear.
    ambient_gamma: vec4<f32>,
}
@group(1) @binding(0) var<uniform> ambience: Ambience;

// The 2018/261 sun pass sums its light in gamma space (D3D9, no sRGB):
// sun x sat(N.L) + ambient + back x sat(BLF - N.L), and the materials multiply
// the albedo by that sum. In linear terms that is albedo x (gamma sum)^2.2.
// The fades and shadows stay the blend's; the sun's specular stays Bevy's.
// The sun pass multiplies the whole sum by globalShadowSampler.y^2 (pe_ssao
// writes that channel: the average visibility of 16 samples x
// occlusionConstants.w 1.3). Bevy's screen-space AO is a different, stronger
// quantity: it darkens open flat ground (visibility about 0.92 on Kingshill's
// market cobbles, where the original's are exactly texture x light). It darkens
// the sun pass's sum once, in linear, as Bevy applies its own AO (ambient_gamma.w
// 0); DSOR_FLAGS=aogamma squares it in the gamma sum like the original's formula,
// ao13 first applies pe_ssao's x 1.3 clamped to 1.
// MEASURED (2026-10-18, after the specular and shadow fixes; mean luminance vs
// the original, zones at the camera's focus; garden / market / market by the
// pumpkins / church / gate / throne / Wild Forest entrance):
//   once in linear:  -6% / 0% / +4% / +14% / +8% / -7% / +4%   (total 43%)
//   x 1.3, squared:  -3% / +3% / +7% / +17% / +12% / -6% / +4%  (total 52%)
//   squared:        -19% / -12% / -10% / +1% / -12% / -10% / -7% (total 71%)
// FAILURE (2026-10-17): Bevy's AO x 1.3 clamped to 1 almost everywhere (its
// visibility is already near 1 on open ground, a different quantity than
// pe_ssao's average) and brought back the sunlit overshoot (Kingshill garden
// p90 228 vs the original's 184; AO^2 as is: 189).
// The albedo as the original multiplies it: the texture's stored value. Bevy
// hands us the sRGB-decoded colour, and the compose pass shows the result with
// a 2.2 power, so the stored value goes in raised to 2.2: on screen it is then
// stored x light exactly, as D3D9 (no sRGB reads or writes) draws it. Taken as
// decoded, dark channels came out lifted (0.106 shown 0.128, 0.243 shown
// 0.252) and every texture lost saturation (Kingshill market grass).
fn gamma_albedo(decoded: vec3<f32>) -> vec3<f32> {
    let x = max(decoded, vec3(0.0));
    let stored = select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3(0.0031308));
    return pow(stored, vec3(2.2));
}

fn apply_ambience_gamma(input: ptr<function, LightingInput>, enable_diffuse: bool, shadow: f32, occlusion: vec3<f32>, points: vec3<f32>) -> vec3<f32> {
    let unit = ambience.gamma_light.y;
    let n = (*input).layers[LAYER_BASE].N;
    var light = ambience.ambient_gamma.rgb;
    var specular = vec3(0.0);
    var sun_shadowed = vec3(0.0);
    for (var i = 0u; i < ambience.counts.y; i += 1u) {
        let term = ambience.terms[i];
        let n_dot_l = dot(n, term.direction.xyz);
        let sun = pow(max(term.front.rgb, vec3(0.0)) / unit, vec3(1.0 / 2.2)) * saturate(n_dot_l);
        light += sun;
        // The share shadow takes away when the floor is off (S's shadowed term).
        sun_shadowed += sun - pow(max(term.front.rgb - term.shadowed.rgb, vec3(0.0)) / unit, vec3(1.0 / 2.2)) * saturate(n_dot_l);
        light += pow(max(term.back.rgb, vec3(0.0)) / unit, vec3(1.0 / 2.2)) * saturate(term.back.w - n_dot_l);
        // The highlight goes with the sun: none in shadow.
        let front = max(vec3(0.0), term.front.rgb - term.shadowed.rgb * (1.0 - shadow));
        if any(front > vec3(0.0)) {
            specular += ambience_directional_light(term.direction.xyz, front, input, false);
        }
    }
    if !enable_diffuse {
        return specular;
    }
    // The sun pass multiplies the whole sum by the blurred shadow value: full
    // shadow keeps gamma_light.z of it (crate::rendering::lighting::blend::shadow_floor).
    // Only with DSOR_FLAGS=shadowfloor (gamma_light.w 1) while interiors differ:
    // Bevy's sun shadow covers church and gate interiors the original leaves
    // unshadowed, and halving their light there made them too dark (Kingshill
    // church 17.9 vs 22.1). Without it shadow removes the sun's share only.
    if ambience.gamma_light.w != 0.0 {
        light *= mix(ambience.gamma_light.z, 1.0, shadow);
    } else {
        light -= sun_shadowed * (1.0 - shadow);
    }
    // The occlusion darkens the sun pass's sum only: once in linear is its
    // 1/2.2 power here.
    if ambience.ambient_gamma.w > 1.5 {
        let o = min(occlusion * 1.3, vec3(1.0));
        light *= o * o;
    } else if ambience.ambient_gamma.w != 0.0 {
        light *= occlusion * occlusion;
    } else {
        light *= pow(occlusion, vec3(1.0 / 2.2));
    }
    // The point lights add into the same gamma light buffer, after the sun pass
    // and so without its shadow and occlusion.
    light += points;
    return gamma_albedo((*input).diffuse_color) * pow(light, vec3(2.2)) * (unit * 0.31830988618) + specular;
}

// One point light's share of that gamma sum, as the 2018/261 point light pass
// writes it: lightColor x sat(N.L) x sat(1 - d / range), diffuse only (its
// alpha, the specular, is 0) and unshadowed. The light's Bevy colour is the
// gamma colour x intensity made linear (lighting::point_light), x PI x UNIT.
// EVIDENCE: shaders_sm30 lightPosRange ps (`mad_sat r0.w, r0.w, -c3.w, 1`,
//   `dp3_sat r0.x, r0, r2`, `mul r0.xyz, r0.x, c4`, `mov oC0.w, 0`).
fn nebula_point_light(light_id: u32, input: ptr<function, LightingInput>) -> vec3<f32> {
    let light = &view_bindings::clustered_lights.data[light_id];
    let to_light = (*light).position_radius.xyz - (*input).P;
    let d = length(to_light);
    let n_dot_l = saturate(dot((*input).layers[LAYER_BASE].N, to_light / max(d, 1e-4)));
    let falloff = saturate(1.0 - d * sqrt((*light).color_inverse_square_range.w));
    let color = max((*light).color_inverse_square_range.rgb, vec3(0.0)) / ambience.gamma_light.y;
    return pow(color, vec3(1.0 / 2.2)) * (n_dot_l * falloff);
}

// The point lights alone, where no sun term takes them into its sum.
fn apply_points_gamma(input: ptr<function, LightingInput>, points: vec3<f32>) -> vec3<f32> {
    return gamma_albedo((*input).diffuse_color) * pow(points, vec3(2.2)) * (ambience.gamma_light.y * 0.31830988618);
}

fn apply_ambience(input: ptr<function, LightingInput>, enable_diffuse: bool, shadow: f32, occlusion: vec3<f32>, points: vec3<f32>) -> vec3<f32> {
    if ambience.gamma_light.x != 0.0 {
        return apply_ambience_gamma(input, enable_diffuse, shadow, occlusion, points);
    }
    var result = vec3(0.0);
    for (var i = 0u; i < ambience.counts.y; i += 1u) {
        let term = ambience.terms[i];
        // The shadowed share fades with its source radiance. Dark target zones
        // lose shadows naturally; LightShadowIntensity controls their contrast.
        let front = max(vec3(0.0), term.front.rgb - term.shadowed.rgb * (1.0 - shadow));
        if any(front > vec3(0.0)) {
            result += ambience_directional_light(term.direction.xyz, front, input, enable_diffuse);
        }
        // The back light, as the 2018/261 sun shader adds it: LightOppositeColor
        // x sat(BackLightFactor - N.L), diffuse only. It also reaches the faces
        // still turned a little towards the light (Wild Forest's orange rims).
        // Scaled as Bevy's diffuse is (albedo / pi), like the front light.
        let back_weight = saturate(term.back.w - dot((*input).layers[LAYER_BASE].N, term.direction.xyz));
        if enable_diffuse && back_weight > 0.0 && any(term.back.rgb > vec3(0.0)) {
            result += (*input).diffuse_color * term.back.rgb * (back_weight * 0.31830988618);
        }
    }
    return result;
}

// Nebula's distance fog: clear = clamp((far - depth) / (far - near), 1 - most, 1)
// with depth along the camera's view (the 2018/261 vertex shaders pass the
// view-space position), the fog colour mixed in gamma space like their lrp.
fn apply_nebula_fog(color: vec4<f32>, world_position: vec3<f32>) -> vec4<f32> {
    let depth = -position_world_to_view(world_position).z;
    let span = max(ambience.fog_dist.y - ambience.fog_dist.x, 1e-4);
    let clear = clamp((ambience.fog_dist.y - depth) / span, 1.0 - ambience.fog_dist.z, 1.0);
    let gamma = pow(max(color.rgb, vec3(0.0)), vec3(1.0 / 2.2));
    return vec4(pow(mix(ambience.fog_color.rgb, gamma, clear), vec3(2.2)), color.a);
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
    let clear = saturate((ambience.fog_heights.y - world_position.y) / span);
    // In gamma space, as the D3D9 shaders' lrp mixed non-sRGB colours: a linear
    // mix washes partial fog out (about 1.8x brighter at 30% over dark ground).
    let gamma = pow(max(color.rgb, vec3(0.0)), vec3(1.0 / 2.2));
    let fog = pow(ambience.height_fog_color.rgb, vec3(1.0 / 2.2));
    return vec4(pow(mix(fog, gamma, clear), vec3(2.2)), color.a);
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
