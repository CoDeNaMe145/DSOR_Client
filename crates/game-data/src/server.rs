//! The account server's static database. A server can play with its own
//! `static.db4` (rows of its own, such as the admin buffs, or another build's),
//! and status effects travel by row (wire id = rowid - 1), so the game data
//! must hold that server's copy byte for byte. Its account service publishes
//! it (dso_server `services/content.py`):
//!
//! - `GET /api/content`: `{"version", "static_db": {"sha256", "size", "packed_size"} or null}`;
//! - `POST /api/content/static-db` with `{"session_id"}`: the database packed as
//!   the CDN packs files (`__ZN`, the size, zlib), for a signed-in player.
//!
//! [`adopt_static_db`] compares the server's sha256 with the folder's copy and,
//! when they differ, downloads the server's, checks its size, sha256 and SQLite
//! header, writes it in place and records it in `.dsor/overrides.txt`, so the
//! CDN check keeps it (crate::sync). Only these new endpoints are asked: what
//! the official client and its launcher use stays as it is.
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::state::{io, lock, read_overrides, write_overrides, Override};
use crate::{unpack, Error};

/// Where the database is in the game data folder.
pub const STATIC_DB: &str = "export_win32/db/static.db4";
/// No static database comes near this.
const MAX_SIZE: u64 = 512 << 20;

/// What adopting the server's database came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Adoption {
    /// The server publishes no database (or is older than the endpoints).
    NotPublished,
    /// The folder's copy is the server's already.
    Matching { sha256: String },
    /// The server's copy was downloaded and put in place.
    Installed { sha256: String, size: u64 },
}

