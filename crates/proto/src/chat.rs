//! The built-in chat service (UDP 2191): its hello, its one text command, and the
//! `#`-separated commands that text carries.
//!
//! SOURCE: dso-server-production (release261):
//!   protocol/chat_protocol.py:1-8     the service: the usual RakNet handshake, 0x82
//!                                     "ChatService", then everything rides command
//!                                     0x04D2 (Chat::BuiltinChatCommand, 1234)
//!   protocol/chat_protocol.py:49-73   hello: 8A <u16 len>"ChatClient" <u32 character>
//!                                     <u16 len=16><session GUID, raw client bytes>
//!   protocol/chat_protocol.py:92-117  text: <8B client | 84 server> D2 04 <u16 len><utf-8>
//!   protocol/chat_protocol.py:134-189 sm#<channel>#<lang>#<usertype>#<text>   (we send)
//!                                     sw#<target>#<usertype>#<text>           (we send)
//!                                     rm#<channel>#<lang>#<name>#<usertype>#<text>
//!                                     rw#<sender>#<usertype>#<text>
//!   services/chat.py:532-594   the hello is checked against the account's session and
//!                              character, answered 0x88; 50 ms later (chat.py:596-627)
//!                              e#Map__<instance>_<uuid>, e#Region__<instance>_<id>,
//!                              e#Group__/e#Guild__... join the channels
//!   services/chat.py:301-336   l#<channel> leaves a group/guild channel
//!   services/chat.py:629-716   sm on a joined channel goes to every member, the sender
//!                              included; text starting with `!` is an admin command
//!                              instead (chat.py:136-243), answered by a whisper from
//!                              "System" (chat.py:119-134); sw goes to the target only.
//! All string fields are byte aligned, so the chat needs no bit reader.

use std::collections::{HashSet, VecDeque};
use std::net::SocketAddrV4;

use dsor_raknet::{Connection, DisconnectReason, Event, Reliability};

/// Chat::BuiltinChatCommand.
pub const TEXT_COMMAND: u16 = 0x04D2;
pub const SERVICE: &str = "ChatClient";
/// Whispers from this sender are the server's own answers (admin commands).
pub const SYSTEM_SENDER: &str = "System";
/// The language field of what we send. UNVERIFIED: the 2018 client's choice; the
///   server only requires it non-empty (chat_protocol.py:146).
const LANGUAGE: &str = "en";
/// A normal retail user; the server substitutes its own (chat.py:666-669, 690).
const USER_TYPE: &str = "ha";

fn push_string(out: &mut Vec<u8>, s: &[u8]) {
    let len = s.len().min(u16::MAX as usize);
    out.extend_from_slice(&(len as u16).to_le_bytes());
    out.extend_from_slice(&s[..len]);
}

/// The 0x8A hello: who is chatting (the CHARACTER id, the server's user id).
pub fn hello(character: u32, session: &[u8; 16]) -> Vec<u8> {
    let mut out = vec![crate::messages::CLIENT_HELLO];
    push_string(&mut out, SERVICE.as_bytes());
    out.extend_from_slice(&character.to_le_bytes());
    push_string(&mut out, session);
    out
}

/// One client chat command, 8B D2 04 <string>.
pub fn text(command: &str) -> Vec<u8> {
    let mut out = vec![crate::messages::CLIENT_COMMAND];
    out.extend_from_slice(&TEXT_COMMAND.to_le_bytes());
    push_string(&mut out, command.as_bytes());
    out
}

/// The string of a server chat command (84 D2 04 <string>), if it is one.
pub fn read_text(data: &[u8]) -> Option<String> {
    if data.len() < 5 || data[0] != crate::messages::COMMAND || u16::from_le_bytes([data[1], data[2]]) != TEXT_COMMAND {
        return None;
    }
    let len = u16::from_le_bytes([data[3], data[4]]) as usize;
    let body = data.get(5..5 + len)?;
    String::from_utf8(body.to_vec()).ok()
}

/// `#` separates fields; a name or channel cannot hold one, the text may.
fn clean(field: &str) -> String {
    field.replace('#', "")
}

/// Say `text` on a channel (`map`, `region`, `group`, `guild`).
pub fn say(channel: &str, text: &str) -> String {
    format!("sm#{}#{LANGUAGE}#{USER_TYPE}#{text}", clean(channel))
}

