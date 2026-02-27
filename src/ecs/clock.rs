use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;

use super::time::SimTime;

/// Simulation clock resource tracking the current time and tick count.
///
/// Advances by one minute per tick. The `advance_clock` system moves the clock
/// forward at the end of each tick (in `SimPhase::Last`), so systems see the
/// current time before it advances.
#[derive(Resource)]
pub struct SimClock {
    pub time: SimTime,
    pub tick_count: u64,
}

impl SimClock {
    pub fn new(start_year: u32) -> Self {
        Self {
            time: SimTime::from_year(start_year),
            tick_count: 0,
        }
    }

    /// Advance the clock by one minute.
    pub fn advance(&mut self) {
        self.time = SimTime::from_minutes(self.time.as_minutes() + 1);
        self.tick_count += 1;
    }
}

/// Bevy system that advances the simulation clock by one minute.
/// Registered in `SimPhase::Last` so all other systems see the current
/// time before it advances.
pub fn advance_clock(mut clock: ResMut<SimClock>) {
    clock.advance();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::time::{MINUTES_PER_DAY, MINUTES_PER_MONTH, MINUTES_PER_YEAR};

    #[test]
    fn new_clock_starts_at_given_year() {
        let clock = SimClock::new(100);
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.month(), 1);
        assert_eq!(clock.time.day(), 1);
        assert_eq!(clock.time.hour(), 0);
        assert_eq!(clock.time.minute(), 0);
        assert_eq!(clock.tick_count, 0);
    }

    #[test]
    fn advance_increments_minute() {
        let mut clock = SimClock::new(100);
        clock.advance();
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.minute(), 1);
        assert_eq!(clock.tick_count, 1);
    }

    #[test]
    fn advance_rolls_over_hour() {
        let mut clock = SimClock::new(100);
        for _ in 0..60 {
            clock.advance();
        }
        assert_eq!(clock.time.hour(), 1);
        assert_eq!(clock.time.minute(), 0);
        assert_eq!(clock.tick_count, 60);
    }

    #[test]
    fn advance_rolls_over_day() {
        let mut clock = SimClock::new(100);
        for _ in 0..MINUTES_PER_DAY {
            clock.advance();
        }
        assert_eq!(clock.time.day(), 2);
        assert_eq!(clock.time.hour(), 0);
        assert_eq!(clock.time.minute(), 0);
    }

    #[test]
    fn advance_rolls_over_month() {
        let mut clock = SimClock::new(100);
        for _ in 0..MINUTES_PER_MONTH {
            clock.advance();
        }
        assert_eq!(clock.time.month(), 2);
        assert_eq!(clock.time.day_of_month(), 1);
    }

    #[test]
    fn advance_rolls_over_year() {
        let mut clock = SimClock::new(100);
        for _ in 0..MINUTES_PER_YEAR {
            clock.advance();
        }
        assert_eq!(clock.time.year(), 101);
        assert_eq!(clock.time.month(), 1);
        assert_eq!(clock.time.day(), 1);
        assert_eq!(clock.tick_count, MINUTES_PER_YEAR as u64);
    }

    #[test]
    fn year_start_every_year() {
        let mut clock = SimClock::new(100);
        let mut year_starts = Vec::new();
        // Run 2 years + 1 tick
        for tick in 0..(MINUTES_PER_YEAR * 2 + 1) {
            if clock.time.is_year_start() {
                year_starts.push(tick);
            }
            clock.advance();
        }
        assert_eq!(year_starts, vec![0, MINUTES_PER_YEAR, MINUTES_PER_YEAR * 2]);
    }
}
