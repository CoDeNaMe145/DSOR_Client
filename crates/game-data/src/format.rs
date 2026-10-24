//! The CDN's containers, as the original client reads them
//! (`Http::HttpNzVirtualFileSystem`) and simo8902's drakensang-nb3-bundle-extractor
//! (WTFPL, `neb.py`) unpacks them: zlib blobs, tables of contents and NB3 bundles.
use std::fmt;

use crate::Error;

/// An md5 digest; the CDN writes it as 32 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Md5(pub [u8; 16]);

impl Md5 {
    pub fn of(data: &[u8]) -> Self {
        use md5::Digest;
        let mut out = [0; 16];
        out.copy_from_slice(&md5::Md5::digest(data));
        Self(out)
    }

    /// 32 hex digits, in either case.
    pub fn parse(hex: &str) -> Option<Self> {
        let hex = hex.as_bytes();
        if hex.len() != 32 {
            return None;
        }
        let mut out = [0; 16];
        for (byte, pair) in out.iter_mut().zip(hex.chunks_exact(2)) {
            let digit = |c: u8| (c as char).to_digit(16);
            *byte = (digit(pair[0])? * 16 + digit(pair[1])?) as u8;
        }
        Some(Self(out))
    }
}

impl fmt::Display for Md5 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for Md5 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// The stored form of every file, table and bundle on the CDN and of every
/// bundle member: `__ZN` (or `ZN__`), the unpacked size as a little-endian u32,
/// then a zlib stream. Anything else is stored as it is.
pub fn unpack(blob: &[u8]) -> Result<Vec<u8>, Error> {
    let packed = blob.len() >= 8 && matches!(&blob[..4], b"__ZN" | b"ZN__");
    if !packed {
        return Ok(blob.to_vec());
    }
    let size = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]) as usize;
    // One pass straight into the buffer; its one byte to spare tells a longer
    // stream from a right one.
    let mut out = Vec::with_capacity(size + 1);
    let status = flate2::Decompress::new(true)
        .decompress_vec(&blob[8..], &mut out, flate2::FlushDecompress::Finish)
        .map_err(|e| Error::Corrupt(format!("zlib stream: {e}")))?;
    if status != flate2::Status::StreamEnd || out.len() != size {
        return Err(Error::Corrupt(format!("unpacked {} bytes where the header says {size}", out.len())));
    }
    Ok(out)
}

/// What a line of a table of contents names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `d`: a folder, with the md5 of its own table.
    Folder,
    /// `f`: a file, with the md5 of its stored blob.
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocEntry {
    pub path: String,
    pub kind: Kind,
    pub md5: Md5,
}

/// A table of contents, unpacked: one `path|d|md5` or `path|f|md5` line per
/// entry, full paths. Other lines are skipped, as the extractor skips them.
pub fn parse_toc(text: &[u8]) -> Vec<TocEntry> {
    text.split(|&b| b == b'\n')
        .filter_map(|line| {
            // Latin-1, as the extractor reads it; the paths are ASCII.
            let line: String = line.iter().map(|&b| b as char).collect();
            let mut parts = line.trim().split('|');
            let (path, kind, md5) = (parts.next()?, parts.next()?, parts.next()?);
            if parts.next().is_some() {
                return None;
            }
            let kind = match kind {
                "d" => Kind::Folder,
                "f" => Kind::File,
                _ => return None,
            };
            let path = path.trim().replace('\\', "/").trim_matches('/').to_string();
            Some(TocEntry { path, kind, md5: Md5::parse(md5.trim())? })
        })
        .collect()
}

/// One file of a bundle: its name as the bundle writes it and its stored blob.
pub struct Member<'a> {
    pub name: String,
    pub md5: Md5,
    pub data: &'a [u8],
}

impl Member<'_> {
    /// Its stored blob, unpacked, once it matches its md5.
    pub fn unpack(&self) -> Result<Vec<u8>, Error> {
        if Md5::of(self.data) != self.md5 {
            return Err(Error::Corrupt(format!("bundle member {} does not match its md5", self.name)));
        }
        unpack(self.data)
    }
}

const BUNDLE_MAGIC: &[u8; 8] = b"_B3NHB3N";
const RECORD: usize = 44;

