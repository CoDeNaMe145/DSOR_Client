use crate::*;
use std::collections::{BTreeMap, BTreeSet};
pub fn declarations(s: &str) -> impl Iterator<Item = (String, bool)> + '_ {
    s.split(';')
        .filter_map(|s| s.split_once('='))
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| {
            (
                k.trim().to_lowercase(),
                v.trim().eq_ignore_ascii_case("true"),
            )
        })
}
pub fn enabled(s: &str, active: &BTreeSet<String>) -> bool {
    let mut positive = false;
    let mut matched = false;
    for (k, v) in declarations(s) {
        if v {
            positive = true;
            matched |= active.contains(&k);
        } else if active.contains(&k) {
            return false;
        }
    }
    !positive || matched
}
#[derive(Default)]
pub struct MapEvents {
    pub sets: BTreeMap<i32, String>,
    pub flags: Vec<String>,
    rules: Value,
}
impl MapEvents {
    pub fn new(records: &Value, rules: &Value) -> Self {
        let sets: BTreeMap<_, _> = object(&records["_Instance_EventSets"])
            .filter_map(|(_, r)| {
                r["Id"]
                    .as_i64()
                    .or_else(|| text(&r["Id"]).parse().ok())
                    .map(|id| (id as i32, text(&r["EventFlags"]).to_owned()))
            })
            .collect();
        let mut flags = BTreeSet::new();
        for (_, rows) in object(records) {
            for (_, r) in object(rows) {
                if let Some(s) = sets.get(&(number(&r["EventSetRow"], -1.) as i32)) {
                    flags.extend(declarations(s).map(|(k, _)| k));
                }
            }
        }
        for (_, r) in object(&records[GEOMETRY]) {
            if !text(&r["event"]).is_empty() {
                flags.insert(text(&r["event"]).to_lowercase());
            }
        }
        for (_, r) in object(rules) {
            flags.extend(declarations(text(r)).map(|(k, _)| k));
        }
        Self {
            sets,
            flags: flags.into_iter().collect(),
            rules: rules.clone(),
        }
    }
    pub fn preview_flags(&self, flag: &str) -> BTreeSet<String> {
        let mut active = BTreeSet::new();
        if !flag.is_empty() {
            active.insert(flag.to_lowercase());
        }
        if active.contains("christmas_2012")
            && self.flags.iter().any(|f| f == "christmas_2012_silent")
        {
            active.insert("christmas_2012_silent".into());
        }
        active
    }
    pub fn active_sets(&self, active: &BTreeSet<String>) -> BTreeSet<i32> {
        std::iter::once(-1)
            .chain(
                self.sets
                    .iter()
                    .filter(|(_, s)| enabled(s, active))
                    .map(|(k, _)| *k),
            )
            .collect()
    }
    pub fn visible(&self, kind: &str, id: &str, row: &Value, active: &BTreeSet<String>) -> bool {
        if kind == GEOMETRY {
            if !text(&row["event"]).is_empty() {
                active.contains(&text(&row["event"]).to_lowercase())
            } else {
                enabled(text(&self.rules[id]), active)
            }
        } else {
            let key = number(&row["EventSetRow"], -1.) as i32;
            key == -1 || self.sets.get(&key).is_some_and(|s| enabled(s, active))
        }
    }
}
pub fn label(flag: &str) -> &str {
    match flag {
        "" => "None",
        "christmas_2012" => "Winter festival",
        "christmas_2012_silent" => "Winter scenery only",
        "anniversary_event" => "Anniversary",
        "halloween_event_01" => "Halloween",
        s => s,
    }
}

