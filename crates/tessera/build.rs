//! Embeds the application icon (drawn by src/brand/art.rs) and the version information in
//! the Windows executable. The resource file is written directly and handed to the
//! linker, so no resource compiler is needed.

#[path = "src/brand/art.rs"]
#[allow(dead_code)]
mod art;

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/brand/art.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    // link.exe takes .res files directly; other linkers would need a converted object.
    if os != "windows" || env != "msvc" {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let path = out.join("tessera.res");
    std::fs::write(&path, resources()).expect("write the Windows resources");
    println!("cargo:rustc-link-arg-bins={}", path.display());
}

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const LANGUAGE: u16 = 0x0409;
/// Id of the icon group; src/brand/mod.rs loads it by this id.
const ICON_GROUP: u16 = 1;

fn resources() -> Vec<u8> {
    let mut res = Vec::new();
    // A .res file starts with an empty entry.
    entry(&mut res, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend(0u16.to_le_bytes()); // reserved
    group.extend(1u16.to_le_bytes()); // type: icon
    group.extend((art::ICON_SIZES.len() as u16).to_le_bytes());
    for (i, &size) in art::ICON_SIZES.iter().enumerate() {
        let id = i as u16 + 1;
        let dib = icon_dib(size, &art::icon_rgba(size));
        entry(&mut res, RT_ICON, id, 0x1010, &dib);
        let side = if size >= 256 { 0 } else { size as u8 };
        group.extend([side, side, 0, 0]);
        group.extend(1u16.to_le_bytes()); // planes
        group.extend(32u16.to_le_bytes()); // bits per pixel
        group.extend((dib.len() as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    entry(&mut res, RT_GROUP_ICON, ICON_GROUP, 0x1030, &group);
    entry(&mut res, RT_VERSION, 1, 0x0030, &version_info());
    res
}

/// One resource: header with numeric type and name, then the data, DWORD aligned.
fn entry(res: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    res.extend((data.len() as u32).to_le_bytes());
    res.extend(32u32.to_le_bytes()); // header size
    res.extend([0xFF, 0xFF]);
    res.extend(kind.to_le_bytes());
    res.extend([0xFF, 0xFF]);
    res.extend(name.to_le_bytes());
    res.extend(0u32.to_le_bytes()); // data version
    res.extend(flags.to_le_bytes());
    res.extend(if kind == 0 { 0u16 } else { LANGUAGE }.to_le_bytes());
    res.extend(0u32.to_le_bytes()); // version
    res.extend(0u32.to_le_bytes()); // characteristics
    res.extend(data);
    pad(res);
}

fn pad(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
}

/// A 32-bit icon image: BITMAPINFOHEADER, BGRA rows bottom-up, then an empty AND mask
/// (the alpha channel does the masking).
fn icon_dib(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut v = Vec::new();
    v.extend(40u32.to_le_bytes());
    v.extend((size as i32).to_le_bytes());
    v.extend((2 * size as i32).to_le_bytes()); // image and mask
    v.extend(1u16.to_le_bytes());
    v.extend(32u16.to_le_bytes());
    v.extend(0u32.to_le_bytes()); // BI_RGB
    v.extend((size * size * 4 + mask_row * size).to_le_bytes());
    v.extend([0u8; 16]); // resolution, palette
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            v.extend([rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    v.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    v
}

enum Value<'a> {
    None,
    Binary(&'a [u8]),
    Text(&'a str),
}

/// A VS_VERSIONINFO block: length, value length, type, key, value, children.
fn block(key: &str, value: Value<'_>, children: &[Vec<u8>]) -> Vec<u8> {
    let utf16 = |s: &str| s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
    let (value_len, kind, bytes) = match value {
        Value::None => (0u16, 1u16, Vec::new()),
        Value::Binary(b) => (b.len() as u16, 0, b.to_vec()),
        Value::Text(t) => ((t.encode_utf16().count() + 1) as u16, 1, utf16(t)),
    };
    let mut v = vec![0, 0];
    v.extend(value_len.to_le_bytes());
    v.extend(kind.to_le_bytes());
    v.extend(utf16(key));
    pad(&mut v);
    v.extend(bytes);
    for child in children {
        pad(&mut v);
        v.extend(child);
    }
    let len = v.len() as u16;
    v[..2].copy_from_slice(&len.to_le_bytes());
    v
}

fn version_info() -> Vec<u8> {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let part = |i: usize| version.split('.').nth(i).and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    let (major, minor, patch) = (part(0), part(1), part(2));
    let mut fixed = Vec::new();
    for word in [
        0xFEEF_04BD,               // signature
        0x0001_0000,               // structure version
        (major << 16) | minor,     // file version
        patch << 16,
        (major << 16) | minor,     // product version
        patch << 16,
        0x3F,                      // flags mask
        0,                         // flags
        0x0004_0004,               // VOS_NT_WINDOWS32
        1,                         // VFT_APP
        0,
        0,
        0,
    ] {
        fixed.extend(u32::to_le_bytes(word));
    }
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", "Tessera"),
        ("FileDescription", "Tessera"),
        ("FileVersion", version.as_str()),
        ("InternalName", "tessera"),
        ("OriginalFilename", "tessera.exe"),
        ("ProductName", "Tessera"),
        ("ProductVersion", version.as_str()),
    ]
    .iter()
    .map(|(k, v)| block(k, Value::Text(v), &[]))
    .collect();
    let table = block("040904B0", Value::None, &strings);
    let string_info = block("StringFileInfo", Value::None, &[table]);
    let translation = [0x09u8, 0x04, 0xB0, 0x04]; // English (US), Unicode
    let var = block("Translation", Value::Binary(&translation), &[]);
    let var_info = block("VarFileInfo", Value::None, &[var]);
    block("VS_VERSION_INFO", Value::Binary(&fixed), &[string_info, var_info])
}
