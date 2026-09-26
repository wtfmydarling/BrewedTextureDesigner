//! Multithreaded SIMD pixel kernels for the CPU side of the pipeline.
//!
//! The GPU does all node processing. The CPU still has bulk pixel work to do:
//! decoding bitmaps for upload, converting GPU readbacks for export, and computing
//! histograms/statistics for the Levels node and the 2D view. These kernels split the
//! work across all cores with rayon and process 8 lanes at a time with `wide::f32x8`
//! (AVX/AVX2 when the build targets the native CPU, see `.cargo/config.toml`).
//! Every kernel has a scalar tail and is tested against a scalar reference.

use half::f16;
use half::slice::HalfFloatSliceExt;
use rayon::prelude::*;
use wide::{f32x8, i32x8};

use crate::gpu::StoredImage;

/// Floats per parallel task: 64K floats = 16K RGBA pixels = 256 KiB.
const TASK: usize = 1 << 16;

/// Pixels converted at a time from an image as it is stored (see [`StoredImage`]): 16 MiB as
/// RGBA floats, whatever the size of the image.
pub const BAND: u32 = 1 << 20;

#[inline(always)]
fn load8(s: &[f32]) -> f32x8 {
    f32x8::new([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]])
}

/// Half floats (as raw bits) to f32, parallel. Uses F16C via the `half` crate.
pub fn f16_to_f32(src: &[u16], dst: &mut [f32]) {
    assert_eq!(src.len(), dst.len());
    dst.par_chunks_mut(TASK).zip(src.par_chunks(TASK)).for_each(|(d, s)| {
        let s: &[f16] = bytemuck::cast_slice(s);
        s.convert_to_f32_slice(d);
    });
}

/// f32 to half floats (as raw bits), parallel.
pub fn f32_to_f16(src: &[f32], dst: &mut [u16]) {
    assert_eq!(src.len(), dst.len());
    dst.par_chunks_mut(TASK).zip(src.par_chunks(TASK)).for_each(|(d, s)| {
        let d: &mut [f16] = bytemuck::cast_slice_mut(d);
        d.convert_from_f32_slice(s);
    });
}

