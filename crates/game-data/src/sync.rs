//! Downloading a build into the game data folder and keeping it complete.
//!
//! The folder holds `export_win32/` exactly as the build's tables list it (the
//! bundles and tables themselves are not kept), `__root.txt` naming the build
//! once every file is on disk, and the downloader's state in `.dsor/`
//! (crate::state).
//!
//! A run asks the CDN for its build, then takes the build's files from its
//! tables (cached by md5), or from the manifest when `__root.txt` names that
//! build already, and checks each one on disk against the manifest (md5 and
//! size, one folder listing per folder). Up to [`Options::single_files`]
//! missing files are downloaded one by one. More unpack bundles: those not
//! unpacked completely at their md5 (a fresh folder, an interrupted run, a new
//! build) and those that held a file now missing; single files fill in what
//! remains. Every blob is checked against its md5 before it is used.
//!
//! Speed: bundles stream in over [`Options::downloads`] connections while
//! [`Options::writers`] threads unpack and write the ones already in, a bounded
//! queue between them, so neither the network nor the disk waits for the
//! other; the bundles' sizes (for the bar) are asked for beside the downloads.
//! Folders are made before the first write, files written in place in one go
//! (no temporary file and rename: a file counts only once its manifest line
//! follows it, so one cut short is just missing), and only the members a
//! folder lacks are checked and unpacked.
//!
//! A run cut short resumes where it stopped. Last, the files an older build
//! had and this one dropped are removed, the manifest is rewritten and
//! `__root.txt` written. Without the CDN, the build `__root.txt` names is
//! checked instead.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{mpsc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::format::{logical_path, parse_bundle, parse_toc, unpack, Kind, Md5, TocEntry};
use crate::state::{io, Entry, Journal, State};
use crate::{Error, Progress, Source, Stage};

/// Tries per download.
const ATTEMPTS: u32 = 4;
/// Small requests in flight at once: tables, sizes, single files. Each one
/// waits about 190 ms on the CDN (2026-10-24: a 100 ms round trip to heredur):
/// 32 read 170 tables a second, 64 no more (nor from alicdn-prod-774), so a
/// first listing takes about 15 s; later ones come from the cache.
const REQUESTS: usize = 32;

pub struct Options {
    /// The game data folder: `export_win32/` goes inside.
    pub root: PathBuf,
    /// Bundle downloads in flight at once.
    pub downloads: usize,
    /// Threads unpacking and writing the bundles downloaded.
    pub writers: usize,
    /// The pause before the first retry of a download; it doubles each time.
    pub retry_delay: Duration,
    /// Up to this many missing files are downloaded one by one; more unpack bundles.
    pub single_files: usize,
}

impl Options {
    /// Writers: creating files is what costs on Windows. On the 2026-10-24
    /// workstation's HDD volume, with Defender scanning, a file took 4.5 ms on
    /// one thread; 8 threads made 1,550 files a second, 16 no more (a
    /// temporary file renamed into place: 135 a second at any count).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), downloads: 16, writers: 8, retry_delay: Duration::from_secs(1), single_files: 2000 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub build: Md5,
    pub files: usize,
    /// Files this run wrote.
    pub written: usize,
    /// Bytes this run downloaded, retries not counted.
    pub downloaded: u64,
    /// Checked without the CDN: the build installed last, not checked for updates.
    pub offline: bool,
}

/// Brings the folder to the CDN's build; `progress` follows it and ends
/// [`Stage::Complete`] or [`Stage::Failed`].
pub fn run(options: &Options, source: &dyn Source, progress: &Progress) -> Result<Summary, Error> {
    let started = std::time::Instant::now();
    let result = State::open(&options.root).and_then(|state| Run { options, source, progress, state }.run());
    match &result {
        Ok(s) => log::info!("game data: build {} complete in {} after {:.1} s, {} files ({} written, {} bytes downloaded){}",
            s.build, options.root.display(), started.elapsed().as_secs_f64(), s.files, s.written, s.downloaded,
            if s.offline { ", offline" } else { "" }),
        Err(error) => log::warn!("game data: {error}"),
    }
    progress.finish(&result);
    result
}

struct Run<'a> {
    options: &'a Options,
    source: &'a dyn Source,
    progress: &'a Progress,
    state: State,
}

/// What a build has.
struct Listing {
    /// Its files under `export_win32/`.
    files: BTreeMap<String, Md5>,
    /// Its bundles; `None` when the files came from the manifest.
    bundles: Option<Vec<(String, Md5)>>,
    /// The tables read, to keep cached.
    tables: HashSet<Md5>,
}

