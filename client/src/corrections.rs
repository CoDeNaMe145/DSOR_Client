//! Reconcile corrections without rewinding ordinary delayed movement echoes.
use std::collections::{HashMap, VecDeque};
use bevy::prelude::*;
use dsor_proto::commands::movement::Move;

#[derive(Default)]
pub struct Corrections {
    pub pending: HashMap<u32, (Vec3, f32)>,
    sent: VecDeque<Move>,
}
impl Corrections {
    pub fn sent(&mut self, movement: Move) {
        self.sent.push_back(movement);
        while self.sent.len() > 128 { self.sent.pop_front(); }
    }
    pub fn force(&mut self, actor: u32, position: Vec3, heading: f32, local: bool) {
        if !position.is_finite() || !heading.is_finite() { return; }
        if local { self.sent.clear(); }
        self.pending.insert(actor, (position, heading));
    }
    pub fn corrected_echo(&mut self, movement: &Move) -> bool {
        let Some(index) = self.sent.iter().position(|m| m.start_tick == movement.start_tick) else { return false };
        let original = self.sent[index];
        self.sent.drain(..=index);
        let changed = (original.x, original.elevation, original.y, original.facing) !=
            (movement.x, movement.elevation, movement.y, movement.facing);
        if changed { self.sent.clear(); }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_echo_is_not_a_correction_and_force_invalidates_old_history() {
        let mut c = Corrections::default();
        let m = Move { start_tick: 10, x: 128, ..Default::default() };
        c.sent(m); c.sent(Move { start_tick: 11, x: 256, ..m });
        assert!(!c.corrected_echo(&m));
        assert!(c.corrected_echo(&Move { start_tick: 11, x: 129, ..m }));
        c.sent(m);c.force(1, Vec3::new(14.0, 0.0, 45.0), 1.0, true);
        assert!(!c.corrected_echo(&Move { x: 99, ..m }));
        assert_eq!(c.pending[&1].0, Vec3::new(14.0, 0.0, 45.0));
    }
}
