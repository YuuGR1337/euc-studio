use std::time::Duration;

const NANOS_PER_SEC: u128 = 1_000_000_000;

/// The one simulated time base: HCLK cycles since power-on.
///
/// Every timestamp in the engine (events, telemetry, recordings) is a cycle count from this clock,
/// so a run replays bit for bit on any host at any speed. Conversions to [`Duration`] use integer
/// math for the same reason; they exist for display and for the pacer outside the core, never for
/// decisions inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    hz: u64,
    now: u64,
}

impl Clock {
    /// A clock at cycle 0 that ticks `hz` times per simulated second.
    ///
    /// # Panics
    ///
    /// If `hz` is zero.
    pub fn new(hz: u64) -> Self {
        assert!(hz > 0, "clock frequency must be non-zero");
        Self { hz, now: 0 }
    }

    /// Cycles per simulated second.
    pub const fn hz(&self) -> u64 {
        self.hz
    }

    /// The current cycle.
    pub const fn now(&self) -> u64 {
        self.now
    }

    /// Moves time forward to cycle `at`.
    ///
    /// # Panics
    ///
    /// If `at` is in the past. Time running backwards means the run loop is broken, and carrying
    /// on would silently reorder events.
    pub fn advance_to(&mut self, at: u64) {
        assert!(at >= self.now, "clock moved backwards: {} -> {at}", self.now);
        self.now = at;
    }

    /// Moves time forward by `cycles`.
    pub fn advance_by(&mut self, cycles: u64) {
        self.advance_to(self.now + cycles);
    }

    /// Whole cycles in `duration`, rounded down (saturates at `u64::MAX`).
    pub fn cycles_in(&self, duration: Duration) -> u64 {
        let cycles = duration.as_nanos() * u128::from(self.hz) / NANOS_PER_SEC;
        u64::try_from(cycles).unwrap_or(u64::MAX)
    }

    /// The simulated time that `cycles` cycles take, rounded down to whole nanoseconds.
    pub fn duration_of(&self, cycles: u64) -> Duration {
        let nanos = u128::from(cycles) * NANOS_PER_SEC / u128::from(self.hz);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }

    /// Simulated time since power-on.
    pub fn elapsed(&self) -> Duration {
        self.duration_of(self.now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HCLK: u64 = 72_000_000;

    #[test]
    fn starts_at_zero_and_advances() {
        let mut clock = Clock::new(HCLK);
        assert_eq!(clock.now(), 0);
        clock.advance_by(100);
        clock.advance_to(250);
        assert_eq!(clock.now(), 250);
        assert_eq!(clock.hz(), HCLK);
    }

    #[test]
    fn converts_with_integer_math() {
        let clock = Clock::new(HCLK);
        assert_eq!(clock.cycles_in(Duration::from_millis(1)), 72_000);
        assert_eq!(clock.duration_of(HCLK), Duration::from_secs(1));
        // Fractions are dropped, never rounded up: 20 ns is 1.44 cycles, 1 cycle is 13.89 ns.
        assert_eq!(clock.cycles_in(Duration::from_nanos(20)), 1);
        assert_eq!(clock.duration_of(1), Duration::from_nanos(13));
        assert_eq!(clock.cycles_in(Duration::MAX), u64::MAX);
    }

    #[test]
    fn elapsed_follows_now() {
        let mut clock = Clock::new(HCLK);
        clock.advance_to(HCLK * 2 + HCLK / 2);
        assert_eq!(clock.elapsed(), Duration::from_millis(2500));
    }

    #[test]
    #[should_panic(expected = "clock moved backwards")]
    fn refuses_to_go_backwards() {
        let mut clock = Clock::new(HCLK);
        clock.advance_to(10);
        clock.advance_to(9);
    }
}
