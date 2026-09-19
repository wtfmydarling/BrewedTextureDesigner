//! Picking colors anywhere on the screen.
//!
//! Every monitor is captured, then covered by a borderless window that shows the frozen
//! capture ([`crate::view::picker`]): a click picks a color, a dragged line picks a
//! gradient. What gets picked is exactly what was on screen, so a reference on another
//! monitor, in a browser or in Tessera itself can be used.

use std::fmt;

use bytes::Bytes;
use iced::widget::image;
use tessera_core::Gradient;
use tessera_core::graph::NodeId;

use crate::app::ColorTarget;

/// Whether this platform can capture the screen.
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

/// What a screen pick fills in.
#[derive(Clone, Debug, PartialEq)]
pub enum PickTarget {
    /// One color: a parameter, an input value or the selected stop of a gradient.
    Color(ColorTarget),
    /// A whole gradient, from the colors along a line.
    Gradient(NodeId, String),
}

impl PickTarget {
    pub fn is_gradient(&self) -> bool {
        matches!(self, PickTarget::Gradient(..))
    }
}

/// One captured monitor.
#[derive(Clone)]
pub struct Shot {
    /// Position of the monitor on the desktop, as the platform reports it (physical
    /// pixels on Windows).
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
    /// `rgba` holds `pixel_size[0] * pixel_size[1]` RGBA8 pixels.
    pub fn new(x: i32, y: i32, width: u32, height: u32, scale: f32, pixel_size: [u32; 2], rgba: Vec<u8>) -> Shot {
        assert_eq!(rgba.len(), pixel_size[0] as usize * pixel_size[1] as usize * 4, "pixel buffer size");
        let pixels = Bytes::from(rgba);
        let handle = image::Handle::from_rgba(pixel_size[0], pixel_size[1], pixels.clone());
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        Shot { x, y, width, height, scale, pixel_size, pixels, handle }
    }

    /// A shot of an image that is not on any monitor.
    #[cfg(test)]
    pub fn from_image(pixel_size: [u32; 2], rgba: Vec<u8>) -> Shot {
        Shot::new(0, 0, pixel_size[0], pixel_size[1], 1.0, pixel_size, rgba)
    }

    /// The color of a pixel (clamped to the image), in the 0..1 range color parameters use.
    pub fn pixel(&self, x: i64, y: i64) -> [f32; 4] {
        let [w, h] = self.pixel_size;
        let x = x.clamp(0, w as i64 - 1) as usize;
        let y = y.clamp(0, h as i64 - 1) as usize;
        let i = (y * w as usize + x) * 4;
        let p = &self.pixels[i..i + 4];
        [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, 1.0]
    }

    /// Average of the (2·radius+1)² pixels around a point: evens out dithering and noise.
    pub fn average(&self, x: f32, y: f32, radius: i64) -> [f32; 4] {
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        let mut sum = [0.0f32; 3];
        let mut n = 0.0;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let p = self.pixel(cx + dx, cy + dy);
                for c in 0..3 {
                    sum[c] += p[c];
                }
                n += 1.0;
            }
        }
        [sum[0] / n, sum[1] / n, sum[2] / n, 1.0]
    }

    /// Colors along a line between two pixel positions: about one sample per pixel of
    /// length (at most 512), each the average of a 3×3 neighborhood.
    pub fn line(&self, from: [f32; 2], to: [f32; 2]) -> Vec<[f32; 4]> {
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let n = (length.round() as usize + 1).clamp(2, 512);
        (0..n)
            .map(|i| {
                let t = i as f32 / (n - 1) as f32;
                self.average(from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t, 1)
            })
            .collect()
    }

    /// The gradient along a line: as few stops as reproduce the colors closely.
    pub fn gradient(&self, from: [f32; 2], to: [f32; 2]) -> Gradient {
        Gradient::from_samples(&self.line(from, to), GRADIENT_TOLERANCE, Gradient::MAX_STOPS)
    }
}

