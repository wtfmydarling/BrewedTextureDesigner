//! Pictures made of pictures (dev-tools): what the cards of the templates and the lessons of
//! Learn show, made of the renders and exports of the program (see the README):
//!
//! - `square IN OUT SIZE`: its middle, square, SIZE pixels a side;
//! - `fit IN OUT SIZE`: whole in a square of SIZE pixels, on the views' dark;
//! - `crop IN OUT X Y W H SIZE`: the part of it at X, Y, W × H, SIZE pixels a side;
//! - `collage OUT W H IN…`: W × H, a slice of each, cut slanting as `/`;
//! - `gradient OUT SIZE`: from black on the left to white on the right;
//! - `sheet OUT COLUMNS SIZE IN…`: all of them side by side, to look at at once.
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
        _ => Err("--compose square|fit|crop|collage|gradient|sheet … (see compose.rs)".into()),
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
