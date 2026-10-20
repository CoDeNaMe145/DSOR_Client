//! The party frames (groupwindow): one row per member of our group, as the server's
//! GroupStatus (151) last described it, with the health of those on our map.
//!
//! The authored window is a template: a header (the group's name) and one player
//! row (name, level ring, health and resource bars, a spirit stone button), plus a
//! bottom row carrying the panel's lower edge. `prepare` clones the row per member
//! and stretches the backdrop; `sync` rebuilds the window when the members change
//! and keeps their health fresh for the gauges (crate::ui::Gauge::Group).
//! SOURCE: dso-server-production services/social.py (GroupStatus goes to every
//!   member on a join, a leave of our own and map changes; never on a hit), so a
//!   member's health is what we saw of its actor (HitCommand victims); a member on
//!   another map has actor 0 and no health here.
use bevy::prelude::*;

use super::template::{grey, map_y, set_text};
use crate::ui::{Figures, Layout, Loaded, Window2018};

pub(in crate::ui) const WINDOW: &str = "groupwindow";
/// DSOR_Server persistence/social_state.py GROUP_MEMBER_LIMIT.
pub(crate) const MOST_MEMBERS: usize = 5;
const ROW: &str = "Canvas/Label_bg/LabelFull/TemplatePlayer";
const LAST_ROW: &str = "Canvas/Label_bg/LabelFull/TemplatePlayerBottom";
const BACKDROP: &str = "Canvas/Label_bg";
/// A row's authored height (TemplatePlayer: 418 to 466).
const ROW_HEIGHT: f32 = 48.0;
/// The parts of a row this client does not drive: friend requests, the spirit stone
/// (a resurrection), group buffs, the resource bars (another player's resource is
/// never sent), the short title and the leader's title variants.
const UNUSED: [&str; 11] = ["ButtonAddToFriends", "SpiritStoneButtonLayer", "AnimationSpiritstone", "GroupBuffIndicator",
    "ManaBar", "RageBar", "StaminaBar", "MechanicResourceBar", "TitleShort", "LeaderTitleLong", "LeaderTitleShort"];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Member {
    pub name: String,
    pub level: u16,
    pub leader: bool,
    pub me: bool,
    /// The member's actor on our map; 0 elsewhere.
    pub actor: u32,
    pub online: bool,
    pub map: String,
    /// (current, maximum); (0, 0) when unknown.
    pub health: (f32, f32),
}

/// Our party as shown.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct Party {
    pub name: String,
    pub members: Vec<Member>,
}

impl Party {
    /// A member's health fraction for its gauge: 0 when unknown or elsewhere.
    pub(in crate::ui) fn health(&self, index: usize) -> f32 {
        self.members.get(index).map_or(0.0, |m| if m.health.1 > 0.0 { (m.health.0 / m.health.1).clamp(0.0, 1.0) } else { 0.0 })
    }
    /// What the window's geometry depends on (health only moves the gauges).
    fn shape(&self) -> Vec<(String, u16, bool, bool, bool)> {
        let mut shape = vec![(self.name.clone(), 0, false, false, false)];
        shape.extend(self.members.iter().map(|m| (m.name.clone(), m.level, m.leader, m.online, m.actor != 0)));
        shape
    }
}

/// The party from the network state; the window is rebuilt when its rows change.
pub(in crate::ui) fn sync(
    mut commands: Commands,
    net: Option<NonSend<crate::network::net::Net>>,
    figures: Res<Figures>,
    mut party: ResMut<Party>,
    mut loaded: ResMut<Loaded>,
    layouts: Res<Assets<Layout>>,
    windows: Query<(Entity, &Window2018)>,
    mut shape: Local<Option<Vec<(String, u16, bool, bool, bool)>>>,
) {
    let mut next = Party::default();
    if let Some(net) = net.as_ref() {
        if let Some(group) = net.group.as_ref().filter(|g| g.grouped()) {
            next.name = group.name.clone();
            for m in group.members.iter().take(MOST_MEMBERS) {
                let me = m.player_id == group.recipient_id;
                let health = if me { figures.health }
                    else if m.actor != 0 { net.monster_vitals.get(&m.actor).map_or((0.0, 0.0), |&(h, max)| (h as f32, max as f32)) }
                    else { (0.0, 0.0) };
                next.members.push(Member { name: m.name.clone(), level: m.level, leader: m.leads(), me, actor: m.actor, online: m.online, map: m.map_name.clone(), health });
            }
        }
    }
    let next_shape = next.shape();
    if *party != next { *party = next; }
    if shape.as_ref() == Some(&next_shape) { return; }
    *shape = Some(next_shape);
    for (entity, window) in &windows {
        if window.0 == WINDOW { commands.entity(entity).despawn(); }
    }
    for i in 0..loaded.layouts.len() {
        if layouts.get(&loaded.layouts[i].0).is_some_and(|l| l.window == WINDOW) { loaded.spawned[i] = false; }
    }
}

