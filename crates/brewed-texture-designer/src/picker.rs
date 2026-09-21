//! Picking on a capture of the screen, where it cannot be done live (gradients on macOS,
//! see [`crate::eyedropper`]).
//!
//! Every monitor is captured, then covered by a borderless window showing its capture
//! pixel for pixel ([`crate::view::picker`]), so nothing seems to change on the screen.
//! A click picks a color; dragging picks a gradient along the path of the mouse.

use std::fmt;

use bytes::Bytes;
use iced::widget::image;
use brewed_texture_core::graph::NodeId;

use crate::app::ColorTarget;
use crate::eyedropper::Mode;

/// What a screen pick fills in.
#[derive(Clone, Debug, PartialEq)]
pub enum PickTarget {
    /// One color: a parameter, an input value or the selected stop of a gradient.
    Color(ColorTarget),
    /// A whole gradient, from the colors along a path.
    Gradient(NodeId, String),
}

impl PickTarget {
    pub fn mode(&self) -> Mode {
        match self {
            PickTarget::Color(_) => Mode::Color,
            PickTarget::Gradient(..) => Mode::Gradient,
        }
    }
}

/// One captured monitor.
#[derive(Clone)]
pub struct Shot {
    /// Position of the monitor on the desktop, in the platform's window coordinates.
    pub x: i32,
    pub y: i32,
    /// Size of the monitor on the desktop (same units as the position).
    pub width: u32,
    pub height: u32,
    /// The monitor's scale factor.
    pub scale: f32,
    /// Size of the captured image in pixels.
    pub pixel_size: [u32; 2],
    /// RGBA8, row by row; shared with `handle`.
    pixels: Bytes,
    pub handle: image::Handle,
}

impl fmt::Debug for Shot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shot")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("scale", &self.scale)
            .field("pixel_size", &self.pixel_size)
            .finish_non_exhaustive()
    }
}

impl Shot {
    /// A shot of `pixel_size` RGBA8 pixels; `None` if `rgba` does not hold them all.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "only macOS captures the screen"))]
    pub fn new(x: i32, y: i32, width: u32, height: u32, scale: f32, pixel_size: [u32; 2], rgba: Vec<u8>) -> Option<Shot> {
        let expected = (pixel_size[0] as usize).checked_mul(pixel_size[1] as usize)?.checked_mul(4)?;
        if expected == 0 || rgba.len() != expected {
            return None;
        }
        let pixels = Bytes::from(rgba);
        let handle = image::Handle::from_rgba(pixel_size[0], pixel_size[1], pixels.clone());
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        Some(Shot { x, y, width, height, scale, pixel_size, pixels, handle })
    }

    /// A shot of an image that is not on any monitor.
    #[cfg(test)]
    pub fn from_image(pixel_size: [u32; 2], rgba: Vec<u8>) -> Shot {
        Shot::new(0, 0, pixel_size[0], pixel_size[1], 1.0, pixel_size, rgba).expect("image size")
    }

    /// A pixel (clamped to the image), RGB.
    pub fn rgb(&self, x: i64, y: i64) -> [u8; 3] {
        let [w, h] = self.pixel_size;
        let x = x.clamp(0, w as i64 - 1) as usize;
        let y = y.clamp(0, h as i64 - 1) as usize;
        let i = (y * w as usize + x) * 4;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]]
    }

    /// The color of a pixel (clamped to the image), in the 0..1 range color parameters use.
    pub fn pixel(&self, x: i64, y: i64) -> [f32; 4] {
        crate::eyedropper::color(self.rgb(x, y))
    }

    /// A point in the coordinates of this shot's window, in pixels of the capture.
    fn to_pixels(&self, p: [f32; 2]) -> (f32, f32) {
        let sx = self.pixel_size[0] as f32 / self.width.max(1) as f32;
        let sy = self.pixel_size[1] as f32 / self.height.max(1) as f32;
        (p[0] * sx, p[1] * sy)
    }
}

/// The colors along a path dragged over the overlay windows, one per pixel it crosses.
/// `points` are in the coordinates of the window of `shots[from]` and may go on to other
/// monitors; points off every monitor are skipped.
pub fn path_colors(shots: &[Shot], from: usize, points: &[[f32; 2]]) -> Vec<[f32; 4]> {
    let Some(origin) = shots.get(from) else { return Vec::new() };
    // Steps small enough to find the edges of the densest capture.
    let density = shots.iter().map(|s| s.pixel_size[0] as f32 / s.width.max(1) as f32).fold(1.0, f32::max).min(8.0);
    let locate = |d: [f32; 2]| {
        let index = shot_at(shots, (d[0].floor() as i32, d[1].floor() as i32))?;
        let shot = &shots[index];
        Some((index, shot.to_pixels([d[0] - shot.x as f32, d[1] - shot.y as f32])))
    };
    let mut trace = crate::eyedropper::Trace::default();
    points
        .iter()
        .flat_map(|p| trace.advance([origin.x as f32 + p[0], origin.y as f32 + p[1]], 0.5 / density, locate))
        .map(|(shot, (x, y))| shots[shot].pixel(x as i64, y as i64))
        .collect()
}

/// "#RRGGBB" of a picked color.
pub fn hex(c: [f32; 4]) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", q(c[0]), q(c[1]), q(c[2]))
}

