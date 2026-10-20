//! Authoritative inventory state. InventoryInfo contains complete allocations,
//! but its item records are incremental (the final mutation packet has none).
use std::collections::HashMap;
use dsor_proto::commands::{inventory::{Inventory, InventoryInfo, ItemInfo}, ClientCommand};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place { Bag(u32), Extra(u32), Bank(u32), ExtraBank(u32), Equipment(i8), Overflow(u32) }

pub const BAG_SLOTS: u32 = 28;
pub const OVERFLOW_PAGE_SLOTS: u32 = 21;

#[derive(Default)]
pub struct InventoryState {
    pub items: HashMap<u32, ItemInfo>,
    pub allocation: InventoryInfo,
    pub revision: u64,
    pub ready: bool,
    pub premium: bool,
}

impl InventoryState {
    pub fn membership(&mut self, tier: u32, active: bool) {
        self.premium = active && matches!(tier, 1 | 2);
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn writable(&self, place: Place) -> bool {
        match place {
            Place::Overflow(_) => false,
            Place::Extra(_) | Place::ExtraBank(_) => self.premium && place.cell() < self.capacity(place),
            _ => place.cell() < self.capacity(place),
        }
    }

    pub fn movable(&self, id: u32) -> bool {
        !self.allocation.overflow.contains(&id) && self.location(id).is_some_and(|p| self.writable(p))
    }
    pub fn apply(&mut self, mut update: InventoryInfo) {
        let has_item_records = !update.items.is_empty();
        for id in &update.removed { self.items.remove(id); }
        // release261 replacement map is NEW -> OLD, not the reverse.
        for (_, old) in &update.map_158 { self.items.remove(old); }
        for item in update.items.drain(..) { self.items.insert(item.id, item); }
        let a = &update;
        let stored = !(a.placements.is_empty() && a.extra.is_empty() && a.bank.is_empty() && a.extra_bank.is_empty()
            && a.overflow.is_empty() && a.invisible.is_empty());
        if update.flag {
            // CONTRACT: a pickup's first InventoryInfo (flag set) holds the new item's
            //   place alone: it joins the storages, it does not replace them; the
            //   equipment map is complete even here.
            // SOURCE: DSOR_Server protocol/world_loot_protocol.py
            //   add_world_drop_to_inventory (delta: field_1e0 = True, one storage entry).
            // FAILURE (2026-10-16): replaced, the bag held the picked item alone, and
            //   the closing packet emptied it ("rien n'apparait dans mon inventaire").
            let old = std::mem::take(&mut self.allocation);
            let moved: std::collections::HashSet<u32> = update.placements.iter().chain(&update.extra).chain(&update.bank)
                .chain(&update.extra_bank).map(|(id, _)| *id).chain(update.slots.iter().map(|(id, _)| *id))
                .chain(update.overflow.iter().chain(&update.invisible).copied()).collect();
            let keep = |v: Vec<(u32, u32)>| v.into_iter().filter(|(id, _)| !moved.contains(id));
            update.placements = keep(old.placements).chain(update.placements).collect();
            update.extra = keep(old.extra).chain(update.extra).collect();
            update.bank = keep(old.bank).chain(update.bank).collect();
            update.extra_bank = keep(old.extra_bank).chain(update.extra_bank).collect();
            update.overflow = old.overflow.into_iter().filter(|id| !moved.contains(id)).chain(update.overflow).collect();
            update.invisible = old.invisible.into_iter().filter(|id| !moved.contains(id)).chain(update.invisible).collect();
        } else if !stored && !has_item_records && !self.allocation.placements.is_empty() {
            // The transaction's closing InventoryInfo carries no item and no storage
            // entry (world_loot_protocol.py: `final`): the storages stand.
            let old = std::mem::take(&mut self.allocation);
            (update.placements, update.extra, update.bank, update.extra_bank, update.overflow, update.invisible) =
                (old.placements, old.extra, old.bank, old.extra_bank, old.overflow, old.invisible);
        }
        // The equipment map is complete even in a partial/closing update. An
        // equipped essence must not also remain in a preserved bag allocation.
        for id in update.removed.iter().chain(update.slots.iter().map(|(id, _)| id)) {
            update.placements.retain(|(i, _)| i != id);
            update.extra.retain(|(i, _)| i != id);
            update.bank.retain(|(i, _)| i != id);
            update.extra_bank.retain(|(i, _)| i != id);
        }
        self.allocation = update;
        self.ready = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn at(&self, place: Place) -> Option<&ItemInfo> {
        let id = match place {
            Place::Equipment(slot) => self.allocation.slots.iter().find(|(_, s)| s.contains(&slot)).map(|(id, _)| *id),
            Place::Overflow(slot) => self.allocation.overflow.get(slot as usize).copied(),
            _ => self.storage(place)?.iter().find(|(_, cell)| *cell == place.cell()).map(|(id, _)| *id),
        }?;
        self.items.get(&id)
    }

    pub fn location(&self, item: u32) -> Option<Place> {
        for base in [Place::Bag(0), Place::Extra(0), Place::Bank(0), Place::ExtraBank(0)] {
            if let Some((_, cell)) = self.storage(base)?.iter().find(|(id, _)| *id == item) { return Some(base.with_cell(*cell)); }
        }
        self.allocation.slots.iter().find(|(id, _)| *id == item)
            .and_then(|(_, s)| s.first()).copied().map(Place::Equipment)
    }

    pub fn capacity(&self, place: Place) -> u32 {
        let s = self.allocation.storage_sizes;
        match place { Place::Bag(_) => s[2], Place::Extra(_) => s[5], Place::Bank(_) => s[3], Place::ExtraBank(_) => s[4],
            Place::Equipment(_) => 38, Place::Overflow(_) => self.allocation.overflow.len() as u32 }
    }

    fn storage(&self, place: Place) -> Option<&[(u32, u32)]> {
        Some(match place { Place::Bag(_) => &self.allocation.placements, Place::Extra(_) => &self.allocation.extra,
            Place::Bank(_) => &self.allocation.bank, Place::ExtraBank(_) => &self.allocation.extra_bank, _ => return None })
    }

    pub fn free_slot(&self, base: Place) -> Option<Place> {
        let used = self.storage(base)?;
        (0..self.capacity(base)).map(|n| base.with_cell(n)).find(|p| self.writable(*p) && !used.iter().any(|(_, n)| *n == p.cell()))
    }

    pub fn split(&self, id: u32, moved: u32, target: Place) -> Option<ClientCommand> {
        let item = self.items.get(&id)?;
        let source = self.location(id)?;
        if !self.movable(id) || !self.writable(target) || source == target || matches!(source, Place::Equipment(_)) || moved == 0 || moved >= item.third
            || target.cell() >= self.capacity(target) || self.at(target).is_some() { return None; }
        let op = match target { Place::Bag(_) => 0x1e, Place::Extra(_) => 0x1f, Place::Bank(_) => 0x20, Place::ExtraBank(_) => 0x21, _ => return None };
        Some(command(id, op, vec![(item.third - moved) as i32, target.cell() as i32]))
    }

    pub fn drop_on(&self, id: u32, target: Place, max_stack: u32) -> Option<ClientCommand> {
        let source = self.items.get(&id)?;
        if !self.ready || !self.movable(id) || !self.writable(target) { return None; }
        if let Some(other) = self.at(target) {
            if other.id == id { return None; }
            if max_stack > 1 && stack_compatible(source, other) && !matches!(target, Place::Equipment(_) | Place::Overflow(_)) {
                if other.third >= max_stack { return None; }
                // Merge operation selects the destination storage, argument is target item ID.
                let op = match target { Place::Bag(_) => 0x0d, Place::Extra(_) => 0x0e, Place::Bank(_) => 0x0f, Place::ExtraBank(_) => 0x10, _ => return None };
                return Some(command(id, op, vec![other.id as i32]));
            }
            return Some(command(id, 0x0b, vec![other.id as i32]));
        }
        let op = match target { Place::Equipment(_) => 6, Place::Bag(_) => 7, Place::Extra(_) => 8, Place::Bank(_) => 9, Place::ExtraBank(_) => 10, _ => return None };
        Some(command(id, op, vec![target.cell() as i32]))
    }
}

impl Place {
    pub fn cell(self) -> u32 { match self { Self::Bag(n) | Self::Extra(n) | Self::Bank(n) | Self::ExtraBank(n) | Self::Overflow(n) => n, Self::Equipment(n) => n as u32 } }
    pub fn with_cell(self, n: u32) -> Self { match self { Self::Bag(_) => Self::Bag(n), Self::Extra(_) => Self::Extra(n), Self::Bank(_) => Self::Bank(n), Self::ExtraBank(_) => Self::ExtraBank(n), Self::Overflow(_) => Self::Overflow(n), Self::Equipment(_) => Self::Equipment(n as i8) } }
}

pub fn command(item: u32, operation: i8, arguments: Vec<i32>) -> ClientCommand {
    ClientCommand::Inventory(Inventory { item, operation, arguments, first: 0, last: 0 })
}

pub fn stack_compatible(a: &ItemInfo, b: &ItemInfo) -> bool {
    a.template == b.template && a.kind == b.kind && a.durability == b.durability && a.tier == b.tier
        && a.statistics == b.statistics && a.names == b.names && a.flag_0 == b.flag_0
        && a.fourth_name == b.fourth_name && a.appearance == b.appearance && a.twentieth == b.twentieth
        && a.twenty_eighth == b.twenty_eighth && a.tenth == b.tenth
        && a.eleventh == b.eleventh && a.twelfth == b.twelfth && a.named == b.named
        && a.second_name == b.second_name && a.fifty_second == b.fifty_second
        && a.flag_1 == b.flag_1 && a.flag_2 == b.flag_2
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: u32, amount: u32) -> ItemInfo { ItemInfo { id, third: amount, template: "potion".into(), ..Default::default() } }
    fn state() -> InventoryState {
        let mut s = InventoryState::default();
        s.apply(InventoryInfo { items: vec![item(1, 10), item(2, 6)], placements: vec![(1, 0), (2, 1)], storage_sizes: [0, 0, 21, 0, 0, 0], ..Default::default() }); s
    }
    #[test]
    fn delta_rekeys_both_stacks_and_empty_final_keeps_records() {
        let mut s = state();
        let delta = InventoryInfo { items: vec![item(3, 7), item(4, 3)], removed: vec![1], map_158: vec![(3, 1)], placements: vec![(3, 0), (2, 1), (4, 2)], storage_sizes: [0, 0, 21, 0, 0, 0], ..Default::default() };
        s.apply(delta.clone());
        s.apply(InventoryInfo { items: vec![], removed: vec![], map_158: vec![], ..delta });
        assert!(!s.items.contains_key(&1));
        assert_eq!(s.at(Place::Bag(0)).unwrap().third, 7);
        assert_eq!(s.at(Place::Bag(2)).unwrap().id, 4);
        assert_eq!(s.at(Place::Bag(1)).unwrap().third, 6);
    }
    #[test]
    fn a_picked_item_joins_the_bag_and_the_closing_packet_keeps_it() {
        let mut s = state();
        let delta = InventoryInfo { items: vec![item(9, 1)], placements: vec![(9, 5)], storage_sizes: [0, 0, 21, 0, 0, 0], flag: true, ..Default::default() };
        s.apply(delta);
        s.apply(InventoryInfo { storage_sizes: [0, 0, 21, 0, 0, 0], ..Default::default() });
        assert_eq!(s.at(Place::Bag(5)).unwrap().id, 9);
        assert_eq!(s.at(Place::Bag(0)).unwrap().id, 1);
        assert_eq!(s.at(Place::Bag(1)).unwrap().id, 2);
    }
    #[test]
    fn split_sends_amount_remaining_and_refuses_occupied_or_locked_slots() {
        let s = state();
        assert_eq!(s.split(1, 3, Place::Bag(2)), Some(command(1, 0x1e, vec![7, 2])));
        for (n, target) in [(0, 2), (10, 2), (2, 0), (2, 1), (2, 21)] { assert!(s.split(1, n, Place::Bag(target)).is_none()); }
    }
    #[test]
    fn drop_distinguishes_merge_swap_move_and_full_stack() {
        let mut s = state();
        assert_eq!(s.drop_on(1, Place::Bag(1), 99), Some(command(1, 0x0d, vec![2])));
        assert_eq!(s.drop_on(1, Place::Bag(2), 99), Some(command(1, 7, vec![2])));
        assert!(s.drop_on(1, Place::Bag(1), 6).is_none());
        s.items.get_mut(&2).unwrap().template = "different".into();
        assert_eq!(s.drop_on(1, Place::Bag(1), 99), Some(command(1, 11, vec![2])));
    }

    #[test]
    fn equipping_the_last_carried_stack_never_keeps_a_stale_bag_copy() {
        for (has_records, partial) in [(true,false),(false,false),(true,true)] {
            let mut s = InventoryState::default();
            s.apply(InventoryInfo { items:vec![item(1,10)], placements:vec![(1,0)],
                storage_sizes:[0,0,28,0,0,0], ..Default::default() });
            s.apply(InventoryInfo { items:if has_records {vec![item(1,10)]} else {vec![]},
                slots:vec![(1,vec![35])], storage_sizes:[0,0,28,0,0,0], flag:partial, ..Default::default() });
            assert_eq!(s.location(1),Some(Place::Equipment(35)));
            assert!(s.at(Place::Bag(0)).is_none());
            assert_eq!(s.drop_on(1,Place::Bag(0),999),Some(command(1,7,vec![0])));
        }
    }

    #[test]
    fn premium_requires_membership_and_overflow_never_accepts_actions() {
        let mut s = state();
        s.allocation.storage_sizes[5] = BAG_SLOTS;
        assert!(s.drop_on(1, Place::Extra(27), 99).is_none());
        s.membership(1, true);
        assert_eq!(s.drop_on(1, Place::Extra(27), 99), Some(command(1, 8, vec![27])));
        assert!(s.drop_on(1, Place::Extra(28), 99).is_none());
        s.membership(1, false);
        s.apply(InventoryInfo { items: vec![item(1, 10)], removed: vec![1], overflow: vec![1],
            placements: vec![(2, 0)], storage_sizes: [0, 0, 28, 0, 0, 0], ..Default::default() });
        assert_eq!(s.at(Place::Overflow(0)).unwrap().third, 10);
        assert!(s.drop_on(1, Place::Bag(1), 99).is_none());
        assert!(s.drop_on(2, Place::Overflow(0), 99).is_none());
        assert!(s.split(1, 3, Place::Bag(1)).is_none());
        assert!(s.split(2, 3, Place::Overflow(0)).is_none());
        // Server recovery updates the allocation; the item then becomes usable.
        s.apply(InventoryInfo { placements: vec![(2, 0), (1, 1)], storage_sizes: [0, 0, 28, 0, 0, 0], ..Default::default() });
        assert!(s.at(Place::Overflow(0)).is_none());
        assert!(s.movable(1));
        assert!(s.split(1, 3, Place::Bag(2)).is_some());
    }
}