/// Clamp to [0, 1] and quantize to 8 bits with rounding. NaN becomes 0.
pub fn quantize_u8(src: &[f32], dst: &mut [u8]) {
    assert_eq!(src.len(), dst.len());
    dst.par_chunks_mut(TASK).zip(src.par_chunks(TASK)).for_each(|(d, s)| {
        let zero = f32x8::splat(0.0);
        let one = f32x8::splat(1.0);
        let scale = f32x8::splat(255.0);
        let half = f32x8::splat(0.5);
        let (s_chunks, s_rest) = s.as_chunks::<8>();
        let (d_chunks, d_rest) = d.as_chunks_mut::<8>();
        for (sc, dc) in s_chunks.iter().zip(d_chunks) {
            let v = load8(sc).max(zero).min(one).mul_add(scale, half);
            let q = v.trunc_int().to_array();
            for (o, q) in dc.iter_mut().zip(q) {
                *o = q as u8;
            }
        }
        for (o, v) in d_rest.iter_mut().zip(s_rest) {
            *o = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    });
}

/// Clamp to [0, 1] and quantize to 16 bits with rounding. NaN becomes 0.
pub fn quantize_u16(src: &[f32], dst: &mut [u16]) {
    assert_eq!(src.len(), dst.len());
    dst.par_chunks_mut(TASK).zip(src.par_chunks(TASK)).for_each(|(d, s)| {
        let zero = f32x8::splat(0.0);
        let one = f32x8::splat(1.0);
        let scale = f32x8::splat(65535.0);
        let half = f32x8::splat(0.5);
        let (s_chunks, s_rest) = s.as_chunks::<8>();
        let (d_chunks, d_rest) = d.as_chunks_mut::<8>();
        for (sc, dc) in s_chunks.iter().zip(d_chunks) {
            let v = load8(sc).max(zero).min(one).mul_add(scale, half);
            let q = v.trunc_int().to_array();
            for (o, q) in dc.iter_mut().zip(q) {
                *o = q as u16;
            }
        }
        for (o, v) in d_rest.iter_mut().zip(s_rest) {
            *o = (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
        }
    });
}

/// 8-bit channels to normalized floats (x / 255), parallel SIMD.
pub fn u8_to_f32(src: &[u8], dst: &mut [f32]) {
    assert_eq!(src.len(), dst.len());
    dst.par_chunks_mut(TASK).zip(src.par_chunks(TASK)).for_each(|(d, s)| {
        let inv = f32x8::splat(1.0 / 255.0);
        let (s_chunks, s_rest) = s.as_chunks::<8>();
        let (d_chunks, d_rest) = d.as_chunks_mut::<8>();
        for (sc, dc) in s_chunks.iter().zip(d_chunks) {
            let i = i32x8::new([
                sc[0] as i32,
                sc[1] as i32,
                sc[2] as i32,
                sc[3] as i32,
                sc[4] as i32,
                sc[5] as i32,
                sc[6] as i32,
                sc[7] as i32,
            ]);
            dc.copy_from_slice(&(f32x8::from_i32x8(i) * inv).to_array());
        }
        for (o, v) in d_rest.iter_mut().zip(s_rest) {
            *o = *v as f32 / 255.0;
        }
    });
}

/// 16-bit channels to normalized floats (x / 65535), parallel SIMD.
pub fn u16_to_f32(src: &[u16], dst: &mut [f32]) {
    assert_eq!(src.len(), dst.len());
    dst.par_chunks_mut(TASK).zip(src.par_chunks(TASK)).for_each(|(d, s)| {
        let inv = f32x8::splat(1.0 / 65535.0);
        let (s_chunks, s_rest) = s.as_chunks::<8>();
        let (d_chunks, d_rest) = d.as_chunks_mut::<8>();
        for (sc, dc) in s_chunks.iter().zip(d_chunks) {
            let i = i32x8::new([
                sc[0] as i32,
                sc[1] as i32,
                sc[2] as i32,
                sc[3] as i32,
                sc[4] as i32,
                sc[5] as i32,
                sc[6] as i32,
                sc[7] as i32,
            ]);
            dc.copy_from_slice(&(f32x8::from_i32x8(i) * inv).to_array());
        }
        for (o, v) in d_rest.iter_mut().zip(s_rest) {
            *o = *v as f32 / 65535.0;
        }
    });
}

/// The half floats of every 8-bit value, as 8-bit channels read (x / 255): what
/// [`u8_to_f32`] and [`f32_to_f16`] make of them, looked up.
fn halves_of_u8() -> &'static [u16; 256] {
    static TABLE: std::sync::OnceLock<[u16; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|v| f16::from_f32(v as f32 * (1.0 / 255.0)).to_bits()))
}

/// The half floats of every 16-bit value (x / 65535), as [`u16_to_f32`] and [`f32_to_f16`]
/// make them: 128 KiB, made once.
fn halves_of_u16() -> &'static [u16] {
    static TABLE: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| (0..=u16::MAX).map(|v| f16::from_f32(f32::from(v) * (1.0 / 65535.0)).to_bits()).collect())
}

