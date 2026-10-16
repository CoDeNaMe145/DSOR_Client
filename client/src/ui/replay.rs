//! Opt-in native pointer/capture fixture for focus and overlap regressions.
use super::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

#[derive(Deserialize)]
struct Step {
    frame: u32,
    /// Reference pixels on a 1024x768 screen, centered at wider aspect ratios.
    point: Option<[f32; 2]>,
    /// Reference pixels anchored to the screen's left edge.
    left_point: Option<[f32; 2]>,
    /// Click the live NPC's torso through the normal picking/movement path.
    npc: Option<String>,
    /// Offline-only fixture placement and quest offer; never sent to a server.
    player: Option<[f32; 3]>,
    npc_quest: Option<String>,
    npc_offers: Option<Vec<String>>,
    key: Option<String>,
    left: Option<bool>,
    capture: Option<String>,
    #[serde(default)] quit: bool,
}

#[derive(Default)]
pub(super) struct Replay {
    loaded: bool,
    frame: u32,
    steps: std::collections::VecDeque<Step>,
    quit_at: Option<u32>,
    key: Option<KeyCode>,
}

pub(super) fn run(mut commands: Commands, mut replay: Local<Replay>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>, mut mouse: ResMut<ButtonInput<MouseButton>>,
    roots: Query<(Entity, &Window2018, &Transform, &Visibility), Without<crate::network::net::LocalPlayer>>, children: Query<&Children>,
    elements: Query<(Option<&GlobalTransform>, Option<&Visibility>, Option<&InheritedVisibility>, Option<&ViewVisibility>, Option<&Text2d>, Has<Mesh2d>, Has<Sprite>)>,
    npcs: Query<(&crate::actors::npc::Npc, &GlobalTransform)>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    mut players: Query<(&mut crate::network::net::LocalPlayer, &mut Transform)>,
    mut net: Option<NonSendMut<crate::network::net::Net>>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    if !replay.loaded {
        replay.loaded = true;
        if let Ok(path) = std::env::var("DSOR_UI_REPLAY") {
            match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
                Ok(steps) => replay.steps = steps,
                Err(e) => error!("UI replay: {e}"),
            }
        }
    }
    if replay.steps.is_empty() && replay.quit_at.is_none() { return; }
    if let Some(key) = replay.key.take() { keys.release(key); }
    let Ok(mut window) = windows.single_mut() else { return };
    // Inject game input without asking winit to warp the desktop cursor.
    let window = window.bypass_change_detection();
    replay.frame += 1;
    if replay.quit_at.is_some_and(|at| replay.frame >= at) { commands.write_message(AppExit::Success); return; }
    while replay.steps.front().is_some_and(|s| s.frame <= replay.frame) {
        let step = replay.steps.pop_front().unwrap();
        if let Some(name) = step.key.as_deref() {
            if let Some(key) = match name { "Escape" => Some(KeyCode::Escape), "V" => Some(KeyCode::KeyV), "I" => Some(KeyCode::KeyI), _ => None } {
                keys.press(key);
                replay.key = Some(key);
            }
        }
        if net.as_ref().is_some_and(|n| n.offline) {
            if let Some(position) = step.player {
                for (mut player, mut at) in &mut players { at.translation = Vec3::from(position); player.target = None; }
            }
        }
        if let Some(template) = &step.npc {
            if let Some((npc, at)) = npcs.iter().find(|(n, _)| &n.template == template) {
                if let (Some(quest), Some(net)) = (&step.npc_quest, net.as_mut().filter(|n| n.offline)) {
                    net.npc_dialogue.infos.insert(npc.actor, dsor_proto::commands::actors::NpcInfo {
                        name: npc.profile.shop.clone(), enabled: true,
                        interactions: vec![dsor_proto::commands::actors::NpcInteraction { name: quest.clone(), quest: quest.clone(), kind: 0, state: 10 }], ..default()
                    });
                }
                if let (Some(items), Some(net)) = (&step.npc_offers, net.as_mut().filter(|n| n.offline)) {
                    use dsor_proto::commands::inventory::{Offer, ShopOffer};
                    let shop = npc.profile.shop.clone();
                    net.npc_dialogue.offers.insert(shop.clone(), Offer { shop,
                        offers: items.iter().enumerate().map(|(i, item)| Some(ShopOffer {
                            id: format!("preview-{i}"), item: item.clone(), amount: (i + 1) as u32,
                            price_vc: (i as u32 + 1) * 125, ..default()
                        })).collect(), ..default()
                    });
                }
                if let Ok((camera, transform)) = cameras.single() {
                    if let Ok(point) = camera.world_to_viewport(transform, at.translation() + Vec3::Y * 1.1) {
                        window.set_cursor_position(Some(point));
                        window.focused = true;
                        info!("NPC replay: clicking {} actor {} at {:?}", template, npc.actor, point);
                    }
                }
            } else { error!("NPC replay: missing {template}"); }
        }
        if let Some([x, y]) = step.point {
            let scale = window.height() / BASE_H;
            let point = Vec2::new(x * scale + (window.width() - BASE_W * scale) * 0.5, y * scale);
            window.set_cursor_position(Some(point));
            window.focused = true;
        }
        if let Some([x, y]) = step.left_point {
            window.set_cursor_position(Some(Vec2::new(x, y) * (window.height() / BASE_H)));
            window.focused = true;
        }
        if let Some(left) = step.left {
            if left { mouse.press(MouseButton::Left); } else { mouse.release(MouseButton::Left); }
        }
        if let Some(path) = step.capture {
            for (player, at) in &players {
                info!("NPC replay frame {}: player {:?}, target {:?}", replay.frame, at.translation, player.target);
            }
            let mut snapshot = Vec::new();
            for (root, name, tf, visible) in &roots {
                if is_menu(&name.0) && *visible != Visibility::Hidden {
                    info!("UI replay frame {}: {} at {:?}", replay.frame, name.0, tf.translation);
                    let mut queue = vec![root];
                    while let Some(e) = queue.pop() {
                        if let Ok(c) = children.get(e) { queue.extend(c.iter()); }
                        if let Ok((transform, visibility, inherited, view, text, mesh, sprite)) = elements.get(e) {
                            if mesh || sprite || text.is_some() {
                                snapshot.push(serde_json::json!({"window":name.0,"entity":format!("{e:?}"),
                                    "position":transform.map(|t|t.translation().to_array()),"visibility":format!("{visibility:?}"),
                                    "inherited":inherited.map(|v|v.get()),"in_view":view.map(|v|v.get()),
                                    "text":text.map(|t|&t.0),"mesh":mesh,"sprite":sprite}));
                            }
                        }
                    }
                }
            }
            if let Err(e) = std::fs::write(std::path::Path::new(&path).with_extension("ui.json"), serde_json::to_vec_pretty(&snapshot).unwrap()) {
                error!("UI replay snapshot: {e}");
            }
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        }
        if step.quit { replay.quit_at = Some(replay.frame + 30); }
    }
}
