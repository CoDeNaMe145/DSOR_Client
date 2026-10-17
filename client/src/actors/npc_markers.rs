//! The original camera-facing, additive status models above NPCs. Their meshes
//! include the overhead height (~2.97 units) and glow; only DB4's per-NPC offset
//! is added here. Quest selection is shared with the minimap.

use super::npc::{Npc, NpcTemplate, NpcVisualPending, NpcVisualSystems};
use crate::ui::MapMarkers;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstance, WorldInstanceSpawner};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Style {
    model: &'static str,
    color: [u8; 3],
}

#[derive(Component)]
struct OverheadMarker(Style);

#[derive(Component)]
struct PendingMarker;

pub(super) struct NpcMarkersPlugin;

impl Plugin for NpcMarkersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (sync, prepare).chain().after(NpcVisualSystems));
    }
}

fn service_model(profile: &NpcTemplate) -> Option<&'static str> {
    // Profession is independent of LocationMapIcon: alchemists, for example,
    // share the map's merchant bag but have their own overhead status model.
    let model = match profile.profession.as_str() {
        "monger" | "food_recycling" | "reward_messenger" => "status_merchant_01",
        "privileged_trader" => "status_premium_trader_01",
        "simple_smith" => "status_smith_01",
        "smith" => "status_smith_03",
        "priest" => "status_priest_01",
        "alchemist" => "status_alchi_01",
        "equip" => "status_theurgic_merchant_01",
        "banker" => "status_banker_01",
        "mount_trader" => "status_mount_trader_01",
        "tailor" => "status_tailor_01",
        "event_manager" => "status_eventmanager_01",
        "essence" => "status_essence_01",
        "jewelry" => "status_jeweler_01",
        "gambler_trader" => "status_clover_01",
        "arena_merchant" => "status_arena_01",
        "fragment_trader" => "status_merchant_materi",
        "crafting" | "crafting_cake" => "status_crafting_01",
        _ => match profile.map_icon.replace('\\', "/").as_str() {
            // Older sidecars may omit Profession.
            "ui/icon_map_merchant" => "status_merchant_01",
            "ui/icon_map_merchant_materi" => "status_merchant_materi",
            "ui/icon_map_theurgic_merchant" => "status_theurgic_merchant_01",
            "ui/icon_map_trader_privileged" => "status_premium_trader_01",
            "ui/icon_map_smith_01" => "status_smith_01",
            "ui/icon_map_smith_02" => "status_smith_02",
            "ui/icon_map_smith_03" => "status_smith_03",
            "ui/icon_map_jewel" => "status_jeweler_01",
            "ui/icon_map_priest" => "status_priest_01",
            "ui/icon_map_banker" => "status_banker_01",
            "ui/icon_map_essence" => "status_essence_01",
            "ui/icon_map_tailor" => "status_tailor_01",
            "ui/icon_map_clover" => "status_clover_01",
            "ui/icon_map_arenamaster" => "status_arena_01",
            "ui/icon_map_trainermage" => "status_trainer_mage_01",
            "ui/icon_map_trainerranger" => "status_trainer_ranger_01",
            "ui/icon_map_trainerwarrior" => "status_trainer_warrior_01",
            "ui/icon_map_trainerdwarf" => "status_trainer_dwarf_01",
            _ => return None,
        },
    };
    Some(model)
}

fn style(npc: &Npc, state: &MapMarkers) -> Option<Style> {
    let service = service_model(&npc.profile);
    // The shared selector also suppresses disabled NPCs. An empty service still
    // allows a quest-only NPC to show an offer or a ready interaction.
    let selected = state.npc_style(npc.actor, service.unwrap_or_default())?;
    let model = match selected.icon.as_str() {
        "ui/icon_map_exclamationmark" => "status_exclamationmark_01",
        "ui/icon_map_questionmark" => "status_questionmark_01",
        _ => service?,
    };
    Some(Style { model, color: selected.color })
}

