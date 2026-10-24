//! The account server's static and map databases. A server can play with its
//! own `static.db4` (rows of its own, such as the admin buffs, or another
//! build's), and status effects travel by row (wire id = rowid - 1), so the
//! game data must hold that server's copy byte for byte. The maps likewise:
//! NewNPC, PropInfo and ExitInfo name level instances by Guid, and Bigpoint's
//! builds give every map row a new one (the October 22 build's maps share no
//! Guid with the October 20 build's), so the client's NPCs, props and exits must
//! come from the server's `maps/*.db4`. Its account service publishes both
//! (dso_server `services/content.py`):
//!
//! - `GET /api/content`: `{"version", "static_db": {"sha256", "size", "packed_size"} or null,
//!   "maps": {"sha256", "files", "size", "packed_size"} or null}`;
//! - `POST /api/content/static-db` with `{"session_id"}`: the database packed as
//!   the CDN packs files (`__ZN`, the size, zlib), for a signed-in player;
//! - `POST /api/content/maps` with `{"session_id"}`: every map database in one
//!   bundle ([`read_map_bundle`]).
//!
//! [`adopt_server_data`] compares the server's with the folder's and downloads
//! what differs, checks sizes, sha256s and SQLite headers, writes the files in
//! place and records them in `.dsor/overrides.txt`, so the CDN check keeps them
//! (crate::sync); `.dsor/server-maps.txt` names the map bundle installed. Only
//! these new endpoints are asked: what the official client and its launcher use
//! stays as it is.
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::state::{io, lock, read_overrides, write_overrides, Override};
use crate::{unpack, Error};

/// Where the database is in the game data folder.
pub const STATIC_DB: &str = "export_win32/db/static.db4";
/// Where the maps' databases are.
const MAPS: &str = "export_win32/maps";
/// The map bundle installed: `sha256 \t files \t source`, written last.
const MAPS_MARKER: &str = "server-maps.txt";
const MAP_BUNDLE_MAGIC: &[u8] = b"DSORMAP1";
/// No static database comes near this.
const MAX_SIZE: u64 = 512 << 20;
/// Nor all the maps' databases together (656 MB in October 2026), packed or not.
const MAX_MAPS_SIZE: u64 = 4 << 30;

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

/// What adopting the account server's data came to, part by part: one that
/// fails does not keep the other from its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerData {
    pub static_db: Result<Adoption, Error>,
    pub maps: Result<Adoption, Error>,
}

/// Gives `root` (a game data folder) the static and map databases of the
/// account service at `address` (`host:port`), signed in with `session`.
pub fn adopt_server_data(root: &Path, address: &str, session: &str) -> Result<ServerData, Error> {
    let agent = agent();
    let base = format!("http://{address}");
    let Some(content) = content(&agent, &base)? else {
        return Ok(ServerData { static_db: Ok(Adoption::NotPublished), maps: Ok(Adoption::NotPublished) });
    };
    let _lock = lock(root)?;
    Ok(ServerData {
        static_db: adopt_db(&agent, &base, root, address, session, &content["static_db"]),
        maps: adopt_maps(&agent, &base, root, address, session, &content["maps"]),
    })
}

/// [`adopt_server_data`] for the static database alone.
pub fn adopt_static_db(root: &Path, address: &str, session: &str) -> Result<Adoption, Error> {
    let agent = agent();
    let base = format!("http://{address}");
    let Some(content) = content(&agent, &base)? else { return Ok(Adoption::NotPublished) };
    let _lock = lock(root)?;
    adopt_db(&agent, &base, root, address, session, &content["static_db"])
}

