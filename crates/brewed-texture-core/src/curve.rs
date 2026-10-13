//! Curves: values mapped to other values through points joined smoothly (tone curves).

use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

/// How close two points of a curve may be across (in x).
pub const MIN_GAP: f32 = 0.005;

/// A curve through points, x ascending in [0, 1] and y in [0, 1]. Between two points it is a
/// monotone cubic (Fritsch–Carlson): smooth, and never past its points, so a curve that only
/// rises never dips and nothing comes out beyond 0 and 1. Before its first point and after its
/// last it is flat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
}

impl Default for Curve {
    /// The straight line: every value as it is.
    fn default() -> Self {
        Curve { points: vec![[0.0, 0.0], [1.0, 1.0]] }
    }
}

impl Curve {
    pub fn from_points(points: &[[f32; 2]]) -> Curve {
        Curve { points: points.to_vec() }.normalized()
    }

    /// The curve as it may be: points finite and within [0, 1], sorted across, at least
    /// [`MIN_GAP`] apart (of two closer, the later stays), at least two of them.
    pub fn normalized(&self) -> Curve {
        let mut points: Vec<[f32; 2]> = self.points.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| p.map(|v| v.clamp(0.0, 1.0))).collect();
        points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut kept: Vec<[f32; 2]> = Vec::with_capacity(points.len());
        for p in points {
            match kept.last_mut() {
                Some(last) if p[0] - last[0] < MIN_GAP => *last = p,
                _ => kept.push(p),
            }
        }
        match kept.len() {
            0 => Curve::default(),
            1 => {
                // One point: a flat line through it.
                let y = kept[0][1];
                Curve { points: vec![[0.0, y], [1.0, y]] }
            }
            _ => Curve { points: kept },
        }
    }

    /// The slopes at the points: the mean of the secants around each (none at a peak or a
    /// valley, where they turn), cut down where they would make the curve overshoot.
    fn tangents(&self) -> Vec<f32> {
        let p = &self.points;
        let n = p.len();
        let secant = |k: usize| (p[k + 1][1] - p[k][1]) / (p[k + 1][0] - p[k][0]).max(1e-6);
        let d: Vec<f32> = (0..n - 1).map(secant).collect();
        let mut m = vec![0.0; n];
        m[0] = d[0];
        m[n - 1] = d[n - 2];
        for k in 1..n - 1 {
            m[k] = if d[k - 1] * d[k] > 0.0 { (d[k - 1] + d[k]) / 2.0 } else { 0.0 };
        }
        for k in 0..n - 1 {
            if d[k] == 0.0 {
                m[k] = 0.0;
                m[k + 1] = 0.0;
                continue;
            }
            let (a, b) = (m[k] / d[k], m[k + 1] / d[k]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m[k] = t * a * d[k];
                m[k + 1] = t * b * d[k];
            }
        }
        m
    }

    /// The value the curve gives `x`.
    pub fn sample(&self, x: f32) -> f32 {
        self.sampler()(x)
    }

    /// A function giving the value the curve gives any x (its slopes worked out once).
    fn sampler(&self) -> impl Fn(f32) -> f32 + '_ {
        let m = self.tangents();
        let p = &self.points;
        move |x: f32| {
            let x = if x.is_finite() { x } else { 0.0 };
            let last = p.len() - 1;
            if x <= p[0][0] {
                return p[0][1];
            }
            if x >= p[last][0] {
                return p[last][1];
            }
            let k = p.partition_point(|q| q[0] <= x).saturating_sub(1).min(last - 1);
            let h = (p[k + 1][0] - p[k][0]).max(1e-6);
            let t = (x - p[k][0]) / h;
            let (t2, t3) = (t * t, t * t * t);
            let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p[k][1] + (t3 - 2.0 * t2 + t) * h * m[k] + (-2.0 * t3 + 3.0 * t2) * p[k + 1][1] + (t3 - t2) * h * m[k + 1];
            y.clamp(0.0, 1.0)
        }
    }

    /// `n` values of the curve, from x = 0 to x = 1 (both in).
    pub fn bake(&self, n: usize) -> Vec<f32> {
        let f = self.sampler();
        let last = n.max(2) - 1;
        (0..=last).map(|i| f(i as f32 / last as f32)).collect()
    }

    /// A point at (`x`, `y`), where there is room for one; its index.
    pub fn insert(&mut self, x: f32, y: f32) -> Option<usize> {
        let x = x.clamp(0.0, 1.0);
        if self.points.iter().any(|p| (p[0] - x).abs() < MIN_GAP) {
            return None;
        }
        let at = self.points.partition_point(|p| p[0] < x);
        self.points.insert(at, [x, y.clamp(0.0, 1.0)]);
        Some(at)
    }

    /// Point `index` moved toward (`x`, `y`): across only as far as its neighbours leave room.
    pub fn moved(&self, index: usize, x: f32, y: f32) -> Curve {
        let mut c = self.clone();
        let lo = if index == 0 { 0.0 } else { c.points[index - 1][0] + MIN_GAP };
        let hi = c.points.get(index + 1).map_or(1.0, |p| p[0] - MIN_GAP);
        if let Some(p) = c.points.get_mut(index) {
            *p = [x.clamp(lo, hi.max(lo)), y.clamp(0.0, 1.0)];
        }
        c
    }

    /// The curve upside down: dark for light.
    pub fn inverted(&self) -> Curve {
        Curve { points: self.points.iter().map(|p| [p[0], 1.0 - p[1]]).collect() }
    }

    pub fn hash_into<H: Hasher>(&self, h: &mut H) {
        self.points.len().hash(h);
        for p in &self.points {
            p.map(f32::to_bits).hash(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_straight_line_and_its_points() {
        let line = Curve::default();
        for x in [0.0, 0.25, 0.5, 0.9, 1.0] {
            assert!((line.sample(x) - x).abs() < 1e-6, "{x}");
        }
        let c = Curve::from_points(&[[0.0, 0.1], [0.4, 0.7], [1.0, 0.9]]);
        for p in &c.points {
            assert!((c.sample(p[0]) - p[1]).abs() < 1e-5, "{p:?}");
        }
        // Flat beyond its ends.
        let short = Curve::from_points(&[[0.2, 0.3], [0.8, 0.6]]);
        assert_eq!((short.sample(0.0), short.sample(1.0), short.sample(f32::NAN)), (0.3, 0.6, 0.3));
    }

    #[test]
    fn a_steep_s_never_overshoots_and_a_bump_stays_within_its_points() {
        let s = Curve::from_points(&[[0.0, 0.0], [0.3, 0.02], [0.7, 0.98], [1.0, 1.0]]);
        let values = s.bake(1001);
        assert!(values.windows(2).all(|w| w[1] >= w[0] - 1e-6), "rising, never dipping");
        assert!(values.iter().all(|v| (0.0..=1.0).contains(v)));
        // A peak: highest at its point, never past it.
        let bump = Curve::from_points(&[[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]]);
        let values = bump.bake(1001);
        let top = values.iter().copied().fold(0.0, f32::max);
        assert!((top - 1.0).abs() < 1e-6 && (values[500] - 1.0).abs() < 1e-6, "{top}");
        assert_eq!((values[0], values[1000]), (0.0, 0.0));
    }

    #[test]
    fn curves_are_kept_in_shape() {
        let messy = Curve { points: vec![[0.9, 2.0], [f32::NAN, 0.5], [0.1, -1.0], [0.1 + MIN_GAP / 2.0, 0.4]] }.normalized();
        assert_eq!(messy.points, vec![[0.1 + MIN_GAP / 2.0, 0.4], [0.9, 1.0]]);
        assert_eq!(Curve { points: vec![] }.normalized(), Curve::default());
        assert_eq!(Curve { points: vec![[0.5, 0.25]] }.normalized().points, vec![[0.0, 0.25], [1.0, 0.25]]);
        // Moved, a point stays between its neighbours; added, where there is room.
        let mut c = Curve::default();
        assert_eq!(c.insert(0.5, 0.8), Some(1));
        assert_eq!(c.insert(0.5 + MIN_GAP / 2.0, 0.1), None);
        let moved = c.moved(1, 1.5, -3.0);
        assert_eq!(moved.points[1], [1.0 - MIN_GAP, 0.0]);
        assert_eq!(c.moved(0, 0.3, 0.2).points[0], [0.3, 0.2]);
        assert_eq!(c.inverted().points, vec![[0.0, 1.0], [0.5, 1.0 - 0.8], [1.0, 0.0]]);
    }
}
