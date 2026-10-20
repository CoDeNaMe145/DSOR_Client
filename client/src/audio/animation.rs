//! Authored sound markers, driven by the same clock as the visible animation.
use super::*;
use crate::actors::character::{AnimLod, Character, CharacterAnim};
use bevy::animation::graph::AnimationNodeType;

#[derive(Default, Deserialize)]
pub(super) struct Bindings {
    models: HashMap<String, HashMap<String, Vec<Marker>>>,
    skins: HashMap<String, HashMap<String, Vec<String>>>,
    sets: HashMap<String, HashMap<String, Vec<String>>>,
    #[serde(default)]
    fallbacks: HashMap<String, HashMap<String, Vec<String>>>,
}
#[derive(Deserialize)]
struct Marker { at: f32, slot: usize }
struct Cue { at: f32, name: String }

impl Bindings {
    fn resolve(&self, model: &str, profile: &str, clip: &str) -> Vec<Cue> {
        let clip = clip.trim_end_matches("-loop");
        let Some(markers) = self.models.get(model).and_then(|m| m.get(clip)) else { return Vec::new() };
        let cues = if let Some(authored) = self.skins.get(profile) {
            // An existing profile's omitted clip is silent too. For example,
            // ordinary horses must not inherit a special mount's idle breath.
            authored.get(clip)
        } else {
            self.sets.get(model).and_then(|m| m.get(profile)).and_then(|parts|
                parts.iter().find_map(|skin| self.skins.get(skin)?.get(clip)))
                .or_else(|| self.fallbacks.get(model)?.get(clip))
        };
        let Some(cues) = cues else { return Vec::new() };
        markers.iter().filter_map(|marker| {
            let name = cues.get(marker.slot)?.trim();
            (!name.is_empty()).then(|| Cue { at: marker.at, name: name.to_owned() })
        }).collect()
    }
}

/// Attached to an animation player when its graph is built. Event resolution is
/// cached per clip; steady frames only compare marker times and queue one-shots.
#[derive(Component)]
pub(crate) struct AnimationAudio {
    owner: Entity,
    model: String,
    profile: String,
    clips: HashMap<AnimationNodeIndex, String>,
    resolved: HashMap<AnimationNodeIndex, Vec<Cue>>,
    last: Option<Cursor>,
    position: Option<Vec3>,
}
#[derive(Clone, Copy)]
struct Cursor { node: AnimationNodeIndex, epoch: u64, phase: f32, elapsed: f32 }

impl AnimationAudio {
    pub(crate) fn new(owner: Entity, path: &str, profile: &str, clips: HashMap<AnimationNodeIndex, String>) -> Self {
        let path = path.trim_end_matches(".glb");
        let (model, encoded) = path.split_once(".dsor-set-").unwrap_or((path, ""));
        let model = model.split(".dsor-state-").next().unwrap_or(model);
        let decoded = || {
            let bytes: Option<Vec<_>> = encoded.as_bytes().chunks_exact(2).map(|p|
                u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok()).collect();
            bytes.and_then(|b| String::from_utf8(b).ok()).unwrap_or_default()
        };
        Self { owner, model: model.into(), profile: if profile.is_empty() { decoded() } else { profile.into() },
            clips, resolved: default(), last: None, position: None }
    }
}