impl Run<'_> {
    fn run(mut self) -> Result<Summary, Error> {
        clear(&self.state.tmp())?;
        self.progress.stage(Stage::Connecting, 0);
        let build = match self.source.root_key() {
            Ok(build) => build,
            Err(error @ (Error::Network(_) | Error::NotFound(_))) => return self.offline(error),
            Err(error) => return Err(error),
        };
        self.progress.update(|status| status.build = Some(build));
        let mut listing = if self.state.build == Some(build) { self.installed() } else { self.list(build)? };
        let mut missing = self.check(&listing.files);
        let (mut written, mut downloaded) = (0, 0);
        if !missing.is_empty() {
            if missing.len() > self.options.single_files && listing.bundles.is_none() {
                // The bundles are needed: the same build's tables list what the manifest does.
                listing = self.list(build)?;
                missing.retain(|path| listing.files.contains_key(path));
                missing.extend(listing.files.iter().filter(|(path, md5)| self.state.files.get(*path).is_none_or(|e| e.md5 != **md5)).map(|(p, _)| p.clone()));
            }
            log::info!("game data: {} of {} files to download", missing.len(), listing.files.len());
            // A kept version that is gone gets the tables' version back, not
            // a check that waits for it forever; the next sign-in brings it again.
            let before = self.state.overrides.len();
            self.state.overrides.retain(|path, _| !missing.contains(path));
            if self.state.overrides.len() != before {
                crate::state::write_overrides(&self.state.dir, &self.state.overrides)?;
            }
            self.state.forget_build()?;
            let work = Work::new(&self, &listing.files, missing)?;
            if work.left() > self.options.single_files {
                work.unpack(&self.bundles_to_unpack(&listing, &work.missing()))?;
            }
            work.singles()?;
            (written, downloaded) = work.finish()?;
        }
        self.complete(build, &listing)?;
        Ok(Summary { build, files: listing.files.len(), written, downloaded, offline: false })
    }

    fn offline(self, error: Error) -> Result<Summary, Error> {
        let Some(build) = self.state.build else { return Err(error) };
        log::warn!("game data: {error}; checking build {build} as installed");
        let listing = self.installed();
        let missing = self.check(&listing.files);
        if !missing.is_empty() {
            log::warn!("game data: {} files are missing and cannot be downloaded", missing.len());
            return Err(error);
        }
        Ok(Summary { build, files: listing.files.len(), written: 0, downloaded: 0, offline: true })
    }

    /// The build `__root.txt` names, from the manifest.
    fn installed(&self) -> Listing {
        Listing { files: self.state.files.iter().map(|(path, e)| (path.clone(), e.md5)).collect(), bundles: None, tables: HashSet::new() }
    }

    /// The build's tables: the main one, then every folder's.
    fn list(&self, build: Md5) -> Result<Listing, Error> {
        self.progress.stage(Stage::Listing, 1);
        let mut tables = HashSet::from([build]);
        let main = parse_toc(&unpack(&self.table("__toc", build)?)?);
        self.progress.advance(1);
        let mut pending: Vec<(String, Md5)> = main.into_iter().filter(|e| e.kind == Kind::Folder).map(|e| (e.path, e.md5)).collect();
        let mut seen: HashSet<(String, Md5)> = pending.iter().cloned().collect();
        let mut read: Vec<(String, Vec<TocEntry>)> = Vec::new();
        // The main table names every folder; a folder's own `d` lines are followed too.
        while !pending.is_empty() {
            self.progress.update(|status| status.total += pending.len() as u64);
            let results = Mutex::new(Vec::new());
            let errors = parallel(REQUESTS, &pending, |(folder, md5)| {
                let entries = parse_toc(&unpack(&self.table(&format!("{folder}/__toc"), *md5)?).map_err(|e| context(folder, e))?);
                lock(&results).push((folder.clone(), entries));
                self.progress.advance(1);
                Ok(())
            });
            if let Some(error) = errors.into_iter().next() {
                return Err(error);
            }
            tables.extend(pending.drain(..).map(|(_, md5)| md5));
            for (folder, entries) in results.into_inner().unwrap_or_else(PoisonError::into_inner) {
                for entry in entries.iter().filter(|e| e.kind == Kind::Folder) {
                    let item = (full(&folder, &entry.path), entry.md5);
                    if seen.insert(item.clone()) {
                        pending.push(item);
                    }
                }
                read.push((folder, entries));
            }
        }
        let (mut files, mut bundles, mut elsewhere) = (BTreeMap::new(), Vec::new(), 0);
        for (folder, entries) in &read {
            for entry in entries.iter().filter(|e| e.kind == Kind::File) {
                match logical_path(&full(folder, &entry.path)) {
                    Some(path) if path.starts_with("bundles/") => bundles.push((path, entry.md5)),
                    Some(path) if path.starts_with("export_win32/") => {
                        if let Some(other) = files.insert(path.clone(), entry.md5).filter(|other| *other != entry.md5) {
                            log::warn!("game data: {path} is listed as {other} and {}; keeping the latter", entry.md5);
                        }
                    }
                    _ => elsewhere += 1,
                }
            }
        }
        if elsewhere > 0 {
            log::warn!("game data: {elsewhere} listed files lie outside export_win32/ and bundles/; skipped");
        }
        bundles.sort();
        log::info!("game data: build {build} lists {} files and {} bundles in {} tables", files.len(), bundles.len(), tables.len());
        Ok(Listing { files, bundles: Some(bundles), tables })
    }

    /// A table's stored blob, from the cache or else the CDN (then cached).
    fn table(&self, path: &str, md5: Md5) -> Result<Vec<u8>, Error> {
        let cached = self.state.tocs().join(md5.to_string());
        if let Ok(blob) = fs::read(&cached) {
            if Md5::of(&blob) == md5 {
                return Ok(blob);
            }
        }
        let blob = fetch(self.options, self.source, None, path, md5)?;
        let part = self.state.tmp().join(format!("{md5}.toc"));
        if fs::write(&part, &blob).and_then(|()| fs::rename(&part, &cached)).is_err() {
            log::warn!("game data: could not keep the table {path}");
        }
        Ok(blob)
    }

    /// The listed files not on disk as the manifest says they were written:
    /// one folder listing gives every file's size in that folder.
    fn check(&self, files: &BTreeMap<String, Md5>) -> HashSet<String> {
        self.progress.stage(Stage::Checking, files.len() as u64);
        let mut missing = HashSet::new();
        let mut folders: HashMap<&str, Vec<(&str, &String, u64)>> = HashMap::new();
        for (path, md5) in files {
            // A file kept in another version (crate::server) is expected at its own size.
            let size = match self.state.overrides.get(path) {
                Some(kept) => Some(kept.size),
                None => self.state.files.get(path).filter(|entry| entry.md5 == *md5).map(|entry| entry.size),
            };
            match size {
                Some(size) => {
                    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
                    folders.entry(folder).or_default().push((name, path, size));
                }
                None => {
                    missing.insert(path.clone());
                }
            }
        }
        self.progress.advance(missing.len() as u64);
        let folders: Vec<_> = folders.into_iter().collect();
        let missing = Mutex::new(missing);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(4, 16);
        parallel(threads, &folders, |(folder, expected)| {
            // Windows: names compare as the file system does, ignoring case.
            let key = |name: &str| if cfg!(windows) { name.to_ascii_lowercase() } else { name.to_string() };
            let sizes: HashMap<String, u64> = fs::read_dir(local(&self.state.root, folder)).into_iter().flatten().flatten()
                .filter_map(|entry| Some((key(entry.file_name().to_str()?), entry.metadata().ok().filter(|m| m.is_file())?.len())))
                .collect();
            let gone: Vec<String> = expected.iter().filter(|(name, _, size)| sizes.get(&key(name)) != Some(size)).map(|(_, path, _)| (*path).clone()).collect();
            self.progress.advance(expected.len() as u64);
            lock(&missing).extend(gone);
            Ok(())
        });
        missing.into_inner().unwrap_or_else(PoisonError::into_inner)
    }

    /// Bundles not unpacked completely at their md5, and those that held a file now missing.
    fn bundles_to_unpack(&self, listing: &Listing, missing: &HashSet<String>) -> Vec<(String, Md5)> {
        let held: HashSet<Md5> = missing.iter()
            .filter_map(|path| self.state.files.get(path).filter(|e| listing.files.get(path) == Some(&e.md5))?.bundle)
            .collect();
        let mut bundles: Vec<(String, Md5)> = listing.bundles.iter().flatten()
            .filter(|(_, md5)| !self.state.bundles.contains_key(md5) || held.contains(md5))
            .cloned().collect();
        // The required ones first, as the original client preloads them.
        bundles.sort_by_key(|(path, _)| (!path.starts_with("bundles/required/"), path.clone()));
        bundles
    }

    fn complete(&mut self, build: Md5, listing: &Listing) -> Result<(), Error> {
        self.state.reload();
        // Files an older build had and this one does not: only those written here, as they were written.
        let mut removed = 0;
        for (path, entry) in &self.state.files {
            let file = local(&self.state.root, path);
            if !listing.files.contains_key(path) && fs::metadata(&file).is_ok_and(|m| m.is_file() && m.len() == entry.size) && fs::remove_file(&file).is_ok() {
                removed += 1;
            }
        }
        if removed > 0 {
            log::info!("game data: removed {removed} files build {build} no longer has");
        }
        self.state.write_manifest(listing.files.iter().filter_map(|(path, md5)| {
            self.state.files.get_key_value(path).filter(|(_, entry)| entry.md5 == *md5)
        }))?;
        if let Some(bundles) = &listing.bundles {
            let listed: HashSet<&Md5> = bundles.iter().map(|(_, md5)| md5).collect();
            self.state.bundles.retain(|md5, _| listed.contains(md5));
            self.state.write_bundles()?;
            prune(&self.state.tocs(), |name| Md5::parse(name).is_some_and(|md5| listing.tables.contains(&md5)));
        }
        self.state.complete(build)
    }
}

