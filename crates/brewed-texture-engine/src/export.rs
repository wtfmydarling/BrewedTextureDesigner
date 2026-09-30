//! Writing Output nodes to image files: PNG of 8 and 16 bits, OpenEXR of half floats and
//! floats, and DDS compressed as game engines read textures (BC1 to BC3, with mipmaps), as
//! the Output's format says (see [`brewed_texture_core::nodes::OUTPUT_DEPTHS`]). Channel
//! selection and quantization run multithreaded with SIMD.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use brewed_texture_core::graph::NodeId;
use brewed_texture_core::nodes::output_format_channels;

use crate::gpu::StoredImage;
use crate::simd;

pub use brewed_texture_core::nodes::{
    DEPTH_8, DEPTH_16, DEPTH_16F, DEPTH_32F, DEPTH_DXT1, DEPTH_DXT2, DEPTH_DXT3, DEPTH_DXT4, DEPTH_DXT5,
};

/// A channel of a file packed of several images (see [`write_packed`], and
/// `brewed_texture_core::export_presets`).
#[derive(Clone, Debug, PartialEq)]
pub enum Packed {
    /// A channel of an image (0 red … 3 alpha; a grayscale image is the same in the first
    /// three), turned over (1 − it) where `invert`. Of an Output, it is what arrives at it (see
    /// [`read_what_arrives`]).
    Channel { node: NodeId, channel: u8, invert: bool },
    /// The same value everywhere.
    Value(f32),
}

/// The images a job reads.
pub fn nodes_of(job: &crate::service::ExportJob) -> Vec<NodeId> {
    if job.pack.is_empty() {
        return vec![job.node];
    }
    job.pack
        .iter()
        .filter_map(|p| match p {
            Packed::Channel { node, .. } => Some(*node),
            Packed::Value(_) => None,
        })
        .collect()
}

/// Point the channels of a packed job at what arrives at the Outputs they name, in `graph` as
/// the engine computes it (the graphs used as nodes laid out in it): the image as the graph
/// makes it, in all its channels and at its precision. An Output's own image is in the
/// channels and at the bit depth of its own file: of an Output of 8 bits, a file of 16 would
/// have 8. An Output nothing arrives at stays (it has no image either).
pub fn read_what_arrives(job: &mut crate::service::ExportJob, graph: &brewed_texture_core::Graph) {
    let arrives = brewed_texture_core::graph::input_socket_key("input");
    for p in &mut job.pack {
        if let Packed::Channel { node, .. } = p
            && graph.node(*node).is_some_and(|n| n.kind == brewed_texture_core::NodeKind::OUTPUT)
            && let Some(link) = graph.links.iter().find(|l| l.to == *node && l.to_socket == arrives)
        {
            *node = link.from;
        }
    }
}

/// Write the file packed of the channels `pack` says of `images` (gray of one; red, green and
/// blue of three; and alpha of four), `size` big (the texture size): an image of another size
/// (a uniform color is a single pixel) is read where the file's pixel is. Composed a band of
/// rows at a time into the file's samples, as an image is written (see [`write_stored`]), at
/// `bit_depth` as an Output's is.
pub fn write_packed(
    path: &Path,
    pack: &[Packed],
    images: &HashMap<NodeId, StoredImage>,
    size: [u32; 2],
    bit_depth: u32,
) -> Result<(), String> {
    let [width, height] = size;
    // Those of another size whole (they are small), the others a band at a time.
    let other = images.iter().filter(|(_, i)| [i.width, i.height] != size).map(|(n, i)| (*n, i.to_f32())).collect();
    let packed = PackedRows { width, height, pack, images, other };
    // As an Output writes R, RGB and RGBA.
    let format = match pack.len() {
        1 => 3,
        4.. => 0,
        _ => 1,
    };
    let rows = (simd::BAND / width.max(1)).max(1);
    brewed_texture_core::files::replace(path, |beside| write_in_bands(beside, &packed, format, bit_depth, rows))
}

/// Rows of pixels a file is written of, a band at a time, as RGBA floats (see
/// [`StoredImage::rows_to_f32`]): an image as the GPU stores it, or the channels of several
/// packed into one (see [`write_packed`]).
trait Rows {
    fn size(&self) -> (u32, u32);
    fn band(&self, rows: Range<u32>) -> Vec<f32>;
}

impl Rows for StoredImage {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn band(&self, rows: Range<u32>) -> Vec<f32> {
        self.rows_to_f32(rows)
    }
}

/// The channels of several images packed into one image `width` × `height` (see
/// [`write_packed`]).
struct PackedRows<'a> {
    width: u32,
    height: u32,
    pack: &'a [Packed],
    images: &'a HashMap<NodeId, StoredImage>,
    /// The images of another size, whole, as RGBA floats.
    other: HashMap<NodeId, Vec<f32>>,
}

