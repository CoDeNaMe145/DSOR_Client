//! Animation culling for every animated model that is not a dressed character
//! (crate::actors::character manages its own): whole-model NPCs (barrel dealers,
//! gamblers...), animated map decor, effect models.
//!
//! bevy's animate_targets visits every bone hooked to a player (AnimatedBy), each
//! frame, whether or not anything is seen: in the browser it was the largest
//! single cost with only 3 of 84 characters animated (Chromium CPU profile of the
//! wasm build: QueryParIter<AnimationTargetId...> 6.5 %). A player away from the
//! player (crate::world::map::CULL_DISTANCE) has its bones unhooked, and hooked again when
//! it comes back; its clip time keeps running (the player itself is untouched).

use std::collections::HashMap;

use bevy::animation::AnimatedBy;
use bevy::prelude::*;

mod visibility;
pub use visibility::AnimationVisibility;

/// Players crate::actors::character drives itself.
#[derive(Component)]
pub struct ManagedAnimation;

/// Keep a new NPC's targets attached until its first usable pose. A newly added
/// or modified graph reaches Bevy's threaded cache one PostUpdate later: its
/// asset events are published after the animation systems consume them.
#[derive(Component, Default)]
pub(crate) struct PendingFirstPose {
    graph: Option<AssetId<AnimationGraph>>,
    passes: u8,
}

/// Bones unhooked from a player while it is culled.
#[derive(Component, Default)]
struct Parked(Vec<Entity>);

/// Re-decided every this many frames (a model crossing the edge of the screen a
/// few frames late is not seen).
const EVERY: u32 = 8;

pub struct AnimCullPlugin;

impl Plugin for AnimCullPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimationVisibility>()
            .add_systems(PostUpdate, cull_animations.before(bevy::app::AnimationSystems))
            .add_systems(PostUpdate, finish_first_poses.after(bevy::app::AnimationSystems)
                .before(bevy::asset::AssetEventSystems))
            .add_systems(PostUpdate, visibility::track_visibility
                .after(bevy::camera::visibility::VisibilitySystems::MarkNewlyHiddenEntitiesInvisible));
    }
}

#[allow(clippy::type_complexity)]
fn cull_animations(
    mut commands: Commands,
    mut frame: Local<u32>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<(Entity, &GlobalTransform, &InheritedVisibility, Option<&Parked>), (With<AnimationPlayer>, Without<ManagedAnimation>)>,
    targets: Query<(Entity, &AnimatedBy)>,
    first_samples: Query<(), With<PendingFirstPose>>,
    me: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
) {
    *frame = frame.wrapping_add(1);
    let poll = *frame % EVERY == 0;
    if !poll && first_samples.is_empty() {
        return;
    }
    let Some((_, cam_at)) = cameras.iter().next() else { return };
    // The zone around the player (crate::world::map::CULL_DISTANCE), not the view.
    let focus = crate::world::map::camera_focus(cam_at, me.iter().next().map(|p| p.translation()), 0.0);
    let mut decided: HashMap<Entity, bool> = HashMap::new();
    for (e, at, shown, parked) in &players {
        let first_sample = first_samples.contains(e);
        if !poll && !first_sample { continue; }
        let p = at.translation();
        let near = Vec2::new(p.x - focus.x, p.z - focus.z).length() <= crate::world::map::CULL_DISTANCE;
        let live = first_sample || (shown.get() && near);
        match (live, parked) {
            (true, Some(Parked(bones))) => {
                for &b in bones {
                    commands.entity(b).try_insert(AnimatedBy(e));
                }
                commands.entity(e).remove::<Parked>();
            }
            (false, None) => {
                decided.insert(e, false);
            }
            _ => {}
        }
    }
    if decided.is_empty() {
        return;
    }
    let mut parked: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for (bone, AnimatedBy(player)) in &targets {
        if decided.contains_key(player) {
            parked.entry(*player).or_default().push(bone);
        }
    }
    for (player, bones) in parked {
        for &b in &bones {
            commands.entity(b).remove::<AnimatedBy>();
        }
        commands.entity(player).insert(Parked(bones));
    }
}

