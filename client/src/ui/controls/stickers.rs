//! Collector's Bag stickers dragged onto the quick bar: a mount onto the mount
//! slot (O), a companion onto its slot (H) or a skill slot. The slot becomes a
//! sticker shortcut (QuickSlot kind 2, the owned item's resource id), which
//! hotbar::input uses with UseStickerBookItem.
//! EVIDENCE: DSOR_Server services/customization.py writes Shadow Souls as
//!   QuickSlotInfo(2, resource_id) in slot 9; inventory_interactions.py
//!   `_handle_quick_slots` keeps slots 8.. on every bar.
//! FAILURE (2026-10-24): stickers could not be dragged out of the bag at all
//!   ("in the original game you can drag the icons to put them in the O slot").
use super::*;
use dsor_proto::commands::{inventory::{QuickSlot as Slot, QuickSlots}, ClientCommand};

const BAR_SIZE: usize = 17;
/// Skill slots (0..8) and the collection slots (8..13: mount, companion...);
/// never the four consumable slots.
const STICKER_SLOTS: usize = 13;
/// The four consumable slots (U, G, R, T): potions, elixirs, the Universal Map.
pub(super) const CONSUMABLE_SLOTS: std::ops::Range<usize> = 13..17;

pub(super) struct StickerDrag {
    id: String,
    origin: Vec2,
    active: bool,
}

/// A press on an owned sticker that moves 4 px picks it up; released over a quick
/// slot it lands there. Returns true when the release belongs to a drag, so it
/// does not also preview the sticker.
pub(super) fn drag(pointer: &mut Pointer, windows: &Windows, views: &systems::Views, mouse: &ButtonInput<MouseButton>,
    hit: Option<(Entity, usize)>, point: Option<Vec2>, allowed: bool) -> bool {
    let action = |(root, button): (Entity, usize)| windows.get(root).ok().map(|(_, name, c, _, _)| (name.0.clone(), c.buttons[button].action.clone()));
    if mouse.just_pressed(MouseButton::Left) {
        pointer.sticker_drag = None;
        if let (true, Some(origin), Some((_, Action::System(systems::Request::Pick(id, _))))) = (allowed, point, hit.and_then(action)) {
            if views.album.sticker(&id).is_some_and(|s| s.owned) {
                pointer.sticker_drag = Some(StickerDrag { id, origin, active: false });
            }
        }
    }
    if let (Some(d), Some(p)) = (&mut pointer.sticker_drag, point) {
        if p.distance(d.origin) >= 4.0 { d.active = true; }
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(d) = pointer.sticker_drag.take().filter(|d| d.active) {
            if let Some(("bottombar", Action::Skill(slot))) = hit.and_then(action).as_ref().map(|(n, a)| (n.as_str(), a.clone())) {
                if slot < STICKER_SLOTS { pointer.sticker_drop = Some((2, d.id, slot)); }
            }
            pointer.down = None;
            return true;
        }
    }
    if !mouse.pressed(MouseButton::Left) { pointer.sticker_drag = None; }
    false
}

/// The dragged sticker's icon, for the cursor overlay.
pub(super) fn dragged<'a>(pointer: &Pointer, views: &'a systems::Views) -> Option<&'a str> {
    pointer.sticker_drag.as_ref().filter(|d| d.active).and_then(|d| views.album.sticker(&d.id)).map(|s| s.icon.image.as_str()).filter(|i| !i.is_empty())
}

/// The active bar with `slot` holding the shortcut (`kind` 2 a sticker, on a
/// skill or collection slot; 1 an item, on a consumable slot); the same one
/// leaves any other slot of the bar. None when nothing changes.
fn edited_bar(slots: &[Slot], kind: i32, slot: usize, key: &str) -> Option<Vec<Slot>> {
    let fits = if kind == 2 { slot < STICKER_SLOTS } else { (CONSUMABLE_SLOTS).contains(&slot) };
    if slots.len() != BAR_SIZE || !fits { return None; }
    let mut edited = slots.to_vec();
    for s in &mut edited {
        if s.kind == kind && s.id.as_deref() == Some(key) { *s = Slot { kind: Slot::EMPTY, id: None }; }
    }
    edited[slot] = Slot { kind, id: Some(key.to_owned()) };
    (edited != slots).then_some(edited)
}

pub(in crate::ui) fn apply_drop(mut pointer: ResMut<Pointer>, views: Res<systems::Views>, hotbar: Res<hotbar::Hotbar>,
    net: Option<NonSendMut<crate::network::net::Net>>) {
    let Some((kind, id, slot)) = pointer.sticker_drop.take() else { return };
    let Some(mut net) = net else { return };
    let key = if kind == 2 {
        let Some(sticker) = views.album.sticker(&id).filter(|s| s.owned) else { return };
        sticker.server_key().to_owned()
    } else { id.clone() };
    let bar = hotbar.active;
    let Some(slots) = net.quickbars.bars.get(bar).and_then(|b| edited_bar(b, kind, slot, &key)) else { return };
    // The collection slots are the same on every bar (the server copies them).
    if slot >= 8 {
        for b in net.quickbars.bars.iter_mut().filter(|b| b.len() == BAR_SIZE) { b[slot] = slots[slot].clone(); }
    }
    net.bar = slots.iter().map(|s| if s.kind == 0 { s.id.clone() } else { None }).collect();
    net.quickbars.bars[bar] = slots.clone();
    info!("shortcut {id} ({key}, kind {kind}) on quick slot {slot}");
    net.send(&ClientCommand::QuickSlots(QuickSlots { slots, bar: bar as u32 }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sticker_takes_its_slot_once_and_never_a_potion_slot() {
        let empty = Slot { kind: Slot::EMPTY, id: None };
        let mut bar = vec![empty.clone(); BAR_SIZE];
        bar[0] = Slot { kind: 0, id: Some("angrystrike".into()) };
        bar[3] = Slot { kind: 2, id: Some("mount_x".into()) };
        let edited = edited_bar(&bar, 2, 8, "mount_x").unwrap();
        assert_eq!(edited[8], Slot { kind: 2, id: Some("mount_x".into()) });
        assert_eq!(edited[3], empty, "moved, not copied");
        assert_eq!(edited[0], bar[0]);
        assert!(edited_bar(&edited, 2, 8, "mount_x").is_none(), "already there");
        assert!(edited_bar(&bar, 2, 13, "mount_x").is_none(), "consumable slot");
        // An item only on the consumable slots.
        assert_eq!(edited_bar(&bar, 1, 14, "misc_travelmap").unwrap()[14], Slot { kind: 1, id: Some("misc_travelmap".into()) });
        assert!(edited_bar(&bar, 1, 2, "misc_travelmap").is_none(), "skill slot");
    }
}
