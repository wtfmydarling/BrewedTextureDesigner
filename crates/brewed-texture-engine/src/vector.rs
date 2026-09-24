//! Vector graphics and text drawn into pixels: what the SVG and Text nodes read.
//!
//! A picture is drawn on the CPU with resvg, exactly at the size of the texture and with the
//! node's transform, so that its edges stay sharp at any size; it is then uploaded like the
//! file of a Bitmap node. It is drawn in bands of rows on every core, and on an axis that tiles
//! the picture wraps around the edge (drawn again one texture over on each side), as the
//! other generators do. Text is laid out by usvg (shaping with rustybuzz): in the node's font,
//! with Inter, built into the program, for what that font does not have, or when it has none.
//! The same text looks the same on every system.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use rayon::prelude::*;
use resvg::{tiny_skia, usvg};

/// Inter, the font text is drawn in when a node names none (SIL Open Font License,
/// `assets/fonts/Inter-LICENSE.txt`).
const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");

/// What to draw.
#[derive(Clone, Debug, PartialEq)]
pub enum Vector {
    /// The file at `path`, fitted as `fit` says (see `brewed_texture_core::nodes::SVG_FITS`);
    /// `color`: its colors, else how much of each pixel it covers.
    Svg { path: String, fit: u32, color: bool },
    /// One line of text, `size` pixels high (the font's em), in the font at `font` (Inter when
    /// empty), aligned at the position as `align` says (see
    /// `brewed_texture_core::nodes::TEXT_ALIGNS`), its letters `spacing` ems apart.
    Text { text: String, font: String, size: f32, align: u32, spacing: f32 },
}

/// A picture to draw: what, at what size, moved, turned and scaled how.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorJob {
    pub what: Vector,
    pub size: [u32; 2],
    /// The offset of the picture's center from the texture's, in textures (the offset of
    /// Transform 2D).
    pub offset: [f32; 2],
    /// Degrees, counter-clockwise.
    pub rotation: f32,
    pub scale: f32,
    /// Tiling bits (see `brewed_texture_core::Tiling`).
    pub tiling: u32,
}

impl VectorJob {
    /// What the picture is, as a number: another job draws another picture.
    pub fn key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match &self.what {
            Vector::Svg { path, fit, color } => (0u8, path, fit, color).hash(&mut h),
            Vector::Text { text, font, size, align, spacing } => {
                (1u8, text, font, size.to_bits(), align, spacing.to_bits()).hash(&mut h)
            }
        }
        (self.size, self.offset.map(f32::to_bits), self.rotation.to_bits(), self.scale.to_bits(), self.tiling)
            .hash(&mut h);
        h.finish()
    }

    /// Whether the picture is in color (the colors of an SVG) or grayscale (how much of each
    /// pixel is covered).
    pub fn is_color(&self) -> bool {
        matches!(self.what, Vector::Svg { color: true, .. })
    }
}

/// A picture's pixels, as they are uploaded: half floats, one per pixel (grayscale) or RGBA.
pub enum Raster {
    Gray(Vec<u16>),
    Color(Vec<u16>),
}

/// The fonts text is drawn in: Inter, and the node's font when it names one.
fn fonts(font: &str) -> Result<(Arc<usvg::fontdb::Database>, String), String> {
    static INTER_DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let inter = INTER_DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_font_data(INTER.to_vec());
        Arc::new(db)
    });
    if font.trim().is_empty() {
        return Ok((inter.clone(), "Inter".to_string()));
    }
    let data = std::fs::read(font).map_err(|e| format!("Cannot read the font '{font}': {e}"))?;
    let mut db = (**inter).clone();
    let before = db.len();
    db.load_font_data(data);
    let family = db
        .faces()
        .skip(before)
        .find_map(|f| f.families.first().map(|(name, _)| name.clone()))
        .ok_or_else(|| format!("'{font}' is not a font"))?;
    Ok((Arc::new(db), family))
}

/// `text` as it can stand in XML.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // What XML cannot hold at all.
            c if (c as u32) < 0x20 && c != '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// The picture as usvg reads it, where its origin is (in its own units) and the scale that
