use crate::*;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub assets: PathBuf,
    pub db_root: Option<PathBuf>,
    pub overrides: PathBuf,
    pub cache: PathBuf,
}
impl Project {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        let config = read_json(root.join(".level-editor/project.json"))?.unwrap_or(json!({}));
        let path = |key: &str, default: PathBuf| {
            config[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or(default)
        };
        let db = path("db_root", root.join(".local/dso_server/data/maps"));
        let p = Self {
            assets: path("assets", root.join("assets")),
            db_root: if db.is_dir() { Some(db) } else { None },
            overrides: path("overrides", root.join("level-overrides")),
            cache: root.join(".level-editor"),
            root,
        };
        p.checked()
    }
    pub fn checked(mut self) -> Result<Self> {
        fn absolute(root: &Path, path: &Path) -> Result<PathBuf> {
            let p = std::path::absolute(if path.is_absolute() {
                path.to_owned()
            } else {
                root.join(path)
            })?;
            // Resolve existing ancestors too, so a symlink cannot put authored data under assets.
            if p.exists() {
                return Ok(fs::canonicalize(p)?);
            }
            let parent = p.parent().ok_or("Invalid path")?;
            Ok(absolute(root, parent)?.join(p.file_name().unwrap()))
        }
        self.assets = absolute(&self.root, &self.assets)?;
        self.overrides = absolute(&self.root, &self.overrides)?;
        self.cache = absolute(&self.root, &self.cache)?;
        if self.overrides.starts_with(&self.assets) || self.cache.starts_with(&self.assets) {
            return Err("Overrides and metadata cache must be outside generated assets".into());
        }
        if let Some(p) = &self.db_root {
            self.db_root = Some(absolute(&self.root, p)?);
        }
        Ok(self)
    }
    pub fn save_settings(&self) -> Result<()> {
        atomic_json(
            self.root.join(".level-editor/project.json"),
            &json!({"assets":self.assets,"db_root":self.db_root,"overrides":self.overrides}),
        )
    }
    pub fn maps(&self) -> Vec<String> {
        let mut names = BTreeSet::new();
        let mut paths = vec![
            (self.assets.join("maps"), ".map.json"),
            (self.overrides.clone(), ".json"),
            (self.cache.join("maps"), ".json"),
        ];
        if let Some(p) = &self.db_root {
            paths.push((p.clone(), ".db4"));
        }
        for (p, suffix) in paths {
            if let Ok(entries) = fs::read_dir(p) {
                for entry in entries.flatten() {
                    if let Some(n) = entry.file_name().to_string_lossy().strip_suffix(suffix) {
                        if map_name(n).is_ok() {
                            names.insert(n.to_owned());
                        }
                    }
                }
            }
        }
        names.into_iter().collect()
    }
    pub fn patch_path(&self, name: &str) -> PathBuf {
        self.overrides.join(format!("{name}.json"))
    }
    pub fn open(&self, name: &str, refresh: bool) -> Result<Session> {
        Session::open(self.clone(), name, refresh)
    }
}
pub struct Session {
    pub project: Project,
    pub name: String,
    pub patch: Value,
    pub snapshot: Value,
    pub sources: Value,
    pub records: Value,
    pub conflicts: Vec<Value>,
    saved: String,
    undo: Vec<Value>,
    redo: Vec<Value>,
}
pub fn empty_patch(name: &str) -> Value {
    json!({"version":1,"map":name,"operations":[]})
}
pub fn validate_patch(patch: &Value, name: &str) -> Result<()> {
    if patch["version"] != 1 || patch["map"] != name {
        return Err("Unsupported override version or mismatched map name".into());
    }
    let ops = patch["operations"]
        .as_array()
        .ok_or("Override operations must be an array")?;
    let mut seen = BTreeSet::new();
    for op in ops {
        let k = text(&op["kind"]);
        let id = op["id"].as_str().ok_or("Missing override identity")?;
        let action = text(&op["action"]);
        if !seen.insert((k, id))
            || !matches!(action, "add" | "update" | "delete")
            || (!matches!(k, GEOMETRY | CAMERA) && !k.starts_with("_Instance_"))
        {
            return Err("Duplicate or invalid override operation".into());
        }
        if action != "add" && !op["base"].is_object()
            || action != "delete" && !op["values"].is_object()
        {
            return Err("Override needs original record and field values".into());
        }
    }
    Ok(())
}
impl Session {
    fn open(project: Project, name: &str, refresh: bool) -> Result<Self> {
        map_name(name)?;
        let patch = read_json(project.patch_path(name))?.unwrap_or_else(|| empty_patch(name));
        validate_patch(&patch, name)?;
        let snapshot = read_json(project.cache.join("maps").join(format!("{name}.json")))?
            .unwrap_or(json!({"version":1,"map":name,"manifest":null,"schemas":{},"records":{}}));
        let sources = json!({"geometry":if snapshot["manifest"].is_object(){"cached"}else{"unavailable"},"camera":if snapshot["records"][CAMERA].is_object(){"cached"}else{"unavailable"},"db4":if object(&snapshot["schemas"]).next().is_some(){"cached"}else{"unavailable"}});
        let mut s = Self {
            project,
            name: name.into(),
            saved: digest(&patch),
            patch,
            snapshot,
            sources,
            records: json!({}),
            conflicts: vec![],
            undo: vec![],
            redo: vec![],
        };
        if refresh {
            s.refresh()?;
        }
        s.recompose();
        Ok(s)
    }
    pub fn refresh(&mut self) -> Result<()> {
        if let Some(mut manifest) = read_json(
            self.project
                .assets
                .join("maps")
                .join(format!("{}.map.json", self.name)),
        )? {
            if manifest.get("_level_override").is_none() {
                if let Some(root) = &self.project.db_root {
                    if root.join(format!("{}.map", self.name)).is_file() {
                        manifest = events::with_event_layers(
                            &manifest,
                            &root.join(format!("{}.map", self.name)),
                            &self.snapshot["manifest"],
                        )?;
                    }
                }
                if manifest.get("event_instances").is_none()
                    && self.snapshot["manifest"].is_object()
                {
                    let cached = &self.snapshot["manifest"];
                    let mut only = cached.clone();
                    only["instances"] = json!([]);
                    let mut rows = geometry_records(&manifest);
                    for (k, v) in object(&geometry_records(&only)) {
                        rows[k] = v.clone();
                    }
                    manifest = manifest_from(&manifest, &rows, &self.name);
                    manifest["event_rules"] = cached["event_rules"].clone();
                }
                self.snapshot["records"][GEOMETRY] = geometry_records(&manifest);
                self.snapshot["manifest"] = manifest;
                self.sources[GEOMETRY] = json!("available");
            } else if self.snapshot["manifest"].is_object()
                && json!(digest(&self.snapshot["manifest"]))
                    == manifest["_level_override"]["base_sha256"]
            {
                self.sources[GEOMETRY] = json!("cached");
            } else {
                self.snapshot["manifest"] = Value::Null;
                self.snapshot["records"]
                    .as_object_mut()
                    .unwrap()
                    .remove(GEOMETRY);
                self.sources[GEOMETRY] = json!("unavailable");
            }
        }
        if let Some(root) = &self.project.db_root {
            let source = root.join(format!("{}.db4", self.name));
            if source.is_file() {
                let dest = self
                    .project
                    .cache
                    .join("db4")
                    .join(format!("{}.db4", self.name));
                fs::create_dir_all(dest.parent().unwrap())?;
                let temp = tempfile::NamedTempFile::new_in(dest.parent().unwrap())?;
                database::backup(&source, temp.path())?;
                let (schemas, records) = database::snapshot(temp.path())?;
                temp.persist(dest).map_err(|e| e.error)?;
                let mut combined = records;
                for k in [GEOMETRY, CAMERA] {
                    if let Some(v) = self.snapshot["records"].get(k) {
                        combined[k] = v.clone();
                    }
                }
                self.snapshot["records"] = combined;
                self.snapshot["schemas"] = schemas;
                self.sources["db4"] = json!("available");
            }
        }
        if self.name == "a0000_char" {
            if let Some(catalog) = read_json(self.project.assets.join("characters/selection.json"))?
            {
                let camera = catalog["camera"]["_level_override"]
                    .get("base")
                    .unwrap_or(&catalog["camera"]);
                self.snapshot["selection_camera"] = camera.clone();
                self.snapshot["records"][CAMERA] =
                    json!({"character-selection":scene::selection_record(camera)?});
                self.sources[CAMERA] = json!("available");
            }
        }
        atomic_json(
            self.project
                .cache
                .join("maps")
                .join(format!("{}.json", self.name)),
            &self.snapshot,
        )
    }
    pub fn recompose(&mut self) {
        (self.records, self.conflicts) = compose(&self.snapshot["records"], &self.patch);
        for op in array(&self.patch["operations"]) {
            let k = text(&op["kind"]);
            let id = text(&op["id"]);
            if self.sources[source_key(k)] == "unavailable"
                && op["action"] == "update"
                && self.records[k][id].is_null()
            {
                if !self.records[k].is_object() {
                    self.records[k] = json!({});
                }
                let mut row = op["base"].clone();
                for (key, v) in object(&op["values"]) {
                    row[key] = v.clone();
                }
                self.records[k][id] = row;
            }
        }
    }
    pub fn dirty(&self) -> bool {
        digest(&self.patch) != self.saved
    }
    fn checkpoint(&mut self) {
        self.undo.push(self.patch.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.patch, p));
            self.recompose();
        }
    }
    pub fn redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.patch, p));
            self.recompose();
        }
    }
    pub fn operation(&self, k: &str, id: &str) -> Option<&Value> {
        array(&self.patch["operations"]).find(|o| o["kind"] == k && o["id"] == id)
    }
    fn remove_op(&mut self, k: &str, id: &str) {
        self.patch["operations"]
            .as_array_mut()
            .unwrap()
            .retain(|o| !(o["kind"] == k && o["id"] == id));
    }
    pub fn validate(&self, k: &str, v: &Value, old: Option<&Value>) -> Result<()> {
        let values = v.as_object().ok_or("Record must be an object")?;
        if k == GEOMETRY {
            let model = text(&v["model"]);
            let relative = model.rsplit(':').next().unwrap().replace('\\', "/");
            if model.trim().is_empty()
                || relative.starts_with('/')
                || relative.split('/').any(|c| c == "..")
            {
                return Err("Model must be a reference inside the assets directory".into());
            }
            for (key, size) in [("p", 3), ("r", 4), ("s", 3)] {
                if v[key].as_array().is_none_or(|a| {
                    a.len() != size || a.iter().any(|n| n.as_f64().is_none_or(|f| !f.is_finite()))
                }) {
                    return Err(format!("{key} needs {size} finite numbers").into());
                }
            }
            if (array(&v["r"]).map(|x| number(x, 0.).powi(2)).sum::<f64>() - 1.).abs() > 0.002
                || array(&v["s"]).any(|x| number(x, 0.).abs() < 1e-7)
            {
                return Err("Rotation must be a unit quaternion and scale cannot be zero".into());
            }
            if let Some(e) = v.get("event") {
                if e.as_str()
                    .is_none_or(|e| e.contains([';', '=', '\n', '\r']))
                {
                    return Err("Event must be a single flag name".into());
                }
            }
            return Ok(());
        }
        let schema = &self.snapshot["schemas"][k];
        if schema.is_object() {
            let columns: Vec<_> = array(&schema["columns"]).collect();
            if columns.len() != values.len()
                || columns
                    .iter()
                    .any(|c| !values.contains_key(text(&c["name"])))
            {
                return Err("DB4 records must preserve every schema column".into());
            }
            for c in columns {
                let key = text(&c["name"]);
                let raw = database::sql(&v[key])?;
                if v[key].is_null() {
                    if flag(&c["notnull"]) || flag(&c["pk"]) {
                        return Err(format!("{key} cannot be null").into());
                    }
                    continue;
                }
                let ty = text(&c["type"]).to_uppercase();
                let valid = if ty.contains("BLOB") {
                    matches!(raw, rusqlite::types::Value::Blob(_))
                } else if ty.contains("INT") {
                    matches!(raw, rusqlite::types::Value::Integer(_))
                } else if ["REAL", "FLOA", "DOUB"].iter().any(|t| ty.contains(t)) {
                    v[key].is_number()
                } else {
                    v[key].is_string()
                };
                if !valid {
                    return Err(format!("{key} must match its {ty} column type").into());
                }
            }
            if let Some(o) = old {
                if array(&schema["keys"]).any(|key| v[text(key)] != o[text(key)]) {
                    return Err(
                        "Primary keys are stable identities; duplicate to create a new record"
                            .into(),
                    );
                }
            }
        } else {
            for v in values.values() {
                database::sql(v)?;
            }
        }
        if let Some(guid) = v.get("Guid") {
            if bytes(guid)?.len() != 16 {
                return Err("Guid must be a 16-byte blob".into());
            }
            if old.is_some_and(|o| o["Guid"] != *guid) {
                return Err("Guid is a stable identity".into());
            }
        }
        if let Some(t) = v.get("Transform").filter(|v| !v.is_null()) {
            let m = floats(t)?;
            if m.len() != 16 || m.iter().any(|n| !n.is_finite()) {
                return Err("Transform needs 16 finite float32 values".into());
            }
        }
        for key in [
            "LightColor",
            "LightOppositeColor",
            "LightAmbient",
            "Balance",
            "BloomColor",
            "FogColor",
            "HeightFogColor",
            "VignetteColor",
            "LocalLightColor",
            "RimLightColor",
            "PrimaryColor",
            "SecondaryColor",
        ] {
            if v[key].is_object() {
                let channels = floats(&v[key])?;
                if channels.len() != 4 || channels.iter().any(|n| !n.is_finite()) {
                    return Err(format!("{key} needs four finite channels").into());
                }
            }
        }
        scene::validate_special(k, v, old)?;
        Ok(())
    }
    pub fn update(&mut self, k: &str, id: &str, v: Value) -> Result<()> {
        let old = &self.records[k][id];
        if old.is_null() {
            return Err("Record no longer exists".into());
        }
        self.validate(k, &v, Some(old))?;
        if old == &v {
            return Ok(());
        }
        if self
            .conflicts
            .iter()
            .any(|c| c["kind"] == k && c["id"] == id)
            && self.sources[source_key(k)] != "unavailable"
        {
            return Err("Resolve this record's conflict before editing".into());
        }
        let op = self.operation(k, id).cloned();
        let original = self.snapshot["records"][k]
            .get(id)
            .cloned()
            .or_else(|| op.as_ref().and_then(|o| o.get("base")).cloned());
        if let Some(o) = &original {
            if o.as_object()
                .unwrap()
                .keys()
                .ne(v.as_object().unwrap().keys())
            {
                return Err("Preserve original record fields".into());
            }
        }
        let next = if op.as_ref().is_some_and(|o| o["action"] == "add") {
            Some(json!({"kind":k,"id":id,"action":"add","values":v}))
        } else {
            let original = original.ok_or("Original record is unavailable")?;
            let delta: Value = Value::Object(
                object(&v)
                    .filter(|(key, v)| original[*key] != **v)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            );
            if object(&delta).next().is_none() {
                None
            } else {
                Some(json!({"kind":k,"id":id,"action":"update","base":original,"values":delta}))
            }
        };
        self.checkpoint();
        self.remove_op(k, id);
        if let Some(op) = next {
            self.patch["operations"].as_array_mut().unwrap().push(op);
        }
        self.recompose();
        Ok(())
    }
    pub fn add(&mut self, k: &str, v: Value) -> Result<String> {
        if k == CAMERA {
            return Err("Edit the existing fixed camera".into());
        }
        self.validate(k, &v, None)?;
        let keys: Vec<_> = array(&self.snapshot["schemas"][k]["keys"])
            .map(text)
            .collect();
        if !keys.is_empty()
            && object(&self.records[k]).any(|(_, r)| keys.iter().all(|key| r[*key] == v[*key]))
        {
            return Err("Primary key already exists".into());
        }
        let id = format!("added:{}", uuid::Uuid::new_v4());
        self.checkpoint();
        self.patch["operations"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":k,"id":id,"action":"add","values":v}));
        self.recompose();
        Ok(id)
    }
    pub fn duplicate(&mut self, k: &str, id: &str) -> Result<String> {
        let mut v = self.records[k][id].clone();
        if k != GEOMETRY {
            if self.snapshot["schemas"][k]["keys"] != json!(["Guid"]) {
                return Err("Duplicate requires a Guid primary key".into());
            }
            v["Guid"] = blob(uuid::Uuid::new_v4().as_bytes());
        }
        self.add(k, v)
    }
    pub fn delete(&mut self, k: &str, id: &str) -> Result<()> {
        if k == CAMERA {
            return Err("This level requires its fixed camera".into());
        }
        let op = self.operation(k, id).cloned();
        let original = self.snapshot["records"][k]
            .get(id)
            .cloned()
            .or_else(|| op.as_ref().and_then(|o| o.get("base")).cloned());
        let added = op.is_some_and(|o| o["action"] == "add");
        if !added && original.is_none() {
            return Err("Original record unavailable".into());
        }
        self.checkpoint();
        self.remove_op(k, id);
        if !added {
            self.patch["operations"]
                .as_array_mut()
                .unwrap()
                .push(json!({"kind":k,"id":id,"action":"delete","base":original}));
        }
        self.recompose();
        Ok(())
    }
    pub fn revert(&mut self, k: &str, id: &str) {
        self.checkpoint();
        self.remove_op(k, id);
        self.recompose();
    }
    pub fn keep_override(&mut self, k: &str, id: &str) -> Result<()> {
        let base = self.snapshot["records"][k][id].clone();
        let mut op = self.operation(k, id).cloned().ok_or("No override")?;
        if base.is_null() || op["action"] == "add" {
            return Err(
                "Cannot rebase a missing identity; revert and reapply to the intended record"
                    .into(),
            );
        }
        op["base"] = base;
        self.checkpoint();
        self.remove_op(k, id);
        self.patch["operations"].as_array_mut().unwrap().push(op);
        self.recompose();
        Ok(())
    }
    pub fn save(&mut self) -> Result<()> {
        let path = self.project.patch_path(&self.name);
        let disk = read_json(&path)?.unwrap_or_else(|| empty_patch(&self.name));
        if digest(&disk) != self.saved {
            return Err("Overrides changed in another editor; reopen before saving".into());
        }
        validate_patch(&self.patch, &self.name)?;
        atomic_json(path, &self.patch)?;
        self.saved = digest(&self.patch);
        Ok(())
    }
    pub fn manifest(&self) -> Value {
        manifest_from(
            &self.snapshot["manifest"],
            &self.records[GEOMETRY],
            &self.name,
        )
    }
}
