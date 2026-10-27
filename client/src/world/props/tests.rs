use super::*;
use dsor_proto::commands::{
    actors::PropInfo,
    player::{EventUpdate, ScheduleEntry},
    props::{DiscardProp, NewProp, PropUpdate},
};

fn row(key: &str, event: i32) -> Row {
    Row {
        key: key.into(),
        guid: "01010101010101010101010101010101".into(),
        graphics: "props/bench".into(),
        map_icon: String::new(),
        m: Mat4::from_scale_rotation_translation(
            Vec3::splat(2.),
            Quat::IDENTITY,
            Vec3::new(5., 1., 9.),
        )
        .to_cols_array(),
        state: "active".into(),
        event,
        kind: String::new(),
        id: String::new(),
        operation: String::new(),
        quest_controlled: false,
        show_always: false,
    }
}
fn entry() -> PropEntry {
    PropEntry {
        prop_id: 65540,
        guid: vec![1; 16],
        template: "bench".into(),
        team: -1,
        state: 1,
        position: [33.5, 1., -38.8],
        rotation: 0.7,
        task_index: -1,
        quest_type: -1,
        ..default()
    }
}
fn announce(net: &mut NetworkProps, e: PropEntry) {
    net.receive(&ServerCommand::PropInfo(PropInfo { entries: vec![e] }));
}
fn event(net: &mut NetworkProps, key: &str, active: bool, local: bool) {
    net.receive(&ServerCommand::EventUpdate(EventUpdate {
        entries: vec![ScheduleEntry {
            key: key.into(),
            flag: active,
            map_local: local,
            ..default()
        }],
    }));
}

#[test]
fn travel_stone_and_decorative_workbench_exist_without_a_server() {
    // Retail marks the decorative workbenches QuestControlled with ShowAlways
    // off. Offline previews have no quest authority, so show authored scenery.
    let mut deco = row("deco", 120);
    deco.quest_controlled = true;
    let map = MapProps {
        objects: vec![row("stone", -1), row("interactive", 119), deco],
        event_sets: BTreeMap::from([
            (119, "craft=True;newmoon=True;".into()),
            (120, "craft=False;newmoon=False;".into()),
        ]),
        ..default()
    };
    let mut net = NetworkProps::default();
    let keys = |net: &NetworkProps| {
        desired(&map, &map, net)
            .into_iter()
            .map(|r| r.key)
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&net), ["stone", "deco"]);
    event(&mut net, "CRAFT", true, true);
    assert_eq!(keys(&net), ["stone", "interactive"]);
    event(&mut net, "craft", false, true);
    event(&mut net, "newmoon", true, true);
    assert_eq!(keys(&net), ["stone", "interactive"]);
    event(&mut net, "newmoon", false, true);
    assert_eq!(keys(&net), ["stone", "deco"]);
}

#[test]
fn event_ids_are_not_row_indices_and_negative_conditions_veto() {
    let sets = BTreeMap::from([(119, "a=True;b=True;stop=False;".into())]);
    let mut net = NetworkProps::default();
    assert!(!net.event_enabled(119, &sets));
    assert!(!net.event_enabled(0, &sets));
    assert!(net.event_enabled(-1, &sets));
    event(&mut net, "b", true, false);
    assert!(net.event_enabled(119, &sets));
    event(&mut net, "stop", true, true);
    assert!(!net.event_enabled(119, &sets));
}

#[test]
fn server_baseline_unlock_shows_interactive_workbench_and_hides_quest_decorative_copy() {
    let mut deco = row("deco", 120);
    deco.quest_controlled = true;
    let map = MapProps {
        objects: vec![row("stone", -1), row("interactive", 119), deco],
        event_sets: BTreeMap::from([
            (119, "a0200_crafting_workbench_exchange_newmoon=True;a0200_crafting_npc_exchange=True;".into()),
            (120, "a0200_crafting_npc_exchange=False;a0200_crafting_workbench_exchange_newmoon=False;".into()),
        ]), ..default()
    };
    let mut net = NetworkProps::online();
    assert_eq!(desired(&map, &map, &net).len(), 1);
    // services/events.py::_baseline_map_local_event_entries sends this unlock.
    event(&mut net, "a0200_crafting_npc_exchange", true, true);
    let list = desired(&map, &map, &net);
    assert_eq!(
        list.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(),
        ["stone", "interactive"]
    );
}

