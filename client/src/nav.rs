//! The map's walkable ground: PathEngine's navigation mesh, the one the 2018 client
//! walks on and the server spawns on (tools/export_navmesh.py writes
//! `maps/<map>.nav.bin` from `navigation/<map>/<map>.tok`).
//!
//! It gives the player their height (the server's elevations are approximate: the
//! Kingshill arrival is 1.2 units under the ground it stands on) and their collisions:
//! a step that would leave the mesh is not taken.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;

/// Grid cell size of the lookup index, world units.
const BUCKET: f32 = 4.0;

#[derive(Asset, TypePath, Debug)]
pub struct NavMesh {
    pub triangles: Vec<[Vec3; 3]>,
    grid: HashMap<(i32, i32), Vec<u32>>,
    /// Each triangle's connected walkable component (triangles sharing an edge),
    /// as the server's PathEngine reader groups them: ground in another one (a
    /// closed room, a decorative island) cannot be walked to.
    component: Vec<u32>,
}

fn cell(v: f32) -> i32 {
    (v / BUCKET).floor() as i32
}

impl NavMesh {
    pub fn new(triangles: Vec<[Vec3; 3]>) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in triangles.iter().enumerate() {
            let (min_x, max_x) = (t[0].x.min(t[1].x).min(t[2].x), t[0].x.max(t[1].x).max(t[2].x));
            let (min_z, max_z) = (t[0].z.min(t[1].z).min(t[2].z), t[0].z.max(t[1].z).max(t[2].z));
            for cx in cell(min_x)..=cell(max_x) {
                for cz in cell(min_z)..=cell(max_z) {
                    grid.entry((cx, cz)).or_default().push(i as u32);
                }
            }
        }
        let component = components(&triangles);
        Self { triangles, grid, component }
    }

    /// The triangle under (x, z) whose height is nearest `p.y` (within `max_step`),
    /// optionally only in `component`.
    fn triangle_at(&self, p: Vec3, max_step: f32, component: Option<u32>) -> Option<(u32, f32)> {
        self.grid
            .get(&(cell(p.x), cell(p.z)))
            .into_iter()
            .flatten()
            .filter(|&&i| component.is_none_or(|c| self.component[i as usize] == c))
            .filter_map(|&i| height_in(&self.triangles[i as usize], p.x, p.z).map(|h| (i, h)))
            .filter(|(_, h)| (h - p.y).abs() <= max_step)
            .min_by(|a, b| (a.1 - p.y).abs().total_cmp(&(b.1 - p.y).abs()))
    }

    /// The component of the ground nearest `p` (within `radius`).
    pub fn component_at(&self, p: Vec3, radius: f32) -> Option<u32> {
        let q = self.nearest(p, radius)?;
        self.triangle_at(q, f32::MAX, None).map(|(i, _)| self.component[i as usize])
            .or_else(|| {
                // q on an edge: any triangle touching it.
                self.grid.get(&(cell(q.x), cell(q.z))).into_iter().flatten()
                    .find(|&&i| self.triangles[i as usize].iter().any(|v| v.distance(q) < 1e-3)
                        || on_edges(&self.triangles[i as usize], q))
                    .map(|&i| self.component[i as usize])
            })
    }

    /// Where a skill aimed from `from` at `to` may land: `to` if it is on ground
    /// the caster can walk to, else the farthest such point back along the line,
    /// or None when there is none (the server refuses the rest, services/
    /// movement_validation.py). Without the caster's component, `to` as it is.
    pub fn reachable_along(&self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let Some(home) = self.component_at(from, 3.0) else { return Some(to) };
        let flat = Vec2::new(to.x - from.x, to.z - from.z);
        let length = flat.length();
        let steps = (length / 0.5).ceil().max(1.0) as i32;
        for i in (0..=steps).rev() {
            let f = i as f32 / steps as f32;
            let p = from.lerp(to, f);
            // Any height of the caster's own ground under this point (stairs,
            // a ledge): the nearest to the line.
            if let Some((_, h)) = self.triangle_at(p, f32::MAX, Some(home)) {
                return Some(Vec3::new(p.x, h, p.z));
            }
            if i == 0 {
                break;
            }
        }
        None
    }

    /// Every ground height under (x, z), from every overlapping level.
    pub fn heights(&self, x: f32, z: f32) -> impl Iterator<Item = f32> + '_ {
        self.grid
            .get(&(cell(x), cell(z)))
            .into_iter()
            .flatten()
            .filter_map(move |&i| height_in(&self.triangles[i as usize], x, z))
    }

    /// The ground at (x, z) on the level nearest `near` and within `max_step` of it.
    pub fn ground(&self, x: f32, z: f32, near: f32, max_step: f32) -> Option<f32> {
        self.heights(x, z)
            .filter(|h| (h - near).abs() <= max_step)
            .min_by(|a, b| (a - near).abs().total_cmp(&(b - near).abs()))
    }

    /// The nearest point on the mesh to `p` (horizontally) within `radius`.
    pub fn nearest(&self, p: Vec3, radius: f32) -> Option<Vec3> {
        if let Some(h) = self.ground(p.x, p.z, p.y, f32::MAX) {
            return Some(Vec3::new(p.x, h, p.z));
        }
        let mut best: Option<(f32, Vec3)> = None;
        for cx in cell(p.x - radius)..=cell(p.x + radius) {
            for cz in cell(p.z - radius)..=cell(p.z + radius) {
                for &i in self.grid.get(&(cx, cz)).into_iter().flatten() {
                    let t = &self.triangles[i as usize];
                    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        let q = closest_on_segment(a, b, p);
                        let d = Vec2::new(q.x - p.x, q.z - p.z).length_squared() + (q.y - p.y).abs() * 0.01;
                        if d <= radius * radius && best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, q));
                        }
                    }
                }
            }
        }
        best.map(|(_, q)| q)
    }

    /// Where a ray first meets the ground.
    pub fn raycast(&self, ray: Ray3d) -> Option<Vec3> {
        let mut best: Option<f32> = None;
        for t in &self.triangles {
            if let Some(d) = ray_triangle(ray, t) {
                if best.is_none_or(|b| d < b) {
                    best = Some(d);
                }
            }
        }
        best.map(|d| ray.get_point(d))
    }
}