/// One run's downloads: what is still missing, and where it goes.
struct Work<'a> {
    options: &'a Options,
    source: &'a dyn Source,
    progress: &'a Progress,
    root: &'a Path,
    files: &'a BTreeMap<String, Md5>,
    missing: Mutex<HashSet<String>>,
    journal: Journal,
    written: AtomicUsize,
    downloaded: AtomicU64,
    /// Bundle members the tables do not list at that md5: other versions.
    unlisted: AtomicUsize,
    failures: Mutex<Vec<Error>>,
}

impl<'a> Work<'a> {
    fn new(run: &'a Run, files: &'a BTreeMap<String, Md5>, missing: HashSet<String>) -> Result<Self, Error> {
        // Every folder a missing file goes in, before the first write.
        let folders: Vec<PathBuf> = missing.iter().filter_map(|path| Some(local(&run.state.root, path.rsplit_once('/')?.0)))
            .collect::<HashSet<_>>().into_iter().collect();
        let errors = parallel(REQUESTS, &folders, |folder| fs::create_dir_all(folder).map_err(|e| io(folder, e)));
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        Ok(Self { options: run.options, source: run.source, progress: run.progress, root: &run.state.root,
            files, missing: Mutex::new(missing), journal: Journal::open(&run.state)?, written: AtomicUsize::new(0),
            downloaded: AtomicU64::new(0), unlisted: AtomicUsize::new(0), failures: Mutex::default() })
    }

