//! Share identical immutable NPC part materials across split GLBs. A material
//! handle is part of Bevy's non-bindless batch-set key, so identical imports from
//! body/head/equipment files otherwise create separate render batch sets.
//!
//! This does not merge meshes or change skin ownership. Original handles stay
//! on remapped meshes for hot reload (and can retain their GPU allocations);
//! the optimization is batch grouping. The cache itself contains only weak
//! handles and cannot keep a streamed-out visual or its textures resident.

use super::{AnimatedCustom, AnimatedMaterial, ShaderAnim, UvAnim};
use crate::rendering::surfaces::{Kind, NebulaMaterial};
use bevy::asset::{AssetId, AssetPath, StrongHandle};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

type MaterialId = AssetId<NebulaMaterial>;
type TextureBucket = [Option<AssetId<Image>>; 4];

#[derive(Component)]
struct OriginalNpcPartMaterial {
    original: Handle<NebulaMaterial>,
    shared: MaterialId,
}

#[derive(Resource, Default)]
struct NpcPartMaterials {
    aliases: HashMap<MaterialId, Weak<StrongHandle>>,
    buckets: HashMap<TextureBucket, Vec<Weak<StrongHandle>>>,
    pending: HashSet<Entity>,
}

/// Reflection covers every StandardMaterial field except cull_mode in Bevy
/// 0.19.1. ExtendedMaterial, NebulaSurface and SurfaceParams ignore no fields.
/// Compare that exception explicitly and fail closed for unsupported fields.
/// In particular, do not use the map merge signature's rounded float values.
fn identical(a: &NebulaMaterial, b: &NebulaMaterial) -> bool {
    a.base.cull_mode == b.base.cull_mode && a.reflect_partial_eq(b) == Some(true)
}

fn bucket(material: &NebulaMaterial) -> TextureBucket {
    [
        material.base.base_color_texture.as_ref().map(Handle::id),
        material.base.normal_map_texture.as_ref().map(Handle::id),
        material.base.emissive_texture.as_ref().map(Handle::id),
        material.extension.mask.as_ref().map(Handle::id),
    ]
}

fn part_source(path: Option<&AssetPath<'_>>) -> bool {
    let Some(path) = path else { return false };
    path.path().extension().is_some_and(|s| s == "glb")
        && path.path().components().any(|c| c.as_os_str() == "parts")
        && path.label().is_some_and(|label| label.starts_with("Material") && label.ends_with("/neb"))
}

impl NpcPartMaterials {
    fn canonical(&mut self, source: &Handle<NebulaMaterial>, materials: &Assets<NebulaMaterial>) -> Option<Handle<NebulaMaterial>> {
        let material = materials.get(source)?;
        if material.extension.params.p0.x != Kind::Character as u32 as f32 {
            return None;
        }
        if let Some(handle) = self.aliases.get(&source.id()).and_then(Weak::upgrade) {
            return Some(Handle::Strong(handle));
        }
        let Handle::Strong(source_handle) = source else { return None };
        let candidates = self.buckets.entry(bucket(material)).or_default();
        candidates.retain(|h| h.strong_count() != 0);
        let canonical = candidates.iter().filter_map(Weak::upgrade).find(|handle| {
            materials.get(Handle::<NebulaMaterial>::Strong(handle.clone()).id()).is_some_and(|other| identical(material, other))
        });
        let canonical = canonical.unwrap_or_else(|| {
            candidates.push(Arc::downgrade(source_handle));
            source_handle.clone()
        });
        self.aliases.insert(source.id(), Arc::downgrade(&canonical));
        Some(Handle::Strong(canonical))
    }
}

type NpcRoots = With<crate::actors::npc::Npc>;
type MutableNodes = Or<(
    With<ShaderAnim>, With<UvAnim>, With<AnimatedCustom>, With<AnimatedMaterial>,
    With<crate::network::net::LocalPlayer>, With<crate::network::net::RemotePlayer>, With<crate::actors::monsters::Monster>,
)>;

fn immutable_npc(entity: Entity, parents: &Query<&ChildOf>, roots: &Query<(), NpcRoots>, animated: &Query<(), MutableNodes>) -> bool {
    let mut npc = false;
    for ancestor in std::iter::once(entity).chain(parents.iter_ancestors(entity)) {
        if animated.contains(ancestor) {
            return false;
        }
        npc |= roots.contains(ancestor);
    }
    npc
}

