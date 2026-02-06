//! Fixed-timestep game clock using an accumulator pattern.
//!
//! `draw_web()` calls at ~60fps with variable delta. GameTime converts
//! this into a fixed number of discrete ticks per second, making game
//! logic deterministic and fully testable.

pub struct GameTime {
    /// Milliseconds per tick (e.g. 100ms = 10 ticks/sec)
    ms_per_tick: f64,
    /// Accumulated milliseconds not yet consumed as ticks
    accumulator: f64,
    /// Total elapsed ticks since creation
    pub total_ticks: u64,
    /// Timestamp of the last update (ms), None if first frame
    last_timestamp: Option<f64>,
}

impl GameTime {
    /// Create a new GameTime with the given tick rate.
    /// `ticks_per_sec`: how many game ticks per real-time second (e.g. 10).
    pub fn new(ticks_per_sec: u32) -> Self {
        Self {
            ms_per_tick: 1000.0 / ticks_per_sec as f64,
            accumulator: 0.0,
            total_ticks: 0,
            last_timestamp: None,
        }
    }

    /// Feed wall-clock timestamp (from `performance.now()` or similar).
    /// Returns the number of discrete ticks to process this frame.
    ///
    /// Call this once per draw frame. The returned tick count should be
    /// passed to `Game::tick(delta_ticks)`.
    pub fn update(&mut self, now_ms: f64) -> u32 {
        let delta = match self.last_timestamp {
            Some(prev) => {
                let d = now_ms - prev;
                // Clamp to avoid spiral-of-death if tab was backgrounded
                d.clamp(0.0, 500.0)
            }
            None => 0.0, // First frame: no delta
        };
        self.last_timestamp = Some(now_ms);

        self.accumulator += delta;
        let ticks = (self.accumulator / self.ms_per_tick) as u32;
        self.accumulator -= ticks as f64 * self.ms_per_tick;
        self.total_ticks += ticks as u64;
        ticks
    }

    /// Directly add ticks (useful for testing without timestamps).
    #[cfg(test)]
    pub fn add_ticks(&mut self, ticks: u32) {
        self.total_ticks += ticks as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_frame_returns_zero_ticks() {
        let mut gt = GameTime::new(10);
        assert_eq!(gt.update(0.0), 0);
    }

    #[test]
    fn one_tick_at_100ms() {
        let mut gt = GameTime::new(10); // 100ms per tick
        gt.update(0.0); // first frame
        assert_eq!(gt.update(100.0), 1);
        assert_eq!(gt.total_ticks, 1);
    }

    #[test]
    fn multiple_ticks_accumulated() {
        let mut gt = GameTime::new(10);
        gt.update(0.0);
        assert_eq!(gt.update(350.0), 3); // 350ms = 3 ticks + 50ms remainder
        assert_eq!(gt.total_ticks, 3);
    }

    #[test]
    fn remainder_carried_over() {
        let mut gt = GameTime::new(10);
        gt.update(0.0);
        gt.update(150.0); // 1 tick, 50ms remainder
        assert_eq!(gt.total_ticks, 1);
        assert_eq!(gt.update(200.0), 1); // 50ms + 50ms delta? No: 200-150=50ms + 50ms acc = 100ms = 1 tick
        assert_eq!(gt.total_ticks, 2);
    }

    #[test]
    fn clamp_large_delta() {
        let mut gt = GameTime::new(10);
        gt.update(0.0);
        // Simulate 10 second gap (tab backgrounded) → clamped to 500ms = 5 ticks
        let ticks = gt.update(10000.0);
        assert_eq!(ticks, 5);
    }

    #[test]
    fn sub_tick_frames_accumulate() {
        let mut gt = GameTime::new(10); // 100ms/tick
        gt.update(0.0);
        assert_eq!(gt.update(16.0), 0); // 16ms < 100ms
        assert_eq!(gt.update(32.0), 0); // +16ms = 32ms total
        assert_eq!(gt.update(48.0), 0); // +16ms = 48ms
        assert_eq!(gt.update(64.0), 0); // +16ms = 64ms
        assert_eq!(gt.update(80.0), 0); // +16ms = 80ms
        assert_eq!(gt.update(96.0), 0); // +16ms = 96ms
        assert_eq!(gt.update(112.0), 1); // +16ms = 112ms → 1 tick, 12ms remainder
        assert_eq!(gt.total_ticks, 1);
    }

    #[test]
    fn steady_60fps() {
        let mut gt = GameTime::new(10);
        gt.update(0.0);
        let mut total = 0u32;
        // 60 frames at ~16.67ms each = 1 second
        for i in 1..=60 {
            total += gt.update(i as f64 * 16.667);
        }
        // Should be approximately 10 ticks (1 second at 10 ticks/sec)
        assert!(total >= 9 && total <= 11, "expected ~10 ticks, got {}", total);
    }

    #[test]
    fn add_ticks_directly() {
        let mut gt = GameTime::new(10);
        gt.add_ticks(5);
        assert_eq!(gt.total_ticks, 5);
        gt.add_ticks(3);
        assert_eq!(gt.total_ticks, 8);
    }
}
