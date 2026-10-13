//! Datagram transport: UDP natively, the WebSocket relay in the browser.
//!
//! One API for both targets. A `Transport` carries whole datagrams (RakNet
//! packets) to and from one `host:port` target:
//! - native: a non-blocking `std::net::UdpSocket` `connect()`ed to the target;
//! - wasm32: a WebSocket to the relay (`crates/relay`), `?target=<host:port>`,
//!   where one binary message is exactly one datagram.
//!
//! CONTRACT: `send` and `poll` never block. Datagrams are unreliable either way;
//! RakNet above handles loss and ordering.

use std::fmt;

#[derive(Debug)]
pub enum TransportError {
    /// The target is not "host:port", or does not resolve.
    InvalidTarget(String),
    /// wasm32 needs a relay URL; none was given.
    NoRelay,
    /// The connection is closed (WebSocket closed/errored).
    Closed,
    /// OS socket error (native).
    Io(std::io::Error),
    /// JavaScript exception (wasm32), stringified.
    Js(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::InvalidTarget(t) => write!(f, "invalid target {t:?}"),
            TransportError::NoRelay => write!(f, "no relay URL given (required in the browser)"),
            TransportError::Closed => write!(f, "transport closed"),
            TransportError::Io(e) => write!(f, "io: {e}"),
            TransportError::Js(e) => write!(f, "js: {e}"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TransportError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        TransportError::Io(e)
    }
}

/// Build the relay URL for `target`: `relay_url` plus `target=<percent-encoded>`.
pub fn relay_url_for(relay_url: &str, target: &str) -> String {
    let mut enc = String::with_capacity(target.len() * 3);
    for b in target.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    let sep = if relay_url.contains('?') { '&' } else { '?' };
    // A bare origin like ws://host:2290 gets the "/" path so the URL stays valid.
    let base = if relay_url.matches('/').count() == 2 {
        format!("{relay_url}/")
    } else {
        relay_url.to_string()
    };
    format!("{base}{sep}target={enc}")
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::TransportError;
    use std::io::ErrorKind;
    use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

    const MAX_DATAGRAM: usize = 65_536;

    pub struct Transport {
        sock: UdpSocket,
        buf: Box<[u8]>,
        open: bool,
    }

    impl Transport {
        /// Non-blocking UDP socket `connect()`ed to `target`. `relay_url` is ignored.
        pub fn connect(target: &str, _relay_url: Option<&str>) -> Result<Transport, TransportError> {
            let addr = target
                .to_socket_addrs()
                .map_err(|e| TransportError::InvalidTarget(format!("{target}: {e}")))?
                .next()
                .ok_or_else(|| TransportError::InvalidTarget(target.to_string()))?;
            let bind: SocketAddr = if addr.is_ipv4() {
                "0.0.0.0:0".parse().unwrap()
            } else {
                "[::]:0".parse().unwrap()
            };
            let sock = UdpSocket::bind(bind)?;
            sock.connect(addr)?;
            sock.set_nonblocking(true)?;
            log::info!("transport: udp {} -> {addr}", sock.local_addr()?);
            Ok(Transport {
                sock,
                buf: vec![0u8; MAX_DATAGRAM].into_boxed_slice(),
                open: true,
            })
        }

        pub fn send(&mut self, datagram: &[u8]) -> Result<(), TransportError> {
            if !self.open {
                return Err(TransportError::Closed);
            }
            match self.sock.send(datagram) {
                Ok(_) => Ok(()),
                // Full send buffer: drop the datagram rather than block (UDP is lossy).
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    log::debug!("transport: send buffer full, dropped {} B", datagram.len());
                    Ok(())
                }
                // ICMP port-unreachable from an earlier datagram; not fatal for UDP.
                Err(e) if e.kind() == ErrorKind::ConnectionRefused => Ok(()),
                Err(e) => Err(e.into()),
            }
        }

        pub fn poll(&mut self, out: &mut Vec<Vec<u8>>) {
            if !self.open {
                return;
            }
            loop {
                match self.sock.recv(&mut self.buf) {
                    Ok(n) => out.push(self.buf[..n].to_vec()),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == ErrorKind::ConnectionRefused => continue,
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => {
                        log::warn!("transport: recv: {e}");
                        break;
                    }
                }
            }
        }

        /// UDP has no handshake: open from `connect` until `close`.
        pub fn is_open(&self) -> bool {
            self.open
        }

        pub fn close(&mut self) {
            self.open = false;
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::{relay_url_for, TransportError};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use web_sys::{BinaryType, CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

    #[derive(Default)]
    struct Shared {
        rx: VecDeque<Vec<u8>>,
        /// Sends issued while CONNECTING, flushed in order on open.
        pending: Vec<Vec<u8>>,
        closed: bool,
    }

    pub struct Transport {
        ws: WebSocket,
        shared: Rc<RefCell<Shared>>,
        _onopen: Closure<dyn FnMut(Event)>,
        _onmessage: Closure<dyn FnMut(MessageEvent)>,
        _onerror: Closure<dyn FnMut(ErrorEvent)>,
        _onclose: Closure<dyn FnMut(CloseEvent)>,
    }

    fn js_err(e: wasm_bindgen::JsValue) -> TransportError {
        TransportError::Js(format!("{e:?}"))
    }

    impl Transport {
        /// WebSocket to `relay_url` with `?target=<target>`, binaryType arraybuffer.
        pub fn connect(target: &str, relay_url: Option<&str>) -> Result<Transport, TransportError> {
            let relay = relay_url.ok_or(TransportError::NoRelay)?;
            let valid = target
                .rsplit_once(':')
                .is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok());
            if !valid {
                return Err(TransportError::InvalidTarget(target.to_string()));
            }
            let url = relay_url_for(relay, target);
            let ws = WebSocket::new(&url).map_err(js_err)?;
            ws.set_binary_type(BinaryType::Arraybuffer);
            let shared = Rc::new(RefCell::new(Shared::default()));

            let onopen = {
                let (ws, shared, url) = (ws.clone(), shared.clone(), url.clone());
                Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
                    log::info!("transport: relay open {url}");
                    let pending = std::mem::take(&mut shared.borrow_mut().pending);
                    for d in pending {
                        if let Err(e) = ws.send_with_u8_array(&d) {
                            log::warn!("transport: flush send: {e:?}");
                        }
                    }
                })
            };
            let onmessage = {
                let shared = shared.clone();
                Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                    if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                        shared
                            .borrow_mut()
                            .rx
                            .push_back(js_sys::Uint8Array::new(&buf).to_vec());
                    } else {
                        log::debug!("transport: ignoring non-binary message");
                    }
                })
            };
            let onerror = {
                let shared = shared.clone();
                Closure::<dyn FnMut(ErrorEvent)>::new(move |e: ErrorEvent| {
                    log::warn!("transport: websocket error: {}", e.message());
                    shared.borrow_mut().closed = true;
                })
            };
            let onclose = {
                let shared = shared.clone();
                Closure::<dyn FnMut(CloseEvent)>::new(move |e: CloseEvent| {
                    log::info!("transport: websocket closed ({} {})", e.code(), e.reason());
                    shared.borrow_mut().closed = true;
                })
            };
            ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));

            Ok(Transport {
                ws,
                shared,
                _onopen: onopen,
                _onmessage: onmessage,
                _onerror: onerror,
                _onclose: onclose,
            })
        }

        /// Sends while CONNECTING are queued and flushed in order on open.
        pub fn send(&mut self, datagram: &[u8]) -> Result<(), TransportError> {
            match self.ws.ready_state() {
                WebSocket::OPEN => self.ws.send_with_u8_array(datagram).map_err(js_err),
                WebSocket::CONNECTING => {
                    self.shared.borrow_mut().pending.push(datagram.to_vec());
                    Ok(())
                }
                _ => Err(TransportError::Closed),
            }
        }

        pub fn poll(&mut self, out: &mut Vec<Vec<u8>>) {
            out.extend(self.shared.borrow_mut().rx.drain(..));
        }

        /// WebSocket OPEN.
        pub fn is_open(&self) -> bool {
            self.ws.ready_state() == WebSocket::OPEN && !self.shared.borrow().closed
        }

        pub fn close(&mut self) {
            let _ = self.ws.close();
        }
    }

    /// The browser's login through the relay (see `AccountLogin`).
    pub struct WsLogin {
        ws: WebSocket,
        reply: Rc<RefCell<Option<Result<(u16, String), String>>>>,
        _onopen: Closure<dyn FnMut(Event)>,
        _onmessage: Closure<dyn FnMut(MessageEvent)>,
        _onclose: Closure<dyn FnMut(CloseEvent)>,
    }

    impl WsLogin {
        pub fn start(url: &str, body: String) -> Result<Self, TransportError> {
            let ws = WebSocket::new(url).map_err(js_err)?;
            let reply = Rc::new(RefCell::new(None));
            let onopen = {
                let ws = ws.clone();
                Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
                    if let Err(e) = ws.send_with_str(&body) {
                        log::warn!("login: send: {e:?}");
                    }
                })
            };
            let onmessage = {
                let reply = reply.clone();
                Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                    if let Some(text) = e.data().as_string() {
                        *reply.borrow_mut() =
                            Some(super::split_login_reply(&text).ok_or_else(|| format!("bad relay reply {text:?}")));
                    }
                })
            };
            let onclose = {
                let reply = reply.clone();
                Closure::<dyn FnMut(CloseEvent)>::new(move |e: CloseEvent| {
                    let mut r = reply.borrow_mut();
                    if r.is_none() {
                        *r = Some(Err(format!("relay closed the login ({} {})", e.code(), e.reason())));
                    }
                })
            };
            ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            Ok(Self { ws, reply, _onopen: onopen, _onmessage: onmessage, _onclose: onclose })
        }

        pub fn poll(&mut self) -> Option<Result<(u16, String), String>> {
            self.reply.borrow_mut().take()
        }
    }

    impl Drop for WsLogin {
        fn drop(&mut self) {
            self.ws.set_onopen(None);
            self.ws.set_onmessage(None);
            self.ws.set_onclose(None);
            let _ = self.ws.close();
        }
    }

    impl Drop for Transport {
        fn drop(&mut self) {
            self.ws.set_onopen(None);
            self.ws.set_onmessage(None);
            self.ws.set_onerror(None);
            self.ws.set_onclose(None);
            let _ = self.ws.close();
        }
    }
}

