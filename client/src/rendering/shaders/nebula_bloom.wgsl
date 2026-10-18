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
    // xy the blur direction (1,0) or (0,1); z 1 when the scene target holds
    // gamma colours (lighting/ambience.wgsl encode_scene)
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

// What the client shows for a scene value, within its range (2.0): the value
// itself in the gamma scene target, else the linear value's gamma.
fn shown(c: vec3<f32>) -> vec3<f32> {
    if params.dir.z != 0.0 {
        return min(max(c, vec3<f32>(0.0)), vec3<f32>(2.0));
    }
    return min(pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), vec3<f32>(2.0));
}

@fragment
fn fs_bright(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // The 2x2 average (pe_downsample2x2) by the bilinear sampler between texels.
    let s = shown(textureSample(source, source_sampler, in.uv).rgb);
    // pe_brightpassfilter: max(2c - hdrBrightPassThreshold, 0) with 2c the shown
    // colour (`mad r1.xyz, r0, 2, -c0.x`): the full threshold, not half.
    let threshold = params.p.x;
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

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow(max((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4));
    return select(low, high, c > vec3<f32>(0.04045));
}

@fragment
fn fs_compose(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(source, source_sampler, in.uv);
    // The original (D3D9, no sRGB) shows its gamma-space colours as they are;
    // the light sums here are taken back to linear with 2.2, so 2.2 shows them.
    // The sRGB curve's linear toe crushed dark products (a dark rock, albedo
    // 0.2 under light 0.25: 0.05 in the original, 0.018 through sRGB).
    // In the gamma scene target the stored values are already what is shown.
    var display = pow(max(scene.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    if params.dir.z != 0.0 {
        display = max(scene.rgb, vec3<f32>(0.0));
    }
    if params.p.y > 0.0 {
        // pe_compose: 2 x scene + bloom x hdrBloomScale (`add r0, r2, r2`,
        // `mad r0.xyz, r1, c0.x, r0`): the scene's x 2 undoes its 8-bit encoding;
        // the bloom texture is added as stored, without a second x 2.
        display += textureSample(bloom, source_sampler, in.uv).rgb * params.p.y;
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
