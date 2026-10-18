use super::*;
use super::super::{Ambience, Level, Sun, LocalBubble, CurrentMap, follow_level, apply_look};

fn preset(dir: Vec3, intensity: f32, shadows: f32) -> Bubble {
    Bubble { dir: Some(dir.to_array()), light_intensity: intensity, light_shadow_intensity: shadows,
        light_color: [1., 0.7, 0.4, 1.], light_opposite_color: [0.1, 0.2, 0.3, 1.], fade_time: 1., ..default() }
}

fn sample(blend: &LightBlend, normal: Vec3, shadow: f32) -> Vec3 {
    blend.0.iter().map(|term| {
        (term.front - term.shadowed * (1. - shadow)) * normal.dot(term.dir).max(0.)
            + term.back * (term.back_offset - normal.dot(term.dir)).clamp(0., 1.)
    }).sum()
}

fn near(a: Vec3, b: Vec3) { assert!(a.distance(b) < 1e-5, "{a:?} != {b:?}"); }

#[test]
fn every_surface_fades_between_fixed_light_contributions() {
    let a = preset(Vec3::new(-0.1356, 0.7211, -0.6794), 1.4, 1.);
    let b = preset(Vec3::Z * 42.603, 0.2, 0.4);
    for t in [0., 0.1, 0.25, 0.5, 0.75, 1.] {
        let mixed = LightBlend::from(&a).mix(&b, t);
        assert!(mixed.0.iter().all(|term| term.dir == direction(a.dir) || term.dir == direction(b.dir)));
        for normal in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::NEG_Z, Vec3::ONE.normalize()] {
            for shadow in [0., 0.4, 1.] {
                near(sample(&mixed, normal, shadow), sample(&LightBlend::from(&a), normal, shadow)
                    .lerp(sample(&LightBlend::with_reference(&b, a.sun_luminance()), normal, shadow), t));
            }
        }
    }
}

#[test]
fn interrupted_and_reversed_transitions_preserve_the_visible_mixture() {
    let a = preset(Vec3::X, 1.4, 1.);
    let b = preset(Vec3::Z, 0.1, 0.);
    let c = preset(Vec3::Y, 0.8, 0.6);
    let current = LightBlend::from(&a).mix(&b, 0.6);
    for target in [&a, &c] {
        assert_eq!(current.mix(target, 0.), current);
        for t in [0.1, 0.7, 1.] {
            for normal in [Vec3::X, Vec3::Z, Vec3::ONE.normalize()] {
                near(sample(&current.mix(target, t), normal, 0.3), sample(&current, normal, 0.3)
                    .lerp(sample(&LightBlend::with_reference(target, current.1), normal, 0.3), t));
            }
        }
        assert_eq!(current.mix(target, 1.), LightBlend::with_reference(target, current.1));
    }
}

#[test]
fn dark_zones_fade_out_shadow_contrast_and_authored_strength_is_respected() {
    let a = preset(Vec3::X, 1., 1.);
    let dark = preset(Vec3::Z, 0., 1.);
    for t in [0., 0.25, 0.5, 0.75, 1.] {
        let blend = LightBlend::from(&a).mix(&dark, t);
        let contrast = sample(&blend, Vec3::X, 1.) - sample(&blend, Vec3::X, 0.);
        near(contrast, LightBlend::from(&a).0[0].front * (1. - t));
    }
    let unshadowed = LightBlend::from(&preset(Vec3::X, 1., 0.));
    near(sample(&unshadowed, Vec3::X, 0.), sample(&unshadowed, Vec3::X, 1.));
    let half = LightBlend::from(&preset(Vec3::X, 1., 0.5));
    near(sample(&half, Vec3::X, 0.), sample(&half, Vec3::X, 1.) * 0.5);

    // A dim interior's shadow still takes its whole sun, as the original's
    // shadowDarkeningFactor is 0 in every Kingshill zone (throne room, church).
    let dim = preset(Vec3::X, 0.2, 1.);
    let blend = LightBlend::from(&a).mix(&dim, 1.);
    let lit = sample(&blend, Vec3::X, 1.);
    near(sample(&blend, Vec3::X, 0.), Vec3::ZERO);
    assert!(lit.length() > 0.);
}

