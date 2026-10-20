//! Click-through terrain overlay: compact upper-left, expanded centre, then off.
//! Transparency lives in the debug menu until a settings window is available.
use super::*;
use crate::world::nav::{CurrentNav, NavMesh};

mod markers;
pub(crate) use markers::{MapIcon, MapLabel, MapMarkers};
pub(super) use markers::update as update_markers;

#[derive(Resource, serde::Serialize, Deserialize)]
pub(crate) struct MapSettings {
    /// 0 = opaque lines, 100 = invisible terrain. Controls and player stay legible.
    transparency: u8,
    #[serde(skip)]
    dirty: bool,
}

impl Default for MapSettings {
    fn default() -> Self { Self { transparency: 45, dirty: false } }
}

impl MapSettings {
    pub(super) fn load() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let raw = std::fs::read_to_string("map-settings.json").ok();
        #[cfg(target_arch = "wasm32")]
        let raw = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item("dsor.map").ok().flatten());
        let mut settings: Self = raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
        settings.transparency = settings.transparency.min(100);
        settings
    }

    pub(crate) fn transparency(&self) -> u8 { self.transparency }

    pub(crate) fn adjust(&mut self, delta: i8) {
        let next = (self.transparency as i16 + delta as i16).clamp(0, 100) as u8;
        if next != self.transparency {
            self.transparency = next;
            self.dirty = true;
        }
    }

    fn save(&self) {
        let Ok(raw) = serde_json::to_string(self) else { return };
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = std::fs::write("map-settings.json", raw) {
            warn!("Could not save map transparency: {error}");
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.set_item("dsor.map", &raw);
        }
    }
}

#[derive(Asset, TypePath, Deserialize)]
#[serde(transparent)]
pub struct MapNames(HashMap<String, String>);

