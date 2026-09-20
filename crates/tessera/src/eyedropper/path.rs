//! The path dragged across the screen to pick a gradient: the pixels it crosses, one per
//! pixel of its length, and the gradient their colors make.

use tessera_core::Gradient;

/// A pixel of the screen (desktop coordinates, physical pixels).
pub type Pixel = (i32, i32);

/// Longest path kept, in samples; longer ones are resampled to this many.
const MAX_SAMPLES: usize = 4096;
/// Colors kept while a path is dragged: about a kilometre of screen, then it stops
/// growing (the gradient is made of far fewer anyway).
pub const MAX_COLORS: usize = 1 << 20;
/// How far (0..1 per channel) the gradient may stray from the colors along the path.
const TOLERANCE: f32 = 0.012;
/// Samples averaged along the path: evens out dithering and noise without blurring edges
/// much.
const SMOOTHING: usize = 5;

/// The pixels along a freehand path, in order, one per pixel of its length.
#[derive(Debug, Default)]
pub struct PathSampler {
    last: Option<(f32, f32)>,
    last_pixel: Option<Pixel>,
}

impl PathSampler {
    /// The pixels to read to extend the path to `to`: the start of the path, or every
    /// pixel of the new segment after its first.
    pub fn advance(&mut self, to: (f32, f32)) -> Vec<Pixel> {
        if !(to.0.is_finite() && to.1.is_finite()) {
            return Vec::new();
        }
        let Some(from) = self.last else {
            self.last = Some(to);
            self.last_pixel = Some(pixel(to));
            return vec![pixel(to)];
        };
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let steps = dx.abs().max(dy.abs()).ceil().min(1e6) as usize;
        if steps == 0 {
            return Vec::new();
        }
        self.last = Some(to);
        let mut out = Vec::with_capacity(steps);
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let p = pixel((from.0 + dx * t, from.1 + dy * t));
            // Along a straight segment a pixel can only repeat right after itself.
            if self.last_pixel != Some(p) {
                out.push(p);
                self.last_pixel = Some(p);
            }
        }
        out
    }

    /// The pixel the path has reached.
    pub fn last_pixel(&self) -> Option<Pixel> {
        self.last_pixel
    }

}

fn pixel(p: (f32, f32)) -> Pixel {
    (p.0.floor() as i32, p.1.floor() as i32)
}

/// Longest jump of the mouse handled in one go: a fast mouse can cross the screen between
/// two of its events, and the eyedropper would read all of it at once.
pub const MAX_JUMP: f32 = 128.0;

/// Split the way from `from` to `to` into steps of at most [`MAX_JUMP`], so that no step
/// reads too much of the screen. The last one is `to` itself.
pub fn steps(from: (f32, f32), to: (f32, f32)) -> Vec<(f32, f32)> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    if !(dx.is_finite() && dy.is_finite()) {
        return Vec::new();
    }
    let n = ((dx.abs().max(dy.abs()) / MAX_JUMP).ceil() as usize).clamp(1, 4096);
    (1..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            (from.0 + dx * t, from.1 + dy * t)
        })
        .collect()
}

/// A path over several monitors, each with pixels of its own (their scales differ): the
/// pixels it crosses, monitor after monitor, in order. Positions are desktop
/// coordinates; `locate` gives the monitor under one, and where it is in that monitor's
/// pixels.
#[derive(Debug, Default)]
pub struct Trace {
    current: Option<(usize, PathSampler)>,
    last: Option<[f32; 2]>,
}

impl Trace {
    /// The pixels to read to extend the path to `to`, as (monitor, pixel). `step` is small
    /// enough (in desktop units) to find where the path goes from a monitor to another.
    pub fn advance(&mut self, to: [f32; 2], step: f32, locate: impl Fn([f32; 2]) -> Option<(usize, (f32, f32))>) -> Vec<(usize, Pixel)> {
        let mut out = Vec::new();
        if !(to[0].is_finite() && to[1].is_finite()) {
            return out;
        }
        let monitor = |p: [f32; 2]| locate(p).map(|(m, _)| m);
        if let Some(from) = self.last
            && monitor(from) != monitor(to)
        {
            // Onto another monitor: end the path on the first at its edge, go on on the
            // other from its edge.
            let n = ((to[0] - from[0]).hypot(to[1] - from[1]) / step.max(1e-3)).ceil().clamp(1.0, 1e6) as usize;
            let mut previous = from;
            for i in 1..n {
                let t = i as f32 / n as f32;
                let p = [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t];
                if monitor(p) != monitor(previous) {
                    self.visit(previous, &locate, &mut out);
                    self.visit(p, &locate, &mut out);
                }
                previous = p;
            }
        }
        self.visit(to, &locate, &mut out);
        self.last = Some(to);
        out
    }

    fn visit(&mut self, p: [f32; 2], locate: &impl Fn([f32; 2]) -> Option<(usize, (f32, f32))>, out: &mut Vec<(usize, Pixel)>) {
        let Some((monitor, at)) = locate(p) else { return };
        if self.current.as_ref().is_none_or(|(m, _)| *m != monitor) {
            self.current = Some((monitor, PathSampler::default()));
        }
        if let Some((m, path)) = &mut self.current {
            out.extend(path.advance(at).into_iter().map(|px| (*m, px)));
        }
    }

    /// The monitor and pixel the path has reached.
    #[cfg_attr(not(free_unix), allow(dead_code, reason = "the Wayland pick follows the path from monitor to monitor"))]
    pub fn last_pixel(&self) -> Option<(usize, Pixel)> {
        self.current.as_ref().and_then(|(m, path)| Some((*m, path.last_pixel()?)))
    }
}

