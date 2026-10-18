// Nebula's shd:refraction, as the 2018 client draws it (shaders_sm30 "refraction"
// ps_3_0, disassembled): the scene behind the surface, read at the pixel moved by
// the DuDv map --
//   offset = (dudv.xy * 2 - 1) * pixelSize * displacementFactor * 10   (ps preshader)
//   colour = scene(screen + offset) * vertex colour, alpha = alphaBlendFactor * vertex alpha
//   dudv uv = uv + uvVelocity * time                                     (vs preshader)
// drawn over that same scene. bevy's copy of the scene (view_transmission_texture)
// stands in for the client's light buffer; the alpha blend is done here, against
// the unmoved scene, so the pass needs no blending state.
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals, view_transmission_texture, view_transmission_sampler},
}

struct Refraction {
    // x: distortion strength (pixels), y: alphaBlendFactor, zw: texture scroll per second.
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> refraction: Refraction;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var dudv_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var dudv_sampler: sampler;

// sRGB decoding undone (the exact curve).
fn to_stored(c: vec2<f32>) -> vec2<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec2<f32>(0.0)), vec2<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec2<f32>(0.0031308));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var tint = vec4<f32>(1.0);
#ifdef VERTEX_COLORS
    tint = in.color;
#endif
    var uv = vec2<f32>(0.0);
#ifdef VERTEX_UVS_A
    uv = in.uv;
#endif
    uv = uv + refraction.params.zw * globals.time;
    // The DuDv map is bound as a colour texture, so the GPU sRGB-decodes it: its
    // neutral 0.5 came out 0.21, and (d x 2 - 1) a constant shift that drew a
    // displaced copy of the scene over the whole surface (a doubled player, a
    // visible rectangle). Back to the stored values first.
    let raw = to_stored(textureSample(dudv_texture, dudv_sampler, uv).xy);
    let d = raw * 2.0 - 1.0;
    // Bevy copies the entire render target, including the area outside an
    // offset viewport (editor panels). Fragment positions are already in that
    // target's pixel coordinates; normalizing against the viewport instead
    // stretches the scene and samples its unrendered margins as black circles.
    let pixel = 1.0 / vec2<f32>(textureDimensions(view_transmission_texture));
    let screen = in.position.xy * pixel;
    // Keep the displaced bilinear footprint inside this view. Clamping only to
    // the texture edge still lets heat haze pull in pixels behind editor panels.
    let lower = (view.main_pass_viewport.xy + vec2<f32>(0.5)) * pixel;
    let upper = (view.main_pass_viewport.xy + view.main_pass_viewport.zw - vec2<f32>(0.5)) * pixel;
    let render_scale = view.main_pass_viewport.zw / view.viewport.zw;
    let displaced = clamp(screen + d * pixel * render_scale * refraction.params.x, lower, upper);
    let moved = textureSample(view_transmission_texture, view_transmission_sampler, displaced);
    let behind = textureSample(view_transmission_texture, view_transmission_sampler, screen);
    let a = clamp(refraction.params.y * tint.a, 0.0, 1.0);
    var out: FragmentOutput;
    out.color = vec4<f32>(mix(behind.rgb, moved.rgb * tint.rgb, a), 1.0);
    return out;
}
