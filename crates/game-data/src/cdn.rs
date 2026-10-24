//! Bigpoint's CDN over HTTPS.
use std::io::Read;
use std::time::Duration;

use crate::{Error, Md5};

/// The CDN the official launcher points the game at (`-rooturl httpnz://.../cdndata`).
pub const DEFAULT_CDN: &str = "https://heredur.drakensang.com/cdndata";

/// The official thin client's user agent, presented with every request.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/538.1 (KHTML, like Gecko) DROThinClient/1.0 Safari/538.1";

/// No stored blob comes near this: the largest bundle is about 8 MB.
const MAX_BLOB: u64 = 1 << 30;

/// Where builds come from: the CDN, or a test's files.
pub trait Source: Sync {
    /// The current build: the md5 of its main table of contents (`__root.txt`).
    fn root_key(&self) -> Result<Md5, Error>;
    /// The stored blob of `path` at `md5`, as the CDN has it; `received` hears of
    /// every chunk as it arrives. The caller checks the md5.
    fn fetch(&self, path: &str, md5: Md5, received: &mut dyn FnMut(u64)) -> Result<Vec<u8>, Error>;
    /// That blob's size, without downloading it.
    fn size(&self, path: &str, md5: Md5) -> Result<u64, Error>;
}

pub struct Cdn {
    base: String,
    agent: ureq::Agent,
}

impl Cdn {
    pub fn new(base: &str) -> Self {
        let config = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            // A transfer stalled for a minute fails and is retried (crate::sync).
            .timeout_per_read(Some(Duration::from_secs(60)))
            // Every connection in flight stays open for the next request. ureq keeps
            // 10 (3 a host) by default: with 32 tables in flight, 22 did a TLS
            // handshake for each one (150 tables a second).
            .max_idle_connections(64)
            .max_idle_connections_per_host(64)
            .build();
        Self { base: base.trim_end_matches('/').to_string(), agent: ureq::Agent::new_with_config(config) }
    }

    /// `<base>/<path>.<md5>.nz`: every file, table and bundle, md5 being the stored blob's.
    pub fn url(&self, path: &str, md5: Md5) -> String {
        format!("{}/{}.{md5}.nz", self.base, encode(path))
    }
}

impl Source for Cdn {
    fn root_key(&self) -> Result<Md5, Error> {
        let url = format!("{}/__root.txt", self.base);
        let text = self.agent.get(&url).call().map_err(|e| network(&url, e))?
            .into_body().into_with_config().limit(4096).read_to_vec().map_err(|e| network(&url, e))?;
        let text = String::from_utf8_lossy(&text);
        text.trim_start_matches('\u{feff}').lines().next().and_then(|line| Md5::parse(line.trim()))
            .ok_or_else(|| Error::Corrupt(format!("{url} holds no build key")))
    }

    fn fetch(&self, path: &str, md5: Md5, received: &mut dyn FnMut(u64)) -> Result<Vec<u8>, Error> {
        let url = self.url(path, md5);
        let response = self.agent.get(&url).call().map_err(|e| network(&url, e))?;
        let length = response.body().content_length();
        let mut reader = response.into_body().into_with_config().limit(MAX_BLOB).reader();
        let mut blob = Vec::with_capacity(length.unwrap_or(0).min(64 << 20) as usize);
        let mut chunk = vec![0; 64 << 10];
        loop {
            let n = reader.read(&mut chunk).map_err(|e| Error::Network(format!("{url}: {e}")))?;
            if n == 0 {
                break;
            }
            blob.extend_from_slice(&chunk[..n]);
            received(n as u64);
        }
        Ok(blob)
    }

    fn size(&self, path: &str, md5: Md5) -> Result<u64, Error> {
        let url = self.url(path, md5);
        let response = self.agent.head(&url).call().map_err(|e| network(&url, e))?;
        response.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse().ok())
            .ok_or_else(|| Error::Network(format!("{url}: no size")))
    }
}

fn network(url: &str, error: ureq::Error) -> Error {
    match error {
        ureq::Error::StatusCode(404) => Error::NotFound(url.to_string()),
        ureq::Error::StatusCode(code) => Error::Network(format!("{url}: HTTP {code}")),
        other => Error::Network(format!("{url}: {other}")),
    }
}

/// A path as a URL path; 13 file names have spaces.
fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/!$&'()*+,;=:@".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_name_the_stored_blob_and_escape_spaces() {
        let cdn = Cdn::new("https://heredur.drakensang.com/cdndata/");
        let md5 = Md5::parse("222927579e50d7b470664938927c651c").unwrap();
        assert_eq!(cdn.url("export_win32/db/static.db4", md5),
            "https://heredur.drakensang.com/cdndata/export_win32/db/static.db4.222927579e50d7b470664938927c651c.nz");
        assert_eq!(cdn.url("__toc", md5), "https://heredur.drakensang.com/cdndata/__toc.222927579e50d7b470664938927c651c.nz");
        assert_eq!(encode("export_win32/textures/a b_(Head)%.dds"), "export_win32/textures/a%20b_(Head)%25.dds");
    }
}
