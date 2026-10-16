// Nebula's bloom (crate::rendering::nebula_bloom), from the 2018 frame shader and its
// shaders_sm30 post effects, in the client's gamma space:
//   SceneScaled  the opaque scene (before the ColorAlpha pass), half size
//   BrightPass   2c x BloomColor x lum(max(2c - threshold, 0))      pe_brightpassfilter
//                (c the 8-bit scene, encoded x 0.5: 2c is what is shown)
//   Bloom0       BrightPass at quarter size                           pe_downsample2x2
//   Bloom1..     (W0 s(0) + W1 (s(+o1) + s(-o1)) + 2 W2 s(+o2)) x 1.1, vertical
//                then horizontal, twice                               pe_bloomvert/hori
//   compose      shown + bloom, saturation, balance, vignette          pe_compose
// The intermediate targets are 8-bit, as the client's.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Params {
    // x threshold, y BloomScale, zw the source's pixel size
    p: vec4<f32>,
    // BloomColor
    color: vec4<f32>,
    // xy the blur direction (1,0) or (0,1)
    dir: vec4<f32>,
    balance: vec4<f32>,
    vignette_color: vec4<f32>,
    // Saturation, VignetteIntensity, VignetteSize, transient death gain
    grading: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var bloom: texture_2d<f32>;

const LUM: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

// What the client shows for a linear HDR value: gamma, within its range (2.0).
fn shown(c: vec3<f32>) -> vec3<f32> {
    return min(pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), vec3<f32>(2.0));
}

@fragment
fn fs_bright(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // The 2x2 average (pe_downsample2x2) by the bilinear sampler between texels.
    let s = shown(textureSample(source, source_sampler, in.uv).rgb);
    let threshold = params.p.x * 0.5;
    let over = max(s - vec3<f32>(threshold), vec3<f32>(0.0));
    return vec4<f32>(s * params.color.rgb * dot(over, LUM), 1.0);
}

@fragment
fn fs_down(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(source, source_sampler, in.uv);
}

@fragment
fn fs_blur(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let px = params.p.zw * params.dir.xy;
    let o1 = px * 1.88462;
    let o2 = px * 3.73077;
    var c = textureSample(source, source_sampler, in.uv) * 0.227027;
    c += (textureSample(source, source_sampler, in.uv + o1) + textureSample(source, source_sampler, in.uv - o1)) * 0.316216;
    c += textureSample(source, source_sampler, in.uv + o2) * (2.0 * 0.0702703);
    return c * 1.1;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(low, high, c > vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow(max((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4));
    return select(low, high, c > vec3<f32>(0.04045));
}

@fragment
fn fs_compose(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(source, source_sampler, in.uv);
    var display = linear_to_srgb(max(scene.rgb, vec3<f32>(0.0)));
    if params.p.y > 0.0 {
        display += textureSample(bloom, source_sampler, in.uv).rgb * params.p.y * 2.0;
    }
    // pe_compose grades the combined scene and bloom in display space. Balance
    // is an RGB multiplier, not Bevy's temperature/tint white-balance control.
    let grey = dot(display, LUM);
    display = mix(vec3<f32>(grey), display, params.grading.x) * params.balance.rgb;
    display = clamp(display, vec3<f32>(0.0), vec3<f32>(1.0));
    let distance = length(in.uv - vec2<f32>(0.5));
    let vignette = clamp(pow(distance, max(params.grading.z, 0.0001)) * params.grading.y, 0.0, 1.0);
    display = mix(display, params.vignette_color.rgb, vignette);
    return vec4<f32>(srgb_to_linear(display) * params.grading.w, scene.a);
}