/// One datagram transport for both targets; see the crate docs.
pub use imp::Transport;

/// The JSON body of an account login: `{"username": .., "password": ..}`.
pub fn login_body(username: &str, password: &str) -> String {
    fn quote(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
    format!("{{\"username\":{},\"password\":{}}}", quote(username), quote(password))
}

/// The relay URL that logs in at the account service `account` ("host:port").
pub fn relay_login_url(relay_url: &str, account: &str) -> String {
    relay_url_for(relay_url, account).replacen("target=", "login=", 1)
}

/// A login at the account service's `POST /api/login`, in flight.
///
/// CONTRACT: the release261 server admits UDP only from the address that logged
///   in over HTTP (production persistence/account_registry.py:764-781), so the
///   browser logs in THROUGH the relay (`?login=host:port`, one text message with
///   the JSON body, one text reply "<status>\n<body>"), which also sends its UDP.
///   Natively the request goes straight from this process.
pub struct AccountLogin {
    #[cfg(not(target_arch = "wasm32"))]
    rx: std::sync::mpsc::Receiver<Result<(u16, String), String>>,
    #[cfg(target_arch = "wasm32")]
    inner: imp::WsLogin,
}

impl AccountLogin {
    /// Start logging in as `username` at `account` ("host:port"); the browser needs
    /// the relay.
    pub fn start(account: &str, relay_url: Option<&str>, username: &str, password: &str) -> Result<Self, TransportError> {
        let body = login_body(username, password);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = relay_url;
            let (tx, rx) = std::sync::mpsc::channel();
            let account = account.to_string();
            std::thread::spawn(move || {
                let _ = tx.send(http_post(&account, "/api/login", &body));
            });
            Ok(Self { rx })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let relay = relay_url.ok_or(TransportError::NoRelay)?;
            Ok(Self { inner: imp::WsLogin::start(&relay_login_url(relay, account), body)? })
        }
    }

    /// The HTTP status and body once the answer is in; an error if it failed.
    pub fn poll(&mut self) -> Option<Result<(u16, String), String>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.rx.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("login thread ended".into())),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.poll()
        }
    }
}

