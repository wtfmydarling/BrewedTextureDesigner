//! Pictures made of pictures (dev-tools): what the cards of the templates and the lessons of
//! Learn show, made of the renders and exports of the program (see the README):
//!
//! - `square IN OUT SIZE`: its middle, square, SIZE pixels a side;
//! - `fit IN OUT SIZE`: whole in a square of SIZE pixels, on the views' dark;
//! - `crop IN OUT X Y W H SIZE`: the part of it at X, Y, W × H, SIZE pixels a side;
//! - `collage OUT W H IN…`: W × H, a slice of each, cut slanting as `/`;
//! - `grid OUT SIZE IN…`: a square of SIZE pixels, the pictures in rows and columns of it;
//! - `gradient OUT SIZE`: from black on the left to white on the right;
//! - `sheet OUT COLUMNS SIZE IN…`: all of them side by side, to look at at once;
//! - `svg IN OUT SIZE`: a drawing, SIZE pixels a side, on the views' dark;
//! - `split OUT SIZE BEFORE AFTER`: the one on the left, the other on the right, a line and an
//!   arrow between them: what something was and what it made of it;
//! - `filter IN OUT gray` or `filter IN OUT tint R G B`: its gray, or its gray in a color;
//! - `scan OUT SIZE`: a photograph of stone, as a camera takes it: lit unevenly, not repeating.
//!
//! A picture of floats (an EXR) is light: what goes above white is white, and it is made sRGB.
//! What is written is PNG, or JPEG where its name says .jpg.

use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};

/// The dark of the views behind a picture that does not fill its square.
const DARK: Rgba<u8> = Rgba([26, 26, 26, 255]);

pub fn run(args: &[String]) -> Result<(), String> {
    let arg = |i: usize| args.get(i).cloned().ok_or_else(|| format!("--compose {}: too few arguments", args.first().map_or("", String::as_str)));
    let number = |i: usize| arg(i).and_then(|s| s.parse::<u32>().map_err(|e| format!("{s}: {e}")));
    let path = |i: usize| arg(i).map(PathBuf::from);
    let rest = |from: usize| args.iter().skip(from).map(PathBuf::from).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("square") => save(&square(&load(&path(1)?)?, number(3)?), &path(2)?),
        Some("fit") => save(&fit(&load(&path(1)?)?, number(3)?), &path(2)?),
        Some("crop") => {
            let picture = load(&path(1)?)?;
            let part = image::imageops::crop_imm(&picture, number(3)?, number(4)?, number(5)?, number(6)?).to_image();
            save(&square(&part, number(7)?), &path(2)?)
        }
        Some("collage") => save(&collage(&rest(4).iter().map(|p| load(p)).collect::<Result<Vec<_>, _>>()?, number(2)?, number(3)?), &path(1)?),
        Some("gradient") => {
            let side = number(2)?;
            let ramp = RgbaImage::from_fn(side, side, |x, _| {
                let v = (x * 255 / side.saturating_sub(1).max(1)) as u8;
                Rgba([v, v, v, 255])
            });
            save(&ramp, &path(1)?)
        }
        Some("sheet") => save(&sheet(&rest(4).iter().map(|p| load(p)).collect::<Result<Vec<_>, _>>()?, number(2)?, number(3)?), &path(1)?),
        Some("grid") => save(&grid(&rest(3).iter().map(|p| load(p)).collect::<Result<Vec<_>, _>>()?, number(2)?), &path(1)?),
        Some("svg") => save(&svg(&path(1)?, number(3)?)?, &path(2)?),
        Some("split") => save(&split(&load(&path(3)?)?, &load(&path(4)?)?, number(2)?), &path(1)?),
        Some("filter") => {
            let picture = load(&path(1)?)?;
            let tint = match arg(3)?.as_str() {
                "gray" => [1.0; 3],
                "tint" => {
                    let channel = |i: usize| arg(i).and_then(|s| s.parse::<f32>().map_err(|e| format!("{s}: {e}")));
                    [channel(4)?, channel(5)?, channel(6)?]
                }
                other => return Err(format!("--compose filter: gray or tint, not {other}")),
            };
            save(&filter(&picture, tint), &path(2)?)
        }
        Some("scan") => save(&scan(number(2)?), &path(1)?),
        _ => Err("--compose square|fit|crop|collage|grid|gradient|sheet|svg|split|filter|scan … (see compose.rs)".into()),
    }
}

/// The picture at `path`: of floats, its light made a picture (see the module).
fn load(path: &Path) -> Result<RgbaImage, String> {
    let picture = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(match picture {
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => {
            let light = picture.to_rgba32f();
            RgbaImage::from_fn(light.width(), light.height(), |x, y| {
                let p = light.get_pixel(x, y).0;
                let srgb = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8;
                Rgba([srgb(p[0]), srgb(p[1]), srgb(p[2]), 255])
            })
        }
        other => other.to_rgba8(),
    })
}

