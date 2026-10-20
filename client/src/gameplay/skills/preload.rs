//! Retain the selected class's effects and summon variants, including their
//! status/hit/death sequences. Requests are bounded and yield to map loading.
use super::*;
use std::collections::{BTreeSet, VecDeque};
use crate::actors::{monsters::{MonsterTemplates, ModelSets, PlayerHits}, npc::Outfits};
use crate::screens::loading::asset_settled;

const MAX_IN_FLIGHT: usize = 4;

#[derive(Resource)]
pub(super) struct Catalog {
    monsters: Handle<MonsterTemplates>,
    hits: Handle<PlayerHits>,
    outfits: [Handle<Outfits>; 2],
}

#[derive(Resource, Default)]
pub(crate) struct FxPreload {
    pub(crate) class: Option<u8>,
    planned: bool,
    pending: VecDeque<String>,
    pub(crate) models: Vec<Handle<Gltf>>,
    sets: Vec<Handle<ModelSets>>,
}

impl FxPreload {
    pub(crate) fn progress(&self, assets: &AssetServer) -> (usize, usize) {
        let done = self.models.iter().filter(|h| asset_settled(assets, h.id().untyped())).count()
            + self.sets.iter().filter(|h| asset_settled(assets, h.id().untyped())).count();
        (done, self.pending.len() + self.models.len() + self.sets.len())
    }

    pub(crate) fn ready(&self, class: Option<u8>, assets: &AssetServer) -> bool {
        class.is_none() || (self.class == class && self.planned && {
            let (done, total) = self.progress(assets);
            done == total
        })
    }
}

pub(super) fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Catalog {
        monsters: assets.load("characters/monster_templates.json"),
        hits: assets.load("characters/player_hits.json"),
        outfits: ["uniskel", "uniskel_dwarf"].map(|s| assets.load(format!("characters/{s}/outfits.json"))),
    });
}