/// Pixels of `channels` values each (gray, gray and alpha, RGB or RGBA) as RGBA half floats,
/// each value by `half`: gray in red, green and blue, and opaque where there is no alpha.
/// Parallel, without a copy in between.
fn to_rgba_f16<T: Copy + Send + Sync>(src: &[T], channels: usize, dst: &mut [u16], half: impl Fn(T) -> u16 + Sync) {
    assert!((1..=4).contains(&channels) && src.len() / channels * 4 == dst.len());
    let one = f16::ONE.to_bits();
    let pixels = TASK / 4;
    dst.par_chunks_mut(pixels * 4).zip(src.par_chunks(pixels * channels)).for_each(|(d, s)| {
        let (out, _) = d.as_chunks_mut::<4>();
        match channels {
            1 => {
                for (o, v) in out.iter_mut().zip(s) {
                    let l = half(*v);
                    *o = [l, l, l, one];
                }
            }
            2 => {
                for (o, p) in out.iter_mut().zip(s.as_chunks::<2>().0) {
                    let l = half(p[0]);
                    *o = [l, l, l, half(p[1])];
                }
            }
            3 => {
                for (o, p) in out.iter_mut().zip(s.as_chunks::<3>().0) {
                    *o = [half(p[0]), half(p[1]), half(p[2]), one];
                }
            }
            _ => {
                for (o, p) in out.iter_mut().zip(s.as_chunks::<4>().0) {
                    *o = p.map(&half);
                }
            }
        }
    });
}

/// Pixels of `channels` 8-bit values each as RGBA half floats (see [`to_rgba_f16`]).
pub fn u8_to_rgba_f16(src: &[u8], channels: usize, dst: &mut [u16]) {
    let table = halves_of_u8();
    to_rgba_f16(src, channels, dst, |v| table[usize::from(v)]);
}

/// Pixels of `channels` 16-bit values each as RGBA half floats (see [`to_rgba_f16`]).
pub fn u16_to_rgba_f16(src: &[u16], channels: usize, dst: &mut [u16]) {
    let table = halves_of_u16();
    to_rgba_f16(src, channels, dst, |v| table[usize::from(v)]);
}

/// 8-bit values as half floats, one for one (a grayscale picture), parallel.
pub fn u8_to_f16(src: &[u8], dst: &mut [u16]) {
    assert_eq!(src.len(), dst.len());
    let table = halves_of_u8();
    dst.par_chunks_mut(TASK).zip(src.par_chunks(TASK)).for_each(|(d, s)| {
        for (o, v) in d.iter_mut().zip(s) {
            *o = table[usize::from(*v)];
        }
    });
}

/// Pick `channels` (indices into RGBA) out of packed RGBA pixels, parallel.
pub fn select_channels<T: Copy + Send + Sync + Default>(rgba: &[T], channels: &[usize]) -> Vec<T> {
    let n = channels.len();
    let pixels = rgba.len() / 4;
    if n == 4 && channels == [0, 1, 2, 3] {
        return rgba.to_vec();
    }
    let mut out = vec![T::default(); pixels * n];
    const PIX: usize = 1 << 14;
    out.par_chunks_mut(PIX * n).zip(rgba.par_chunks(PIX * 4)).for_each(|(o, s)| {
        let (pixels, _) = s.as_chunks::<4>();
        for (op, sp) in o.chunks_exact_mut(n).zip(pixels) {
            for (dst, &c) in op.iter_mut().zip(channels) {
                *dst = sp[c];
            }
        }
    });
    out
}

/// Histograms and per-channel statistics of an RGBA float image.
#[derive(Clone, Debug)]
pub struct ImageStats {
    pub width: u32,
    pub height: u32,
    /// 256 bins over [0, 1] for R, G, B, A and luminance (values outside are clamped).
    pub hist: Box<[[u32; 256]; 5]>,
    pub min: [f32; 4],
    pub max: [f32; 4],
    pub mean: [f32; 4],
}

impl ImageStats {
    pub fn channel_max_count(&self, channel: usize) -> u32 {
        self.hist[channel].iter().copied().max().unwrap_or(0)
    }
}

struct Partial {
    hist: Box<[[u32; 256]; 5]>,
    min: f32x8,
    max: f32x8,
    sum: [f64; 8],
}

impl Partial {
    fn new() -> Self {
        Self {
            hist: Box::new([[0; 256]; 5]),
            min: f32x8::splat(f32::INFINITY),
            max: f32x8::splat(f32::NEG_INFINITY),
            sum: [0.0; 8],
        }
    }

