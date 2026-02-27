use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;

use crate::model::timestamp::MONTHS_PER_YEAR;
use crate::model::SimTimestamp;

/// Simulation clock resource tracking the current timestamp and tick count.
///
/// Advances by one month per tick. The `advance_clock` system moves the clock
/// forward at the end of each tick (in `SimPhase::Last`), so systems see the
/// current timestamp before it advances.
#[derive(Resource)]
pub struct SimClock {
    pub time: SimTimestamp,
    pub tick_count: u64,
}

impl SimClock {
    pub fn new(start_year: u32) -> Self {
        Self {
            time: SimTimestamp::from_year(start_year),
            tick_count: 0,
        }
    }

    /// Advance the clock by one month, rolling over the year if needed.
    pub fn advance(&mut self) {
        let year = self.time.year();
        let month = self.time.month();
        if month < MONTHS_PER_YEAR {
            self.time = SimTimestamp::from_year_month(year, month + 1);
        } else {
            self.time = SimTimestamp::from_year(year + 1);
        }
        self.tick_count += 1;
    }
}

/// Bevy system that advances the simulation clock by one month.
/// Registered in `SimPhase::Last` so all other systems see the current
/// timestamp before it advances.
pub fn advance_clock(mut clock: ResMut<SimClock>) {
    clock.advance();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clock_starts_at_given_year() {
        let clock = SimClock::new(100);
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.month(), 1);
        assert_eq!(clock.time.day(), 1);
        assert_eq!(clock.time.hour(), 0);
        assert_eq!(clock.tick_count, 0);
    }

    #[test]
    fn advance_increments_month() {
        let mut clock = SimClock::new(100);
        clock.advance();
        assert_eq!(clock.time.year(), 100);
        assert_eq!(clock.time.month(), 2);
        assert_eq!(clock.tick_count, 1);
    }

    #[test]
    fn advance_rolls_over_year() {
        let mut clock = SimClock::new(100);
        for _ in 0..12 {
            clock.advance();
        }
        assert_eq!(clock.time.year(), 101);
        assert_eq!(clock.time.month(), 1);
        assert_eq!(clock.tick_count, 12);
    }

    #[test]
    fn advance_through_multiple_years() {
        let mut clock = SimClock::new(100);
        for _ in 0..120 {
            clock.advance();
        }
        assert_eq!(clock.time.year(), 110);
        assert_eq!(clock.time.month(), 1);
        assert_eq!(clock.tick_count, 120);
    }

    #[test]
    fn advance_preserves_month_start_invariant() {
        let mut clock = SimClock::new(100);
        for _ in 0..24 {
            assert!(
                clock.time.is_month_start(),
                "timestamp {} is not a month start",
                clock.time
            );
            clock.advance();
        }
    }

    #[test]
    fn year_start_every_twelve_ticks() {
        let mut clock = SimClock::new(100);
        let mut year_starts = Vec::new();
        for tick in 0..36 {
            if clock.time.is_year_start() {
                year_starts.push(tick);
            }
            clock.advance();
        }
        // Year starts at ticks 0, 12, 24 (Y100, Y101, Y102)
        assert_eq!(year_starts, vec![0, 12, 24]);
    }
}