/// fits it into the texture.
fn tree(job: &VectorJob) -> Result<(usvg::Tree, [f32; 2], [f32; 2]), String> {
    let [w, h] = job.size.map(|s| s as f32);
    match &job.what {
        Vector::Svg { path, fit, .. } => {
            if path.trim().is_empty() {
                return Err("No file selected".into());
            }
            let data = std::fs::read(path).map_err(|e| format!("Cannot open '{path}': {e}"))?;
            let options = usvg::Options {
                resources_dir: std::path::Path::new(path).parent().map(Into::into),
                ..usvg::Options::default()
            };
            let tree = usvg::Tree::from_data(&data, &options).map_err(|e| format!("Cannot read '{path}': {e}"))?;
            let size = tree.size();
            let (sw, sh) = (size.width().max(1e-3), size.height().max(1e-3));
            let fit = match fit {
                0 => {
                    let s = (w / sw).min(h / sh);
                    [s, s]
                }
                1 => {
                    let s = (w / sw).max(h / sh);
                    [s, s]
                }
                _ => [w / sw, h / sh],
            };
            Ok((tree, [sw / 2.0, sh / 2.0], fit))
        }
        Vector::Text { text, font, size, align, spacing } => {
            let (fontdb, family) = fonts(font)?;
            let anchor = match align {
                0 => "start",
                2 => "end",
                _ => "middle",
            };
            let px = size.max(0.0);
            let svg = format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><text x="0" y="0" font-family="{family}" font-size="{px}" letter-spacing="{ls}" text-anchor="{anchor}" dominant-baseline="central" fill="#ffffff">{text}</text></svg>"##,
                family = escaped(&family),
                ls = spacing * px,
                text = escaped(text),
            );
            let options = usvg::Options { fontdb, font_family: family, ..usvg::Options::default() };
            let tree = usvg::Tree::from_str(&svg, &options).map_err(|e| format!("Cannot lay out the text: {e}"))?;
            Ok((tree, [0.0, 0.0], [1.0, 1.0]))
        }
    }
}

/// Rows drawn together: enough for the cores to share out, few enough to keep each band's
/// pixels small.
const BAND_ROWS: u32 = 256;