    fn left(&self) -> usize {
        lock(&self.missing).len()
    }

    fn missing(&self) -> HashSet<String> {
        lock(&self.missing).clone()
    }

    /// Downloads beside unpacking: `downloads` connections feed a queue that
    /// `writers` threads empty.
    fn unpack(&self, bundles: &[(String, Md5)]) -> Result<(), Error> {
        self.progress.stage(Stage::Bundles, bundles.len() as u64);
        log::info!("game data: unpacking {} bundles", bundles.len());
        let (queue, arrivals) = mpsc::sync_channel::<(&(String, Md5), Vec<u8>)>(self.options.writers * 2);
        let arrivals = Mutex::new(arrivals);
        let next = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let errors = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            // The sizes, for the bar; the bar counts bundles until they are in.
            scope.spawn(|| {
                let sizes = Mutex::new(Vec::new());
                parallel(REQUESTS, bundles, |(path, md5)| {
                    lock(&sizes).push(self.source.size(path, *md5).ok());
                    Ok(())
                });
                let sizes = sizes.into_inner().unwrap_or_else(PoisonError::into_inner);
                let known: Vec<u64> = sizes.iter().flatten().copied().collect();
                let sum: u64 = known.iter().sum();
                let total = sum + sum / known.len().max(1) as u64 * (sizes.len() - known.len()) as u64;
                self.progress.update(|status| status.bytes_total = total);
            });
            let (next, stop, errors, arrivals) = (&next, &stop, &errors, &arrivals);
            for _ in 0..self.options.downloads.clamp(1, bundles.len().max(1)) {
                let queue = queue.clone();
                scope.spawn(move || {
                    while !stop.load(Relaxed) {
                        let Some(bundle) = bundles.get(next.fetch_add(1, Relaxed)) else { break };
                        match fetch(self.options, self.source, Some(self.progress), &bundle.0, bundle.1) {
                            Ok(blob) => {
                                self.downloaded.fetch_add(blob.len() as u64, Relaxed);
                                if queue.send((bundle, blob)).is_err() {
                                    break;
                                }
                            }
                            Err(error) => {
                                self.progress.advance(1);
                                lock(&errors).push(error);
                            }
                        }
                    }
                });
            }
            drop(queue);
            for _ in 0..self.options.writers.max(1) {
                scope.spawn(move || loop {
                    let Ok(((path, md5), blob)) = lock(&arrivals).recv() else { break };
                    // After a disk error the queue is emptied unread, so no download waits on it.
                    if stop.load(Relaxed) {
                        continue;
                    }
                    let result = self.bundle(path, *md5, blob);
                    self.progress.advance(1);
                    if let Err(error) = result {
                        if matches!(error, Error::Io(_)) {
                            stop.store(true, Relaxed);
                        }
                        lock(&errors).push(error);
                    }
                });
            }
        });
        self.failed(errors.into_inner().unwrap_or_else(PoisonError::into_inner))
    }

    fn bundle(&self, path: &str, md5: Md5, blob: Vec<u8>) -> Result<(), Error> {
        let raw = unpack(&blob).map_err(|e| context(path, e))?;
        drop(blob);
        for member in parse_bundle(&raw).map_err(|e| context(path, e))? {
            let name = member.name.strip_suffix(".nz").unwrap_or(&member.name);
            // Only the tables' version of a listed path goes in place.
            let Some(file) = logical_path(name).filter(|file| self.files.get(file) == Some(&member.md5)) else {
                self.unlisted.fetch_add(1, Relaxed);
                continue;
            };
            if !lock(&self.missing).contains(&file) {
                continue;
            }
            let data = member.unpack().map_err(|e| context(path, e))?;
            self.write(&file, &data, Entry { md5: member.md5, size: data.len() as u64, bundle: Some(md5) })?;
        }
        self.journal.bundle(md5, path)
    }

    fn singles(&self) -> Result<(), Error> {
        let mut left: Vec<String> = lock(&self.missing).iter().cloned().collect();
        if left.is_empty() {
            return Ok(());
        }
        left.sort();
        self.progress.stage(Stage::Files, left.len() as u64);
        let errors = parallel(REQUESTS, &left, |path| {
            let md5 = self.files[path];
            let result = fetch(self.options, self.source, Some(self.progress), path, md5).and_then(|blob| {
                self.downloaded.fetch_add(blob.len() as u64, Relaxed);
                let data = unpack(&blob).map_err(|e| context(path, e))?;
                self.write(path, &data, Entry { md5, size: data.len() as u64, bundle: None })
            });
            self.progress.advance(1);
            result
        });
        self.failed(errors)
    }

    /// Writes a file in place in one go; its manifest line, after it, makes it count.
    fn write(&self, path: &str, data: &[u8], entry: Entry) -> Result<(), Error> {
        let target = local(self.root, path);
        if let Err(error) = fs::write(&target, data) {
            // A folder made away since the run began.
            let folder = target.parent().filter(|_| error.kind() == std::io::ErrorKind::NotFound).ok_or_else(|| io(&target, error))?;
            fs::create_dir_all(folder).and_then(|()| fs::write(&target, data)).map_err(|e| io(&target, e))?;
        }
        self.journal.file(path, &entry)?;
        lock(&self.missing).remove(path);
        self.written.fetch_add(1, Relaxed);
        Ok(())
    }

    /// A disk error ends the run; a download error leaves its files to the next pass or run.
    fn failed(&self, errors: Vec<Error>) -> Result<(), Error> {
        if let Some(error) = errors.iter().find(|e| matches!(e, Error::Io(_))) {
            return Err(error.clone());
        }
        lock(&self.failures).extend(errors);
        Ok(())
    }

    /// Files written and bytes downloaded, if nothing is missing any more.
    fn finish(self) -> Result<(usize, u64), Error> {
        self.journal.flush()?;
        let unlisted = self.unlisted.into_inner();
        if unlisted > 0 {
            log::info!("game data: {unlisted} bundle members are other versions than the tables list; skipped");
        }
        let missing = self.missing.into_inner().unwrap_or_else(PoisonError::into_inner);
        if let Some(path) = missing.iter().min() {
            let failures = self.failures.into_inner().unwrap_or_else(PoisonError::into_inner);
            let first = failures.first().map_or_else(|| format!("{path} was in no bundle"), ToString::to_string);
            return Err(Error::Incomplete { missing: missing.len(), first });
        }
        Ok((self.written.into_inner(), self.downloaded.into_inner()))
    }
}

