//! Even frame pacing ("Smooth motion"): frames over Wi-Fi arrive in clumps, and the TV's
//! decoder runs in low-delay mode and shows each frame as soon as it is fed, so uneven
//! arrival becomes uneven motion. The pacer releases frames at the rhythm the computer
//! captured them in (the encoder's `pts`, milliseconds), behind the earliest-arriving frame
//! of the last two seconds by the measured arrival jitter (90th percentile, capped), so a
//! steady network adds about nothing and a jittery one about one or two frames.

use std::collections::VecDeque;

const WINDOW_MS: i64 = 2_000;
const MAX_DELAY_MS: f64 = 80.0;
/// Frames are never held longer than this past their arrival.
const MAX_HOLD_MS: f64 = 120.0;
/// A pts jump larger than this (or backwards) starts over (new stream, encoder restart).
const GAP_MS: i64 = 1_000;

#[derive(Default)]
pub struct Pacer {
    /// (arrival ms, pts ms) of recent frames.
    hist: VecDeque<(f64, i64)>,
    last_pts: Option<i64>,
    last_release: f64,
    /// Current playout delay (rises at once, falls slowly so the rhythm stays even).
    delay: f64,
}

impl Pacer {
    pub fn reset(&mut self) {
        *self = Pacer::default();
    }

    pub fn delay_ms(&self) -> f64 {
        self.delay
    }

    /// When to release a frame with `pts` that arrived at `arrival` (both ms, any epoch).
    pub fn schedule(&mut self, arrival: f64, pts: i64) -> f64 {
        if self.last_pts.is_none_or(|p| pts < p || pts - p > GAP_MS) {
            self.hist.clear();
            self.delay = 0.0;
            self.last_release = f64::MIN;
        }
        self.last_pts = Some(pts);
        self.hist.push_back((arrival, pts));
        while self.hist.front().is_some_and(|&(_, p)| pts - p > WINDOW_MS) || self.hist.len() > 512 {
            self.hist.pop_front();
        }
        let offsets: Vec<f64> = self.hist.iter().map(|&(a, p)| a - p as f64).collect();
        let anchor = offsets.iter().copied().fold(f64::INFINITY, f64::min);
        let mut late: Vec<f64> = offsets.iter().map(|o| o - anchor).collect();
        late.sort_by(|a, b| a.total_cmp(b));
        let p90 = late[(late.len() * 9 / 10).min(late.len() - 1)].min(MAX_DELAY_MS);
        self.delay = if p90 > self.delay {
            p90
        } else {
            (self.delay - 0.2).max(p90)
        };
        let release = (pts as f64 + anchor + self.delay)
            .max(self.last_release)
            .clamp(arrival, arrival + MAX_HOLD_MS);
        self.last_release = release;
        release
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_arrival_is_not_delayed() {
        let mut p = Pacer::default();
        for i in 0..100 {
            let t = 1000.0 + i as f64 * 16.0;
            assert_eq!(p.schedule(t, i * 16), t);
        }
    }

    #[test]
    fn clumped_arrival_is_spread_evenly() {
        let mut p = Pacer::default();
        let mut releases = vec![];
        // Captured every 16 ms, delivered in pairs every 32 ms (the second one 16 ms late).
        for i in 0..200i64 {
            let arrival = 1000.0 + (i / 2 * 32 + 16) as f64;
            releases.push(p.schedule(arrival, i * 16));
        }
        let gaps: Vec<f64> = releases[100..].windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.iter().all(|g| (g - 16.0).abs() < 0.5), "{gaps:?}");
        assert!(p.delay_ms() <= 17.0);
    }

    #[test]
    fn never_early_and_restarts_on_a_gap() {
        let mut p = Pacer::default();
        assert_eq!(p.schedule(500.0, 10_000), 500.0);
        assert!(p.schedule(520.0, 10_016) >= 520.0);
        // pts jumps back (new stream): starts over without holding.
        assert_eq!(p.schedule(600.0, 0), 600.0);
    }
}
