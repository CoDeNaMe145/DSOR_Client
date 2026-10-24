//! Account UI built from the same atlases as the in-game windows. Atlas crops
//! stay in source pixels; nine-slicing preserves the bevels at every window size.
use super::{Action, Form, Page, Root};
use bevy::prelude::*;
use bevy::sprite::{BorderRect, TextureSlicer};
use bevy::window::PrimaryWindow;

pub(super) const GOLD: Color = Color::srgb(0.85, 0.73, 0.48);
pub(super) const IVORY: Color = Color::srgb(0.96, 0.91, 0.79);
pub(super) const MUTED: Color = Color::srgb(0.65, 0.65, 0.61);
pub(super) const ERROR: Color = Color::srgb(1.0, 0.59, 0.49);
const PANEL_WIDTH: f32 = 480.0;

#[derive(Resource)]
pub(super) struct Art {
    backdrop: Handle<Image>,
    stone: Handle<Image>,
    decor: Handle<Image>,
    frame: Handle<Image>,
    primary: Handle<Image>,
    progress: Handle<Image>,
    github: Handle<Image>,
    font: Handle<Font>,
}

#[derive(Component)] pub(super) struct Backdrop;
#[derive(Component)] pub(super) struct Card;
#[derive(Component)] pub(super) struct Hero;
#[derive(Component)] pub(super) struct Field(usize);
#[derive(Component)] pub(super) struct FieldText(usize);
#[derive(Component)] pub(super) struct Status;
#[derive(Component)] pub(super) struct StatusMark;
#[derive(Component)] pub(super) struct RevealLabel;
#[derive(Component)] pub(super) struct Sweep;
#[derive(Component)] pub(super) struct ButtonSkin(bool);

pub(super) fn load(mut commands: Commands, assets: Res<AssetServer>, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(Art {
        backdrop: assets.load("textures/ui_chapterartworks/act_00_crunch.png"),
        stone: assets.load("textures/ui/struktur.png"),
        decor: assets.load("textures/ui/deco_elemente.png"),
        frame: assets.load("textures/ui/deco_elemente_5.png"),
        primary: assets.load("textures/ui/charselectionwindow_button_01_crunch.png"),
        progress: assets.load("textures/ui/statusbar_connenctscreen_new.png"),
        github: super::footer::icon(&mut images),
        font: assets.load("fonts/tahoma.ttf"),
    });
}

pub(super) fn preload(art: Res<Art>, mut loading: ResMut<crate::screens::loading::LoadingScreen>) {
    loading.begin_assets("Preparing account screen...", vec![
        art.backdrop.clone().untyped(), art.stone.clone().untyped(), art.decor.clone().untyped(),
        art.frame.clone().untyped(), art.primary.clone().untyped(),
        art.progress.clone().untyped(), art.font.clone().untyped(),
    ]);
}

pub(super) fn text(art: &Art, value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(value), TextFont { font: art.font.clone().into(), font_size: FontSize::Px(size), ..default() },
        TextColor(color), TextShadow { offset: Vec2::new(0.0, 1.0), color: Color::BLACK })
}

fn slice(image: &Handle<Image>, rect: Rect, border: f32) -> ImageNode {
    ImageNode { image: image.clone(), rect: Some(rect), visual_box: bevy::ui::VisualBox::BorderBox,
        image_mode: NodeImageMode::Sliced(TextureSlicer {
            border: BorderRect::all(border), max_corner_scale: 1.0, ..default()
        }), ..default() }
}

fn fill() -> Node {
    Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }
}

fn ornament(parent: &mut ChildSpawnerCommands, art: &Art, rect: Rect, node: Node, flip_x: bool, flip_y: bool) {
    parent.spawn((node, ImageNode { image: art.frame.clone(), rect: Some(rect), flip_x, flip_y,
        image_mode: NodeImageMode::Stretch, ..default() }, Pickable::IGNORE));
}

/// The original social-window frame's corner, rails and central metal rivet.
/// Mirroring its corner avoids stretching the engraved metal over the panel.
fn frame(parent: &mut ChildSpawnerCommands, art: &Art) {
    for bottom in [false, true] {
        for right in [false, true] {
            ornament(parent, art, Rect::new(43.0, 128.0, 78.0, 162.0), Node {
                position_type: PositionType::Absolute, width: px(35), height: px(34),
                left: if right { Val::Auto } else { px(0) }, right: if right { px(0) } else { Val::Auto },
                top: if bottom { Val::Auto } else { px(0) }, bottom: if bottom { px(0) } else { Val::Auto }, ..default()
            }, right, bottom);
        }
        ornament(parent, art, Rect::new(82.0, 135.0, 240.0, 146.0), Node {
            position_type: PositionType::Absolute, left: px(34), right: px(34), height: px(11),
            top: if bottom { Val::Auto } else { px(7) }, bottom: if bottom { px(7) } else { Val::Auto }, ..default()
        }, false, bottom);
    }
    for right in [false, true] {
        ornament(parent, art, Rect::new(49.0, 168.0, 61.0, 351.0), Node {
            position_type: PositionType::Absolute, width: px(12), top: px(33), bottom: px(33),
            left: if right { Val::Auto } else { px(6) }, right: if right { px(6) } else { Val::Auto }, ..default()
        }, right, false);
    }
    ornament(parent, art, Rect::new(249.0, 128.0, 279.0, 155.0), Node {
        position_type: PositionType::Absolute, width: px(30), height: px(27),
        left: px(PANEL_WIDTH / 2.0 - 15.0), bottom: px(-5), ..default()
    }, false, true);
}