#[test]
fn opposite_directions_and_repeated_entries_stay_finite_and_compact() {
    let a = preset(Vec3::X, 1., 1.);
    let b = preset(Vec3::NEG_X, 1., 0.);
    let mut blend = LightBlend::from(&a);
    for i in 0..1000 {
        blend = blend.mix(if i % 2 == 0 { &a } else { &b }, 0.1);
        assert!(blend.0.len() <= 2);
        assert!(blend.0.iter().all(|t| t.dir.is_finite() && t.front.is_finite()));
    }
}

#[test]
fn legacy_assets_default_shadow_strength_and_keep_separate_projection() {
    let b: Bubble = serde_json::from_str("{}").unwrap();
    assert_eq!(b.light_shadow_intensity, 1.);
    let ambience: Ambience = serde_json::from_str(r#"{"shadow":[0,1,0],"global":{"dir":[1,0,0],"LightShadowIntensity":0.3}}"#).unwrap();
    assert_eq!(ambience.shadow, Some(Vec3::Y.to_array()));
    assert_eq!(ambience.global.unwrap().light_shadow_intensity, 0.3);
}

#[test]
fn entering_exiting_and_retargeting_never_move_the_shadow_sun() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Ambience>().init_resource::<GpuAmbience>()
        .init_resource::<Lighting>().init_resource::<GlobalAmbientLight>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(100)))
        .add_systems(Update, (follow_level, apply_look).chain());
    let global = preset(Vec3::new(-0.1356, 0.7211, -0.6794), 1.4, 1.);
    let local = preset(Vec3::Z * 42.603, 0.1, 0.);
    let shadow = Vec3::new(-0.2747, 0.7545, -0.5961).normalize();
    let handle = app.world_mut().resource_mut::<Assets<Ambience>>().add(Ambience {
        global: Some(global.clone()), shadow: Some(shadow.to_array()), bubbles: vec![LocalBubble {
            look: local.clone(), m: Some(Mat4::from_scale(Vec3::splat(10.)).to_cols_array()),
            priority: 1, event: -1, shape: "Box".into(), enabled: true,
        }], ..default()
    });
    app.insert_resource(CurrentMap::new("test".into(), Handle::default()))
        .insert_resource(Level { map: "test".into(), ambience: handle, ..default() });
    let camera = app.world_mut().spawn((Camera3d::default(), GlobalTransform::from_translation(Vec3::X * 20.))).id();
    let sun = app.world_mut().spawn((Sun, DirectionalLight::default(), Transform::default())).id();
    app.update();
    for (position, frames) in [(Vec3::ZERO, 4), (Vec3::X * 20., 3), (Vec3::ZERO, 15), (Vec3::X * 20., 15)] {
        app.world_mut().entity_mut(camera).insert(GlobalTransform::from_translation(position));
        for _ in 0..frames {
            app.update();
            near(*app.world().get::<Transform>(sun).unwrap().back(), shadow);
            near(*app.world().get::<GlobalTransform>(sun).unwrap().back(), shadow);
            assert!(app.world().resource::<Level>().blend.0.len() <= 2);
        }
    }
    let level = app.world().resource::<Level>();
    assert!(level.fade.is_none());
    assert_eq!(level.bubble, global);
    assert_eq!(level.blend, LightBlend::from(&global));
}

#[test]
fn uniform_layout_matches_wgsl_and_remains_within_webgl_limits() {
    assert_eq!(GpuTerm::min_size().get(), 64);
    // counts, the terms, height fog colour and heights, rim colour and direction, fog colour and distances, gamma light and ambient, output.
    assert_eq!(GpuAmbience::min_size().get(), 16 + 64 * MAX_TERMS as u64 + 32 + 32 + 32 + 32 + 16);
    assert!(GpuAmbience::min_size().get() <= 16384);
    let disabled = GpuAmbience::default();
    assert_eq!(disabled.counts.x, 0);
    let active = LightBlend::from(&preset(Vec3::X, 1., 1.)).gpu(&Lighting::default());
    assert_eq!(active.counts, UVec4::new(1, 1, 0, 0));
}