/// An NB3 bundle (`.nb`), already unpacked from its `__ZN` blob: `_B3NHB3N`;
/// the member count, 24 (the name table's offset), the offsets of the records
/// and of the data (u32 each); the names (u16 length, UTF-8); then a 44-byte
/// record per member: the offset of its name in the name table (u32), the md5
/// of its stored blob (32 hex digits), its size and its offset from the data
/// offset (u32 each). A member is checked against its md5 when it is unpacked
/// ([`Member::unpack`]), so the ones already on disk cost nothing.
pub fn parse_bundle(raw: &[u8]) -> Result<Vec<Member<'_>>, Error> {
    let bad = |what: &str| Error::Corrupt(format!("bundle: {what}"));
    if raw.len() < 24 || &raw[..8] != BUNDLE_MAGIC {
        return Err(bad("not an NB3 bundle"));
    }
    let word = |at: usize| u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]) as usize;
    let (count, records, base) = (word(8), word(16), word(20));
    if count > raw.len() / RECORD || records.checked_add(count * RECORD).is_none_or(|end| end > raw.len()) {
        return Err(bad("the record table lies outside the bundle"));
    }
    let mut names = Vec::with_capacity(count);
    let mut at = 24;
    for _ in 0..count {
        let length = raw.get(at..at + 2).ok_or_else(|| bad("the name table is cut short"))?;
        let length = u16::from_le_bytes([length[0], length[1]]) as usize;
        let name = raw.get(at + 2..at + 2 + length).ok_or_else(|| bad("a name is cut short"))?;
        names.push(String::from_utf8_lossy(name).into_owned());
        at += 2 + length;
    }
    if at != records {
        return Err(bad("the name table does not end where the records begin"));
    }
    names
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            let record = &raw[records + i * RECORD..records + (i + 1) * RECORD];
            let md5 = std::str::from_utf8(&record[4..36]).ok().and_then(Md5::parse).ok_or_else(|| bad("a record's md5"))?;
            let size = u32::from_le_bytes([record[36], record[37], record[38], record[39]]) as usize;
            let offset = u32::from_le_bytes([record[40], record[41], record[42], record[43]]) as usize;
            let data = base
                .checked_add(offset)
                .and_then(|start| raw.get(start..start.checked_add(size)?))
                .ok_or_else(|| bad(&format!("{name} lies outside the bundle")))?;
            Ok(Member { name, md5, data })
        })
        .collect()
}

