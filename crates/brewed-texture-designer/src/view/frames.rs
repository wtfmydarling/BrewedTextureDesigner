//! Frames drawn over the last second: the 3D view counts its own (see `view::view3d`), and
//! says a few times a second how many it drew in the last one.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How far back frames are counted.
pub const WINDOW: Duration = Duration::from_secs(1);
/// How often the rate is said: often enough to follow, seldom enough to read.
const EVERY: Duration = Duration::from_millis(250);

/// Frames drawn over the last second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameRate {
    pub fps: f32,
    /// How long a frame took on average, from one to the next.
    pub frame_ms: f32,
    /// The longest a frame took: a hitch shows here even when the average is fine.
    pub worst_ms: f32,
}

impl FrameRate {
    /// The rate of frames drawn at `frames` (oldest first).
    pub fn of(frames: &VecDeque<Instant>) -> FrameRate {
        let (Some(first), Some(last)) = (frames.front(), frames.back()) else { return FrameRate::default() };
        let span = last.duration_since(*first).as_secs_f32();
        if frames.len() < 2 || span <= 0.0 {
            return FrameRate::default();
        }
        let intervals = frames.len() - 1;
        let worst = frames.iter().zip(frames.iter().skip(1)).map(|(a, b)| b.duration_since(*a).as_secs_f32()).fold(0.0, f32::max);
        FrameRate { fps: intervals as f32 / span, frame_ms: span * 1000.0 / intervals as f32, worst_ms: worst * 1000.0 }
    }
}

/// The frames something drew over the last second.
#[derive(Debug, Default)]
pub struct FrameCounter {
    frames: VecDeque<Instant>,
    said: Option<Instant>,
}

impl FrameCounter {
    /// Count a frame drawn at `now`: the rate over the last second, when it is time to say it.
    pub fn frame(&mut self, now: Instant) -> Option<FrameRate> {
        self.frames.push_back(now);
        while self.frames.front().is_some_and(|f| now.duration_since(*f) > WINDOW) {
            self.frames.pop_front();
        }
        if self.said.is_some_and(|at| now.duration_since(at) < EVERY) {
            return None;
        }
        self.said = Some(now);
        Some(FrameRate::of(&self.frames))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_is_what_the_frames_say() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        // Sixty frames a second, one of them late.
        let mut frames: VecDeque<Instant> = (0..=60u64).map(|i| at(i * 1000 / 60)).collect();
        let rate = FrameRate::of(&frames);
        assert!((rate.fps - 60.0).abs() < 0.5, "{rate:?}");
        assert!((rate.frame_ms - 16.67).abs() < 0.1, "{rate:?}");
        frames.push_back(at(1100));
        assert!(FrameRate::of(&frames).worst_ms > 99.0);
        // One frame is no rate at all, and neither is none.
        assert_eq!(FrameRate::of(&VecDeque::from([start])), FrameRate::default());
        assert_eq!(FrameRate::of(&VecDeque::new()), FrameRate::default());
    }

    #[test]
    fn the_rate_is_said_four_times_a_second() {
        let start = Instant::now();
        let mut counter = FrameCounter::default();
        let said = (0..120u64).filter(|i| counter.frame(start + Duration::from_millis(i * 1000 / 120)).is_some()).count();
        assert_eq!(said, 4, "at 0, 250, 500 and 750 ms of the first second");
    }
}
