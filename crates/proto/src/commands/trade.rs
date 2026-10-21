//! Trade between players: a DSOR addition, absent from every game client.
//! The game has no such feature (no command, window or text in the 2018 or the
//! release261 client), so these ids sit far above the game's own (release261_192
//! registers up to 381): 0x0F00.. from the client, 0x0F10 from the server. Only
//! a client that announced itself with TradeHello receives TradeState.
//! CONTRACT: DSOR_Server protocol/trade_protocol.py writes and reads the same
//!   layouts.

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{counted, Body, ReadExt, WriteExt, MOST};
use super::DecodeError;

pub const TRADE_HELLO: u16 = 0x0F00;
pub const TRADE_REQUEST: u16 = 0x0F01;
pub const TRADE_ANSWER: u16 = 0x0F02;
pub const TRADE_OFFER: u16 = 0x0F03;
pub const TRADE_READY: u16 = 0x0F04;
pub const TRADE_CANCEL: u16 = 0x0F05;
pub const TRADE_STATE: u16 = 0x0F10;

/// Client -> server, once in the game: this client shows trades (u8 version 1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeHello {
    pub version: u8,
}

impl Body for TradeHello {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> { Ok(Self { version: r.u8()? }) }
    fn encode(&self, w: &mut BitWriter) { w.u8(self.version); }
}

/// Client -> server: ask this player (by name) to trade.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeRequest {
    pub target: String,
}

impl Body for TradeRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> { Ok(Self { target: r.string()? }) }
    fn encode(&self, w: &mut BitWriter) { w.string(&self.target); }
}

/// Client -> server: accept or decline the trade we were asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeAnswer {
    pub accept: bool,
}

impl Body for TradeAnswer {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> { Ok(Self { accept: r.u8()? != 0 }) }
    fn encode(&self, w: &mut BitWriter) { w.u8(u8::from(self.accept)); }
}

/// Client -> server: our whole offer, replacing the last one: gold, then
/// (inventory item id, amount) pairs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeOffer {
    pub gold: u32,
    pub items: Vec<(u32, u32)>,
}

impl Body for TradeOffer {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let gold = r.u32()?;
        Ok(Self { gold, items: counted(r, 64, "trade offer size", |r| Ok((r.u32()?, r.u32()?)))? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.gold);
        w.u32(self.items.len() as u32);
        for (id, amount) in &self.items {
            w.u32(*id);
            w.u32(*amount);
        }
    }
}

/// Client -> server: we accept the offers as they stand (or take it back).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeReady {
    pub ready: bool,
}

impl Body for TradeReady {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> { Ok(Self { ready: r.u8()? != 0 }) }
    fn encode(&self, w: &mut BitWriter) { w.u8(u8::from(self.ready)); }
}

/// Client -> server: end the trade (or withdraw our request). An empty body
/// (one u8 0, so the message has one).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeCancel;

impl Body for TradeCancel {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> { r.u8()?; Ok(Self) }
    fn encode(&self, w: &mut BitWriter) { w.u8(0); }
}

/// One item of the partner's offer: enough to draw it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeItem {
    pub item_id: u32,
    pub template: String,
    pub amount: u32,
    pub rarity: u8,
}

/// Where a trade stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TradePhase {
    /// No trade (with `message` saying why it ended, if it did).
    #[default]
    Closed,
    /// The partner asks us to trade.
    Invited,
    /// We asked the partner; waiting for the answer.
    Requested,
    /// Both offers are open.
    Open,
    /// The exchange was made.
    Done,
}

impl TradePhase {
    fn from_u8(v: u8) -> Self {
        match v { 1 => Self::Invited, 2 => Self::Requested, 3 => Self::Open, 4 => Self::Done, _ => Self::Closed }
    }
    fn to_u8(self) -> u8 {
        match self { Self::Closed => 0, Self::Invited => 1, Self::Requested => 2, Self::Open => 3, Self::Done => 4 }
    }
}

/// Server -> client (0x84, no actor): the whole trade as the server holds it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeState {
    pub phase: TradePhase,
    pub partner: String,
    /// Our offer as the server accepted it: gold, (item id, amount).
    pub my_gold: u32,
    pub my_items: Vec<(u32, u32)>,
    pub my_ready: bool,
    pub their_gold: u32,
    pub their_items: Vec<TradeItem>,
    pub their_ready: bool,
    /// Why it ended or what went wrong ("declined", "cancelled", "bags full"...).
    pub message: String,
}

impl Body for TradeState {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            phase: TradePhase::from_u8(r.u8()?),
            partner: r.string()?,
            my_gold: r.u32()?,
            my_items: counted(r, MOST, "trade items", |r| Ok((r.u32()?, r.u32()?)))?,
            my_ready: r.u8()? != 0,
            their_gold: r.u32()?,
            their_items: counted(r, MOST, "trade items", |r| Ok(TradeItem { item_id: r.u32()?, template: r.string()?, amount: r.u32()?, rarity: r.u8()? }))?,
            their_ready: r.u8()? != 0,
            message: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.phase.to_u8());
        w.string(&self.partner);
        w.u32(self.my_gold);
        w.u32(self.my_items.len() as u32);
        for (id, amount) in &self.my_items {
            w.u32(*id);
            w.u32(*amount);
        }
        w.u8(u8::from(self.my_ready));
        w.u32(self.their_gold);
        w.u32(self.their_items.len() as u32);
        for i in &self.their_items {
            w.u32(i.item_id);
            w.string(&i.template);
            w.u32(i.amount);
            w.u8(i.rarity);
        }
        w.u8(u8::from(self.their_ready));
        w.string(&self.message);
    }
}