#[test]
fn height_fog_reaches_the_gpu_linear_and_only_once_enabled() {
    // The inactive buffer (views without the level's look) is all zeros: no height fog.
    assert_eq!(GpuAmbience::default().height_fog_color.w, 0.);
    let wild = Bubble { height_fog_color: [0.2275, 0.6627, 0.6902, 1.], fog_near_height: -3., fog_far_height: -20., ..default() };
    let gpu = LightBlend::from(&wild).gpu(&Lighting::default()).with_height_fog(&wild);
    let c = Color::srgb(0.2275, 0.6627, 0.6902).to_linear();
    near(gpu.height_fog_color.truncate(), Vec3::new(c.red, c.green, c.blue));
    assert_eq!(gpu.height_fog_color.w, 1.);
    assert_eq!(gpu.fog_heights, Vec4::new(-3., -20., 0., 0.));
}

#[test]
fn back_light_factor_reaches_the_gpu_and_fades_with_the_zone() {
    // a0302_wildforest: BackLightFactor 0.3, an orange LightOppositeColor.
    let wild = Bubble { back_light_factor: 0.3, light_opposite_color: [1., 0.349, 0., 1.], ..preset(Vec3::Y, 0.25, 1.) };
    let blend = LightBlend::from(&wild);
    assert_eq!(blend.0[0].back_offset, 0.3);
    assert_eq!(blend.gpu(&Lighting::default()).terms[0].back.w, 0.3);
    // The original's sat(BackLightFactor - N.L): a face turned slightly towards
    // the light (N.L 0.1) still gets two tenths of it, one facing it gets none.
    let side = Vec3::new(0.995, 0.1, 0.).normalize();
    near(sample(&blend, side, 1.), blend.0[0].front * side.dot(Vec3::Y) + blend.0[0].back * (0.3 - side.dot(Vec3::Y)));
    near(sample(&blend, Vec3::Y, 1.) - blend.0[0].front, Vec3::ZERO);
    // Fading to a zone of the same light direction blends the offset too.
    let indoor = Bubble { back_light_factor: 0.0, ..wild.clone() };
    let half = blend.mix(&indoor, 0.5);
    assert_eq!(half.0.len(), 1);
    assert!((half.0[0].back_offset - 0.15).abs() < 1e-6);
}

#[test]
fn pbr_patch_adds_height_fog_after_distance_fog_on_fogged_materials_only() {
    let source = r#"
    // Point lights (direct)
    for (var i: u32 = 0u; i < 1u; i = i + 1u) {
        let light_contrib = lighting::point_light(light_id, &lighting_input, enable_diffuse, true);
        direct_light += light_contrib * shadow;
    }
    // directional lights (direct)
    let n_directional_lights = view_bindings::lights.n_directional_lights;
        var light_contrib = lighting::directional_light(i, &lighting_input, enable_diffuse);
        direct_light += light_contrib * shadow;
        let transmitted_light_contrib =
            lighting::directional_light(i, &transmissive_lighting_input, enable_diffuse);
        transmitted_light += transmitted_light_contrib * transmitted_shadow;
    if ((pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT) != 0u) {
        output_color = apply_fog(
            view_bindings::fog,
            output_color,
            pbr_input.world_position.xyz,
            view_bindings::view.world_position.xyz,
            pbr_input.frag_coord.xy,
        );
    }
#endif  // DISTANCE_FOG
#ifdef PREMULTIPLY_ALPHA
    output_color = premultiply_alpha(pbr_input.material.flags, output_color);
#endif
    return output_color;
"#;
    let patched = blend_function(source).expect("anchors present");
    let fog = patched.find("output_color = apply_fog(").unwrap();
    let height = patched.find("output_color = apply_height_fog(output_color, pbr_input.world_position.xyz);").unwrap();
    let fogged_block_end = patched.find("#endif  // DISTANCE_FOG").unwrap();
    assert!(fog < height && height < fogged_block_end, "height fog follows distance fog inside its block");
    assert!(patched.contains("#import dsor::ambience::{ambience, apply_ambience, apply_height_fog, apply_nebula_fog, apply_points_gamma, encode_scene, nebula_point_light}"));
    // The gamma scene target: encoded after the fogs, before the premultiplication.
    let encoded = patched.find("output_color = encode_scene(output_color);").unwrap();
    assert!(height < encoded && encoded < patched.find("output_color = premultiply_alpha(").unwrap());
    // Point lights join the gamma sum before the sun term reads it; Bevy's
    // point light stays for the linear sum and without DSOR_AMBIENCE.
    let declared = patched.find("var dsor_point_gamma = vec3<f32>(0.0);").unwrap();
    let gathered = patched.find("dsor_point_gamma += nebula_point_light(light_id, &lighting_input);").unwrap();
    let alone = patched.find("direct_light += apply_points_gamma(&lighting_input, dsor_point_gamma);").unwrap();
    let summed = patched.find("apply_ambience(&lighting_input, enable_diffuse, shadow, dsor_occlusion, dsor_point_gamma)").unwrap();
    // The original's AO estimate is read as written, not Bevy's multi-bounce.
    assert!(patched.find("var dsor_occlusion = diffuse_occlusion;").unwrap() < summed);
    assert!(declared < gathered && gathered < alone && alone < summed);
    assert_eq!(patched.matches("lighting::point_light(light_id, &lighting_input, enable_diffuse, true)").count(), 2);
    // Nebula's distance fog replaces Bevy's where the level's look is active;
    // Bevy's stays for the inactive buffer and without DSOR_AMBIENCE.
    let nebula = patched.find("output_color = apply_nebula_fog(output_color, pbr_input.world_position.xyz);").unwrap();
    assert!(nebula < fog && fog < height, "Nebula fog first, Bevy's as the fallback, then height fog");
    assert_eq!(patched.matches("output_color = apply_fog(").count(), 2, "fallback under the ambience and without it");
    // A Bevy version without this fog block must fail loudly, never silently skip.
    assert!(blend_function(&source.replace("#endif  // DISTANCE_FOG", "#endif")).is_none());
}

