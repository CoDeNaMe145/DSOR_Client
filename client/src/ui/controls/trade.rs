//! Trade between players: a DSOR addition (crate::commands::trade in dsor-proto).
//! The game has no such feature, so its window is composed from the game's own
//! art by tools/compose_trade_ui.py (tradewindow), opened beside the inventory.
//!
//! Flow: the player menu's Trade button asks the server (TradeRequest); the
//! other player sees a handshake in the notification column (crate's social
//! column, type TRADE_INVITE), and accepts it in the invitation dialog
//! (TradeAnswer). Both then fill their offer: an item dragged from the bags
//! onto their side, or right-clicked in the bags, joins it; a click on one of
//! their slots takes it back; the gold entry sets the gold (TradeOffer). Accept
//! (TradeReady) locks the offer in; any change unlocks both; when both have
//! accepted, the server makes the exchange. Closing the window ends the trade
//! (TradeCancel). The server's TradeState is the only truth shown.

use super::*;
use crate::network::chat::Kind;
use dsor_proto::commands::trade::{TradeCancel, TradeHello, TradeOffer, TradePhase, TradeReady};
use dsor_proto::commands::ClientCommand;

pub(in crate::ui) const WINDOW: &str = social::TRADE_WINDOW;
/// The handshake notification a trade invitation shows in the column (a client
/// type, never sent to the server; icon_notifications_invitation).
pub(in crate::ui) const TRADE_INVITE: u8 = 250;
const TRADE_GUID: &[u8] = b"DSOR-TRADE";
/// Over the window's art (crate::ui::WINDOW_ART_DEPTH).
const CONTENT_Z: f32 = 7.5;
pub(in crate::ui) const SLOTS: usize = 12;

#[derive(Resource, Default)]
pub(crate) struct TradeUi {
    /// Items to add to our offer (dropped or right-clicked in the bags).
    pub(super) add: Vec<u32>,
    /// A slot of our offer clicked: take it back.
    pub(super) remove: Option<usize>,
    pub(super) ready: bool,
    /// The gold entry's last value sent.
    gold: u32,
    hello: bool,
    phase: TradePhase,
}

pub(super) fn action(window: &str, w: &Widget) -> Option<Action> {
    if window != WINDOW { return None; }
    if let Some(n) = w.event.strip_prefix("trade_mine:").and_then(|n| n.parse().ok()) { return Some(Action::Social(social::Social::TradeMine(n))); }
    if w.event.starts_with("trade_theirs:") { return Some(Action::Unavailable); }
    if w.event == "trade_ready" { return Some(Action::Social(social::Social::TradeReady)); }
    None
}

/// The trade invitation as a column notification, while one is pending.
pub(super) fn invite_entry(partner: &str) -> dsor_proto::commands::social::NotificationEntry {
    let mut guid = TRADE_GUID.to_vec();
    guid.extend(partner.as_bytes());
    dsor_proto::commands::social::NotificationEntry {
        note_type: TRADE_INVITE, guid, params: serde_json::json!({ "playerName": partner }).to_string(), ..default()
    }
}

