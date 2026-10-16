//! Pose sampling for ordinary NPCs follows the union of camera and shadow views.
//! Read the completed visibility pass; Update consumes it on the next frame.

use bevy::ecs::entity::{EntityHashMap, EntityHashSet};
use bevy::prelude::*;
use crate::actors::character::{AnimLod, CharacterAnim};

const HIDE_DELAY: f64 = 0.25;

#[derive(Default)]
struct Seen {
    unseen_since: Option<f64>,
    has_mesh: bool,
    visible: bool,
    culled: bool,
}

/// Unknown/loading actors remain active. Only NPCs already using animation LOD
/// participate; local players, portraits and other animations keep their policy.
#[derive(Resource, Default)]
pub struct AnimationVisibility {
    actors: EntityHashMap<Seen>,
}

#[derive(Default)]
pub(super) struct MeshOwners {
    owners: EntityHashMap<Option<Entity>>,
    ancestors: EntityHashSet,
}

impl AnimationVisibility {
    pub fn needs_animation(&self, actor: Entity) -> bool {
        self.actors.get(&actor).is_none_or(|seen| !seen.culled)
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn track_visibility(
    time: Res<Time>,
    mut visibility: ResMut<AnimationVisibility>,
    actors: Query<Entity, (With<CharacterAnim>, With<AnimLod>, Without<crate::screens::selection::Preview>, Without<crate::network::net::LocalPlayer>)>,
    meshes: Query<(Entity, &ViewVisibility), With<Mesh3d>>,
    emitters: Query<Entity, With<crate::rendering::particles::Emitter>>,
    parents: Query<&ChildOf>,
    reparented: Query<Entity, Changed<ChildOf>>,
    mut removed_parents: RemovedComponents<ChildOf>,
    mut removed_meshes: RemovedComponents<Mesh3d>,
    mut cache: Local<MeshOwners>,
) {
    if crate::flag("noanimviewcull") {
        visibility.actors.clear();
        return;
    }
    // Ownership is stable between scene loads and outfit swaps. Cache it instead
    // of walking thousands of clothing hierarchies every frame.
    // UI glyphs and transient 2D entities also have ChildOf. Their churn must
    // not force us to walk every 3D hierarchy again.
    let affects_mesh = |entity| cache.owners.contains_key(&entity) || cache.ancestors.contains(&entity);
    let mut hierarchy_changed = false;
    for entity in removed_parents.read() { hierarchy_changed |= affects_mesh(entity); }
    hierarchy_changed |= reparented.iter().any(affects_mesh);
    if hierarchy_changed || actors.iter().any(|actor| !visibility.actors.contains_key(&actor)) {
        cache.owners.clear();
        cache.ancestors.clear();
    }
    for entity in removed_meshes.read() { cache.owners.remove(&entity); }
    visibility.actors.retain(|entity, _| actors.contains(*entity));
    for entity in &actors {
        let seen = visibility.actors.entry(entity).or_default();
        seen.has_mesh = false;
        seen.visible = false;
    }
    let MeshOwners { owners, ancestors } = &mut *cache;
    let mut owner = |entity| *owners.entry(entity).or_insert_with(|| {
        if actors.contains(entity) { return Some(entity); }
        parents.iter_ancestors(entity).find(|ancestor| {
            ancestors.insert(*ancestor);
            actors.contains(*ancestor)
        })
    });
    for (entity, visible) in &meshes {
        let Some(actor) = owner(entity) else { continue };
        let Some(seen) = visibility.actors.get_mut(&actor) else { continue };
        seen.has_mesh = true;
        // ViewVisibility includes offscreen casters needed by any shadow view.
        seen.visible |= visible.get();
    }
    // Particle batches live outside the actor hierarchy: a visible trail or
    // emitter can need its animated attachment even when the body is offscreen.
    for entity in &emitters {
        if let Some(seen) = owner(entity).and_then(|actor| visibility.actors.get_mut(&actor)) {
            seen.visible = true;
        }
    }
    let now = time.elapsed_secs_f64();
    for seen in visibility.actors.values_mut() {
        if !seen.has_mesh || seen.visible {
            seen.unseen_since = None;
            seen.culled = false;
        } else {
            let since = *seen.unseen_since.get_or_insert(now);
            seen.culled = now - since >= HIDE_DELAY;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<AnimationVisibility>()
            .add_systems(Update, track_visibility);
        app
    }

    fn actor(world: &mut World) -> Entity {
        world.spawn((CharacterAnim::default(), AnimLod::new(3, 0))).id()
    }

    fn step(app: &mut App, seconds: f64) {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f64(seconds));
        app.update();
    }

    fn live(app: &App, actor: Entity) -> bool {
        app.world().resource::<AnimationVisibility>().needs_animation(actor)
    }

    #[test]
    fn all_views_must_be_hidden_and_reentry_is_immediate() {
        let mut app = app();
        let root = actor(app.world_mut());
        let body = app.world_mut().spawn((Mesh3d::default(), ViewVisibility::HIDDEN, ChildOf(root))).id();
        // A second mesh visible only to a shadow view still needs the skeleton.
        let shadow = app.world_mut().spawn((Mesh3d::default(), ViewVisibility::VISIBLE, ChildOf(root))).id();
        step(&mut app, 1.0);
        step(&mut app, 1.0);
        assert!(live(&app, root));
        app.world_mut().entity_mut(shadow).insert(ViewVisibility::HIDDEN);
        step(&mut app, 0.01);
        assert!(live(&app, root), "do not park at a flickering frustum edge");
        step(&mut app, HIDE_DELAY + 0.01);
        assert!(!live(&app, root));
        app.world_mut().entity_mut(body).insert(ViewVisibility::VISIBLE);
        step(&mut app, 0.01);
        assert!(live(&app, root), "reappearance must not wait for a polling interval");
    }

    #[test]
    fn loading_portraits_and_particle_attachments_stay_active() {
        let mut app = app();
        let loading = actor(app.world_mut());
        let portrait = actor(app.world_mut());
        app.world_mut().entity_mut(portrait).insert(crate::screens::selection::Preview);
        app.world_mut().spawn((Mesh3d::default(), ViewVisibility::HIDDEN, ChildOf(portrait)));
        let particles = actor(app.world_mut());
        app.world_mut().spawn((Mesh3d::default(), ViewVisibility::HIDDEN, ChildOf(particles)));
        app.world_mut().spawn((crate::rendering::particles::Emitter::default(), ChildOf(particles)));
        step(&mut app, 1.0);
        step(&mut app, 1.0);
        assert!(live(&app, loading));
        assert!(live(&app, portrait));
        assert!(live(&app, particles));
    }

    #[test]
    fn outfit_reparenting_and_despawn_invalidate_cached_ownership() {
        let mut app = app();
        let first = actor(app.world_mut());
        let second = actor(app.world_mut());
        let outfit = app.world_mut().spawn(ChildOf(first)).id();
        let mesh = app.world_mut().spawn((Mesh3d::default(), ViewVisibility::HIDDEN, ChildOf(outfit))).id();
        step(&mut app, 1.0);
        step(&mut app, 1.0);
        assert!(!live(&app, first));
        assert!(live(&app, second));
        app.world_mut().entity_mut(outfit).insert(ChildOf(second));
        step(&mut app, 1.0);
        step(&mut app, 1.0);
        assert!(live(&app, first));
        assert!(!live(&app, second));
        app.world_mut().despawn(mesh);
        step(&mut app, 1.0);
        assert!(live(&app, second));
    }
}
