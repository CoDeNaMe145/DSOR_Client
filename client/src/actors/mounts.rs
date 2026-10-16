//! Mounts: a ride status effect puts its mount under its holder.
//!
//! The server runs the whole flow (DSOR_Server services/customization.py
//! `_toggle_mount_definition`): using a mount -- from the bag (InventoryItemAction
//! 0x13) or the sticker book (UseStickerBookItemCommand) -- starts its cast effect
//! (cast_mount_*: the holder channels, `hometown_teleport`, drawn by
//! crate::gameplay::skills' status visuals), then its ride effect (ride_mount_*),
//! whose StartModifiers name the mount ("Mounted:Mount,<id>") and its
//! MovementBonus. Using it again, or casting a skill, removes the ride.
//!
//! Here: while a holder has a ride effect, its mount model (_Template_Mount
//! Graphics with its CharacterSet, crate::actors::monsters::spawn_model) stands
//! under it, playing its anim set's Idle or Run with the rider; the rider sits on
//! the mount's `Saddle` joint and plays the same anim set -- its Idle / Run clips
//! have rider twins of the same name in the uniskel skeletons
//! (mount_idle_polarbear_01-loop in both polarbear.glb and uniskel/__animations).
//! The local player walks RUN_SPEED + the bonus (the server's speed: RunSpeed 5,
//! then MovementBonus on top, gameplay/player_combat_stats.py).
//! UNVERIFIED: the rider's attachment is taken as the `mount` joint's transform
//!   (position and rotation) in the mount's frame; the client code that seats
//!   the rider was not traced.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::Deserialize;

use crate::actors::character::{AnimState, Character, CharacterAnim, ModelBones};
use crate::actors::monsters::{json_loader, spawn_model};
use crate::gameplay::skills::{SkillData, StatusTable};
use crate::network::net::{LocalPlayer, Net, RemotePlayer, RUN_SPEED};

#[derive(Deserialize, Clone, Debug)]
pub struct MountDef {
    pub graphics: String,
    pub set: String,
    pub anim_set: String,
    /// The speed its run clip was made for (AnimRunSpeed).
    pub run_speed: f32,
    pub dye: [[f32; 4]; 2],
}

#[derive(Asset, TypePath, Deserialize)]
#[serde(transparent)]
pub struct MountTable(pub HashMap<String, MountDef>);
json_loader!(MountTableLoader, MountTable, "mounts.json");

#[derive(Resource)]
struct Data(Handle<MountTable>);

/// On a holder while its ride effect lasts.
#[derive(Component)]
pub struct Riding {
    instance: u32,
    /// Its MovementBonus (crate::network::net::walk_local adds it).
    pub bonus: f32,
    model: Entity,
    run_speed: f32,
}

/// Ride events not applied yet: their holder or the tables not in.
#[derive(Default)]
struct Pending(Vec<(u32, u16, u32, bool, f32)>);

pub struct MountsPlugin;

