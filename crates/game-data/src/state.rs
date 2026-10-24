//! The downloader's own files in `<root>/.dsor/`, and `<root>/__root.txt`.
//!
//! - `manifest.txt`: a line per file written, `md5 \t size \t bundle \t path`,
//!   bundle being the md5 of the bundle it came from or `-`; the last line of a
//!   path counts. A file is present when its line has the tables' md5 and its
//!   size matches the disk.
//! - `bundles.txt`: the bundles unpacked completely, `md5 \t path`.
//! - `tocs/<md5>`: the tables of contents read (an md5 never names another table).
//! - `overrides.txt`: files kept in another version than the tables' (the
//!   account server's static database, crate::server): `sha256 \t size \t
//!   source \t path`. The check expects their size; downloading the tables'
//!   version again drops the line.
//! - `tmp/`: files being written, renamed into place once complete.
//! - `lock`: held by the run using the folder.
//!
//! Lines are appended as work completes and count only once their newline is
//! written, so a run cut short leaves a usable state.
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::{logical_path, Error, Md5};

pub(crate) const DIR: &str = ".dsor";
/// The build key, written once every file of the build is on disk, as the
/// official launcher's `__root.txt` names the build its cache holds.
pub(crate) const BUILD: &str = "__root.txt";

/// What the manifest knows of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub md5: Md5,
    pub size: u64,
    /// The bundle it was unpacked from.
    pub bundle: Option<Md5>,
}

/// A file kept in another version than the tables list (`overrides.txt`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Override {
    /// Of the file as written: hex sha256 and size.
    pub sha256: String,
    pub size: u64,
    /// Who it came from: the account server's address.
    pub source: String,
}

pub(crate) struct State {
    pub root: PathBuf,
    pub dir: PathBuf,
    /// `__root.txt`: the build found complete last.
    pub build: Option<Md5>,
    pub files: HashMap<String, Entry>,
    pub bundles: HashMap<Md5, String>,
    pub overrides: HashMap<String, Override>,
    _lock: File,
}

/// Creates the folder's state directories and takes its lock, which the
/// system releases when the process ends.
pub(crate) fn lock(root: &Path) -> Result<File, Error> {
    let dir = root.join(DIR);
    for folder in [dir.join("tocs"), dir.join("tmp")] {
        fs::create_dir_all(&folder).map_err(|e| io(&folder, e))?;
    }
    let path = dir.join("lock");
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(&path).map_err(|e| io(&path, e))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(fs::TryLockError::WouldBlock) => Err(Error::Busy),
        Err(fs::TryLockError::Error(e)) => Err(io(&path, e)),
    }
}

pub(crate) fn read_overrides(dir: &Path) -> HashMap<String, Override> {
    let text = fs::read_to_string(dir.join("overrides.txt")).unwrap_or_default();
    records(&text, 4)
        .filter(|f| f[3].starts_with("export_win32/") && logical_path(f[3]).as_deref() == Some(f[3]))
        .filter_map(|f| Some((f[3].to_string(), Override { sha256: f[0].to_string(), size: f[1].parse().ok()?, source: f[2].to_string() })))
        .collect()
}

pub(crate) fn write_overrides(dir: &Path, overrides: &HashMap<String, Override>) -> Result<(), Error> {
    let mut lines: Vec<String> = overrides.iter()
        .map(|(path, o)| format!("{}\t{}\t{}\t{path}\n", o.sha256, o.size, o.source.replace(['\t', '\n', '\r'], " ")))
        .collect();
    lines.sort();
    let (part, path) = (dir.join("tmp").join("overrides.txt"), dir.join("overrides.txt"));
    fs::write(&part, lines.concat()).and_then(|()| fs::rename(&part, &path)).map_err(|e| io(&path, e))
}

impl State {
    /// Opens the folder's state, creating it, and takes the folder's lock.
    pub fn open(root: &Path) -> Result<Self, Error> {
        let lock = lock(root)?;
        let dir = root.join(DIR);
        let build = fs::read_to_string(root.join(BUILD)).ok().and_then(|text| Md5::parse(text.trim()));
        let mut state = Self { root: root.to_path_buf(), dir, build, files: HashMap::new(), bundles: HashMap::new(),
            overrides: HashMap::new(), _lock: lock };
        state.reload();
        Ok(state)
    }

    pub fn tocs(&self) -> PathBuf {
        self.dir.join("tocs")
    }

    pub fn tmp(&self) -> PathBuf {
        self.dir.join("tmp")
    }

