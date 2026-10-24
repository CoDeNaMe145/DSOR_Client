//! Converting the game data into the assets the client loads, with the
//! converter a release ships beside the client (docs/release.md): its own
//! Python runs `tools/convert_assets.py` over `export_win32/`.
//!
//! The assets' `conversion.json` says what they were converted from: the
//! converter's version, the build (`__root.txt`) and the static database's
//! sha256, and the account server's map bundle when one is installed
//! (crate::server). [`needed`] compares it with the game data: another
//! converter or build converts everything again (into `<assets>.partial`, put
//! in place once complete); only another database or map bundle (the account
//! server's) reruns the steps that read them.
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::state::{lock, BUILD};
use crate::{server_maps, static_db_sha256, Error, Progress};

/// What the assets were converted from, in the assets folder.
pub const STAMP: &str = "conversion.json";

/// A release's `converter/` folder: `VERSION`, `python/`, `tools/`, `bin/`,
/// `DSO_Godot/`, `level-overrides/`, `vfx-overrides/`.
#[derive(Clone, Debug)]
pub struct Converter {
    pub folder: PathBuf,
    pub python: PathBuf,
    pub version: String,
}

impl Converter {
    /// The converter in `folder`, when it is a complete one.
    pub fn find(folder: &Path) -> Option<Self> {
        let python = if cfg!(windows) { folder.join("python").join("python.exe") } else { folder.join("python").join("bin").join("python3") };
        let version = std::fs::read_to_string(folder.join("VERSION")).ok()?.trim().to_string();
        (!version.is_empty() && python.is_file() && folder.join("tools").join("convert_assets.py").is_file())
            .then(|| Self { folder: folder.into(), python, version })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conversion {
    /// Everything, into a new folder.
    Full,
    /// The steps that read the static or map databases, over the assets as they are.
    Database,
}

/// What `assets` need to match the complete game data in `game_data`, if anything.
pub fn needed(converter: &Converter, game_data: &Path, assets: &Path) -> Option<Conversion> {
    let stamp: serde_json::Value = std::fs::read(assets.join(STAMP)).ok()
        .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let build = std::fs::read_to_string(game_data.join(BUILD)).ok().map(|b| b.trim().to_string());
    if stamp["converter"].as_str() != Some(converter.version.as_str()) || stamp["build"].as_str() != build.as_deref() {
        return Some(Conversion::Full);
    }
    let database = static_db_sha256(game_data).ok();
    let maps = server_maps(game_data);
    (stamp["static_db"].as_str() != database.as_deref() || stamp["maps"].as_str() != maps.as_deref()).then_some(Conversion::Database)
}

/// Converts `game_data` into `assets`, reporting each step to `progress`
/// (Stage::Converting, then Stage::Converted or Stage::Failed). Holds the game
/// data's lock meanwhile: another client waits rather than downloading under it.
pub fn convert(converter: &Converter, conversion: Conversion, game_data: &Path, assets: &Path, progress: &Progress) -> Result<(), Error> {
    let result = run(converter, conversion, game_data, assets, progress);
    match &result {
        Ok(()) => progress.converted(),
        Err(error) => {
            log::warn!("conversion: {error}");
            progress.abandon(&error.summary());
        }
    }
    result
}

fn run(converter: &Converter, conversion: Conversion, game_data: &Path, assets: &Path, progress: &Progress) -> Result<(), Error> {
    let _lock = lock(game_data)?;
    progress.converting(conversion);
    let mut command = Command::new(&converter.python);
    command.arg(converter.folder.join("tools").join("convert_assets.py")).arg(game_data.join("export_win32")).arg(assets)
        .arg(match conversion { Conversion::Full => "--replace", Conversion::Database => "--database-only" })
        .args(["--progress", "--watch-stdin"])
        .current_dir(&converter.folder)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console window beside the game's.
        command.creation_flags(0x0800_0000);
    }
    log::info!("conversion: {conversion:?} with {} ({})", converter.folder.display(), converter.version);
    let mut child = command.spawn().map_err(|e| Error::Conversion(format!("{}: {e}", converter.python.display())))?;
    // Held open while the converter runs: it stops when this closes, which the
    // system does when the client quits however it quits.
    let _stdin = child.stdin.take();
    let errors = child.stderr.take().map(|stderr| std::thread::spawn(move || last_lines(stderr, 8)));
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            match parse(&line) {
                Some((done, total, step)) => progress.conversion_step(done, total, step),
                None => log::info!("conversion: {line}"),
            }
        }
    }
    let status = child.wait().map_err(|e| Error::Conversion(e.to_string()))?;
    let errors = errors.and_then(|t| t.join().ok()).unwrap_or_default();
    for line in &errors {
        log::warn!("conversion: {line}");
    }
    if !status.success() {
        return Err(Error::Conversion(reason(&errors).unwrap_or_else(|| format!("the converter stopped ({status})"))));
    }
    Ok(())
}

/// The line of the converter's last words that says why: its own `error:`,
/// else Python's fatal error (its last line is only `<no Python frame>`).
fn reason(errors: &[String]) -> Option<String> {
    let line = errors.iter().rev().find(|l| l.starts_with("error:"))
        .or_else(|| errors.iter().find(|l| l.starts_with("Fatal Python error")))
        .or_else(|| errors.iter().rev().find(|l| !l.trim().is_empty()))?;
    Some(line.trim_start_matches("error:").trim().to_string())
}

/// `@progress <done> <total> <step>`, the converter's progress lines.
fn parse(line: &str) -> Option<(u64, u64, &str)> {
    let mut parts = line.strip_prefix("@progress ")?.splitn(3, ' ');
    let done = parts.next()?.parse().ok()?;
    let total = parts.next()?.parse().ok()?;
    Some((done, total, parts.next().unwrap_or("").trim()))
}

