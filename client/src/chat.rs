//! In-game chat: a second connection, to the built-in chat service (UDP 2191), and
//! what the chat window (crate::ui, the 2018 `chatwindow2`) shows and sends.
//!
//! The connection (dsor_proto::chat) opens once a character is on a map, with the
//! account's login session, and stays open across map changes: the game session is
//! handed off from server to server, the chat is not. Lost, it reconnects.
//!
//! The window's keys are crate::ui's (controls::keyboard / edit_text): V shows or
//! hides it, Enter focuses its TextEntry, Enter again sends (`ChatLog::submit`),
//! Escape cancels; while typing, `UiInput.typing` holds the game's keys. Text
//! starting with `!` is sent as ordinary chat: the server reads it as an admin
//! command and answers with a whisper from "System" (production
//! services/chat.py:678-689).
//!
//! The channel shortcuts are the 2018 client's own, from chatwindow2dropdown's
//! labels: /s local (map), /a region, /p group, /g guild, /w <name> whisper.
//! UNVERIFIED: the channel colours are ours: no colour table for them was found in
//!   the exported data.

use std::collections::VecDeque;

use bevy::prelude::*;
use dsor_proto::chat::{self, ChatClient, ChatEvent};
use dsor_transport::Transport;

use crate::net::{now_ms, Net, NetConfig};

/// Lines kept for scrolling back.
const KEPT: usize = 200;
/// Wait between two connection attempts.
const RETRY_MS: u64 = 5_000;

/// The chat window's channels, in the order of its tabs (ChannelTabMap, Region,
/// Party, Guild) and of the dropdown (TextButtonMap ... TextButtonWhisper).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Map,
    Region,
    Group,
    Guild,
    Whisper,
    System,
    /// The client's own notes (connection state).
    Local,
}

impl Kind {
    /// The channel of a tab or dropdown entry.
    pub fn channel(index: u8) -> Kind {
        match index {
            1 => Kind::Region,
            2 => Kind::Group,
            3 => Kind::Guild,
            4 => Kind::Whisper,
            _ => Kind::Map,
        }
    }

    fn of(channel: &str) -> Kind {
        match channel {
            "map" => Kind::Map,
            "region" => Kind::Region,
            "group" => Kind::Group,
            "guild" => Kind::Guild,
            _ => Kind::Local,
        }
    }