/// "<status>\n<body>", the relay's login reply, back to (status, body).
pub fn split_login_reply(reply: &str) -> Option<(u16, String)> {
    let (status, body) = reply.split_once('\n')?;
    Some((status.trim().parse().ok()?, body.to_string()))
}

/// A blocking HTTP/1.1 POST of a JSON body to `host:port` + `path`; (status, body).
#[cfg(not(target_arch = "wasm32"))]
pub fn http_post(host_port: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;
    let addr = host_port
        .to_socket_addrs()
        .map_err(|e| format!("{host_port}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host_port} does not resolve"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("{host_port}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("no HTTP header end")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad status line {:?}", head.lines().next()))?;
    Ok((status, body.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_url() {
        assert_eq!(
            relay_url_for("ws://h:2290", "127.0.0.1:2190"),
            "ws://h:2290/?target=127.0.0.1%3A2190"
        );
        assert_eq!(
            relay_url_for("ws://h:2290/?x=1", "127.0.0.1:30000"),
            "ws://h:2290/?x=1&target=127.0.0.1%3A30000"
        );
    }

    #[test]
    fn login_pieces() {
        assert_eq!(login_body("Hero", "p\"w\\d"), r#"{"username":"Hero","password":"p\"w\\d"}"#);
        assert_eq!(relay_login_url("ws://h:2290", "127.0.0.1:8080"), "ws://h:2290/?login=127.0.0.1%3A8080");
        assert_eq!(split_login_reply("200\n{\"a\":1}"), Some((200, "{\"a\":1}".to_string())));
        assert_eq!(split_login_reply("garbage"), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn udp_loopback() {
        use std::net::UdpSocket;
        use std::time::{Duration, Instant};

        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        server.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let target = server.local_addr().unwrap().to_string();
        let mut t = Transport::connect(&target, None).unwrap();
        assert!(t.is_open());

        let mut out = Vec::new();
        t.poll(&mut out); // nothing yet, must not block
        assert!(out.is_empty());

        let sent: Vec<Vec<u8>> = vec![vec![1, 2, 3], vec![0xAA; 1200], vec![9]];
        for d in &sent {
            t.send(d).unwrap();
        }
        let mut buf = [0u8; 2048];
        for d in &sent {
            let (n, from) = server.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..n], &d[..]);
            server.send_to(&buf[..n], from).unwrap();
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        while out.len() < sent.len() && Instant::now() < deadline {
            t.poll(&mut out);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(out, sent);
    }
}
