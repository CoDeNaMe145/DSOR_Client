//! Click-through NPC name/profession preview at the top center of the HUD.
//! Shares the outline's picked entity, including its UI and monster priority.
use super::*;
use crate::actors::npc::{Npc, NpcVisualPending};
use crate::rendering::highlight::Highlighted;

// Full-screen reference: the preview sits below the navigation, with compact
// captions and the original interactinfo/LabelGradient's fading side edges.
const WIDTH: f32 = 232.0;
const HEIGHT: f32 = 80.0;
const TOP: f32 = 53.0;
const CENTER_X: f32 = BASE_W * 0.5 - 11.0;

fn backdrop() -> Mesh {
    // LabelGradient is 231.35 pixels wide, with a 94.99-pixel dark center.
    // Its top and bottom stay flat; only the left and right edges fade out.
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    for (x, alpha) in [(-WIDTH * 0.5, 0.0), (-47.5, 0.502), (47.5, 0.502), (WIDTH * 0.5, 0.0)] {
        positions.extend([[x, -HEIGHT, 0.0], [x, 0.0, 0.0]]);
        colors.extend([[0.0, 0.0, 0.0, alpha]; 2]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; 8])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(vec![0, 2, 1, 1, 2, 3, 2, 4, 3, 3, 4, 5, 4, 6, 5, 5, 6, 7]))
}

#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct Caption { profession: bool, size: f32 }

