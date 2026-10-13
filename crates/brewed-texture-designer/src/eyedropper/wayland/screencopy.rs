//! Reading the screen with wlr-screencopy (the wlroots desktops: Sway, Hyprland, river,
//! niri, labwc, Wayfire...): a small region of an output at a time, without the cursor.

use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use wayland_client::protocol::wl_output::Transform;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::{Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use super::{Geometry, Pixels, State};
use crate::eyedropper::path::Pixel;
use crate::eyedropper::stroke::Area;

/// A region being read.
pub struct Capture {
    output: usize,
    /// The pixels asked for (upright, as the desktop shows them).
    area: Area,
    geometry: Geometry,
    /// Where the region is in the output's buffer (its own orientation), as the
    /// compositor computes it.
    boxes: [(i32, i32, i32, i32); 2],
    frame: ZwlrScreencopyFrameV1,
    format: Option<(Format, i32, i32, i32)>,
    buffer: Option<Buffer>,
    y_invert: bool,
}

/// The data of a frame: which capture it is.
pub struct FrameData(pub usize);

impl Capture {
    /// Ask for the pixels of `area` of an output.
    pub fn start(
        manager: &ZwlrScreencopyManagerV1,
        qh: &QueueHandle<State>,
        id: usize,
        output: usize,
        wl_output: &wayland_client::protocol::wl_output::WlOutput,
        geometry: Geometry,
        area: Area,
    ) -> Capture {
        let (sx, sy) = geometry.scale();
        // The region in logical units (a little larger), within the output.
        let (lw, lh) = (geometry.width.round() as i32, geometry.height.round() as i32);
        let l0 = ((area.0 as f32 / sx).floor() as i32 - 1).clamp(0, lw);
        let t0 = ((area.1 as f32 / sy).floor() as i32 - 1).clamp(0, lh);
        let r0 = ((area.2 as f32 / sx).ceil() as i32 + 1).clamp(0, lw);
        let b0 = ((area.3 as f32 / sy).ceil() as i32 + 1).clamp(0, lh);
        let logical = (l0, t0, (r0 - l0).max(1), (b0 - t0).max(1));
        // As wlroots does it: into the buffer's orientation, then scaled (truncating); some
        // compositors round instead.
        let turned = transform_box(invert(geometry.transform), logical, (lw, lh));
        let scale = (geometry.pixels.0 as f32 / lw.max(1) as f32, geometry.pixels.1 as f32 / lh.max(1) as f32);
        let (scale_x, scale_y) = if geometry.transform as u32 % 2 == 1 { (scale.1, scale.0) } else { scale };
        let truncated =
            ((turned.0 as f32 * scale_x) as i32, (turned.1 as f32 * scale_y) as i32, (turned.2 as f32 * scale_x) as i32, (turned.3 as f32 * scale_y) as i32);
        let rounded = (
            (turned.0 as f32 * scale_x).round() as i32,
            (turned.1 as f32 * scale_y).round() as i32,
            (turned.2 as f32 * scale_x).round() as i32,
            (turned.3 as f32 * scale_y).round() as i32,
        );
        let frame = manager.capture_output_region(0, wl_output, logical.0, logical.1, logical.2, logical.3, qh, FrameData(id));
        Capture { output, area, geometry, boxes: [truncated, rounded], frame, format: None, buffer: None, y_invert: false }
    }

    pub fn output(&self) -> usize {
        self.output
    }

    /// Give the compositor a buffer to copy into.
    fn copy(&mut self, pool: &mut SlotPool) -> bool {
        let Some((format, width, height, stride)) = self.format else { return false };
        match pool.create_buffer(width, height, stride, format) {
            Ok((buffer, _)) => {
                self.frame.copy(buffer.wl_buffer());
                self.buffer = Some(buffer);
                true
            }
            Err(_) => false,
        }
    }

    /// The pixels read, once the copy is done.
    fn pixels(&self, pool: &mut SlotPool) -> Option<Pixels> {
        let (format, width, height, stride) = self.format?;
        let data = self.buffer.as_ref()?.canvas(pool)?;
        let bytes = bytes_per_pixel(format)?;
        // The box the compositor used is the one whose size matches the buffer.
        let bbox = self.boxes.iter().copied().find(|b| (b.2, b.3) == (width, height)).unwrap_or((self.boxes[0].0, self.boxes[0].1, width, height));
        let upright = self.geometry.pixels;
        let (x0, y0, x1, y1) = self.area;
        let mut rgb = Vec::with_capacity(((x1 - x0).max(0) * (y1 - y0).max(0)) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let (bx, by) = to_buffer(self.geometry.transform, (x, y), upright);
                let (fx, mut fy) = (bx - bbox.0, by - bbox.1);
                if self.y_invert {
                    fy = height - 1 - fy;
                }
                let color = (fx >= 0 && fy >= 0 && fx < width && fy < height)
                    .then(|| {
                        let at = fy as usize * stride as usize + fx as usize * bytes;
                        decode(format, data.get(at..at + bytes)?)
                    })
                    .flatten();
                rgb.push(color);
            }
        }
        Some(Pixels { area: self.area, rgb })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.frame.destroy();
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, FrameData> for State {
    fn event(
        state: &mut State,
        frame: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        data: &FrameData,
        _: &wayland_client::Connection,
        _: &QueueHandle<State>,
    ) {
        use zwlr_screencopy_frame_v1::Event;
        let id = data.0;
        let Some(capture) = state.captures.get_mut(&id) else { return };
        match event {
            Event::Buffer { format: WEnum::Value(format), width, height, stride } => {
                if bytes_per_pixel(format).is_some() && width > 0 && height > 0 {
                    capture.format = Some((format, width as i32, height as i32, stride as i32));
                }
                // Before version 3 no buffer_done follows: copy right away.
                if frame.version() < 3 && !capture.copy(&mut state.pool) {
                    state.capture_done(id, None);
                }
            }
            Event::BufferDone => {
                if !capture.copy(&mut state.pool) {
                    state.capture_done(id, None);
                }
            }
            Event::Flags { flags } => {
                capture.y_invert = matches!(flags, WEnum::Value(f) if f.contains(zwlr_screencopy_frame_v1::Flags::YInvert));
            }
            Event::Ready { .. } => {
                let pixels = capture.pixels(&mut state.pool);
                state.capture_done(id, pixels);
            }
            Event::Failed => state.capture_done(id, None),
            _ => {}
        }
    }
}

fn invert(t: Transform) -> Transform {
    match t {
        Transform::_90 => Transform::_270,
        Transform::_270 => Transform::_90,
        other => other,
    }
}

/// A box of a `size` space, transformed (as `wlr_box_transform`).
fn transform_box(t: Transform, (x, y, w, h): (i32, i32, i32, i32), (width, height): (i32, i32)) -> (i32, i32, i32, i32) {
    let (w2, h2) = if t as u32 % 2 == 1 { (h, w) } else { (w, h) };
    let (x2, y2) = match t {
        Transform::_90 => (height - y - h, x),
        Transform::_180 => (width - x - w, height - y - h),
        Transform::_270 => (y, width - x - w),
        Transform::Flipped => (width - x - w, y),
        Transform::Flipped90 => (y, x),
        Transform::Flipped180 => (x, height - y - h),
        Transform::Flipped270 => (height - y - h, width - x - w),
        _ => (x, y),
    };
    (x2, y2, w2, h2)
}

/// Where an upright pixel of an output (`size` upright pixels) is in its buffer.
fn to_buffer(t: Transform, p: Pixel, size: (i32, i32)) -> Pixel {
    let (x, y, _, _) = transform_box(invert(t), (p.0, p.1, 1, 1), size);
    (x, y)
}

fn bytes_per_pixel(format: Format) -> Option<usize> {
    match format {
        Format::Argb8888 | Format::Xrgb8888 | Format::Abgr8888 | Format::Xbgr8888 => Some(4),
        Format::Argb2101010 | Format::Xrgb2101010 | Format::Abgr2101010 | Format::Xbgr2101010 => Some(4),
        Format::Rgb888 | Format::Bgr888 => Some(3),
        _ => None,
    }
}

/// RGB of a pixel in one of the formats above (all little-endian).
fn decode(format: Format, p: &[u8]) -> Option<[u8; 3]> {
    let word = |p: &[u8]| u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
    let ten = |v: u32| ((v & 0x3ff) * 255 + 511) / 1023;
    Some(match format {
        Format::Argb8888 | Format::Xrgb8888 => [p[2], p[1], p[0]],
        Format::Abgr8888 | Format::Xbgr8888 => [p[0], p[1], p[2]],
        Format::Argb2101010 | Format::Xrgb2101010 => {
            let v = word(p);
            [ten(v >> 20) as u8, ten(v >> 10) as u8, ten(v) as u8]
        }
        Format::Abgr2101010 | Format::Xbgr2101010 => {
            let v = word(p);
            [ten(v) as u8, ten(v >> 10) as u8, ten(v >> 20) as u8]
        }
        Format::Rgb888 => [p[2], p[1], p[0]],
        Format::Bgr888 => [p[0], p[1], p[2]],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_are_found_in_turned_buffers() {
        // An output 4 × 2 pixels upright; its buffer, turned 90°, is 2 × 4.
        let size = (4, 2);
        for t in [
            Transform::Normal,
            Transform::_90,
            Transform::_180,
            Transform::_270,
            Transform::Flipped,
            Transform::Flipped90,
            Transform::Flipped180,
            Transform::Flipped270,
        ] {
            let buffer = if t as u32 % 2 == 1 { (2, 4) } else { (4, 2) };
            let mut seen = std::collections::HashSet::new();
            for y in 0..2 {
                for x in 0..4 {
                    let b = to_buffer(t, (x, y), size);
                    assert!(b.0 >= 0 && b.1 >= 0 && b.0 < buffer.0 && b.1 < buffer.1, "{t:?}: {b:?}");
                    assert!(seen.insert(b), "{t:?}: two pixels at {b:?}");
                }
            }
        }
        assert_eq!(to_buffer(Transform::Normal, (3, 1), size), (3, 1));
        assert_eq!(to_buffer(Transform::_180, (0, 0), size), (3, 1));
    }

    #[test]
    fn formats_are_decoded() {
        assert_eq!(decode(Format::Xrgb8888, &[3, 2, 1, 0]), Some([1, 2, 3]));
        assert_eq!(decode(Format::Xbgr8888, &[1, 2, 3, 0]), Some([1, 2, 3]));
        let v: u32 = (1023 << 20) | (512 << 10);
        assert_eq!(decode(Format::Xrgb2101010, &v.to_le_bytes()), Some([255, 128, 0]));
    }
}