/// A stored blob, checked against its md5, retried with a growing pause.
fn fetch(options: &Options, source: &dyn Source, progress: Option<&Progress>, path: &str, md5: Md5) -> Result<Vec<u8>, Error> {
    let mut delay = options.retry_delay;
    let mut attempt = 1;
    loop {
        let mut received = 0;
        let result = source.fetch(path, md5, &mut |n| {
            received += n;
            if let Some(progress) = progress {
                progress.received(n);
            }
        });
        let error = match result {
            Ok(blob) if Md5::of(&blob) == md5 => return Ok(blob),
            Ok(blob) => Error::Corrupt(format!("{path}: the {} bytes received do not match the md5 {md5}", blob.len())),
            Err(error) => error,
        };
        if let Some(progress) = progress {
            progress.unreceived(received);
        }
        if matches!(error, Error::NotFound(_)) || attempt == ATTEMPTS {
            return Err(error);
        }
        log::warn!("game data: {error}; retrying (attempt {} of {ATTEMPTS})", attempt + 1);
        std::thread::sleep(delay);
        delay *= 2;
        attempt += 1;
    }
}

/// Runs `job` over `items` on up to `threads` threads; a disk error stops the
/// rest (every write after it would fail alike). The errors, in no order.
fn parallel<T: Sync>(threads: usize, items: &[T], job: impl Fn(&T) -> Result<(), Error> + Sync) -> Vec<Error> {
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let errors = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                while !stop.load(Relaxed) {
                    let Some(item) = items.get(next.fetch_add(1, Relaxed)) else { break };
                    if let Err(error) = job(item) {
                        if matches!(error, Error::Io(_)) {
                            stop.store(true, Relaxed);
                        }
                        lock(&errors).push(error);
                    }
                }
            });
        }
    });
    errors.into_inner().unwrap_or_else(PoisonError::into_inner)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A table entry's full path: tables list full paths, but a bare name is its folder's.
