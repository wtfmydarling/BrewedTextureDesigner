// The application icon, drawn in code: a mug of beer on a dark rounded square, with one
// corner peeled off to show the magenta and black checker of a missing texture. The window, the taskbar and the executable (build.rs includes this
// file) all use this artwork. Plain `std` only, so the build script can compile it too.

/// Sizes embedded in the Windows executable (used by build.rs and the tests).
#[allow(dead_code)]
pub const ICON_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];


/// RGBA pixels (straight alpha) of the icon at `size`×`size`.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 6;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let small = size <= 24;
    for py in 0..size {
        for px in 0..size {
            // Supersampled coverage, premultiplied accumulation.
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    let c = shade(x, y, small);
                    for i in 0..3 {
                        acc[i] += c[i] * c[3];
                    }
                    acc[3] += c[3];
                }
            }
            let n = (SS * SS) as f32;
            let a = acc[3] / n;
            let unpremultiply = |v: f32| if a > 0.0 { (v / n / a).clamp(0.0, 1.0) } else { 0.0 };
            out.extend([
                (unpremultiply(acc[0]) * 255.0).round() as u8,
                (unpremultiply(acc[1]) * 255.0).round() as u8,
                (unpremultiply(acc[2]) * 255.0).round() as u8,
                (a * 255.0).round() as u8,
            ]);
        }
    }
    out
}

/// Straight RGBA (0..1) at a point of the unit square.
fn shade(x: f32, y: f32, small: bool) -> [f32; 4] {
    let rgb = |c: [u8; 3]| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0];
    let mix = |a: [f32; 4], b: [f32; 4], t: f32| [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t.clamp(0.0, 1.0));
    // Background: rounded square with a subtle vertical gradient and rim.
    let (inset, radius) = if small { (0.02, 0.16) } else { (0.04, 0.2) };
    if !in_rounded_rect(x, y, inset, inset, 1.0 - inset, 1.0 - inset, radius) {
        return [0.0; 4];
    }

    // The mug: foam on top, amber beer in a glass, a handle on the right.
    let (mx0, my0, mx1, my1) = if small { (0.12, 0.30, 0.64, 0.92) } else { (0.16, 0.32, 0.62, 0.88) };
    let wall = if small { 0.055 } else { 0.035 };
    let glass = rgb([214, 220, 226]);

    // Foam, overflowing the rim a little: a band with round heads above it.
    let heads: &[(f32, f32)] = if small {
        &[(0.22, 0.29), (0.38, 0.24), (0.54, 0.28)]
    } else {
        &[(0.21, 0.31), (0.31, 0.25), (0.43, 0.27), (0.54, 0.24), (0.62, 0.30)]
    };
    let head_r = if small { 0.1 } else { 0.075 };
    let in_band = x >= mx0 - 0.015 && x <= mx1 + 0.015 && y >= my0 - 0.02 && y <= my0 + 0.06;
    let in_head = heads.iter().any(|&(cx, cy)| (x - cx).powi(2) + (y - cy).powi(2) <= head_r * head_r);
    if in_band || in_head {
        let shadow = ((y - (my0 + 0.01)) / 0.05).clamp(0.0, 1.0);
        return mix(rgb([252, 248, 238]), rgb([226, 214, 190]), shadow);
    }

    // Handle: the right half of a ring beside the glass.
    let (hx, hy) = (mx1, (my0 + my1) * 0.5 + 0.02);
    let (outer, thick) = if small { (0.2, 0.08) } else { (0.17, 0.055) };
    let d = ((x - hx).powi(2) + (y - hy).powi(2)).sqrt();
    if x > mx1 - wall && d <= outer && d >= outer - thick {
        return glass;
    }

    if in_rounded_rect(x, y, mx0, my0, mx1, my1, 0.05) {
        // The glass: its walls and a thick bottom.
        let bottom = if small { 0.07 } else { 0.05 };
        if !in_rounded_rect(x, y, mx0 + wall, my0, mx1 - wall, my1 - bottom, 0.025) {
            return glass;
        }
        // The beer: lighter at the top, deeper at the bottom, with bubbles rising and a
        // shine along the glass.
        let t = (y - my0) / (my1 - my0);
        let beer = mix(rgb([252, 200, 64]), rgb([206, 118, 16]), t);
        if !small {
            let bubbles = [(0.34, 0.52, 0.018), (0.46, 0.64, 0.014), (0.29, 0.72, 0.012), (0.52, 0.47, 0.012), (0.41, 0.78, 0.016)];
            if bubbles.iter().any(|&(bx, by, r)| (x - bx).powi(2) + (y - by).powi(2) <= r * r) {
                return mix(beer, rgb([255, 236, 170]), 0.8);
            }
        }
        let (s0, s1) = (mx0 + wall + 0.02, mx0 + wall + if small { 0.07 } else { 0.05 });
        if x >= s0 && x <= s1 && y >= my0 + 0.1 && y <= my1 - 0.12 {
            return mix(beer, rgb([255, 244, 214]), 0.55);
        }
        return beer;
    }

    // A corner of the icon peels off to show what is under every texture that is not
    // there yet: the magenta and black checker of a game engine's missing texture.
    let fold = if small { 0.52 } else { 0.56 };
    if x - y > fold {
        let cells = if small { 6.0 } else { 10.0 };
        let (cx, cy) = ((x * cells).floor() as i32, (y * cells).floor() as i32);
        let checker = if (cx + cy).rem_euclid(2) == 0 { rgb([232, 0, 230]) } else { rgb([18, 18, 18]) };
        // A shadow along the fold.
        let edge = if small { 0.0 } else { 0.03 };
        if x - y < fold + edge {
            return mix(rgb([10, 10, 10]), checker, (x - y - fold) / edge.max(1e-6));
        }
        return checker;
    }

    let top = rgb([46, 46, 46]);
    let bottom = rgb([26, 26, 26]);
    let mut c = mix(top, bottom, y);
    // A light rim along the edge.
    let rim = if small { 0.0 } else { 0.014 };
    if rim > 0.0 && !in_rounded_rect(x, y, inset + rim, inset + rim, 1.0 - inset - rim, 1.0 - inset - rim, radius - rim) {
        c = mix(c, rgb([84, 84, 84]), 0.6);
    }
    c
}

fn in_rounded_rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy <= r * r
}
