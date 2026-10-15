//! Editor persistence is separate from generated assets and has no renderer dependency.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub mod database;
pub mod events;
pub mod props;
pub mod export;
pub mod scene;
pub mod session;
use serde_json::{json, Value};
pub use session::{Project, Session};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};
pub const GEOMETRY: &str = "geometry";
pub const CAMERA: &str = "camera";
pub const LIGHT: &str = "_Instance_Light";
pub const AMBIENCE: &str = "_Instance_AmbienceBubble";

/// Version-one identities were produced by Python's sorted, compact JSON.
/// Keep its float exponent thresholds, sign and padding for existing overrides.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}:{}", serde_json::to_string(k).unwrap(), canonical(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Number(n) if n.is_f64() => {
            let raw = n.to_string();
            let (sign, unsigned) = if let Some(s) = raw.strip_prefix('-') {
                ("-", s)
            } else {
                ("", raw.as_str())
            };
            let (mantissa, exponent) = unsigned
                .split_once('e')
                .map(|(m, e)| (m, e.parse::<i32>().unwrap()))
                .unwrap_or((unsigned, 0));
            let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
            let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
            let leading = digits.bytes().take_while(|c| *c == b'0').count();
            let trimmed = digits[leading..].trim_end_matches('0');
            if trimmed.is_empty() {
                return format!("{sign}0.0");
            }
            let exp = point + exponent - leading as i32 - 1;
            if !(-4..16).contains(&exp) {
                let tail = if trimmed.len() > 1 {
                    format!(".{}", &trimmed[1..])
                } else {
                    String::new()
                };
                format!(
                    "{sign}{}{tail}e{}{abs:02}",
                    &trimmed[..1],
                    if exp < 0 { "-" } else { "+" },
                    abs = exp.abs()
                )
            } else {
                let position = exp + 1;
                if position <= 0 {
                    format!("{sign}0.{}{trimmed}", "0".repeat((-position) as usize))
                } else if position as usize >= trimmed.len() {
                    format!(
                        "{sign}{trimmed}{}.0",
                        "0".repeat(position as usize - trimmed.len())
                    )
                } else {
                    format!(
                        "{sign}{}.{}",
                        &trimmed[..position as usize],
                        &trimmed[position as usize..]
                    )
                }
            }
        }
        _ => value.to_string(),
    }
}
pub fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}
pub fn read_json(path: impl AsRef<Path>) -> Result<Option<Value>> {
    match fs::read(path) {
        Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn atomic_json(path: impl AsRef<Path>, value: &Value) -> Result<()> {
    let path = path.as_ref();
    let parent = path.parent().ok_or("Missing parent directory")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn blob(bytes: &[u8]) -> Value {
    json!({"$blob": bytes.iter().map(|b|format!("{b:02X}")).collect::<String>()})
}
pub fn bytes(value: &Value) -> Result<Vec<u8>> {
    let s = value["$blob"].as_str().ok_or("Expected a binary blob")?;
    if s.len() % 2 != 0 || !s.is_ascii() {
        return Err("Invalid blob encoding".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(Into::into))
        .collect()
}
pub fn floats(value: &Value) -> Result<Vec<f32>> {
    let b = bytes(value)?;
    if b.len() % 4 != 0 {
        return Err("Expected float32 blob".into());
    }
    Ok(b.chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect())
}
pub fn float_blob(values: impl IntoIterator<Item = f32>) -> Value {
    blob(
        &values
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}
pub fn number(v: &Value, fallback: f64) -> f64 {
    v.as_f64().unwrap_or(fallback)
}
pub fn flag(v: &Value) -> bool {
    v.as_bool().unwrap_or_else(|| v.as_i64().unwrap_or(0) != 0)
}
pub fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn object(v: &Value) -> impl Iterator<Item = (&String, &Value)> {
    v.as_object().into_iter().flat_map(|m| m.iter())
}
pub fn array(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}
pub fn map_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        Err("Invalid map name".into())
    } else {
        Ok(())
    }
}
pub fn source_key(kind: &str) -> &str {
    if matches!(kind, GEOMETRY | CAMERA) {
        kind
    } else {
        "db4"
    }
}
pub fn geometry_records(manifest: &Value) -> Value {
    let mut rows = json!({});
    let mut counts = std::collections::HashMap::new();
    for row in array(&manifest["instances"]).chain(array(&manifest["event_instances"])) {
        let mut v = row.clone();
        let index = v.as_object_mut().unwrap().remove("m").unwrap();
        v["model"] = manifest["models"][index.as_u64().unwrap() as usize].clone();
        let hash = digest(&v);
        let count = counts.entry(hash.clone()).or_insert(0);
        rows[format!("placement:{hash}:{count}")] = v;
        *count += 1;
    }
    rows
}
pub fn manifest_from(base: &Value, rows: &Value, name: &str) -> Value {
    let mut result = if base.is_object() {
        base.clone()
    } else {
        json!({"map":name,"center":[0,0,0],"extents":[50,20,50],"models":[]})
    };
    let mut models = result["models"].as_array().cloned().unwrap_or_default();
    result["instances"] = json!([]);
    result["event_instances"] = json!([]);
    for (_, v) in object(rows) {
        let mut row = v.clone();
        let model = row.as_object_mut().unwrap().remove("model").unwrap();
        let i = models.iter().position(|m| m == &model).unwrap_or_else(|| {
            models.push(model);
            models.len() - 1
        });
        row["m"] = json!(i);
        let key = if text(&row["event"]).is_empty() {
            "instances"
        } else {
            "event_instances"
        };
        result[key].as_array_mut().unwrap().push(row);
    }
    result["models"] = json!(models);
    result
}

pub fn compose(base: &Value, patch: &Value) -> (Value, Vec<Value>) {
    let mut result = base.clone();
    let mut conflicts = vec![];
    for op in array(&patch["operations"]) {
        let kind = text(&op["kind"]);
        let id = text(&op["id"]);
        let action = text(&op["action"]);
        if !result[kind].is_object() {
            result[kind] = json!({});
        }
        let current = result[kind][id].clone();
        let mut problem = None;
        if kind != GEOMETRY && base.get(kind).is_none() {
            problem = Some("Original DB4 table is unavailable or was removed".to_string());
            if action == "add" {
                result[kind][id] = op["values"].clone();
            }
        } else {
            match action {
                "add" => {
                    if !current.is_null() && current != op["values"] {
                        problem = Some("Added identity already exists in the base".into())
                    } else {
                        result[kind][id] = op["values"].clone();
                    }
                }
                "delete" => {
                    if current.is_null() && kind == GEOMETRY {
                        problem = Some(
                            "Deleted placement is missing or its original identity changed".into(),
                        )
                    } else if !current.is_null() && current != op["base"] {
                        problem = Some("Deleted record changed in the base".into())
                    } else {
                        result[kind].as_object_mut().unwrap().remove(id);
                    }
                }
                "update" => {
                    if current.is_null() {
                        problem = Some(
                            "Original record is missing or its placement identity changed".into(),
                        )
                    } else {
                        let changed: Vec<_> = object(&op["values"])
                            .filter(|(k, v)| {
                                current.get(*k).is_none()
                                    || (current[*k] != op["base"][*k] && current[*k] != **v)
                            })
                            .map(|(k, _)| k.clone())
                            .collect();
                        if !changed.is_empty() {
                            problem = Some(format!(
                                "Base changed the edited fields: {}",
                                changed.join(", ")
                            ));
                        } else {
                            for (k, v) in object(&op["values"]) {
                                result[kind][id][k] = v.clone();
                            }
                        }
                    }
                }
                _ => problem = Some("Invalid override action".into()),
            }
        }
        if let Some(reason) = problem {
            conflicts.push(json!({"kind":kind,"id":id,"reason":reason}));
        }
    }
    (result, conflicts)
}
