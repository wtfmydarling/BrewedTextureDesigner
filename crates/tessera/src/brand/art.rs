// The application icon, drawn in code: a mug of beer on a dark rounded square — the foam
// on top, and the beer laid out as a tiled texture of amber bricks. The window, the taskbar and the executable (build.rs includes this
// file) all use this artwork. Plain `std` only, so the build script can compile it too.

/// Sizes embedded in the Windows executable (used by build.rs and the tests).
#[allow(dead_code)]
pub const ICON_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

const BRIGHT: [[u8; 3]; 5] = [[244, 184, 96], [236, 166, 66], [226, 148, 46], [230, 156, 56], [216, 136, 38]];

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
    // Background: rounded square with a subtle vertical gradient and rim.
    let (inset, radius) = if small { (0.02, 0.16) } else { (0.04, 0.2) };
    if !in_rounded_rect(x, y, inset, inset, 1.0 - inset, 1.0 - inset, radius) {
        return [0.0; 4];
    }

    // The mug: foam on top, a glass with a handle, and in it the beer — laid out as a
    // tiled texture, bricks of amber with darker joints.
    let (mx0, my0, mx1, my1) = if small { (0.14, 0.30, 0.66, 0.90) } else { (0.18, 0.32, 0.66, 0.86) };
    let wall = if small { 0.06 } else { 0.04 };

    // Foam: a band along the rim with round heads above it.
    let heads: &[(f32, f32)] = if small {
        &[(0.24, 0.29), (0.40, 0.25), (0.56, 0.29)]
    } else {
        &[(0.24, 0.31), (0.35, 0.26), (0.47, 0.28), (0.58, 0.25), (0.65, 0.31)]
    };
    let head_r = if small { 0.1 } else { 0.075 };
    let in_band = x >= mx0 - 0.01 && x <= mx1 + 0.01 && y >= my0 - 0.02 && y <= my0 + 0.07;
    let in_head = heads.iter().any(|&(cx, cy)| (x - cx).powi(2) + (y - cy).powi(2) <= head_r * head_r);
    if in_band || in_head {
        // A little shade on the lower side of the foam.
        return if y > my0 + 0.04 { rgb([226, 218, 200]) } else { rgb([248, 243, 230]) };
    }

    // Handle: the right half of a ring beside the glass.
    let (hx, hy, outer, thick) = (mx1, (my0 + my1) * 0.52, if small { 0.2 } else { 0.17 }, if small { 0.08 } else { 0.06 });
    let d = ((x - hx).powi(2) + (y - hy).powi(2)).sqrt();
    if x > mx1 - wall * 0.5 && d <= outer && d >= outer - thick {
        return rgb([196, 196, 196]);
    }

    if in_rounded_rect(x, y, mx0, my0, mx1, my1, 0.06) {
        // The glass wall.
        if !in_rounded_rect(x, y, mx0 + wall, my0, mx1 - wall, my1 - wall, 0.03) {
            return rgb([196, 196, 196]);
        }
        // The beer: a brick bond of tiles, the texture being designed.
        let (ix0, iy0, ix1, iy1) = (mx0 + wall, my0 + 0.07, mx1 - wall, my1 - wall);
        let (cols, rows) = if small { (2.0, 3.0) } else { (3.0, 4.0) };
        let (w, h) = ((ix1 - ix0) / cols, (iy1 - iy0) / rows);
        let row = ((y - iy0) / h).floor().max(0.0);
        let shift = if row as i32 % 2 == 1 { w * 0.5 } else { 0.0 };
        let col = ((x - ix0 + shift) / w).floor();
        let (u, v) = ((x - ix0 + shift) / w - col, (y - iy0) / h - row);
        let joint = if small { 0.1 } else { 0.08 };
        if u < joint || v < joint * (w / h) {
            return rgb([124, 70, 22]);
        }
        let i = ((row as i32 * 3 + col as i32).rem_euclid(BRIGHT.len() as i32)) as usize;
        return rgb(BRIGHT[i]);
    }

    let top = rgb([46, 46, 46]);
    let bottom = rgb([26, 26, 26]);
    let t = y.clamp(0.0, 1.0);
    let mut c = [0.0, 0.0, 0.0, 1.0];
    for i in 0..3 {
        c[i] = top[i] + (bottom[i] - top[i]) * t;
    }
    // A light rim along the edge.
    let rim = if small { 0.0 } else { 0.014 };
    if rim > 0.0 && !in_rounded_rect(x, y, inset + rim, inset + rim, 1.0 - inset - rim, 1.0 - inset - rim, radius - rim) {
        let edge = rgb([84, 84, 84]);
        for i in 0..3 {
            c[i] = c[i] * 0.4 + edge[i] * 0.6;
        }
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
