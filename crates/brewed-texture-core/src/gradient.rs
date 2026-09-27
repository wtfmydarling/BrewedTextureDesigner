//! Color gradients with any number of stops (Gradient Map, gradient picking).

use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// Position in [0, 1].
    pub pos: f32,
    /// Straight (not premultiplied) RGBA.
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interpolation {
    #[default]
    Linear,
    /// Hermite easing between neighbouring stops.
    Smooth,
    /// Hard steps: every stop holds its color until the next one.
    Constant,
}

impl Interpolation {
    pub const ALL: [Interpolation; 3] = [Interpolation::Linear, Interpolation::Smooth, Interpolation::Constant];

    pub fn label(self) -> &'static str {
        match self {
            Interpolation::Linear => "Linear",
            Interpolation::Smooth => "Smooth",
            Interpolation::Constant => "Constant",
        }
    }
}

impl std::fmt::Display for Interpolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A color ramp. Stops are kept sorted by position; [`Gradient::normalized`] repairs
/// gradients that come from files or user edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub stops: Vec<GradientStop>,
    #[serde(default)]
    pub interpolation: Interpolation,
}

impl Default for Gradient {
    fn default() -> Self {
        Gradient::from_pairs(&[(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [1.0, 1.0, 1.0, 1.0])])
    }
}

fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

impl Gradient {
    /// More stops than this are merged when a gradient is picked from an image.
    pub const MAX_STOPS: usize = 64;

    /// The most stops a gradient keeps: more than the table it is drawn through has entries
    /// (1024) make no difference. Of more (a file can say anything), as many are kept, evenly
    /// across them, the first and the last among them.
    pub const MAX_KEPT: usize = 1024;

    pub fn from_pairs(pairs: &[(f32, [f32; 4])]) -> Gradient {
        Gradient {
            stops: pairs.iter().map(|&(pos, color)| GradientStop { pos, color }).collect(),
            interpolation: Interpolation::Linear,
        }
        .normalized()
    }

    /// Positions clamped to [0, 1], non-finite values replaced, stops sorted (stable, so
    /// stops at the same position keep their order), at least one stop.
    pub fn normalized(&self) -> Gradient {
        let mut stops: Vec<GradientStop> = self
            .stops
            .iter()
            .map(|s| GradientStop { pos: finite(s.pos, 0.0).clamp(0.0, 1.0), color: s.color.map(|c| finite(c, 0.0)) })
            .collect();
        stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        if stops.is_empty() {
            return Gradient { interpolation: self.interpolation, ..Gradient::default() };
        }
        if stops.len() > Self::MAX_KEPT {
            let n = stops.len();
            stops = (0..Self::MAX_KEPT).map(|i| stops[i * (n - 1) / (Self::MAX_KEPT - 1)]).collect();
        }
        Gradient { stops, interpolation: self.interpolation }
    }

    /// Color at `t` (clamped to [0, 1]). Expects sorted stops.
    pub fn sample(&self, t: f32) -> [f32; 4] {
        let t = finite(t, 0.0).clamp(0.0, 1.0);
        let stops = &self.stops;
        let Some(first) = stops.first() else { return [0.0, 0.0, 0.0, 1.0] };
        if t <= first.pos {
            return first.color;
        }
        let last = stops[stops.len() - 1];
        if t >= last.pos {
            return last.color;
        }
        // First stop strictly after t.
        let i = stops.partition_point(|s| s.pos <= t).clamp(1, stops.len() - 1);
        let (a, b) = (stops[i - 1], stops[i]);
        let span = b.pos - a.pos;
        let mut f = if span > 1e-9 { (t - a.pos) / span } else { 1.0 };
        match self.interpolation {
            Interpolation::Linear => {}
            Interpolation::Smooth => f = f * f * (3.0 - 2.0 * f),
            Interpolation::Constant => f = 0.0,
        }
        std::array::from_fn(|c| a.color[c] + (b.color[c] - a.color[c]) * f)
    }

