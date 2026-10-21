//! The models' textures as KTX2 beside their PNGs: block-compressed (BC) with
//! their mipmaps and zstd, the format a desktop GPU samples as it is (WebGPU's
//! texture-compression-bc, every native backend).
//!
//!   dsor-ktx2 <export_win32> <assets> [--force]
//!
//! Every PNG a .glb references under textures/ gets textures/<same>.ktx2:
//! - the game's own DXT1/DXT3/DXT5 blocks and mips, copied, when the PNG is
//!   still that DDS decoded (no quality lost, no re-encoding); a DDS without
//!   mips keeps its top level and gets the lower levels encoded alike;
//! - otherwise (normal maps rebuilt from bump maps, roughness maps, CRN without
//!   a transcoded DDS, uncompressed DDS, PNGs a later tool rewrote) BC7 encoded
//!   from the PNG, with a mip chain.
//! A texture whose size is not a multiple of 4 stays PNG only (WebGPU refuses
//! such a BC texture).
//!
//! textures/ktx2.list then names every packed PNG, one asset path per line: the
//! client (rendering/model_variant.rs) points a model at the .ktx2 only for
//! those, and only when the GPU samples BC; the PNGs stay for any other GPU.
//! Rerun after a tool rewrites textures: a .ktx2 older than its PNG is redone.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

const VK_BC1_RGBA_UNORM: u32 = 133;
const VK_BC2_UNORM: u32 = 135;
const VK_BC3_UNORM: u32 = 137;
const VK_BC7_UNORM: u32 = 145;
const ZSTD_LEVEL: i32 = 15;

#[derive(Clone, Copy)]
enum Bc {
    Bc1,
    Bc2,
    Bc3,
    Bc7,
}

impl Bc {
    fn block_bytes(self) -> usize {
        if matches!(self, Bc::Bc1) { 8 } else { 16 }
    }
    fn vk_format(self) -> u32 {
        match self { Bc::Bc1 => VK_BC1_RGBA_UNORM, Bc::Bc2 => VK_BC2_UNORM, Bc::Bc3 => VK_BC3_UNORM, Bc::Bc7 => VK_BC7_UNORM }
    }
    fn texpresso(self) -> Option<texpresso::Format> {
        match self { Bc::Bc1 => Some(texpresso::Format::Bc1), Bc::Bc2 => Some(texpresso::Format::Bc2),
            Bc::Bc3 => Some(texpresso::Format::Bc3), Bc::Bc7 => None }
    }
    fn level_bytes(self, w: u32, h: u32) -> usize {
        (w.div_ceil(4) * h.div_ceil(4)) as usize * self.block_bytes()
    }
}

struct Packed {
    format: Bc,
    width: u32,
    height: u32,
    levels: Vec<Vec<u8>>,
}

enum Outcome {
    Copied,
    Encoded,
    Kept,
    Skipped(String),
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [export, assets] = paths.as_slice() else {
        eprintln!("usage: dsor-ktx2 <export_win32> <assets> [--force]");
        std::process::exit(2);
    };
    let (export, assets) = (Path::new(export.as_str()), Path::new(assets.as_str()));
    let textures = referenced_textures(assets);
    println!("[ktx2] {} model textures", textures.len());
    let done = AtomicUsize::new(0);
    let results: Vec<(String, Outcome)> = textures.par_iter().map(|rel| {
        let outcome = pack_one(export, assets, rel, force).unwrap_or_else(Outcome::Skipped);
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        if n % 2000 == 0 { println!("[ktx2] {n}/{}", textures.len()); }
        (rel.clone(), outcome)
    }).collect();
    let mut list = String::new();
    let (mut copied, mut encoded, mut kept, mut skipped) = (0, 0, 0, 0);
    for (rel, outcome) in &results {
        match outcome {
            Outcome::Copied => copied += 1,
            Outcome::Encoded => encoded += 1,
            Outcome::Kept => kept += 1,
            Outcome::Skipped(why) => {
                skipped += 1;
                println!("[ktx2] png only: {rel}: {why}");
                continue;
            }
        }
        list.push_str(rel);
        list.push('\n');
    }
    std::fs::write(assets.join("textures/ktx2.list"), list).expect("write textures/ktx2.list");
    println!("[ktx2] game blocks copied {copied}, BC7 encoded {encoded}, up to date {kept}, png only {skipped}");
}

