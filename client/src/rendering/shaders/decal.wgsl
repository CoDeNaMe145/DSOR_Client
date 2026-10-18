// Nebula decal: the colour (base colour texture, uv) repeats over the ground at
// the node's Scale; the mask (uv_b, box coordinates) cuts its outline.
// SEE: client/src/rendering/decals.rs.
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var mask_sampler: sampler;

// The mask as stored. Bevy's glTF loader decodes every texture that is not a
// normal, metallic-roughness or occlusion map as sRGB, the mask included: a 0.5
// mask read 0.21, and Kingshill's grass decals showed at about 40% strength
// over the cobbles (pale, hazy green where the original's is a full olive).
fn stored(c: f32) -> f32 {
    return select(1.055 * pow(max(c, 0.0), 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
#ifdef VERTEX_UVS_B
    pbr_input.material.base_color.a *= stored(textureSample(mask_texture, mask_sampler, in.uv_b).r);
#endif
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
