//! How the path of a gradient pick is drawn: a black line with a light, half transparent
//! edge (so that it shows on dark colors too), antialiased.

use super::path::Pixel;

/// Distances are kept per pixel in 1/16 of a pixel, up to this (255: far from the path).
const FAR: u8 = 255;

#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    /// Radius of the black line, in pixels.
    core: f32,
    /// Radius of the light edge around it.
    edge: f32,
}

/// A rectangle of pixels: left, top, right, bottom (exclusive).
pub type Area = (i32, i32, i32, i32);

impl Stroke {
    /// The stroke for a display scale factor (2 pixels wide at 100%).
    pub fn new(scale: f32) -> Stroke {
        let scale = if scale.is_finite() { scale.clamp(1.0, 4.0) } else { 1.0 };
        Stroke { core: 1.0 * scale, edge: 2.0 * scale }
    }

    /// The pixels a segment of the path may touch.
    pub fn bounds(&self, a: Pixel, b: Pixel) -> Area {
        let r = self.edge.ceil() as i32 + 1;
        (a.0.min(b.0) - r, a.1.min(b.1) - r, a.0.max(b.0) + r + 1, a.1.max(b.1) + r + 1)
    }

    /// Whether a pixel at this distance from the path is covered by it.
    pub fn covers(&self, distance: f32) -> bool {
        distance < self.edge + 0.5
    }

    /// Premultiplied RGBA (all channels gray, so BGRA alike) at a distance from the path.
    pub fn shade(&self, distance: f32) -> [u8; 4] {
        let black = (self.core + 0.5 - distance).clamp(0.0, 1.0);
        let light = (self.edge + 0.5 - distance).clamp(0.0, 1.0) * 0.55 * (1.0 - black);
        let v = (light * 255.0).round() as u8;
        [v, v, v, ((black + light) * 255.0).round() as u8]
    }

    #[cfg(any(windows, test))]
    /// Draw a segment of the path into an image covering `area`, where `distance` keeps
    /// how far each pixel is from the path drawn so far. Returns the pixels changed.
    pub fn draw(&self, a: Pixel, b: Pixel, area: Area, distance: &mut [u8], pixels: &mut [u8]) -> Area {
        let changed = self.mark(a, b, area, distance);
        self.render(area, distance, changed, pixels);
        changed
    }

    /// Add a segment of the path to the distances only (see [`draw`](Stroke::draw));
    /// returns the pixels to render again.
    pub fn mark(&self, a: Pixel, b: Pixel, area: Area, distance: &mut [u8]) -> Area {
        let (x0, y0, x1, y1) = area;
        let width = (x1 - x0) as usize;
        let (bx0, by0, bx1, by1) = self.bounds(a, b);
        let clip = (bx0.max(x0), by0.max(y0), bx1.min(x1), by1.min(y1));
        for y in clip.1..clip.3 {
            for x in clip.0..clip.2 {
                let i = (y - y0) as usize * width + (x - x0) as usize;
                let d = (segment_distance(a, b, (x, y)) * 16.0).min(FAR as f32) as u8;
                if let Some(old) = distance.get_mut(i)
                    && d < *old
                {
                    *old = d;
                }
            }
        }
        clip
    }

    /// Render `rect` into an image of `size` pixels covering the same area (coarser, or
    /// the same); returns the pixels of the image changed.
    #[cfg_attr(not(free_unix), allow(dead_code, reason = "Wayland surfaces may show the path scaled"))]
    pub fn render_scaled(&self, area: Area, distance: &[u8], rect: Area, pixels: &mut [u8], size: (i32, i32)) -> Area {
        let (aw, ah) = (area.2 - area.0, area.3 - area.1);
        if (aw, ah) == size {
            self.render(area, distance, rect, pixels);
            return (rect.0 - area.0, rect.1 - area.1, rect.2 - area.0, rect.3 - area.1);
        }
        let (fx, fy) = (size.0 as f32 / aw.max(1) as f32, size.1 as f32 / ah.max(1) as f32);
        let out = (
            (((rect.0 - area.0) as f32 * fx).floor() as i32).clamp(0, size.0),
            (((rect.1 - area.1) as f32 * fy).floor() as i32).clamp(0, size.1),
            (((rect.2 - area.0) as f32 * fx).ceil() as i32).clamp(0, size.0),
            (((rect.3 - area.1) as f32 * fy).ceil() as i32).clamp(0, size.1),
        );
        for y in out.1..out.3 {
            for x in out.0..out.2 {
                let ax = (((x as f32 + 0.5) / fx) as i32).clamp(0, aw - 1);
                let ay = (((y as f32 + 0.5) / fy) as i32).clamp(0, ah - 1);
                let d = distance.get((ay * aw + ax) as usize).copied().unwrap_or(FAR);
                let i = (y * size.0 + x) as usize * 4;
                if let Some(px) = pixels.get_mut(i..i + 4) {
                    px.copy_from_slice(&if d == FAR { [0; 4] } else { self.shade(d as f32 / 16.0) });
                }
            }
        }
        out
    }

