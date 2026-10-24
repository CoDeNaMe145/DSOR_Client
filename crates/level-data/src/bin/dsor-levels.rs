//! Headless composition entry point used by asset rebuilds and CI.
use dsor_level_data::*;
use serde_json::json;
use std::path::PathBuf;
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
/// A release's `converter/bin` (`converter/VERSION` beside it) or any folder
/// outside the workspace it was built in: the executable's folder, the
/// conversion then passing every path (`--overrides`, `--cache`, ...). Run
/// from inside its workspace (`target/...`): the workspace.
fn root() -> PathBuf {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let folder = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from));
    match (std::fs::canonicalize(&workspace).ok(), folder) {
        (_, Some(folder)) if folder.parent().is_some_and(|c| c.join("VERSION").is_file()) => folder,
        (Some(ws), Some(folder)) if std::fs::canonicalize(&folder).is_ok_and(|f| f.starts_with(&ws)) => workspace,
        (_, Some(folder)) => folder,
        (_, None) => workspace,
    }
}
fn run() -> Result<()> {
    let mut project = Project::load(root())?;
    let mut args = std::env::args().skip(1).peekable();
    let command = if args
        .peek()
        .is_some_and(|a| matches!(a.as_str(), "apply" | "inspect" | "events" | "props"))
    {
        args.next().unwrap()
    } else {
        "apply".into()
    };
    if command == "events" {
        let source = PathBuf::from(
            args.next()
                .ok_or("events requires SOURCE_MAPS OUTPUT_MAPS")?,
        );
        let output = PathBuf::from(
            args.next()
                .ok_or("events requires SOURCE_MAPS OUTPUT_MAPS")?,
        );
        let mut count = 0;
        for entry in std::fs::read_dir(source)?.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|s| s != "map") {
                continue;
            }
            let target = output.join(format!(
                "{}.map.json",
                path.file_stem().unwrap().to_string_lossy()
            ));
            if let Some(manifest) = read_json(&target)? {
                if manifest.get("_level_override").is_some() {
                    return Err(
                        format!("Run the base map exporter first: {}", target.display()).into(),
                    );
                }
                atomic_json(
                    target,
                    &events::with_event_layers(&manifest, &path, &serde_json::Value::Null)?,
                )?;
                count += 1;
            }
        }
        println!("[maps] Event layers exported for {count} maps");
        return Ok(());
    }
    let mut maps = vec![];
    let mut check = false;
    let mut db_out = None;
    let mut output = None;
    let mut source_root = None;
    let mut static_db: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--static-db" => static_db = Some(args.next().ok_or("--static-db requires a path")?.into()),
            "--assets" => project.assets = args.next().ok_or("--assets requires a path")?.into(),
            "--db-root" => {
                project.db_root = Some(args.next().ok_or("--db-root requires a path")?.into())
            }
            "--source-root" => {
                source_root = Some(PathBuf::from(
                    args.next().ok_or("--source-root requires a path")?,
                ))
            }
            "--overrides" => {
                project.overrides = args.next().ok_or("--overrides requires a path")?.into()
            }
            "--cache" => project.cache = args.next().ok_or("--cache requires a path")?.into(),
            "--db-out" => {
                db_out = Some(PathBuf::from(
                    args.next().ok_or("--db-out requires a path")?,
                ))
            }
            "--output" => {
                output = Some(PathBuf::from(
                    args.next().ok_or("--output requires a path")?,
                ))
            }
            "--check" => check = true,
            "--help" => {
                println!("dsor-levels [apply|inspect|props] [MAP ...] [--assets DIR] [--db-root DIR] [--source-root DIR] [--static-db FILE] [--overrides DIR] [--cache DIR] [--db-out DIR] [--check] [--output FILE]\ndsor-levels events SOURCE_MAPS OUTPUT_MAPS");
                return Ok(());
            }
            s if !s.starts_with('-') => maps.push(s.into()),
            _ => return Err(format!("Unknown option {arg}").into()),
        }
    }
    for (env, opt) in [
        ("DSOR_ASSETS", "--assets"),
        ("DSOR_MAP_DB", "--db-root"),
        ("DSOR_LEVEL_OVERRIDES", "--overrides"),
    ] {
        if !std::env::args().any(|a| a == opt) {
            if let Ok(value) = std::env::var(env) {
                match opt {
                    "--assets" => project.assets = value.into(),
                    "--db-root" => project.db_root = Some(value.into()),
                    _ => project.overrides = value.into(),
                }
            }
        }
    }
    let configured = read_json(project.root.join(".level-editor/project.json"))?
        .is_some_and(|v| v["db_root"].as_str().is_some_and(|s| !s.is_empty()));
    let explicit_db = configured || std::env::var_os("DSOR_MAP_DB").is_some()
        || std::env::args().any(|a| a == "--db-root");
    if !explicit_db {
        if let Some(source_root) = source_root { project.db_root = Some(source_root.join("maps")); }
    }
    project = project.checked()?;
    if command == "props" {
        if check { return Err("Use inspect to review props; props does not accept --check".into()); }
        if maps.is_empty() {
            let root = project.db_root.as_ref().ok_or("Prop export needs --db-root")?;
            maps = std::fs::read_dir(root)?.filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "db4"))
                .map(|e| e.path().file_stem().unwrap().to_string_lossy().into_owned()).collect();
            maps.sort();
        }
        let inferred = project.db_root.as_ref().and_then(|p| p.parent()).map(|p| p.join("db/static.db4")).filter(|p| p.is_file());
        let count = props::export_all(&project, &maps, static_db.as_deref().or(inferred.as_deref()))?;
        println!("[props] Exported placements and event rules for {count} maps");
        return Ok(());
    }
    if maps.is_empty() {
        if let Ok(entries) = std::fs::read_dir(&project.overrides) {
            maps = entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|e| e == "json"))
                .map(|e| e.path().file_stem().unwrap().to_string_lossy().into_owned())
                .collect();
            maps.sort();
        }
    }
    let mut failures = vec![];
    for map in &maps {
        let result = (|| -> Result<serde_json::Value> {
            let s = project.open(map, true)?;
            if command == "inspect" {
                return Ok(
                    json!({"snapshot":s.snapshot,"records":s.records,"patch":s.patch,"sources":s.sources,"conflicts":s.conflicts,"scene":scene::document(&s,&mut scene::Catalogs::default(),1)?}),
                );
            }
            if !s.conflicts.is_empty() {
                return Err(serde_json::to_string(&s.conflicts)?.into());
            }
            if check {
                Ok(json!({"map":map,"conflicts":[],"sources":s.sources}))
            } else {
                export::build(&s, None, db_out.as_deref())
            }
        })();
        match result {
            Ok(report) => {
                if let Some(path) = &output {
                    if maps.len() != 1 {
                        return Err("--output requires exactly one map".into());
                    }
                    atomic_json(path, &report)?;
                } else {
                    println!("{}", serde_json::to_string(&report)?);
                }
            }
            Err(e) => failures.push(json!({"map":map,"error":e.to_string()})),
        }
    }
    if !failures.is_empty() {
        return Err(
            serde_json::to_string_pretty(&json!({"level_override_errors":failures}))?.into(),
        );
    }
    if maps.is_empty() {
        println!("[levels] No authored overrides to apply");
    }
    Ok(())
}
