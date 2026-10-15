#define_import_path dsor::occlusion

#import bevy_pbr::mesh_view_bindings::view

struct CutoutParams {
    camera: vec4<f32>,
    focus: vec4<f32>,
    limits: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var<uniform> cutout: CutoutParams;

fn player_cutout(world: vec3<f32>, pixel: vec2<f32>) {
    // A shadow camera must still see the complete wall/roof. The player's
    // camera and its depth/normal prepass share exactly the same cutout.
    if cutout.focus.w <= 0.0 || distance(view.world_position, cutout.camera.xyz) > 0.01 {
        return;
    }
    let axis = cutout.focus.xyz - cutout.camera.xyz;
    let axis_length = length(axis);
    if axis_length < 0.1 || world.y <= cutout.limits.x { return; }
    let direction = axis / axis_length;
    let relative = world - cutout.camera.xyz;
    let along = dot(relative, direction);
    if along <= 0.05 || along >= axis_length - cutout.limits.y { return; }
    // A cone keeps the opening's apparent size stable from camera to player.
    let radius = cutout.camera.w * along / axis_length;
    let radial = length(relative - along * direction) / max(radius, 0.001);
    let fade = (1.0 - smoothstep(0.65, 1.0, radial)) * cutout.focus.w;
    // Fixed screen-space noise avoids transparent sorting and temporal flicker.
    let noise = fract(52.9829189 * fract(dot(floor(pixel), vec2<f32>(0.06711056, 0.00583715))));
    if noise < fade { discard; }
}