fn on_edges(t: &[Vec3; 3], q: Vec3) -> bool {
    [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])].iter().any(|&(a, b)| {
        let c = closest_on_segment(a, b, q);
        Vec2::new(c.x - q.x, c.z - q.z).length() < 1e-3
    })
}

/// Connected components over shared edges (vertices matched to 1/64 unit).
fn components(triangles: &[[Vec3; 3]]) -> Vec<u32> {
    let mut parent: Vec<u32> = (0..triangles.len() as u32).collect();
    fn find(parent: &mut [u32], mut i: u32) -> u32 {
        while parent[i as usize] != i {
            parent[i as usize] = parent[parent[i as usize] as usize];
            i = parent[i as usize];
        }
        i
    }
    let key = |v: Vec3| ((v.x * 64.0).round() as i64, (v.y * 64.0).round() as i64, (v.z * 64.0).round() as i64);
    let mut edges: HashMap<((i64, i64, i64), (i64, i64, i64)), u32> = HashMap::new();
    for (i, t) in triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (key(a), key(b));
            let e = if ka < kb { (ka, kb) } else { (kb, ka) };
            match edges.get(&e) {
                Some(&j) => {
                    let (ri, rj) = (find(&mut parent, i as u32), find(&mut parent, j));
                    if ri != rj {
                        parent[ri as usize] = rj;
                    }
                }
                None => {
                    edges.insert(e, i as u32);
                }
            }
        }
    }
    (0..triangles.len() as u32).map(|i| find(&mut parent, i)).collect()
}

fn closest_on_segment(a: Vec3, b: Vec3, p: Vec3) -> Vec3 {
    let ab = Vec2::new(b.x - a.x, b.z - a.z);
    let len = ab.length_squared();
    let t = if len == 0.0 { 0.0 } else { (Vec2::new(p.x - a.x, p.z - a.z).dot(ab) / len).clamp(0.0, 1.0) };
    a + (b - a) * t
}

/// The height of triangle `t` at (x, z), when (x, z) is inside it.
fn height_in(t: &[Vec3; 3], x: f32, z: f32) -> Option<f32> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let d = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
    if d.abs() < 1e-9 {
        return None;
    }
    let w1 = ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / d;
    let w2 = ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / d;
    let w3 = 1.0 - w1 - w2;
    const E: f32 = -1e-4;
    (w1 >= E && w2 >= E && w3 >= E).then(|| w1 * a.y + w2 * b.y + w3 * c.y)
}

/// Möller-Trumbore, both faces.
fn ray_triangle(ray: Ray3d, t: &[Vec3; 3]) -> Option<f32> {
    let dir: Vec3 = *ray.direction;
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let s = ray.origin - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let d = e2.dot(q) * inv;
    (d > 0.0).then_some(d)
}

#[derive(Default, TypePath)]
pub struct NavMeshLoader;

impl AssetLoader for NavMeshLoader {
    type Asset = NavMesh;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<NavMesh, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        if b.len() < 4 {
            return Err(std::io::Error::other("navigation mesh too short"));
        }
        let n = u32::from_le_bytes(b[0..4].try_into().unwrap()) as usize;
        if b.len() < 4 + n * 36 {
            return Err(std::io::Error::other("navigation mesh truncated"));
        }
        let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let triangles = (0..n)
            .map(|i| {
                let at = 4 + i * 36;
                [0, 1, 2].map(|k| Vec3::new(f(at + k * 12), f(at + k * 12 + 4), f(at + k * 12 + 8)))
            })
            .collect();
        Ok(NavMesh::new(triangles))
    }
    fn extensions(&self) -> &[&str] {
        &["nav.bin"]
    }
}