/// `GET /api/content`, or None from a server without the endpoint.
fn content(agent: &ureq::Agent, base: &str) -> Result<Option<serde_json::Value>, Error> {
    let mut response = agent.get(format!("{base}/api/content")).call().map_err(|e| network(base, e))?;
    if response.status() == 404 {
        return Ok(None);
    }
    if response.status() != 200 {
        return Err(Error::Network(format!("{base}/api/content: HTTP {}", response.status())));
    }
    let text = response.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| network(base, e))?;
    serde_json::from_str(&text).map(Some).map_err(|e| Error::Corrupt(format!("{base}/api/content: {e}")))
}

/// The static database, the folder's lock held.
fn adopt_db(agent: &ureq::Agent, base: &str, root: &Path, address: &str, session: &str, published: &serde_json::Value) -> Result<Adoption, Error> {
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
    let dir = root.join(crate::state::DIR);
    if static_db_sha256(root).ok().as_deref() == Some(sha256.as_str()) {
        return Ok(Adoption::Matching { sha256 });
    }
    log::info!("game data: the server's static database ({sha256}) differs from the folder's; downloading it");
    let body = serde_json::json!({ "session_id": session }).to_string();
    let mut response = agent.post(format!("{base}/api/content/static-db")).header("Content-Type", "application/json")
        .send(body).map_err(|e| network(base, e))?;
    if response.status() != 200 {
        let why = response.body_mut().with_config().limit(4 << 10).read_to_string().unwrap_or_default();
        return Err(Error::Network(format!("{base}/api/content/static-db: HTTP {} {}", response.status(), why.trim())));
    }
    let blob = response.body_mut().with_config().limit(packed + 1).read_to_vec().map_err(|e| network(base, e))?;
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

/// The map databases, the folder's lock held.
fn adopt_maps(agent: &ureq::Agent, base: &str, root: &Path, address: &str, session: &str, published: &serde_json::Value) -> Result<Adoption, Error> {
    if published.is_null() {
        return Ok(Adoption::NotPublished);
    }
    let (Some(sha256), Some(files), Some(size), Some(packed)) = (published["sha256"].as_str(), published["files"].as_u64(),
        published["size"].as_u64(), published["packed_size"].as_u64()) else {
        return Err(Error::Corrupt(format!("{base}/api/content: maps without sha256, files, size or packed size")));
    };
    let sha256 = sha256.to_ascii_lowercase();
    if !is_sha256(&sha256) || files == 0 || files > 8192 || size > MAX_MAPS_SIZE || packed > MAX_MAPS_SIZE {
        return Err(Error::Corrupt(format!("{base}/api/content: unlikely maps ({files} files, {size} bytes, {sha256:?})")));
    }
    if server_maps(root).as_deref() == Some(sha256.as_str()) {
        return Ok(Adoption::Matching { sha256 });
    }
    log::info!("game data: the server's maps ({sha256}, {files} files) are not the folder's; downloading them");
    let body = serde_json::json!({ "session_id": session }).to_string();
    let mut response = agent.post(format!("{base}/api/content/maps")).header("Content-Type", "application/json")
        .send(body).map_err(|e| network(base, e))?;
    if response.status() != 200 {
        let why = response.body_mut().with_config().limit(4 << 10).read_to_string().unwrap_or_default();
        return Err(Error::Network(format!("{base}/api/content/maps: HTTP {} {}", response.status(), why.trim())));
    }
    let blob = response.body_mut().with_config().limit(packed + 1).read_to_vec().map_err(|e| network(base, e))?;
    let maps = read_map_bundle(&blob)?;
    let got = map_bundle_sha256(maps.iter().map(|m| (m.name.as_str(), m.sha256.as_str())));
    if got != sha256 || maps.len() as u64 != files {
        return Err(Error::Corrupt(format!("the server's maps arrived as {} files, sha256 {got}; announced {files}, {sha256}", maps.len())));
    }
    let dir = root.join(crate::state::DIR);
    let folder = root.join(MAPS);
    fs::create_dir_all(&folder).map_err(|e| io(&folder, e))?;
    // Unpacked one at a time; each in place in one step.
    let mut overrides = read_overrides(&dir);
    let mut total = 0;
    for map in &maps {
        let data = unpack(map.packed)?;
        if data.len() as u64 != map.size || hex(&Sha256::digest(&data)) != map.sha256 {
            return Err(Error::Corrupt(format!("the server's {} does not match its size or sha256", map.name)));
        }
        if !data.starts_with(b"SQLite format 3\0") {
            return Err(Error::Corrupt(format!("the server's {} is no SQLite database", map.name)));
        }
        let (part, target) = (dir.join("tmp").join(&map.name), folder.join(&map.name));
        fs::write(&part, &data).and_then(|()| fs::rename(&part, &target)).map_err(|e| io(&target, e))?;
        overrides.insert(format!("{MAPS}/{}", map.name), Override { sha256: map.sha256.clone(), size: map.size, source: address.to_string() });
        total += map.size;
    }
    write_overrides(&dir, &overrides)?;
    let marker = dir.join(MAPS_MARKER);
    let part = dir.join("tmp").join(MAPS_MARKER);
    fs::write(&part, format!("{sha256}\t{files}\t{}\n", address.replace(['\t', '\n', '\r'], " ")))
        .and_then(|()| fs::rename(&part, &marker)).map_err(|e| io(&marker, e))?;
    log::info!("game data: installed the maps of {address} ({files} files, {total} bytes, {sha256})");
    Ok(Adoption::Installed { sha256, size: total })
}

/// The sha256 of the server's map bundle the folder holds: named by
/// `.dsor/server-maps.txt` while every map it installed is there at its size.
pub fn server_maps(root: &Path) -> Option<String> {
    let dir = root.join(crate::state::DIR);
    let marker = fs::read_to_string(dir.join(MAPS_MARKER)).ok()?;
    let mut fields = marker.trim_end().split('\t');
    let (sha256, files) = (fields.next()?.to_string(), fields.next()?.parse::<usize>().ok()?);
    let prefix = format!("{MAPS}/");
    let installed: Vec<_> = read_overrides(&dir).into_iter().filter(|(path, _)| path.starts_with(&prefix)).collect();
    let whole = installed.len() >= files
        && installed.iter().all(|(path, kept)| fs::metadata(root.join(path)).is_ok_and(|m| m.len() == kept.size));
    (is_sha256(&sha256) && whole).then_some(sha256)
}

/// One map database of a bundle.
#[derive(Debug)]
pub struct BundledMap<'a> {
    pub name: String,
    pub sha256: String,
    pub size: u64,
    /// `__ZN`-packed, as [`unpack`] reads.
    pub packed: &'a [u8],
}

