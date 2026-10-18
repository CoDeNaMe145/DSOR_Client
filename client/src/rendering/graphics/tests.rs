use super::*;

#[test]
fn preferences_migrate_and_round_trip() {
    let old: GraphicsSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(old, GraphicsSettings::default());
    let changed = GraphicsSettings { anti_aliasing: AntiAliasing::SmaaHigh, shadows: Quality::Low,
        shadow_filter: ShadowFilter::Fast, ambient_occlusion: Quality::Off, sharpening: 63,
        bloom: false, vignette: false };
    let restored: GraphicsSettings = serde_json::from_str(&serde_json::to_string(&changed).unwrap()).unwrap();
    assert_eq!(restored, changed);
}

#[test]
fn camera_switches_remove_old_effects_and_preserve_auxiliary_views() {
    let mut app = App::new();
    app.init_resource::<GraphicsSettings>().init_resource::<DirectionalLightShadowMap>()
        .add_systems(Update, apply_cameras);
    let camera = app.world_mut().spawn((Camera3d::default(), GameGraphics)).id();
    let auxiliary = app.world_mut().spawn((Camera3d::default(), Fxaa::default(), Msaa::Sample4)).id();
    app.update();
    assert!(app.world().get::<Fxaa>(camera).is_some());
    assert_eq!(app.world().get::<Msaa>(camera), Some(&Msaa::Off));

    {
        let mut graphics = app.world_mut().resource_mut::<GraphicsSettings>();
        graphics.anti_aliasing = AntiAliasing::SmaaHigh;
        graphics.sharpening = 63;
        graphics.shadows = Quality::Low;
        graphics.ambient_occlusion = Quality::Off;
        graphics.shadow_filter = ShadowFilter::Fast;
    }
    app.update();
    assert!(app.world().get::<Fxaa>(camera).is_none());
    assert!(app.world().get::<Smaa>(camera).unwrap().preset == SmaaPreset::High);
    assert_eq!(app.world().get::<ContrastAdaptiveSharpening>(camera).unwrap().sharpening_strength, 0.63);
    assert_eq!(app.world().resource::<DirectionalLightShadowMap>().size, 512);
    assert!(matches!(app.world().get::<ShadowFilteringMethod>(camera), Some(ShadowFilteringMethod::Hardware2x2)));
    assert!(app.world().get::<bevy::pbr::ScreenSpaceAmbientOcclusion>(camera).is_none());
    assert!(app.world().get::<bevy::core_pipeline::prepass::NormalPrepass>(camera).is_none());
    assert!(app.world().get::<Fxaa>(auxiliary).is_some());
    assert_eq!(app.world().get::<Msaa>(auxiliary), Some(&Msaa::Sample4));

    {
        let mut graphics = app.world_mut().resource_mut::<GraphicsSettings>();
        graphics.anti_aliasing = AntiAliasing::Off;
        graphics.sharpening = 0;
        graphics.ambient_occlusion = Quality::High;
    }
    app.update();
    assert!(app.world().get::<Smaa>(camera).is_none());
    assert!(app.world().get::<ContrastAdaptiveSharpening>(camera).is_none());
    assert_eq!(app.world().get::<bevy::pbr::ScreenSpaceAmbientOcclusion>(camera).unwrap().quality_level,
        bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::High);
    assert!(app.world().get::<bevy::core_pipeline::prepass::NormalPrepass>(camera).is_some());

    // A replacement game camera inherits preferences even without another Apply.
    let replacement = app.world_mut().spawn((Camera3d::default(), GameGraphics)).id();
    app.update();
    assert!(app.world().get::<Fxaa>(replacement).is_none());
    assert_eq!(app.world().get::<bevy::pbr::ScreenSpaceAmbientOcclusion>(replacement).unwrap().quality_level,
        bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::High);
}
