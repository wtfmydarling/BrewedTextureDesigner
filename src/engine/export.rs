//! Export of Output nodes to image files.
//!
//! The GPU image is read back on the UI thread (fast), then channel selection,
//! quantization (multithreaded SIMD) and PNG encoding run on a worker thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::graph::defs::output_format_channels;
use crate::graph::{Graph, NodeKind};
use crate::simd;

use super::Engine;

#[derive(Debug)]
pub enum ExportMessage {
    Saved(PathBuf),
    Failed(String),
}

/// Make an output name safe to use as a file name.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim_matches(|c| c == '.' || c == ' ').to_string();
    if cleaned.is_empty() { "output".to_string() } else { cleaned }
}

/// Which channels end up in the file for an output format. Two channel images are
/// written as RGB with an empty blue channel since PNG has no RG layout.
fn file_channels(format: u32) -> &'static [usize] {
    match output_format_channels(format).len() {
        4 => &[0, 1, 2, 3],
        2 | 3 => &[0, 1, 2],
        // Single channel formats are stored as gray in the output node image.
        _ => &[0],
    }
}

/// Encode already formatted RGBA f32 pixels to a PNG file.
pub fn write_png(path: &Path, rgba: &[f32], width: u32, height: u32, format: u32, sixteen_bit: bool) -> Result<(), String> {
    let channels = file_channels(format);
    let err = |e: image::ImageError| format!("Cannot write '{}': {e}", path.display());
    if sixteen_bit {
        let mut q = vec![0u16; rgba.len()];
        simd::quantize_u16(rgba, &mut q);
        let data = simd::select_channels(&q, channels);
        match channels.len() {
            4 => image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
            3 => image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
            _ => image::ImageBuffer::<image::Luma<u16>, _>::from_raw(width, height, data).map(|b| b.save(path)),
        }
        .ok_or("image buffer size mismatch")?
        .map_err(err)
    } else {
        let mut q = vec![0u8; rgba.len()];
        simd::quantize_u8(rgba, &mut q);
        let data = simd::select_channels(&q, channels);
        match channels.len() {
            4 => image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, data).map(|b| b.save(path)),
            3 => image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(width, height, data).map(|b| b.save(path)),
            _ => image::ImageBuffer::<image::Luma<u8>, _>::from_raw(width, height, data).map(|b| b.save(path)),
        }
        .ok_or("image buffer size mismatch")?
        .map_err(err)
    }
}

/// Start exporting every Output node of `graph` into `dir`. Returns the number of
/// files being written; progress arrives through `tx`.
pub fn export_outputs(engine: &Engine, graph: &Graph, dir: &Path, tx: &Sender<ExportMessage>) -> usize {
    let mut started = 0;
    let mut used_names: HashSet<String> = HashSet::new();
    for node in graph.nodes.iter().filter(|n| n.kind == NodeKind::Output) {
        let base = sanitize_file_name(node.text("name"));
        let mut name = base.clone();
        let mut n = 2;
        while !used_names.insert(name.to_lowercase()) {
            name = format!("{base}_{n}");
            n += 1;
        }
        let state = engine.state(node.id);
        let format = state
            .and_then(|s| s.driven.get("format"))
            .map_or(node.enum_value("format"), |v| v.as_f32() as u32);
        let sixteen = node.enum_value("bit_depth") == 1;
        let Some(image) = engine.image(node.id) else {
            let _ = tx.send(ExportMessage::Failed(format!("Output '{name}' has no image")));
            continue;
        };
        if graph.link_into(node.id, "i:input").is_none() {
            let _ = tx.send(ExportMessage::Failed(format!("Output '{name}' is not connected")));
            continue;
        }
        let rgba = match engine.gpu.read_image_blocking(image) {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(ExportMessage::Failed(e));
                continue;
            }
        };
        let (w, h) = (image.width(), image.height());
        let path = dir.join(format!("{name}.png"));
        let tx = tx.clone();
        started += 1;
        std::thread::spawn(move || {
            let msg = match write_png(&path, &rgba, w, h, format, sixteen) {
                Ok(()) => ExportMessage::Saved(path),
                Err(e) => ExportMessage::Failed(e),
            };
            let _ = tx.send(msg);
        });
    }
    started
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
    }

    #[test]
    fn png_roundtrip_formats() {
        let dir = std::env::temp_dir().join(format!("ferrotex_png_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (5u32, 3u32);
        let rgba: Vec<f32> = (0..w * h * 4).map(|i| (i % 7) as f32 / 6.0).collect();
        for (format, sixteen, expected) in [(0, false, 4u8), (1, false, 3), (2, true, 3), (3, true, 1)] {
            let path = dir.join(format!("f{format}.png"));
            write_png(&path, &rgba, w, h, format, sixteen).unwrap();
            let img = image::open(&path).unwrap();
            assert_eq!((img.width(), img.height()), (w, h));
            assert_eq!(img.color().channel_count(), expected);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