fn profession(npc: &Npc) -> &str {
    if !npc.profile.profession_title.is_empty() { return &npc.profile.profession_title; }
    // Older sidecars have only the internal service id. Never show that id as
    // a localized caption ("jewelry", "privileged_trader", etc.).
    match npc.profile.profession.as_str() {
        "monger" | "food_recycling" => "Merchant",
        "jewelry" => "Jeweler",
        "alchemist" => "Alchemist",
        "smith" | "simple_smith" => "Blacksmith",
        "priest" => "Priest",
        "banker" => "Banker",
        "tailor" => "Tailor",
        "essence" => "Essence Merchant",
        "equip" => "Equipment Merchant",
        "privileged_trader" => "Premium Merchant",
        "mount_trader" => "Mount Merchant",
        "gambler_trader" => "Gambler",
        "arena_merchant" => "Arena Merchant",
        "fragment_trader" => "Materi Fragment Trader",
        "event_manager" => "Event Merchant",
        "reward_messenger" => "Reward Messenger",
        "crafting" | "crafting_cake" => "Crafting Merchant",
        _ => "",
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn show(
    mut commands: Commands,
    highlighted: Res<Highlighted>,
    npcs: Query<(&Npc, &InheritedVisibility), Without<NpcVisualPending>>,
    screen: Res<Screen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui: Res<input::UiInput>,
    loading: Res<crate::screens::loading::LoadingScreen>,
    login: Option<Res<crate::LoginGate>>,
    selection: Option<Res<crate::screens::selection::Selection>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
    mut panels: Query<(&mut Visibility, &mut Transform), (With<Panel>, Without<Caption>)>,
    mut captions: Query<(&Caption, &mut Text2d, &mut TextFont, &mut Transform, &mut bevy::text::TextBounds, &mut bevy::sprite::Text2dShadow), Without<Panel>>,
) {
    let enabled = !ui.over_ui && windows.single().is_ok_and(|w| w.focused)
        && crate::screens::loading::gameplay_ready(login, loading, selection);
    let content = enabled.then_some(highlighted.0).flatten()
        .and_then(|(entity, _)| npcs.get(entity).ok())
        .filter(|(npc, visible)| visible.get() && !npc.profile.title.is_empty())
        .map(|(npc, _)| (npc.profile.title.as_str(), profession(npc)));
    // Reference-pixel placement scales with the HUD and remains centered at any
    // aspect ratio. Keep the backdrop below the top navigation's depth (30).
    let position = screen.world(0.5, CENTER_X, TOP).extend(25.0);
    if let Ok((mut visibility, mut at)) = panels.single_mut() {
        let wanted = if content.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted { *visibility = wanted; }
        if at.translation != position { at.translation = position; }
        let Some((name, profession)) = content else { return; };
        for (caption, mut text, mut font, mut transform, mut bounds, mut shadow) in &mut captions {
            let value = if caption.profession { profession } else { name };
            if text.0 != value { text.0 = value.to_owned(); }
            let size = bevy::text::FontSize::Px(caption.size * screen.scale);
            if font.font_size != size {
                font.font_size = size;
                transform.scale = Vec3::splat(1.0 / screen.scale);
                bounds.width = Some((WIDTH - 24.0) * screen.scale);
                shadow.offset = Vec2::new(1.0, -1.0) * screen.scale;
            }
        }
        return;
    }
    let Some((name, profession)) = content else { return; };
    let root = commands.spawn((Panel, Name::new("NPC hover preview"),
        Transform::from_translation(position), Visibility::Inherited, Pickable::IGNORE)).id();
    commands.spawn((Mesh2d(meshes.add(backdrop())), MeshMaterial2d(materials.add(material(None))),
        RenderLayers::layer(LAYER), ChildOf(root), Pickable::IGNORE));
    for (is_profession, value, size, y) in [(false, name, 20.0 * TEXT_SCALE, -24.0), (true, profession, 15.0 * TEXT_SCALE, -52.0)] {
        commands.spawn((Caption { profession: is_profession, size }, Text2d::new(value),
            TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(size * screen.scale), ..default() },
            TextColor(Color::WHITE), TextLayout::justify(Justify::Center),
            bevy::text::TextBounds::new_horizontal((WIDTH - 24.0) * screen.scale),
            bevy::sprite::Text2dShadow { offset: Vec2::new(1.0, -1.0) * screen.scale, color: Color::BLACK },
            Transform::from_xyz(0.0, y, 1.0).with_scale(Vec3::splat(1.0 / screen.scale)),
            RenderLayers::layer(LAYER), ChildOf(root), Pickable::IGNORE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::npc::NpcTemplate;

    fn fixture() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<Font>()
            .init_asset::<Mesh>().init_asset::<UiMaterial>()
            .init_resource::<Highlighted>().init_resource::<Screen>().init_resource::<input::UiInput>()
            .init_resource::<crate::screens::loading::LoadingScreen>().add_systems(Update, show);
        app.world_mut().spawn((Window { focused: true, ..default() }, PrimaryWindow));
        let npc = app.world_mut().spawn((Npc { profile: NpcTemplate {
            title: "Garnet".into(), profession: "jewelry".into(), profession_title: "Jeweler".into(), ..default()
        }, ..default() }, InheritedVisibility::VISIBLE)).id();
        app.world_mut().resource_mut::<Highlighted>().0 = Some((npc, [0; 4]));
        (app, npc)
    }

    #[test]
    fn hover_switches_labels_without_recreating_the_panel_and_hides_when_blocked() {
        let (mut app, npc) = fixture();
        app.update();
        let mut panels = app.world_mut().query_filtered::<(Entity, &Visibility), With<Panel>>();
        let root = panels.single(app.world()).unwrap().0;
        let mut text = app.world_mut().query::<(&Caption, &Text2d)>();
        assert_eq!(text.iter(app.world()).find(|(c, _)| !c.profession).unwrap().1.0, "Garnet");
        assert_eq!(text.iter(app.world()).find(|(c, _)| c.profession).unwrap().1.0, "Jeweler");
        let other = app.world_mut().spawn((Npc { profile: NpcTemplate { title: "Guard".into(), ..default() }, ..default() }, InheritedVisibility::VISIBLE)).id();
        app.world_mut().resource_mut::<Highlighted>().0 = Some((other, [0; 4]));
        app.update();
        assert_eq!(panels.single(app.world()).unwrap().0, root);
        assert_eq!(text.iter(app.world()).find(|(c, _)| !c.profession).unwrap().1.0, "Guard");
        assert_eq!(text.iter(app.world()).find(|(c, _)| c.profession).unwrap().1.0, "");
        app.world_mut().resource_mut::<input::UiInput>().over_ui = true;
        app.update();
        assert_eq!(*panels.single(app.world()).unwrap().1, Visibility::Hidden);
        app.world_mut().resource_mut::<input::UiInput>().over_ui = false;
        app.world_mut().resource_mut::<Highlighted>().0 = Some((npc, [0; 4]));
        app.update();
        assert_eq!(*panels.single(app.world()).unwrap().1, Visibility::Inherited);
        app.world_mut().entity_mut(npc).insert(NpcVisualPending);
        app.update();
        assert_eq!(*panels.single(app.world()).unwrap().1, Visibility::Hidden);
        app.world_mut().despawn(npc);
        app.update();
        assert_eq!(*panels.single(app.world()).unwrap().1, Visibility::Hidden);
        // Hovering a monster or the ground cannot retain the previous NPC.
        app.world_mut().resource_mut::<Highlighted>().0 = None;
        app.update();
        assert_eq!(*panels.single(app.world()).unwrap().1, Visibility::Hidden);
    }

    #[test]
    fn preview_tracks_hud_scale_and_clears_during_loading() {
        let (mut app, _) = fixture();
        app.update();
        *app.world_mut().resource_mut::<Screen>() = Screen { width: 1700.0, scale: 1.5 };
        app.update();
        let mut panels = app.world_mut().query_filtered::<(&Transform, &Visibility), With<Panel>>();
        assert_eq!(panels.single(app.world()).unwrap().0.translation.x, CENTER_X - BASE_W * 0.5);
        let mut captions = app.world_mut().query::<(&Caption, &TextFont, &Transform)>();
        for (caption, font, at) in captions.iter(app.world()) {
            assert_eq!(font.font_size, bevy::text::FontSize::Px(caption.size * 1.5));
            assert_eq!(at.scale, Vec3::splat(1.0 / 1.5));
        }
        app.world_mut().resource_mut::<crate::screens::loading::LoadingScreen>().connecting();
        app.update();
        assert_eq!(*panels.single(app.world()).unwrap().1, Visibility::Hidden);
    }
}