#[test]
fn rim_light_reaches_the_gpu_in_gamma_with_the_engine_direction() {
    // The inactive buffer is all zeros: no rim.
    assert_eq!(GpuAmbience::default().rim_color, Vec4::ZERO);
    // a0302_wildforest, as the running release261_192 client sends it: c8 =
    // RimLightColor x RimLightIntensity, c9 = (0.6963, 0.6963, 0.1741, 4).
    let wild = Bubble { rim_light_color: [0.2275, 0.6627, 0.6902, 1.], rim_light_intensity: 0.5, ..default() };
    let gpu = GpuAmbience::default().with_rim_light(&wild);
    near(gpu.rim_color.truncate(), Vec3::new(0.2275, 0.6627, 0.6902) * 0.5);
    assert!(gpu.rim_color.truncate().distance(Vec3::new(0.1137, 0.3314, 0.3451)) < 1e-3);
    assert_eq!(gpu.rim_color.w, 0.5);
    assert!(gpu.rim_dir_spec.truncate().distance(Vec3::new(0.6963, 0.6963, 0.1741)) < 1e-3);
    assert_eq!(gpu.rim_dir_spec.w, 4.);
    let off = GpuAmbience::default().with_rim_light(&Bubble { rim_light_intensity: 0., ..wild });
    assert_eq!(off.rim_color, Vec4::ZERO);
}

#[test]
fn the_back_light_is_not_scaled_by_the_sun_intensity() {
    // As the running client sends globalBackLightColor: LightOppositeColor as is,
    // while the sun's colour is scaled by LightIntensity (Kingshill 1.4).
    let at = |intensity: f32| LightBlend::from(&Bubble { light_intensity: intensity, light_opposite_color: [0.298, 0.3373, 0.3647, 1.], ..preset(Vec3::Y, intensity, 1.) });
    near(at(1.4).0[0].back, at(0.25).0[0].back);
    let c = Color::srgb(0.298, 0.3373, 0.3647).to_linear();
    near(at(1.4).0[0].back, Vec3::new(c.red, c.green, c.blue));
    assert!(at(1.4).0[0].front.x > at(0.25).0[0].front.x);
}