fn full(folder: &str, entry: &str) -> String {
    if entry.contains('/') { entry.to_string() } else { format!("{folder}/{entry}") }
}

/// A folder-relative `/` path on this system.
fn local(root: &Path, path: &str) -> PathBuf {
    path.split('/').fold(root.to_path_buf(), |at, part| at.join(part))
}

fn context(path: &str, error: Error) -> Error {
    match error {
        Error::Corrupt(what) => Error::Corrupt(format!("{path}: {what}")),
        other => other,
    }
}

/// Leftovers of a run cut short.
fn clear(folder: &Path) -> Result<(), Error> {
    for entry in fs::read_dir(folder).map_err(|e| io(folder, e))?.flatten() {
        let _ = fs::remove_file(entry.path());
    }
    Ok(())
}

fn prune(folder: &Path, keep: impl Fn(&str) -> bool) {
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        if !entry.file_name().to_str().is_some_and(&keep) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::tests::{bundle, zn};

    /// A CDN in memory: blobs by (path, md5), as `<path>.<md5>.nz`.
    #[derive(Default)]
    struct Cdn {
        build: Mutex<Option<Md5>>,
        blobs: Mutex<HashMap<(String, Md5), Vec<u8>>>,
        fetched: Mutex<Vec<String>>,
        /// Served damaged once, then right.
        damaged: Mutex<HashSet<String>>,
        /// Not served at all.
        withheld: Mutex<HashSet<String>>,
    }

    impl Source for Cdn {
        fn root_key(&self) -> Result<Md5, Error> {
            lock(&self.fetched).push("__root.txt".into());
            lock(&self.build).ok_or_else(|| Error::Network("offline".into()))
        }

        fn fetch(&self, path: &str, md5: Md5, received: &mut dyn FnMut(u64)) -> Result<Vec<u8>, Error> {
            lock(&self.fetched).push(path.to_string());
            if lock(&self.withheld).contains(path) {
                return Err(Error::NotFound(path.into()));
            }
            let mut blob = lock(&self.blobs).get(&(path.to_string(), md5)).cloned().ok_or_else(|| Error::NotFound(path.into()))?;
            if lock(&self.damaged).remove(path) {
                blob[0] ^= 1;
            }
            received(blob.len() as u64);
            Ok(blob)
        }

        fn size(&self, path: &str, md5: Md5) -> Result<u64, Error> {
            lock(&self.blobs).get(&(path.to_string(), md5)).map(|b| b.len() as u64).ok_or_else(|| Error::NotFound(path.into()))
        }
    }

    impl Cdn {
        /// Publishes a build of these files, packed into these bundles (member
        /// names may differ from the listed paths, as the database's does), and
        /// makes it the current one.
        fn publish(&self, files: &[(&str, &[u8])], bundles: &[&[(&str, &str)]]) -> Md5 {
            let mut blobs = lock(&self.blobs);
            let mut folders: BTreeMap<String, Vec<(String, Md5)>> = BTreeMap::new();
            let mut stored = HashMap::new();
            for (path, data) in files {
                let blob = zn(data);
                let md5 = Md5::of(&blob);
                blobs.insert((path.to_string(), md5), blob.clone());
                stored.insert(path.to_string(), blob);
                folders.entry(path.rsplit_once('/').unwrap().0.to_string()).or_default().push((path.to_string(), md5));
            }
            for (i, members) in bundles.iter().enumerate() {
                let raw = bundle(&members.iter().map(|(name, path)| (*name, stored[*path].clone())).collect::<Vec<_>>());
                let blob = zn(&raw);
                let path = format!("bundles/optional/bundle{i}.nb");
                let md5 = Md5::of(&blob);
                blobs.insert((path.clone(), md5), blob);
                folders.entry("bundles/optional".into()).or_default().push((path, md5));
            }
            let mut main = String::new();
            for (folder, entries) in folders {
                let text: String = entries.iter().map(|(path, md5)| format!("{path}|f|{md5}\n")).collect();
                let blob = zn(text.as_bytes());
                let md5 = Md5::of(&blob);
                blobs.insert((format!("{folder}/__toc"), md5), blob);
                main += &format!("{folder}|d|{md5}\n");
            }
            let blob = zn(main.as_bytes());
            let key = Md5::of(&blob);
            blobs.insert(("__toc".into(), key), blob);
            *lock(&self.build) = Some(key);
            key
        }

        /// The requests since the last call, tables left out.
        fn requests(&self) -> Vec<String> {
            let mut out: Vec<String> = lock(&self.fetched).drain(..).filter(|p| !p.ends_with("__toc")).collect();
            out.sort();
            out
        }
    }

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("dsor-game-data-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }

        fn read(&self, path: &str) -> Option<Vec<u8>> {
            fs::read(local(&self.0, path)).ok()
        }

        fn sync(&self, cdn: &Cdn, single_files: usize) -> Result<Summary, Error> {
            let options = Options { downloads: 3, writers: 2, retry_delay: Duration::ZERO, single_files, ..Options::new(&self.0) };
            let progress = Progress::default();
            let result = run(&options, cdn, &progress);
            assert_eq!(progress.status().stage, if result.is_ok() { Stage::Complete } else { Stage::Failed });
            result
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const DB: &str = "export_win32/db/static.db4";
    const MAP: &str = "export_win32/maps/a0200_kingscity.map";
    const UI: &str = "export_win32/ui/chatwindow2.bxml";
    const MESH: &str = "export_win32/meshes/t001_hub/arch house_01_s_0.nvx2";

    fn first_build(cdn: &Cdn) -> Md5 {
        cdn.publish(&[(DB, b"SQLite format 3"), (MAP, b"MAP kingscity"), (UI, b"LMXB chat"), (MESH, b"NVX2 arch")],
            &[&[(MAP, MAP), (UI, UI)], &[(MESH, MESH)], &[("global/export_win32/db/static.db4", DB)]])
    }

    #[test]
    fn a_fresh_folder_is_filled_from_the_bundles_with_the_database_in_export_win32() {
        let (folder, cdn) = (Folder::new("fresh"), Cdn::default());
        let build = first_build(&cdn);
        let summary = folder.sync(&cdn, 0).unwrap();
        assert_eq!((summary.build, summary.files, summary.written, summary.offline), (build, 4, 4, false));
        assert_eq!(cdn.requests(), ["__root.txt", "bundles/optional/bundle0.nb", "bundles/optional/bundle1.nb", "bundles/optional/bundle2.nb"]);
        assert_eq!(folder.read(DB).as_deref(), Some(&b"SQLite format 3"[..]));
        assert_eq!(folder.read(MESH).as_deref(), Some(&b"NVX2 arch"[..]));
        assert!(!folder.0.join("global").exists(), "nothing outside export_win32");
        assert_eq!(fs::read_to_string(folder.0.join("__root.txt")).unwrap(), format!("{build}\n"));
    }

    #[test]
    fn a_complete_folder_is_checked_without_downloading_and_repaired_file_by_file() {
        let (folder, cdn) = (Folder::new("repair"), Cdn::default());
        first_build(&cdn);
        folder.sync(&cdn, 0).unwrap();
        cdn.requests();
        let summary = folder.sync(&cdn, 0).unwrap();
        assert_eq!(summary.written, 0);
        assert_eq!(cdn.requests(), ["__root.txt"], "nothing but the build key");
        fs::remove_file(local(&folder.0, UI)).unwrap();
        fs::write(local(&folder.0, MAP), b"MAP kings").unwrap();
        assert_eq!(folder.sync(&cdn, 10).unwrap().written, 2);
        assert_eq!(cdn.requests(), ["__root.txt", MAP, UI]);
        assert_eq!(folder.read(MAP).as_deref(), Some(&b"MAP kingscity"[..]));
    }

    #[test]
    fn many_missing_files_bring_back_the_bundles_that_held_them() {
        let (folder, cdn) = (Folder::new("bundles-again"), Cdn::default());
        first_build(&cdn);
        folder.sync(&cdn, 0).unwrap();
        cdn.requests();
        fs::remove_dir_all(local(&folder.0, "export_win32/meshes")).unwrap();
        folder.sync(&cdn, 0).unwrap();
        assert_eq!(cdn.requests(), ["__root.txt", "bundles/optional/bundle1.nb"]);
        assert!(folder.read(MESH).is_some());
    }

    #[test]
    fn a_new_build_replaces_changed_files_and_drops_removed_ones() {
        let (folder, cdn) = (Folder::new("update"), Cdn::default());
        first_build(&cdn);
        folder.sync(&cdn, 0).unwrap();
        cdn.requests();
        let sequence = "export_win32/sequences/death.pbxml";
        let build = cdn.publish(&[(DB, b"SQLite format 3"), (MAP, b"MAP kingscity 2"), (MESH, b"NVX2 arch"), (sequence, b"KCAP death")],
            &[&[(MAP, MAP), (sequence, sequence)], &[(MESH, MESH)], &[("global/export_win32/db/static.db4", DB)]]);
        let summary = folder.sync(&cdn, 10).unwrap();
        assert_eq!((summary.build, summary.files, summary.written), (build, 4, 2));
        assert_eq!(cdn.requests(), ["__root.txt", MAP, sequence], "the changed and the new file only");
        assert_eq!(folder.read(MAP).as_deref(), Some(&b"MAP kingscity 2"[..]));
        assert_eq!(folder.read(UI), None, "dropped by the new build");
        assert_eq!(fs::read_to_string(folder.0.join("__root.txt")).unwrap(), format!("{build}\n"));
    }

    #[test]
    fn an_interrupted_download_resumes_without_the_bundles_already_unpacked() {
        let (folder, cdn) = (Folder::new("resume"), Cdn::default());
        first_build(&cdn);
        lock(&cdn.withheld).extend(["bundles/optional/bundle1.nb".to_string(), MESH.to_string()]);
        let error = folder.sync(&cdn, 0).unwrap_err();
        assert!(matches!(&error, Error::Incomplete { missing: 1, .. }), "{error:?}");
        assert!(!folder.0.join("__root.txt").exists(), "incomplete");
        assert!(folder.read(DB).is_some());
        cdn.requests();
        lock(&cdn.withheld).clear();
        folder.sync(&cdn, 0).unwrap();
        assert_eq!(cdn.requests(), ["__root.txt", "bundles/optional/bundle1.nb"]);
    }

    #[test]
    fn damaged_downloads_are_retried() {
        let (folder, cdn) = (Folder::new("damaged"), Cdn::default());
        first_build(&cdn);
        lock(&cdn.damaged).insert("bundles/optional/bundle0.nb".into());
        folder.sync(&cdn, 0).unwrap();
        assert_eq!(cdn.requests().iter().filter(|p| p.ends_with("bundle0.nb")).count(), 2);
        assert!(folder.read(UI).is_some());
    }

    #[test]
    fn offline_the_installed_build_is_checked_instead() {
        let (folder, cdn) = (Folder::new("offline"), Cdn::default());
        assert!(matches!(folder.sync(&cdn, 0), Err(Error::Network(_))), "nothing installed, nothing to check");
        let build = first_build(&cdn);
        folder.sync(&cdn, 0).unwrap();
        *lock(&cdn.build) = None;
        let summary = folder.sync(&cdn, 0).unwrap();
        assert_eq!((summary.build, summary.offline), (build, true));
        fs::remove_file(local(&folder.0, DB)).unwrap();
        assert!(matches!(folder.sync(&cdn, 0), Err(Error::Network(_))));
    }

    #[test]
    fn the_servers_database_is_kept_and_a_lost_one_comes_back_from_the_cdn() {
        use crate::state::{read_overrides, write_overrides, Override};
        let (folder, cdn) = (Folder::new("kept"), Cdn::default());
        first_build(&cdn);
        folder.sync(&cdn, 0).unwrap();
        // As crate::server installs it.
        let server = b"SQLite format 3\0with the server's own rows".to_vec();
        fs::write(local(&folder.0, DB), &server).unwrap();
        let kept = HashMap::from([(DB.to_string(), Override { sha256: "ab".repeat(32), size: server.len() as u64, source: "127.0.0.1:8080".into() })]);
        write_overrides(&folder.0.join(".dsor"), &kept).unwrap();
        cdn.requests();
        folder.sync(&cdn, 10).unwrap();
        assert_eq!(cdn.requests(), ["__root.txt"], "the server's copy counts as present");
        assert_eq!(folder.read(DB).unwrap(), server);
        fs::remove_file(local(&folder.0, DB)).unwrap();
        folder.sync(&cdn, 10).unwrap();
        assert_eq!(cdn.requests(), ["__root.txt", DB]);
        assert_eq!(folder.read(DB).as_deref(), Some(&b"SQLite format 3"[..]));
        assert!(read_overrides(&folder.0.join(".dsor")).is_empty(), "the next sign-in brings it again");
    }

    #[test]
    fn a_second_run_on_the_same_folder_is_refused() {
        let (folder, cdn) = (Folder::new("busy"), Cdn::default());
        first_build(&cdn);
        let _first = State::open(&folder.0).unwrap();
        assert_eq!(folder.sync(&cdn, 0), Err(Error::Busy));
    }
}
