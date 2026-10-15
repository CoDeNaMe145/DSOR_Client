use crate::*;
use rusqlite::{
    types::{Value as SqlValue, ValueRef},
    Connection, OpenFlags,
};
use std::path::Path;
pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
pub fn connect(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?)
}
pub fn backup(source: &Path, target: &Path) -> Result<()> {
    let source = connect(source)?;
    let mut target = Connection::open(target)?;
    rusqlite::backup::Backup::new(&source, &mut target)?.run_to_completion(
        100,
        std::time::Duration::from_millis(10),
        None,
    )?;
    Ok(())
}
pub fn sql(v: &Value) -> Result<SqlValue> {
    Ok(match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else {
                SqlValue::Real(n.as_f64().ok_or("Invalid number")?)
            }
        }
        Value::String(s) => SqlValue::Text(s.clone()),
        Value::Object(m) if m.len() == 1 => SqlValue::Blob(bytes(v)?),
        _ => return Err("Database values must be scalars or blobs".into()),
    })
}
pub fn snapshot(path: &Path) -> Result<(Value, Value)> {
    let db = connect(path)?;
    let mut schemas = json!({});
    let mut records = json!({});
    let mut names =
        db.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
    for name in names.query_map([], |r| r.get::<_, String>(0))? {
        let name = name?;
        if !name.starts_with("_Instance_") {
            continue;
        }
        let mut statement = db.prepare(&format!("PRAGMA table_info({})", quote(&name)))?;
        let columns:Vec<Value>=statement.query_map([],|r|Ok(json!({"cid":r.get::<_,i64>(0)?,"name":r.get::<_,String>(1)?,"type":r.get::<_,String>(2)?,"notnull":r.get::<_,i64>(3)?,"dflt_value":r.get::<_,Option<String>>(4)?,"pk":r.get::<_,i64>(5)?})))?.collect::<std::result::Result<_,_>>()?;
        let mut primary: Vec<_> = columns
            .iter()
            .filter(|c| number(&c["pk"], 0.) > 0.)
            .collect();
        primary.sort_by_key(|c| c["pk"].as_i64());
        let keys: Vec<_> = primary
            .iter()
            .map(|c| text(&c["name"]).to_string())
            .collect();
        schemas[&name] = json!({"columns":columns,"keys":keys});
        let mut statement = db.prepare(&format!("SELECT * FROM {}", quote(&name)))?;
        let names: Vec<_> = statement
            .column_names()
            .iter()
            .map(|n| n.to_string())
            .collect();
        let mut query = statement.query([])?;
        let mut rows = json!({});
        let mut counts = std::collections::HashMap::new();
        while let Some(row) = query.next()? {
            let mut values = json!({});
            for (i, key) in names.iter().enumerate() {
                values[key] = match row.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(i) => json!(i),
                    ValueRef::Real(f) => json!(f),
                    ValueRef::Text(t) => json!(std::str::from_utf8(t)?),
                    ValueRef::Blob(b) => blob(b),
                };
            }
            let identity = if keys.is_empty() {
                values.clone()
            } else {
                Value::Object(
                    keys.iter()
                        .map(|k| (k.clone(), values[k].clone()))
                        .collect(),
                )
            };
            let hash = digest(&identity);
            let count = counts.entry(hash.clone()).or_insert(0);
            rows[format!("row:{hash}:{count}")] = values;
            *count += 1;
        }
        records[&name] = rows;
    }
    Ok((schemas, records))
}
pub fn export(session: &Session, destination: &Path) -> Result<()> {
    let source = session
        .project
        .cache
        .join("db4")
        .join(format!("{}.db4", session.name));
    let destination = std::path::absolute(destination)?;
    fs::create_dir_all(destination.parent().ok_or("Missing export parent")?)?;
    let dest = if destination.exists() {
        fs::canonicalize(&destination)?
    } else {
        fs::canonicalize(destination.parent().unwrap())?.join(destination.file_name().unwrap())
    };
    if dest == source
        || session
            .project
            .db_root
            .as_ref()
            .is_some_and(|p| dest == p.join(format!("{}.db4", session.name)))
    {
        return Err("Cannot export over original or cached DB4".into());
    }
    if !session.conflicts.is_empty() {
        return Err("Resolve conflicts before exporting".into());
    }
    let (schemas, records) = snapshot(&source)?;
    let expected: Value = Value::Object(
        object(&session.snapshot["records"])
            .filter(|(k, _)| k.starts_with("_Instance_"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    );
    if schemas != session.snapshot["schemas"] || records != expected {
        return Err("Cached DB4 changed in another session; reopen before exporting".into());
    }
    fs::create_dir_all(dest.parent().unwrap())?;
    let temp = tempfile::NamedTempFile::new_in(dest.parent().unwrap())?;
    backup(&source, temp.path())?;
    {
        let mut db = Connection::open(temp.path())?;
        let tx = db.transaction()?;
        for op in array(&session.patch["operations"])
            .filter(|o| text(&o["kind"]).starts_with("_Instance_"))
        {
            let kind = text(&op["kind"]);
            let id = text(&op["id"]);
            let action = text(&op["action"]);
            let original = &records[kind][id];
            if action == "add" {
                session.validate(kind, &op["values"], None)?;
                let keys: Vec<_> = object(&op["values"]).map(|(k, _)| k).collect();
                let args: Vec<_> = keys
                    .iter()
                    .map(|k| sql(&op["values"][*k]))
                    .collect::<Result<_>>()?;
                tx.execute(
                    &format!(
                        "INSERT INTO {} ({}) VALUES ({})",
                        quote(kind),
                        keys.iter().map(|k| quote(k)).collect::<Vec<_>>().join(","),
                        vec!["?"; keys.len()].join(",")
                    ),
                    rusqlite::params_from_iter(args),
                )?;
                continue;
            }
            if original.is_null() && action == "delete" {
                continue;
            }
            let mut keys: Vec<_> = array(&schemas[kind]["keys"]).map(text).collect();
            if keys.is_empty() {
                keys = object(original).map(|(k, _)| k.as_str()).collect();
            }
            let clause = keys
                .iter()
                .map(|k| format!("{} IS ?", quote(k)))
                .collect::<Vec<_>>()
                .join(" AND ");
            let args: Vec<_> = keys
                .iter()
                .map(|k| sql(&original[*k]))
                .collect::<Result<_>>()?;
            let count: i64 = tx.query_row(
                &format!("SELECT COUNT(*) FROM {} WHERE {clause}", quote(kind)),
                rusqlite::params_from_iter(&args),
                |r| r.get(0),
            )?;
            if count != 1 {
                return Err(format!("Ambiguous DB4 identity in {kind}").into());
            }
            if action == "delete" {
                tx.execute(
                    &format!("DELETE FROM {} WHERE {clause}", quote(kind)),
                    rusqlite::params_from_iter(args),
                )?;
            } else {
                let mut merged = original.clone();
                for (k, v) in object(&op["values"]) {
                    merged[k] = v.clone();
                }
                session.validate(kind, &merged, Some(original))?;
                let assignments = object(&op["values"])
                    .map(|(k, _)| format!("{}=?", quote(k)))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut values: Vec<_> = object(&op["values"])
                    .map(|(_, v)| sql(v))
                    .collect::<Result<_>>()?;
                values.extend(args);
                tx.execute(
                    &format!("UPDATE {} SET {assignments} WHERE {clause}", quote(kind)),
                    rusqlite::params_from_iter(values),
                )?;
            }
        }
        if tx.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? != "ok" {
            return Err("Generated DB4 failed integrity check".into());
        }
        tx.commit()?;
    }
    temp.persist(&dest).map_err(|e| e.error)?;
    Ok(())
}