/// Whisper `text` to a character. A name with spaces is fine: the server
/// recombines the longest online name (chat.py:418-464).
pub fn whisper(target: &str, text: &str) -> String {
    format!("sw#{}#{USER_TYPE}#{text}", clean(target))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatEvent {
    /// Logged in (the 0x88 answer to the hello).
    LoggedIn,
    /// A channel joined: its client name (`map`, ...) and the server's room.
    Joined { channel: String, room: String },
    Left(String),
    /// A line on a channel (`rm`); `admin` is the `hb` user type.
    Message { channel: String, sender: String, admin: bool, text: String },
    /// A whisper to us (`rw`); from `SYSTEM_SENDER` it is the server's own answer.
    Whisper { sender: String, admin: bool, text: String },
    /// A command this client does not know, kept for the log.
    Unknown(String),
    Disconnected(DisconnectReason),
}

/// The client channel of a server room: `Map__481_<uuid>` -> `map`.
fn channel_of(room: &str) -> String {
    room.split("__").next().unwrap_or(room).to_ascii_lowercase()
}

/// One server chat command.
pub fn parse(command: &str) -> ChatEvent {
    let unknown = || ChatEvent::Unknown(command.to_string());
    let Some((tag, rest)) = command.split_once('#') else { return unknown() };
    match tag {
        "e" => ChatEvent::Joined { channel: channel_of(rest), room: rest.to_string() },
        "l" => ChatEvent::Left(channel_of(rest)),
        "rm" => {
            let f: Vec<&str> = rest.splitn(5, '#').collect();
            let [channel, _language, sender, user_type, text] = f[..] else { return unknown() };
            ChatEvent::Message { channel: channel.to_ascii_lowercase(), sender: sender.into(), admin: user_type == "hb", text: text.into() }
        }
        "rw" => {
            let f: Vec<&str> = rest.splitn(3, '#').collect();
            let [sender, user_type, text] = f[..] else { return unknown() };
            ChatEvent::Whisper { sender: sender.into(), admin: user_type == "hb", text: text.into() }
        }
        _ => unknown(),
    }
}

/// The chat connection, with no I/O (as `session::Session`): feed it datagrams, send
/// what `drain_outgoing` yields to `server()`.
/// CONTRACT: it lives beside the game session and is never handed off: map changes
///   move the game session only, the chat stays connected (chat.py routes `map` by the
///   character's CURRENT map, chat.py:376-400).
pub struct ChatClient {
    conn: Connection,
    character: u32,
    session: [u8; 16],
    logged_in: bool,
    joined: HashSet<String>,
    events: VecDeque<ChatEvent>,
}

impl ChatClient {
    pub fn new(server: SocketAddrV4, guid: u64, character: u32, session: [u8; 16], now: u64) -> Self {
        Self { conn: Connection::new(server, guid, now), character, session, logged_in: false, joined: HashSet::new(), events: VecDeque::new() }
    }

    pub fn server(&self) -> SocketAddrV4 {
        self.conn.server()
    }

    pub fn character(&self) -> u32 {
        self.character
    }

    pub fn is_closed(&self) -> bool {
        self.conn.is_closed()
    }

    pub fn logged_in(&self) -> bool {
        self.logged_in
    }

    pub fn joined(&self, channel: &str) -> bool {
        self.joined.contains(channel)
    }

    pub fn poll_event(&mut self) -> Option<ChatEvent> {
        self.events.pop_front()
    }

    pub fn drain_outgoing(&mut self, into: &mut Vec<Vec<u8>>) {
        self.conn.drain_outgoing(into);
    }

    /// Send one chat command (`say`, `whisper`).
    pub fn send(&mut self, command: &str, now: u64) {
        let p = text(command);
        self.conn.send(&p, p.len() * 8, Reliability::ReliableOrdered, 0);
        self.conn.flush(now);
    }

    pub fn disconnect(&mut self, now: u64) {
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
                    let p = hello(self.character, &self.session);
                    self.conn.send(&p, p.len() * 8, Reliability::ReliableOrdered, 0);
                    self.conn.flush(now);
                }
                Event::Message { data, .. } => self.on_message(&data),
                Event::Disconnected(reason) => {
                    self.logged_in = false;
                    self.joined.clear();
                    self.events.push_back(ChatEvent::Disconnected(reason));
                }
            }
        }
    }

    fn on_message(&mut self, data: &[u8]) {
        match data.first() {
            Some(&crate::messages::PROCEED) => {
                self.logged_in = true;
                self.events.push_back(ChatEvent::LoggedIn);
            }
            // 0x82 "ChatService" and RakNet's own: nothing to do.
            Some(&crate::messages::COMMAND) => {
                let Some(command) = read_text(data) else {
                    log::debug!("chat: unknown 0x84 message {:02x?}", &data[..data.len().min(16)]);
                    return;
                };
                let event = parse(&command);
                match &event {
                    ChatEvent::Joined { channel, .. } => { self.joined.insert(channel.clone()); }
                    ChatEvent::Left(channel) => { self.joined.remove(channel); }
                    _ => {}
                }
                self.events.push_back(event);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn hello_is_the_servers_layout() {
        // encode_chat_client_hello(ChatClientHello("ChatClient", 200000001, bytes(range(16)))).
        let session: [u8; 16] = core::array::from_fn(|i| i as u8);
        assert_eq!(hello(200_000_001, &session), unhex("8a0a0043686174436c69656e7401c2eb0b1000000102030405060708090a0b0c0d0e0f"));
    }

    #[test]
    fn text_round_trips_both_directions() {
        // encode_chat_text("sm#map#en#ha#!help", from_client=True).
        assert_eq!(text(&say("map", "!help")), unhex("8bd2041200736d236d617023656e236861232168656c70"));
        let mut server = text("rw#System#hb#Admin commands: !help");
        server[0] = 0x84;
        assert_eq!(read_text(&server).as_deref(), Some("rw#System#hb#Admin commands: !help"));
        assert_eq!(read_text(&text("x")), None, "a client packet is not a server command");
        assert_eq!(read_text(&[0x84, 0xd2, 0x04, 0x05, 0x00, b'a']), None, "truncated");
        let utf8 = text("sm#map#en#ha#Grüße");
        assert_eq!(u16::from_le_bytes([utf8[3], utf8[4]]) as usize, utf8.len() - 5, "length counts utf-8 bytes");
    }

    #[test]
    fn commands_parse_as_the_server_writes_them() {
        assert_eq!(parse("e#Map__481_6c0f9d4e-2a33-4e5b-9c1d-0a1b2c3d4e5f"),
            ChatEvent::Joined { channel: "map".into(), room: "Map__481_6c0f9d4e-2a33-4e5b-9c1d-0a1b2c3d4e5f".into() });
        assert!(matches!(parse("e#Region__481_2"), ChatEvent::Joined { channel, .. } if channel == "region"));
        assert!(matches!(parse("e#Group__481_7"), ChatEvent::Joined { channel, .. } if channel == "group"));
        assert_eq!(parse("l#guild"), ChatEvent::Left("guild".into()));
        assert_eq!(parse("rm#map#en#Hero One#ha#hi # there"),
            ChatEvent::Message { channel: "map".into(), sender: "Hero One".into(), admin: false, text: "hi # there".into() });
        assert_eq!(parse("rw#System#hb#Command failed: x"),
            ChatEvent::Whisper { sender: SYSTEM_SENDER.into(), admin: true, text: "Command failed: x".into() });
        assert_eq!(parse("rm#map#en"), ChatEvent::Unknown("rm#map#en".into()));
        assert_eq!(parse("zz"), ChatEvent::Unknown("zz".into()));
    }

    #[test]
    fn outgoing_commands_keep_hashes_out_of_their_fields() {
        assert_eq!(say("map", "a#b"), "sm#map#en#ha#a#b");
        assert_eq!(whisper("Bad#Name", "hello"), "sw#BadName#ha#hello");
    }

    #[test]
    fn login_and_joins_are_tracked() {
        let mut c = ChatClient::new("127.0.0.1:2191".parse().unwrap(), 1, 5, [0; 16], 0);
        c.on_message(&[0x82, 0x0b, 0x00]);
        c.on_message(&[0x88]);
        let mut join = text("e#Region__481_2");
        join[0] = 0x84;
        c.on_message(&join);
        assert!(c.logged_in() && c.joined("region") && !c.joined("map"));
        assert_eq!(c.poll_event(), Some(ChatEvent::LoggedIn));
        let mut leave = text("l#region");
        leave[0] = 0x84;
        c.on_message(&leave);
        assert!(!c.joined("region"));
    }
}
