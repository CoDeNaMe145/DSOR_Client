//! The original game client's data, straight from Bigpoint's CDN.
//!
//! The CDN (`https://heredur.drakensang.com/cdndata`, the `-rooturl` the official
//! launcher gives the game) serves one build, the current one; an older build's
//! files are gone once it is replaced. Everything is checked from one key down:
//!
//! - `__root.txt` holds the build key: the md5 of the main table of contents;
//! - every file is at `<path>.<md5>.nz`, md5 being that of the stored blob
//!   (`__ZN` + zlib, [`unpack`]), which its parent table lists. The main table
//!   `__toc.<key>.nz` lists every folder (`path|d|md5`, 2,843 of them); each
//!   folder's table `<folder>/__toc.<md5>.nz` lists its files (`path|f|md5`, full
//!   paths: 164,026 under `export_win32/`). Beside each table,
//!   `__toc_human_readable.txt` is the same listing unpacked (no md5 to check it by).
//! - `bundles/required` and `bundles/optional` list the NB3 bundles that pack
//!   every `export_win32` file ([`parse_bundle`]): 644 bundles, about 2.2 GB,
//!   unpacking to about 4.8 GB. Each file is also on the CDN on its own.
//!
//! [`run`] downloads a build into a folder, unpacks it into `export_win32/`
//! exactly as the tables list it, verifies it and keeps it complete
//! (crate::sync); the account screen runs it, and so does the `dsor-game-data`
//! command. [`adopt_static_db`] then replaces the folder's `static.db4` with
//! the account server's when the two differ (crate::server). A release converts
//! the data into the client's assets with its bundled converter (crate::convert).
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

mod cdn;
mod convert;
mod format;
mod server;
mod state;
mod sync;

pub use cdn::{Cdn, Source, DEFAULT_CDN, USER_AGENT};
pub use format::{logical_path, parse_bundle, parse_toc, unpack, Kind, Md5, Member, TocEntry};
pub use convert::{convert, needed, Conversion, Converter, STAMP};
pub use server::{adopt_server_data, adopt_static_db, map_bundle_sha256, read_map_bundle, server_maps, static_db_sha256, Adoption, BundledMap, ServerData, STATIC_DB};
pub use sync::{run, Options, Summary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The CDN has no such file (HTTP 404).
    NotFound(String),
    /// The CDN cannot be reached, or a transfer broke off.
    Network(String),
    /// A download that does not match its md5, or a malformed container.
    Corrupt(String),
    /// Reading or writing the game data folder.
    Io(String),
    /// Another client is already updating this folder.
    Busy,
    /// Some files could not be downloaded; `first` says why the first did not.
    Incomplete { missing: usize, first: String },
    /// The converter could not start or stopped: why.
    Conversion(String),
}