/// Draw the picture: its pixels at the job's size.
pub fn rasterize(job: &VectorJob) -> Result<Raster, String> {
    let [w, h] = job.size;
    if w == 0 || h == 0 {
        return Err("Nothing to draw into".into());
    }
    let (tree, origin, fit) = tree(job)?;
    let (s, c) = job.rotation.to_radians().sin_cos();
    let center = [w as f32 * (0.5 + job.offset[0]), h as f32 * (0.5 + job.offset[1])];
    // Its origin to the texture's center (moved), scaled to fit and by the node's scale, and
    // turned counter-clockwise as Transform 2D turns an image.
    let placed = tiny_skia::Transform::from_translate(-origin[0], -origin[1])
        .post_scale(fit[0] * job.scale, fit[1] * job.scale)
        .post_concat(tiny_skia::Transform::from_row(c, -s, s, c, 0.0, 0.0))
        .post_translate(center[0], center[1]);
    // On an axis that tiles, drawn again one texture over on each side: what leaves one edge
    // comes back at the other.
    let xs: &[f32] = if job.tiling & 1 != 0 { &[-1.0, 0.0, 1.0] } else { &[0.0] };
    let ys: &[f32] = if job.tiling & 2 != 0 { &[-1.0, 0.0, 1.0] } else { &[0.0] };
    let color = job.is_color();
    let channels = if color { 4 } else { 1 };
    // Straight (not premultiplied) values of each byte, as half floats.
    let half: [u16; 256] = std::array::from_fn(|i| half::f16::from_f32(i as f32 / 255.0).to_bits());
    let mut pixels = vec![0u16; w as usize * h as usize * channels];
    let band = BAND_ROWS as usize * w as usize * channels;
    let failed = std::sync::atomic::AtomicBool::new(false);
    pixels.par_chunks_mut(band).enumerate().for_each(|(i, out)| {
        let y0 = i as u32 * BAND_ROWS;
        let rows = BAND_ROWS.min(h - y0);
        let Some(mut pixmap) = tiny_skia::Pixmap::new(w, rows) else {
            failed.store(true, std::sync::atomic::Ordering::Relaxed);
            return;
        };
        for dy in ys {
            for dx in xs {
                let at = placed.post_translate(dx * w as f32, dy * h as f32 - y0 as f32);
                resvg::render(&tree, at, &mut pixmap.as_mut());
            }
        }
        for (px, rgba) in out.chunks_mut(channels).zip(pixmap.pixels()) {
            let a = rgba.alpha();
            if color {
                let c = rgba.demultiply();
                px.copy_from_slice(&[
                    half[c.red() as usize],
                    half[c.green() as usize],
                    half[c.blue() as usize],
                    half[a as usize],
                ]);
            } else {
                px[0] = half[a as usize];
            }
        }
    });
    if failed.into_inner() {
        return Err(format!("Cannot draw at {w}×{h}"));
    }
    Ok(if color { Raster::Color(pixels) } else { Raster::Gray(pixels) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> VectorJob {
        VectorJob {
            what: Vector::Text { text: text.into(), font: String::new(), size: 40.0, align: 1, spacing: 0.0 },
            size: [128, 64],
            offset: [0.0, 0.0],
            rotation: 0.0,
            scale: 1.0,
            tiling: 0,
        }
    }

    fn coverage(job: &VectorJob) -> Vec<f32> {
        match rasterize(job).expect("drawn") {
            Raster::Gray(p) => p.iter().map(|v| half::f16::from_bits(*v).to_f32()).collect(),
            Raster::Color(_) => panic!("text is grayscale"),
        }
    }

    #[test]
    fn text_is_drawn_centered_and_wraps_where_it_tiles() {
        let job = text("Mm");
        let px = coverage(&job);
        let total: f32 = px.iter().sum();
        assert!(total > 100.0, "letters drawn: {total}");
        // Centered: as much on the left half as on the right, near enough.
        let half_sum = |left: bool| -> f32 {
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (y, x)))
                .map(|(y, x)| px[y * 128 + if left { x } else { x + 64 }])
                .sum()
        };
        let (l, r) = (half_sum(true), half_sum(false));
        assert!((l - r).abs() < 0.25 * (l + r), "{l} {r}");
        // Moved half a texture right: on a tiling axis half of it comes back at the left edge.
        let moved = VectorJob { offset: [0.5, 0.0], tiling: 1, ..job.clone() };
        let wrapped = coverage(&moved);
        assert!((wrapped.iter().sum::<f32>() - total).abs() < 0.02 * total, "nothing lost across the edge");
        let not = coverage(&VectorJob { tiling: 0, ..moved });
        assert!(not.iter().sum::<f32>() < 0.7 * total, "without tiling, what leaves is gone");
        // Things XML does not take are text like any other.
        assert!(coverage(&text("<&\"'>")).iter().sum::<f32>() > 10.0);
        let missing =
            Vector::Text { text: "x".into(), font: "/no/such/font.ttf".into(), size: 40.0, align: 1, spacing: 0.0 };
        assert!(rasterize(&VectorJob { what: missing, ..text("x") }).is_err());
    }

    #[test]
    fn svg_files_fit_and_color() {
        let dir = std::env::temp_dir().join(format!("brewed-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("square.svg");
        // A red square in the left half of a picture twice as wide as it is high.
        std::fs::write(&path, r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect x="0" y="0" width="100" height="100" fill="#ff0000"/></svg>"##).unwrap();
        let job = VectorJob {
            what: Vector::Svg { path: path.display().to_string(), fit: 0, color: true },
            size: [64, 64],
            offset: [0.0, 0.0],
            rotation: 0.0,
            scale: 1.0,
            tiling: 0,
        };
        let Raster::Color(px) = rasterize(&job).unwrap() else { panic!("color") };
        let at = |x: usize, y: usize| -> [f32; 4] {
            std::array::from_fn(|c| half::f16::from_bits(px[(y * 64 + x) * 4 + c]).to_f32())
        };
        // Fit: 200×100 into 64×64 is 64×32 in the middle; the square is its left half.
        assert_eq!(at(10, 32), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(at(50, 32)[3], 0.0, "the right half is empty");
        assert_eq!(at(10, 5)[3], 0.0, "above the picture, nothing");
        // Stretched: the square covers the left half from top to bottom.
        let stretched =
            VectorJob { what: Vector::Svg { path: path.display().to_string(), fit: 2, color: false }, ..job };
        let Raster::Gray(g) = rasterize(&stretched).unwrap() else { panic!("coverage is grayscale") };
        assert_eq!(half::f16::from_bits(g[5 * 64 + 10]).to_f32(), 1.0);
        let broken = dir.join("broken.svg");
        std::fs::write(&broken, "not svg").unwrap();
        assert!(
            rasterize(&VectorJob {
                what: Vector::Svg { path: broken.display().to_string(), fit: 0, color: true },
                ..stretched
            })
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
