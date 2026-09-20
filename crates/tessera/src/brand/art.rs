// The application icon, drawn in code: a 3×3 mosaic whose bright tiles form a "T", on a
// dark rounded square. The window, the taskbar and the executable (build.rs includes this
// file) all use this artwork. Plain `std` only, so the build script can compile it too.

/// Sizes embedded in the Windows executable (used by build.rs and the tests).
#[allow(dead_code)]
pub const ICON_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

const BRIGHT: [[u8; 3]; 5] = [[244, 184, 96], [236, 166, 66], [226, 148, 46], [230, 156, 56], [216, 136, 38]];
const DIM: [[u8; 3]; 4] = [[68, 68, 68], [60, 60, 60], [64, 64, 64], [56, 56, 56]];

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
    let rgb = |c: [u8; 3]| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
    // Background: rounded square with a subtle vertical gradient and rim.
    let (inset, radius) = if small { (0.02, 0.16) } else { (0.04, 0.2) };
    if !in_rounded_rect(x, y, inset, inset, 1.0 - inset, 1.0 - inset, radius) {
        return [0.0; 4];
    }
    // Tiles: 3×3 grid.
    let (g0, g1) = if small { (0.14, 0.86) } else { (0.2, 0.8) };
    let cell = (g1 - g0) / 3.0;
    let gap = cell * if small { 0.16 } else { 0.14 };
    if x >= g0 && x < g1 && y >= g0 && y < g1 {
        let col = ((x - g0) / cell).floor() as usize;
        let row = ((y - g0) / cell).floor() as usize;
        let (x0, y0) = (g0 + col as f32 * cell + gap * 0.5, g0 + row as f32 * cell + gap * 0.5);
        if in_rounded_rect(x, y, x0, y0, x0 + cell - gap, y0 + cell - gap, cell * 0.16) {
            let bright = row == 0 || col == 1;
            let c = if bright {
                let i = if row == 0 { col } else { 2 + row };
                BRIGHT[i]
            } else {
                DIM[(row / 2) * 2 + col / 2]
            };
            let [r, g, b] = rgb(c);
            return [r, g, b, 1.0];
        }
    }
    let top = rgb([46, 46, 46]);
    let bottom = rgb([26, 26, 26]);
    let t = y.clamp(0.0, 1.0);
    let mut c = [0.0; 3];
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
    [c[0], c[1], c[2], 1.0]
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