/// Follows the server's TradeState: the invitation in the column, the window
/// open with the inventory while trading, and our offer's changes sent.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn sync(
    net: Option<NonSendMut<crate::network::net::Net>>,
    mut trade: ResMut<TradeUi>,
    mut windows: Windows,
    mut chat: ResMut<crate::network::chat::ChatLog>,
) {
    let Some(mut net) = net else { return };
    // DSOR_TEST_TRADE=1: an open trade with Simo's offer (screenshots, offline).
    let test = net.offline && std::env::var_os("DSOR_TEST_TRADE").is_some();
    if test && trade.phase == TradePhase::Closed && net.trade.phase == TradePhase::Closed {
        net.trade = dsor_proto::commands::trade::TradeState {
            phase: TradePhase::Open, partner: "Simo".into(), their_gold: 2500, their_ready: true,
            their_items: vec![
                dsor_proto::commands::trade::TradeItem { item_id: 1, template: "epicmount_death_knight_horse".into(), amount: 1, rarity: 4 },
                dsor_proto::commands::trade::TradeItem { item_id: 2, template: "dailylogin_lootbox_day_03".into(), amount: 3, rarity: 0 },
            ],
            ..default()
        };
    }
    if net.offline && !test { return; }
    // This client shows trades: the server sends TradeState only to such clients.
    if !trade.hello && net.local_actor.is_some() {
        trade.hello = true;
        net.send(&ClientCommand::TradeHello(TradeHello { version: 1 }));
    }
    let state = net.trade.clone();
    // The invitation's handshake, only while invited.
    let invited = (state.phase == TradePhase::Invited).then(|| invite_entry(&state.partner));
    net.notifications.retain(|n| n.note_type != TRADE_INVITE || invited.as_ref().is_some_and(|i| i.guid == n.guid));
    if let Some(entry) = invited {
        if !net.notifications.iter().any(|n| n.guid == entry.guid) { net.notifications.push(entry); }
    }
    if state.phase != trade.phase {
        match state.phase {
            TradePhase::Requested => chat.push(Kind::System, format!("Trade request sent to {}.", state.partner)),
            TradePhase::Done => chat.push(Kind::System, format!("Trade with {} completed.", state.partner)),
            TradePhase::Closed if !state.message.is_empty() => chat.push(Kind::System, format!("Trade: {}.", state.message)),
            _ => {}
        }
        if state.phase == TradePhase::Open {
            trade.gold = 0;
            for (_, w, mut c, mut v, _) in &mut windows {
                if w.0 == WINDOW {
                    for entry in c.entries.values_mut() { *entry = TextEntry::default(); }
                    *v = Visibility::Inherited;
                }
                if w.0 == "inventory" { *v = Visibility::Inherited; }
            }
        }
        if trade.phase == TradePhase::Open {
            for (_, w, _, mut v, _) in &mut windows {
                if w.0 == WINDOW { *v = Visibility::Hidden; }
            }
        }
        trade.phase = state.phase;
    } else if state.phase == TradePhase::Open && !state.message.is_empty() && net.trade_message_new {
        chat.push(Kind::System, format!("Trade: {}.", state.message));
    }
    net.trade_message_new = false;
    if state.phase != TradePhase::Open {
        trade.add.clear();
        trade.remove = None;
        trade.ready = false;
        return;
    }
    // Closed by its X: the trade ends.
    let open = windows.iter().any(|(_, w, _, v, _)| w.0 == WINDOW && *v != Visibility::Hidden);
    if !open {
        net.send(&ClientCommand::TradeCancel(TradeCancel));
        net.trade.phase = TradePhase::Closed;
        trade.phase = TradePhase::Closed;
        return;
    }
    // Our offer: the server's, plus what was added, less what was taken back.
    let mut items = state.my_items.clone();
    let mut changed = false;
    for id in std::mem::take(&mut trade.add) {
        let Some(item) = net.inventory.items.get(&id) else { continue };
        let tradeable = net.inventory.movable(id) && matches!(net.inventory.location(id), Some(crate::gameplay::inventory::Place::Bag(_) | crate::gameplay::inventory::Place::Extra(_)));
        if !tradeable || items.iter().any(|(i, _)| *i == id) || items.len() >= SLOTS { continue; }
        items.push((id, item.third.max(1)));
        changed = true;
    }
    if let Some(n) = trade.remove.take() {
        if n < items.len() { items.remove(n); changed = true; }
    }
    let typed = windows.iter().find(|(_, w, _, _, _)| w.0 == WINDOW)
        .and_then(|(_, _, c, _, _)| c.entries.values().next().map(|e| e.text.chars().filter(char::is_ascii_digit).collect::<String>()))
        .and_then(|t| t.parse::<u32>().ok()).unwrap_or(0);
    if typed != trade.gold {
        trade.gold = typed;
        changed = true;
    }
    if changed {
        net.send(&ClientCommand::TradeOffer(TradeOffer { gold: trade.gold, items }));
    }
    if std::mem::take(&mut trade.ready) {
        net.send(&ClientCommand::TradeReady(TradeReady { ready: !state.my_ready }));
    }
}