    /// `n` evenly spaced samples from t = 0 to t = 1 (both included).
    pub fn bake(&self, n: usize) -> Vec<[f32; 4]> {
        let n = n.max(2);
        (0..n).map(|i| self.sample(i as f32 / (n - 1) as f32)).collect()
    }

    pub fn hash_into<H: Hasher>(&self, h: &mut H) {
        self.interpolation.hash(h);
        for s in &self.stops {
            s.pos.to_bits().hash(h);
            s.color.map(f32::to_bits).hash(h);
        }
    }

    /// Insert a stop at `pos` with the color the gradient currently has there.
    /// Returns the index of the new stop.
    pub fn insert_at(&mut self, pos: f32) -> usize {
        let pos = finite(pos, 0.5).clamp(0.0, 1.0);
        let color = self.sample(pos);
        let i = self.stops.partition_point(|s| s.pos <= pos);
        self.stops.insert(i, GradientStop { pos, color });
        i
    }

    /// Mirror the gradient (t becomes 1 - t).
    pub fn reversed(&self) -> Gradient {
        let mut stops: Vec<GradientStop> =
            self.stops.iter().rev().map(|s| GradientStop { pos: 1.0 - s.pos, color: s.color }).collect();
        stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        Gradient { stops, interpolation: self.interpolation }
    }

    /// Spread the stops evenly over [0, 1], keeping their order.
    pub fn distributed(&self) -> Gradient {
        let n = self.stops.len();
        let stops = self
            .stops
            .iter()
            .enumerate()
            .map(|(i, s)| GradientStop { pos: if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 }, color: s.color })
            .collect();
        Gradient { stops, interpolation: self.interpolation }
    }

    /// Build a gradient that reproduces evenly spaced color samples (e.g. picked along a
    /// line in an image) with as few stops as possible: the polyline through the samples
    /// is simplified (Ramer–Douglas–Peucker in RGBA space) until at most `max_stops`
    /// stops remain and every sample is within `tolerance` of the result.
    pub fn from_samples(samples: &[[f32; 4]], tolerance: f32, max_stops: usize) -> Gradient {
        let samples: Vec<[f32; 4]> = samples.iter().map(|c| c.map(|v| finite(v, 0.0))).collect();
        match samples.len() {
            0 => return Gradient::default(),
            1 => return Gradient::from_pairs(&[(0.0, samples[0]), (1.0, samples[0])]),
            _ => {}
        }
        let n = samples.len();
        let pos = |i: usize| i as f32 / (n - 1) as f32;
        let max_stops = max_stops.max(2);
        let mut tol = tolerance.max(1e-4);
        loop {
            let mut keep = vec![false; n];
            keep[0] = true;
            keep[n - 1] = true;
            rdp(&samples, 0, n - 1, tol, &mut keep);
            let count = keep.iter().filter(|k| **k).count();
            if count <= max_stops {
                let stops = (0..n).filter(|i| keep[*i]).map(|i| GradientStop { pos: pos(i), color: samples[i] });
                return Gradient { stops: stops.collect(), interpolation: Interpolation::Linear };
            }
            tol *= 1.5;
        }
    }
}