impl Rows for PackedRows<'_> {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn band(&self, rows: Range<u32>) -> Vec<f32> {
        let width = self.width as usize;
        let mut band = vec![1.0f32; width * (rows.end - rows.start) as usize * 4];
        // The rows of each image as big as the file, read once.
        let mut read: HashMap<NodeId, Vec<f32>> = HashMap::new();
        for (c, source) in self.pack.iter().take(4).enumerate() {
            let targets = band.iter_mut().skip(c).step_by(4);
            let (node, channel, invert) = match source {
                Packed::Value(v) => {
                    targets.for_each(|s| *s = *v);
                    continue;
                }
                Packed::Channel { node, channel, invert } => (*node, usize::from((*channel).min(3)), *invert),
            };
            let turn = |v: f32| if invert { 1.0 - v } else { v };
            match (self.other.get(&node), self.images.get(&node)) {
                (Some(pixels), Some(image)) => {
                    let (w, h) = (image.width as usize, image.height as usize);
                    for (i, s) in targets.enumerate() {
                        let (x, y) = (i % width, rows.start as usize + i / width);
                        let (sx, sy) = (x * w / width, y * h / self.height as usize);
                        *s = turn(pixels[(sy * w + sx) * 4 + channel]);
                    }
                }
                (None, Some(image)) => {
                    let values = read.entry(node).or_insert_with(|| image.rows_to_f32(rows.clone()));
                    for (s, v) in targets.zip(values.iter().skip(channel).step_by(4)) {
                        *s = turn(*v);
                    }
                }
                _ => targets.for_each(|s| *s = 0.0),
            }
        }
        band
    }
}

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

/// File extension for an Output's format.
pub fn extension(bit_depth: u32) -> &'static str {
    match bit_depth {
        DEPTH_16F | DEPTH_32F => "exr",
        DEPTH_DXT1..=DEPTH_DXT5 => "dds",
        _ => "png",
    }
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
/// samples, and no more (not the image as RGBA floats). The file is replaced whole (see
/// `brewed_texture_core::files::replace`): what the export stops at leaves the one before, and
/// an engine that watches the folder never imports half a texture.
pub fn write_stored(path: &Path, image: &StoredImage, format: u32, bit_depth: u32) -> Result<(), String> {
    let rows = (simd::BAND / image.width.max(1)).max(1);
    brewed_texture_core::files::replace(path, |beside| write_in_bands(beside, image, format, bit_depth, rows))
}

/// The samples of a file: the channels it keeps of the pixels, as `quantize` makes them,
/// converted `rows` at a time.
fn samples<T: Copy + Default + Send + Sync>(
    image: &impl Rows,
    channels: &[usize],
    rows: u32,
    quantize: fn(&[f32], &mut [T]),
) -> Vec<T> {
    let (width, height) = image.size();
    let mut out = Vec::with_capacity(width as usize * height as usize * channels.len());
    let mut q = Vec::new();
    for y in (0..height).step_by(rows as usize) {
        let rgba = image.band(y..(y + rows).min(height));
        q.clear();
        q.resize(rgba.len(), T::default());
        quantize(&rgba, &mut q);
        out.extend(simd::select_channels(&q, channels));
    }
    out
}

fn write_in_bands(path: &Path, image: &impl Rows, format: u32, bit_depth: u32, rows: u32) -> Result<(), String> {
    let (width, height) = image.size();
    let channels = file_channels(format);
    let err = |e: image::ImageError| e.to_string();
    let size_err = || "image buffer size mismatch".to_string();
    match bit_depth {
        DEPTH_DXT1..=DEPTH_DXT5 => write_dds(path, image, bit_depth, rows),
        DEPTH_16F => {
            // OpenEXR of half floats, RGB or RGBA; single channels are replicated to gray.
            let four = channels.len() == 4;
            let keep: &[usize] = if four { &[0, 1, 2, 3] } else { &[0, 1, 2] };
            let data: Vec<half::f16> = samples(image, keep, rows, |s, d| {
                for (d, s) in d.iter_mut().zip(s) {
                    *d = half::f16::from_f32(*s);
                }
            });
            let (w, h, n) = (width as usize, height as usize, keep.len());
            let written = if four {
                exr::prelude::write_rgba_file(path, w, h, |x, y| {
                    let i = (y * w + x) * n;
                    (data[i], data[i + 1], data[i + 2], data[i + 3])
                })
            } else {
                exr::prelude::write_rgb_file(path, w, h, |x, y| {
                    let i = (y * w + x) * n;
                    (data[i], data[i + 1], data[i + 2])
                })
            };
            written.map_err(|e| e.to_string())
        }
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
                4 => image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(width, height, data).map(|b| b.save_with_format(path, image::ImageFormat::Png)),
                3 => image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(width, height, data).map(|b| b.save_with_format(path, image::ImageFormat::Png)),
                _ => image::ImageBuffer::<image::Luma<u16>, _>::from_raw(width, height, data).map(|b| b.save_with_format(path, image::ImageFormat::Png)),
            }
            .ok_or_else(size_err)?
            .map_err(err)
        }
        _ => {
            let data = samples(image, channels, rows, simd::quantize_u8);
            match channels.len() {
                4 => image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, data).map(|b| b.save_with_format(path, image::ImageFormat::Png)),
                3 => image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(width, height, data).map(|b| b.save_with_format(path, image::ImageFormat::Png)),
                _ => image::ImageBuffer::<image::Luma<u8>, _>::from_raw(width, height, data).map(|b| b.save_with_format(path, image::ImageFormat::Png)),
            }
            .ok_or_else(size_err)?
            .map_err(err)
        }
    }
}