/// How far (0..1 per channel) a simplified gradient may stray from the picked colors.
const GRADIENT_TOLERANCE: f32 = 0.012;

/// "#RRGGBB" of a picked color.
pub fn hex(c: [f32; 4]) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", q(c[0]), q(c[1]), q(c[2]))
}

/// Capture every monitor (blocking; call it off the UI thread).
#[cfg(any(windows, target_os = "macos"))]
pub fn capture() -> Result<Vec<Shot>, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Cannot list the monitors: {e}"))?;
    let mut shots = Vec::new();
    for monitor in monitors {
        let geometry = (|| -> xcap::XCapResult<_> {
            Ok((monitor.x()?, monitor.y()?, monitor.width()?, monitor.height()?, monitor.scale_factor().unwrap_or(1.0)))
        })();
        let (x, y, width, height, scale) = geometry.map_err(|e| format!("Cannot read the monitor layout: {e}"))?;
        let image = monitor.capture_image().map_err(|e| format!("Cannot capture the screen: {e}"))?;
        let pixel_size = [image.width(), image.height()];
        if pixel_size[0] == 0 || pixel_size[1] == 0 {
            continue;
        }
        shots.push(Shot::new(x, y, width, height, scale, pixel_size, image.into_raw()));
    }
    if shots.is_empty() {
        return Err("No monitor could be captured".into());
    }
    Ok(shots)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn capture() -> Result<Vec<Shot>, String> {
    Err("Picking colors from the screen is not supported on this platform yet".into())
}

/// Window settings for the overlay covering a monitor.
pub fn overlay_settings(shot: &Shot, icon: Option<iced::window::Icon>) -> iced::window::Settings {
    use iced::window;
    let s = shot.scale;
    window::Settings {
        size: iced::Size::new(shot.width as f32 / s, shot.height as f32 / s),
        position: window::Position::Specific(iced::Point::new(shot.x as f32 / s, shot.y as f32 / s)),
        // Windows places the window exactly (see `cover`) and then shows it.
        visible: !cfg!(windows),
        fullscreen: !cfg!(windows),
        decorations: false,
        resizable: false,
        minimizable: false,
        level: window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        icon,
        #[cfg(windows)]
        platform_specific: window::settings::PlatformSpecific {
            skip_taskbar: true,
            drag_and_drop: false,
            undecorated_shadow: false,
            corner_preference: window::settings::platform::CornerPreference::DoNotRound,
        },
        ..Default::default()
    }
}

/// Put an overlay window exactly over its monitor and bring it to the front.
///
/// Windows positions windows in physical pixels, which is exact whatever the scale
/// factors of the monitors. The window is moved onto the monitor first (so its scale
/// factor applies), then given the monitor's rectangle.
#[cfg(windows)]
pub fn cover(window: &dyn iced::window::Window, shot: &Shot, focus: bool) {
    use iced::window::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos};

    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else { return };
    let hwnd = win32.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    // SAFETY: the handle belongs to a live window of this thread's event loop.
    unsafe {
        SetWindowPos(hwnd, HWND_TOPMOST, shot.x, shot.y, 64, 64, SWP_NOACTIVATE);
        SetWindowPos(hwnd, HWND_TOPMOST, shot.x, shot.y, shot.width as i32, shot.height as i32, SWP_SHOWWINDOW);
        if focus {
            SetForegroundWindow(hwnd);
        }
    }
}

/// Where the mouse is on the desktop, in the units the shots use.
#[cfg(windows)]
pub fn cursor_position() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: `p` is a valid out pointer.
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
}

#[cfg(not(windows))]
pub fn cursor_position() -> Option<(i32, i32)> {
    None
}