    /// The server's channel name (dsor_proto::chat::say).
    fn wire(self) -> Option<&'static str> {
        Some(match self {
            Kind::Map => "map",
            Kind::Region => "region",
            Kind::Group => "group",
            Kind::Guild => "guild",
            _ => return None,
        })
    }

    /// UNVERIFIED: our colours (see the module notes).
    pub fn colour(self) -> Color {
        match self {
            Kind::Map => Color::srgb(0.96, 0.93, 0.86),
            Kind::Region => Color::srgb(1.0, 0.76, 0.42),
            Kind::Group => Color::srgb(0.48, 0.76, 1.0),
            Kind::Guild => Color::srgb(0.52, 0.94, 0.52),
            Kind::Whisper => Color::srgb(0.96, 0.58, 0.96),
            Kind::System => Color::srgb(1.0, 0.87, 0.3),
            Kind::Local => Color::srgb(0.65, 0.65, 0.65),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChatLine {
    pub kind: Kind,
    pub text: String,
}

/// What the window shows and what waits to be sent.
#[derive(Resource, Default)]
pub struct ChatLog {
    pub lines: VecDeque<ChatLine>,
    /// Display rows scrolled back from the newest (the window's scroll buttons).
    pub scroll: usize,
    /// Chat commands (`chat::say`, `chat::whisper`) for `pump`.
    outbox: Vec<String>,
}

impl ChatLog {
    pub fn push(&mut self, kind: Kind, text: String) {
        info!("chat: {text}");
        self.lines.push_back(ChatLine { kind, text });
        while self.lines.len() > KEPT {
            self.lines.pop_front();
        }
    }

    /// One server event, as a line when it is one.
    fn event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::LoggedIn => self.push(Kind::Local, "Connected to chat.".into()),
            ChatEvent::Joined { channel, room } => info!("chat: joined {channel} ({room})"),
            ChatEvent::Left(channel) => info!("chat: left {channel}"),
            ChatEvent::Message { channel, sender, text, .. } => {
                let kind = Kind::of(&channel);
                self.push(kind, format!("[{}] {sender}: {text}", label(kind)));
            }
            ChatEvent::Whisper { sender, text, .. } if sender == chat::SYSTEM_SENDER => {
                self.push(Kind::System, format!("[System] {text}"));
            }
            ChatEvent::Whisper { sender, text, .. } => self.push(Kind::Whisper, format!("[From {sender}] {text}")),
            ChatEvent::Unknown(command) => info!("chat: unknown command {command:?}"),
            ChatEvent::Disconnected(reason) => self.push(Kind::Local, format!("Chat disconnected ({reason:?}).")),
        }
    }

    /// What the player typed on the window's current channel, as a chat command.
    /// A leading /s /a /p /g /w picks another channel for this line; a whisper is
    /// echoed here (the server sends it to the target only, production
    /// protocol/chat_protocol.py:178-183).
    pub fn submit(&mut self, channel: Kind, typed: &str) {
        let typed = typed.trim();
        if typed.is_empty() {
            return;
        }
        let (kind, text) = match typed.split_once(' ') {
            Some(("/s", rest)) => (Kind::Map, rest),
            Some(("/a", rest)) => (Kind::Region, rest),
            Some(("/p", rest)) => (Kind::Group, rest),
            Some(("/g", rest)) => (Kind::Guild, rest),
            Some(("/w", rest)) => (Kind::Whisper, rest),
            _ => (channel, typed),
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.scroll = 0;
        match kind.wire() {
            Some(wire) => self.outbox.push(chat::say(wire, text)),
            None => {
                let Some((target, text)) = text.split_once(' ') else {
                    self.push(Kind::Local, "Whisper: <name> <text>".into());
                    return;
                };
                self.push(Kind::Whisper, format!("[To {target}] {text}"));
                self.outbox.push(chat::whisper(target, text));
            }
        }
    }

    #[cfg(test)]
    pub fn outbox(&self) -> &[String] {
        &self.outbox
    }

    /// The newest `rows` display rows, `columns` characters wide at most, oldest
    /// first, after scrolling back `self.scroll` rows (clamped).
    pub fn rows(&mut self, rows: usize, columns: usize) -> Vec<(Kind, String)> {
        let mut all: Vec<(Kind, String)> = Vec::new();
        for line in &self.lines {
            all.extend(wrap(&line.text, columns).into_iter().map(|row| (line.kind, row)));
        }
        self.scroll = self.scroll.min(all.len().saturating_sub(rows));
        let end = all.len() - self.scroll;
        all.drain(end.saturating_sub(rows)..end).collect()
    }
}

/// Word wrap by characters (the log rows are one Text2d each).
fn wrap(text: &str, columns: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    for word in text.split(' ') {
        let mut word = word.to_string();
        loop {
            let used = row.chars().count();
            let len = word.chars().count();
            if used + usize::from(used > 0) + len <= columns {
                if used > 0 { row.push(' '); }
                row.push_str(&word);
                break;
            }
            if used > 0 {
                rows.push(std::mem::take(&mut row));
                continue;
            }
            // A word longer than a row: cut it.
            let cut: String = word.chars().take(columns).collect();
            word = word.chars().skip(columns).collect();
            rows.push(cut);
        }
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Map => "Map",
        Kind::Region => "Region",
        Kind::Group => "Group",
        Kind::Guild => "Guild",
        Kind::Whisper => "Whisper",
        Kind::System => "System",
        Kind::Local => "Chat",
    }
}

/// The chat connection. CONTRACT: non-send, as `Net` (the browser's transport).
#[derive(Default)]
struct ChatNet {
    client: Option<ChatClient>,
    transport: Option<Transport>,
    retry_at: u64,
    /// DSOR_CHAT_SEND was sent (`pump`).
    scripted: bool,
}

impl Drop for ChatNet {
    fn drop(&mut self) {
        self.close(u64::MAX / 2);
    }
}

impl ChatNet {
    fn close(&mut self, now: u64) {
        if let Some(mut c) = self.client.take() {
            c.disconnect(now);
            self.flush_from(&mut c);
        }
        self.transport = None;
    }

    fn flush_from(&mut self, client: &mut ChatClient) {
        let mut out = Vec::new();
        client.drain_outgoing(&mut out);
        if let Some(t) = self.transport.as_mut() {
            for d in &out {
                let _ = t.send(d);
            }
        }
    }
}

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatLog>()
            .insert_non_send(ChatNet::default())
            .add_systems(PreUpdate, pump.after(crate::net::NetReceive));
    }
}


