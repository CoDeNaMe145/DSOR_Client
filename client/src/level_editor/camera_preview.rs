//! Live, independent render of the selected fixed camera, composed over the view.
use super::*;
use bevy::camera::{visibility::RenderLayers, RenderTarget};
use bevy::render::render_resource::TextureFormat;

pub(super) const EDITOR_LAYER: usize = 30;
#[derive(Component)] pub(super) struct CameraMonitor;
#[derive(Component)] pub(super) struct Panel;
#[derive(Component)] pub(super) struct Picture;
#[derive(Component)] pub(super) struct Caption;
#[derive(Resource, Default)]
pub(super) struct Monitor {
    pub image: Handle<Image>, size: UVec2,
    pub state: Option<serde_json::Value>,
}

fn dimensions(window: &Window, aspect: f32) -> Vec2 {
    let width = (window.width() * 0.34).clamp(200., 400.).min((window.width() - 24.).max(1.));
    Vec2::new(width, width / aspect.clamp(0.25, 4.) + 30.)
}
pub(super) fn contains(editor: &Editor, window: &Window, cursor: Vec2) -> bool {
    if editor.desktop { return false; } // The Rust UI owns the monitor hit region.
    let Some(lens) = editor.selected().and_then(|p| p.object.camera.as_ref()) else { return false };
    let size = dimensions(window, lens.aspect);
    cursor.x >= window.width() - size.x - 12. && cursor.x <= window.width() - 12.
        && cursor.y >= 12. && cursor.y <= 12. + size.y
}

pub(super) fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut monitor: ResMut<Monitor>,
    view: Single<Entity, With<ViewCamera>>) {
    monitor.size = UVec2::new(640, 360);
    monitor.image = images.add(Image::new_target_texture(640, 360, TextureFormat::Rgba8UnormSrgb, None));
    commands.spawn((CameraMonitor, Camera3d::default(),
        Camera { order: -5, is_active: false, ..default() }, RenderTarget::Image(monitor.image.clone().into()),
        RenderLayers::layer(0), bevy::core_pipeline::tonemapping::Tonemapping::None,
        Msaa::Off, bevy::core_pipeline::prepass::DepthPrepass, bevy::anti_alias::fxaa::Fxaa::default(), Transform::default()));
    let panel = commands.spawn((Panel, Node { position_type: PositionType::Absolute,
        top: Val::Px(12.), right: Val::Px(12.), width: Val::Px(400.), flex_direction: FlexDirection::Column,
        padding: UiRect::all(Val::Px(2.)), display: Display::None, ..default() },
        UiTargetCamera(*view), BackgroundColor(Color::srgb(0.06, 0.075, 0.10)), GlobalZIndex(50), Pickable::IGNORE)).id();
    commands.spawn((Caption, Text::new("FIXED CAMERA"), TextFont { font_size: FontSize::Px(12.), ..default() },
        TextColor(Color::srgb(0.65, 0.8, 1.)), Node { height: Val::Px(26.), padding: UiRect::all(Val::Px(5.)), ..default() },
        ChildOf(panel), Pickable::IGNORE));
    commands.spawn((Picture, ImageNode::new(monitor.image.clone()),
        Node { width: Val::Percent(100.), height: Val::Px(225.), ..default() }, ChildOf(panel), Pickable::IGNORE));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(editor: Res<Editor>, windows: Query<&Window>, mut monitor: ResMut<Monitor>,
    mut images: ResMut<Assets<Image>>,
    roots: Query<&Transform, (With<ObjectRoot>, Without<CameraMonitor>)>,
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<CameraMonitor>>,
    mut panels: Query<&mut Node, (With<Panel>, Without<Picture>)>,
    mut pictures: Query<&mut Node, (With<Picture>, Without<Panel>)>, mut captions: Query<&mut Text, With<Caption>>) {
    let Ok((mut camera, mut at, mut projection)) = cameras.single_mut() else { return };
    let Ok(mut panel) = panels.single_mut() else { return };
    let selected = editor.selected().filter(|p| p.object.camera.is_some());
    camera.is_active = selected.is_some();
    panel.display = if selected.is_some() && !editor.desktop { Display::Flex } else { Display::None };
    monitor.state = None;
    let Some(placed) = selected else { return };
    let lens = placed.object.camera.as_ref().unwrap();
    let Ok(transform) = roots.get(placed.entity) else { return };
    *at = *transform;
    at.scale = Vec3::ONE;
    *projection = lens.projection();
    let size = UVec2::new(640, (640. / lens.aspect.clamp(0.25, 4.)).round() as u32);
    if size != monitor.size {
        if let Some(mut image) = images.get_mut(&monitor.image) {
            *image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
        }
        monitor.size = size;
    }
    if let Ok(window) = windows.single() {
        let size = dimensions(window, lens.aspect);
        panel.width = Val::Px(size.x);
        if let Ok(mut picture) = pictures.single_mut() { picture.height = Val::Px((size.x - 4.) / lens.aspect); }
    }
    if let Ok(mut text) = captions.single_mut() {
        **text = format!("FIXED CAMERA  |  {:.1} deg  |  {:.2}:1", lens.fov, lens.aspect);
    }
    monitor.state = Some(json!({"key": placed.object.key, "position": at.translation.to_array(),
        "rotation": at.rotation.to_array(), "lens": lens}));
}

pub(super) fn draw(editor: Res<Editor>, roots: Query<&Transform, With<ObjectRoot>>, mut gizmos: Gizmos<Handles>) {
    for placed in editor.placed.values().filter(|p| p.object.camera.is_some() && editor.controls.visible(&p.object)) {
        let Ok(at) = roots.get(placed.entity) else { continue };
        let lens = placed.object.camera.as_ref().unwrap();
        let selected = editor.controls.selected.as_ref() == Some(&placed.object.key);
        let color = if selected { Color::srgb(1., 0.8, 0.3) } else { Color::srgb(0.3, 0.7, 1.) };
        let frame = Transform::from_translation(at.translation).with_rotation(at.rotation);
        gizmos.cube(frame.with_scale(Vec3::new(0.7, 0.5, 0.5)), color);
        let distance = if selected { 6. } else { 1.5 };
        let half_y = (lens.fov.to_radians() * 0.5).tan() * distance;
        let half_x = half_y * lens.aspect;
        let corners = [Vec3::new(-half_x, -half_y, -distance), Vec3::new(half_x, -half_y, -distance),
                       Vec3::new(half_x, half_y, -distance), Vec3::new(-half_x, half_y, -distance)]
            .map(|p| frame.transform_point(p));
        for i in 0..4 { gizmos.line(at.translation, corners[i], color); gizmos.line(corners[i], corners[(i+1)%4], color); }
        gizmos.line(frame.transform_point(Vec3::new(-0.25, 0.3, 0.)), frame.transform_point(Vec3::new(0., 0.65, 0.)), color);
        gizmos.line(frame.transform_point(Vec3::new(0., 0.65, 0.)), frame.transform_point(Vec3::new(0.25, 0.3, 0.)), color);
    }
}