impl Error {
    /// One line for the account screen; the log has the details (Display).
    pub fn summary(&self) -> String {
        match self {
            Error::NotFound(_) | Error::Corrupt(_) => format!("The download server sent a bad file. Retry later. ({self})"),
            Error::Network(_) => "The download server cannot be reached. Check the connection and retry.".into(),
            Error::Io(_) => format!("The game data cannot be written: {self}"),
            Error::Busy => "Another client is updating the game data; retry when it has finished.".into(),
            Error::Incomplete { missing, .. } => format!("{missing} files could not be downloaded. Retry to fetch them."),
            Error::Conversion(why) => format!("The conversion failed: {why}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::NotFound(url) => write!(f, "{url} is not on the CDN"),
            Error::Network(what) | Error::Corrupt(what) | Error::Io(what) | Error::Conversion(what) => f.write_str(what),
            Error::Busy => f.write_str("another process holds the game data folder"),
            Error::Incomplete { missing, first } => write!(f, "{missing} files are still missing; the first failed with: {first}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Starting,
    /// Asking the CDN for its build.
    Connecting,
    /// Reading the build's tables of contents: `done` of `total`.
    Listing,
    /// Checking the files on disk: `done` of `total`.
    Checking,
    /// Downloading bundles: `bytes` of `bytes_total`, `done` of `total` bundles.
    Bundles,
    /// Downloading single files: `done` of `total`.
    Files,
    /// Every file of `build` is on disk.
    Complete,
    /// Converting the files into the client's assets (crate::convert): `step`,
    /// `done` of `total` (the steps' weights).
    Converting,
    /// Converted: the client starts again on the new assets.
    Converted,
    /// Stopped: `message` says why.
    Failed,
}

/// A snapshot of a run, for the account screen and the command line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub stage: Stage,
    pub done: u64,
    pub total: u64,
    pub bytes: u64,
    pub bytes_total: u64,
    /// Bytes a second since the stage began.
    pub rate: f64,
    /// The build being installed or installed.
    pub build: Option<Md5>,
    /// The build's file count, once complete.
    pub files: u64,
    /// Complete without reaching the CDN: the build installed last, not checked for updates.
    pub offline: bool,
    /// Why it failed.
    pub message: Option<String>,
    /// Converting: everything or the database's steps, and the step running.
    pub conversion: Option<Conversion>,
    pub step: Option<String>,
    /// Seconds since the stage began.
    pub elapsed: f64,
}

impl Status {
    /// How far the current stage is, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        let part = |done: u64, total: u64| if total == 0 { 0.0 } else { (done as f64 / total as f64).min(1.0) as f32 };
        match self.stage {
            Stage::Complete | Stage::Converted => 1.0,
            Stage::Bundles if self.bytes_total > 0 => part(self.bytes, self.bytes_total),
            _ => part(self.done, self.total),
        }
    }

    pub fn headline(&self) -> &'static str {
        match self.stage {
            Stage::Starting | Stage::Connecting => "Game data",
            Stage::Listing => "Checking for updates",
            Stage::Checking => "Checking the game data",
            Stage::Bundles => "Downloading the game data",
            Stage::Files => "Downloading missing files",
            Stage::Complete => "Game data ready",
            Stage::Converting if self.conversion == Some(Conversion::Database) => "Updating the game tables",
            Stage::Converting => "Converting the game data",
            Stage::Converted => "Game data converted",
            Stage::Failed => "Game data incomplete",
        }
    }

    pub fn detail(&self) -> String {
        match self.stage {
            Stage::Starting | Stage::Connecting => "Contacting the download server...".into(),
            Stage::Listing => format!("Reading the file lists: {} of {}", count(self.done), count(self.total)),
            Stage::Checking => format!("{} of {} files", count(self.done), count(self.total)),
            Stage::Bundles if self.bytes == 0 => "Preparing the download...".into(),
            Stage::Bundles => {
                // The total comes from the bundles' sizes, asked for beside the downloads.
                let mut line = if self.bytes_total > 0 {
                    format!("{} of {}", size(self.bytes), size(self.bytes_total))
                } else {
                    format!("{}, {} of {} bundles", size(self.bytes), count(self.done), count(self.total))
                };
                if self.rate > 0.0 {
                    line += &format!(" at {}/s", size(self.rate as u64));
                    if self.bytes_total > 0 {
                        let left = self.bytes_total.saturating_sub(self.bytes) as f64 / self.rate;
                        line += &format!(", {}", remaining(left));
                    }
                }
                line
            }
            Stage::Files => format!("{} of {} files, {}", count(self.done), count(self.total), size(self.bytes)),
            Stage::Complete => {
                let build = self.build.map(|b| b.to_string()[..8].to_string()).unwrap_or_default();
                let note = if self.offline { " (offline: not checked for updates)" } else { "" };
                format!("{} files, build {build}{note}", count(self.files))
            }
            Stage::Converting => {
                let percent = (self.fraction() * 100.0).round();
                let so_far = match (self.elapsed / 60.0) as u64 {
                    0 => "under a minute so far".to_string(),
                    minutes => format!("{minutes} min so far"),
                };
                match self.step.as_deref().filter(|s| !s.is_empty()) {
                    Some(step) => format!("{step} ({percent}%), {so_far}"),
                    None => "Starting the converter...".into(),
                }
            }
            Stage::Converted => "Starting again with the new assets...".into(),
            Stage::Failed => self.message.clone().unwrap_or_default(),
        }
    }
}

/// 164026 as "164,026".
fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Bytes as "2.21 GB", "512.4 MB", "34 KB".
fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 { format!("{:.2} GB", b / 1e9) } else if b >= 1e6 { format!("{:.1} MB", b / 1e6) } else { format!("{:.0} KB", b / 1e3) }
}

fn remaining(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match minutes {
        0 => "less than a minute left".into(),
        1..=59 => format!("about {minutes} min left"),
        _ => format!("about {} h {:02} min left", minutes / 60, minutes % 60),
    }
}

/// A run's state, shared between its threads and whoever shows it. Readers
/// compare [`Progress::revision`] with the one they showed last and take a
/// [`Progress::status`] only when it moved.
#[derive(Default)]
pub struct Progress {
    inner: Mutex<Inner>,
    done: AtomicU64,
    bytes: AtomicU64,
    revision: AtomicU64,
}

#[derive(Default)]
struct Inner {
    status: Status,
    started: Option<Instant>,
}

