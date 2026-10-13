//! What the Explorer knows of the files the open packages use: whether each is there, and a
//! picture of it (an image made small, vector graphics drawn, a font by "Aa" in it). Files are
//! looked at in the background every few seconds, since one on disk can come, go or change,
//! and a picture is made again when its file changed; the thread that draws the window never
//! asks the disk.

use std::time::SystemTime;

use brewed_texture_core::files;
use brewed_texture_core::package::{ResourceKind, file_name};

/// How large a picture of a file is made: pixels on its longer side.
pub const SIZE: u32 = 128;

/// How many pictures are made at a time: each decodes a whole file first (an 8K picture of
/// floats takes 1 GB), and a package of many files would otherwise decode them all at once.
pub const AT_ONCE: usize = 2;

/// What is known of a file.
#[derive(Clone, Debug, Default)]
pub struct FileState {
    /// It was there when it was looked at last (not known before: taken as there).
    pub there: bool,
    /// When it changed last, as the disk says (none for a file a package holds).
    pub modified: Option<SystemTime>,
    /// How large it is.
    pub bytes: Option<u64>,
    /// Its picture, of the file as it was when it was made.
    pub preview: Option<Preview>,
    /// Why no picture could be made of the file as it was when it was tried.
    pub failed: Option<(Option<SystemTime>, String)>,
    /// A picture is being made.
    pub making: bool,
}

/// A picture of a file, and what the file is ("2048 × 2048, RGBA 8 bits").
#[derive(Clone, Debug)]
pub struct Preview {
    pub image: iced::widget::image::Handle,
    pub about: String,
    /// When the file it was made of changed last.
    pub of: Option<SystemTime>,
}

impl FileState {
    /// Should a picture be made of the file as it is now?
    pub fn wants_preview(&self) -> bool {
        self.there
            && !self.making
            && self.preview.as_ref().is_none_or(|p| p.of != self.modified)
            && self.failed.as_ref().is_none_or(|(of, _)| *of != self.modified)
    }
}

/// What looking at a file found.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub path: String,
    pub there: bool,
    pub modified: Option<SystemTime>,
    pub bytes: Option<u64>,
}

/// Look at each file: a file a package holds is there when it is held, one on disk as the disk
/// says (this asks it).
pub fn look(paths: &[String]) -> Vec<Look> {
    paths
        .iter()
        .map(|path| {
            if files::is_embedded(path) {
                let held = files::embedded(path);
                Look { path: path.clone(), there: held.is_some(), modified: None, bytes: held.map(|b| b.len() as u64) }
            } else {
                let meta = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file);
                Look { path: path.clone(), there: meta.is_some(), modified: meta.as_ref().and_then(|m| m.modified().ok()), bytes: meta.map(|m| m.len()) }
            }
        })
        .collect()
}

/// A picture of the file at `path` of kind `kind` ([`SIZE`] pixels on its longer side at
/// most), and what the file is: RGBA of 8 bits and its size.
pub fn make(path: &str, kind: ResourceKind) -> Result<(Vec<u8>, u32, u32, String), String> {
    match kind {
        ResourceKind::Bitmap => {
            let bytes = files::read(path)?;
            let picture = brewed_texture_engine::open_picture(&bytes, &file_name(path))?;
            let about = format!("{} × {}, {}", picture.width(), picture.height(), describe(picture.color()));
            let small = picture.thumbnail(SIZE, SIZE).to_rgba8();
            let (w, h) = small.dimensions();
            Ok((small.into_raw(), w, h, about))
        }
        ResourceKind::Svg => Ok((brewed_texture_engine::vector::preview(path, false, SIZE)?, SIZE, SIZE, "Vector graphics".into())),
        ResourceKind::Font => Ok((brewed_texture_engine::vector::preview(path, true, SIZE)?, SIZE, SIZE, "Font".into())),
    }
}

/// What the pixels of an image are, as the Explorer says it.
fn describe(color: image::ColorType) -> &'static str {
    use image::ColorType as C;
    match color {
        C::L8 => "gray 8 bits",
        C::La8 => "gray and alpha 8 bits",
        C::Rgb8 => "RGB 8 bits",
        C::Rgba8 => "RGBA 8 bits",
        C::L16 => "gray 16 bits",
        C::La16 => "gray and alpha 16 bits",
        C::Rgb16 => "RGB 16 bits",
        C::Rgba16 => "RGBA 16 bits",
        C::Rgb32F => "RGB 32 bits float",
        C::Rgba32F => "RGBA 32 bits float",
        _ => "pixels",
    }
}

/// A size in bytes, as people read it.
pub fn bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{n} bytes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of an image, of vector graphics and of a font, whether each is on disk or held
    /// by a package; what cannot be read says why.
    #[test]
    fn pictures_of_the_files_packages_use() {
        let dir = std::env::temp_dir().join(format!("brewed_previews_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("wide.png");
        image::RgbaImage::from_pixel(400, 100, image::Rgba([200, 60, 20, 255])).save(&png).unwrap();
        let png = png.display().to_string();
        let (rgba, w, h, about) = make(&png, ResourceKind::Bitmap).unwrap();
        assert_eq!((w, h, rgba.len()), (SIZE, SIZE / 4, (SIZE * SIZE / 4 * 4) as usize));
        assert_eq!(&rgba[..4], &[200, 60, 20, 255]);
        assert_eq!(about, "400 × 100, RGBA 8 bits");
        // Held by a package: the same picture.
        let bytes = std::sync::Arc::new(std::fs::read(&png).unwrap());
        let held = files::embedded_path(&png, &bytes);
        files::keep(&held, &bytes, "wide.png");
        assert_eq!(make(&held, ResourceKind::Bitmap).unwrap().0, rgba);
        let looks = look(&[png.clone(), held, dir.join("gone.png").display().to_string()]);
        assert_eq!(looks.iter().map(|l| l.there).collect::<Vec<_>>(), [true, true, false]);
        assert!(looks[0].modified.is_some() && looks[1].modified.is_none());
        assert_eq!(looks[0].bytes, looks[1].bytes);
        // Vector graphics, drawn; a font, by "Aa" in it (Inter, built into the program, for a
        // font file that is not there says so).
        let svg = dir.join("disc.svg");
        std::fs::write(&svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><circle cx="5" cy="5" r="4" fill="#ff0000"/></svg>"##).unwrap();
        let (rgba, ..) = make(&svg.display().to_string(), ResourceKind::Svg).unwrap();
        let middle = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
        assert_eq!(&rgba[middle..middle + 4], &[255, 0, 0, 255]);
        assert!(make(&dir.join("no.ttf").display().to_string(), ResourceKind::Font).is_err());
        assert!(make(&png, ResourceKind::Font).is_err(), "a picture is not a font");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
