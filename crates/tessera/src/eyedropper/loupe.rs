//! The loupe shown while picking one color: a round magnifier of the pixels around the
//! cursor, the picked one outlined in the middle. It is the mouse cursor itself (where
//! the platform allows), so it follows the mouse without lag and never shows up in what
//! is read from the screen.

/// Pixels across the loupe (odd, so that one is in the middle).
pub const PIXELS: usize = 13;
/// Largest loupe, in pixels: bigger cursors are not supported everywhere.
const MAX_SIZE: u32 = 256;

/// A rendered loupe: premultiplied RGBA, `size` × `size`.
pub struct Loupe {
    pub size: u32,
    /// The point of the image over the picked pixel.
    pub hotspot: (u32, u32),
    pub rgba: Vec<u8>,
}

/// Render the loupe for the `PIXELS` × `PIXELS` screen pixels around the cursor (row by
/// row, RGB; `None` off the screen) at a display scale factor.
pub fn render(pixels: &[Option<[u8; 3]>], scale: f32) -> Loupe {
    debug_assert_eq!(pixels.len(), PIXELS * PIXELS);
    let scale = if scale.is_finite() { scale.clamp(1.0, 4.0) } else { 1.0 };
    let ring = (2.0 * scale).round().max(2.0) as u32;
    let cell = ((9.0 * scale).round() as u32).clamp(5, (MAX_SIZE - 2 * ring - 2) / PIXELS as u32);
    let inner = cell * PIXELS as u32;
    let size = inner + 2 * ring + 2;
    let center = size as f32 / 2.0;
    let (inner_r, outer_r) = (inner as f32 / 2.0, inner as f32 / 2.0 + ring as f32);
    let origin = (size - inner) / 2;
    let middle = PIXELS as u32 / 2;
    // The picked pixel, outlined: white inside its edge, black around it.
    let (m0, m1) = (origin + middle * cell, origin + (middle + 1) * cell);
    let off_screen = [48u8, 48, 48];

    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5 - center, y as f32 + 0.5 - center);
            let r = fx.hypot(fy);
            let coverage = (outer_r + 0.5 - r).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let magnified = |x: u32, y: u32| -> [u8; 3] {
                let cx = (x.saturating_sub(origin) / cell).min(PIXELS as u32 - 1) as usize;
                let cy = (y.saturating_sub(origin) / cell).min(PIXELS as u32 - 1) as usize;
                pixels[cy * PIXELS + cx].unwrap_or(off_screen)
            };
            let inside = |a: u32, b: u32| (a..b).contains(&x) && (a..b).contains(&y);
            let mut color = if inside(m0, m1) && !inside(m0 + 1, m1 - 1) {
                [255, 255, 255]
            } else if inside(m0 - 1, m1 + 1) && !inside(m0, m1) {
                [0, 0, 0]
            } else {
                magnified(x, y)
            };
            // The ring, blended with the pixels at its inner edge.
            let ring_color = [28u8, 28, 28];
            let in_ring = (r - inner_r + 0.5).clamp(0.0, 1.0);
            for c in 0..3 {
                color[c] = (color[c] as f32 * (1.0 - in_ring) + ring_color[c] as f32 * in_ring).round() as u8;
            }
            let i = ((y * size + x) * 4) as usize;
            let a = coverage;
            rgba[i] = (color[0] as f32 * a).round() as u8;
            rgba[i + 1] = (color[1] as f32 * a).round() as u8;
            rgba[i + 2] = (color[2] as f32 * a).round() as u8;
            rgba[i + 3] = (255.0 * a).round() as u8;
        }
    }
    let hot = origin + middle * cell + cell / 2;
    Loupe { size, hotspot: (hot, hot), rgba }
}

impl Loupe {
    /// The image with straight (not premultiplied) alpha.
    pub fn straight_rgba(&self) -> Vec<u8> {
        let mut out = self.rgba.clone();
        for p in out.chunks_exact_mut(4) {
            let a = p[3] as f32 / 255.0;
            if a > 0.0 && a < 1.0 {
                for c in &mut p[..3] {
                    *c = (*c as f32 / a).round().min(255.0) as u8;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(l: &Loupe, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * l.size + x) * 4) as usize;
        [l.rgba[i], l.rgba[i + 1], l.rgba[i + 2], l.rgba[i + 3]]
    }

    #[test]
    fn the_middle_pixel_is_under_the_hotspot() {
        let mut pixels = vec![Some([0u8, 0, 255]); PIXELS * PIXELS];
        pixels[PIXELS * PIXELS / 2] = Some([200, 10, 10]);
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0, 10.0, f32::NAN] {
            let l = render(&pixels, scale);
            assert!(l.size <= MAX_SIZE, "{} at {scale}", l.size);
            assert_eq!(l.rgba.len(), (l.size * l.size * 4) as usize);
            let (hx, hy) = l.hotspot;
            assert_eq!(pixel(&l, hx, hy), [200, 10, 10, 255], "scale {scale}");
            // Neighbors are magnified, the corners are transparent.
            assert_eq!(pixel(&l, hx, hy - l.size / 4), [0, 0, 255, 255]);
            assert_eq!(pixel(&l, 0, 0)[3], 0);
        }
    }

    #[test]
    fn off_screen_pixels_are_gray() {
        let l = render(&vec![None; PIXELS * PIXELS], 1.0);
        let (hx, hy) = l.hotspot;
        assert_eq!(pixel(&l, hx, hy + l.size / 4), [48, 48, 48, 255]);
    }

    #[test]
    fn snapshot() {
        // A color wheel around a white pixel, at 100% and 200%.
        let pixels: Vec<Option<[u8; 3]>> = (0..PIXELS * PIXELS)
            .map(|i| {
                let (x, y) = ((i % PIXELS) as f32 - 6.0, (i / PIXELS) as f32 - 6.0);
                if x == 0.0 && y == 0.0 {
                    return Some([255, 255, 255]);
                }
                let h = (y.atan2(x) / std::f32::consts::TAU + 0.5) * 6.0;
                let c = |k: f32| ((((h + k) % 6.0) - 3.0).abs() - 1.0).clamp(0.0, 1.0);
                (x > -6.0).then(|| [c(0.0), c(4.0), c(2.0)].map(|v| (v * 255.0) as u8))
            })
            .collect();
        for scale in [1.0, 2.0] {
            let l = render(&pixels, scale);
            crate::eyedropper::save_snapshot(&format!("eyedropper_loupe_{scale}x"), l.size, l.size, &l.rgba, true);
        }
    }
}