fn sync(
    mut commands: Commands,
    npcs: Query<(Entity, &Npc, Has<NpcVisualPending>)>,
    mut markers: Query<(Entity, &ChildOf, &OverheadMarker, &mut Transform)>,
    net: Option<NonSend<crate::network::net::Net>>,
    assets: Res<AssetServer>,
) {
    let empty = MapMarkers::default();
    let state = net.as_ref().map(|n| &n.map_markers).unwrap_or(&empty);
    let mut wanted: HashMap<_, _> = npcs.iter().filter_map(|(entity, npc, pending)| {
        if pending { return None; }
        style(npc, state).map(|style| (entity, (style, Vec3::from(npc.profile.overhead_offset))))
    }).collect();
    for (entity, parent, marker, mut at) in &mut markers {
        match wanted.get(&parent.parent()) {
            Some((style, offset)) if marker.0 == *style => {
                if at.translation != *offset { at.translation = *offset; }
                wanted.remove(&parent.parent());
            }
            _ => { commands.entity(entity).despawn(); }
        }
    }
    for (parent, (style, offset)) in wanted {
        commands.spawn((
            Name::new(format!("NPC overhead {}", style.model)),
            OverheadMarker(style), PendingMarker,
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(format!("t000_general/{}.glb", style.model)))),
            Transform::from_translation(offset), Visibility::Hidden, ChildOf(parent),
            Pickable::IGNORE, crate::rendering::highlight::NoHighlight,
        ));
    }
}

