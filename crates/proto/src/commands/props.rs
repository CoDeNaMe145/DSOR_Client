//! release261 prop lifecycle; layouts from the server's protocol/prop_protocol.py.
use super::{
    actors::{read_blob, read_ranged_i8, write_blob, PropEntry},
    Body, DecodeError, ReadExt, WriteExt,
};
use dsor_raknet::{BitReader, BitWriter};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewProp(pub PropEntry);
impl Body for NewProp {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let mut e = PropEntry {
            prop_id: r.u32()?,
            template: r.string()?,
            guid: read_blob(r)?,
            team: r.i32()?,
            state: read_ranged_i8(r, -1, 3, "prop state")?,
            unknown_u32_0: r.u32()?,
            interaction: r.string()?,
            unknown_string_0: r.string()?,
            task_index: r.i32()?,
            quest_type: read_ranged_i8(r, -1, 13, "prop quest type")?,
            ..Default::default()
        };
        e.position = r.vec3()?;
        e.rotation = r.f32()?;
        Ok(Self(e))
    }
    fn encode(&self, w: &mut BitWriter) {
        let e = &self.0;
        w.u32(e.prop_id);
        w.string(&e.template);
        write_blob(w, &e.guid);
        w.i32(e.team);
        w.i8(e.state);
        w.u32(e.unknown_u32_0);
        w.string(&e.interaction);
        w.string(&e.unknown_string_0);
        w.i32(e.task_index);
        w.i8(e.quest_type);
        w.vec3(e.position);
        w.f32(e.rotation);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiscardProp {
    pub prop_id: u32,
}
impl Body for DiscardProp {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { prop_id: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.prop_id);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropUpdate {
    pub prop_id: u32,
    pub variant: i32,
    pub actor_id: u32,
    pub state: i8,
    pub counter: u32,
    pub state_changed: bool,
    pub operation: String,
    pub quest_id: String,
    pub task_index: i32,
    pub quest_indication: i8,
    pub unknown_c0: u32,
    pub unknown_c4: f32,
    pub operation_changed: bool,
    pub unknown_cc: i32,
    pub unknown_d0: u32,
    pub keep_visual_state: bool,
}
impl Body for PropUpdate {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            prop_id: r.u32()?,
            variant: r.i32()?,
            actor_id: r.u32()?,
            state: read_ranged_i8(r, -1, 3, "prop state")?,
            counter: r.u32()?,
            state_changed: r.bit()?,
            operation: r.string()?,
            quest_id: r.string()?,
            task_index: r.i32()?,
            quest_indication: read_ranged_i8(r, -1, 13, "prop quest type")?,
            unknown_c0: r.u32()?,
            unknown_c4: r.f32()?,
            operation_changed: r.bit()?,
            unknown_cc: r.i32()?,
            unknown_d0: r.u32()?,
            keep_visual_state: r.bit()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.prop_id);
        w.i32(self.variant);
        w.u32(self.actor_id);
        w.i8(self.state);
        w.u32(self.counter);
        w.bit(self.state_changed);
        w.string(&self.operation);
        w.string(&self.quest_id);
        w.i32(self.task_index);
        w.i8(self.quest_indication);
        w.u32(self.unknown_c0);
        w.f32(self.unknown_c4);
        w.bit(self.operation_changed);
        w.i32(self.unknown_cc);
        w.u32(self.unknown_d0);
        w.bit(self.keep_visual_state);
    }
}
