//! Project notice and repository link, shared by every account-screen state.
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;

use super::view::{text, Art, GOLD, IVORY, MUTED};

const REPOSITORY: &str = "https://github.com/DSO-Reconstruction/DSOR_Client";

#[derive(Component, Default)]
pub(super) struct RepositoryLink {
    hover: f32,
    elapsed: f32,
    armed: bool,
}
#[derive(Component)] pub(super) struct Face;
#[derive(Component)] pub(super) struct Mark;
#[derive(Component)] pub(super) struct Label;
#[derive(Component)] pub(super) struct Underline;

pub(super) fn icon(images: &mut Assets<Image>) -> Handle<Image> {
    // Embed the licensed Octicons mark so it is also available in offline builds.
    images.add(Image::from_buffer(include_bytes!("icons/github.png"), ImageType::Extension("png"),
        CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::default())
        .expect("embedded GitHub mark is a valid PNG"))
}

pub(super) fn spawn(parent: &mut ChildSpawnerCommands, art: &Art, icon: &Handle<Image>) {
    parent.spawn((Name::new("Project notice"), Node {
        position_type: PositionType::Absolute, bottom: px(10), width: percent(100),
        min_height: px(40), padding: UiRect::horizontal(px(16)), column_gap: px(16),
        justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default()
    })).with_children(|footer| {
        footer.spawn((text(art, "A non-profit, open-source project.", 13.0, IVORY),
            TextLayout::justify(Justify::Center), Node { max_width: percent(65), ..default() }));
        // The hit area stays still while the visual face lifts, avoiding edge flicker.
        footer.spawn((Button, RepositoryLink::default(), Name::new("Open project source on GitHub"),
            Node { width: px(116), height: px(40), flex_shrink: 0.0, ..default() }))
            .with_children(|button| {
                button.spawn((Face, Node { width: percent(100), height: percent(100),
                    justify_content: JustifyContent::Center, align_items: AlignItems::Center,
                    column_gap: px(9), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(4)), ..default() },
                    BackgroundColor(Color::srgba(0.025, 0.027, 0.03, 0.65)),
                    BorderColor::all(GOLD.with_alpha(0.3)), BoxShadow::default(), Pickable::IGNORE))
                    .with_children(|face| {
                        face.spawn((Mark, ImageNode { image: icon.clone(), color: IVORY, ..default() },
                            Node { width: px(22), height: px(22), ..default() }, Pickable::IGNORE));
                        face.spawn((Label, text(art, "GitHub", 13.0, MUTED), Pickable::IGNORE));
                        face.spawn((Underline, Node { position_type: PositionType::Absolute,
                            left: px(12), bottom: px(3), width: px(0), height: px(1), ..default() },
                            BackgroundColor(GOLD), Pickable::IGNORE));
                    });
            });
    });
}

pub(super) fn animate(time: Res<Time>, mouse: Res<ButtonInput<MouseButton>>,
    mut links: Query<(&Interaction, &mut RepositoryLink)>,
    mut faces: Query<(&mut UiTransform, &mut BorderColor, &mut BoxShadow), With<Face>>,
    mut marks: Query<(&mut UiTransform, &mut ImageNode), (With<Mark>, Without<Face>)>,
    mut labels: Query<&mut TextColor, With<Label>>,
    mut lines: Query<&mut Node, With<Underline>>) {
    let Ok((interaction, mut link)) = links.single_mut() else { return; };
    let active = *interaction != Interaction::None;
    let pressed = *interaction == Interaction::Pressed;
    // Activate once on release inside the button; dragging away cancels the click.
    if link.armed && *interaction == Interaction::Hovered && mouse.just_released(MouseButton::Left) {
        open_repository();
    }
    link.armed = pressed;
    let dt = time.delta_secs().min(0.1);
    link.elapsed = if active { link.elapsed + dt } else { 0.0 };
    link.hover += (if active { 1.0 } else { 0.0 } - link.hover) * (1.0 - (-14.0 * dt).exp());
    let glow = link.hover;
    for (mut transform, mut border, mut shadow) in &mut faces {
        transform.translation = Val2::px(0.0, if pressed { 0.0 } else { -2.0 * glow });
        *border = BorderColor::all(GOLD.with_alpha(0.3 + 0.6 * glow));
        *shadow = BoxShadow(vec![ShadowStyle { color: GOLD.with_alpha(0.3 * glow),
            x_offset: px(0), y_offset: px(0), spread_radius: px(2.0 * glow), blur_radius: px(14), }]);
    }
    for (mut transform, mut image) in &mut marks {
        transform.scale = Vec2::splat(if pressed { 0.96 } else { 1.0 + 0.12 * glow });
        // One damped flourish per hover, then settle instead of spinning forever.
        transform.rotation = Rot2::radians((link.elapsed * 19.0).sin() * (-4.0 * link.elapsed).exp() * 0.28 * glow);
        image.color = IVORY.mix(&GOLD, glow);
    }
    for mut color in &mut labels { color.0 = MUTED.mix(&IVORY, glow); }
    for mut node in &mut lines { node.width = px(90.0 * glow); }
}

fn open_repository() {
    // Browser startup must not stall rendering or an in-flight sign-in request.
    if let Err(error) = std::thread::Builder::new().name("project-link".into()).spawn(|| {
        match launch_browser() {
            Ok(()) => info!("Opened project repository: {REPOSITORY}"),
            Err(error) => warn!("Could not open {REPOSITORY}: {error}"),
        }
    }) { warn!("Could not start browser task: {error}"); }
}

#[cfg(target_os = "windows")]
fn launch_browser() -> Result<(), String> {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(window: *mut std::ffi::c_void, operation: *const u16, file: *const u16,
            parameters: *const u16, directory: *const u16, show: i32) -> isize;
    }
    let url: Vec<u16> = REPOSITORY.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the URL is a live NUL-terminated UTF-16 string; optional pointers
    // are null. Use Windows' registered HTTPS handler without a command shell.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), std::ptr::null(), url.as_ptr(),
        std::ptr::null(), std::ptr::null(), 1) };
    if result > 32 { Ok(()) } else { Err(format!("Windows browser error {result}")) }
}

#[cfg(not(target_os = "windows"))]
fn launch_browser() -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let status = std::process::Command::new(opener).arg(REPOSITORY).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("{opener} exited with {status}")) }
}