fn button(parent: &mut ChildSpawnerCommands, art: &Art, label: &str, action: Action, primary: bool) {
    let image = if primary {
        slice(&art.primary, Rect::new(0.0, 0.0, 256.0, 92.0), 25.0)
    } else {
        slice(&art.frame, Rect::new(181.0, 397.0, 331.0, 461.0), 12.0)
    };
    parent.spawn((Button, action, ButtonSkin(primary), image, Node {
        width: percent(100), min_width: px(0), height: px(if primary { 54 } else { 42 }),
        flex_basis: px(0), flex_grow: 1.0, flex_shrink: 0.0,
        justify_content: JustifyContent::Center, align_items: AlignItems::Center,
        padding: UiRect::axes(px(12), px(7)), ..default()
    })).with_children(|b| { b.spawn(text(art, label, if primary { 17.0 } else { 14.0 }, IVORY)); });
}

fn field(parent: &mut ChildSpawnerCommands, art: &Art, i: usize) {
    let label = ["Username", "Password", "Confirm password", "Account server", "New character name (optional)"][i];
    parent.spawn(Node { width: percent(100), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: px(7), ..default() })
        .with_children(|group| {
            group.spawn(Node { width: percent(100), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() })
                .with_children(|row| {
                    row.spawn(text(art, label, 14.0, GOLD));
                    if i == 1 {
                        row.spawn((Button, Action::Reveal, Node { padding: UiRect::horizontal(px(6)), ..default() }))
                            .with_children(|b| { b.spawn((RevealLabel, text(art, "Show", 12.0, MUTED))); });
                    }
                });
            // Social window's InputQueryBackground: a recessed, bevelled field.
            group.spawn((Button, Action::Field(i), Field(i),
                slice(&art.decor, Rect::new(12.0, 114.0, 87.0, 188.0), 12.0),
                Node { width: percent(100), height: px(46), padding: UiRect::axes(px(16), px(8)),
                    align_items: AlignItems::Center, overflow: Overflow::clip(), ..default() }))
                .with_children(|f| { f.spawn((FieldText(i), text(art, "", 16.0, IVORY))); });
        });
}

fn panel_height(form: &Form) -> f32 {
    if form.busy() { 443.0 } else {
        match form.page { Page::Login => 563.0, Page::Register => 647.0, Page::Settings => 723.0 }
    }
}

