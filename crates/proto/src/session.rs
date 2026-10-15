//! From the launcher's credentials to standing on a map: login -> character service ->
//! login again with the chosen character -> map server, following the server's
//! hand-offs.
//!
//! SOURCE: dso-server-production (release261), as traced in services/login_game.py,
//!   services/lifecycle.py, services/dispatch.py, services/map_transition.py:
//!   2190 login:      0x82 Login, we 0x8A, SwitchMap(<host>:2192, true), 0x88, we 0x15
//!   2192 characters: 0x82 DrasaCharacterService, we 0x8A, 0x86 "a0000_char", 0x88,
//!                    we 0x8D, 0x1B clock (ms), CharacterSelection op 1 (sent TWICE:
//!                    once at once, once after its 80 ms fallback), we op 3, op 5,
//!                    SwitchMap(<host>:2190, false), we 0x15
//!   2190 login:      we 0x8A (long, with the character), SwitchMap(<host>:2192, false)
//!   2192 map:        0x82 DrasaOnlineMapServer, we 0x8A, 0x86 map, 0x88, we 0x8D,
//!                    the world arrives (NewPlayer 1 s later without StillAlive)
//!   a map exit:      SwitchMap(<host>:2190, false) in a 0x85 batch, then as above.
//! The account must have logged in over HTTP first (crate::account), from the same
//! address: the server ignores a connection request from any other.
//!
//! No I/O: the caller owns the transport. When the session moves to another service it
//! emits `SessionEvent::Connect(addr)`; the caller then routes datagrams to and from
//! that address only.

use std::collections::VecDeque;
use std::net::SocketAddrV4;
#[cfg(not(target_arch = "wasm32"))]
use std::net::ToSocketAddrs;

use dsor_raknet::{Connection, DisconnectReason, Event, Reliability};