/// Return the interval just traversed, unwrapped across one loop. Large seeks
/// and stalled frames deliberately drop old impacts instead of bursting them.
fn interval(previous: Option<f32>, phase: f32, duration: f32, looping: bool, advance: f32) -> Option<(f32, f32)> {
    if !phase.is_finite() || duration <= 0.0 || !duration.is_finite() { return None; }
    let start = previous.unwrap_or(-f32::EPSILON);
    let end = if phase < start && looping { phase + duration } else { phase };
    let distance = end - start;
    (distance > 0.0 && distance <= advance + 0.06).then_some((start, end))
}
fn crossed(at: f32, start: f32, end: f32, duration: f32, looping: bool) -> bool {
    (at > start && at <= end) || (looping && at + duration > start && at + duration <= end)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit(
    mut state: ResMut<AudioState>, catalogs: Res<Assets<Catalog>>, mut requests: ResMut<AudioRequests>,
    time: Res<Time>, current: Option<Res<CurrentMap>>, loading: Option<Res<crate::screens::loading::LoadingScreen>>,
    mut players: Query<(&AnimationPlayer, &AnimationGraphHandle, &mut AnimationAudio)>,
    owners: Query<(&GlobalTransform, Option<&CharacterAnim>, Option<&AnimLod>, Option<&Character>, Option<&InheritedVisibility>)>,
    listener: Query<&GlobalTransform, With<LocalPlayer>>,
    graphs: Res<Assets<AnimationGraph>>, clips: Res<Assets<AnimationClip>>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("audio_animation::emit");
    let ready = current.is_some_and(|m| m.spawned) && !loading.is_some_and(|s| s.active());
    let Some(catalog) = catalogs.get(&state.catalog) else { return };
    let Some(listener) = listener.iter().next().map(|t| t.translation()) else { return };
    for (player, graph, mut audio) in &mut players {
        let Ok((transform, controlled, lod, character, visibility)) = owners.get(audio.owner) else { continue };
        let position = transform.translation();
        if !ready || character.is_some_and(|c| c.riding.is_some()) {
            audio.last = None;
            audio.position = Some(position);
            continue;
        }
        let clock = if let Some(controlled) = controlled {
            controlled.audio_clock(lod)
        } else {
            player.playing_animations().filter(|(_, a)| !a.is_paused() && a.weight() > 0.)
                .max_by(|(_, a), (_, b)| a.weight().total_cmp(&b.weight())).map(|(n, _)| (*n, None, 0))
        };
        let Some((node, held, epoch)) = clock else { audio.last = None; continue };
        let active = player.animation(node);
        if held.is_none() && active.is_none_or(|a| a.is_paused() || a.speed() <= 0.) { continue; }
        let Some(AnimationNodeType::Clip(handle)) = graphs.get(&graph.0).and_then(|g| g.graph.node_weight(node)).map(|n| &n.node_type) else { continue };
        let Some(clip) = clips.get(handle) else { continue };
        let duration = clip.duration();
        if duration <= 0. { continue; }
        let looping = held.is_some() || active.is_some_and(|a| a.repeat_mode() == bevy::animation::RepeatAnimation::Forever);
        let phase = held.map(|t| t.rem_euclid(duration)).or_else(|| active.map(|a| a.seek_time())).unwrap_or(0.);
        let elapsed = active.map_or(0., |a| a.elapsed());
        let speed = controlled.map_or_else(|| active.map_or(1., |a| a.speed()), |a| a.speed);
        let previous = audio.last.filter(|c| c.node == node && c.epoch == epoch
            && (controlled.is_some() || elapsed >= c.elapsed)).map(|c| c.phase);
        audio.last = Some(Cursor { node, epoch, phase, elapsed });
        let jumped = audio.position.replace(position).is_some_and(|old| old.distance_squared(position) > 64.);
        let audible = position.distance_squared(listener) <= 45. * 45. && visibility.is_none_or(|v| v.get());
        if !audible || jumped || time.delta_secs() > 0.25 { continue; }
        if !audio.resolved.contains_key(&node) {
            let Some(name) = audio.clips.get(&node) else { continue };
            let cues = catalog.animations.resolve(&audio.model, &audio.profile, name);
            for cue in &cues {
                if let Some(event) = catalog.events.get(&cue.name) {
                    for path in &event.samples {
                        if state.warm.len() < 512 && !state.cache.contains_key(path) && !state.warm.contains(path) {
                            state.warm.push_front(path.clone());
                        }
                    }
                }
            }
            audio.resolved.insert(node, cues);
        }
        let Some((start, end)) = interval(previous, phase, duration, looping, time.delta_secs() * speed.max(0.)) else { continue };
        for cue in &audio.resolved[&node] {
            if crossed(cue.at, start, end, duration, looping) && catalog.events.contains_key(&cue.name) {
                requests.spatial_one_shot(&cue.name, position, audio.owner);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markers_follow_wraps_pause_and_seeks_without_catchup() {
        let (a, b) = interval(Some(0.89), 0.02, 0.92, true, 0.05).unwrap();
        assert!(crossed(0., a, b, 0.92, true));
        assert!(!crossed(0.48, a, b, 0.92, true));
        assert!(!crossed(0., 0.9, 0.92, 0.92, false), "one-shot clips do not fire frame zero again at the end");
        assert!(interval(Some(0.4), 0.4, 3.44, true, 0.016).is_none());
        assert!(interval(Some(0.1), 2.3, 3.44, true, 0.016).is_none());
        assert!(interval(Some(2.3), 0.01, 3.44, false, 0.016).is_none());
        let (a, b) = interval(None, 0.01, 0.92, true, 0.016).unwrap();
        assert!(crossed(0., a, b, 0.92, true));
        assert!(interval(None, 2.3, 3.44, true, 0.016).is_none());
    }
    #[test]
    fn skin_specific_markers_preserve_silent_slots_and_model_identity() {
        let bindings: Bindings = serde_json::from_str(r#"{
            "models":{"characters/horse":{"run":[{"at":0.0,"slot":0},{"at":0.08,"slot":1},{"at":0.24,"slot":2}]}},
            "skins":{"horse":{"run":["hoof","","hoof2"]},"zebra":{"run":["zebra_a","zebra_b",""]},"silent":{"idle":[]}},
            "sets":{"characters/horse":{"new_horse":["horse"]}},
            "fallbacks":{"characters/horse":{"run":["hoof","","hoof2"]}}
        }"#).unwrap();
        let horse = bindings.resolve("characters/horse", "new_horse", "run-loop");
        assert_eq!(horse.iter().map(|c| (c.at, c.name.as_str())).collect::<Vec<_>>(), [(0., "hoof"), (0.24, "hoof2")]);
        let zebra = bindings.resolve("characters/horse", "zebra", "run-loop");
        assert_eq!(zebra[1].name, "zebra_b");
        assert!(bindings.resolve("characters/uniskel", "horse", "run-loop").is_empty(), "rider must not duplicate mount footsteps");
        assert!(bindings.resolve("characters/horse", "silent", "run-loop").is_empty(), "missing clips in an authored profile stay silent");
        assert_eq!(bindings.resolve("characters/horse", "new_cosmetic", "run-loop")[0].name, "hoof");
        let binding = AnimationAudio::new(Entity::PLACEHOLDER, "characters/horse.dsor-set-7a65627261.glb", "", default());
        assert_eq!((binding.model.as_str(), binding.profile.as_str()), ("characters/horse", "zebra"));
    }
}