/// The textures/**.png the models reference, as asset paths.
fn referenced_textures(assets: &Path) -> BTreeSet<String> {
    let mut glbs = Vec::new();
    collect_glbs(assets, assets, &mut glbs);
    glbs.par_iter().flat_map_iter(|glb| {
        let mut found = Vec::new();
        let Ok(bytes) = std::fs::read(assets.join(glb)) else { return found };
        let Some(doc) = glb_json(&bytes) else { return found };
        let dir = glb.parent().unwrap_or(Path::new(""));
        for image in doc["images"].as_array().into_iter().flatten() {
            let Some(uri) = image["uri"].as_str() else { continue };
            if uri.starts_with("data:") || !uri.ends_with(".png") { continue; }
            if let Some(path) = normalize(&dir.join(uri)) {
                if path.starts_with("textures/") { found.push(path); }
            }
        }
        found
    }).collect()
}

fn collect_glbs(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if dir == root && matches!(entry.file_name().to_str(), Some("textures" | "logs" | "interface" | "fonts")) { continue; }
            collect_glbs(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "glb") {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

fn glb_json(bytes: &[u8]) -> Option<serde_json::Value> {
    let word = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    if word(0)? != 0x46546c67 || word(16)? != 0x4e4f534a { return None; }
    serde_json::from_slice(bytes.get(20..20 + word(12)? as usize)?).ok()
}

/// "a/b/../textures/x.png" -> "a/textures/x.png"; None if it leaves the root.
fn normalize(path: &Path) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::ParentDir => { parts.pop()?; }
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

fn pack_one(export: &Path, assets: &Path, rel: &str, force: bool) -> Result<Outcome, String> {
    let png_path = assets.join(rel);
    let out_path = png_path.with_extension("ktx2");
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if !force {
        if let (Some(out), Some(png)) = (modified(&out_path), modified(&png_path)) {
            if out >= png { return Ok(Outcome::Kept); }
        }
    }
    let image = image::open(&png_path).map_err(|e| e.to_string())?.to_rgba8();
    let (width, height) = image.dimensions();
    if width % 4 != 0 || height % 4 != 0 {
        return Err(format!("{width}x{height} is not a multiple of 4"));
    }
    let rgba = image.into_raw();
    let stem = rel.strip_prefix("textures/").unwrap().strip_suffix(".png").unwrap();
    let (packed, copied) = match game_blocks(export, stem, width, height, &rgba) {
        Some(packed) => (packed, true),
        None => (encode_bc7(width, height, rgba), false),
    };
    std::fs::write(&out_path, write_ktx2(&packed)).map_err(|e| e.to_string())?;
    Ok(if copied { Outcome::Copied } else { Outcome::Encoded })
}

/// The DDS this PNG was decoded from: export_win32/textures/<stem>.dds, or the
/// DDS DSO_Godot transcoded a <stem>.crn into (dsoexp/resources.py crn_to_dds).
fn source_dds(export: &Path, stem: &str) -> Option<Vec<u8>> {
    let base = export.join("textures").join(stem);
    if let Ok(bytes) = std::fs::read(base.with_extension("dds")) { return Some(bytes); }
    let crn = std::fs::canonicalize(base.with_extension("crn")).ok()?;
    let key = crn.to_str()?.trim_start_matches('/').replace('/', "__");
    std::fs::read(std::env::temp_dir().join("dso_crn_cache").join(format!("{key}.dds"))).ok()
}

/// The game's own blocks, when the PNG is that DDS's top level decoded.
fn game_blocks(export: &Path, stem: &str, width: u32, height: u32, rgba: &[u8]) -> Option<Packed> {
    let dds = source_dds(export, stem)?;
    let word = |at: usize| dds.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    if dds.get(..4)? != b"DDS " || word(112)? & 0x200 != 0 || word(80)? & 4 == 0 { return None; }
    let format = match dds.get(84..88)? {
        b"DXT1" => Bc::Bc1,
        b"DXT2" | b"DXT3" => Bc::Bc2,
        b"DXT4" | b"DXT5" => Bc::Bc3,
        _ => return None,
    };
    if (word(16)?, word(12)?) != (width, height) { return None; }
    let mut levels = Vec::new();
    let mut at = 128;
    for level in 0..word(28)?.max(1) {
        let (w, h) = ((width >> level).max(1), (height >> level).max(1));
        let Some(data) = dds.get(at..at + format.level_bytes(w, h)) else { break };
        levels.push(data.to_vec());
        at += data.len();
        if w == 1 && h == 1 { break; }
    }
    // Still the DDS? Decoders round the interpolated colours differently by a
    // step or two; a rewritten PNG differs by far more, or in many texels.
    let codec = format.texpresso()?;
    let mut decoded = vec![0u8; rgba.len()];
    codec.decompress(levels.first()?, width as usize, height as usize, &mut decoded);
    let off = decoded.chunks_exact(4).zip(rgba.chunks_exact(4))
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
    if off > (width * height / 1000) as usize { return None; }
    if levels.len() == 1 && (width > 1 || height > 1) {
        // No mips in the game file: the lower levels encoded in its own format.
        let mut level = rgba.to_vec();
        let (mut w, mut h) = (width, height);
        while w > 1 || h > 1 {
            (level, w, h) = half(&level, w, h);
            let mut blocks = vec![0u8; codec.compressed_size(w as usize, h as usize)];
            codec.compress(&level, w as usize, h as usize, texpresso::Params::default(), &mut blocks);
            levels.push(blocks);
        }
    }
    Some(Packed { format, width, height, levels })
}

fn encode_bc7(width: u32, height: u32, rgba: Vec<u8>) -> Packed {
    let opaque = rgba.chunks_exact(4).all(|p| p[3] == 255);
    let settings = if opaque { intel_tex_2::bc7::opaque_basic_settings() } else { intel_tex_2::bc7::alpha_basic_settings() };
    let mut levels = Vec::new();
    let (mut level, mut w, mut h) = (rgba, width, height);
    loop {
        let (padded, pw, ph) = pad4(&level, w, h);
        levels.push(intel_tex_2::bc7::compress_blocks(&settings,
            &intel_tex_2::RgbaSurface { data: &padded, width: pw, height: ph, stride: pw * 4 }));
        if w == 1 && h == 1 { break; }
        (level, w, h) = half(&level, w, h);
    }
    Packed { format: Bc::Bc7, width, height, levels }
}

/// The next mip level: each 2x2 texels averaged (1 wide or high: 2x1).
fn half(rgba: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let mut sum = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(w - 1);
                    let sy = (y * 2 + dy).min(h - 1);
                    sum += rgba[((sy * w + sx) * 4 + c) as usize] as u32;
                }
                out[((y * nw + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
            }
        }
    }
    (out, nw, nh)
}

/// The level grown to whole 4x4 blocks, its edge texels repeated.
fn pad4(rgba: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
    if (pw, ph) == (w, h) { return (rgba.to_vec(), w, h); }
    let mut out = vec![0u8; (pw * ph * 4) as usize];
    for y in 0..ph {
        for x in 0..pw {
            let from = ((y.min(h - 1) * w + x.min(w - 1)) * 4) as usize;
            let to = ((y * pw + x) * 4) as usize;
            out[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
        }
    }
    (out, pw, ph)
}

/// A KTX 2.0 file: one 2D texture, its levels zstd-compressed (scheme 2), the
/// level data stored smallest first, a basic data format descriptor.
/// The vkFormat is the UNORM one; the client reads it as sRGB where the
/// material says so (bevy_image ktx2_format_to_texture_format, is_srgb).
fn write_ktx2(packed: &Packed) -> Vec<u8> {
    let levels: Vec<Vec<u8>> = packed.levels.iter().map(|l| zstd::bulk::compress(l, ZSTD_LEVEL).unwrap()).collect();
    let dfd = basic_dfd(packed.format);
    let level_index_at = 80;
    let dfd_at = level_index_at + 24 * levels.len();
    let data_at = dfd_at + dfd.len();
    let mut out = Vec::with_capacity(data_at + levels.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(&[0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n']);
    for value in [packed.format.vk_format(), 1, packed.width, packed.height, 0, 0, 1, levels.len() as u32, 2] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for value in [dfd_at as u32, dfd.len() as u32, 0, 0] { out.extend_from_slice(&value.to_le_bytes()); }
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    // Offsets: the smallest level first in the file.
    let mut offsets = vec![0usize; levels.len()];
    let mut at = data_at;
    for i in (0..levels.len()).rev() {
        offsets[i] = at;
        at += levels[i].len();
    }
    for (i, level) in levels.iter().enumerate() {
        out.extend_from_slice(&(offsets[i] as u64).to_le_bytes());
        out.extend_from_slice(&(level.len() as u64).to_le_bytes());
        out.extend_from_slice(&(packed.levels[i].len() as u64).to_le_bytes());
    }
    out.extend_from_slice(&dfd);
    for level in levels.iter().rev() { out.extend_from_slice(level); }
    out
}

/// KHR_DF basic block for a BC format: 4x4 texel blocks, linear transfer.
fn basic_dfd(format: Bc) -> Vec<u8> {
    // (colour model, samples as (bit offset, bit length, channel id)).
    let (model, samples): (u8, &[(u16, u8, u8)]) = match format {
        Bc::Bc1 => (128, &[(0, 64, 0)]),
        Bc::Bc2 => (129, &[(0, 64, 15), (64, 64, 0)]),
        Bc::Bc3 => (130, &[(0, 64, 15), (64, 64, 0)]),
        Bc::Bc7 => (134, &[(0, 128, 0)]),
    };
    let block_size = 24 + 16 * samples.len();
    let mut out = Vec::with_capacity(4 + block_size);
    out.extend_from_slice(&((4 + block_size) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // vendor 0 (Khronos), type 0 (basic)
    out.extend_from_slice(&(2u32 | (block_size as u32) << 16).to_le_bytes()); // version 2
    out.extend_from_slice(&[model, 1, 1, 0]); // BT.709 primaries, linear, straight alpha
    out.extend_from_slice(&[3, 3, 0, 0]); // 4x4x1x1 texels per block
    out.extend_from_slice(&[format.block_bytes() as u8, 0, 0, 0, 0, 0, 0, 0]);
    for &(offset, length, channel) in samples {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&[length - 1, channel]);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&u32::MAX.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// bevy_image's own KTX2 reader takes the file: header, DFD, every level
    /// back to its blocks, the largest first.
    #[test]
    fn bevy_reads_what_is_written() {
        let rgba: Vec<u8> = (0..8 * 8 * 4).map(|i| (i * 7) as u8).collect();
        for packed in [encode_bc7(8, 8, rgba.clone()), Packed { format: Bc::Bc1, width: 8, height: 8,
            levels: vec![vec![1; 32], vec![2; 8], vec![3; 8], vec![4; 8]] }] {
            let file = write_ktx2(&packed);
            let reader = ktx2::Reader::new(&file[..]).expect("a valid KTX2");
            let header = reader.header();
            assert_eq!(header.format.map(|f| f.value()), Some(packed.format.vk_format()));
            assert_eq!((header.pixel_width, header.pixel_height, header.level_count), (8, 8, packed.levels.len() as u32));
            assert_eq!(header.supercompression_scheme, Some(ktx2::SupercompressionScheme::Zstandard));
            assert!(reader.basic_dfd().is_some());
            let levels: Vec<Vec<u8>> = reader.levels().map(|l| zstd::decode_all(l.data).unwrap()).collect();
            assert_eq!(levels, packed.levels);
        }
        assert_eq!(encode_bc7(8, 8, rgba).levels.iter().map(Vec::len).collect::<Vec<_>>(), [64, 16, 16, 16]);
    }
}