impl Plugin for MountsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MountTable>()
            .register_asset_loader(MountTableLoader)
            .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
                commands.insert_resource(Data(assets.load("characters/mounts.json")));
            })
            .add_systems(Update, apply_rides)
            .add_systems(PostUpdate, seat_riders.after(TransformSystems::Propagate));
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_rides(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    skills: Option<Res<SkillData>>,
    statuses: Res<Assets<StatusTable>>,
    data: Res<Data>,
    mounts: Res<Assets<MountTable>>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    actors: Query<(Entity, Option<&LocalPlayer>, Option<&RemotePlayer>), With<Character>>,
    mut riders: Query<(&mut Character, Option<&Riding>)>,
    mut pending: Local<Pending>,
    mut tested: Local<bool>,
) {
    let now = time.elapsed_secs();
    if let Some(mut net) = net {
        // DSOR_TEST_MOUNT=<ride effect id>: the offline player rides it (screenshots).
        if net.offline && !*tested {
            if let (Ok(ride), Some(table)) = (std::env::var("DSOR_TEST_MOUNT"), skills.as_ref().and_then(|s| statuses.get(s.status()))) {
                if let (Some(index), Some(local)) = (table.0.iter().find(|(_, d)| d.id == ride).map(|(i, _)| *i), net.local_actor) {
                    *tested = true;
                    net.mount_events.push((local, index as u16, 1, true));
                }
            }
        }
        pending.0.extend(std::mem::take(&mut net.mount_events).into_iter().map(|(h, i, n, a)| (h, i, n, a, now)));
    }
    if pending.0.is_empty() {
        return;
    }
    let (Some(table), Some(mounts)) = (skills.as_ref().and_then(|s| statuses.get(s.status())), mounts.get(&data.0)) else { return };
    let mut keep = Vec::new();
    for (holder, index, instance, added, at) in std::mem::take(&mut pending.0) {
        let Some(def) = table.0.get(&(index as u32)).filter(|d| !d.mount.is_empty()) else { continue };
        let entity = actors.iter().find_map(|(e, l, r)| {
            (l.is_some_and(|l| l.actor == holder) || r.is_some_and(|r| r.actor == holder)).then_some(e)
        });
        let Some(entity) = entity else {
            // The holder may not be spawned yet (a ride already on at arrival).
            if now - at < 10.0 {
                keep.push((holder, index, instance, added, at));
            }
            continue;
        };
        let Ok((mut character, riding)) = riders.get_mut(entity) else { continue };
        if added {
            if riding.is_some_and(|r| r.instance == instance) {
                continue;
            }
            if let Some(old) = riding {
                commands.entity(old.model).try_despawn();
            }
            let Some(mount) = mounts.0.get(&def.mount) else {
                warn!("ride effect {}: no mount {}", def.id, def.mount);
                continue;
            };
            let model = spawn_model(&mut commands, &assets, &mount.graphics, &mount.set, &mount.anim_set, mount.dye);
            commands.entity(model).insert((Name::new(format!("mount {}", def.mount)), ChildOf(entity)));
            commands.entity(entity).try_insert(Riding { instance, bonus: def.movement_bonus, model, run_speed: mount.run_speed });
            character.riding = Some(mount.anim_set.clone());
            info!("{holder:#x} rides {} (+{} speed)", def.mount, def.movement_bonus);
        } else if let Some(r) = riding.filter(|r| r.instance == instance) {
            commands.entity(r.model).try_despawn();
            commands.entity(entity).remove::<Riding>();
            character.riding = None;
            if let Some(skeleton) = character.skeleton_root() {
                commands.entity(skeleton).try_insert(Transform::IDENTITY);
            }
            info!("{holder:#x} gets off {}", def.mount);
        }
    }
    pending.0 = keep;
}

/// The rider on the saddle, the mount running or standing with it.
fn seat_riders(
    riders: Query<(&GlobalTransform, &Character, &CharacterAnim, &Riding), Without<ModelBones>>,
    mut mounts: Query<(&ModelBones, &mut CharacterAnim), Without<Riding>>,
    globals: Query<&GlobalTransform>,
    mut transforms: Query<&mut Transform>,
) {
    for (root, character, anim, riding) in &riders {
        let Ok((bones, mut mount_anim)) = mounts.get_mut(riding.model) else { continue };
        let running = anim.state == AnimState::Run;
        let state = if running { AnimState::Run } else { AnimState::Idle };
        if mount_anim.state != state {
            mount_anim.state = state;
        }
        // Its run clip at the speed it covers ground (AnimRunSpeed).
        let speed = if running && riding.run_speed > 0.0 { (RUN_SPEED + riding.bonus) / riding.run_speed } else { 1.0 };
        if (mount_anim.speed - speed).abs() > 1e-3 {
            mount_anim.speed = speed;
        }
        // Every mount skeleton has a `mount` joint (polarbear, horse, mount_tiger...);
        // only some also have a Saddle. FAILURE (2026-10-16): seated on Saddle
        // alone, a tiger or horse rider stayed at the root ("je suis dans le sol").
        let seat = bones.0.get("mount").or_else(|| bones.0.get("Saddle")).and_then(|s| globals.get(*s).ok());
        let (Some(saddle), Some(skeleton)) = (seat, character.skeleton_root()) else { continue };
        let local = root.affine().inverse() * saddle.affine();
        if let Ok(mut tf) = transforms.get_mut(skeleton) {
            let seat = Transform::from_matrix(local.into());
            if *tf != seat {
                *tf = seat;
            }
        }
    }
}
