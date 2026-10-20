use super::*;

#[derive(Default)]
pub(super) struct EquipmentSounds {
    actor: Option<u32>,
    slots: Option<BTreeMap<i8, u32>>,
    revision: Option<u64>,
}

fn equipment_cue(old: &BTreeMap<i8, u32>, new: &BTreeMap<i8, u32>) -> Option<&'static str> {
    // Prefer the newly equipped item when a swap removes several occupied slots.
    let slot = new.iter().find(|(slot, id)| old.get(slot) != Some(id)).map(|(s, _)| *s)
        .or_else(|| old.keys().find(|s| !new.contains_key(s)).copied())?;
    Some(match slot {
        5 | 6 | 12 => "ui/inventory/ui_inventory_equip_weapon",
        8..=10 | 13..=25 => "ui/inventory/ui_inventory_equip_jewelry",
        0..=4 | 7 | 11 | 26 | 27 => "ui/inventory/ui_inventory_equip_armor",
        _ => "ui/inventory/ui_inventory_move_item",
    })
}

pub(super) fn emit(
    net: Option<NonSend<Net>>, current: Option<Res<CurrentMap>>,
    loading: Option<Res<crate::screens::loading::LoadingScreen>>,
    mut state: Local<EquipmentSounds>, mut requests: ResMut<AudioRequests>,
) {
    let Some(net) = net.filter(|n| n.inventory.ready) else { *state = default(); return };
    if state.actor != net.local_actor { *state = EquipmentSounds { actor: net.local_actor, ..default() }; }
    if state.revision == Some(net.inventory.revision) { return; }
    let slots = net.inventory.allocation.slots.iter().flat_map(|(id, slots)| slots.iter().map(|s| (*s, *id))).collect();
    if current.is_some_and(|m| m.spawned) && !loading.is_some_and(|s| s.active()) {
        if let Some(cue) = state.slots.as_ref().and_then(|old| equipment_cue(old, &slots)) { requests.ui(cue); }
    }
    state.slots = Some(slots);
    state.revision = Some(net.inventory.revision);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equipment_sounds_follow_confirmed_allocations_once_per_swap() {
        let old = BTreeMap::from([(2, 10), (5, 20), (6, 21)]);
        assert_eq!(equipment_cue(&old, &old), None, "closing packets, dye updates and bag moves stay silent");
        let new = BTreeMap::from([(2, 10), (5, 22), (6, 22)]);
        assert_eq!(equipment_cue(&old, &new), Some("ui/inventory/ui_inventory_equip_weapon"));
        assert_eq!(equipment_cue(&new, &BTreeMap::from([(5, 22), (6, 22)])), Some("ui/inventory/ui_inventory_equip_armor"));
        assert_eq!(equipment_cue(&old, &BTreeMap::from([(8, 40)])), Some("ui/inventory/ui_inventory_equip_jewelry"));
    }
}