/// Arrival work only. Runtime dye/hit/animated clones have no source path;
/// both they and player/monster hierarchies are excluded. Existing mutation
/// systems clone before writing, so later ownership changes stay independent.
#[allow(clippy::too_many_arguments)]
fn share_npc_parts(
    mut commands: Commands,
    mut mesh_queries: ParamSet<(
        Query<Entity, Changed<MeshMaterial3d<NebulaMaterial>>>,
        Query<(Entity, &mut MeshMaterial3d<NebulaMaterial>, Option<&OriginalNpcPartMaterial>)>,
        Query<(&MeshMaterial3d<NebulaMaterial>, &OriginalNpcPartMaterial)>,
    )>,
    parents: Query<&ChildOf>,
    roots: Query<(), NpcRoots>,
    animated: Query<(), MutableNodes>,
    mut events: MessageReader<AssetEvent<NebulaMaterial>>,
    materials: Res<Assets<NebulaMaterial>>,
    mut cache: ResMut<NpcPartMaterials>,
) {
    let mut invalidated = false;
    let mut removed = HashSet::new();
    for event in events.read() {
        match event {
            AssetEvent::Modified { id } => invalidated |= cache.aliases.contains_key(id),
            AssetEvent::Removed { id } if cache.aliases.contains_key(id) => { removed.insert(*id); }
            _ => {}
        }
    }
    if !removed.is_empty() {
        // Ordinary visual unloading removes imported materials too. An unused
        // alias going away only prunes the weak cache; it must not rewrite the
        // remaining crowd's handles. Check only current remaps, once per batch
        // of removals, before considering a full hot-reload restoration.
        invalidated |= mesh_queries.p2().iter().any(|(material, original)| {
            material.0.id() == original.shared
                && (removed.contains(&original.shared) || removed.contains(&original.original.id()))
        });
        cache.aliases.retain(|id, shared| {
            !removed.contains(id) && shared.upgrade().is_some_and(|h| !removed.contains(&Handle::<NebulaMaterial>::Strong(h).id()))
        });
        cache.buckets.retain(|_, candidates| {
            candidates.retain(|shared| shared.upgrade().is_some_and(|h| !removed.contains(&Handle::<NebulaMaterial>::Strong(h).id())));
            !candidates.is_empty()
        });
    }
    let mut work = std::mem::take(&mut cache.pending);
    work.extend(mesh_queries.p0().iter());
    let mut meshes = mesh_queries.p1();
    if invalidated {
        // Restore original file ownership before rebuilding equivalence after
        // hot reload. Do not overwrite a dye/effect copy that has taken over.
        cache.aliases.clear();
        cache.buckets.clear();
        for (entity, mut material, original) in &mut meshes {
            work.insert(entity);
            if let Some(original) = original {
                if material.0.id() == original.shared {
                    material.0 = original.original.clone();
                }
            }
        }
    }
    for entity in work {
        let Ok((_, mut material, original)) = meshes.get_mut(entity) else { continue };
        // A shared handle deliberately retains its source path. Keep the
        // original mapping unless invalidated or replaced by an owned copy.
        if let Some(original) = original {
            if !invalidated && material.0.id() == original.shared {
                continue;
            }
            commands.entity(entity).remove::<OriginalNpcPartMaterial>();
        }
        if !part_source(material.0.path()) || !immutable_npc(entity, &parents, &roots, &animated) {
            continue;
        }
        if !materials.contains(material.0.id()) {
            cache.pending.insert(entity);
            continue;
        }
        let Some(shared) = cache.canonical(&material.0, &materials) else { continue };
        if shared.id() != material.0.id() {
            commands.entity(entity).insert(OriginalNpcPartMaterial { original: material.0.clone(), shared: shared.id() });
            material.0 = shared;
        }
    }
}

