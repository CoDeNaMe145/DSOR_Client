//! The release261 game protocol on top of RakNet (dso-server-production is the
//! spec: tests/commands_release261.rs checks every layout against its encoders).
//!
//! - `identity`: the client's 0x8A hello, account and session.
//! - `messages`: the envelopes (0x82, 0x84, 0x85, 0x86, 0x88, 0x8B, 0x8D, 0x1B).
//! - `commands`: every game command, decoded and encoded field by field.
//! - `session`: login -> character service -> map, across the server's hand-offs.
//! - `chat`: the built-in chat service beside it (UDP 2191).

pub mod chat;
pub mod commands;
pub mod identity;
pub mod messages;
pub mod session;

pub use dsor_raknet::{BitReader, BitWriter};