/// The current map's mesh, once the map is known.
#[derive(Resource)]
pub struct CurrentNav(pub Handle<NavMesh>);

/// F6 or Ctrl+Shift+N (Cmd+Shift+N): the walkable ground drawn as wireframe around the player (or the camera's
/// focus), one colour per connected component: where the player may walk and
/// where skills may land (NavMesh::reachable_along).
#[derive(Resource, Default)]
pub struct NavDebug(pub bool);

fn toggle_nav_debug(keys: Res<ButtonInput<KeyCode>>, mut debug: ResMut<NavDebug>) {
    // Ctrl+Shift+N too: a Mac keyboard sends F keys only with fn (or not at all).
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::F6) || (ctrl && shift && keys.just_pressed(KeyCode::KeyN)) {
        debug.0 = !debug.0;
        let on = debug.0;
        info!("navigation mesh debug {}", if on { "on" } else { "off" });
    }
}

fn draw_nav_debug(
    debug: Res<NavDebug>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
    players: Query<&GlobalTransform, With<crate::net::LocalPlayer>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::highlight::HighlightCamera>)>,
    mut gizmos: Gizmos,
) {
    if !debug.0 {
        return;
    }
    let Some(mesh) = nav.as_ref().and_then(|n| meshes.get(&n.0)) else { return };
    let centre = players.iter().next().map(|p| p.translation())
        .or_else(|| cameras.iter().next().map(|c| c.translation()))
        .unwrap_or(Vec3::ZERO);
    const RADIUS: f32 = 35.0;
    let lift = Vec3::Y * 0.06;
    let here = mesh.component_at(centre, 3.0);
    for (i, t) in mesh.triangles.iter().enumerate() {
        let mid = (t[0] + t[1] + t[2]) / 3.0;
        if Vec2::new(mid.x - centre.x, mid.z - centre.z).length() > RADIUS {
            continue;
        }
        let c = mesh.component[i];
        // The player's component green; the others in colours of their own.
        let color = if Some(c) == here {
            Color::srgba(0.2, 1.0, 0.3, 0.9)
        } else {
            Color::hsla((c.wrapping_mul(2_654_435_761) % 360) as f32, 0.9, 0.55, 0.9)
        };
        gizmos.line(t[0] + lift, t[1] + lift, color);
        gizmos.line(t[1] + lift, t[2] + lift, color);
        gizmos.line(t[2] + lift, t[0] + lift, color);
    }
}

pub struct NavPlugin;

impl Plugin for NavPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<NavMesh>()
            .register_asset_loader(NavMeshLoader)
            .add_systems(Update, follow_map.run_if(resource_exists::<crate::map::CurrentMap>))
            .init_resource::<NavDebug>()
            .add_systems(Update, (toggle_nav_debug, draw_nav_debug).chain());
    }
}

fn follow_map(
    mut commands: Commands,
    current: Res<crate::map::CurrentMap>,
    nav: Option<Res<CurrentNav>>,
    assets: Res<AssetServer>,
    mut loaded_for: Local<String>,
) {
    if *loaded_for == current.name && nav.is_some() {
        return;
    }
    *loaded_for = current.name.clone();
    commands.insert_resource(CurrentNav(assets.load(format!("maps/{}.nav.bin", current.name))));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10 x 10 square of two triangles at (x0, 0).
    fn square(x0: f32) -> [[Vec3; 3]; 2] {
        let (a, b, c, d) = (Vec3::new(x0, 0.0, 0.0), Vec3::new(x0 + 10.0, 0.0, 0.0), Vec3::new(x0 + 10.0, 0.0, 10.0), Vec3::new(x0, 0.0, 10.0));
        [[a, b, c], [a, c, d]]
    }

    #[test]
    fn a_point_across_the_void_lands_on_the_casters_side() {
        // Two islands, the gap between x = 10 and x = 20.
        let mut tris: Vec<[Vec3; 3]> = square(0.0).to_vec();
        tris.extend(square(20.0));
        let mesh = NavMesh::new(tris);
        assert_ne!(mesh.component_at(Vec3::new(5.0, 0.0, 5.0), 1.0), mesh.component_at(Vec3::new(25.0, 0.0, 5.0), 1.0));
        // Aimed on the other island: back to the edge of the caster's.
        let p = mesh.reachable_along(Vec3::new(5.0, 0.0, 5.0), Vec3::new(25.0, 0.0, 5.0)).unwrap();
        assert!((p.x - 10.0).abs() <= 0.5 && p.x <= 10.0 + 1e-3, "{p:?}");
        // Aimed on its own island: where it was aimed.
        let q = mesh.reachable_along(Vec3::new(5.0, 0.0, 5.0), Vec3::new(8.0, 0.0, 9.0)).unwrap();
        assert!(q.distance(Vec3::new(8.0, 0.0, 9.0)) < 1e-3);
    }
}
