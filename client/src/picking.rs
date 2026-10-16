//! What the cursor picks an actor by: the oriented boxes the 2018 client binds to
//! its joints (`characters/picking.json`, tools/export_picking.py), following
//! the animated pose.
//!
//! EVIDENCE: ActorPickingProperty::SetupScaledPickingShapes (0x4B894B): each
//!   rigid body "<joint>_Picking" of the model's picking composite rides that
//!   joint, scaled by the joint's scale. Bundles: picking/monster.pkg, npc.pkg.
//! UNVERIFIED: actors whose Graphics has no entry (the human NPCs and players)
//!   are picked otherwise in the client (PickingProperty, not traced): they keep
//!   their callers' cylinder.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct PickBox {
    joint: String,
    pos: [f32; 3],
    /// x y z w
    rot: [f32; 4],
    /// Full edges.
    size: [f32; 3],
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct PickingShapes(pub HashMap<String, Vec<PickBox>>);

crate::monsters::json_loader!(PickingShapesLoader, PickingShapes, "picking.json");

#[derive(Resource)]
struct PickingFile(Handle<PickingShapes>);

/// The actor's Graphics ("characters/<model>"): its picking boxes' key.
#[derive(Component)]
pub struct PickModel(pub String);

/// The actor's boxes bound to its joints: (joint, box in joint space, half edges).
/// Missing until the model's joints are in; an actor without an entry never
/// gets one.
#[derive(Component)]
pub struct PickBoxes(pub Vec<(Entity, Transform, Vec3)>);

pub struct PickingPlugin;

impl Plugin for PickingPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<PickingShapes>()
            .register_asset_loader(PickingShapesLoader)
            .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
                commands.insert_resource(PickingFile(assets.load("characters/picking.json")));
            })
            .add_systems(Update, bind.after(crate::npc::NpcVisualSystems));
    }
}

/// Bind each box to its joint once the model's joints are spawned.
fn bind(
    mut commands: Commands,
    file: Res<PickingFile>,
    shapes: Res<Assets<PickingShapes>>,
    actors: Query<(Entity, &PickModel), Without<PickBoxes>>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    let Some(shapes) = shapes.get(&file.0) else { return };
    for (e, model) in &actors {
        let Some(boxes) = shapes.0.get(&model.0) else {
            commands.entity(e).try_insert(PickBoxes(Vec::new()));
            continue;
        };
        let mut joints: HashMap<&str, Entity> = HashMap::new();
        for d in children.iter_descendants(e) {
            if let Ok(n) = names.get(d) {
                joints.entry(n.as_str()).or_insert(d);
            }
        }
        // Every joint in, or the model is still loading.
        if !boxes.iter().all(|b| joints.contains_key(b.joint.as_str())) {
            continue;
        }
        let bound = boxes
            .iter()
            .map(|b| {
                let local = Transform::from_translation(Vec3::from(b.pos)).with_rotation(Quat::from_array(b.rot).normalize());
                (joints[b.joint.as_str()], local, Vec3::from(b.size) * 0.5)
            })
            .collect();
        commands.entity(e).try_insert(PickBoxes(bound));
    }
}

/// The ray's nearest entry into any of the boxes (in units of `ray.direction`),
/// None when it misses them all or the actor has no boxes.
pub fn hit(ray: Ray3d, boxes: &PickBoxes, joints: &Query<&GlobalTransform>) -> Option<f32> {
    let mut best: Option<f32> = None;
    for (joint, local, half) in &boxes.0 {
        let Ok(at) = joints.get(*joint) else { continue };
        let m = at.affine() * local.compute_affine();
        let inv = m.inverse();
        let o = inv.transform_point3(ray.origin);
        let d = inv.transform_vector3(*ray.direction);
        let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
        for i in 0..3 {
            if d[i].abs() < 1e-8 {
                if o[i].abs() > half[i] {
                    near = f32::INFINITY;
                    break;
                }
                continue;
            }
            let (a, b) = ((-half[i] - o[i]) / d[i], (half[i] - o[i]) / d[i]);
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
        if near <= far && far >= 0.0 {
            let t = near.max(0.0);
            best = Some(best.map_or(t, |b: f32| b.min(t)));
        }
    }
    best
}