fn last_lines(stream: impl Read, keep: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        if lines.len() == keep * 4 {
            lines.drain(..keep * 3);
        }
        lines.push(line);
    }
    let from = lines.len().saturating_sub(keep);
    lines.split_off(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("dsor-convert-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn progress_lines_are_read_and_others_left_alone() {
        assert_eq!(parse("@progress 12 120 models"), Some((12, 120, "models")));
        assert_eq!(parse("@progress 0 9 the systems windows"), Some((0, 9, "the systems windows")));
        assert_eq!(parse("@progress 120 120"), Some((120, 120, "")));
        assert_eq!(parse("[convert] models"), None);
        assert_eq!(parse("@progress x 120 models"), None);
    }

    #[test]
    fn a_failure_is_told_by_its_reason_not_its_last_line() {
        let lines = |text: &str| text.lines().map(String::from).collect::<Vec<_>>();
        let failed = lines("logs/post.log tail\nerror: export_ui.py failed (exit 1); log: x");
        assert_eq!(reason(&failed).as_deref(), Some("export_ui.py failed (exit 1); log: x"));
        let crashed = lines("Fatal Python error: _enter_buffered_busy: could not acquire lock\nPython runtime state: finalizing\n\nCurrent thread 0x1 (most recent call first):\n  <no Python frame>");
        assert_eq!(reason(&crashed).as_deref(), Some("Fatal Python error: _enter_buffered_busy: could not acquire lock"));
        assert_eq!(reason(&lines("something else\n\n")).as_deref(), Some("something else"));
        assert_eq!(reason(&[]), None);
    }

    #[test]
    fn a_converter_needs_its_python_script_and_version() {
        let root = folder("find");
        assert!(Converter::find(&root).is_none());
        let python = if cfg!(windows) { root.join("python/python.exe") } else { root.join("python/bin/python3") };
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::create_dir_all(root.join("tools")).unwrap();
        std::fs::write(&python, b"").unwrap();
        std::fs::write(root.join("tools/convert_assets.py"), b"").unwrap();
        assert!(Converter::find(&root).is_none(), "no VERSION");
        std::fs::write(root.join("VERSION"), "v0.1.0-abc\n").unwrap();
        assert_eq!(Converter::find(&root).unwrap().version, "v0.1.0-abc");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_stamp_decides_between_everything_the_tables_and_nothing() {
        let (game_data, assets) = (folder("data"), folder("assets"));
        std::fs::create_dir_all(game_data.join("export_win32/db")).unwrap();
        std::fs::write(game_data.join(BUILD), "6a8b704d041044688ebee52da5b48b55").unwrap();
        std::fs::write(game_data.join("export_win32/db/static.db4"), b"SQLite format 3\0one").unwrap();
        let converter = Converter { folder: PathBuf::new(), python: PathBuf::new(), version: "v1".into() };
        assert_eq!(needed(&converter, &game_data, &assets), Some(Conversion::Full), "never converted");
        let database = static_db_sha256(&game_data).unwrap();
        let stamp = |converter: &str, build: &str, database: &str| std::fs::write(assets.join(STAMP),
            serde_json::json!({"converter": converter, "build": build, "static_db": database}).to_string()).unwrap();
        stamp("v1", "6a8b704d041044688ebee52da5b48b55", &database);
        assert_eq!(needed(&converter, &game_data, &assets), None);
        stamp("v0", "6a8b704d041044688ebee52da5b48b55", &database);
        assert_eq!(needed(&converter, &game_data, &assets), Some(Conversion::Full), "another converter");
        stamp("v1", "f910a2e8000000000000000000000000", &database);
        assert_eq!(needed(&converter, &game_data, &assets), Some(Conversion::Full), "another build");
        stamp("v1", "6a8b704d041044688ebee52da5b48b55", &database);
        std::fs::write(game_data.join("export_win32/db/static.db4"), b"SQLite format 3\0two").unwrap();
        assert_eq!(needed(&converter, &game_data, &assets), Some(Conversion::Database), "the server's database");
        // The server's maps installed (crate::server): their bundle joins the stamp.
        let database = static_db_sha256(&game_data).unwrap();
        stamp("v1", "6a8b704d041044688ebee52da5b48b55", &database);
        assert_eq!(needed(&converter, &game_data, &assets), None);
        std::fs::create_dir_all(game_data.join("export_win32/maps")).unwrap();
        const MAP: &[u8] = b"SQLite format 3\0map";
        std::fs::write(game_data.join("export_win32/maps/a0000_char.db4"), MAP).unwrap();
        std::fs::create_dir_all(game_data.join(".dsor")).unwrap();
        // Installed at its size, as server.rs records it.
        std::fs::write(game_data.join(".dsor/overrides.txt"),
            format!("{}\t{}\t127.0.0.1:8080\texport_win32/maps/a0000_char.db4\n", "c".repeat(64), MAP.len())).unwrap();
        std::fs::write(game_data.join(".dsor/server-maps.txt"), format!("{}\t1\t127.0.0.1:8080\n", "b".repeat(64))).unwrap();
        assert_eq!(server_maps(&game_data), Some("b".repeat(64)));
        assert_eq!(needed(&converter, &game_data, &assets), Some(Conversion::Database), "the server's maps");
        std::fs::write(assets.join(STAMP), serde_json::json!({"converter": "v1", "build": "6a8b704d041044688ebee52da5b48b55",
            "static_db": database, "maps": "b".repeat(64)}).to_string()).unwrap();
        assert_eq!(needed(&converter, &game_data, &assets), None);
        let _ = (std::fs::remove_dir_all(game_data), std::fs::remove_dir_all(assets));
    }
}