fn save(picture: &RgbaImage, path: &Path) -> Result<(), String> {
    crate::app::dev::save_picture(picture, path)
}

/// `picture` made `w` × `h` by covering it: as large as it takes to fill it, its middle.
fn cover(picture: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let scale = (w as f32 / picture.width() as f32).max(h as f32 / picture.height() as f32);
    let [sw, sh] = [(picture.width() as f32 * scale).ceil() as u32, (picture.height() as f32 * scale).ceil() as u32];
    let scaled = image::imageops::resize(picture, sw.max(w), sh.max(h), FilterType::Lanczos3);
    image::imageops::crop_imm(&scaled, (scaled.width() - w) / 2, (scaled.height() - h) / 2, w, h).to_image()
}

/// `pictures` in a square `side` pixels a side, as many rows as columns (two for four), each
/// covering its cell, the cells a line of the dark apart.
fn grid(pictures: &[RgbaImage], side: u32) -> RgbaImage {
    let n = (pictures.len().max(1) as f32).sqrt().ceil() as u32;
    let cell = side / n;
    let mut out = RgbaImage::from_pixel(side, side, DARK);
    for (i, p) in pictures.iter().enumerate() {
        let (col, row) = (i as u32 % n, i as u32 / n);
        let covered = cover(p, cell - 2, cell - 2);
        image::imageops::overlay(&mut out, &covered, i64::from(col * cell + 1), i64::from(row * cell + 1));
    }
    out
}

/// The middle of `picture`, square, `side` pixels a side.
fn square(picture: &RgbaImage, side: u32) -> RgbaImage {
    cover(picture, side, side)
}

/// `picture` whole in a square of `side` pixels, on the dark.
fn fit(picture: &RgbaImage, side: u32) -> RgbaImage {
    let scale = (side as f32 / picture.width() as f32).min(side as f32 / picture.height() as f32);
    let [w, h] = [((picture.width() as f32 * scale).round() as u32).max(1), ((picture.height() as f32 * scale).round() as u32).max(1)];
    let scaled = image::imageops::resize(picture, w, h, FilterType::Lanczos3);
    let mut out = RgbaImage::from_pixel(side, side, DARK);
    image::imageops::overlay(&mut out, &scaled, i64::from((side - w) / 2), i64::from((side - h) / 2));
    out
}

/// `pictures` side by side in a picture `w` × `h`, each covering it where its slice is: the
/// slices cut slanting as `/`, a dark line along each cut, the cuts spread over the middle of
/// the picture, where the model of a render is (a slice at either side shows its side of it).
fn collage(pictures: &[RgbaImage], w: u32, h: u32) -> RgbaImage {
    let n = pictures.len().max(1);
    let covered: Vec<RgbaImage> = pictures.iter().map(|p| cover(p, w, h)).collect();
    // Where the cuts are, along the middle of the picture: its middle two fifths.
    let (from, width) = (w as f32 * 0.3, w as f32 * 0.4);
    let cuts: Vec<f32> = (1..n).map(|i| from + width * i as f32 / n as f32).collect();
    // How far a cut leans: higher up, further right, by this much a pixel.
    const LEAN: f32 = 0.25;
    RgbaImage::from_fn(w, h, |x, y| {
        let along = x as f32 + (y as f32 - h as f32 / 2.0) * LEAN;
        if cuts.iter().any(|c| (along - c).abs() < 1.6) {
            return Rgba([18, 18, 18, 255]);
        }
        let k = cuts.iter().filter(|c| along > **c).count();
        *covered.get(k).map_or(&DARK, |p| p.get_pixel(x, y))
    })
}

/// The SVG drawing at `path`, `side` pixels a side (as wide or as high as it is drawn), on the
/// views' dark.
fn svg(path: &Path, side: u32) -> Result<RgbaImage, String> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut pixmap = tiny_skia::Pixmap::new(side.max(1), side.max(1)).ok_or("no room for the drawing")?;
    pixmap.fill(tiny_skia::Color::from_rgba8(DARK[0], DARK[1], DARK[2], 255));
    let scale = side as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(RgbaImage::from_fn(side.max(1), side.max(1), |x, y| {
        let p = pixmap.pixel(x, y).map(|p| p.demultiply()).map_or([0, 0, 0], |c| [c.red(), c.green(), c.blue()]);
        Rgba([p[0], p[1], p[2], 255])
    }))
}

/// `before` on the left of a square `side` pixels a side and `after` on the right, each covering
/// the square (so what is in both meets where it is), a light line between them with an arrow
/// on it from the one to the other.
fn split(before: &RgbaImage, after: &RgbaImage, side: u32) -> RgbaImage {
    let (a, b) = (cover(before, side, side), cover(after, side, side));
    let (mid, cy) = (side as f32 / 2.0, side as f32 / 2.0);
    // The arrow: a triangle pointing right, as large as a twentieth of the picture.
    let s = side as f32 / 20.0;
    let arrow = |x: f32, y: f32, grow: f32| {
        let (left, right) = (mid - s * 0.7 - grow, mid + s * 0.9 + grow * 1.6);
        let half = (s + grow) * (right - x) / (right - left);
        x >= left && x <= right && (y - cy).abs() <= half
    };
    RgbaImage::from_fn(side, side, |x, y| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        if arrow(fx, fy, 0.0) {
            Rgba([240, 240, 240, 255])
        } else if arrow(fx, fy, 2.5) {
            Rgba([24, 24, 24, 255])
        } else if (fx - mid).abs() < 1.5 {
            Rgba([230, 230, 230, 255])
        } else if fx < mid {
            *a.get_pixel(x, y)
        } else {
            *b.get_pixel(x, y)
        }
    })
}

