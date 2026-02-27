use bevy_ecs::system::Res;

use crate::model::SimTimestamp;

use super::clock::SimClock;

// Internal check functions for testability.

fn yearly_check(time: SimTimestamp) -> bool {
    time.hour() == 0 && time.day() == 1
}

fn monthly_check(time: SimTimestamp) -> bool {
    time.hour() == 0 && time.day_of_month() == 1
}

fn weekly_check(time: SimTimestamp) -> bool {
    time.hour() == 0 && (time.day() - 1).is_multiple_of(7)
}

fn daily_check(time: SimTimestamp) -> bool {
    time.hour() == 0
}

fn hourly_check(_time: SimTimestamp) -> bool {
    true
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
    use crate::model::timestamp::{DAYS_PER_MONTH, MONTHS_PER_YEAR};
    use crate::sim::should_fire;
    use crate::sim::TickFrequency;

    /// Cross-reference every condition against the existing `should_fire()` function
    /// for a variety of timestamps.
    #[test]
    fn parity_with_should_fire() {
        let timestamps = [
            SimTimestamp::new(1, 1, 0),    // year start
            SimTimestamp::new(1, 1, 5),    // year start but non-zero hour
            SimTimestamp::new(1, 31, 0),   // month 2 start
            SimTimestamp::new(1, 8, 0),    // week 2 start
            SimTimestamp::new(1, 15, 0),   // mid-month
            SimTimestamp::new(1, 180, 12), // mid-year, mid-day
            SimTimestamp::new(1, 360, 0),  // last day
            SimTimestamp::new(1, 360, 23), // last hour of year
            SimTimestamp::new(1, 211, 0),  // month 8 start (week-aligned)
        ];

        for ts in timestamps {
            assert_eq!(
                yearly_check(ts),
                should_fire(TickFrequency::Yearly, ts),
                "yearly mismatch at {ts}"
            );
            assert_eq!(
                monthly_check(ts),
                should_fire(TickFrequency::Monthly, ts),
                "monthly mismatch at {ts}"
            );
            assert_eq!(
                weekly_check(ts),
                should_fire(TickFrequency::Weekly, ts),
                "weekly mismatch at {ts}"
            );
            assert_eq!(
                daily_check(ts),
                should_fire(TickFrequency::Daily, ts),
                "daily mismatch at {ts}"
            );
            assert_eq!(
                hourly_check(ts),
                should_fire(TickFrequency::Hourly, ts),
                "hourly mismatch at {ts}"
            );
        }
    }

    /// Monthly fires 12 times per year (once per month start).
    #[test]
    fn monthly_fires_twelve_per_year() {
        let mut count = 0;
        for month in 1..=MONTHS_PER_YEAR {
            let ts = SimTimestamp::from_year_month(1, month);
            if monthly_check(ts) {
                count += 1;
            }
        }
        assert_eq!(count, 12);
    }

    /// Yearly fires exactly once per year.
    #[test]
    fn yearly_fires_once_per_year() {
        let mut count = 0;
        for month in 1..=MONTHS_PER_YEAR {
            let ts = SimTimestamp::from_year_month(1, month);
            if yearly_check(ts) {
                count += 1;
            }
        }
        assert_eq!(count, 1);
    }

    /// Weekly with monthly advance: only fires when the month-start day is week-aligned.
    /// Month start days: 1, 31, 61, 91, 121, 151, 181, 211, 241, 271, 301, 331
    /// (day-1) % 7 == 0 for day 1 (0%7=0) and day 211 (210%7=0).
    #[test]
    fn weekly_with_monthly_advance() {
        let mut fired_months = Vec::new();
        for month in 1..=MONTHS_PER_YEAR {
            let day = (month - 1) * DAYS_PER_MONTH + 1;
            let ts = SimTimestamp::new(1, day, 0);
            if weekly_check(ts) {
                fired_months.push(month);
            }
        }
        assert_eq!(fired_months, vec![1, 8]);
    }
}