/// A DDS of an image compressed as game engines read textures, in the DXT format of `depth`
/// (BC1, BC2 or BC3; DXT2 and DXT4 with the colors multiplied by the alpha), with its
/// mipmaps: the pixels as the output keeps them, as RGBA of 8 bits (what it writes of fewer
/// channels is kept with an opaque alpha, one channel as gray, RG with an empty blue).
fn write_dds(path: &Path, image: &impl Rows, depth: u32, rows: u32) -> Result<(), String> {
    use rayon::prelude::*;
    let (width, height) = image.size();
    let (width, height) = (width as usize, height as usize);
    let mut rgba: Vec<u8> = samples(image, &[0, 1, 2, 3], rows, simd::quantize_u8);
    if depth == DEPTH_DXT2 || depth == DEPTH_DXT4 {
        rgba.par_chunks_mut(4).for_each(|px| {
            for c in 0..3 {
                px[c] = ((u32::from(px[c]) * u32::from(px[3]) + 127) / 255) as u8;
            }
        });
    }
    let (format, fourcc) = match depth {
        DEPTH_DXT1 => (texpresso::Format::Bc1, *b"DXT1"),
        DEPTH_DXT2 => (texpresso::Format::Bc2, *b"DXT2"),
        DEPTH_DXT3 => (texpresso::Format::Bc2, *b"DXT3"),
        DEPTH_DXT4 => (texpresso::Format::Bc3, *b"DXT4"),
        _ => (texpresso::Format::Bc3, *b"DXT5"),
    };
    // Each level half the one above (a pixel at least), down to a pixel.
    let mut levels = vec![(width, height, rgba)];
    while let Some((w, h, pixels)) = levels.last()
        && (*w > 1 || *h > 1)
    {
        let next = halved(pixels, *w, *h);
        levels.push(next);
    }
    let mut file = dds_header(width, height, levels.len(), fourcc, format.compressed_size(width, height));
    for (w, h, pixels) in &levels {
        let mut blocks = vec![0u8; format.compressed_size(*w, *h)];
        format.compress(pixels, *w, *h, texpresso::Params::default(), &mut blocks);
        file.extend_from_slice(&blocks);
    }
    std::fs::write(path, file).map_err(|e| e.to_string())
}

