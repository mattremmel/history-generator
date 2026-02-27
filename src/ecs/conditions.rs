use bevy_ecs::system::Res;

use super::clock::SimClock;
use super::time::{
    SimTime, MINUTES_PER_DAY, MINUTES_PER_HOUR, MINUTES_PER_MONTH, MINUTES_PER_WEEK,
    MINUTES_PER_YEAR,
};

// Internal check functions for testability.

fn yearly_check(time: SimTime) -> bool {
    time.as_minutes().is_multiple_of(MINUTES_PER_YEAR)
}

fn monthly_check(time: SimTime) -> bool {
    time.as_minutes().is_multiple_of(MINUTES_PER_MONTH)
}

fn weekly_check(time: SimTime) -> bool {
    (time.as_minutes() % MINUTES_PER_YEAR).is_multiple_of(MINUTES_PER_WEEK)
}

fn daily_check(time: SimTime) -> bool {
    time.as_minutes().is_multiple_of(MINUTES_PER_DAY)
}

fn hourly_check(time: SimTime) -> bool {
    time.as_minutes().is_multiple_of(MINUTES_PER_HOUR)
}

// Bevy run condition functions (for use with `.run_if()`).

pub fn yearly(clock: Res<SimClock>) -> bool {
    yearly_check(clock.time)
}

pub fn monthly(clock: Res<SimClock>) -> bool {
    monthly_check(clock.time)
}

pub fn weekly(clock: Res<SimClock>) -> bool {
    weekly_check(clock.time)
}

pub fn daily(clock: Res<SimClock>) -> bool {
    daily_check(clock.time)
}

pub fn hourly(clock: Res<SimClock>) -> bool {
    hourly_check(clock.time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yearly_at_year_start() {
        assert!(yearly_check(SimTime::from_year(100)));
        assert!(yearly_check(SimTime::from_year(0)));
    }

    #[test]
    fn yearly_not_mid_year() {
        assert!(!yearly_check(SimTime::from_year_month(100, 2)));
        assert!(!yearly_check(SimTime::new(100, 1, 0, 1)));
    }

    #[test]
    fn monthly_at_month_starts() {
        for m in 1..=12 {
            assert!(
                monthly_check(SimTime::from_year_month(100, m)),
                "month {m} should fire"
            );
        }
    }

    #[test]
    fn monthly_not_mid_month() {
        assert!(!monthly_check(SimTime::new(100, 2, 0, 0)));
        assert!(!monthly_check(SimTime::new(100, 1, 0, 1)));
    }

    #[test]
    fn monthly_fires_twelve_per_year() {
        let mut count = 0;
        for m in 1..=12 {
            if monthly_check(SimTime::from_year_month(1, m)) {
                count += 1;
            }
        }
        assert_eq!(count, 12);
    }

    #[test]
    fn yearly_fires_once_per_year() {
        let mut count = 0;
        for m in 1..=12 {
            if yearly_check(SimTime::from_year_month(1, m)) {
                count += 1;
            }
        }
        assert_eq!(count, 1);
    }

    #[test]
    fn weekly_at_year_start() {
        // Day 1 of year is always a week start
        assert!(weekly_check(SimTime::from_year(100)));
    }

    #[test]
    fn weekly_every_seven_days() {
        let base = SimTime::from_year(1);
        let mut fired_days = Vec::new();
        for d in 1..=30 {
            let t = SimTime::new(1, d, 0, 0);
            if weekly_check(t) {
                fired_days.push(d);
            }
        }
        // Days 1, 8, 15, 22, 29
        assert_eq!(fired_days, vec![1, 8, 15, 22, 29]);
        // Verify base fires
        assert!(weekly_check(base));
    }

    #[test]
    fn weekly_not_mid_day() {
        // Day 8 at hour 1 should not fire
        assert!(!weekly_check(SimTime::new(1, 8, 1, 0)));
        assert!(!weekly_check(SimTime::new(1, 8, 0, 1)));
    }

    #[test]
    fn daily_at_midnight() {
        assert!(daily_check(SimTime::new(100, 1, 0, 0)));
        assert!(daily_check(SimTime::new(100, 15, 0, 0)));
    }

    #[test]
    fn daily_not_mid_day() {
        assert!(!daily_check(SimTime::new(100, 1, 5, 0)));
        assert!(!daily_check(SimTime::new(100, 1, 0, 1)));
    }

    #[test]
    fn hourly_at_hour_start() {
        assert!(hourly_check(SimTime::new(100, 1, 0, 0)));
        assert!(hourly_check(SimTime::new(100, 1, 12, 0)));
    }

    #[test]
    fn hourly_not_mid_hour() {
        assert!(!hourly_check(SimTime::new(100, 1, 0, 30)));
    }

    #[test]
    fn hourly_fires_24_per_day() {
        let mut count = 0;
        for h in 0..24 {
            if hourly_check(SimTime::new(1, 1, h, 0)) {
                count += 1;
            }
        }
        assert_eq!(count, 24);
    }
}