#[test]
fn guid_binding_reuses_authored_prop_and_keeps_scale_at_absolute_server_position() {
    let map = MapProps {
        objects: vec![row("bench", -1)],
        ..default()
    };
    let mut net = NetworkProps::default();
    announce(&mut net, entry()); // may arrive before MapProps is loaded
    announce(&mut net, entry()); // refresh must not duplicate the model
    let list = desired(&map, &map, &net);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].key, "bench");
    assert_eq!(list[0].transform.translation, Vec3::new(33.5, 1., -38.8));
    assert_eq!(list[0].transform.scale, Vec3::splat(2.));
    assert_eq!(list[0].transform.rotation, Quat::from_rotation_y(0.7));
    net.receive(&ServerCommand::DiscardProp(DiscardProp { prop_id: 65540 }));
    assert!(desired(&map, &map, &net).is_empty());
    announce(&mut net, entry());
    assert_eq!(desired(&map, &map, &net).len(), 1);
}

#[test]
fn quest_controlled_scene_props_wait_for_announcement_unless_show_always() {
    let mut r = row("quest", -1);
    r.quest_controlled = true;
    let mut map = MapProps {
        objects: vec![r],
        ..default()
    };
    assert_eq!(desired(&map, &map, &NetworkProps::default()).len(), 1);
    let mut net = NetworkProps::online();
    assert!(desired(&map, &map, &net).is_empty());
    map.objects[0].show_always = true;
    assert_eq!(desired(&map, &map, &net).len(), 1);
    map.objects[0].show_always = false;
    announce(&mut net, entry());
    assert_eq!(desired(&map, &map, &net).len(), 1);
}

#[test]
fn zero_guid_creates_template_prop_and_updates_select_used_state_and_colour() {
    let map = MapProps {
        templates: HashMap::from([("bench".into(), "props/bench".into())]),
        ..default()
    };
    let mut net = NetworkProps::default();
    let mut e = entry();
    e.guid = vec![0; 16];
    net.receive(&ServerCommand::NewProp(NewProp(e.clone())));
    e.state = 0;
    net.receive(&ServerCommand::NewProp(NewProp(e))); // duplicate NewProp ignored
    assert_eq!(desired(&map, &map, &net)[0].state, "active");
    let mut update = PropUpdate {
        prop_id: 65540,
        state: 3,
        variant: -1,
        ..default()
    };
    net.receive(&ServerCommand::PropUpdate(update.clone()));
    assert_eq!(desired(&map, &map, &net)[0].state, "inactive");
    update.variant = 1;
    net.receive(&ServerCommand::PropUpdate(update.clone()));
    assert_eq!(desired(&map, &map, &net)[0].state, "red");
    update.variant = -1;
    update.state = 1;
    update.keep_visual_state = true;
    net.receive(&ServerCommand::PropUpdate(update));
    assert_eq!(desired(&map, &map, &net)[0].state, "red");
    net.receive(&ServerCommand::DiscardProp(DiscardProp { prop_id: 65540 }));
    assert!(desired(&map, &map, &net).is_empty());
}

#[test]
fn map_change_clears_props_and_local_flags_but_keeps_global_events() {
    let mut net = NetworkProps::default();
    announce(&mut net, entry());
    event(&mut net, "boss_dead", true, true);
    event(&mut net, "christmas", true, false);
    let revision = net.revision;
    net.reset_map();
    assert!(net.entries.is_empty());
    assert!(net.local_events.is_empty());
    assert!(net.global_events.contains_key("christmas"));
    assert!(net.flag_active("christmas"));
    assert!(net.revision > revision);
}

#[test]
fn republishing_a_guid_with_a_new_prop_id_replaces_the_old_discard_tombstone() {
    let map = MapProps {
        objects: vec![row("bench", -1)],
        ..default()
    };
    let mut net = NetworkProps::default();
    announce(&mut net, entry());
    net.receive(&ServerCommand::DiscardProp(DiscardProp { prop_id: 65540 }));
    let mut e = entry();
    e.prop_id = 65550;
    announce(&mut net, e);
    assert_eq!(net.entries.len(), 1);
    assert_eq!(desired(&map, &map, &net).len(), 1);
}

#[test]
fn visual_states_choose_one_group_and_fall_back_for_missing_cooldown() {
    let groups = HashSet::from(["active".into(), "inactive".into(), "goingactive".into()]);
    assert_eq!(selected_state("active", &groups), "active");
    assert_eq!(selected_state("inactive", &groups), "inactive");
    assert_eq!(selected_state("cooldown", &groups), "inactive");
    assert_eq!(selected_state("invalidstate", &groups), "invalidstate");
}