/// Largest deviation of the samples between `a` and `b` from the straight segment `a`–`b`.
fn rdp(samples: &[[f32; 4]], a: usize, b: usize, tolerance: f32, keep: &mut [bool]) {
    if b <= a + 1 {
        return;
    }
    let (ca, cb) = (samples[a], samples[b]);
    let mut worst = (0.0f32, a);
    for (i, s) in samples.iter().enumerate().take(b).skip(a + 1) {
        let f = (i - a) as f32 / (b - a) as f32;
        let d = (0..4).map(|c| (s[c] - (ca[c] + (cb[c] - ca[c]) * f)).abs()).fold(0.0f32, f32::max);
        if d > worst.0 {
            worst = (d, i);
        }
    }
    if worst.0 > tolerance {
        keep[worst.1] = true;
        rdp(samples, a, worst.1, tolerance, keep);
        rdp(samples, worst.1, b, tolerance, keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn sampling() {
        let g = Gradient::from_pairs(&[(0.25, [0.0, 0.0, 0.0, 1.0]), (0.75, [1.0, 0.5, 0.0, 1.0])]);
        assert!(close(g.sample(0.0), [0.0, 0.0, 0.0, 1.0]));
        assert!(close(g.sample(0.5), [0.5, 0.25, 0.0, 1.0]));
        assert!(close(g.sample(1.0), [1.0, 0.5, 0.0, 1.0]));
        let steps = Gradient { interpolation: Interpolation::Constant, ..g.clone() };
        assert!(close(steps.sample(0.7), [0.0, 0.0, 0.0, 1.0]));
        assert!(close(steps.sample(0.8), [1.0, 0.5, 0.0, 1.0]));
        let lut = g.bake(5);
        assert_eq!(lut.len(), 5);
        assert!(close(lut[2], g.sample(0.5)));
    }

    /// Of more stops than a gradient keeps (a file can say a million), as many are kept, evenly
    /// across, the first and the last among them; fewer are all kept.
    #[test]
    fn a_gradient_keeps_as_many_stops_as_it_can_draw() {
        let pairs: Vec<(f32, [f32; 4])> = (0..100_000).map(|i| (i as f32 / 99_999.0, [i as f32, 0.0, 0.0, 1.0])).collect();
        let g = Gradient::from_pairs(&pairs);
        assert_eq!(g.stops.len(), Gradient::MAX_KEPT);
        assert_eq!((g.stops[0].color[0], g.stops[Gradient::MAX_KEPT - 1].color[0]), (0.0, 99_999.0));
        assert!(g.stops.windows(2).all(|w| w[0].pos <= w[1].pos));
        assert_eq!(g.normalized(), g, "kept as it is read again");
        assert_eq!(Gradient::from_pairs(&pairs[..500]).stops.len(), 500);
    }

    #[test]
    fn normalizing_repairs_input() {
        let g = Gradient {
            stops: vec![
                GradientStop { pos: 2.0, color: [1.0; 4] },
                GradientStop { pos: f32::NAN, color: [f32::INFINITY, 0.0, 0.0, 1.0] },
            ],
            interpolation: Interpolation::Linear,
        }
        .normalized();
        assert_eq!(g.stops[0].pos, 0.0);
        assert_eq!(g.stops[0].color[0], 0.0);
        assert_eq!(g.stops[1].pos, 1.0);
        assert_eq!(Gradient { stops: vec![], interpolation: Interpolation::Smooth }.normalized().stops.len(), 2);
    }

    #[test]
    fn simplifying_samples() {
        // A ramp black -> red -> white sampled 101 times collapses to three stops.
        let samples: Vec<[f32; 4]> = (0..=100)
            .map(|i| {
                let t = i as f32 / 100.0;
                if t < 0.5 { [t * 2.0, 0.0, 0.0, 1.0] } else { [1.0, (t - 0.5) * 2.0, (t - 0.5) * 2.0, 1.0] }
            })
            .collect();
        let g = Gradient::from_samples(&samples, 0.01, 16);
        assert_eq!(g.stops.len(), 3);
        for (i, s) in samples.iter().enumerate() {
            assert!(close(g.sample(i as f32 / 100.0), *s));
        }
        // Noise is capped at the requested number of stops.
        let noisy: Vec<[f32; 4]> = (0..200).map(|i| [((i * 37) % 11) as f32 / 10.0, 0.0, 0.0, 1.0]).collect();
        assert!(Gradient::from_samples(&noisy, 0.001, 12).stops.len() <= 12);
    }

    #[test]
    fn editing_helpers() {
        let mut g = Gradient::default();
        let i = g.insert_at(0.5);
        assert_eq!(i, 1);
        assert!(close(g.stops[1].color, [0.5, 0.5, 0.5, 1.0]));
        let r = g.reversed();
        assert!(close(r.sample(0.0), [1.0; 4]));
        let d = Gradient::from_pairs(&[(0.0, [0.0; 4]), (0.1, [0.5; 4]), (0.2, [1.0; 4])]).distributed();
        assert_eq!(d.stops[1].pos, 0.5);
        assert_eq!(d.stops[2].pos, 1.0);
    }
}
