//! Writing Output nodes to image files: PNG (8 / 16 bit) and OpenEXR (32 bit float).
//! Channel selection and quantization run multithreaded with SIMD.

use std::path::{Path, PathBuf};

use tessera_core::nodes::output_format_channels;

use crate::simd;

/// Bit depth choices of the Output node ("bit_depth" parameter).
pub const DEPTH_8: u32 = 0;
pub const DEPTH_16: u32 = 1;
pub const DEPTH_32F: u32 = 2;

/// Make an output name safe to use as a file name.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String =
        name.trim().chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim_matches(|c| c == '.' || c == ' ').to_string();
    if cleaned.is_empty() { "output".to_string() } else { cleaned }
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
    let channels = file_channels(format);
    let err = |e: image::ImageError| format!("Cannot write '{}': {e}", path.display());
    let size_err = || "image buffer size mismatch".to_string();
    match bit_depth {
        DEPTH_32F => {
            // OpenEXR stores RGB or RGBA; single channels are replicated to gray.
            let data = if channels.len() == 4 { rgba.to_vec() } else { simd::select_channels(rgba, &[0, 1, 2]) };
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
            let mut q = vec![0u16; rgba.len()];
            simd::quantize_u16(rgba, &mut q);
            let data = simd::select_channels(&q, channels);
            match channels.len() {
                4 => image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
                3 => image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
                _ => image::ImageBuffer::<image::Luma<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
            }
            .ok_or_else(size_err)?
            .map_err(err)
        }
        _ => {
            let mut q = vec![0u8; rgba.len()];
            simd::quantize_u8(rgba, &mut q);
            let data = simd::select_channels(&q, channels);
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
        let paths = output_paths(Path::new("d"), &[("a".into(), 0), ("A".into(), 0), ("a".into(), 2)]);
        let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["a.png", "A_2.png", "a.exr"]);
    }

    #[test]
    fn roundtrip_formats() {
        let dir = std::env::temp_dir().join(format!("tessera_export_{}", std::process::id()));
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
        let exr = dir.join("f.exr");
        write_image(&exr, &rgba, w, h, 0, DEPTH_32F).unwrap();
        let back = image::open(&exr).unwrap().into_rgba32f();
        assert!((back.as_raw()[5] - rgba[5]).abs() < 1e-6, "EXR keeps full precision");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