fn stretch(x: f32, top: f32, bottom: f32, extra: f32) -> f32 {
    x + if x <= top { 0.0 } else if x >= bottom { extra } else { extra * (x - top) / (bottom - top) }
}

/// The window for this party: nothing while we are not grouped.
pub(in crate::ui) fn prepare(source: &Layout, party: &Party) -> Layout {
    let mut layout = Layout { window: source.window.clone(), widgets: Vec::new() };
    if party.members.is_empty() { return layout; }
    let rows = party.members.len();
    let extra = (rows as f32 - 1.0) * ROW_HEIGHT;
    for widget in &source.widgets {
        let id = &widget.id;
        // The compact mode and its toggles, the group level, and the row templates.
        if id.contains("/LabelCompact") || id.contains("/ButtonToggleMode") || id.contains("/GroupLevel")
            || id == ROW || id.starts_with(&format!("{ROW}/")) || id == LAST_ROW || id.starts_with(&format!("{LAST_ROW}/")) { continue; }
        let mut w = widget.clone();
        if id.ends_with("/BackroundTop/Groupname") { set_text(&mut w, &party.name); }
        // The backdrop is authored for one row: it grows with the others.
        if id == BACKDROP { map_y(&mut w, |y| stretch(y, 400.0, 420.0, extra)); }
        layout.widgets.push(w);
    }
    for (index, member) in party.members.iter().enumerate() {
        let template = if index + 1 == rows { LAST_ROW } else { ROW };
        let replacement = format!("Canvas/Label_bg/LabelFull/GroupMember{index}");
        let dy = index as f32 * ROW_HEIGHT;
        for widget in source.widgets.iter().filter(|w| w.id == template || w.id.starts_with(&format!("{template}/"))) {
            let parts: Vec<&str> = widget.id.split('/').collect();
            let leaf = parts.last().copied().unwrap_or("");
            if parts.iter().any(|p| UNUSED.contains(p)) { continue; }
            if leaf == "LeaderSymbol" && !member.leader { continue; }
            let mut w = widget.clone();
            w.id = w.id.replacen(template, &replacement, 1);
            w.button = w.button.map(|b| b.replacen(template, &replacement, 1));
            if leaf == "TitleLong" { set_text(&mut w, &member.name); }
            if leaf == "Level" { set_text(&mut w, &member.level.to_string()); }
            // Elsewhere, or offline: greyed, its health unknown here.
            if !member.online || member.actor == 0 { grey(&mut w); }
            map_y(&mut w, |y| y + dy);
            layout.widgets.push(w);
        }
    }
    layout
}