impl MapNames {
    pub(in crate::ui) fn caption(&self, map: &str) -> String {
        self.0.get(map).filter(|name| !name.trim().is_empty()).cloned().unwrap_or_else(|| {
            // Debug/untranslated maps still get a readable caption, without
            // leaking the internal area prefix into the normal map UI.
            let words = map.split_once('_').filter(|(prefix, _)| prefix.starts_with('a') && prefix[1..].chars().all(|c| c.is_ascii_digit()))
                .map_or(map, |(_, name)| name).replace('_', " ");
            let mut chars = words.chars();
            chars.next().map(|first| first.to_uppercase().to_string() + chars.as_str()).unwrap_or_else(|| "Map".into())
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MapMode { Compact, Expanded }

#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    k: f32,
    origin: Vec2,
    size: Vec2,
}

impl View {
    fn new(mode: MapMode, width: f32) -> Self {
        match mode {
            MapMode::Compact => Self { k: 0.0, origin: Vec2::new(20.0, 108.0), size: Vec2::new(280.0_f32.min((width-40.0).max(40.0)), 230.0) },
            MapMode::Expanded => {
                let size = Vec2::new(680.0_f32.min((width-40.0).max(40.0)), 520.0);
                Self { k: 0.5, origin: Vec2::new((BASE_W-size.x)*0.5, 104.0), size }
            }
        }
    }
}

#[derive(Component)]
pub(super) struct MapCaption;

#[derive(Component)]
pub(super) struct MapTerrain;

pub(super) fn appearance(
    mut settings: ResMut<MapSettings>,
    terrain: Query<&MeshMaterial2d<UiMaterial>, With<MapTerrain>>,
    mut materials: ResMut<Assets<UiMaterial>>,
    current: Option<Res<crate::world::map::CurrentMap>>,
    names: Res<Assets<MapNames>>,
    loaded: Res<Loaded>,
    mut labels: Query<&mut Text2d, With<MapCaption>>,
) {
    // Also handles terrain/labels first appearing after asynchronous asset loads.
    let transparency = settings.transparency as f32 / 100.0;
    for handle in &terrain {
        if materials.get(&handle.0).is_some_and(|mat| mat.flags.y != transparency) {
            materials.get_mut(&handle.0).unwrap().flags.y = transparency;
        }
    }
    let fallback = MapNames(HashMap::new());
    let caption = current.as_ref().map(|m| names.get(&loaded.map_names).unwrap_or(&fallback).caption(&m.name)).unwrap_or_else(|| "Map".into());
    for mut label in &mut labels {
        if label.0 != caption { label.0.clone_from(&caption); }
    }
    if settings.dirty {
        settings.save();
        settings.dirty = false;
    }
}

#[derive(Component)]
pub(super) struct MapGeometry {
    source: AssetId<NavMesh>,
    view: View,
    projection: GroundProjection,
    min: Vec2,
    origin: Vec2,
    scale: f32,
}

/// Camera right/down projected onto the ground. Reference UI coordinates are
/// already Y-down; Follows performs their one conversion to the Y-up UI camera.
/// Treating +Z as map-up here mirrored the terrain relative to the world view.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GroundProjection {
    right: Vec2,
    down: Vec2,
}

impl GroundProjection {
    fn gameplay() -> Self {
        Self::from_rotation(Quat::from_euler(EulerRot::YXZ,
            crate::network::net::CAM_YAW_DEG.to_radians(), crate::network::net::CAM_PITCH_DEG.to_radians(), 0.0))
    }

    fn from_rotation(rotation: Quat) -> Self {
        Self { right: (rotation * Vec3::X).xz(), down: (rotation * Vec3::NEG_Y).xz() }
    }

    fn project(self, position: Vec3) -> Vec2 {
        Vec2::new(self.right.dot(position.xz()), self.down.dot(position.xz()))
    }
}

impl MapGeometry {
    fn project(&self, position: Vec3) -> Vec2 {
        self.origin + (self.projection.project(position) - self.min) * self.scale
    }
}

/// Weld duplicate vertices to millimetres, retaining height so bridges do not
/// cancel edges on the floor beneath them. Draw walls, never triangulation seams.
fn boundary_edges(triangles: &[[Vec3; 3]]) -> Vec<[Vec3; 2]> {
    let key = |p: Vec3| p.to_array().map(|v| (v * 1000.0).round() as i64);
    let mut edges = HashMap::new();
    for &[a, b, c] in triangles {
        for (a, b) in [(a, b), (b, c), (c, a)] {
            let (ka, kb) = (key(a), key(b));
            if ka == kb { continue; }
            let pair = if ka < kb { (ka, kb) } else { (kb, ka) };
            let entry = edges.entry(pair).or_insert((0, [a, b]));
            entry.0 += 1;
        }
    }
    // Stable order keeps overlapping joins deterministic across loads.
    let mut edges: Vec<_> = edges.into_iter().filter(|(_, (count, _))| *count == 1).collect();
    edges.sort_by_key(|(key, _)| *key);
    edges.into_iter().map(|(_, (_, edge))| edge).collect()
}

fn polygon(run: &mut Run, points: &[Vec2], color: [f32; 4]) {
    let base = run.pos.len() as u32;
    for p in points {
        run.pos.push([p.x, p.y, 0.0]);
        run.uv.push([0.0, 0.0]);
        run.color.push(color);
    }
    for i in 1..points.len().saturating_sub(1) {
        run.idx.extend([base, base + i as u32, base + i as u32 + 1]);
    }
}

fn line(run: &mut Run, a: Vec2, b: Vec2, width: f32, color: [f32; 4]) {
    let d = (b - a).normalize_or_zero();
    let n = Vec2::new(-d.y, d.x) * width * 0.5;
    polygon(run, &[a - n, b - n, b + n, a + n], color);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    nav: Option<Res<CurrentNav>>,
    navigation: Res<Assets<NavMesh>>,
    screen: Res<Screen>,
    assets: Res<AssetServer>,
    roots: Query<(Entity, &Window2018, &WindowControls)>,
    geometry: Query<(Entity, &MapGeometry)>,
    cameras: Query<&Transform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
) {
    let Some((root, _, controls)) = roots.iter().find(|(_, w, _)| w.0 == "locationmap") else { return };
    let view = View::new(controls.map_mode, screen.width);
    let projection = cameras.iter().next().map(|camera| GroundProjection::from_rotation(camera.rotation))
        // The scene camera starts unframed; do not flatten the map to a line.
        .filter(|p| p.right.perp_dot(p.down).abs() > 1.0e-4)
        .unwrap_or_else(GroundProjection::gameplay);
    let source = nav.as_ref().map(|n| n.0.id());
    for (entity, map) in &geometry {
        if Some(map.source) == source && map.view == view && map.projection == projection {
            return;
        }
        // Old geometry and caption leave together, including while a new zone loads.
        commands.entity(entity).despawn();
    }
    let Some(nav) = nav else { return };
    let Some(mesh) = navigation.get(&nav.0) else { return };
    if mesh.triangles.is_empty() { return; }
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    for p in mesh.triangles.iter().flatten() {
        let p = projection.project(*p);
        min = min.min(p);
        max = max.max(p);
    }
    let extent = (max - min).max(Vec2::ONE);
    let scale = (view.size / extent).min_element();
    let origin = view.origin + (view.size - extent * scale) * 0.5;
    let map = MapGeometry { source: nav.0.id(), view, projection, min, origin, scale };
    let mut terrain = Run::default();
    for triangle in &mesh.triangles {
        polygon(&mut terrain, &triangle.map(|p| map.project(p)), [1.0, 1.0, 1.0, 0.08]);
    }
    let edges = boundary_edges(&mesh.triangles);
    for (width, color) in [(3.4, [0.03, 0.03, 0.03, 0.65]), (1.25, [1.0, 1.0, 1.0, 1.0])] {
        for &[a, b] in &edges { line(&mut terrain, map.project(a), map.project(b), width, color); }
    }
    let caption_at = origin + extent * scale + Vec2::new(0.0, 10.0);
    let parent = commands.spawn((map, Transform::default(), Visibility::Inherited, ChildOf(root))).id();
    let art = commands.spawn((markers::MarkerLayer, Follows { k: view.k, flip: true }, follow_transform(&screen, view.k, true),
        Visibility::Inherited, ChildOf(parent))).id();
    commands.spawn((MapTerrain, Mesh2d(meshes.add(terrain.mesh())), MeshMaterial2d(materials.add(material(None))),
        Transform::from_xyz(0.0, 0.0, 0.03), Visibility::Inherited, RenderLayers::layer(LAYER), ChildOf(art)));
    let text_parent = commands.spawn((Follows { k: view.k, flip: false }, follow_transform(&screen, view.k, false),
        Visibility::Inherited, ChildOf(parent))).id();
    commands.spawn((MapCaption, Text2d::new(""),
        TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(14.0 * TEXT_SCALE * screen.scale), ..default() },
        TextColor(Color::WHITE),
        bevy::sprite::Text2dShadow { offset: Vec2::new(1.0, -1.0), color: Color::BLACK },
        Anchor(Vec2::new(0.5, 0.5)), TextLayout::justify(Justify::Right),
        bevy::text::TextBounds::new_horizontal(view.size.x * screen.scale),
        Transform::from_xyz(caption_at.x, -caption_at.y, 1.0).with_scale(Vec3::splat(1.0 / screen.scale)),
        Visibility::Inherited, RenderLayers::layer(LAYER), ChildOf(text_parent)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_and_marker_directions_match_the_scene_camera_in_both_views() {
        // Compare the final UI-space result (including its Y flip) against
        // actual camera-local directions, so a second inversion cannot pass.
        for camera in [
            Transform::from_xyz(0.0, 10.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
            Transform::from_rotation(Quat::from_euler(EulerRot::YXZ,
                crate::network::net::CAM_YAW_DEG.to_radians(), crate::network::net::CAM_PITCH_DEG.to_radians(), 0.0)),
            Transform::from_xyz(12.0, 15.0, -8.0).looking_at(Vec3::ZERO, Vec3::Y),
        ] {
            for mode in [MapMode::Compact, MapMode::Expanded] {
                let view = View::new(mode, BASE_W);
                let map = MapGeometry { source: Handle::<NavMesh>::default().id(), view,
                    projection: GroundProjection::from_rotation(camera.rotation),
                    min: Vec2::splat(-20.0), origin: view.origin, scale: 2.0 };
                let ui = follow_transform(&Screen::default(), view.k, true);
                let start = Vec3::new(3.0, 0.0, 5.0);
                for heading in 0..8 {
                    let rotation = Quat::from_rotation_y(heading as f32 * std::f32::consts::FRAC_PI_4);
                    let facing = rotation * Vec3::Z;
                    let expected = (camera.rotation.inverse() * facing).xy() * map.scale;
                    let rendered = ui.transform_point(map.project(start + facing).extend(0.0))
                        - ui.transform_point(map.project(start).extend(0.0));
                    assert!(rendered.xy().distance(expected) < 1.0e-4, "world and map must move in the same screen direction");
                    assert!(map.project(start).is_finite());
                }
            }
        }
    }

    #[test]
    fn map_views_anchor_to_corner_or_centre_at_different_aspect_ratios() {
        for width in [600.0, 1024.0, 1600.0, 2400.0] {
            let compact = View::new(MapMode::Compact, width);
            let expanded = View::new(MapMode::Expanded, width);
            assert_eq!(compact.k, 0.0);
            assert_eq!(compact.origin, Vec2::new(20.0, 108.0));
            let center = expanded.origin.x + expanded.size.x * 0.5 + expanded.k * (width-BASE_W);
            assert!((center-width*0.5).abs() < 0.01);
            assert!(compact.size.x < expanded.size.x && compact.size.y < expanded.size.y);
            assert!(expanded.origin.y + expanded.size.y + 24.0 < 680.0, "caption stays above the action bar");
        }
    }

    #[test]
    #[ignore = "requires locally converted game localization"]
    fn map_captions_use_localized_names_and_readable_fallbacks() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface/maps.names.json");
        let names: MapNames = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(names.caption("a0200_kingscity"), "Kingshill");
        assert_eq!(names.caption("a0302_wildforest"), "Wild Forest");
        assert_eq!(names.caption("debug"), "Debug");
        assert_eq!(names.caption("a9999_unknown_zone"), "Unknown zone");
    }

    #[test]
    fn walls_exclude_shared_seams_and_keep_separate_elevations() {
        let (a, b, c, d) = (Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Z, Vec3::Z);
        let triangles = [[a, b, c], [a + Vec3::splat(0.00001), c, d]];
        let walls = boundary_edges(&triangles);
        assert_eq!(walls.len(), 4, "the two floor triangles must not draw their diagonal");
        assert!(walls.iter().all(|e| !((e[0] - a).length() < 0.001 && e[1] == c)));
        let raised = triangles.map(|t| t.map(|p| p + Vec3::Y));
        assert_eq!(boundary_edges(&[triangles[0], triangles[1], raised[0], raised[1]]).len(), 8);
    }
}