/// The connection: open it once a character plays, feed it, send the outbox.
fn pump(
    mut chat: NonSendMut<ChatNet>,
    net: Option<NonSend<Net>>,
    config: Option<Res<NetConfig>>,
    time: Res<Time<Real>>,
    mut log: ResMut<ChatLog>,
) {
    let now = now_ms(&time);
    let chat = &mut *chat;
    let Some(net) = net else {
        // Signed out (crate::login drops Net): the chat goes with it.
        if chat.client.is_some() {
            chat.close(now);
        }
        if !log.outbox.is_empty() {
            log.outbox.clear();
        }
        return;
    };
    if let Some((account, character, session)) = net.chat_identity() {
        if chat.client.as_ref().is_some_and(|c| c.character() != character) {
            info!("chat: character changed, reconnecting");
            chat.close(now);
        }
        if chat.client.is_none() && now >= chat.retry_at {
            chat.retry_at = now + RETRY_MS;
            let target = config.as_ref().and_then(|c| c.chat.clone()).unwrap_or_else(|| {
                let login = config.as_ref().map_or("127.0.0.1", |c| c.login.as_str());
                format!("{}:2191", login.rsplit_once(':').map_or(login, |(h, _)| h))
            });
            match dsor_proto::session::resolve(&target, 2191) {
                Some(addr) => match Transport::connect(&addr.to_string(), net.relay()) {
                    Ok(t) => {
                        info!("chat: connecting to {addr} as character {character}");
                        // The same GUID scheme as the game session (crate::net::start_session).
                        let guid = 0x0660_0000_0000_0000 | (account as u64) << 8 | 0x43;
                        chat.client = Some(ChatClient::new(addr, guid, character, session, now));
                        chat.transport = Some(t);
                    }
                    Err(e) => warn!("chat: cannot reach {addr}: {e}"),
                },
                None => warn!("chat: {target} does not resolve"),
            }
        }
    }
    // DSOR_CHAT_SEND="hello|!help": lines to send once the map channel is joined
    // (a test hook, as DSOR_CAST or DSOR_AUTOWALK).
    #[cfg(not(target_arch = "wasm32"))]
    if chat.client.as_ref().is_some_and(|c| c.joined("map")) && !std::mem::replace(&mut chat.scripted, true) {
        if let Ok(script) = std::env::var("DSOR_CHAT_SEND") {
            for line in script.split('|') {
                log.submit(Kind::Map, line);
            }
        }
    }
    let Some(mut client) = chat.client.take() else {
        if !log.outbox.is_empty() {
            log.outbox.clear();
            log.push(Kind::Local, "Not connected to chat.".into());
        }
        return;
    };
    if let Some(t) = chat.transport.as_mut() {
        let mut inbox = Vec::new();
        t.poll(&mut inbox);
        for d in inbox {
            client.receive(&d, now);
        }
    }
    client.update(now);
    let outbox = if log.outbox.is_empty() { Vec::new() } else { std::mem::take(&mut log.outbox) };
    for command in outbox {
        // The server drops a line on a channel not joined yet (chat.py:671-676).
        let channel = command.strip_prefix("sm#").and_then(|r| r.split('#').next()).unwrap_or("");
        if !client.logged_in() || (!channel.is_empty() && !client.joined(channel)) {
            log.push(Kind::Local, format!("Not in the {} channel.", if channel.is_empty() { "chat" } else { label(Kind::of(channel)) }));
            continue;
        }
        info!("chat: send {command:?}");
        client.send(&command, now);
    }
    while let Some(event) = client.poll_event() {
        log.event(event);
    }
    chat.flush_from(&mut client);
    if client.is_closed() {
        chat.transport = None;
        chat.retry_at = now + RETRY_MS;
    } else {
        chat.client = Some(client);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lines_become_chat_commands() {
        let mut log = ChatLog::default();
        log.submit(Kind::Map, "  hello there ");
        log.submit(Kind::Map, "!map a0200_kingscity");
        log.submit(Kind::Region, "anyone?");
        log.submit(Kind::Map, "/p ready?");
        log.submit(Kind::Map, "/g guild line");
        log.submit(Kind::Map, "/w Hero Two psst");
        log.submit(Kind::Whisper, "Lonely");
        log.submit(Kind::Map, "   ");
        assert_eq!(log.outbox, vec![
            "sm#map#en#ha#hello there",
            "sm#map#en#ha#!map a0200_kingscity",
            "sm#region#en#ha#anyone?",
            "sm#group#en#ha#ready?",
            "sm#guild#en#ha#guild line",
            "sw#Hero#ha#Two psst",
        ]);
        assert_eq!(log.lines.iter().map(|l| (l.kind, l.text.as_str())).collect::<Vec<_>>(), vec![
            (Kind::Whisper, "[To Hero] Two psst"),
            (Kind::Local, "Whisper: <name> <text>"),
        ]);
    }

    #[test]
    fn server_lines_name_their_sender_and_channel() {
        let mut log = ChatLog::default();
        log.event(chat::parse("rm#region#en#Hero One#ha#hi"));
        log.event(chat::parse("rw#System#hb#Admin commands: !help"));
        log.event(chat::parse("rw#Hero Two#ha#psst"));
        log.event(chat::parse("e#Map__481_x"));
        assert_eq!(log.lines.iter().map(|l| (l.kind, l.text.as_str())).collect::<Vec<_>>(), vec![
            (Kind::Region, "[Region] Hero One: hi"),
            (Kind::System, "[System] Admin commands: !help"),
            (Kind::Whisper, "[From Hero Two] psst"),
        ]);
    }

    #[test]
    fn rows_wrap_and_scroll_from_the_newest() {
        assert_eq!(wrap("aa bb cc", 5), vec!["aa bb", "cc"]);
        assert_eq!(wrap("abcdefgh", 3), vec!["abc", "def", "gh"]);
        assert_eq!(wrap("", 3), vec![""]);
        let mut log = ChatLog::default();
        for i in 0..5 {
            log.push(Kind::Map, format!("line {i}"));
        }
        log.push(Kind::System, "long long".into());
        let texts = |rows: Vec<(Kind, String)>| rows.into_iter().map(|r| r.1).collect::<Vec<_>>();
        assert_eq!(texts(log.rows(3, 6)), vec!["line 4", "long", "long"]);
        log.scroll = 2;
        assert_eq!(texts(log.rows(3, 6)), vec!["line 2", "line 3", "line 4"]);
        log.scroll = 99;
        assert_eq!(texts(log.rows(3, 6)), vec!["line 0", "line 1", "line 2"]);
        assert_eq!(log.scroll, 4, "clamped to the oldest row");
    }
}