#[derive(Component)]
pub(in crate::ui) struct TradeContent;

/// Both offers drawn in their slots, the titles, their gold and the status.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn render(
    mut commands: Commands,
    net: Option<NonSend<crate::network::net::Net>>,
    windows: Query<(Entity, &Window2018, &WindowControls, &Visibility)>,
    old: Query<Entity, With<TradeContent>>,
    data: Res<items::Data>,
    catalogs: Res<Assets<items::Catalog>>,
    layouts: Res<Assets<Layout>>,
    loaded: Res<crate::ui::Loaded>,
    screen: Res<Screen>,
    assets: Res<AssetServer>,
    mut last: Local<Option<(Entity, String, u64, u32)>>,
) {
    let Some(net) = net else { return };
    let window = windows.iter().find(|(_, w, _, v)| w.0 == WINDOW && **v != Visibility::Hidden).map(|(e, _, _, _)| e);
    let catalog = catalogs.get(&data.catalog);
    let key = window.filter(|_| catalog.is_some()).map(|e| (e, format!("{:?}", net.trade), net.inventory.revision, screen.scale.to_bits()));
    if *last == key { return; }
    *last = key;
    for e in &old { commands.entity(e).despawn(); }
    let (Some(root), Some(catalog)) = (window, catalog) else { return };
    let Some(layout) = loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == WINDOW) else { return };
    let widget = |id: &str| layout.widgets.iter().find(|w| w.id == id);
    let group = commands.spawn((TradeContent, Transform::from_xyz(0.0, 0.0, CONTENT_Z), Visibility::Inherited, ChildOf(root))).id();
    let state = &net.trade;
    let text = Color::srgb(0.92, 0.87, 0.76);
    let green = Color::srgb(0.45, 0.95, 0.35);
    let mut write = |id: &str, value: String, color: Color| {
        if let Some(w) = widget(id) {
            let [l, t, r, _] = w.rect;
            social::label(&mut commands, &assets, &screen, group, value, screen.world(w.k, l, t).extend(0.1), 11.0, color, r - l, bevy::text::Justify::Left);
        }
    };
    write("Canvas/MineTitle", if state.my_ready { "Your offer - accepted".into() } else { "Your offer".into() }, if state.my_ready { green } else { text });
    write("Canvas/TheirsTitle", format!("{}{}", state.partner, if state.their_ready { " - accepted" } else { "" }), if state.their_ready { green } else { text });
    write("Canvas/TheirGold", state.their_gold.to_string(), text);
    let status = if state.my_ready && state.their_ready { "Exchanging..." } else if state.my_ready { "Waiting for the other player to accept." }
        else if state.their_ready { "The other player has accepted." } else { "Drag items from your bags onto your side." };
    write("Canvas/Status", status.into(), text);
    for (n, (id, amount)) in state.my_items.iter().enumerate().take(SLOTS) {
        let Some(slot) = widget(&format!("Canvas/Mine/Slot{n}")) else { continue };
        let Some(item) = net.inventory.items.get(id) else { continue };
        social::item_icon(&mut commands, &assets, &screen, group, catalog, &item.template, item.kind, slot.k, slot.rect, Some(*amount));
    }
    for (n, item) in state.their_items.iter().enumerate().take(SLOTS) {
        let Some(slot) = widget(&format!("Canvas/Theirs/Slot{n}")) else { continue };
        social::item_icon(&mut commands, &assets, &screen, group, catalog, &item.template, item.rarity, slot.k, slot.rect, Some(item.amount));
    }
}