/// Shots laid out like the real monitors but showing a test pattern instead of the
/// screen (the `--scenario eyedropper` self-check; nothing on the screen is captured).
#[cfg(any(windows, target_os = "macos"))]
pub fn test_pattern() -> Result<Vec<Shot>, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Cannot list the monitors: {e}"))?;
    let mut shots = Vec::new();
    for m in monitors {
        let layout = (|| -> xcap::XCapResult<_> { Ok((m.x()?, m.y()?, m.width()?, m.height()?, m.scale_factor().unwrap_or(1.0))) })();
        let (x, y, width, height, scale) = layout.map_err(|e| format!("Cannot read the monitor layout: {e}"))?;
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for py in 0..height {
            for px in 0..width {
                // Hue across, brightness down, a grid every 64 pixels.
                let grid = px % 64 == 0 || py % 64 == 0;
                let h = px as f32 / width as f32 * 6.0;
                let v = 1.0 - 0.6 * py as f32 / height as f32;
                let channel = |center: f32| (2.0 - (h - center).abs().min(6.0 - (h - center).abs())).clamp(0.0, 1.0) * v;
                let [r, g, b] = if grid { [0.9, 0.9, 0.9] } else { [channel(0.0), channel(2.0), channel(4.0)] };
                rgba.extend([(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]);
            }
        }
        shots.push(Shot::new(x, y, width, height, scale, [width, height], rgba));
    }
    Ok(shots)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn test_pattern() -> Result<Vec<Shot>, String> {
    Err("Picking colors from the screen is not supported on this platform yet".into())
}

/// A window's rectangle on the desktop (left, top, right, bottom), for checks.
#[cfg(windows)]
pub fn window_rect(window: &dyn iced::window::Window) -> Option<[i32; 4]> {
    use iced::window::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else { return None };
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a live window handle and a valid out pointer.
    (unsafe { GetWindowRect(win32.hwnd.get() as windows_sys::Win32::Foundation::HWND, &mut r) } != 0).then_some([r.left, r.top, r.right, r.bottom])
}

#[cfg(not(windows))]
pub fn window_rect(_window: &dyn iced::window::Window) -> Option<[i32; 4]> {
    None
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
    fn averaging_evens_out_noise() {
        // A checkerboard of black and white averages to gray.
        let mut rgba = Vec::new();
        for y in 0..9u32 {
            for x in 0..9u32 {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                rgba.extend([v, v, v, 255]);
            }
        }
        let shot = Shot::from_image([9, 9], rgba);
        let avg = shot.average(4.5, 4.5, 1);
        assert!((avg[0] - 5.0 / 9.0).abs() < 1e-5, "{avg:?}");
    }

    #[test]
    fn a_line_becomes_a_gradient_with_few_stops() {
        let shot = ramp(400, 5);
        let samples = shot.line([0.0, 2.0], [399.0, 2.0]);
        assert_eq!(samples.len(), 400);
        let g = shot.gradient([0.0, 2.0], [399.0, 2.0]);
        // Two linear segments: black → red → white.
        assert!(g.stops.len() <= 4, "{} stops", g.stops.len());
        let mid = g.sample(0.5);
        assert!(mid[0] > 0.95 && mid[1] < 0.05, "{mid:?}");
        let end = g.sample(1.0);
        assert!(end[1] > 0.95, "{end:?}");
        // Reversed line, reversed gradient.
        let back = shot.gradient([399.0, 2.0], [0.0, 2.0]);
        assert!(back.sample(0.0)[1] > 0.95 && back.sample(1.0)[0] < 0.05);
    }

    #[test]
    fn short_lines_still_give_two_samples() {
        let shot = ramp(10, 10);
        assert_eq!(shot.line([3.0, 3.0], [3.0, 3.0]).len(), 2);
    }

    /// Captures the real screen: run by hand (`cargo test -- --ignored capturing`).
    /// Only the sizes are checked; nothing is saved.
    #[test]
    #[ignore]
    fn capturing_every_monitor() {
        let started = std::time::Instant::now();
        let shots = capture().expect("capture");
        for s in &shots {
            println!("monitor at {},{} {}x{} (scale {}): image {:?}", s.x, s.y, s.width, s.height, s.scale, s.pixel_size);
            assert!(s.pixel_size[0] > 0 && s.pixel_size[1] > 0);
        }
        println!("{} monitors captured in {:?}", shots.len(), started.elapsed());
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
