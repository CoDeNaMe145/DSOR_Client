//! Small, isolated renders for the desktop object browser. No map edits.
use super::*;
use bevy::camera::{visibility::RenderLayers, RenderTarget};
use bevy::render::{render_resource::TextureFormat, view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk}};

const LAYER: usize = 31;
const ORIGIN: Vec3 = Vec3::new(0., -10000., 0.);
#[derive(Component)] pub(super) struct PreviewCamera;
#[derive(Component)] pub(super) struct PreviewRoot;
#[derive(Clone, Deserialize)]
pub(super) struct Request { pub model: String, pub key: Option<String>, pub path: PathBuf }
struct Job { request: Request, root: Entity, started: f32, ready_since: Option<f32>, captured: bool }
#[derive(Resource, Default)]
pub(super) struct Previews {
    stamp: Option<SystemTime>, poll_at: f32, pub queue: Vec<Request>, job: Option<Job>, image: Handle<Image>,
}

pub(super) fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut previews: ResMut<Previews>) {
    previews.image = images.add(Image::new_target_texture(256, 256, TextureFormat::Rgba8UnormSrgb, None));
    commands.spawn((PreviewCamera, crate::rendering::lighting::NoLevelLook, Camera3d::default(),
        AmbientLight { color: Color::srgb(0.75, 0.82, 0.93), brightness: 650., ..default() },
        Camera { order: -10, is_active: false, clear_color: ClearColorConfig::Custom(Color::srgb(0.065, 0.095, 0.13)), ..default() },
        RenderTarget::Image(previews.image.clone().into()), RenderLayers::layer(LAYER),
        bevy::core_pipeline::tonemapping::Tonemapping::None, Msaa::Off,
        bevy::core_pipeline::prepass::DepthPrepass, bevy::anti_alias::fxaa::Fxaa::default(), Transform::default()));
    commands.spawn((DirectionalLight { illuminance: 12000., shadow_maps_enabled: false, ..default() },
        RenderLayers::layer(LAYER), Transform::default().looking_to(Vec3::new(-0.7, -1., -0.5), Vec3::Y)));
}