struct Reader {
    data: Vec<u8>,
    at: usize,
}
impl Reader {
    fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            data: fs::read(path)?,
            at: 0,
        })
    }
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.at.checked_add(n).ok_or("Invalid map length")?;
        let data = self
            .data
            .get(self.at..end)
            .ok_or("Truncated map event data")?;
        self.at = end;
        Ok(data)
    }
    fn tag(&mut self, s: &str) -> Result<()> {
        if self.take(4)? != s.bytes().rev().collect::<Vec<_>>() {
            return Err(format!("Expected map tag {s}").into());
        }
        Ok(())
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn vec(&mut self) -> Result<Vec<f64>> {
        let mut a = vec![];
        for _ in 0..4 {
            a.push(f32::from_le_bytes(self.take(4)?.try_into()?) as f64);
        }
        Ok(a)
    }
    fn header(&mut self) -> Result<()> {
        self.tag("DSOM")?;
        if self.u32()? != 5 {
            return Err("Unsupported map version".into());
        }
        Ok(())
    }
    fn instances(
        &mut self,
        strings: &[String],
        templates: &[usize],
    ) -> Result<Vec<(Value, usize)>> {
        self.tag("INST")?;
        let count = self.u32()?;
        let mut rows = vec![];
        for _ in 0..count {
            let p = self.vec()?;
            let r = self.vec()?;
            let s = if self.take(1)?[0] != 0 {
                self.vec()?
            } else {
                vec![1., 1., 1., 0.]
            };
            let col = self.take(1)?[0] != 0;
            self.take(1)?;
            let template = self.u16()? as i16;
            let group = self.u16()? as i16;
            let name = self.u16()? as usize;
            let mapping = self.u16()? as usize;
            if template == -1 {
                continue;
            }
            let model = strings
                .get(
                    *templates
                        .get(template as usize)
                        .ok_or("Invalid event template")?,
                )
                .ok_or("Invalid template string")?;
            if model.is_empty() {
                continue;
            }
            let rounded = |v: &[f64], places: i32| {
                v.iter()
                    .map(|x| (x * 10f64.powi(places)).round_ties_even() / 10f64.powi(places))
                    .collect::<Vec<_>>()
            };
            rows.push((json!({"model":model,"p":rounded(&p[..3],5),"r":rounded(&r,6),"s":rounded(&s[..3],5),"n":strings.get(name).map(String::as_str).unwrap_or(""),"g":group,"col":col}),mapping));
        }
        Ok(rows)
    }
}
pub fn with_event_layers(manifest: &Value, path: &Path, cached: &Value) -> Result<Value> {
    let mut r = Reader::new(path)?;
    r.header()?;
    r.tag("MAPI")?;
    r.take(54)?;
    r.tag("STRT")?;
    let count = r.u32()? as usize;
    let len = r.u32()? as usize;
    let strings: Vec<_> = String::from_utf8_lossy(r.take(len)?)
        .split('\0')
        .take(count)
        .map(str::to_owned)
        .collect();
    r.tag("SETT")?;
    let len = r.u32()? as usize;
    r.take(len.checked_mul(4).ok_or("Invalid settings length")?)?;
    r.tag("EVET")?;
    let mut flags = vec![];
    for _ in 0..r.u32()? {
        flags.push(
            strings
                .get(r.u32()? as usize)
                .ok_or("Invalid event name index")?
                .clone(),
        );
    }
    r.tag("EMAL")?;
    let mut masks = vec![];
    for _ in 0..r.u32()? {
        let len = r.u16()? as usize;
        masks.push(String::from_utf8(r.take(len)?.to_vec())?);
    }
    r.tag("TMPL")?;
    let mut templates = vec![];
    for _ in 0..r.u32()? {
        templates.push(r.u16()? as usize);
        r.take(44)?;
    }
    let mut result = manifest.clone();
    result["event_instances"] = json!([]);
    result["event_rules"] = json!({});
    let mut counts = BTreeMap::new();
    for (row, mapping) in r.instances(&strings, &templates)? {
        let hash = digest(&row);
        let count = counts.entry(hash.clone()).or_insert(0);
        let mask = masks.get(mapping).ok_or("Invalid event mask index")?;
        if mask.len() != flags.len() || mask.bytes().any(|b| !b"01".contains(&b)) {
            return Err("Invalid event mask".into());
        }
        let rule = flags
            .iter()
            .zip(mask.bytes())
            .filter(|(_, b)| *b == b'0')
            .map(|(s, _)| format!("{s}=False;"))
            .collect::<String>();
        if !rule.is_empty() {
            result["event_rules"][format!("placement:{hash}:{count}")] = json!(rule);
        }
        *count += 1;
    }
    let mut models = result["models"]
        .as_array()
        .cloned()
        .ok_or("Map models missing")?;
    for flag in flags {
        if flag.is_empty() || flag.contains(['/', '\\', ':']) || flag == "." || flag == ".." {
            return Err("Invalid map event filename".into());
        }
        let file = path.with_file_name(format!(
            "{}_{}.eve",
            path.file_stem().unwrap().to_string_lossy(),
            flag
        ));
        let mut rows = vec![];
        if file.is_file() {
            let mut event = Reader::new(&file)?;
            event.header()?;
            for (mut row, _) in event.instances(&strings, &templates)? {
                row["event"] = json!(flag.to_lowercase());
                rows.push(row);
            }
        } else {
            let previous = if manifest.get("event_instances").is_some() {
                manifest
            } else {
                cached
            };
            for row in array(&previous["event_instances"])
                .filter(|r| text(&r["event"]).eq_ignore_ascii_case(&flag))
            {
                let mut row = row.clone();
                row["model"] = previous["models"]
                    [row["m"].as_u64().ok_or("Invalid cached model index")? as usize]
                    .clone();
                rows.push(row);
            }
        }
        for mut row in rows {
            let model = row.as_object_mut().unwrap().remove("model").unwrap();
            let i = models.iter().position(|m| *m == model).unwrap_or_else(|| {
                models.push(model);
                models.len() - 1
            });
            row["m"] = json!(i);
            result["event_instances"].as_array_mut().unwrap().push(row);
        }
    }
    result["models"] = json!(models);
    Ok(result)
}