    /// The manifest and the bundle list as they are on disk.
    pub fn reload(&mut self) {
        self.files = HashMap::new();
        let manifest = fs::read_to_string(self.dir.join("manifest.txt")).unwrap_or_default();
        for fields in records(&manifest, 4) {
            let (Some(md5), Ok(size)) = (Md5::parse(fields[0]), fields[1].parse()) else { continue };
            let bundle = match fields[2] {
                "-" => None,
                other => match Md5::parse(other) { Some(md5) => Some(md5), None => continue },
            };
            // A line can only name a file the downloader may write.
            let path = fields[3];
            if path.starts_with("export_win32/") && logical_path(path).as_deref() == Some(path) {
                self.files.insert(path.to_string(), Entry { md5, size, bundle });
            }
        }
        let bundles = fs::read_to_string(self.dir.join("bundles.txt")).unwrap_or_default();
        self.bundles = records(&bundles, 2).filter_map(|f| Some((Md5::parse(f[0])?, f[1].to_string()))).collect();
        self.overrides = read_overrides(&self.dir);
    }

    /// The folder holds no complete build any more, until [`State::complete`].
    pub fn forget_build(&mut self) -> Result<(), Error> {
        self.build = None;
        let path = self.root.join(BUILD);
        match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(&path, e)),
            _ => Ok(()),
        }
    }

    /// Rewrites the manifest with these files only.
    pub fn write_manifest<'a>(&self, files: impl Iterator<Item = (&'a String, &'a Entry)>) -> Result<(), Error> {
        let mut text = String::new();
        for (path, entry) in files {
            text += &line(path, entry);
        }
        self.replace("manifest.txt", text.as_bytes())
    }

    pub fn write_bundles(&self) -> Result<(), Error> {
        let mut bundles: Vec<_> = self.bundles.iter().collect();
        bundles.sort_by(|a, b| a.1.cmp(b.1));
        let text: String = bundles.into_iter().map(|(md5, path)| format!("{md5}\t{path}\n")).collect();
        self.replace("bundles.txt", text.as_bytes())
    }

    /// Every file of `build` is on disk.
    pub fn complete(&mut self, build: Md5) -> Result<(), Error> {
        let part = self.tmp().join(BUILD);
        let path = self.root.join(BUILD);
        fs::write(&part, format!("{build}\n")).and_then(|()| fs::rename(&part, &path)).map_err(|e| io(&path, e))?;
        self.build = Some(build);
        Ok(())
    }

    fn replace(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        let part = self.tmp().join(name);
        let path = self.dir.join(name);
        fs::write(&part, data).and_then(|()| fs::rename(&part, &path)).map_err(|e| io(&path, e))
    }
}

/// A state file's complete lines, split into `fields` tab-separated fields (the
/// last may hold anything but a newline).
fn records(text: &str, fields: usize) -> impl Iterator<Item = Vec<&str>> {
    text.split_inclusive('\n')
        .filter_map(|line| line.strip_suffix('\n'))
        .map(move |line| line.trim_end_matches('\r').splitn(fields, '\t').collect::<Vec<_>>())
        .filter(move |parts| parts.len() == fields)
}

fn line(path: &str, entry: &Entry) -> String {
    let bundle = entry.bundle.map_or_else(|| "-".to_string(), |b| b.to_string());
    format!("{}\t{}\t{bundle}\t{path}\n", entry.md5, entry.size)
}

/// Appends to the manifest and the bundle list as files and bundles complete.
pub(crate) struct Journal {
    manifest: Mutex<BufWriter<File>>,
    bundles: Mutex<File>,
}

impl Journal {
    pub fn open(state: &State) -> Result<Self, Error> {
        let open = |name: &str| {
            let path = state.dir.join(name);
            let mut file = OpenOptions::new().create(true).read(true).append(true).open(&path).map_err(|e| io(&path, e))?;
            // A run cut short mid-line: end that line, so it stays the only one lost.
            let mut last = [0];
            if file.seek(SeekFrom::End(-1)).is_ok() && file.read_exact(&mut last).is_ok() && last[0] != b'\n' {
                file.write_all(b"\n").map_err(|e| io(&path, e))?;
            }
            Ok::<_, Error>(file)
        };
        Ok(Self { manifest: Mutex::new(BufWriter::new(open("manifest.txt")?)), bundles: Mutex::new(open("bundles.txt")?) })
    }

    pub fn file(&self, path: &str, entry: &Entry) -> Result<(), Error> {
        let mut manifest = self.manifest.lock().unwrap_or_else(PoisonError::into_inner);
        manifest.write_all(line(path, entry).as_bytes()).map_err(|e| Error::Io(format!("manifest: {e}")))
    }

    /// A bundle unpacked completely; its members' lines reach the disk first.
    pub fn bundle(&self, md5: Md5, path: &str) -> Result<(), Error> {
        self.flush()?;
        let mut bundles = self.bundles.lock().unwrap_or_else(PoisonError::into_inner);
        bundles.write_all(format!("{md5}\t{path}\n").as_bytes()).map_err(|e| Error::Io(format!("bundle list: {e}")))
    }

    pub fn flush(&self) -> Result<(), Error> {
        self.manifest.lock().unwrap_or_else(PoisonError::into_inner).flush().map_err(|e| Error::Io(format!("manifest: {e}")))
    }
}

pub(crate) fn io(path: &Path, error: std::io::Error) -> Error {
    Error::Io(format!("{}: {error}", path.display()))
}