fn finish_first_poses(
    mut commands: Commands,
    mut pending: Query<(Entity, &AnimationPlayer, &AnimationGraphHandle, &mut PendingFirstPose)>,
    graphs: Res<Assets<AnimationGraph>>,
    clips: Res<Assets<AnimationClip>>,
) {
    for (entity, player, handle, mut first) in &mut pending {
        let ready = graphs.get(&handle.0).is_some_and(|graph|
            player.playing_animations().any(|(node, active)| active.weight() > 0.0 &&
                graph.get(*node).is_some_and(|node| match &node.node_type {
                    bevy::animation::graph::AnimationNodeType::Clip(clip) => clips.contains(clip),
                    _ => false,
                })));
        if !ready || first.graph != Some(handle.0.id()) {
            first.passes = 0;
            first.graph = Some(handle.0.id());
        }
        if !ready { continue; }
        first.passes += 1;
        // The first eligible pass flushes the graph's asset event afterwards;
        // the second consumes it, threads the graph and samples its loaded clip.
        if first.passes >= 2 { commands.entity(entity).remove::<PendingFirstPose>(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::animation::{AnimationPlugin, AnimationTargetId, animated_field};

    #[test]
    fn new_whole_model_samples_before_reveal_even_if_parked_between_cull_polls() {
        use crate::actors::npc::placement_tests::{finish_pose_test, mark_resident_for_pose_test};
        // Cover both the ordinary eight-frame poll and the intervening frames.
        for preceding_frames in [0, EVERY - 1] {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin, AnimCullPlugin))
                .init_resource::<Assets<crate::world::map::MapManifest>>()
                .add_systems(Update, finish_pose_test);
            let camera = Transform::from_xyz(0.0, 10.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z);
            app.world_mut().spawn((Camera3d::default(), GlobalTransform::from(camera)));
            for _ in 0..preceding_frames { app.update(); }
            let target = AnimationTargetId::from_name(&Name::new("model bone"));
            let pose = Vec3::Y * 2.0;
            let mut clip = AnimationClip::default();
            clip.add_curve_to_target(target, AnimatableCurve::new(animated_field!(Transform::translation),
                UnevenSampleAutoCurve::new([(0.0, pose), (2.0, pose)]).unwrap()));
            let clip = app.world_mut().resource_mut::<Assets<AnimationClip>>().add(clip);
            let (graph, node) = AnimationGraph::from_clip(clip);
            let graph = app.world_mut().resource_mut::<Assets<AnimationGraph>>().add(graph);
            let root = app.world_mut().spawn((GlobalTransform::default(), Visibility::Hidden)).id();
            mark_resident_for_pose_test(app.world_mut(), root);
            // Visibility and propagated location may still belong to the hidden
            // loading frame. Previously either could park this newly shown rig.
            let mut animation = AnimationPlayer::default();
            animation.play(node).repeat();
            let player = app.world_mut().spawn((animation, AnimationGraphHandle(graph),
                GlobalTransform::from_translation(Vec3::X * 1000.0), InheritedVisibility::HIDDEN,
                PendingFirstPose::default(), ChildOf(root))).id();
            let bone = app.world_mut().spawn((target, Transform::default(), ChildOf(player))).id();
            app.world_mut().entity_mut(player).insert(Parked(vec![bone]));
            app.world_mut().entity_mut(root).insert(crate::actors::character::ModelBones(
                [("model bone".into(), bone)].into_iter().collect()));
            app.update();
            assert_eq!(app.world().get::<AnimatedBy>(bone).unwrap().0, player);
            assert!(app.world().get::<PendingFirstPose>(player).is_some(),
                "a newly added graph has not reached Bevy's threaded cache yet");
            assert!(app.world().get::<crate::actors::npc::NpcVisualPending>(root).is_some());
            assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
            app.update();
            assert_eq!(app.world().get::<Transform>(bone).unwrap().translation, pose);
            assert!(app.world().get::<Parked>(player).is_none());
            assert!(app.world().get::<PendingFirstPose>(player).is_none());
            app.update();
            assert!(app.world().get::<crate::actors::npc::NpcVisualPending>(root).is_none());
            assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
            assert_eq!(app.world().get::<Transform>(bone).unwrap().translation, pose);
        }
    }
}
