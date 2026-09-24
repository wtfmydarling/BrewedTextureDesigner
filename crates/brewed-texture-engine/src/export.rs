//! Writing Output nodes to image files: PNG (8 / 16 bit) and OpenEXR (32 bit float).
//! Channel selection and quantization run multithreaded with SIMD.

use std::path::{Path, PathBuf};

use brewed_texture_core::nodes::output_format_channels;

use crate::gpu::StoredImage;
use crate::simd;

/// Bit depth choices of the Output node ("bit_depth" parameter).
pub const DEPTH_8: u32 = 0;
pub const DEPTH_16: u32 = 1;
pub const DEPTH_32F: u32 = 2;

/// Make an output name safe to use as a file name.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String =
        name.trim().chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim_matches(['.', ' ']);
    // Short enough for every file system (255 bytes), with a number and the extension.
    let mut end = cleaned.len().min(200);
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    let cleaned = cleaned[..end].trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        return "output".to_string();
    }
    // Names Windows keeps for devices, whatever follows the first dot: a file of that name
    // cannot be written there (before Windows 11). Everywhere the same name, for the same
    // files wherever a project is exported.
    const DEVICES: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
        "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = cleaned.split('.').next().unwrap_or_default();
    if DEVICES.iter().any(|d| d.eq_ignore_ascii_case(stem.trim_end())) {
        return format!("{stem}_{}", &cleaned[stem.len()..]);
    }
    cleaned.to_string()
}

/// File extension for a bit depth.
pub fn extension(bit_depth: u32) -> &'static str {
    if bit_depth == DEPTH_32F { "exr" } else { "png" }
}

/// Unique file paths `dir/name.ext` for a list of (name, bit depth); duplicates get a
/// numeric suffix (names are compared case-insensitively, like on Windows).
pub fn output_paths(dir: &Path, outputs: &[(String, u32)]) -> Vec<PathBuf> {
    let mut used = std::collections::HashSet::new();
    outputs
        .iter()
        .map(|(name, depth)| {
            let base = sanitize_file_name(name);
            let mut candidate = base.clone();
            let mut n = 2;
            while !used.insert(format!("{}.{}", candidate.to_lowercase(), extension(*depth))) {
                candidate = format!("{base}_{n}");
                n += 1;
            }
            dir.join(format!("{candidate}.{}", extension(*depth)))
        })
        .collect()
}

/// Which channels end up in the file for an output format. PNG has no two channel
/// layout, so RG is written as RGB with an empty blue channel.
fn file_channels(format: u32) -> &'static [usize] {
    match output_format_channels(format).len() {
        4 => &[0, 1, 2, 3],
        2 | 3 => &[0, 1, 2],
        // Single channel formats are stored as gray in the output node image.
        _ => &[0],
    }
}

/// Encode already formatted RGBA f32 pixels.
pub fn write_image(
    path: &Path,
    rgba: &[f32],
    width: u32,
    height: u32,
    format: u32,
    bit_depth: u32,
) -> Result<(), String> {
    write_stored(path, &StoredImage::from_f32(width, height, rgba), format, bit_depth)
}

/// Encode an image as it is stored (an output read back), converted a band of rows at a
/// time: an export takes the memory of its image as the GPU has it and of its file's
/// samples, and no more (not the image as RGBA floats).
pub fn write_stored(path: &Path, image: &StoredImage, format: u32, bit_depth: u32) -> Result<(), String> {
    write_in_bands(path, image, format, bit_depth, (simd::BAND / image.width.max(1)).max(1))
}

/// The samples of a file: the channels it keeps of the pixels, as `quantize` makes them,
/// converted `rows` at a time.
fn samples<T: Copy + Default + Send + Sync>(
    image: &StoredImage,
    channels: &[usize],
    rows: u32,
    quantize: fn(&[f32], &mut [T]),
) -> Vec<T> {
    let mut out = Vec::with_capacity(image.width as usize * image.height as usize * channels.len());
    let mut q = Vec::new();
    for y in (0..image.height).step_by(rows as usize) {
        let rgba = image.rows_to_f32(y..(y + rows).min(image.height));
        q.clear();
        q.resize(rgba.len(), T::default());
        quantize(&rgba, &mut q);
        out.extend(simd::select_channels(&q, channels));
    }
    out
}