/// The gradient made by the colors along a path: as few stops as reproduce them closely.
pub fn gradient_from_path(colors: &[[f32; 4]]) -> Gradient {
    let resampled: Vec<[f32; 4]> = if colors.len() > MAX_SAMPLES {
        (0..MAX_SAMPLES).map(|i| colors[i * (colors.len() - 1) / (MAX_SAMPLES - 1)]).collect()
    } else {
        colors.to_vec()
    };
    Gradient::from_samples(&smooth(&resampled, SMOOTHING), TOLERANCE, Gradient::MAX_STOPS)
}

/// Moving average over `window` samples (shorter at the ends).
fn smooth(colors: &[[f32; 4]], window: usize) -> Vec<[f32; 4]> {
    let half = window / 2;
    (0..colors.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(colors.len()));
            let mut sum = [0.0f32; 4];
            for c in &colors[a..b] {
                for k in 0..4 {
                    sum[k] += c[k];
                }
            }
            sum.map(|v| v / (b - a) as f32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_sampled_once_per_pixel() {
        let mut path = PathSampler::default();
        assert_eq!(path.advance((10.5, 10.5)), vec![(10, 10)]);
        // Ten pixels to the right, then five down: every pixel once, none twice.
        let right = path.advance((20.5, 10.5));
        assert_eq!(right.len(), 10);
        assert_eq!(right.first(), Some(&(11, 10)));
        assert_eq!(right.last(), Some(&(20, 10)));
        let down = path.advance((20.5, 15.5));
        assert_eq!(down, (11..=15).map(|y| (20, y)).collect::<Vec<_>>());
        // Moving within the same pixel adds nothing.
        assert!(path.advance((20.6, 15.6)).is_empty());
    }

    #[test]
    fn long_jumps_are_split() {
        // A mouse that crosses the screen between two events.
        let long = steps((0.0, 0.0), (1000.0, 500.0));
        assert_eq!(long.len(), 8);
        assert_eq!(long.last(), Some(&(1000.0, 500.0)));
        assert!(long.windows(2).all(|w| (w[1].0 - w[0].0).abs() <= MAX_JUMP));
        // Short ones are one step, and nonsense is none.
        assert_eq!(steps((10.0, 10.0), (12.0, 11.0)), [(12.0, 11.0)]);
        assert!(steps((0.0, 0.0), (f32::NAN, 0.0)).is_empty());
    }

    #[test]
    fn a_trace_goes_from_monitor_to_monitor() {
        // Two monitors side by side, 100 units wide; the right one has twice the pixels.
        let locate = |p: [f32; 2]| match p[0] {
            x if (0.0..100.0).contains(&x) && (0.0..10.0).contains(&p[1]) => Some((0, (x, p[1]))),
            x if (100.0..200.0).contains(&x) && (0.0..10.0).contains(&p[1]) => Some((1, ((x - 100.0) * 2.0, p[1] * 2.0))),
            _ => None,
        };
        let mut trace = Trace::default();
        let mut pixels = trace.advance([50.5, 5.5], 0.25, locate);
        pixels.extend(trace.advance([150.5, 5.5], 0.25, locate));
        assert_eq!(pixels.iter().filter(|(m, _)| *m == 0).count(), 50);
        assert_eq!(pixels.iter().filter(|(m, _)| *m == 1).count(), 102);
        assert_eq!(pixels.first(), Some(&(0, (50, 5))));
        assert_eq!(trace.last_pixel(), Some((1, (101, 11))));
        // Off every monitor: nothing, and the path goes on from where it comes back.
        assert!(trace.advance([150.5, 50.0], 0.25, locate).len() < 10);
        assert!(trace.advance([f32::NAN, 0.0], 0.25, locate).is_empty());
    }

    #[test]
    fn a_curved_path_gives_the_colors_in_order() {
        // Black → red → white along an L-shaped path.
        let mut colors = Vec::new();
        for i in 0..100 {
            colors.push([i as f32 / 99.0, 0.0, 0.0, 1.0]);
        }
        for i in 0..100 {
            let t = i as f32 / 99.0;
            colors.push([1.0, t, t, 1.0]);
        }
        let g = gradient_from_path(&colors);
        assert!(g.stops.len() <= 5, "{} stops", g.stops.len());
        assert!(g.sample(0.0)[0] < 0.05);
        let mid = g.sample(0.5);
        assert!(mid[0] > 0.9 && mid[1] < 0.1, "{mid:?}");
        assert!(g.sample(1.0)[1] > 0.95);
    }

    #[test]
    fn noise_does_not_make_many_stops() {
        let colors: Vec<[f32; 4]> =
            (0..400).map(|i| if i % 2 == 0 { [0.50, 0.5, 0.5, 1.0] } else { [0.52, 0.5, 0.5, 1.0] }).collect();
        assert!(gradient_from_path(&colors).stops.len() <= 3);
    }

    #[test]
    fn long_paths_are_resampled() {
        let colors: Vec<[f32; 4]> = (0..20_000).map(|i| [i as f32 / 19_999.0, 0.0, 0.0, 1.0]).collect();
        let g = gradient_from_path(&colors);
        assert!(g.sample(1.0)[0] > 0.98 && g.sample(0.0)[0] < 0.02);
    }

    #[test]
    fn one_or_no_color() {
        assert_eq!(gradient_from_path(&[[0.2, 0.4, 0.6, 1.0]]).sample(0.7), [0.2, 0.4, 0.6, 1.0]);
        let _ = gradient_from_path(&[]);
    }
}