#[test]
fn prop_residency_uses_a_buffer_and_ignores_height() {
    assert!(prop_near(Vec3::new(69., 100., 0.), Vec3::ZERO, false));
    assert!(!prop_near(Vec3::new(80., 0., 0.), Vec3::ZERO, false));
    assert!(prop_near(Vec3::new(80., 0., 0.), Vec3::ZERO, true));
    assert!(!prop_near(Vec3::new(91., 0., 0.), Vec3::ZERO, true));
    assert!(prop_near(Vec3::new(91., 0., 0.), Vec3::new(40., 0., 0.), false));
}

/// FAILURE (2026-10-23): the map viewer (--character, no Net) panicked in
/// `interact` as soon as gameplay was ready.
#[test]
fn prop_clicks_wait_for_a_game_session() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::ui::input::UiInput>().init_resource::<crate::gameplay::dialogue::Dialogue>()
        .init_resource::<PropTarget>()
        .add_systems(Update, interact);
    app.world_mut().spawn((Transform::default(), crate::network::net::LocalPlayer::default()));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.update();
    app.update();
    assert!(app.world().resource::<PropTarget>().pending.is_none());
}

/// dso_server event_system.active_event_flags: a running event gates map content
/// by its name and by its schedule's tags; it stops at its end, which the server
/// never announces.
#[test]
fn running_events_gate_by_name_and_tags_until_their_end() {
    let mut net = NetworkProps::online();
    let now = unix_seconds();
    let date = |at: u64| {
        let (days, secs) = ((at / 86_400) as i64, at % 86_400);
        // Civil date from days since the epoch (the inverse of `epoch`).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        [y as u32, m as u32, d as u32, (secs / 3600) as u32, (secs / 60 % 60) as u32, (secs % 60) as u32]
    };
    assert_eq!(epoch(date(now)), now);
    net.receive(&ServerCommand::EventUpdate(EventUpdate { entries: vec![ScheduleEntry {
        key: "Event_Winter_2026".into(), flag: true, parameters: vec!["christmas_2012".into()], date: date(now + 3600), ..default()
    }] }));
    assert!(net.flag_active("event_winter_2026"));
    assert!(net.flag_active("christmas_2012"), "a tag runs as its event does");
    assert!(net.rule_enabled("christmas_2012=True;"));
    assert!(!net.rule_enabled("christmas_2012_silent=True;"));
    let revision = net.revision;
    assert!(!net.expire(now + 60), "still running");
    assert_eq!(net.revision, revision);
    assert!(net.expire(now + 3600), "its end passed");
    assert!(net.revision > revision);
    assert!(!net.flag_active("christmas_2012") && !net.flag_active("event_winter_2026"));
    // An end already past, or none.
    event(&mut net, "anniversary_event", true, false);
    assert!(net.flag_active("anniversary_event"));
    net.receive(&ServerCommand::EventUpdate(EventUpdate { entries: vec![ScheduleEntry {
        key: "halloween_event_01".into(), flag: true, date: date(now - 60), ..default()
    }] }));
    assert!(!net.flag_active("halloween_event_01"));
}

/// ExitInfo (68): every exit of the map, active or not, by Guid; merged by name.
#[test]
fn exit_info_says_which_exits_lead_somewhere_until_the_map_changes() {
    use dsor_proto::commands::player::{Exit, ExitInfo};
    let mut net = NetworkProps::online();
    assert!(!net.exits_announced());
    assert_eq!(net.exit_active("exit_general_01", "AB01"), None);
    net.receive(&ServerCommand::ExitInfo(ExitInfo { exits: vec![
        Exit { name: "Exit_General_01".into(), destination: "a0302_wildforest".into(), value: 1, guid: vec![0xAB, 0x01] },
        Exit { name: "a0200_to_e_hal_01".into(), destination: "e_hal_01_pumpkin_dun".into(), value: 0, guid: vec![0xCD, 0x02] },
    ] }));
    assert!(net.exits_announced());
    assert_eq!(net.exit_active("someone_else", "ab01"), Some(true), "by Guid first");
    assert_eq!(net.exit_active("exit_general_01", ""), Some(true), "else by name");
    assert_eq!(net.exit_active("a0200_to_e_hal_01", "CD02"), Some(false));
    assert_eq!(net.exit_active("never_sent", "EEEE"), None);
    net.reset_map();
    assert!(!net.exits_announced());
}