#[test]
fn uniform_fields_follow_the_wgsl_order() {
    // The GPU reads the uniform by position: GpuAmbience's Rust field order must
    // be the WGSL struct's. FAILURE (2026-10-17): the fog fields were added before
    // rim_dir_spec in Rust and after it in WGSL; the fog read its enable flag
    // from fog_dist.w (0) and the rim its direction from the fog colour.
    let rust = include_str!("blend.rs");
    let wgsl = include_str!("ambience.wgsl");
    let fields = |src: &str, start: &str, sep: &str| -> Vec<String> {
        let body = &src[src.find(start).unwrap() + start.len()..];
        let body = &body[..body.find('}').unwrap()];
        body.lines().map(str::trim).filter(|l| !l.starts_with("//") && l.contains(sep))
            .map(|l| l.split(sep).next().unwrap().trim().to_string()).collect()
    };
    assert_eq!(fields(rust, "pub(super) struct GpuAmbience {", ":"), fields(wgsl, "struct Ambience {", ":"));
}

#[test]
fn nebula_fog_takes_the_zones_distances_from_the_camera_and_intensity_as_its_most() {
    // Kingshill's gate zone: FogIntensity 0.8, the running client sends fogColor.w
    // = 0.2 (the clear floor) and fogDistances 25 / 75, unshifted.
    let gate = Bubble { fog_color: [0.749, 0.9059, 1.0, 1.0], fog_intensity: 0.8, fog_near_dist: 25.0, fog_far_dist: 75.0, ..default() };
    let gpu = GpuAmbience::default().with_fog(&gate, &Lighting::default());
    assert_eq!(gpu.fog_color, Vec4::new(0.749, 0.9059, 1.0, 1.0));
    near(gpu.fog_dist.truncate(), Vec3::new(25.0, 75.0, 0.8));
    // The panel's Fog x scales the most fog, Fog range x the distances.
    let half = GpuAmbience::default().with_fog(&gate, &Lighting { fog: 0.5, fog_range: 2.0, ..default() });
    near(half.fog_dist.truncate(), Vec3::new(50.0, 150.0, 0.4));
    // The inactive buffer keeps Bevy's fog.
    assert_eq!(GpuAmbience::default().fog_color.w, 0.0);
}

#[test]
fn full_shadow_keeps_half_the_light_as_the_blurred_shadow_buffer_does() {
    // Shadow pass 0, then two blurs of 1 - 0.8 x 0.883 x (1 - x): 1 - 0.7063^2.
    assert!((super::shadow_floor(1.0) - 0.5011).abs() < 1e-3);
    // Beyond 1 the darkening cannot go below the pass's 0.
    assert_eq!(super::shadow_floor(3.0), super::shadow_floor(1.0));
    // Softer shadows for lower intensities (mapping UNVERIFIED), none at 0.
    assert!(super::shadow_floor(0.25) > super::shadow_floor(0.5));
    assert_eq!(super::shadow_floor(0.0), 1.0);
}

#[test]
fn render_switches_reach_the_gpu_the_frame_they_change() {
    use super::super::{AoMode, RenderSwitches};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Ambience>().init_resource::<GpuAmbience>()
        .init_resource::<Lighting>().init_resource::<GlobalAmbientLight>()
        .insert_resource(RenderSwitches { gamma_light: true, ao: AoMode::Linear, shadow_floor: false, rim: true, gamma_scene: true })
        .insert_resource(Level::default())
        .add_systems(Update, apply_look);
    app.update();
    let gpu = app.world().resource::<GpuAmbience>().clone();
    assert_eq!((gpu.gamma_light.x, gpu.gamma_light.w, gpu.ambient_gamma.w), (1.0, 0.0, 0.0));
    assert_eq!(gpu.output.x, 1.0, "the gamma scene target");
    *app.world_mut().resource_mut::<RenderSwitches>() = RenderSwitches { gamma_light: false, ao: AoMode::Clamped, shadow_floor: true, rim: false, gamma_scene: false };
    app.update();
    let gpu = app.world().resource::<GpuAmbience>().clone();
    assert_eq!((gpu.gamma_light.x, gpu.gamma_light.w, gpu.ambient_gamma.w), (0.0, 1.0, 2.0));
    assert_eq!(gpu.output.x, 0.0, "Bevy's linear target");
    assert_eq!(gpu.rim_color, Vec4::ZERO);
    // Linear: Bevy's ambient light is back on, as before the gamma sum.
    assert!(app.world().resource::<GlobalAmbientLight>().brightness > 0.0);
}
