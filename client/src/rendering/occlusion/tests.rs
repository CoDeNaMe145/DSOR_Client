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
    assert_eq!(full_occlusion(&mut raycast,
        &MeshRayCastSettings::default().with_visibility(RayCastVisibility::Visible).never_early_exit(),
        Vec3::ZERO, Vec3::new(0.0, 1.0, 10.0), Vec3::X), HashSet::from([front, back]));
}

const TEST_CAMERA: Vec3 = Vec3::new(0.0, 3.0, 10.0);

fn sight_world() -> World {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let mut world = World::new();
    world.init_resource::<Assets<Mesh>>();
    world
}

fn obstacle(world: &mut World, size: Vec3, center: Vec3) -> Entity {
    let mesh = Mesh::from(Cuboid::from_size(size));
    let bounds = mesh.compute_aabb().unwrap();
    let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
    world.spawn((Mesh3d(mesh), bounds, GlobalTransform::from_translation(center),
        InheritedVisibility::VISIBLE, ViewVisibility::default(), RayCastBackfaces)).id()
}

fn reveal_set(world: &mut World) -> HashSet<Entity> {
    let mut state = SystemState::<MeshRayCast>::new(world);
    let mut raycast = state.get_mut(world).unwrap();
    full_occlusion(&mut raycast,
        &MeshRayCastSettings::default().with_visibility(RayCastVisibility::Visible).never_early_exit(),
        Vec3::ZERO, TEST_CAMERA, Vec3::X)
}

fn sample_blocked(world: &mut World, sample: usize) -> bool {
    let mut state = SystemState::<MeshRayCast>::new(world);
    let mut raycast = state.get_mut(world).unwrap();
    let delta = sight_targets(Vec3::ZERO, Vec3::X)[sample] - TEST_CAMERA;
    raycast.cast_ray(Ray3d::new(TEST_CAMERA, Dir3::new(delta).unwrap()),
        &MeshRayCastSettings::default().with_visibility(RayCastVisibility::Visible).never_early_exit())
        .iter().any(|(_, hit)| blocks_player(hit.distance, delta.length(), hit.point, Vec3::ZERO))
}

#[test]
fn low_furniture_does_not_reveal_when_the_head_is_visible() {
    for height in [0.9, 1.5] {
        let mut world = sight_world();
        obstacle(&mut world, Vec3::new(4.0, height, 0.4), Vec3::new(0.0, height * 0.5, 1.5));
        assert!(sample_blocked(&mut world, 2), "the prop must actually obscure the lower body");
        assert!(!sample_blocked(&mut world, 1), "the head must remain visible");
        assert!(reveal_set(&mut world).is_empty());
    }
}

#[test]
fn narrow_pillar_does_not_reveal_when_the_sides_are_visible() {
    let mut world = sight_world();
    obstacle(&mut world, Vec3::new(0.25, 4.0, 0.4), Vec3::new(0.0, 2.0, 1.5));
    for sample in 0..3 { assert!(sample_blocked(&mut world, sample)); }
    for sample in 3..5 { assert!(!sample_blocked(&mut world, sample)); }
    assert!(reveal_set(&mut world).is_empty());
}

#[test]
fn separate_meshes_can_jointly_occlude_the_whole_character() {
    let mut world = sight_world();
    let upper = obstacle(&mut world, Vec3::new(4.0, 2.0, 0.2), Vec3::new(0.0, 2.1, 1.5));
    assert!(sample_blocked(&mut world, 0));
    assert!(!sample_blocked(&mut world, 2));
    assert!(reveal_set(&mut world).is_empty());
    let lower = obstacle(&mut world, Vec3::new(4.0, 1.1, 0.2), Vec3::new(0.0, 0.55, 1.5));
    assert_eq!(reveal_set(&mut world), HashSet::from([upper, lower]));
}

#[test]
fn exposing_one_side_closes_an_existing_reveal() {
    let mut world = sight_world();
    let wall = obstacle(&mut world, Vec3::new(1.0, 4.0, 0.2), Vec3::new(0.0, 2.0, 1.5));
    let mut hold = 0.0;
    let amount = advance_fade(0.0, &mut hold, reveal_set(&mut world).contains(&wall), FADE_IN);
    assert_eq!(amount, 1.0);

    world.entity_mut(wall).insert(GlobalTransform::from_xyz(0.2, 2.0, 1.5));
    for sample in 0..4 { assert!(sample_blocked(&mut world, sample)); }
    assert!(!sample_blocked(&mut world, 4));
    let hits = reveal_set(&mut world);
    assert!(hits.is_empty());
    assert_eq!(advance_fade(amount, &mut hold, hits.contains(&wall), HOLD_SECONDS + FADE_OUT), 0.0);
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

#[test]
fn a_blocking_visibility_group_fades_as_a_whole_and_others_stay() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).insert_resource(ShadersReady(true)).init_resource::<GroupFades>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(50)))
        .init_resource::<Assets<Mesh>>().init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<NebulaMaterial>>().init_resource::<Assets<CutoutMaterial>>()
        .init_resource::<Assets<CutoutSurfaceMaterial>>()
        .add_systems(Update, fade_groups);
    app.world_mut().spawn((Camera3d::default(), GlobalTransform::from_translation(TEST_CAMERA)));
    app.world_mut().spawn((GlobalTransform::IDENTITY, crate::network::net::LocalPlayer::default()));
    let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
    let mut member = |center: Vec3, group: u32| {
        let world = app.world_mut();
        let entity = obstacle(world, Vec3::new(4.0, 4.0, 0.2), center);
        world.entity_mut(entity).insert((VisibilityGroup(group), MeshMaterial3d(material.clone())));
        entity
    };
    let blocking = member(Vec3::new(0.0, 2.0, 5.0), 1);
    let same_group_aside = member(Vec3::new(20.0, 2.0, 5.0), 1);
    let other_group = member(Vec3::new(-20.0, 2.0, 5.0), 2);
    for _ in 0..10 { app.update(); }
    for entity in [blocking, same_group_aside] {
        assert!(app.world().get::<MeshMaterial3d<CutoutMaterial>>(entity).is_some(), "the whole group fades");
    }
    assert_eq!(app.world().get::<MeshMaterial3d<StandardMaterial>>(other_group).unwrap().0, material);
    // The camera moves away from behind the wall: the group comes back.
    let camera = app.world_mut().query_filtered::<Entity, With<Camera3d>>().single(app.world()).unwrap();
    app.world_mut().entity_mut(camera).insert(GlobalTransform::from_xyz(0.0, 3.0, -10.0));
    for _ in 0..20 { app.update(); }
    for entity in [blocking, same_group_aside] {
        assert_eq!(app.world().get::<MeshMaterial3d<StandardMaterial>>(entity).unwrap().0, material);
        assert!(app.world().get::<Occluded>(entity).is_none());
    }
}
