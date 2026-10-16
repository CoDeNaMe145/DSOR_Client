//! Opt-in offline regression route. Normal gameplay never reads a replay file.
use super::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use serde::Deserialize;

#[derive(Deserialize)]
struct Step {
    frame: u32,
    position: Option<[f32; 3]>,
    capture: Option<String>,
    #[serde(default)] quit: bool,
}

#[derive(Default)]
pub(super) struct Replay {
    loaded: bool,
    frame: u32,
    steps: std::collections::VecDeque<Step>,
    quit_at: Option<u32>,
}

pub(super) fn run(mut commands: Commands, mut replay: Local<Replay>,
    map: Option<Res<crate::world::map::CurrentMap>>, net: Option<NonSend<crate::network::net::Net>>,
    mut players: Query<(&mut Transform, &mut crate::network::net::LocalPlayer)>, active: Query<&Occluded>) {
    if !replay.loaded {
        replay.loaded = true;
        if let Ok(path) = std::env::var("DSOR_OCCLUSION_REPLAY") {
            match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
                Ok(steps) => replay.steps = steps,
                Err(e) => error!("Occlusion replay: {e}"),
            }
        }
    }
    if replay.steps.is_empty() && replay.quit_at.is_none() { return; }
    if !net.is_some_and(|n| n.offline) || !map.is_some_and(|m| m.spawned) { return; }
    let Ok((mut transform, mut player)) = players.single_mut() else { return };
    replay.frame += 1;
    if replay.quit_at.is_some_and(|frame| replay.frame >= frame) { commands.write_message(AppExit::Success); return; }
    while replay.steps.front().is_some_and(|s| s.frame <= replay.frame) {
        let step = replay.steps.pop_front().unwrap();
        if let Some(position) = step.position {
            transform.translation = Vec3::from_array(position);
            player.target = None;
        }
        if let Some(path) = step.capture {
            info!("Occlusion replay frame {}: player {:?}, {} occluders", replay.frame, transform.translation, active.iter().count());
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        }
        if step.quit { replay.quit_at = Some(replay.frame + 30); }
    }
}
