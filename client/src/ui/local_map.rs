//! The original location-map widget has no bitmap: its content is drawn by
//! the game. Use the loaded walkable geometry and the real player's position.
use super::*;
use crate::nav::{CurrentNav, NavMesh};

#[derive(Component)]
pub(super) struct MapGeometry {
    source: AssetId<NavMesh>,
    min: Vec2,
    max: Vec2,
    origin: Vec2,
    scale: f32,
}

impl MapGeometry {
    fn project(&self, position: Vec3) -> Vec2 {
        self.origin + Vec2::new(position.x - self.min.x, self.max.y - position.z) * self.scale
    }
}

#[derive(Component)]
pub(super) struct PlayerMarker;

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    nav: Option<Res<CurrentNav>>,
    navigation: Res<Assets<NavMesh>>,
    screen: Res<Screen>,
    layouts: Res<Assets<Layout>>,
    loaded: Res<Loaded>,
    roots: Query<(Entity, &Window2018)>,
    geometry: Query<(Entity, &MapGeometry)>,
    mut markers: Query<&mut Transform, With<PlayerMarker>>,
    players: Query<&Transform, (With<crate::net::LocalPlayer>, Without<PlayerMarker>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
) {
    let Some(nav) = nav else { return };
    let Some(mesh) = navigation.get(&nav.0) else { return };
    let Some((root, _)) = roots.iter().find(|(_, w)| w.0 == "locationmap") else { return };
    if let Some((entity, map)) = geometry.iter().next() {
        if map.source == nav.0.id() {
            if let Some(player) = players.iter().next() {
                let p = map.project(player.translation);
                for mut marker in &mut markers { marker.translation = p.extend(1.0); }
            }
            return;
        }
        commands.entity(entity).despawn();
    }
    let Some(widget) = loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h))
        .find(|l| l.window == "locationmap").and_then(|l| l.widgets.iter().find(|w| w.id == "Canvas/map")) else { return };
    if mesh.triangles.is_empty() { return; }
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    for p in mesh.triangles.iter().flatten() {
        let p = Vec2::new(p.x, p.z);
        min = min.min(p);
        max = max.max(p);
    }
    let [l, t, r, b] = widget.rect;
    let size = Vec2::new(r - l - 24.0, b - t - 52.0);
    let extent = (max - min).max(Vec2::ONE);
    let scale = (size / extent).min_element();
    let origin = Vec2::new(l + 12.0, t + 40.0) + (size - extent * scale) * 0.5;
    let map = MapGeometry { source: nav.0.id(), min, max, origin, scale };
    let mut art = Art { tex: None, pos: Vec::new(), uv: Vec::new(), color: Vec::new(), idx: Vec::new() };
    for triangle in &mesh.triangles {
        for p in triangle {
            let point = map.project(*p);
            art.idx.push(art.idx.len() as u32);
            art.pos.extend([point.x, point.y]);
            art.uv.extend([0.0, 0.0]);
            art.color.extend([0.54, 0.47, 0.33, 1.0]);
        }
    }
    let mut run = Run::default();
    run.push(&art, 0.0);
    let parent = commands.spawn((map, Follows { k: widget.k, flip: true }, follow_transform(&screen, widget.k, true),
        Visibility::Inherited, ChildOf(root))).id();
    commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(materials.add(material(None))),
        Transform::from_xyz(0.0, 0.0, 0.03), Visibility::Inherited, RenderLayers::layer(LAYER), ChildOf(parent)));
    let mut marker = Run::default();
    marker.push(&Art { tex: None, pos: vec![0.0,-6.0,5.0,4.0,-5.0,4.0], uv: vec![0.0; 6],
        color: [1.0,0.85,0.2,1.0].repeat(3), idx: vec![0,1,2] }, 0.0);
    commands.spawn((PlayerMarker, Mesh2d(meshes.add(marker.mesh())), MeshMaterial2d(materials.add(material(None))),
        Transform::from_xyz(origin.x, origin.y, 1.0), Visibility::Inherited, RenderLayers::layer(LAYER), ChildOf(parent)));
}
