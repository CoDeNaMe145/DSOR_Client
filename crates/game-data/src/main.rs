//! `dsor-game-data [<folder>] [--cdn <url>] [--downloads <n>] [--writers <n>]`:
//! downloads the original client's data from Bigpoint's CDN into `<folder>`
//! (default `game-data`), or brings it up to date and checks it, as the
//! account screen does. Exits 0 once every file is on disk.
use std::sync::Arc;
use std::time::Duration;

use dsor_game_data::{run, Cdn, Options, Progress, DEFAULT_CDN};

struct Log;

impl log::Log for Log {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info && metadata.target().starts_with("dsor_game_data")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }

    fn flush(&self) {}
}

fn main() {
    let mut options = Options::new("game-data");
    let mut cdn = DEFAULT_CDN.to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--downloads" => options.downloads = number(&mut args, &arg),
            "--writers" => options.writers = number(&mut args, &arg),
            "--cdn" => cdn = args.next().unwrap_or_else(|| usage("--cdn needs a URL")),
            "-h" | "--help" => usage(""),
            flag if flag.starts_with('-') => usage(&format!("unknown option {flag}")),
            folder => options.root = folder.into(),
        }
    }
    let _ = log::set_logger(&Log).map(|()| log::set_max_level(log::LevelFilter::Info));
    let progress = Arc::new(Progress::default());
    let worker = {
        let progress = progress.clone();
        std::thread::spawn(move || run(&options, &Cdn::new(&cdn), &progress))
    };
    let mut shown = String::new();
    while !worker.is_finished() {
        std::thread::sleep(Duration::from_millis(250));
        let status = progress.status();
        let line = format!("{}: {}", status.headline(), status.detail());
        if line != shown {
            eprintln!("{line}");
            shown = line;
        }
        // Once a second at most while bytes move.
        std::thread::sleep(Duration::from_millis(750));
    }
    match worker.join() {
        Ok(Ok(summary)) => println!("complete: build {}, {} files, {} written, {:.2} GB downloaded{}",
            summary.build, summary.files, summary.written, summary.downloaded as f64 / 1e9,
            if summary.offline { " (offline: not checked for updates)" } else { "" }),
        Ok(Err(error)) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
        Err(_) => std::process::exit(2),
    }
}

fn number(args: &mut impl Iterator<Item = String>, option: &str) -> usize {
    args.next().and_then(|n| n.parse().ok()).filter(|n| *n > 0).unwrap_or_else(|| usage(&format!("{option} needs a count")))
}

fn usage(problem: &str) -> ! {
    if !problem.is_empty() {
        eprintln!("{problem}");
    }
    eprintln!("usage: dsor-game-data [<folder>] [--cdn <url>] [--downloads <n>] [--writers <n>]");
    std::process::exit(2)
}
