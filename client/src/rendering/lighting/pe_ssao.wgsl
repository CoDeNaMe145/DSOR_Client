// The 2018/261 client's screen-space ambient occlusion (shaders_sm30 pe_ssao),
// patched into Bevy's SSAO compute pass (crate::rendering::lighting
// patch_ssao) in place of its GTAO when the view's object thickness is
// negative (AoMode::Original). Bevy's depth preprocessing, normal prepass and
// spatial denoiser stay; only the per-pixel estimate is the original's.
//
// pe_ssao, per pixel: P the view-space position (eye distance x view ray), N
// its normal, (T, B, N) a frame around N, r a vector from a 4x4 random table.
// For each of 16 fixed offsets s: s' = (s - 2 (s.r) r) x 0.4 (occlusionConstants.x),
// the sample S = P + T s'.x + N max(0.05, |s'.y|) + B s'.z, projected to the
// screen; with d the eye distance stored there, D = |S| - d counts when
// 0.05 < D < 0.4 (occlusionConstants.y, .z), by sat(1 - D). The sum x 1.3 / 16
// (occlusionConstants.w) is subtracted from the shadow buffer's y (1 where
// nothing occludes), then the two shadow blurs (pe_blurshadowhori / vert) each
// output 1 - (0.242 + 0.399 + 0.242) x (1 - y): what is left of the occlusion
// is 0.883^2 of it. The sun pass multiplies its light by y squared
// (lighting/ambience.wgsl, ambient_gamma.w 4).
// EVIDENCE: shaders_sm30 pe_ssao ps (the 16 offsets are sampleOffset's default
//   in the effect, the random table textures/system/randomtable.dds, 4 x 4,
//   read as rgb x 2 - 1 at the pixel modulo 4); occlusionConstants captured
//   (0.4, 0.05, 0.4, 1.3) in the running release261_192 client, which drew
//   pe_ssao once per frame in Kingshill's garden, church and throne room
//   (lighting-work/captures).
// UNVERIFIED: the blend that subtracts pe_ssao's output (its x 0, y the
//   occlusion, z/w 1 fit only a reverse-subtract write); the frame's tangent
//   handedness (only the noise pattern depends on it).

fn dsor_eye_distance(uv: vec2<f32>) -> f32 {
    let depth = textureSampleLevel(preprocessed_depth, point_clamp_sampler, uv, 0.0).r;
    if depth <= 0.0 {
        return 1e9;
    }
    return length(reconstruct_view_space_position(depth, uv));
}

fn dsor_pe_ssao(pixel: vec2<i32>, pixel_depth: f32) -> f32 {
    var offsets = array<vec3<f32>, 16>(
        vec3<f32>(0.42576271, 0.58859158, -0.14068450),
        vec3<f32>(0.14389460, 0.61704439, -0.68005031),
        vec3<f32>(-0.19622080, 0.03216840, -0.65260619),
        vec3<f32>(0.05483650, 0.20523620, 0.45520449),
        vec3<f32>(0.49072590, 0.46578890, 0.43356001),
        vec3<f32>(0.07976050, 0.49238971, -0.63035947),
        vec3<f32>(-0.71181440, 0.24536429, -0.42245609),
        vec3<f32>(-0.39402381, 0.12137420, -0.45991570),
        vec3<f32>(-0.65528709, 0.55461258, 0.51254117),
        vec3<f32>(-0.46236500, 0.16360560, 0.14650770),
        vec3<f32>(0.01240810, 0.07936010, 0.95602667),
        vec3<f32>(0.58552790, 0.48316830, 0.41097459),
        vec3<f32>(0.45031580, 0.46063891, 0.73132062),
        vec3<f32>(-0.57773900, 0.30280679, 0.47087029),
        vec3<f32>(-0.75984901, 0.40231469, 0.27086419),
        vec3<f32>(-0.16585530, 0.33916911, -0.73764718),
    );
    // randomtable.dds texels (rgb / 255 x 2 - 1), row by row.
    var random = array<vec3<f32>, 16>(
        vec3<f32>(-0.30980, -1.0, 1.0), vec3<f32>(-0.52941, 1.0, 1.0), vec3<f32>(1.0, -1.0, -1.0), vec3<f32>(-1.0, 1.0, -1.0),
        vec3<f32>(1.0, -0.74118, 0.52157), vec3<f32>(0.74902, -0.89020, -1.0), vec3<f32>(-0.50588, -1.0, 0.85882), vec3<f32>(0.38039, -0.58431, -1.0),
        vec3<f32>(1.0, 1.0, -1.0), vec3<f32>(0.70980, -1.0, 1.0), vec3<f32>(-0.42745, 1.0, -1.0), vec3<f32>(0.43529, -1.0, -0.92941),
        vec3<f32>(1.0, -1.0, -1.0), vec3<f32>(-1.0, -1.0, -1.0), vec3<f32>(-1.0, 1.0, -0.23137), vec3<f32>(-1.0, -1.0, 1.0),
    );
    // Nothing drawn (Bevy's pass adds 0.00001 to the reverse-Z depth first).
    if pixel_depth <= 0.00002 {
        return 1.0;
    }
    let uv = (vec2<f32>(pixel) + 0.5) / view.viewport.zw;
    let p = reconstruct_view_space_position(pixel_depth, uv);
    let n = normalize(load_normal_view_space(uv));
    let ray = normalize(p);
    var c = cross(n, ray);
    if dot(c, c) < 1e-8 {
        c = cross(n, vec3<f32>(1.0, 0.0, 0.0));
    }
    let t = normalize(cross(n, normalize(c)));
    let b = normalize(cross(n, t));
    let r = random[(pixel.y & 3) * 4 + (pixel.x & 3)];
    var occlusion = 0.0;
    for (var i = 0; i < 16; i += 1) {
        let s = offsets[i];
        let o = (s - 2.0 * dot(s, r) * r) * 0.4;
        let q = p + t * o.x + n * max(0.05, abs(o.y)) + b * o.z;
        let clip = view.clip_from_view * vec4<f32>(q, 1.0);
        if clip.w <= 0.0 {
            continue;
        }
        let suv = vec2<f32>(clip.x / clip.w * 0.5 + 0.5, clip.y / clip.w * -0.5 + 0.5);
        let d = length(q) - dsor_eye_distance(suv);
        if d > 0.05 && d < 0.4 {
            occlusion += saturate(1.0 - d);
        }
    }
    let written = min(occlusion * 1.3 / 16.0, 1.0);
    return 1.0 - 0.883 * 0.883 * written;
}