/// The next mip level of RGBA pixels of 8 bits, `w` × `h`: half as wide and as high (a pixel at
/// least), each pixel the average of the pixels under it.
fn halved(pixels: &[u8], w: usize, h: usize) -> (usize, usize, Vec<u8>) {
    use rayon::prelude::*;
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; nw * nh * 4];
    out.par_chunks_mut(nw * 4).enumerate().for_each(|(y, row)| {
        for x in 0..nw {
            for c in 0..4 {
                let mut sum = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((2 * x + dx).min(w - 1), (2 * y + dy).min(h - 1));
                    sum += u32::from(pixels[(sy * w + sx) * 4 + c]);
                }
                row[x * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    });
    (nw, nh, out)
}

/// The header of a DDS file of `levels` mip levels, the first `width` × `height`, in the DXT
/// format `fourcc`, whose first level takes `size` bytes.
fn dds_header(width: usize, height: usize, levels: usize, fourcc: [u8; 4], size: usize) -> Vec<u8> {
    const CAPS: u32 = 0x1;
    const HEIGHT: u32 = 0x2;
    const WIDTH: u32 = 0x4;
    const PIXEL_FORMAT: u32 = 0x1000;
    const MIPMAP_COUNT: u32 = 0x2_0000;
    const LINEAR_SIZE: u32 = 0x8_0000;
    const FOURCC: u32 = 0x4;
    const COMPLEX: u32 = 0x8;
    const TEXTURE: u32 = 0x1000;
    const MIPMAP: u32 = 0x40_0000;
    let mipped = levels > 1;
    let mut header = Vec::with_capacity(128);
    header.extend_from_slice(b"DDS ");
    let flags = CAPS | HEIGHT | WIDTH | PIXEL_FORMAT | LINEAR_SIZE | if mipped { MIPMAP_COUNT } else { 0 };
    for v in [124, flags, height as u32, width as u32, size as u32, 0, levels as u32] {
        header.extend_from_slice(&u32::to_le_bytes(v));
    }
    header.extend_from_slice(&[0; 44]);
    // The pixel format: its size, that a FourCC says it, the FourCC, and no bits nor masks.
    for v in [32, FOURCC] {
        header.extend_from_slice(&u32::to_le_bytes(v));
    }
    header.extend_from_slice(&fourcc);
    header.extend_from_slice(&[0; 20]);
    let caps = TEXTURE | if mipped { COMPLEX | MIPMAP } else { 0 };
    for v in [caps, 0, 0, 0, 0] {
        header.extend_from_slice(&u32::to_le_bytes(v));
    }
    header
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
            for depth in [DEPTH_8, DEPTH_16, DEPTH_16F, DEPTH_32F, DEPTH_DXT1, DEPTH_DXT4] {
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
        // Of half floats, as precisely as they are.
        write_image(&exr, &rgba, w, h, 0, DEPTH_16F).unwrap();
        let back = image::open(&exr).unwrap().into_rgba32f();
        for (a, b) in back.as_raw().iter().zip(&rgba) {
            assert!((a - b).abs() < 1e-3, "{a} for {b}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A DDS is its header, then every mip level compressed (BC1 to BC3): what it holds is the
    /// image, as closely as the blocks can say it, and its levels the image made smaller.
    #[test]
    fn dds_files_hold_the_image_and_its_mipmaps() {
        let dir = std::env::temp_dir().join(format!("brewed_dds_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (37u32, 20u32);
        // A soft ramp in each channel, and an alpha that fades.
        let rgba: Vec<f32> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| [x as f32 / w as f32, y as f32 / h as f32, 0.5, 1.0 - x as f32 / (2 * w) as f32]))
            .collect();
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        for (depth, fourcc, format) in [
            (DEPTH_DXT1, b"DXT1", texpresso::Format::Bc1),
            (DEPTH_DXT2, b"DXT2", texpresso::Format::Bc2),
            (DEPTH_DXT3, b"DXT3", texpresso::Format::Bc2),
            (DEPTH_DXT4, b"DXT4", texpresso::Format::Bc3),
            (DEPTH_DXT5, b"DXT5", texpresso::Format::Bc3),
        ] {
            let path = output_paths(&dir, &[("maps".into(), depth)]).remove(0);
            assert_eq!(path.extension().unwrap(), "dds");
            write_image(&path, &rgba, w, h, 0, depth).unwrap();
            let file = std::fs::read(&path).unwrap();
            assert_eq!(&file[..4], b"DDS ");
            assert_eq!((u32_at(&file, 4), u32_at(&file, 12), u32_at(&file, 16)), (124, h, w));
            assert_eq!(&file[84..88], fourcc);
            // 37 × 20 down to 1 × 1: six levels, one after another.
            let levels = u32_at(&file, 28) as usize;
            assert_eq!(levels, 6);
            let mut size = 128;
            let (mut lw, mut lh) = (w as usize, h as usize);
            for _ in 0..levels {
                size += format.compressed_size(lw, lh);
                (lw, lh) = ((lw / 2).max(1), (lh / 2).max(1));
            }
            assert_eq!(file.len(), size);
            // The first level, as a game engine reads it back.
            let mut back = vec![0u8; (w * h * 4) as usize];
            format.decompress(&file[128..], w as usize, h as usize, &mut back);
            let premultiplied = depth == DEPTH_DXT2 || depth == DEPTH_DXT4;
            let mut off = 0.0f32;
            for (i, px) in back.chunks(4).enumerate() {
                let want = &rgba[i * 4..i * 4 + 4];
                let alpha = if depth == DEPTH_DXT1 { 1.0 } else { want[3] };
                for c in 0..3 {
                    let value = if premultiplied { want[c] * want[3] } else { want[c] };
                    off = off.max((f32::from(px[c]) / 255.0 - value).abs());
                }
                if depth != DEPTH_DXT1 {
                    off = off.max((f32::from(px[3]) / 255.0 - alpha).abs());
                }
            }
            assert!(off < 0.08, "{fourcc:?}: {off} off");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