pub(super) fn draw(mut commands: Commands, mut form: ResMut<Form>, art: Res<Art>, roots: Query<Entity, With<Root>>) {
    if !form.redraw { return; }
    form.redraw = false;
    for entity in &roots { commands.entity(entity).despawn(); }
    commands.spawn((Root, Name::new("Account screen"), fill(), BackgroundColor(Color::srgb(0.025, 0.031, 0.04)), GlobalZIndex(10000)))
        .with_children(|root| {
            // The chapter atlas has 100 unused transparent rows below the artwork.
            root.spawn((Backdrop, ImageNode { image: art.backdrop.clone(), image_mode: NodeImageMode::Stretch,
                rect: Some(Rect::new(0.0, 0.0, 1024.0, 410.0)), ..default() }, fill(), Pickable::IGNORE));
            root.spawn((fill(), BackgroundColor(Color::srgba(0.015, 0.02, 0.03, 0.28)), Pickable::IGNORE));
            root.spawn((Hero, Node { position_type: PositionType::Absolute, left: percent(7), top: percent(62),
                width: percent(43), flex_direction: FlexDirection::Column, row_gap: px(12), ..default() }))
                .with_children(|hero| {
                    hero.spawn(text(&art, "D R A K E N S A N G   O N L I N E", 17.0, GOLD));
                    hero.spawn(text(&art, "Welcome to Dracania", 36.0, IVORY));
                    hero.spawn(text(&art, "Your journey continues.", 18.0, MUTED));
                });
            root.spawn((Card, Name::new("Account panel"), Node {
                position_type: PositionType::Absolute, width: px(PANEL_WIDTH), height: px(panel_height(&form)),
                padding: UiRect::all(px(32)), flex_direction: FlexDirection::Column, row_gap: px(14),
                ..default()
            }, ImageNode { image: art.stone.clone(), color: Color::srgb(0.25, 0.24, 0.21),
                image_mode: NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 1.0 },
                visual_box: bevy::ui::VisualBox::BorderBox, ..default() }))
                .with_children(|card| {
                    frame(card, &art);
                    let (title, subtitle) = if form.busy() {
                        ("Opening the gates", "Please wait while we prepare your journey.")
                    } else { match form.page {
                        Page::Login => ("Welcome back", "Sign in to continue your adventure."),
                        Page::Register => ("Begin your journey", "Create your account and enter Dracania."),
                        Page::Settings => ("Connection settings", "Choose your server and character defaults."),
                    } };
                    card.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(8), ..default() }).with_children(|header| {
                        header.spawn(text(&art, title, 26.0, IVORY));
                        header.spawn(text(&art, subtitle, 14.0, MUTED));
                    });
                    if !form.busy() {
                        for &i in form.fields() { field(card, &art, i); }
                        if form.page == Page::Settings {
                            card.spawn(text(&art, "Server format: host:port, e.g. 127.0.0.1:8080", 12.0, MUTED));
                            card.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(8), ..default() }).with_children(|defaults| {
                                defaults.spawn(text(&art, "NEW CHARACTER", 12.0, GOLD));
                                defaults.spawn(Node { width: percent(100), column_gap: px(10), ..default() }).with_children(|row| {
                                    button(row, &art, ["Dragonknight", "Spellweaver", "Ranger", "Steam Mechanicus"][form.class as usize], Action::Class, false);
                                    button(row, &art, ["Male", "Female"][form.gender as usize], Action::Gender, false);
                                });
                                defaults.spawn(text(&art, "Used only if your account has no character.", 12.0, MUTED));
                            });
                        }
                    } else {
                        card.spawn((Node { width: percent(100), height: px(22), margin: UiRect::vertical(px(18)), overflow: Overflow::clip(), ..default() },
                            slice(&art.progress, Rect::new(0.0, 28.0, 953.0, 61.0), 9.0)))
                            .with_children(|track| {
                                track.spawn((Sweep, Node { position_type: PositionType::Absolute, top: px(4), width: percent(24), height: px(14), ..default() },
                                    slice(&art.progress, Rect::new(0.0, 0.0, 953.0, 28.0), 9.0)));
                            });
                    }
                    card.spawn((Node { width: percent(100), min_height: px(64), padding: UiRect::all(px(12)), column_gap: px(10),
                        align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.28))))
                        .with_children(|status| {
                            status.spawn((StatusMark, Node { width: px(14), height: px(14), flex_shrink: 0.0, ..default() },
                                ImageNode { image: art.decor.clone(), image_mode: NodeImageMode::Stretch,
                                    rect: Some(Rect::new(453.0, 404.0, 482.0, 432.0)), ..default() }));
                            status.spawn((Status, text(&art, &form.status, 13.0, MUTED), Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), ..default() }));
                            if form.page == Page::Login && !form.busy() {
                                status.spawn((Button, Action::Retry, Node { padding: UiRect::axes(px(3), px(8)), ..default() }))
                                    .with_children(|b| { b.spawn(text(&art, "Refresh", 12.0, GOLD)); });
                            }
                        });
                    card.spawn(Node { width: percent(100), height: px(54), flex_shrink: 0.0,
                        margin: UiRect::top(Val::Auto), ..default() }).with_children(|row| {
                        button(row, &art, if form.busy() { "Cancel sign-in" } else { match form.page {
                            Page::Login => "SIGN IN & PLAY", Page::Register => "CREATE ACCOUNT & PLAY", Page::Settings => "SAVE SETTINGS",
                        } }, if form.busy() { Action::Cancel } else { Action::Play }, true);
                    });
                    if !form.busy() {
                        card.spawn(Node { width: percent(100), height: px(42), flex_shrink: 0.0, column_gap: px(10), ..default() }).with_children(|row| {
                            if form.page == Page::Login {
                                button(row, &art, "Create account", Action::Register, false);
                                button(row, &art, "Settings", Action::Settings, false);
                            } else { button(row, &art, "Back to sign in", Action::Back, false); }
                        });
                    }
                    card.spawn((text(&art, "Your password is never saved on this device.", 11.0, MUTED), Node { align_self: AlignSelf::Center, ..default() }));
                });
            super::footer::spawn(root, &art, &art.github);
        });
}