    /// Render the pixels of `rect` from the distances (premultiplied, gray: RGBA and BGRA
    /// alike).
    pub fn render(&self, area: Area, distance: &[u8], rect: Area, pixels: &mut [u8]) {
        let (x0, y0, x1, y1) = area;
        let width = (x1 - x0) as usize;
        for y in rect.1.max(y0)..rect.3.min(y1) {
            for x in rect.0.max(x0)..rect.2.min(x1) {
                let i = (y - y0) as usize * width + (x - x0) as usize;
                let (Some(d), Some(px)) = (distance.get(i), pixels.get_mut(i * 4..i * 4 + 4)) else { continue };
                px.copy_from_slice(&if *d == FAR { [0; 4] } else { self.shade(*d as f32 / 16.0) });
            }
        }
    }

    /// Whether the path drawn so far covers a pixel of the image.
    pub fn covered(&self, area: Area, distance: &[u8], p: Pixel) -> bool {
        let (x0, y0, x1, y1) = area;
        if p.0 < x0 || p.1 < y0 || p.0 >= x1 || p.1 >= y1 {
            return false;
        }
        let i = (p.1 - y0) as usize * (x1 - x0) as usize + (p.0 - x0) as usize;
        distance.get(i).is_some_and(|d| self.covers(*d as f32 / 16.0))
    }
}

/// A distance map with nothing drawn yet.
pub fn empty_distances(area: Area) -> Vec<u8> {
    let (x0, y0, x1, y1) = area;
    vec![FAR; ((x1 - x0).max(0) as usize) * ((y1 - y0).max(0) as usize)]
}

/// Distance from a pixel to a segment (all at pixel centers).
pub fn segment_distance(a: Pixel, b: Pixel, p: Pixel) -> f32 {
    let (px, py) = (p.0 as f32, p.1 as f32);
    let (ax, ay, bx, by) = (a.0 as f32, a.1 as f32, b.0 as f32, b.1 as f32);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (px - ax - t * dx).hypot(py - ay - t * dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_black_with_a_light_edge() {
        let stroke = Stroke::new(1.0);
        let area = (0, 0, 64, 40);
        let mut distance = empty_distances(area);
        let mut pixels = vec![0u8; 64 * 40 * 4];
        let path = [(4, 30), (20, 8), (40, 32), (60, 10)];
        for w in path.windows(2) {
            stroke.draw(w[0], w[1], area, &mut distance, &mut pixels);
        }
        let at = |x: usize, y: usize| &pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        // On the path: opaque black; a little away: the light edge; far: nothing.
        assert_eq!(at(20, 8), [0, 0, 0, 255]);
        assert_eq!(at(20, 6), stroke.shade(2.0));
        assert!(stroke.shade(2.0)[0] > 50 && stroke.shade(2.0)[3] > 50);
        assert_eq!(at(20, 20), [0, 0, 0, 0]);
        assert!(stroke.covered(area, &distance, (40, 32)));
        assert!(!stroke.covered(area, &distance, (40, 20)));
        assert!(!stroke.covered(area, &distance, (400, 20)));
        // Segments partly outside the image are clipped.
        stroke.draw((-50, -50), (100, 100), area, &mut distance, &mut pixels);
        crate::eyedropper::save_snapshot("eyedropper_stroke", 64, 40, &pixels, true);
    }
}