    fn merge(mut self, other: Partial) -> Partial {
        for (a, b) in self.hist.iter_mut().zip(other.hist.iter()) {
            for (x, y) in a.iter_mut().zip(b.iter()) {
                *x += *y;
            }
        }
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        for (a, b) in self.sum.iter_mut().zip(other.sum) {
            *a += b;
        }
        self
    }
}

#[inline(always)]
fn bin_of(v: f32) -> usize {
    // `as usize` saturates and maps NaN to 0.
    ((v * 256.0) as usize).min(255)
}

/// Compute histograms, min, max and mean of packed RGBA f32 pixels in parallel.
pub fn image_stats(rgba: &[f32], width: u32, height: u32) -> ImageStats {
    finish_stats(partial_stats(rgba), rgba.len() / 4, width, height)
}

/// [`image_stats`] of an image as it is stored, a band of rows at a time: no more memory than
/// a band as floats, whatever the size of the image.
pub fn stored_stats(image: &StoredImage) -> ImageStats {
    let rows = (BAND / image.width.max(1)).max(1);
    let mut partial = Partial::new();
    let mut pixels = 0;
    for y in (0..image.height).step_by(rows as usize) {
        let band = image.rows_to_f32(y..(y + rows).min(image.height));
        pixels += band.len() / 4;
        partial = partial.merge(partial_stats(&band));
    }
    finish_stats(partial, pixels, image.width, image.height)
}

/// Histograms, extremes and sums of packed RGBA f32 pixels, in parallel.
fn partial_stats(rgba: &[f32]) -> Partial {
    let pixels = rgba.len() / 4;
    let rgba = &rgba[..pixels * 4];
    rgba
        .par_chunks(TASK)
        .fold(Partial::new, |mut p, chunk| {
            let zero = f32x8::splat(0.0);
            let bins = f32x8::splat(256.0);
            let top = f32x8::splat(255.0);
            let luma_w = f32x8::new([0.2126, 0.7152, 0.0722, 0.0, 0.2126, 0.7152, 0.0722, 0.0]);
            let mut vsum = f32x8::splat(0.0);
            let mut vmin = p.min;
            let mut vmax = p.max;
            let (eights, tail) = chunk.as_chunks::<8>();
            for c in eights {
                let v = load8(c);
                vmin = v.min(vmin);
                vmax = v.max(vmax);
                vsum += v.is_nan().select(zero, v); // NaN pixels do not poison the mean
                let idx = (v * bins).max(zero).min(top).trunc_int().to_array();
                let hist = &mut p.hist;
                hist[0][idx[0] as usize] += 1;
                hist[1][idx[1] as usize] += 1;
                hist[2][idx[2] as usize] += 1;
                hist[3][idx[3] as usize] += 1;
                hist[0][idx[4] as usize] += 1;
                hist[1][idx[5] as usize] += 1;
                hist[2][idx[6] as usize] += 1;
                hist[3][idx[7] as usize] += 1;
                let l = (v * luma_w).to_array();
                hist[4][bin_of(l[0] + l[1] + l[2])] += 1;
                hist[4][bin_of(l[4] + l[5] + l[6])] += 1;
            }
            let mut tail_sum = [0.0f64; 8];
            let (tail_pixels, _) = tail.as_chunks::<4>();
            for px in tail_pixels {
                for c in 0..4 {
                    p.hist[c][bin_of(px[c])] += 1;
                    if !px[c].is_nan() {
                        tail_sum[c] += px[c] as f64;
                    }
                }
                let l = 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2];
                p.hist[4][bin_of(l)] += 1;
                let mut mn = vmin.to_array();
                let mut mx = vmax.to_array();
                for c in 0..4 {
                    if px[c] < mn[c] {
                        mn[c] = px[c];
                    }
                    if px[c] > mx[c] {
                        mx[c] = px[c];
                    }
                }
                vmin = f32x8::new(mn);
                vmax = f32x8::new(mx);
            }
            p.min = vmin;
            p.max = vmax;
            for ((s, v), t) in p.sum.iter_mut().zip(vsum.to_array()).zip(tail_sum) {
                *s += v as f64 + t;
            }
            p
        })
        .reduce(Partial::new, Partial::merge)
}