/// Capture every monitor (blocking; call it off the UI thread).
#[cfg(target_os = "macos")]
pub fn capture() -> Result<Vec<Shot>, String> {
    crate::eyedropper::macos::screen_capture_allowed()?;
    let monitors = xcap::Monitor::all().map_err(|e| format!("Cannot list the monitors: {e}"))?;
    let mut shots = Vec::new();
    for monitor in monitors {
        let geometry = (|| -> xcap::XCapResult<_> {
            Ok((monitor.x()?, monitor.y()?, monitor.width()?, monitor.height()?, monitor.scale_factor().unwrap_or(1.0)))
        })();
        let (x, y, width, height, scale) = geometry.map_err(|e| format!("Cannot read the monitor layout: {e}"))?;
        let image = monitor.capture_image().map_err(|e| format!("Cannot capture the screen: {e}"))?;
        let pixel_size = [image.width(), image.height()];
        shots.extend(Shot::new(x, y, width, height, scale, pixel_size, image.into_raw()));
    }
    if shots.is_empty() {
        return Err("No monitor could be captured".into());
    }
    Ok(shots)
}

/// Everywhere else the eyedropper picks live and never needs a capture.
#[cfg(not(target_os = "macos"))]
pub fn capture() -> Result<Vec<Shot>, String> {
    Err("The screen cannot be captured on this system".into())
}

/// Window settings for the overlay covering a monitor.
pub fn overlay_settings(shot: &Shot, icon: Option<iced::window::Icon>) -> iced::window::Settings {
    use iced::window;
    window::Settings {
        // Positions and sizes are in the same (logical) units as the monitor layout.
        size: iced::Size::new(shot.width as f32, shot.height as f32),
        position: window::Position::Specific(iced::Point::new(shot.x as f32, shot.y as f32)),
        min_size: None,
        // Covering the monitor, not a fullscreen space: that would animate.
        fullscreen: false,
        decorations: false,
        // Until the capture is drawn the window shows what is under it, the same screen:
        // no blank flash.
        transparent: true,
        resizable: false,
        minimizable: false,
        level: window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        icon,
        ..Default::default()
    }
}

/// Index of the shot whose monitor contains a desktop point.
pub fn shot_at(shots: &[Shot], (x, y): (i32, i32)) -> Option<usize> {
    shots.iter().position(|s| x >= s.x && y >= s.y && x < s.x + s.width as i32 && y < s.y + s.height as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A horizontal ramp: black → red → white over `w` pixels.
    fn ramp(w: u32, h: u32) -> Shot {
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let t = x as f32 / (w - 1) as f32;
                let (r, g) = if t < 0.5 { (t * 2.0, 0.0) } else { (1.0, (t - 0.5) * 2.0) };
                rgba.extend([(r * 255.0).round() as u8, (g * 255.0).round() as u8, (g * 255.0).round() as u8, 255]);
            }
        }
        Shot::from_image([w, h], rgba)
    }

    #[test]
    fn pixels_are_read_and_clamped() {
        let shot = ramp(101, 3);
        assert_eq!(shot.pixel(0, 1), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(shot.pixel(1000, -5), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(hex(shot.pixel(50, 1)), "#FF0000");
    }

    #[test]
    fn a_path_gives_one_color_per_pixel_in_order() {
        let shots = [ramp(400, 50)];
        // Along the ramp and back a little: a freehand path, not a line.
        let colors = path_colors(&shots, 0, &[[0.5, 10.5], [399.5, 30.5], [300.5, 40.5]]);
        assert!(colors.len() >= 400 && colors.len() < 520, "{}", colors.len());
        assert_eq!(colors[0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(colors[399], [1.0, 1.0, 1.0, 1.0]);
        let g = crate::eyedropper::gradient_from_path(&colors);
        assert!(g.sample(0.0)[0] < 0.05);
        // Garbage points are skipped; off the monitors the path ends at the edge.
        assert_eq!(path_colors(&shots, 0, &[[f32::NAN, 0.0], [1.5, 1.5], [-20.0, 1.5]]).len(), 2);
        assert!(path_colors(&shots, 7, &[[1.5, 1.5]]).is_empty());
    }

    #[test]
    fn a_path_goes_on_to_the_next_monitor() {
        // Two monitors side by side, the right one at twice the pixel density.
        let left = Shot::new(0, 0, 100, 10, 1.0, [100, 10], [[255, 0, 0, 255]; 1000].concat()).unwrap();
        let right = Shot::new(100, 0, 100, 10, 2.0, [200, 20], [[0, 0, 255, 255]; 4000].concat()).unwrap();
        let colors = path_colors(&[left, right], 0, &[[50.5, 5.5], [150.5, 5.5]]);
        let reds = colors.iter().filter(|c| c[0] == 1.0).count();
        let blues = colors.iter().filter(|c| c[2] == 1.0).count();
        assert_eq!(reds, 50);
        // One color per pixel of the capture: twice as many on the denser monitor.
        assert!((100..=102).contains(&blues), "{blues}");
        assert_eq!(colors.last(), Some(&[0.0, 0.0, 1.0, 1.0]));
    }

    #[test]
    fn shots_need_all_their_pixels() {
        assert!(Shot::new(0, 0, 2, 2, 1.0, [2, 2], vec![0; 15]).is_none());
        assert!(Shot::new(0, 0, 0, 0, 1.0, [0, 0], vec![]).is_none());
        assert!(Shot::new(0, 0, 2, 2, f32::NAN, [2, 2], vec![0; 16]).is_some_and(|s| s.scale == 1.0));
    }

    #[test]
    fn monitors_are_found_by_point() {
        let mut a = ramp(4, 4);
        a.width = 1920;
        a.height = 1080;
        let mut b = a.clone();
        b.x = 1920;
        let shots = [a, b];
        assert_eq!(shot_at(&shots, (100, 100)), Some(0));
        assert_eq!(shot_at(&shots, (2000, 5)), Some(1));
        assert_eq!(shot_at(&shots, (-1, 5)), None);
    }
}