/// A map bundle: `DSORMAP1`, the count (u32 LE), then per file its name
/// (u16 LE length, UTF-8), sha256 (32 bytes), size and packed length (u32 LE
/// each) and the packed database (dso_server content.py MapDatabasesPublisher).
pub fn read_map_bundle(blob: &[u8]) -> Result<Vec<BundledMap<'_>>, Error> {
    let corrupt = |why: &str| Error::Corrupt(format!("the server's map bundle: {why}"));
    let rest = blob.strip_prefix(MAP_BUNDLE_MAGIC).ok_or_else(|| corrupt("no DSORMAP1 header"))?;
    let take = |at: &mut usize, n: usize| -> Result<&[u8], Error> {
        let part = rest.get(*at..*at + n).ok_or_else(|| corrupt("cut short"))?;
        *at += n;
        Ok(part)
    };
    let mut at = 0;
    let u32_at = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap());
    let count = u32_at(take(&mut at, 4)?) as usize;
    if count > 8192 {
        return Err(corrupt("too many files"));
    }
    let mut maps = Vec::with_capacity(count);
    for _ in 0..count {
        let length = u16::from_le_bytes(take(&mut at, 2)?.try_into().unwrap()) as usize;
        let name = std::str::from_utf8(take(&mut at, length)?).map_err(|_| corrupt("a name is not UTF-8"))?.to_string();
        // A map's file name, nothing that leaves the maps folder.
        let plain = name.strip_suffix(".db4").is_some_and(|stem| !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
        if !plain {
            return Err(corrupt(&format!("{name:?} is not a map database's name")));
        }
        let sha256 = hex(take(&mut at, 32)?);
        let size = u32_at(take(&mut at, 4)?) as u64;
        let length = u32_at(take(&mut at, 4)?) as usize;
        maps.push(BundledMap { name, sha256, size, packed: take(&mut at, length)? });
    }
    if at != rest.len() {
        return Err(corrupt("bytes after the last file"));
    }
    Ok(maps)
}