fn write_in_bands(path: &Path, image: &StoredImage, format: u32, bit_depth: u32, rows: u32) -> Result<(), String> {
    let (width, height) = (image.width, image.height);
    let channels = file_channels(format);
    let err = |e: image::ImageError| format!("Cannot write '{}': {e}", path.display());
    let size_err = || "image buffer size mismatch".to_string();
    match bit_depth {
        DEPTH_32F => {
            // OpenEXR stores RGB or RGBA; single channels are replicated to gray.
            let keep: &[usize] = if channels.len() == 4 { &[0, 1, 2, 3] } else { &[0, 1, 2] };
            let data = samples(image, keep, rows, |s, d| d.copy_from_slice(s));
            let img = if channels.len() == 4 {
                image::DynamicImage::ImageRgba32F(
                    image::ImageBuffer::from_raw(width, height, data).ok_or_else(size_err)?,
                )
            } else {
                image::DynamicImage::ImageRgb32F(
                    image::ImageBuffer::from_raw(width, height, data).ok_or_else(size_err)?,
                )
            };
            img.save_with_format(path, image::ImageFormat::OpenExr).map_err(err)
        }
        DEPTH_16 => {
            let data = samples(image, channels, rows, simd::quantize_u16);
            match channels.len() {
                4 => image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
                3 => image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
                _ => image::ImageBuffer::<image::Luma<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
            }
            .ok_or_else(size_err)?
            .map_err(err)
        }
        _ => {
            let data = samples(image, channels, rows, simd::quantize_u8);
            match channels.len() {
                4 => image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, data).map(|b| b.save(path)),
                3 => image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(width, height, data).map(|b| b.save(path)),
                _ => image::ImageBuffer::<image::Luma<u8>, _>::from_raw(width, height, data).map(|b| b.save(path)),
            }
            .ok_or_else(size_err)?
            .map_err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(sanitize_file_name("base color"), "base color");
        assert_eq!(sanitize_file_name("a/b:c*"), "a_b_c_");
        assert_eq!(sanitize_file_name("  "), "output");
        assert_eq!(sanitize_file_name("..."), "output");
        // Device names of Windows.
        assert_eq!(sanitize_file_name("aux"), "aux_");
        assert_eq!(sanitize_file_name("CON.map"), "CON_.map");
        assert_eq!(sanitize_file_name("Com1 "), "Com1_");
        assert_eq!(sanitize_file_name("console"), "console");
        assert_eq!(sanitize_file_name("nul_map"), "nul_map");
        // Long names, cut where a character ends.
        let long = sanitize_file_name(&"я".repeat(300));
        assert!(long.len() <= 200 && long.chars().all(|c| c == 'я'), "{}", long.len());
        assert_eq!(sanitize_file_name(&format!("{}. .x", "a".repeat(199))), "a".repeat(199));
        let paths = output_paths(Path::new("d"), &[("a".into(), 0), ("A".into(), 0), ("a".into(), 2)]);
        let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["a.png", "A_2.png", "a.exr"]);
    }

    #[test]
    fn stored_images_write_as_their_floats_do() {
        use wgpu::TextureFormat as F;
        let dir = std::env::temp_dir().join(format!("brewed_stored_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (37u32, 23u32);
        let mut seed = 1u32;
        let mut value = || {
            seed = brewed_texture_core::expr::builtins::pcg_hash(seed);
            seed as f32 / u32::MAX as f32 * 1.4 - 0.2
        };
        // Every way an image is stored, written five rows at a time, makes the file its RGBA
        // floats make.
        for format in [F::R8Unorm, F::Rg8Unorm, F::Rgba8Unorm, F::R16Float, F::Rg16Float, F::Rgba16Float, F::R32Float] {
            let values = (w * h * format.components() as u32) as usize;
            let bytes: Vec<u8> = match format.block_copy_size(None).unwrap() / format.components() as u32 {
                1 => (0..values).map(|_| (value().clamp(0.0, 1.0) * 255.0) as u8).collect(),
                2 => (0..values).flat_map(|_| half::f16::from_f32(value()).to_bits().to_ne_bytes()).collect(),
                _ => (0..values).flat_map(|_| value().to_ne_bytes()).collect(),
            };
            let stored = StoredImage { width: w, height: h, format, bytes };
            let rgba = stored.to_f32();
            for depth in [DEPTH_8, DEPTH_16, DEPTH_32F] {
                for channels in 0..7 {
                    let ext = extension(depth);
                    let (a, b) = (dir.join(format!("a.{ext}")), dir.join(format!("b.{ext}")));
                    write_image(&a, &rgba, w, h, channels, depth).unwrap();
                    write_in_bands(&b, &stored, channels, depth, 5).unwrap();
                    let same = std::fs::read(&a).unwrap() == std::fs::read(&b).unwrap();
                    assert!(same, "{format:?}, format {channels}, depth {depth}");
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_formats() {
        let dir = std::env::temp_dir().join(format!("brewed_export_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (5u32, 3u32);
        let rgba: Vec<f32> = (0..w * h * 4).map(|i| (i % 7) as f32 / 6.0).collect();
        for (format, depth, expected) in [(0, DEPTH_8, 4u8), (1, DEPTH_8, 3), (2, DEPTH_16, 3), (3, DEPTH_16, 1)] {
            let path = dir.join(format!("f{format}.png"));
            write_image(&path, &rgba, w, h, format, depth).unwrap();
            let img = image::open(&path).unwrap();
            assert_eq!((img.width(), img.height()), (w, h));
            assert_eq!(img.color().channel_count(), expected);
        }
        // Whatever an output is called, its file is written.
        for name in ["aux", "nul.height", &"я".repeat(300), &"b".repeat(1000)] {
            let path = output_paths(&dir, &[(name.to_string(), DEPTH_8)]).remove(0);
            write_image(&path, &rgba, w, h, 0, DEPTH_8).unwrap();
            assert!(image::open(&path).is_ok(), "{}", path.display());
        }
        let exr = dir.join("f.exr");
        write_image(&exr, &rgba, w, h, 0, DEPTH_32F).unwrap();
        let back = image::open(&exr).unwrap().into_rgba32f();
        assert!((back.as_raw()[5] - rgba[5]).abs() < 1e-6, "EXR keeps full precision");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
