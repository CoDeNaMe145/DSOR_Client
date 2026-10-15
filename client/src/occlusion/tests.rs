use super::*;
use bevy::camera::primitives::MeshAabb;
use bevy::ecs::system::SystemState;

#[test]
fn raycasts_find_multiple_walls_but_reject_floor_and_background() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let mut world = World::new();
    world.init_resource::<Assets<Mesh>>();
    let wall = Mesh::from(Cuboid::new(4.0, 4.0, 0.2));
    let bounds = wall.compute_aabb().unwrap();
    let mesh = world.resource_mut::<Assets<Mesh>>().add(wall);
    let mut spawn = |z| world.spawn((Mesh3d(mesh.clone()), bounds, GlobalTransform::from_xyz(0.0, 1.0, z),
        InheritedVisibility::VISIBLE, ViewVisibility::default(), RayCastBackfaces)).id();
    let front = spawn(6.0);
    let back = spawn(3.0);
    let behind_player = spawn(-3.0);
    let mut state = SystemState::<MeshRayCast>::new(&mut world);
    let mut raycast = state.get_mut(&mut world).unwrap();
    let hits = raycast.cast_ray(Ray3d::new(Vec3::new(0.0, 1.0, 10.0), Dir3::NEG_Z),
        &MeshRayCastSettings::default().with_visibility(RayCastVisibility::Visible).never_early_exit());
    let blocked: HashSet<_> = hits.iter().filter(|(_, hit)| blocks_player(hit.distance, 10.0, hit.point, Vec3::ZERO))
        .map(|(e, _)| *e).collect();
    assert_eq!(blocked, HashSet::from([front, back]));
    assert!(!blocked.contains(&behind_player));
    assert!(!blocks_player(5.0, 10.0, Vec3::new(0.0, 0.05, 5.0), Vec3::ZERO));
    assert!(!blocks_player(9.95, 10.0, Vec3::Y, Vec3::ZERO));
}

#[test]
fn brief_clearance_does_not_flicker_and_leaving_restores_visibility() {
    let mut hold = 0.0;
    let mut amount = advance_fade(0.0, &mut hold, true, FADE_IN);
    assert_eq!(amount, 1.0);
    amount = advance_fade(amount, &mut hold, false, 1.0 / 60.0);
    assert_eq!(amount, 1.0);
    amount = advance_fade(amount, &mut hold, false, HOLD_SECONDS + FADE_OUT);
    assert_eq!(amount, 0.0);
}

#[test]
fn losing_player_restores_original_material_without_changing_shared_asset() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).insert_resource(ShadersReady(true))
        .init_resource::<Assets<Mesh>>().init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<NebulaMaterial>>().init_resource::<Assets<CutoutMaterial>>()
        .init_resource::<Assets<CutoutSurfaceMaterial>>()
        .add_systems(Update, reveal_player);
    let original = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.6, 0.8), ..default()
    });
    let material = app.world().resource::<Assets<StandardMaterial>>().get(&original).unwrap().clone();
    let cutout = app.world_mut().resource_mut::<Assets<CutoutMaterial>>().add(CutoutMaterial { base: material,
        extension: Cutout { params: default() } });
    let affected = app.world_mut().spawn((Candidate, MeshMaterial3d(cutout.clone()), Occluded {
        material: MaterialPair::Standard(original.clone(), cutout), amount: 0.0, hold: 0.0, params: default()
    })).id();
    let untouched = app.world_mut().spawn(MeshMaterial3d(original.clone())).id();
    app.update();
    assert!(app.world().get::<Occluded>(affected).is_none());
    assert!(app.world().get::<MeshMaterial3d<CutoutMaterial>>(affected).is_none());
    for entity in [affected, untouched] {
        assert_eq!(app.world().get::<MeshMaterial3d<StandardMaterial>>(entity).unwrap().0, original);
    }
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().get(&original).unwrap().base_color,
        Color::srgb(0.4, 0.6, 0.8));
}

#[test]
fn surface_shader_can_be_extended_without_changing_its_lighting() {
    let source = include_str!("../shaders/surface.wgsl");
    assert!(cutout_shader(source, "var pbr_input = pbr_input_from_standard_material(moved, is_front);",
        "in.world_position.xyz", "test.wgsl").is_some());
    assert!(cutout_shader("unknown entry point", "missing anchor", "in.world_position.xyz", "test.wgsl").is_none());
}