impl Progress {
    /// Moves on with every change.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn status(&self) -> Status {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let mut status = inner.status.clone();
        status.done = self.done.load(Ordering::Relaxed);
        status.bytes = self.bytes.load(Ordering::Relaxed);
        let seconds = inner.started.map_or(0.0, |t| t.elapsed().as_secs_f64());
        status.rate = if seconds > 2.0 { status.bytes as f64 / seconds } else { 0.0 };
        status.elapsed = seconds;
        status
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut Status)) {
        change(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner).status);
        self.bump();
    }

    /// A new stage, its counters from zero.
    pub(crate) fn stage(&self, stage: Stage, total: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        self.done.store(0, Ordering::Relaxed);
        self.bytes.store(0, Ordering::Relaxed);
        inner.started = Some(Instant::now());
        let status = &mut inner.status;
        (status.stage, status.total, status.bytes_total, status.message) = (stage, total, 0, None);
        drop(inner);
        self.bump();
    }

    pub(crate) fn advance(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
        self.bump();
    }

    pub(crate) fn received(&self, n: u64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
        self.bump();
    }

    /// A download that failed: its bytes no longer count. (Never below zero:
    /// the bytes taken back are that download's, and stages do not overlap.)
    pub(crate) fn unreceived(&self, n: u64) {
        self.bytes.fetch_sub(n, Ordering::Relaxed);
        self.bump();
    }

    /// A conversion begins.
    pub(crate) fn converting(&self, conversion: Conversion) {
        self.stage(Stage::Converting, 0);
        self.update(|status| (status.conversion, status.step) = (Some(conversion), None));
    }

    /// The converter moved on to `step`; `done` of `total` are the steps' weights.
    pub(crate) fn conversion_step(&self, done: u64, total: u64, step: &str) {
        self.done.store(done, Ordering::Relaxed);
        self.update(|status| (status.total, status.step) = (total, Some(step.to_string())));
    }

    pub(crate) fn converted(&self) {
        self.update(|status| (status.stage, status.message) = (Stage::Converted, None));
    }

    /// Ends a run that could not report its own end (its thread panicked).
    pub fn abandon(&self, message: &str) {
        self.update(|status| (status.stage, status.message) = (Stage::Failed, Some(message.into())));
    }

    pub(crate) fn finish(&self, result: &Result<Summary, Error>) {
        self.update(|status| match result {
            Ok(summary) => {
                (status.stage, status.build, status.files, status.offline, status.message) =
                    (Stage::Complete, Some(summary.build), summary.files as u64, summary.offline, None);
            }
            Err(error) => (status.stage, status.message) = (Stage::Failed, Some(error.summary())),
        });
    }

    fn bump(&self) {
        self.revision.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_read_well_at_every_stage() {
        let mut status = Status { stage: Stage::Bundles, bytes: 1_310_000_000, bytes_total: 2_207_500_000, rate: 8_400_000.0, ..Status::default() };
        assert_eq!(status.detail(), "1.31 GB of 2.21 GB at 8.4 MB/s, about 2 min left");
        assert!((status.fraction() - 0.5934).abs() < 0.001);
        // Before the sizes are in, the bundles count.
        status = Status { stage: Stage::Bundles, done: 10, total: 644, bytes: 50_000_000, rate: 9_000_000.0, ..Status::default() };
        assert_eq!(status.detail(), "50.0 MB, 10 of 644 bundles at 9.0 MB/s");
        assert!((status.fraction() - 10.0 / 644.0).abs() < 0.001);
        status = Status { stage: Stage::Checking, done: 52_000, total: 164_026, ..Status::default() };
        assert_eq!(status.detail(), "52,000 of 164,026 files");
        status = Status { stage: Stage::Complete, files: 164_026, build: Md5::parse("6a8b704d041044688ebee52da5b48b55"), offline: true, ..Status::default() };
        assert_eq!(status.detail(), "164,026 files, build 6a8b704d (offline: not checked for updates)");
        assert_eq!(status.fraction(), 1.0);
        status = Status { stage: Stage::Converting, done: 30, total: 120, step: Some("models".into()), elapsed: 750.0, conversion: Some(Conversion::Full), ..Status::default() };
        assert_eq!((status.headline(), status.detail().as_str()), ("Converting the game data", "models (25%), 12 min so far"));
        status = Status { stage: Stage::Converting, conversion: Some(Conversion::Database), ..Status::default() };
        assert_eq!((status.headline(), status.detail().as_str()), ("Updating the game tables", "Starting the converter..."));
        assert_eq!(remaining(7_500.0), "about 2 h 05 min left");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1,000");
    }

    #[test]
    fn every_change_moves_the_revision() {
        let progress = Progress::default();
        let start = progress.revision();
        progress.stage(Stage::Bundles, 3);
        progress.received(10);
        progress.advance(1);
        progress.unreceived(10);
        assert_eq!(progress.revision(), start + 4);
        let status = progress.status();
        assert_eq!((status.stage, status.done, status.total, status.bytes), (Stage::Bundles, 1, 3, 0));
    }
}