/// Gives `root` (a game data folder) the static database of the account
/// service at `address` (`host:port`), signed in with `session`.
pub fn adopt_static_db(root: &Path, address: &str, session: &str) -> Result<Adoption, Error> {
    let agent = agent();
    let base = format!("http://{address}");
    let mut response = agent.get(format!("{base}/api/content")).call().map_err(|e| network(&base, e))?;
    if response.status() == 404 {
        return Ok(Adoption::NotPublished);
    }
    if response.status() != 200 {
        return Err(Error::Network(format!("{base}/api/content: HTTP {}", response.status())));
    }
    let text = response.body_mut().with_config().limit(64 << 10).read_to_string().map_err(|e| network(&base, e))?;
    let content: serde_json::Value = serde_json::from_str(&text).map_err(|e| Error::Corrupt(format!("{base}/api/content: {e}")))?;
    let published = &content["static_db"];
    if published.is_null() {
        return Ok(Adoption::NotPublished);
    }
    let (Some(sha256), Some(size), Some(packed)) = (published["sha256"].as_str(), published["size"].as_u64(), published["packed_size"].as_u64()) else {
        return Err(Error::Corrupt(format!("{base}/api/content: no sha256, size or packed size")));
    };
    let sha256 = sha256.to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) || size > MAX_SIZE || packed > MAX_SIZE {
        return Err(Error::Corrupt(format!("{base}/api/content: an unlikely database ({size} bytes, {sha256:?})")));
    }
    let target = root.join(STATIC_DB);
    let _lock = lock(root)?;
    let dir = root.join(crate::state::DIR);
    // Installed here before: its recorded sha256 stands for it while its size holds.
    let kept = read_overrides(&dir).remove(STATIC_DB).filter(|kept| fs::metadata(&target).is_ok_and(|m| m.len() == kept.size));
    let local = match kept {
        Some(kept) => Some(kept.sha256),
        None => file_sha256(&target).ok(),
    };
    if local.as_deref() == Some(sha256.as_str()) {
        return Ok(Adoption::Matching { sha256 });
    }
    log::info!("game data: the server's static database ({sha256}) differs from the folder's; downloading it");
    let body = serde_json::json!({ "session_id": session }).to_string();
    let mut response = agent.post(format!("{base}/api/content/static-db")).header("Content-Type", "application/json")
        .send(body).map_err(|e| network(&base, e))?;
    if response.status() != 200 {
        let why = response.body_mut().with_config().limit(4 << 10).read_to_string().unwrap_or_default();
        return Err(Error::Network(format!("{base}/api/content/static-db: HTTP {} {}", response.status(), why.trim())));
    }
    let blob = response.body_mut().with_config().limit(packed + 1).read_to_vec().map_err(|e| network(&base, e))?;
    let data = unpack(&blob)?;
    let got = hex(&Sha256::digest(&data));
    if data.len() as u64 != size || got != sha256 {
        return Err(Error::Corrupt(format!("the server's static database arrived as {} bytes, sha256 {got}; announced {size}, {sha256}", data.len())));
    }
    if !data.starts_with(b"SQLite format 3\0") {
        return Err(Error::Corrupt("the server's static database is no SQLite database".into()));
    }
    // In place in one step: the old copy stays whole until the new one is.
    let part = root.join(crate::state::DIR).join("tmp").join("static.db4");
    if let Some(folder) = target.parent() {
        fs::create_dir_all(folder).map_err(|e| io(folder, e))?;
    }
    fs::write(&part, &data).and_then(|()| fs::rename(&part, &target)).map_err(|e| io(&target, e))?;
    let mut overrides = read_overrides(&dir);
    overrides.insert(STATIC_DB.to_string(), Override { sha256: sha256.clone(), size, source: address.to_string() });
    write_overrides(&dir, &overrides)?;
    log::info!("game data: installed the static database of {address} ({size} bytes, {sha256})");
    Ok(Adoption::Installed { sha256, size })
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .user_agent(concat!("dsor-client/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build();
    ureq::Agent::new_with_config(config)
}

fn network(base: &str, error: ureq::Error) -> Error {
    Error::Network(format!("{base}: {error}"))
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&chunk[..n]);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::tests::zn;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// An account service in a thread: answers each request from `reply`
    /// (method, path, body) and records the requests.
    fn serve(reply: impl Fn(&str, &str, &str) -> (u16, Vec<u8>) + Send + 'static) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let (method, path) = { let mut p = line.split_whitespace(); (p.next().unwrap_or("").to_string(), p.next().unwrap_or("").to_string()) };
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body = String::from_utf8_lossy(&body).to_string();
                log.lock().unwrap().push(format!("{method} {path}"));
                let (status, answer) = reply(&method, &path, &body);
                let mut stream = stream;
                let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", answer.len());
                let _ = stream.write_all(&answer);
            }
        });
        (address, seen)
    }

    fn folder(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("dsor-server-db-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("export_win32/db")).unwrap();
        fs::write(path.join(STATIC_DB), b"SQLite format 3\0the CDN's").unwrap();
        path
    }

    fn server_with(database: &'static [u8]) -> (String, Arc<Mutex<Vec<String>>>) {
        serve(move |_, path, body| match path {
            "/api/content" => (200, serde_json::json!({"version": 1, "static_db": {
                "sha256": hex(&Sha256::digest(database)), "size": database.len(), "packed_size": zn(database).len()}}).to_string().into_bytes()),
            "/api/content/static-db" if body.contains("\"good\"") => (200, zn(database)),
            _ => (401, b"{\"error\": \"sign in\"}".to_vec()),
        })
    }

    #[test]
    fn a_different_database_is_downloaded_checked_and_kept() {
        const SERVER: &[u8] = b"SQLite format 3\0the server's, with admin buffs";
        let root = folder("adopt");
        let (address, seen) = server_with(SERVER);
        let adopted = adopt_static_db(&root, &address, "good").unwrap();
        assert_eq!(adopted, Adoption::Installed { sha256: hex(&Sha256::digest(SERVER)), size: SERVER.len() as u64 });
        assert_eq!(fs::read(root.join(STATIC_DB)).unwrap(), SERVER);
        let kept = read_overrides(&root.join(crate::state::DIR));
        assert_eq!(kept[STATIC_DB].size, SERVER.len() as u64);
        assert_eq!(kept[STATIC_DB].source, address);
        assert!(matches!(adopt_static_db(&root, &address, "good").unwrap(), Adoption::Matching { .. }));
        assert_eq!(seen.lock().unwrap().iter().filter(|r| r.starts_with("POST")).count(), 1, "downloaded once");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn nothing_is_written_without_a_session_or_from_a_server_without_the_endpoints() {
        const SERVER: &[u8] = b"SQLite format 3\0the server's";
        let root = folder("refused");
        let (address, _) = server_with(SERVER);
        assert!(matches!(adopt_static_db(&root, &address, "expired"), Err(Error::Network(m)) if m.contains("401")));
        let (old, _) = serve(|_, _, _| (404, b"{\"error\": \"no endpoint\"}".to_vec()));
        assert_eq!(adopt_static_db(&root, &old, "good").unwrap(), Adoption::NotPublished);
        assert_eq!(fs::read(root.join(STATIC_DB)).unwrap(), b"SQLite format 3\0the CDN's");
        assert!(read_overrides(&root.join(crate::state::DIR)).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_damaged_or_foreign_download_is_refused() {
        let root = folder("damaged");
        let lying = serve(|_, path, _| match path {
            "/api/content" => (200, serde_json::json!({"static_db": {"sha256": "0".repeat(64), "size": 15, "packed_size": 64}}).to_string().into_bytes()),
            _ => (200, zn(b"SQLite format 3")),
        }).0;
        assert!(matches!(adopt_static_db(&root, &lying, "good"), Err(Error::Corrupt(_))), "sha256 differs");
        const NOT_SQLITE: &[u8] = b"MZ an executable";
        let foreign = server_with(NOT_SQLITE).0;
        assert!(matches!(adopt_static_db(&root, &foreign, "good"), Err(Error::Corrupt(m)) if m.contains("SQLite")));
        assert_eq!(fs::read(root.join(STATIC_DB)).unwrap(), b"SQLite format 3\0the CDN's");
        let _ = fs::remove_dir_all(root);
    }
}