/// The statistics of `pixels` pixels from what [`partial_stats`] found of them.
fn finish_stats(partial: Partial, pixels: usize, width: u32, height: u32) -> ImageStats {
    let mn = partial.min.to_array();
    let mx = partial.max.to_array();
    let mut stats = ImageStats { width, height, hist: partial.hist, min: [0.0; 4], max: [0.0; 4], mean: [0.0; 4] };
    let count = pixels.max(1) as f64;
    for c in 0..4 {
        stats.min[c] = mn[c].min(mn[c + 4]);
        stats.max[c] = mx[c].max(mx[c + 4]);
        if pixels == 0 {
            stats.min[c] = 0.0;
            stats.max[c] = 0.0;
        }
        stats.mean[c] = ((partial.sum[c] + partial.sum[c + 4]) / count) as f32;
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pictures of 8 and 16 bits become half floats, looked up, exactly as they did through
    /// floats: every value, in every layout of channels.
    #[test]
    fn channels_become_the_same_half_floats_as_through_floats() {
        let through = |floats: Vec<f32>| {
            let mut half = vec![0u16; floats.len()];
            f32_to_f16(&floats, &mut half);
            half
        };
        let bytes: Vec<u8> = (0..=255u8).cycle().take(4 * 1000).collect();
        let mut floats = vec![0.0; bytes.len()];
        u8_to_f32(&bytes, &mut floats);
        let mut rgba = vec![0u16; bytes.len()];
        u8_to_rgba_f16(&bytes, 4, &mut rgba);
        assert_eq!(rgba, through(floats.clone()));
        let mut gray = vec![0u16; bytes.len()];
        u8_to_f16(&bytes, &mut gray);
        assert_eq!(gray, through(floats));
        let words: Vec<u16> = (0..=u16::MAX).collect();
        let mut floats = vec![0.0; words.len()];
        u16_to_f32(&words, &mut floats);
        let mut rgba = vec![0u16; words.len()];
        u16_to_rgba_f16(&words, 4, &mut rgba);
        assert_eq!(rgba, through(floats));
        // Gray, gray and alpha, RGB: spread, and opaque without alpha.
        let one = f16::ONE.to_bits();
        let h = |v: u8| halves_of_u8()[usize::from(v)];
        for (channels, pixel, want) in [
            (1, vec![51u8], [h(51), h(51), h(51), one]),
            (2, vec![51, 102], [h(51), h(51), h(51), h(102)]),
            (3, vec![51, 102, 153], [h(51), h(102), h(153), one]),
        ] {
            let src: Vec<u8> = pixel.iter().copied().cycle().take(channels * 70_000).collect();
            let mut dst = vec![0u16; 4 * 70_000];
            u8_to_rgba_f16(&src, channels, &mut dst);
            assert!(dst.as_chunks::<4>().0.iter().all(|p| *p == want), "{channels} channels");
        }
    }

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = brewed_texture_core::expr::builtins::pcg_hash(s);
                (s as f32 / u32::MAX as f32) * 1.4 - 0.2 // covers values outside [0, 1]
            })
            .collect()
    }

    #[test]
    fn stats_of_a_stored_image_are_its_stats() {
        // More than a band of pixels.
        let (w, h) = (1500u32, 1000u32);
        let rgba = noise((w * h * 4) as usize, 3);
        let whole = image_stats(&rgba, w, h);
        let banded = stored_stats(&StoredImage::from_f32(w, h, &rgba));
        assert_eq!(whole.hist, banded.hist);
        assert_eq!((whole.min, whole.max), (banded.min, banded.max));
        for c in 0..4 {
            assert!((whole.mean[c] - banded.mean[c]).abs() < 1e-5, "{:?} {:?}", whole.mean, banded.mean);
        }
    }

    #[test]
    fn quantize_matches_scalar() {
        let src = noise(1_000_003, 1);
        let mut simd8 = vec![0u8; src.len()];
        let mut simd16 = vec![0u16; src.len()];
        quantize_u8(&src, &mut simd8);
        quantize_u16(&src, &mut simd16);
        for (i, v) in src.iter().enumerate() {
            assert_eq!(simd8[i], (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8, "u8 at {i}");
            let r16 = (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
            assert!(simd16[i].abs_diff(r16) <= 1, "u16 at {i}"); // fma rounding may differ by 1
        }
        let mut nan = vec![0u8; 9];
        quantize_u8(&[f32::NAN; 9], &mut nan);
        assert!(nan.iter().all(|&v| v == 0));
    }

    #[test]
    fn int_to_float_roundtrip() {
        let bytes: Vec<u8> = (0..100_003u32).map(|i| (i * 7 % 256) as u8).collect();
        let mut f = vec![0.0; bytes.len()];
        u8_to_f32(&bytes, &mut f);
        let mut back = vec![0u8; bytes.len()];
        quantize_u8(&f, &mut back);
        assert_eq!(bytes, back);

        let words: Vec<u16> = (0..70_001u32).map(|i| (i * 13 % 65536) as u16).collect();
        let mut f = vec![0.0; words.len()];
        u16_to_f32(&words, &mut f);
        let mut back = vec![0u16; words.len()];
        quantize_u16(&f, &mut back);
        assert_eq!(words, back);
    }

    #[test]
    fn half_roundtrip() {
        let src = noise(300_001, 7);
        let mut h = vec![0u16; src.len()];
        f32_to_f16(&src, &mut h);
        let mut back = vec![0.0f32; src.len()];
        f16_to_f32(&h, &mut back);
        for (a, b) in src.iter().zip(&back) {
            assert!((a - b).abs() <= 1e-3 * a.abs().max(1e-2));
        }
    }

    #[test]
    fn channel_selection() {
        let rgba: Vec<u8> = (0..40u8).collect();
        assert_eq!(select_channels(&rgba, &[0]), vec![0, 4, 8, 12, 16, 20, 24, 28, 32, 36]);
        assert_eq!(select_channels(&rgba, &[1, 3])[..4], [1, 3, 5, 7]);
        assert_eq!(select_channels(&rgba, &[0, 1, 2, 3]), rgba);
    }

    #[test]
    fn stats_match_scalar() {
        let (w, h) = (613u32, 419u32); // odd sizes exercise the scalar tails
        let src = noise((w * h * 4) as usize, 3);
        let stats = image_stats(&src, w, h);
        let mut hist = [[0u32; 256]; 5];
        let mut mn = [f32::INFINITY; 4];
        let mut mx = [f32::NEG_INFINITY; 4];
        let mut sum = [0.0f64; 4];
        let (pixels, _) = src.as_chunks::<4>();
        for px in pixels {
            for c in 0..4 {
                hist[c][bin_of(px[c])] += 1;
                mn[c] = mn[c].min(px[c]);
                mx[c] = mx[c].max(px[c]);
                sum[c] += px[c] as f64;
            }
            hist[4][bin_of(0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2])] += 1;
        }
        for c in 0..4 {
            assert_eq!(stats.hist[c], hist[c], "channel {c}");
            assert_eq!(stats.min[c], mn[c]);
            assert_eq!(stats.max[c], mx[c]);
            let mean = (sum[c] / (w * h) as f64) as f32;
            assert!((stats.mean[c] - mean).abs() < 1e-3);
        }
        // Luminance is computed with SIMD multiplies, allow bins to differ on exact edges.
        let total: u32 = stats.hist[4].iter().sum();
        assert_eq!(total, w * h);
        let diff: u32 = stats.hist[4].iter().zip(&hist[4]).map(|(a, b)| a.abs_diff(*b)).sum();
        assert!(diff < 10);
    }
}