fn prepare(
    mut commands: Commands,
    pending: Query<(Entity, &OverheadMarker, Option<&WorldInstance>), With<PendingMarker>>,
    instances: Res<WorldInstanceSpawner>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, marker, instance) in &pending {
        if !instance.is_some_and(|i| instances.instance_is_ready(**i)) { continue; }
        let parts: Vec<_> = children.iter_descendants(entity).filter(|e| meshes.contains(*e)).collect();
        if parts.is_empty() || parts.iter().any(|e| meshes.get(*e).is_ok_and(|m| !materials.contains(&m.0))) { continue; }
        for part in parts {
            let mut handle = meshes.get_mut(part).unwrap();
            // Status quads were authored facing -Z with reversed U. FacesViewer
            // aligns them to the camera's +Z, so restore readable symbols here.
            // Keep the imported material unchanged for other scene instances.
            let mut material = materials.get(&handle.0).unwrap().clone();
            material.uv_transform *= bevy::math::Affine2::from_scale_angle_translation(
                Vec2::new(-1.0, 1.0), 0.0, Vec2::X,
            );
            if marker.0.color != [255; 3] {
                // Never tint the shared glTF material: different NPCs can have
                // different quest categories while using the same model.
                let [r, g, b] = marker.0.color;
                let tint = Color::srgb_u8(r, g, b).to_linear();
                let base = material.base_color.to_linear();
                material.base_color = LinearRgba::new(base.red * tint.red, base.green * tint.green, base.blue * tint.blue, base.alpha).into();
            }
            handle.0 = materials.add(material);
            commands.entity(part).insert((Pickable::IGNORE, bevy::light::NotShadowCaster));
        }
        commands.entity(entity).remove::<PendingMarker>().insert(Visibility::Inherited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_proto::commands::{actors::{NpcInfo, NpcInteraction, QuestUpdate}, ServerCommand};

    #[test]
    fn quest_markers_replace_services_only_when_an_interaction_is_ready() {
        let npc = Npc { actor: 7, profile: NpcTemplate { profession: "monger".into(), ..default() }, ..default() };
        let mut state = MapMarkers::default();
        assert_eq!(style(&npc, &state).unwrap().model, "status_merchant_01");
        let info = NpcInfo { enabled: true, interactions: vec![
            NpcInteraction { name: "quest".into(), kind: 0, state: 6, ..default() },
        ], ..default() };
        state.receive(&ServerCommand::NpcInfo(info.clone()), Some(7));
        assert_eq!(style(&npc, &state).unwrap(), Style { model: "status_exclamationmark_01", color: [255, 215, 65] });
        let update = |value| ServerCommand::QuestUpdate(QuestUpdate {
            kind: 2, ints: vec![(2, value)], strings: vec![(0, "QUEST".into())],
        });
        state.receive(&update(0), None);
        assert_eq!(style(&npc, &state).unwrap().model, "status_merchant_01", "unfinished quests are not turn-ins");
        state.receive(&update(1), None);
        assert_eq!(style(&npc, &state).unwrap().model, "status_questionmark_01");
        let mut another_offer = info.clone();
        another_offer.interactions.push(NpcInteraction { name: "other".into(), kind: 0, ..default() });
        state.receive(&ServerCommand::NpcInfo(another_offer), Some(7));
        assert_eq!(style(&npc, &state).unwrap().model, "status_questionmark_01", "turn-ins win over offers");
        state.receive(&ServerCommand::NpcInfo(info), Some(7));
        state.receive(&update(2), None);
        assert_eq!(style(&npc, &state).unwrap().model, "status_merchant_01");
        state.receive(&update(3), None);
        assert_eq!(style(&npc, &state).unwrap().model, "status_exclamationmark_01");
        state.receive(&ServerCommand::NpcInfo(NpcInfo::default()), Some(7));
        assert_eq!(style(&npc, &state), None);
        state.reset_map();
        assert_eq!(style(&npc, &state).unwrap().model, "status_merchant_01");
    }

    #[test]
    fn quest_only_npcs_and_legacy_service_metadata_are_supported() {
        let mut npc = Npc { actor: 9, ..default() };
        let mut state = MapMarkers::default();
        assert_eq!(style(&npc, &state), None);
        state.receive(&ServerCommand::NpcInfo(NpcInfo { enabled: true, interactions: vec![
            NpcInteraction { name: "quest".into(), kind: 0, ..default() },
        ], ..default() }), Some(9));
        assert_eq!(style(&npc, &state).unwrap().model, "status_exclamationmark_01");
        npc.profile.map_icon = "ui\\icon_map_merchant".into();
        assert_eq!(service_model(&npc.profile), Some("status_merchant_01"));
        npc.profile.profession = "alchemist".into();
        assert_eq!(service_model(&npc.profile), Some("status_alchi_01"));
    }

    #[test]
    fn markers_follow_actor_lifetime_and_visual_streaming() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<bevy::world_serialization::WorldAsset>().add_systems(Update, sync);
        let npc = app.world_mut().spawn((Npc { profile: NpcTemplate {
            profession: "monger".into(), overhead_offset: [0.5, 1.5, -0.25], ..default()
        }, ..default() }, Transform::default(), Visibility::Inherited)).id();
        let mut markers = app.world_mut().query_filtered::<(Entity, &ChildOf, &Transform), With<OverheadMarker>>();
        app.update();
        let (first, parent, at) = markers.single(app.world()).unwrap();
        assert_eq!(parent.parent(), npc);
        assert_eq!(at.translation, Vec3::new(0.5, 1.5, -0.25));
        app.update();
        assert_eq!(markers.single(app.world()).unwrap().0, first, "unchanged markers are reused");
        app.world_mut().entity_mut(npc).insert(NpcVisualPending);
        app.update();
        assert_eq!(markers.iter(app.world()).count(), 0);
        app.world_mut().entity_mut(npc).remove::<NpcVisualPending>();
        app.update();
        assert_eq!(markers.iter(app.world()).count(), 1);
        app.world_mut().entity_mut(npc).get_mut::<Npc>().unwrap().profile.profession = "smith".into();
        app.update();
        assert_eq!(markers.iter(app.world()).count(), 1);
        assert_ne!(markers.single(app.world()).unwrap().0, first);
        app.world_mut().despawn(npc);
        assert_eq!(markers.iter(app.world()).count(), 0);
    }
}