/// The gauge a row's health bar piece belongs to (crate::ui::gauge_of).
pub(in crate::ui) fn gauge_of(id: &str) -> Option<u8> {
    let (_, rest) = id.split_once("/GroupMember")?;
    let index: u8 = rest.split('/').next()?.parse().ok()?;
    (rest.contains("/HealthBar/") && (index as usize) < MOST_MEMBERS).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    const GREY: [f32; 4] = [0.55, 0.55, 0.55, 1.0];

    fn layout() -> Layout {
        let (uv, color) = (vec![0.0f32; 8], vec![1.0f32; 16]);
        let widget = |id: &str, text: bool| serde_json::json!({
            "id": id, "type": if text { "TextLabel" } else { "Label" }, "k": 0.5, "rect": [0.0, 418.0, 10.0, 466.0], "hidden": false,
            "art": {"tex": null, "pos": [0.0, 418.0, 10.0, 418.0, 10.0, 466.0, 0.0, 466.0], "uv": uv, "color": color, "idx": [0, 1, 2, 0, 2, 3]},
            "text": if text { serde_json::json!({"text": "Text", "key": "Text", "size": 10.0, "color": [1.0, 1.0, 1.0, 1.0], "align": [0, 1]}) } else { serde_json::Value::Null },
        });
        let ids = [
            ("Canvas", false), ("Canvas/Label_bg", false), ("Canvas/Label_bg/LabelFull/BackroundTop/Groupname", true),
            ("Canvas/Label_bg/LabelFull/GroupLevel/Grouplevel", true), ("Canvas/Label_bg/LabelCompact/Frame", false),
            (ROW, false), ("Canvas/Label_bg/LabelFull/TemplatePlayer/TemplatePlayer/PlayerInfo/TitleLong", true),
            ("Canvas/Label_bg/LabelFull/TemplatePlayer/TemplatePlayer/PlayerInfo/TitleShort", true),
            ("Canvas/Label_bg/LabelFull/TemplatePlayer/TemplatePlayer/PlayerInfo/Level", true),
            ("Canvas/Label_bg/LabelFull/TemplatePlayer/TemplatePlayer/PlayerInfo/TemplateLeader/LeaderSymbol", false),
            ("Canvas/Label_bg/LabelFull/TemplatePlayer/TemplatePlayer/HealthBar/middle", false),
            ("Canvas/Label_bg/LabelFull/TemplatePlayer/TemplatePlayer/ManaBar/middle", false),
            ("Canvas/Label_bg/LabelFull/TemplatePlayer/Levelring/SpiritStoneButtonLayer/ButtonSpiritstoneQuickuse", false),
            (LAST_ROW, false), ("Canvas/Label_bg/LabelFull/TemplatePlayerBottom/TemplatePlayer/PlayerInfo/TitleLong", true),
            ("Canvas/Label_bg/LabelFull/TemplatePlayerBottom/TemplatePlayer/HealthBar/middle", false),
        ];
        serde_json::from_value(serde_json::json!({ "window": WINDOW, "widgets": ids.iter().map(|(id, text)| widget(id, *text)).collect::<Vec<_>>() })).unwrap()
    }

    fn party() -> Party {
        Party { name: "Leader's group".into(), members: vec![
            Member { name: "Leader".into(), level: 45, leader: true, me: true, actor: 1, online: true, map: "a".into(), health: (50.0, 100.0) },
            Member { name: "Member".into(), level: 30, leader: false, me: false, actor: 0, online: false, map: "b".into(), health: (0.0, 0.0) },
        ] }
    }

    #[test]
    fn rows_are_cloned_per_member_with_the_last_template_closing_the_panel() {
        let prepared = prepare(&layout(), &party());
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.as_str()).collect();
        assert!(ids.contains(&"Canvas/Label_bg/LabelFull/GroupMember0/TemplatePlayer/PlayerInfo/TitleLong"));
        assert!(ids.contains(&"Canvas/Label_bg/LabelFull/GroupMember0/TemplatePlayer/PlayerInfo/TemplateLeader/LeaderSymbol"));
        assert!(ids.contains(&"Canvas/Label_bg/LabelFull/GroupMember1/TemplatePlayer/PlayerInfo/TitleLong"));
        assert!(!ids.iter().any(|id| id.contains("GroupMember1/TemplatePlayer/PlayerInfo/TemplateLeader")));
        assert!(!ids.iter().any(|id| id.contains("ManaBar") || id.contains("Spiritstone") || id.contains("TitleShort") || id.contains("GroupLevel") || id.contains("LabelCompact")));
        assert!(!ids.iter().any(|id| id.starts_with(ROW) || id.starts_with(LAST_ROW)));
        let title = |n: usize| prepared.widgets.iter().find(|w| w.id == format!("Canvas/Label_bg/LabelFull/GroupMember{n}/TemplatePlayer/PlayerInfo/TitleLong")).unwrap();
        assert_eq!((title(0).text.as_ref().unwrap().text.as_str(), title(0).rect[1]), ("Leader", 418.0));
        assert_eq!((title(1).text.as_ref().unwrap().text.as_str(), title(1).rect[1], title(1).text.as_ref().unwrap().color), ("Member", 466.0, GREY));
        let name = prepared.widgets.iter().find(|w| w.id.ends_with("/Groupname")).unwrap();
        assert_eq!(name.text.as_ref().unwrap().text, "Leader's group");
        // The backdrop grew by one row.
        let backdrop = prepared.widgets.iter().find(|w| w.id == BACKDROP).unwrap();
        assert_eq!(backdrop.rect[3], 466.0 + ROW_HEIGHT);
        assert!(prepare(&layout(), &Party::default()).widgets.is_empty());
    }

    #[test]
    fn health_gauges_follow_the_member_index() {
        assert_eq!(gauge_of("Canvas/Label_bg/LabelFull/GroupMember1/TemplatePlayer/HealthBar/end/verlauf_text_hell"), Some(1));
        assert_eq!(gauge_of("Canvas/Label_bg/LabelFull/GroupMember1/TemplatePlayer/PlayerInfo/Level"), None);
        assert_eq!(gauge_of("Canvas/Label_bg/LabelFull/GroupMember9/TemplatePlayer/HealthBar/middle"), None);
        let p = party();
        assert_eq!((p.health(0), p.health(1), p.health(2)), (0.5, 0.0, 0.0));
        assert_ne!(p.shape(), Party { members: vec![], ..p.clone() }.shape());
    }
}