// RenderLayers are not inherited. Scene/character children arrive asynchronously.
pub(super) fn isolate(mut commands: Commands, roots: Query<Entity, With<PreviewRoot>>, children: Query<&Children>, layers: Query<&RenderLayers>) {
    for root in &roots {
        for entity in std::iter::once(root).chain(children.iter_descendants(root)) {
            if layers.get(entity).is_ok_and(|layer| *layer == RenderLayers::layer(LAYER)) { continue; }
            commands.entity(entity).insert(RenderLayers::layer(LAYER));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(mut commands: Commands, mut previews: ResMut<Previews>, mut editor: ResMut<Editor>,
    time: Res<Time>, assets: Res<AssetServer>, children: Query<&Children>,
    meshes: Query<(&Mesh3d, &GlobalTransform)>, mesh_assets: Res<Assets<Mesh>>,
    characters: Query<&crate::actors::character::Character>,
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<PreviewCamera>>) {
    let now = time.elapsed_secs();
    let Ok((mut camera, mut camera_at, mut projection)) = cameras.single_mut() else { return };
    if now >= previews.poll_at {
        previews.poll_at = now + 0.2;
        let path = editor.directory.join("previews.json");
        let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if stamp.is_some() && stamp != previews.stamp {
            if let Some(queue) = std::fs::read(path).ok().and_then(|raw| serde_json::from_slice::<Vec<Request>>(&raw).ok()) {
                previews.stamp = stamp;
                previews.queue = queue;
            }
        }
    }
    if let Some(mut job) = previews.job.take() {
        if (job.captured && job.request.path.is_file()) || now - job.started > 20. {
            if !job.request.path.is_file() { let _ = std::fs::write(job.request.path.with_extension("failed"), b"Preview unavailable"); }
            commands.entity(job.root).despawn();
            camera.is_active = false;
        } else {
            let mut low = Vec3::splat(f32::INFINITY);
            let mut high = Vec3::splat(f32::NEG_INFINITY);
            let mut ready = true;
            if let Some(model) = editor.models.get(&job.request.model) {
                ready &= model.handle.as_ref().is_none_or(|handle| assets.is_loaded_with_dependencies(handle));
            }
            for entity in children.iter_descendants(job.root) {
                if let Ok(character) = characters.get(entity) { ready &= character.outfit_ready(); }
                let Ok((mesh, at)) = meshes.get(entity) else { continue };
                let Some(bounds) = mesh_assets.get(&mesh.0).and_then(|mesh| mesh.compute_aabb()) else { continue };
                for x in [-1., 1.] { for y in [-1., 1.] { for z in [-1., 1.] {
                    let point = at.transform_point(Vec3::from(bounds.center) + Vec3::from(bounds.half_extents) * Vec3::new(x, y, z));
                    low = low.min(point); high = high.max(point);
                } } }
            }
            if ready && low.is_finite() && high.is_finite() {
                let center = (low + high) * 0.5;
                let radius = ((high - low) * 0.5).length().max(0.05);
                *camera_at = Transform::from_translation(center + Vec3::new(1., 0.75, 1.3).normalize() * radius * 3.5).looking_at(center, Vec3::Y);
                *projection = Projection::Perspective(PerspectiveProjection { fov: 0.75, near: radius * 0.01, far: radius * 12., ..default() });
                let since = job.ready_since.get_or_insert(now);
                // Let textures, skinned poses and render pipelines settle before readback.
                if !job.captured && now - *since > 0.65 {
                    let path = job.request.path.clone();
                    let mut save = save_to_disk(path.clone());
                    commands.spawn(Screenshot::image(previews.image.clone())).observe(
                        move |captured: On<ScreenshotCaptured>, mut previews: ResMut<Previews>| {
                            // Asset readiness precedes GPU pipeline compilation. Never
                            // cache an empty first frame; retry while the job is alive.
                            let visible = captured.image.data.as_ref().is_some_and(|data|
                                data.len() >= 4 && data.chunks_exact(4).any(|pixel|
                                    pixel[..3].iter().zip(&data[..3]).any(|(a, b)| a.abs_diff(*b) > 8)));
                            if visible { save(captured); }
                            else if let Some(job) = previews.job.as_mut().filter(|job| job.request.path == path) {
                                job.captured = false;
                                job.ready_since = None;
                            }
                        });
                    job.captured = true;
                }
            } else { job.ready_since = None; }
            previews.job = Some(job);
            return;
        }
    }
    let Some(request) = previews.queue.iter().find(|request| !request.path.is_file() && !request.path.with_extension("failed").is_file()).cloned() else { return };
    let character = request.key.as_ref().and_then(|key| editor.placed.get(key)).and_then(|placed| placed.object.character.clone());
    if character.is_none() {
        let model = editor.models.entry(request.model.clone()).or_insert(Model { handle: None, visual: None, bounds: (Vec3::ZERO, Vec3::ONE) });
        match model.visual {
            None => return,
            Some(Visual::Missing) => {
                let _ = std::fs::write(request.path.with_extension("failed"), b"Model unavailable");
                return;
            }
            _ => {}
        }
    }
    let root = commands.spawn((PreviewRoot, Transform::from_translation(ORIGIN), Visibility::Inherited, RenderLayers::layer(LAYER))).id();
    if let Some(character) = character {
        let mut animation = crate::actors::character::CharacterAnim::default();
        animation.state = crate::actors::character::AnimState::Named(character.state);
        let desc = crate::actors::character::CharacterDesc { look: Some(crate::actors::character::NpcLook {
            sound_profile: String::new(),
            skeleton: character.skeleton, parts: character.parts, anim_set: character.anim_set,
            variation: character.variation.into_iter().map(|v| (v.bone, Vec3::from(v.t), Vec3::from(v.s))).collect(),
        }), ..default() };
        let child = crate::actors::character::spawn_character(&mut commands, desc, Transform::IDENTITY);
        commands.entity(child).insert((ChildOf(root), RenderLayers::layer(LAYER), crate::screens::selection::Preview, animation));
    } else if let Some(model) = editor.models.get(&request.model) {
        match model.visual.as_ref().unwrap() {
            Visual::Flat(surfaces) => for surface in surfaces {
                let mut child = commands.spawn((Mesh3d(surface.mesh.clone()), Transform::from_matrix(Mat4::from(surface.local)),
                    ChildOf(root), RenderLayers::layer(LAYER)));
                surface.material.insert(&mut child);
            },
            Visual::Scene(scene) => { commands.spawn((WorldAssetRoot(scene.clone()), Transform::IDENTITY, ChildOf(root), RenderLayers::layer(LAYER))); }
            Visual::Missing => {}
        }
    }
    camera.is_active = true;
    previews.job = Some(Job { request, root, started: now, ready_since: None, captured: false });
}
