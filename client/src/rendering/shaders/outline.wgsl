// The hovered actor's outline (crate::rendering::highlight): its silhouette (the highlight
// camera's image, transparent elsewhere) grown outward and drawn outside it in
// the highlight colour, fading over a few pixels -- the edge pe_outline gathers
// and pe_highlightvert/hori blur in the 2018 client (UNVERIFIED: width, falloff).
#import bevy_ui::ui_vertex_output::UiVertexOutput

struct Params {
    color: vec4<f32>,
    // xy: one pixel of the silhouette image, in uv; z: the ring spacing, pixels
    texel: vec4<f32>,
}

@group(1) @binding(0) var<uniform> params: Params;
@group(1) @binding(1) var mask: texture_2d<f32>;
@group(1) @binding(2) var mask_sampler: sampler;

const TAU: f32 = 6.2831853;

// The silhouette image is cleared to pure magenta: whatever is not is the actor.
fn covered(uv: vec2<f32>) -> f32 {
    let c = textureSample(mask, mask_sampler, uv).rgb;
    return step(0.02, distance(c, vec3<f32>(1.0, 0.0, 1.0)));
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    if params.color.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let inside = covered(in.uv);
    var glow = 0.0;
    // Three rings, each weaker farther out.
    for (var ring = 1; ring <= 3; ring++) {
        let r = f32(ring) * max(params.texel.z, 1.0);
        let w = 1.0 - (f32(ring) - 1.0) * 0.3;
        for (var i = 0; i < 12; i++) {
            let a = TAU * f32(i) / 12.0;
            let o = vec2<f32>(cos(a), sin(a)) * r * params.texel.xy;
            glow = max(glow, covered(in.uv + o) * w);
        }
    }
    let edge = glow * (1.0 - inside);
    return vec4<f32>(params.color.rgb, edge * params.color.a);
}