fn collect(class: &str, table: &SkillTable, statuses: Option<&StatusTable>, seqs: &SequenceTable,
    monsters: Option<&MonsterTemplates>, hits: Option<&PlayerHits>, outfits: [Option<&Outfits>; 2]) -> BTreeSet<String> {
    let prefix = format!("{class}_");
    let mut names = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let statuses: HashMap<_, _> = statuses.into_iter().flat_map(|s| s.0.values()).map(|s| (s.id.as_str(), s)).collect();
    for skill in table.0.values().filter(|s| s.class == class || s.id.starts_with(&prefix)) {
        names.extend(skill.execute.values().map(String::as_str));
        names.extend([skill.impact.as_str(), skill.shifted.as_str()]);
        if let Some(b) = &skill.bullet { names.extend([b.loop_seq.as_str(), b.impact.as_str(), b.death.as_str()]); }
        for (id, _) in skill.user_status.iter().chain(&skill.victim_status).chain(&skill.location_status) {
            if let Some(s) = statuses.get(id.as_str()) {
                names.extend(s.start.values().chain(s.tick.values()).chain(s.done.values()).chain(s.stop.values()).map(String::as_str));
            }
        }
        if let Some(monster) = skill.summon.as_ref().and_then(|s| monsters?.0.get(&s.id)) {
            names.extend(monster.hit.values().map(String::as_str));
            names.extend([monster.critical.as_str(), monster.heavy.as_str(), monster.death.as_str(), monster.summon.as_str()]);
            if let Some(index) = ["characters/uniskel", "characters/uniskel_dwarf"].iter().position(|s| *s == monster.graphics) {
                if let Some(outfit) = outfits[index].and_then(|all| all.outfits.iter().find(|o| o.name == monster.set)) {
                    paths.extend(outfit.parts.iter().map(|part| format!("{}/parts/{part}.glb", monster.graphics)));
                }
            } else if !monster.graphics.is_empty() {
                paths.insert(crate::rendering::model_variant::path(&monster.graphics, &monster.set));
                paths.insert(format!("{}.sets.json", monster.graphics));
            }
        }
    }
    if let Some(hit) = hits.and_then(|h| h.0.get(class)) {
        names.extend(hit.hit.values().map(String::as_str));
        names.extend([hit.critical.as_str(), hit.heavy.as_str()]);
    }
    for name in names {
        for track in seqs.0.get(name).into_iter().flat_map(|s| &s.tracks) {
            if let Track::Fx { graphics, .. } = track {
                if !graphics.is_empty() { paths.insert(format!("{graphics}.glb")); }
            }
        }
    }
    paths
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(mut preload: ResMut<FxPreload>, data: Option<Res<SkillData>>, catalog: Res<Catalog>,
    tables: Res<Assets<SkillTable>>, statuses: Res<Assets<StatusTable>>, sequences: Res<Assets<SequenceTable>>,
    monsters: Res<Assets<MonsterTemplates>>, hits: Res<Assets<PlayerHits>>, outfits: Res<Assets<Outfits>>,
    assets: Res<AssetServer>, locals: Query<&Character, With<LocalPlayer>>, others: Query<&Character>,
    map: Option<Res<crate::world::map::CurrentMap>>, props: Option<Res<crate::world::props::PropScene>>) {
    // A map change despawns every actor before the next map's player arrives.
    // Keep the class's handles through that gap: dropping them would unload the
    // effect models and request them all again on every transition.
    let class = locals.iter().next().or_else(|| others.iter().next()).map(|c| c.desc.class);
    if class.is_some() && class != preload.class { *preload = FxPreload { class, ..default() }; }
    let Some(class) = preload.class else { return };
    let Some(data) = data else { return };
    if !preload.planned {
        let handles = [data.skills.id().untyped(), data.status.id().untyped(), data.sequences.id().untyped(),
            catalog.monsters.id().untyped(), catalog.hits.id().untyped(),
            catalog.outfits[0].id().untyped(), catalog.outfits[1].id().untyped()];
        if !handles.into_iter().all(|h| asset_settled(&assets, h)) { return; }
        preload.planned = true;
        let (Some(table), Some(seqs)) = (tables.get(&data.skills), sequences.get(&data.sequences)) else {
            warn!("effect preload: skill catalog unavailable"); return;
        };
        let class_name = crate::actors::character::CLASSES.get(class as usize).copied().unwrap_or("mage");
        preload.pending = collect(class_name, table, statuses.get(&data.status), seqs,
            monsters.get(&catalog.monsters), hits.get(&catalog.hits), catalog.outfits.each_ref().map(|h| outfits.get(h))).into_iter().collect();
        info!("preload: {} assets for {class_name} skills, statuses and summons", preload.pending.len());
    }
    if preload.pending.is_empty() { return; }
    // Don't add background GLB jobs to the initial geometry/prop burst. The
    // per-batch limit counts recursive texture dependencies, not just parsing.
    if let Some(m) = map.as_ref() {
        let (done, total) = m.progress(&assets);
        if !m.spawned || done != total || props.as_ref().is_some_and(|p| !p.ready(m, &assets)) { return; }
    }
    let (done, _) = preload.progress(&assets);
    let active = preload.models.len() + preload.sets.len() - done;
    for _ in active..MAX_IN_FLIGHT {
        let Some(path) = preload.pending.pop_front() else { break };
        if path.ends_with(".sets.json") { preload.sets.push(assets.load(path)); }
        else { preload.models.push(assets.load(path)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn includes_status_lifecycle_summon_variants_and_hit_effects_without_other_classes() {
        let skill: SkillDef = serde_json::from_value(json!({
            "id":"spell", "class":"mage", "type":"Ranged", "hit_frame":0,
            "loop_start":0,"unblock":1,"motion_unblock":1,"range":10.,"cooldown":1.,
            "execute":{"*":"cast"},"impact":"impact", "shifted":"ground",
            "bullet":{"loop":"flight","impact":"impact","death":"bullet_end",
                "motion":"Straight","velocity":1.,"lifetime":1.,"radius":1.,"start":[0.,0.,0.]},
            "user_status":[["burn",1.]],"victim_status":[["burn",1.]],
            "summon":{"id":"pet","amount":1,"offset":[0.,0.,0.],"delay":0}
        })).unwrap();
        let mut unrelated = skill.clone(); unrelated.class = "ranger".into(); unrelated.execute = HashMap::from([("*".into(), "other".into())]);
        let table = SkillTable(HashMap::from([(1, skill), (2, unrelated)]));
        let statuses: StatusTable = serde_json::from_value(json!({"1":{
            "id":"burn","duration":1.,"start":{"*":"status_start"},"tick":{"*":"status_tick"},
            "done":{"*":"status_end"},"stop":{"*":"status_stop"}
        }})).unwrap();
        let monsters: MonsterTemplates = serde_json::from_value(json!({"pet":{
            "graphics":"characters/pet","set":"one","anim_set":"pet","state":"Idle","loop":true,
            "hit":{"Fire":"pet_hit"},"critical":"pet_critical","heavy":"pet_heavy","death":"pet_death","summon":"pet_spawn"
        }})).unwrap();
        let hits: PlayerHits = serde_json::from_value(json!({"mage":{"hit":{"Fire":"player_hit"},"critical":"player_critical","heavy":"player_heavy"}})).unwrap();
        let expected = ["cast", "impact", "ground", "flight", "bullet_end", "status_start", "status_tick", "status_end", "status_stop",
            "pet_hit", "pet_critical", "pet_heavy", "pet_death", "pet_spawn", "player_hit", "player_critical", "player_heavy"];
        let seqs = SequenceTable(expected.into_iter().chain(["other"]).map(|name| (name.into(), Sequence {
            length: 10, repeat: false, tracks: vec![Track::Fx { graphics: format!("effects/{name}"), joint: String::new(),
                at: [0.; 9], start: 0, end: 10, curves: HashMap::new() }],
        })).collect());
        let paths = collect("mage", &table, Some(&statuses), &seqs, Some(&monsters), Some(&hits), [None, None]);
        assert_eq!(paths.len(), expected.len() + 2);
        for name in expected { assert!(paths.contains(&format!("effects/{name}.glb")), "{name}"); }
        assert!(paths.contains(&crate::rendering::model_variant::path("characters/pet", "one")));
        assert!(paths.contains("characters/pet.sets.json"));
        assert!(!paths.contains("characters/pet.glb"), "must not retain every skin variant");
        assert!(!paths.contains("effects/other.glb"));
    }

    #[test]
    fn queued_unrequested_assets_and_changed_classes_keep_loading_open() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        let assets = app.world().resource::<AssetServer>();
        let mut preload = FxPreload { class: Some(1), planned: true,
            pending: VecDeque::from(["effects/pending.glb".into()]), ..default() };
        assert_eq!(preload.progress(assets), (0, 1));
        assert!(!preload.ready(Some(1), assets));
        preload.pending.clear();
        assert!(preload.ready(Some(1), assets));
        assert!(!preload.ready(Some(2), assets));
        preload.planned = false;
        assert!(!preload.ready(Some(1), assets), "metadata planning is part of readiness");
        assert!(preload.ready(None, assets), "a map viewer needs no class assets");
    }

    #[test]
    fn a_map_change_without_actors_keeps_the_class_preload_until_another_class_appears() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<SkillTable>().init_asset::<StatusTable>().init_asset::<SequenceTable>()
            .init_asset::<MonsterTemplates>().init_asset::<PlayerHits>().init_asset::<Outfits>()
            .insert_resource(Catalog { monsters: Handle::default(), hits: Handle::default(), outfits: [Handle::default(), Handle::default()] })
            .insert_resource(FxPreload { class: Some(1), planned: true, models: vec![Handle::default()], ..default() });
        app.world_mut().run_system_once(update).unwrap();
        let preload = app.world().resource::<FxPreload>();
        assert_eq!((preload.class, preload.planned, preload.models.len()), (Some(1), true, 1),
            "the old map's actors are gone and the next player has not arrived: keep the handles");
        // A bystander of the same class changes nothing; the local player's
        // class takes precedence and replans.
        for (class, local) in [(1u8, false), (2, true)] {
            app.world_mut().run_system_once(move |mut commands: Commands| {
                let entity = crate::actors::character::spawn_character(&mut commands,
                    crate::actors::character::CharacterDesc { class, ..default() }, Transform::default());
                if local { commands.entity(entity).insert(LocalPlayer::default()); }
            }).unwrap();
            app.world_mut().run_system_once(update).unwrap();
            let preload = app.world().resource::<FxPreload>();
            let expected = if local { (Some(2), false, 0) } else { (Some(1), true, 1) };
            assert_eq!((preload.class, preload.planned, preload.models.len()), expected, "class {class}");
        }
    }
}