/// The bundle's identity: sha256 of its sorted `name \t sha256 \n` lines.
pub fn map_bundle_sha256<'a>(entries: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut lines: Vec<String> = entries.map(|(name, sha)| format!("{name}\t{sha}\n")).collect();
    lines.sort();
    hex(&Sha256::digest(lines.concat().as_bytes()))
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The sha256 of the folder's `static.db4`. One the account server installed
/// stands for its recorded sha256 while its size holds; another is read whole
/// (68.8 MB: a fraction of a second).
pub fn static_db_sha256(root: &Path) -> Result<String, Error> {
    let target = root.join(STATIC_DB);
    let kept = read_overrides(&root.join(crate::state::DIR)).remove(STATIC_DB)
        .filter(|kept| fs::metadata(&target).is_ok_and(|m| m.len() == kept.size));
    match kept {
        Some(kept) => Ok(kept.sha256),
        None => file_sha256(&target).map_err(|e| io(&target, e)),
    }
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

    /// A map bundle as dso_server content.py MapDatabasesPublisher writes it.
    fn bundle(maps: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = MAP_BUNDLE_MAGIC.to_vec();
        out.extend((maps.len() as u32).to_le_bytes());
        for (name, data) in maps {
            let packed = zn(data);
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(Sha256::digest(data));
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((packed.len() as u32).to_le_bytes());
            out.extend(packed);
        }
        out
    }

    fn maps_server(maps: &'static [(&'static str, &'static [u8])]) -> (String, Arc<Mutex<Vec<String>>>) {
        let sha = map_bundle_sha256(maps.iter().map(|(n, d)| (*n, Box::leak(hex(&Sha256::digest(d)).into_boxed_str()) as &str)));
        let size: usize = maps.iter().map(|(_, d)| d.len()).sum();
        let blob = bundle(maps);
        serve(move |_, path, body| match path {
            "/api/content" => (200, serde_json::json!({"version": 2, "static_db": null, "maps": {
                "sha256": sha, "files": maps.len(), "size": size, "packed_size": blob.len()}}).to_string().into_bytes()),
            "/api/content/maps" if body.contains("\"good\"") => (200, blob.clone()),
            _ => (401, b"{\"error\": \"sign in\"}".to_vec()),
        })
    }

    const KINGSCITY: &[u8] = b"SQLite format 3\0the server's Kingshill, its Guids";
    const CHAR: &[u8] = b"SQLite format 3\0the server's selection map";

    #[test]
    fn the_servers_maps_are_installed_kept_and_fetched_again_once_lost() {
        let root = folder("maps");
        fs::create_dir_all(root.join(MAPS)).unwrap();
        fs::write(root.join(MAPS).join("a0200_kingscity.db4"), b"SQLite format 3\0the CDN's Kingshill").unwrap();
        let (address, seen) = maps_server(&[("a0200_kingscity.db4", KINGSCITY), ("a0000_char.db4", CHAR)]);
        assert_eq!(server_maps(&root), None);
        let data = adopt_server_data(&root, &address, "good").unwrap();
        assert_eq!(data.static_db, Ok(Adoption::NotPublished));
        assert!(matches!(data.maps, Ok(Adoption::Installed { size, .. }) if size == (KINGSCITY.len() + CHAR.len()) as u64));
        assert_eq!(fs::read(root.join(MAPS).join("a0200_kingscity.db4")).unwrap(), KINGSCITY);
        let installed = server_maps(&root).expect("the marker names the bundle");
        assert!(matches!(adopt_server_data(&root, &address, "good").unwrap().maps, Ok(Adoption::Matching { sha256 }) if sha256 == installed));
        let kept = read_overrides(&root.join(crate::state::DIR));
        assert_eq!(kept["export_win32/maps/a0000_char.db4"].size, CHAR.len() as u64, "the CDN check keeps them");
        // A map lost (or put back by the CDN check) is not the server's bundle any more.
        fs::remove_file(root.join(MAPS).join("a0000_char.db4")).unwrap();
        assert_eq!(server_maps(&root), None);
        assert!(matches!(adopt_server_data(&root, &address, "good").unwrap().maps, Ok(Adoption::Installed { .. })));
        assert_eq!(seen.lock().unwrap().iter().filter(|r| r.starts_with("POST")).count(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_bundle_with_a_foreign_name_or_a_wrong_sha256_writes_nothing() {
        assert!(matches!(read_map_bundle(&bundle(&[("../static.db4", KINGSCITY)])), Err(Error::Corrupt(m)) if m.contains("name")));
        assert!(matches!(read_map_bundle(&bundle(&[("maps/a.db4", KINGSCITY)])), Err(Error::Corrupt(_))));
        assert!(matches!(read_map_bundle(&bundle(&[("a0200_kingscity.map", KINGSCITY)])), Err(Error::Corrupt(_))));
        let cut = bundle(&[("a0200_kingscity.db4", KINGSCITY)]);
        assert!(matches!(read_map_bundle(&cut[..cut.len() - 1]), Err(Error::Corrupt(_))));
        assert_eq!(read_map_bundle(&cut).unwrap()[0].size, KINGSCITY.len() as u64);
        let root = folder("lying-maps");
        let blob = bundle(&[("a0200_kingscity.db4", KINGSCITY)]);
        let lying = serve(move |_, path, _| match path {
            "/api/content" => (200, serde_json::json!({"maps": {"sha256": "0".repeat(64), "files": 1, "size": 10, "packed_size": blob.len()}}).to_string().into_bytes()),
            _ => (200, blob.clone()),
        }).0;
        assert!(matches!(adopt_server_data(&root, &lying, "good").unwrap().maps, Err(Error::Corrupt(_))));
        assert!(!root.join(MAPS).join("a0200_kingscity.db4").exists());
        assert_eq!(server_maps(&root), None);
        let _ = fs::remove_dir_all(root);
    }

    /// Against a real account service (dso_server's): DSOR_TEST_ACCOUNT_SERVICE
    /// (host:port), DSOR_TEST_SESSION (a signed-in session) and
    /// DSOR_TEST_GAME_DATA (a game data folder it may change).
    #[test]
    #[ignore = "needs a running account service, a session and a game data folder"]
    fn a_real_account_service_hands_over_its_data() {
        let var = |name| std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
        let (address, session, folder) = (var("DSOR_TEST_ACCOUNT_SERVICE"), var("DSOR_TEST_SESSION"), var("DSOR_TEST_GAME_DATA"));
        let root = std::path::Path::new(&folder);
        let started = std::time::Instant::now();
        let data = adopt_server_data(root, &address, &session).unwrap();
        println!("adopted in {:.1} s: {data:?}", started.elapsed().as_secs_f64());
        assert!(data.static_db.is_ok() && data.maps.is_ok(), "{data:?}");
        let installed = server_maps(root).expect("the server's maps are installed");
        let again = adopt_server_data(root, &address, &session).unwrap();
        assert_eq!(again.maps, Ok(Adoption::Matching { sha256: installed }), "the second sign-in downloads nothing");
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