/// Where a table entry or a bundle member goes in the game data folder: `/`
/// separators, no leading `/`, and no `global/` before `export_win32/`. The
/// database is the one member stored that way (`global/export_win32/db/static.db4`
/// in bundle633) while the tables list it as `export_win32/db/static.db4` with
/// the same md5; the callers place a member only where the tables list its md5,
/// so the whole tree stays inside `export_win32/`. `None` for a path that could
/// leave the folder or that Windows cannot hold: `..`, a drive or stream
/// (`:`), control characters.
pub fn logical_path(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            part if part.contains(':') || part.chars().any(char::is_control) => return None,
            part => parts.push(part),
        }
    }
    if parts.len() > 2 && parts[0].eq_ignore_ascii_case("global") && parts[1] == "export_win32" {
        parts.remove(0);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// `data` stored as the CDN stores it.
    pub(crate) fn zn(data: &[u8]) -> Vec<u8> {
        let mut out = b"__ZN".to_vec();
        out.extend((data.len() as u32).to_le_bytes());
        let mut zlib = flate2::write::ZlibEncoder::new(out, flate2::Compression::default());
        zlib.write_all(data).unwrap();
        zlib.finish().unwrap()
    }

    /// An unpacked bundle of these (name, stored blob) members.
    pub(crate) fn bundle(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut names = Vec::new();
        let mut offsets = Vec::new();
        for (name, _) in members {
            offsets.push(names.len() as u32);
            names.extend((name.len() as u16).to_le_bytes());
            names.extend(name.as_bytes());
        }
        let records = 24 + names.len();
        let base = records + members.len() * RECORD;
        let mut out = BUNDLE_MAGIC.to_vec();
        for value in [members.len(), 24, records, base] {
            out.extend((value as u32).to_le_bytes());
        }
        out.extend(&names);
        let mut at = 0u32;
        for ((_, blob), name_at) in members.iter().zip(offsets) {
            out.extend(name_at.to_le_bytes());
            out.extend(Md5::of(blob).to_string().as_bytes());
            out.extend((blob.len() as u32).to_le_bytes());
            out.extend(at.to_le_bytes());
            at += blob.len() as u32;
        }
        for (_, blob) in members {
            out.extend(blob);
        }
        out
    }

    #[test]
    fn md5_reads_and_writes_the_cdns_hex() {
        let key = Md5::parse("6A8B704D041044688EBEE52DA5B48B55").unwrap();
        assert_eq!(key.to_string(), "6a8b704d041044688ebee52da5b48b55");
        assert_eq!(Md5::of(b"").to_string(), "d41d8cd98f00b204e9800998ecf8427e");
        for bad in ["", "6a8b", "6a8b704d041044688ebee52da5b48b5g", "6a8b704d041044688ebee52da5b48b5500"] {
            assert_eq!(Md5::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn stored_blobs_unpack_to_their_declared_size() {
        let data = b"export_win32/db/static.db4|f|222927579e50d7b470664938927c651c\n".repeat(50);
        assert_eq!(unpack(&zn(&data)).unwrap(), data);
        assert_eq!(unpack(b"KCAP raw member").unwrap(), b"KCAP raw member");
        let mut lying = zn(&data);
        lying[4] ^= 1;
        assert!(matches!(unpack(&lying), Err(Error::Corrupt(_))));
        let cut = zn(&data);
        assert!(unpack(&cut[..cut.len() / 2]).is_err());
    }

    #[test]
    fn tables_list_folders_and_files_and_skip_anything_else() {
        let text = b"bundles/optional|d|74e23e3e462240e9f0f511de1b04d4aa\r\n\
            export_win32/db/static.db4|f|222927579E50D7B470664938927C651C\n\
            \n# comment\nexport_win32/x|q|222927579e50d7b470664938927c651c\nbroken|f|1234\n\
            export_win32\\maps\\a b.map|f|0759abc7da59f32f33cdf6a014ec5781";
        let entries = parse_toc(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], TocEntry { path: "bundles/optional".into(), kind: Kind::Folder,
            md5: Md5::parse("74e23e3e462240e9f0f511de1b04d4aa").unwrap() });
        assert_eq!(entries[1].path, "export_win32/db/static.db4");
        assert_eq!(entries[1].kind, Kind::File);
        assert_eq!(entries[2].path, "export_win32/maps/a b.map");
    }

    #[test]
    fn bundles_give_every_member_with_its_md5_checked() {
        let map = zn(b"MAP!");
        let raw = bundle(&[("export_win32/maps/newmap_3.map", map.clone()), ("global/export_win32/db/static.db4", b"raw".to_vec())]);
        let members = parse_bundle(&raw).unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "export_win32/maps/newmap_3.map");
        assert_eq!(members[0].md5, Md5::of(&map));
        assert_eq!(members[0].unpack().unwrap(), b"MAP!");
        assert_eq!(members[1].unpack().unwrap(), b"raw");
        let mut damaged = raw.clone();
        *damaged.last_mut().unwrap() ^= 0xff;
        let members = parse_bundle(&damaged).unwrap();
        assert!(members[0].unpack().is_ok());
        assert!(matches!(members[1].unpack(), Err(Error::Corrupt(m)) if m.contains("static.db4")));
        assert!(parse_bundle(&raw[..raw.len() - 1]).is_err(), "a member past the end");
        assert!(parse_bundle(b"_B3NHB3N").is_err());
        assert!(parse_bundle(&[0; 64]).is_err());
    }

    #[test]
    fn paths_stay_inside_export_win32_and_lose_the_global_folder() {
        assert_eq!(logical_path("global/export_win32/db/static.db4").as_deref(), Some("export_win32/db/static.db4"));
        assert_eq!(logical_path("/export_win32\\ui\\./chatwindow2.bxml").as_deref(), Some("export_win32/ui/chatwindow2.bxml"));
        assert_eq!(logical_path("global/readme.txt").as_deref(), Some("global/readme.txt"), "only before export_win32");
        for bad in ["export_win32/../../evil.dll", "C:/Windows/evil.dll", "export_win32/a.n3:stream", "export_win32/a\u{1}b", "", "/"] {
            assert_eq!(logical_path(bad), None, "{bad:?}");
        }
    }
}