/// `picture` gray, as bright as it is (sRGB's luminance of its linear light), times `tint`.
fn filter(picture: &RgbaImage, tint: [f32; 3]) -> RgbaImage {
    let linear = |v: u8| (f32::from(v) / 255.0).powf(2.2);
    let srgb = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8;
    RgbaImage::from_fn(picture.width(), picture.height(), |x, y| {
        let p = picture.get_pixel(x, y).0;
        let l = 0.2126 * linear(p[0]) + 0.7152 * linear(p[1]) + 0.0722 * linear(p[2]);
        Rgba([srgb(l * tint[0]), srgb(l * tint[1]), srgb(l * tint[2]), p[3]])
    })
}

/// A photograph of stone, `side` pixels a side, made up: grains and cracks of a few sizes, in
/// the grays and browns of rock, lit as a camera's flash and the sky light it (brighter at the
/// top left, falling off to a shadow at the bottom right), and not repeating: what a scan is
/// before it is processed.
fn scan(side: u32) -> RgbaImage {
    // Value noise: a random value at each point of a lattice, smoothly between them.
    let hash = |x: i32, y: i32, k: u32| {
        let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ k.wrapping_mul(0xcb1a_b31f);
        h ^= h >> 13;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^= h >> 16;
        (h & 0xffff) as f32 / 65535.0
    };
    let noise = |x: f32, y: f32, k: u32| {
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let top = hash(ix, iy, k) + (hash(ix + 1, iy, k) - hash(ix, iy, k)) * sx;
        let bottom = hash(ix, iy + 1, k) + (hash(ix + 1, iy + 1, k) - hash(ix, iy + 1, k)) * sx;
        top + (bottom - top) * sy
    };
    let fractal = |x: f32, y: f32, k: u32, octaves: u32| {
        let (mut sum, mut amp, mut freq, mut total) = (0.0, 1.0, 1.0, 0.0);
        for o in 0..octaves {
            sum += noise(x * freq, y * freq, k + o) * amp;
            total += amp;
            amp *= 0.55;
            freq *= 2.03;
        }
        sum / total
    };
    let n = side as f32;
    RgbaImage::from_fn(side, side, |x, y| {
        let (u, v) = (x as f32 / n, y as f32 / n);
        // The rock: broad patches, finer grains, and cracks where a ridged noise is low.
        let patches = fractal(u * 3.0, v * 3.0, 11, 4);
        let grains = fractal(u * 40.0, v * 40.0, 23, 3);
        let ridge = 1.0 - (fractal(u * 7.0, v * 7.0, 37, 4) * 2.0 - 1.0).abs();
        let crack = ((ridge - 0.82) / 0.18).clamp(0.0, 1.0);
        let shade = (0.55 + 0.5 * (patches - 0.5) + 0.25 * (grains - 0.5)) * (1.0 - 0.6 * crack);
        // Gray to brown, as the patches are.
        let warm = (patches - 0.35).clamp(0.0, 1.0);
        let rock = [shade * (1.0 + 0.25 * warm), shade * (0.95 + 0.08 * warm), shade * (0.88 - 0.1 * warm)];
        // The light it was photographed in.
        let light = (1.25 - 0.75 * (u * 0.6 + v * 0.8)).clamp(0.35, 1.3) * (1.0 - 0.25 * ((u - 0.35).powi(2) + (v - 0.3).powi(2)));
        let srgb = |c: f32| ((c * light).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8;
        Rgba([srgb(rock[0] * 0.75), srgb(rock[1] * 0.75), srgb(rock[2] * 0.75), 255])
    })
}

/// `pictures` in rows of `columns`, each `side` pixels square, to look at at once.
fn sheet(pictures: &[RgbaImage], columns: u32, side: u32) -> RgbaImage {
    let columns = columns.max(1);
    let rows = (pictures.len() as u32).div_ceil(columns).max(1);
    let gap = 8;
    let mut out = RgbaImage::from_pixel(columns * (side + gap) + gap, rows * (side + gap) + gap, Rgba([60, 60, 60, 255]));
    for (i, picture) in pictures.iter().enumerate() {
        let (c, r) = (i as u32 % columns, i as u32 / columns);
        image::imageops::overlay(&mut out, &fit(picture, side), i64::from(gap + c * (side + gap)), i64::from(gap + r * (side + gap)));
    }
    out
}
