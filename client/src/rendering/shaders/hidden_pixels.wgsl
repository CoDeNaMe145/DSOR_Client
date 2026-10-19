// The local character where map geometry hides it, as the 2018/261 client's
// HiddenCharacterColor pass draws it (crate::rendering::hidden_pixels): the
// highlight colour where the character lies more than 1.5 units behind the
// visible surface, added over the frame. Nearer than that is its own body (or
// what it stands against): nothing.
// EVIDENCE: shaders_sm30 character / monster HiddenPixelsColor ps: the stored
//   eye distance at the pixel against the fragment's (`mad r0.x, r0.y, -c0.x,
//   r0.x`, `add r0.x, -r0.x, c4.z` with c4.z 1.5), colour highlightColor, alpha
//   hiddenPixelsDepthFactor beyond 1.5; frame/default.xml blurs it
//   (BlurHighlight0..2) and pe_compose adds it (HighlightBuffer).
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    view_transformations::{depth_ndc_to_view_z, position_world_to_view},
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif
#ifdef DSOR_AMBIENCE
#import dsor::ambience::encode_scene
#endif

struct Hidden {
    // Linear rgb, a its strength.
    color: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> hidden: Hidden;

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
#ifdef DEPTH_PREPASS
    let scene = depth_ndc_to_view_z(prepass_depth(in.position, 0u));
    let own = position_world_to_view(in.world_position.xyz).z;
    if scene - own <= 1.5 {
        discard;
    }
#else
    discard;
#endif
    var c = vec4<f32>(hidden.color.rgb, 1.0);
#ifdef DSOR_AMBIENCE
    c = encode_scene(c);
#endif
    // Added (AlphaMode::Add): premultiplied by the strength, alpha 0.
    out.color = vec4<f32>(c.rgb * hidden.color.a, 0.0);
    return out;
}