pub(super) fn install(app: &mut App) {
    if !crate::flag("legacynpcmaterials") {
        // Equipment dyes and shader animation establish their owned copies in
        // Update. NPC parts arriving later remain eligible on the next frame.
        app.init_resource::<NpcPartMaterials>().add_systems(PostUpdate, share_npc_parts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::surfaces::{NebulaSurface, SurfaceParams};
    use bevy::render::render_resource::Face;

    fn material() -> NebulaMaterial {
        NebulaMaterial {
            base: StandardMaterial { alpha_mode: AlphaMode::Mask(0.5), ..default() },
            extension: NebulaSurface {
                params: SurfaceParams { p0: Vec4::new(Kind::Character as u32 as f32, 0.0, 1.0, 0.0), ..default() },
                cube: None, layer: None, mask: None,
            },
        }
    }

    #[test]
    fn exact_equality_preserves_render_state_and_texture_identity() {
        let a = material();
        assert!(identical(&a, &a.clone()), "all reflected field types must support equality");
        let differences: &[fn(&mut NebulaMaterial)] = &[
            |m| m.base.cull_mode = None,
            |m| m.base.cull_mode = Some(Face::Front),
            |m| m.base.perceptual_roughness += 0.00001,
            |m| m.base.alpha_mode = AlphaMode::Mask(0.50001),
            |m| m.base.base_color_channel = bevy::mesh::UvChannel::Uv1,
            |m| m.base.emissive_exposure_weight = 0.125,
            |m| m.base.uv_transform.translation.x = 0.00001,
            |m| m.base.fog_enabled = false,
            |m| m.extension.params.p1.x = 0.00001,
            |m| m.extension.params.p3.w = 0.5,
        ];
        for change in differences {
            let mut b = a.clone();
            change(&mut b);
            assert!(!identical(&a, &b), "render-affecting differences cannot be merged: {b:?}");
        }
        let mut images = Assets::<Image>::default();
        let texture_a = images.add(Image::default());
        let texture_b = images.add(Image::default());
        let mut a = a;
        a.extension.mask = Some(texture_a);
        let mut b = a.clone();
        b.extension.mask = Some(texture_b);
        assert!(!identical(&a, &b));
        a.base.perceptual_roughness = f32::NAN;
        assert!(!identical(&a, &a), "unknown/non-comparable values must fail closed");
    }

    #[test]
    fn only_original_split_part_material_paths_qualify() {
        for path in ["characters/uniskel/parts/head.glb#Material0/neb", "characters/uniskel/parts/body.glb#Material12/neb"] {
            assert!(part_source(Some(&AssetPath::from(path))));
        }
        for path in ["characters/uniskel/model.glb#Material0/neb", "effects/part.glb#Material0/neb", "characters/uniskel/parts/head.glb#Material0", "characters/uniskel/parts/head.glb#Material0/refr"] {
            assert!(!part_source(Some(&AssetPath::from(path))));
        }
        assert!(!part_source(None), "runtime-owned copies have no source path");
    }

    #[test]
    fn identical_imports_share_but_owned_copies_remain_independent() {
        let mut materials = Assets::<NebulaMaterial>::default();
        let first = materials.add(material());
        let second = materials.add(material());
        let mut cache = NpcPartMaterials::default();
        assert_eq!(cache.canonical(&first, &materials).unwrap().id(), first.id());
        let shared = cache.canonical(&second, &materials).unwrap();
        assert_eq!(shared.id(), first.id());
        let owned = materials.add(materials.get(&shared).unwrap().clone());
        assert!(!part_source(owned.path()));
        materials.get_mut(&owned).unwrap().extension.params.p1.x = 0.75;
        assert_eq!(materials.get(&shared).unwrap().extension.params.p1.x, 0.0);
        assert_eq!(materials.get(&second).unwrap().extension.params.p1.x, 0.0);
        assert_ne!(owned.id(), shared.id());
    }

    #[test]
    fn cache_does_not_retain_streamed_out_materials() {
        let mut materials = Assets::<NebulaMaterial>::default();
        let first = materials.add(material());
        let second = materials.add(material());
        let mut cache = NpcPartMaterials::default();
        cache.canonical(&first, &materials);
        let shared = cache.canonical(&second, &materials).unwrap();
        let id = first.id();
        drop(first);
        drop(second);
        drop(shared);
        assert!(cache.aliases[&id].upgrade().is_none(), "the cache must not own strong handles");
    }

    #[test]
    fn material_arriving_after_its_mesh_can_join_existing_group() {
        let mut materials = Assets::<NebulaMaterial>::default();
        let first = materials.add(material());
        let late = materials.reserve_handle();
        let mut cache = NpcPartMaterials::default();
        cache.canonical(&first, &materials);
        assert!(cache.canonical(&late, &materials).is_none());
        materials.insert(late.id(), material()).unwrap();
        assert_eq!(cache.canonical(&late, &materials).unwrap().id(), first.id());
    }

    #[test]
    fn animated_and_player_ancestors_are_excluded_even_with_an_npc_child() {
        use bevy::ecs::system::SystemState;
        let mut world = World::new();
        let npc = world.spawn(crate::actors::npc::Npc::default()).id();
        let plain = world.spawn(ChildOf(npc)).id();
        let animator = world.spawn((ChildOf(npc), ShaderAnim::default())).id();
        let animated = world.spawn(ChildOf(animator)).id();
        let uv_animated = world.spawn((ChildOf(npc), UvAnim::default())).id();
        let player = world.spawn(crate::network::net::LocalPlayer::default()).id();
        let nested_npc = world.spawn((ChildOf(player), crate::actors::npc::Npc::default())).id();
        let player_part = world.spawn(ChildOf(nested_npc)).id();
        let remote_npc = world.spawn((crate::actors::npc::Npc::default(), crate::network::net::RemotePlayer::default())).id();
        let remote_part = world.spawn(ChildOf(remote_npc)).id();
        let mut state: SystemState<(Query<&ChildOf>, Query<(), NpcRoots>, Query<(), MutableNodes>)> = SystemState::new(&mut world);
        let (parents, roots, blocked) = state.get(&world).unwrap();
        assert!(immutable_npc(plain, &parents, &roots, &blocked));
        for entity in [animated, uv_animated, player_part, remote_part] {
            assert!(!immutable_npc(entity, &parents, &roots, &blocked));
        }
    }

    #[test]
    fn hot_reload_of_either_source_restores_imports_without_overwriting_owned_copy() {
        for change_canonical in [false, true] {
            let mut app = App::new();
            app.init_resource::<Assets<NebulaMaterial>>()
                .init_resource::<NpcPartMaterials>()
                .add_message::<AssetEvent<NebulaMaterial>>()
                .add_systems(Update, share_npc_parts);
            let mut materials = app.world_mut().resource_mut::<Assets<NebulaMaterial>>();
            let first = materials.add(material());
            let second = materials.add(material());
            let owned = materials.add(material());
            let mut cache = NpcPartMaterials::default();
            cache.canonical(&first, &materials);
            cache.canonical(&second, &materials);
            app.insert_resource(cache);
            let imported = app.world_mut().spawn((
                MeshMaterial3d(first.clone()),
                OriginalNpcPartMaterial { original: second.clone(), shared: first.id() },
            )).id();
            let independent = app.world_mut().spawn((
                MeshMaterial3d(owned.clone()),
                OriginalNpcPartMaterial { original: second.clone(), shared: first.id() },
            )).id();
            let changed = if change_canonical { first.id() } else { second.id() };
            app.world_mut().resource_mut::<Assets<NebulaMaterial>>().get_mut(changed).unwrap().extension.params.p1.x = 0.5;
            app.world_mut().write_message(AssetEvent::<NebulaMaterial>::Modified { id: changed });
            app.update();
            assert_eq!(app.world().get::<MeshMaterial3d<NebulaMaterial>>(imported).unwrap().0.id(), second.id());
            assert_eq!(app.world().get::<MeshMaterial3d<NebulaMaterial>>(independent).unwrap().0.id(), owned.id());
            assert!(app.world().get::<OriginalNpcPartMaterial>(imported).is_none());
            assert!(app.world().get::<OriginalNpcPartMaterial>(independent).is_none());
        }
    }

    #[test]
    fn unloading_unused_source_does_not_restore_or_mark_other_mesh_materials() {
        let mut app = App::new();
        app.init_resource::<Assets<NebulaMaterial>>()
            .init_resource::<NpcPartMaterials>()
            .add_message::<AssetEvent<NebulaMaterial>>()
            .add_systems(Update, share_npc_parts);
        let mut materials = app.world_mut().resource_mut::<Assets<NebulaMaterial>>();
        let first = materials.add(material());
        let second = materials.add(material());
        let unused = materials.add(material());
        let mut cache = NpcPartMaterials::default();
        cache.canonical(&first, &materials);
        cache.canonical(&second, &materials);
        cache.canonical(&unused, &materials);
        app.insert_resource(cache);
        let entity = app.world_mut().spawn((
            MeshMaterial3d(first.clone()),
            OriginalNpcPartMaterial { original: second, shared: first.id() },
        )).id();
        app.update();
        let changed = app.world().entity(entity).get_ref::<MeshMaterial3d<NebulaMaterial>>().unwrap().last_changed();
        let removed = unused.id();
        drop(unused);
        app.world_mut().resource_mut::<Assets<NebulaMaterial>>().remove(removed);
        app.world_mut().write_message(AssetEvent::<NebulaMaterial>::Removed { id: removed });
        app.update();
        let material = app.world().entity(entity).get_ref::<MeshMaterial3d<NebulaMaterial>>().unwrap();
        assert_eq!(material.0.id(), first.id());
        assert_eq!(material.last_changed(), changed, "unloading an unrelated source must not dirty render data");
        assert!(app.world().get::<OriginalNpcPartMaterial>(entity).is_some());
        assert!(!app.world().resource::<NpcPartMaterials>().aliases.contains_key(&removed));
    }
}
