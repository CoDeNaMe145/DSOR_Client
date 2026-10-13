//! End-to-end: WS client -> relay -> UDP echo server -> relay -> WS client.

use dsor_relay::{serve, Config};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use tokio_tungstenite::tungstenite::Message;

async fn start_echo() -> u16 {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = sock.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        loop {
            let (n, from) = sock.recv_from(&mut buf).await.unwrap();
            sock.send_to(&buf[..n], from).await.unwrap();
        }
    });
    port
}

async fn start_relay(allow_port: u16) -> u16 {
    start_relay_with(allow_port, 0).await
}

async fn start_relay_with(allow_port: u16, account_port: u16) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cfg = Config {
        listen: listener.local_addr().unwrap(),
        allow_hosts: vec!["127.0.0.1".into()],
        allow_ports: vec![(allow_port, allow_port)],
        account_ports: vec![(account_port, account_port)],
    };
    tokio::spawn(serve(listener, Arc::new(cfg)));
    port
}

#[tokio::test]
async fn round_trip_datagrams() {
    let echo = start_echo().await;
    let relay = start_relay(echo).await;
    // Percent-encoded colon, as a browser's URLSearchParams would send it.
    let url = format!("ws://127.0.0.1:{relay}/?target=127.0.0.1%3A{echo}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();

    let datagrams: Vec<Vec<u8>> = vec![
        vec![0x05, 0x00, 0xff, 0xff, 0x00],
        vec![],
        (0..=255u8).collect(),
        vec![0xAB; 1400],
        vec![0x84, 0x01, 0x02, 0x03],
    ];
    for d in &datagrams {
        ws.send(Message::Binary(d.clone())).await.unwrap();
    }
    for d in &datagrams {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timeout")
            .expect("stream ended")
            .expect("ws error");
        match msg {
            Message::Binary(got) => assert_eq!(&got, d, "datagram boundaries preserved"),
            other => panic!("unexpected {other:?}"),
        }
    }
    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn rejects_targets_outside_allowlist() {
    let echo = start_echo().await;
    let relay = start_relay(echo).await;
    for target in [
        format!("127.0.0.1:{}", echo.wrapping_add(1)), // port not allowed
        format!("10.1.2.3:{echo}"),                    // host not allowed
        "garbage".to_string(),
    ] {
        let url = format!("ws://127.0.0.1:{relay}/?target={target}");
        let err = tokio_tungstenite::connect_async(url).await.unwrap_err();
        match err {
            tokio_tungstenite::tungstenite::Error::Http(resp) => {
                assert_eq!(resp.status(), 403, "target {target}")
            }
            other => panic!("target {target}: expected HTTP 403, got {other:?}"),
        }
    }
    // No target at all -> 400.
    let url = format!("ws://127.0.0.1:{relay}/");
    match tokio_tungstenite::connect_async(url).await.unwrap_err() {
        tokio_tungstenite::tungstenite::Error::Http(resp) => assert_eq!(resp.status(), 400),
        other => panic!("expected HTTP 400, got {other:?}"),
    }
}

/// A one-shot account service: answers the first request with 200 and a body
/// that echoes the request's JSON, so the test sees the body went through.
async fn start_account() -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let mut got = Vec::new();
        while !String::from_utf8_lossy(&got).contains("}") {
            let n = s.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        let text = String::from_utf8_lossy(&got).to_string();
        assert!(text.starts_with("POST /api/login HTTP/1.1\r\n"), "{text}");
        let body = text.split_once("\r\n\r\n").unwrap().1.to_string();
        let reply = format!("{{\"echo\":{body}}}");
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
        s.write_all(resp.as_bytes()).await.unwrap();
    });
    port
}

#[tokio::test]
async fn logs_in_through_the_relay() {
    let account = start_account().await;
    let relay = start_relay_with(1, account).await;
    let url = format!("ws://127.0.0.1:{relay}/?login=127.0.0.1%3A{account}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    ws.send(Message::Text(r#"{"username":"Hero","password":"secret"}"#.into())).await.unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timeout").unwrap().unwrap();
    match msg {
        Message::Text(t) => assert_eq!(t.as_str(), "200\n{\"echo\":{\"username\":\"Hero\",\"password\":\"secret\"}}"),
        other => panic!("unexpected {other:?}"),
    }
    // A login port outside the list is refused like a target.
    let url = format!("ws://127.0.0.1:{relay}/?login=127.0.0.1%3A{}", account.wrapping_add(1));
    match tokio_tungstenite::connect_async(url).await.unwrap_err() {
        tokio_tungstenite::tungstenite::Error::Http(resp) => assert_eq!(resp.status(), 403),
        other => panic!("expected HTTP 403, got {other:?}"),
    }
}
