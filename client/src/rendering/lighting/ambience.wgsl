#define_import_path dsor::ambience

#import bevy_pbr::lighting::{LightingInput, ambience_directional_light}

struct AmbienceTerm {
    direction: vec4<f32>,
    front: vec4<f32>,
    shadowed: vec4<f32>,
    back: vec4<f32>,
}
struct Ambience {
    counts: vec4<u32>,
    terms: array<AmbienceTerm, 64>,
}
@group(1) @binding(0) var<uniform> ambience: Ambience;

fn apply_ambience(input: ptr<function, LightingInput>, enable_diffuse: bool, shadow: f32) -> vec3<f32> {
    var result = vec3(0.0);
    for (var i = 0u; i < ambience.counts.y; i += 1u) {
        let term = ambience.terms[i];
        // The shadowed share fades with its source radiance. Dark target zones
        // lose shadows naturally; LightShadowIntensity controls their contrast.
        let front = max(vec3(0.0), term.front.rgb - term.shadowed.rgb * (1.0 - shadow));
        if any(front > vec3(0.0)) {
            result += ambience_directional_light(term.direction.xyz, front, input, enable_diffuse);
        }
        if any(term.back.rgb > vec3(0.0)) {
            result += ambience_directional_light(-term.direction.xyz, term.back.rgb, input, enable_diffuse);
        }
    }
    return result;
}