pub(super) fn resize(form: Res<Form>, windows: Query<&Window, With<PrimaryWindow>>,
    mut nodes: ParamSet<(Query<(&mut Node, &mut UiTransform), With<Card>>, Query<&mut Node, With<Backdrop>>, Query<&mut Node, With<Hero>>)>) {
    let Ok(window) = windows.single() else { return; };
    let (w, h) = (window.width(), window.height());
    let height = panel_height(&form);
    let scale = ((w - 32.0) / PANEL_WIDTH).min((h - 104.0) / height).clamp(0.1, 1.0);
    let wide = w >= 1100.0 && w / h >= 1.5;
    for (mut node, mut transform) in &mut nodes.p0() {
        node.left = px((w - PANEL_WIDTH) * 0.5 + if wide { w * 0.235 } else { 0.0 });
        node.top = px((h - height) * 0.5 - 14.0);
        transform.scale = Vec2::splat(scale);
    }
    // Cover using the artwork's occupied atlas region, without distorting it.
    let aspect = 1024.0 / 410.0;
    let background_width = w.max(h * aspect);
    for mut node in &mut nodes.p1() {
        node.width = px(background_width); node.height = px(background_width / aspect);
        node.left = px((w - background_width) / 2.0); node.top = px((h - background_width / aspect) / 2.0);
    }
    for mut node in &mut nodes.p2() { node.display = if wide { Display::Flex } else { Display::None }; }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(form: Res<Form>, time: Res<Time>,
    mut texts: Query<(&FieldText, &mut Text, &mut TextColor)>,
    mut fields: Query<(&Field, &Interaction, &mut ImageNode), Without<ButtonSkin>>,
    mut buttons: Query<(&ButtonSkin, &Interaction, &mut ImageNode), Without<Field>>,
    mut status: Query<(&mut Text, &mut TextColor), (With<Status>, Without<FieldText>, Without<StatusMark>, Without<RevealLabel>)>,
    mut marks: Query<&mut ImageNode, (With<StatusMark>, Without<Field>, Without<ButtonSkin>)>,
    mut reveal: Query<&mut Text, (With<RevealLabel>, Without<FieldText>)>,
    mut sweeps: Query<&mut Node, With<Sweep>>, data: Option<Res<super::game_data::GameData>>) {
    // Play waits for the game data (super::game_data): its button looks it.
    let locked = !form.busy() && form.page != Page::Settings && data.is_some_and(|d| !d.ready());
    for (field, mut text, mut color) in &mut texts {
        let input = &form.inputs[field.0];
        let focus = form.focus == field.0;
        **text = if input.value.is_empty() && !focus {
            ["Enter your username", "Enter your password", "Re-enter your password", "127.0.0.1:8080", "Use your account name"][field.0].into()
        } else { input.display((field.0 == 1 || field.0 == 2) && !form.reveal_password,
            focus && (time.elapsed_secs() * 2.0) as u32 % 2 == 0) };
        color.0 = if input.value.is_empty() && !focus { MUTED } else { IVORY };
    }
    for (field, interaction, mut image) in &mut fields {
        image.color = if form.focus == field.0 {
            if form.inputs[field.0].selected { Color::srgb(0.55, 0.65, 0.85) } else { Color::srgb(0.86, 0.73, 0.47) }
        } else if *interaction == Interaction::Hovered { Color::srgb(0.65, 0.62, 0.54) }
        else { Color::srgb(0.46, 0.46, 0.43) };
    }
    for (skin, interaction, mut image) in &mut buttons {
        image.rect = Some(if skin.0 {
            if *interaction == Interaction::Hovered { Rect::new(0.0, 92.0, 256.0, 184.0) }
            else { Rect::new(0.0, 0.0, 256.0, 92.0) }
        } else { match interaction {
            Interaction::Hovered => Rect::new(11.0, 397.0, 162.0, 461.0),
            Interaction::Pressed => Rect::new(351.0, 397.0, 501.0, 461.0),
            Interaction::None => Rect::new(181.0, 397.0, 331.0, 461.0),
        } });
        image.color = if skin.0 && locked { Color::srgb(0.45, 0.45, 0.45) }
            else if *interaction == Interaction::Pressed { Color::srgb(0.72, 0.72, 0.72) } else { Color::WHITE };
    }
    for (mut text, mut color) in &mut status {
        **text = form.status.clone(); color.0 = if form.error { ERROR } else { MUTED };
    }
    for mut image in &mut marks {
        image.rect = Some(if form.error { Rect::new(453.0, 437.0, 482.0, 465.0) }
            else if form.status.ends_with("is online") { Rect::new(453.0, 404.0, 482.0, 432.0) }
            else { Rect::new(453.0, 363.0, 482.0, 391.0) });
        image.color = if form.job.is_some() || form.connecting.is_some() { GOLD } else { Color::WHITE };
    }
    for mut text in &mut reveal { **text = if form.reveal_password { "Hide" } else { "Show" }.into(); }
    for mut node in &mut sweeps { node.left = percent((time.elapsed_secs() * 2.0).sin() * 38.0 + 38.0); }
}