use crate::identity::{hello, Credentials};
use crate::messages::{self, classify, Envelope, RosterEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Login,
    Characters,
    LoginWithCharacter,
    Map,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{ClientCommand, player::{CharacterSelection, SelectionCharacter, SelectionEquipment}};

    fn packet(c: CharacterSelection) -> (Vec<u8>, usize) {
        let (mut bytes, bits) = ClientCommand::CharacterSelection(c).encode();
        bytes[0] = messages::COMMAND;
        (bytes, bits)
    }

    #[test]
    fn character_service_exposes_equipment_and_refreshes_without_committing_a_character() {
        let mut session = Session::new("127.0.0.1:2190".parse().unwrap(), Credentials { account: 1, session: [0; 16] }, 1, 0);
        session.events.clear(); session.stage = Stage::Characters;
        let roster = CharacterSelection { operation: 1, character: 10, free: 5,
            characters: vec![SelectionCharacter { character: 10, name: "Hero".into(), ..Default::default() }], ..Default::default() };
        let (bytes, bits) = packet(roster.clone());
        session.on_message(&bytes, bits, 1); session.on_message(&bytes, bits, 2);
        assert_eq!(session.events.iter().filter(|e| matches!(e, SessionEvent::Roster(_))).count(), 1);
        assert_eq!(session.events.iter().filter(|e| matches!(e, SessionEvent::CharacterSelection(_))).count(), 2);
        session.events.clear();
        session.character_operation(8, 10, 3);
        assert_eq!(session.character(), None, "preview must retain the short service hello");
        let equipment = CharacterSelection { operation: 9, character: 10, equipment: vec![SelectionEquipment {
            template: "sword".into(), slot: "transmog".into(), colour: [0.1, 0.2, 0.3, 1.0], ..Default::default()
        }], ..Default::default() };
        let (bytes, bits) = packet(equipment.clone()); session.on_message(&bytes, bits, 4);
        assert_eq!(session.poll_event(), Some(SessionEvent::CharacterSelection(equipment)));
        let (bytes, bits) = packet(CharacterSelection { characters: vec![], ..roster });
        session.on_message(&bytes, bits, 5);
        assert!(matches!(session.poll_event(), Some(SessionEvent::CharacterSelection(c)) if c.characters.is_empty()));
        session.select_character(10, 6); assert_eq!(session.character(), Some(10));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    /// Send `farewell` on the current transport (if any), then open one to `target`;
    /// everything after goes there.
    Connect { target: SocketAddrV4, farewell: Vec<Vec<u8>> },
    Service(String),
    /// The character service's roster: answer with `select_character`.
    Roster(Vec<RosterEntry>),
    /// Full roster and preview replies. Browsing (op 8/9) does not select a
    /// gameplay character; only select_character (op 3) starts that handoff.
    CharacterSelection(crate::commands::player::CharacterSelection),
    /// The account has no character: the server opened the character editor
    /// (CharacterGeneration op 2); answer with `create_character`.
    CharacterEditor,
    /// The server refused the new character (CharacterGeneration op 4): its deny
    /// reason (1 name taken, 2 empty, 3 invalid character, 7 already made, 8 other).
    CreationRefused(u8),
    MapAssigned { name: String, rule_set: u32 },
    /// Game commands on the map (0x84 / 0x85), for `commands::decode_message`.
    Commands { data: Vec<u8>, bits: usize },
    Clock(u32),
    Disconnected(DisconnectReason),
}

pub struct Session {
    credentials: Credentials,
    guid: u64,
    login: SocketAddrV4,
    stage: Stage,
    conn: Connection,
    character: Option<u32>,
    first_login: bool,
    readies: u8,
    /// The roster was handed out on this character service already (the server
    /// sends it twice; a second pick would re-arm the hand-off).
    rostered: bool,
    events: VecDeque<SessionEvent>,
    started: u64,
    handing_off: bool,
}

/// "host:port" to an IPv4 socket address; a bare host keeps `default_port`.
pub fn resolve(target: &str, default_port: u16) -> Option<SocketAddrV4> {
    let with_port = if target.contains(':') { target.to_string() } else { format!("{target}:{default_port}") };
    if let Ok(a) = with_port.parse::<SocketAddrV4>() {
        return Some(a);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        with_port.to_socket_addrs().ok()?.find_map(|a| match a {
            std::net::SocketAddr::V4(v4) => Some(v4),
            _ => None,
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

impl Session {
    pub fn new(login: SocketAddrV4, credentials: Credentials, guid: u64, now: u64) -> Self {
        let mut events = VecDeque::new();
        events.push_back(SessionEvent::Connect { target: login, farewell: Vec::new() });
        Self {
            credentials,
            guid,
            login,
            stage: Stage::Login,
            conn: Connection::new(login, guid, now),
            character: None,
            first_login: true,
            readies: 0,
            rostered: false,
            events,
            started: now,
            handing_off: false,
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn character(&self) -> Option<u32> {
        self.character
    }

    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn poll_event(&mut self) -> Option<SessionEvent> {
        self.events.pop_front()
    }

    /// Datagrams for the CURRENT target. Poll events first: a `Connect` changes it.
    pub fn drain_outgoing(&mut self, into: &mut Vec<Vec<u8>>) {
        self.conn.drain_outgoing(into);
    }

    /// The game clock (25 ticks per second) the client keeps since it started.
    pub fn local_tick(&self, now: u64) -> u32 {
        ((now - self.started) / 40) as u32
    }

    /// Pick the character to play (from the roster).
    pub fn select_character(&mut self, character: u32, now: u64) {
        self.character = Some(character);
        let (bytes, bits) = messages::select_character(character);
        self.conn.send(&bytes, bits, Reliability::ReliableOrdered, 0);
        self.conn.flush(now);
    }

    pub fn character_operation(&mut self, operation: u8, character: u32, now: u64) {
        let c = crate::commands::player::CharacterSelection { operation, character, ..Default::default() };
        let (bytes, bits) = crate::commands::ClientCommand::CharacterSelection(c).encode();
        self.send_command(&bytes, bits);
        self.conn.flush(now);
    }

    pub fn open_character_editor(&mut self, now: u64) {
        let c = crate::commands::player::CharacterGeneration { operation: 1, ..Default::default() };
        let (bytes, bits) = crate::commands::ClientCommand::CharacterGeneration(c).encode();
        self.send_command(&bytes, bits);
        self.conn.flush(now);
    }

    /// Create a character in the editor the server opened (CharacterGeneration op 3).
    /// On success the server answers op 5 with its id and hands off to the world.
    /// SOURCE: dso-server-production services/character_selection.py:485-691,
    ///   protocol/character_generation.py:87-124 (our layout, client tail 16 bits).
    pub fn create_character(&mut self, name: &str, class: u8, gender: u8, now: u64) {
        let c = crate::commands::player::CharacterGeneration {
            operation: 3,
            player_class: class as i8,
            gender: gender as i8,
            name: name.to_string(),
            ..Default::default()
        };
        let (bytes, bits) = crate::commands::ClientCommand::CharacterGeneration(c).encode();
        self.conn.send(&bytes, bits, Reliability::ReliableOrdered, 0);
        self.conn.flush(now);
    }

    /// A game command (0x8B ...) to the current map server.
    pub fn send_command(&mut self, payload: &[u8], bits: usize) {
        self.conn.send(payload, bits, Reliability::ReliableOrdered, 0);
    }

    /// A game command with its own reliability: the 2018 client sends MoveCommand
    /// unreliable sequenced (captured: reliability 1, one per tick).
    pub fn send_command_with(&mut self, payload: &[u8], bits: usize, reliability: Reliability) {
        self.conn.send(payload, bits, reliability, 0);
    }

    /// Say goodbye to the current server (DISCONNECTION_NOTIFICATION).
    pub fn disconnect(&mut self, now: u64) {
        self.handing_off = true;
        self.conn.disconnect(now);
    }

    pub fn update(&mut self, now: u64) {
        self.conn.update(now);
        self.pump(now);
    }

    pub fn receive(&mut self, datagram: &[u8], now: u64) {
        self.conn.receive(datagram, now);
        self.pump(now);
    }

    fn pump(&mut self, now: u64) {
        while let Some(event) = self.conn.poll_event() {
            match event {
                Event::Connected => {
                    let long = match self.stage {
                        Stage::Login | Stage::Characters => None,
                        Stage::LoginWithCharacter | Stage::Map => self.character,
                    };
                    let p = hello(&self.credentials, long, self.first_login);
                    self.first_login = false;
                    self.conn.send(&p, p.len() * 8, Reliability::ReliableOrdered, 0);
                    self.conn.flush(now);
                }
                Event::Message { data, bits, .. } => self.on_message(&data, bits, now),
                Event::Disconnected(reason) => {
                    if !self.handing_off {
                        self.events.push_back(SessionEvent::Disconnected(reason));
                    }
                }
            }
        }
    }

    fn ready(&mut self, now: u64) {
        // No 0x1B clock of our own: the release261 server echoes it back, and the
        // echo read as its clock (production runtime/transport.py:2211-2212).
        self.conn.send(&[messages::READY], 8, Reliability::ReliableOrdered, 0);
        self.readies = self.readies.saturating_add(1);
        self.conn.flush(now);
    }

    fn on_message(&mut self, data: &[u8], bits: usize, now: u64) {
        match classify(data, bits) {
            Envelope::Identity(name) => {
                // CONTRACT: the service says what it is; the stage follows it, not the
                //   order of hand-offs. FAILURE: a login server that remembers the
                //   account sends it straight to the map (experimental
                //   Sessions.character_of), and the world was read as a roster.
                match name.as_str() {
                    "DrasaOnlineMapServer" => self.stage = Stage::Map,
                    "DrasaCharacterService" => {
                        self.stage = Stage::Characters;
                        self.rostered = false;
                    }
                    // Every map change passes the login server (release261).
                    "DrasaOnlineLoginServer" => {
                        self.stage = if self.character.is_some() { Stage::LoginWithCharacter } else { Stage::Login }
                    }
                    _ => {}
                }
                self.events.push_back(SessionEvent::Service(name))
            }
            Envelope::MapAssignment { name, rule_set } => {
                self.events.push_back(SessionEvent::MapAssigned { name, rule_set })
            }
            Envelope::Proceed => {
                // The login server says "proceed" without wanting a ready; the services
                // that hold a world (characters, map) do.
                if matches!(self.stage, Stage::Characters | Stage::Map) {
                    self.ready(now);
                }
            }
            // release261: milliseconds since the server started; its tick is ms / 40
            // (production services/lifecycle.py:803-817, services/customization.py:400).
            Envelope::Clock { tick } => self.events.push_back(SessionEvent::Clock(tick / 40)),
            Envelope::Handoff { target } => self.hand_off(&target, now),
            Envelope::Commands { data, bits } => {
                // A SwitchMap in a 0x85 batch is a hand-off too (map exits, switching
                // character: production protocol/map_packets.py:65-90).
                if let Some(target) = messages::chained_handoff(&data, bits) {
                    self.hand_off(&target, now);
                    return;
                }
                if self.stage == Stage::Characters {
                    if let Some(c) = messages::character_generation(&data, bits) {
                        match c.operation {
                            2 => self.events.push_back(SessionEvent::CharacterEditor),
                            4 => self.events.push_back(SessionEvent::CreationRefused(c.deny_reason)),
                            // Created: the hand-off that follows plays it.
                            5 => self.character = Some(c.character),
                            _ => {}
                        }
                        return;
                    }
                    if let Some((operation, roster)) = messages::read_roster(&data, bits) {
                        if operation == 1 && !self.rostered {
                            self.rostered = true;
                            self.events.push_back(SessionEvent::Roster(roster));
                        }
                    }
                    if let Some(selection) = messages::character_selection(&data, bits) {
                        self.events.push_back(SessionEvent::CharacterSelection(selection));
                    }
                    return;
                }
                self.events.push_back(SessionEvent::Commands { data, bits });
            }
            Envelope::Other { .. } => {}
        }
    }

    fn hand_off(&mut self, target: &str, now: u64) {
        let next = if target.is_empty() {
            Some(self.login)
        } else {
            resolve(target, self.login.port())
        };
        let Some(next) = next else {
            log::warn!("hand-off to {target:?}, which does not resolve");
            return;
        };
        // Where it leads; the next service's 0x82 name corrects it when it differs
        // (a map exit goes back through the login server).
        self.stage = match (self.stage, self.character.is_some()) {
            (Stage::Login, false) => Stage::Characters,
            (Stage::Characters, _) => Stage::LoginWithCharacter,
            (Stage::Login, true) | (Stage::LoginWithCharacter, _) => Stage::Map,
            (Stage::Map, _) => Stage::LoginWithCharacter,
        };
        self.handing_off = true;
        self.conn.disconnect(now);
        while self.conn.poll_event().is_some() {}
        // The goodbye still has to reach the OLD server: it travels with the event.
        let mut farewell = Vec::new();
        self.conn.drain_outgoing(&mut farewell);
        self.conn = Connection::new(next, self.guid, now);
        self.readies = 0;
        self.handing_off = false;
        self.events.push_back(SessionEvent::Connect { target: next, farewell });
    }
}
