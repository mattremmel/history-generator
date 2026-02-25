use std::fmt;

use serde::{Deserialize, Serialize};

const HOUR_BITS: u32 = 5;
const DAY_BITS: u32 = 9;
const DAY_SHIFT: u32 = HOUR_BITS;
const YEAR_SHIFT: u32 = HOUR_BITS + DAY_BITS;

const HOUR_MASK: u32 = (1 << HOUR_BITS) - 1;
const DAY_MASK: u32 = (1 << DAY_BITS) - 1;

pub const DAYS_PER_YEAR: u32 = 360;
pub const HOURS_PER_DAY: u32 = 24;
pub const MONTHS_PER_YEAR: u32 = 12;
pub const DAYS_PER_MONTH: u32 = 30;

/// Compact simulation timestamp encoding year/day/hour in a single `u32`.
///
/// Bit layout: `[year:18][day_of_year:9][hour:5]`
/// - bits 14-31: year (0–262,143)
/// - bits 5-13:  day  (1–360)
/// - bits 0-4:   hour (0–23)
///
/// Natural `u32` ordering equals chronological ordering.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "TimestampRepr", from = "TimestampRepr")]
pub struct SimTimestamp(u32);

#[derive(Serialize, Deserialize)]
struct TimestampRepr {
    year: u32,
    day: u32,
    hour: u32,
}

impl From<SimTimestamp> for TimestampRepr {
    fn from(ts: SimTimestamp) -> Self {
        TimestampRepr {
            year: ts.year(),
            day: ts.day(),
            hour: ts.hour(),
        }
    }
}

impl From<TimestampRepr> for SimTimestamp {
    fn from(repr: TimestampRepr) -> Self {
        SimTimestamp::new(repr.year, repr.day, repr.hour)
    }
}

impl SimTimestamp {
    /// Create a timestamp from year, day-of-year (1–360), and hour (0–23).
    pub fn new(year: u32, day: u32, hour: u32) -> Self {
        assert!(
            (1..=DAYS_PER_YEAR).contains(&day),
            "day out of range: {day}"
        );
        assert!(hour < HOURS_PER_DAY, "hour out of range: {hour}");
        Self((year << YEAR_SHIFT) | (day << DAY_SHIFT) | hour)
    }

    /// Create a timestamp for the start of a year (day 1, hour 0).
    pub fn from_year(year: u32) -> Self {
        Self::new(year, 1, 0)
    }

    /// Create a timestamp for the first day of a month (day 1 of that month, hour 0).
    pub fn from_year_month(year: u32, month: u32) -> Self {
        debug_assert!(
            (1..=MONTHS_PER_YEAR).contains(&month),
            "month out of range: {month}"
        );
        let day = (month - 1) * DAYS_PER_MONTH + 1;
        Self::new(year, day, 0)
    }

    /// Create a timestamp from a raw packed `u32`.
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub fn year(self) -> u32 {
        self.0 >> YEAR_SHIFT
    }

    pub fn day(self) -> u32 {
        (self.0 >> DAY_SHIFT) & DAY_MASK
    }

    pub fn hour(self) -> u32 {
        self.0 & HOUR_MASK
    }

    /// Month of year (1–12), derived from day.
    pub fn month(self) -> u32 {
        (self.day() - 1) / DAYS_PER_MONTH + 1
    }

    /// Day within the month (1–30).
    pub fn day_of_month(self) -> u32 {
        (self.day() - 1) % DAYS_PER_MONTH + 1
    }

    /// Return the raw packed `u32` value.
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for SimTimestamp {
    fn default() -> Self {
        Self::from_year(0)
    }
}

impl fmt::Display for SimTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Y{}.D{}.H{}", self.year(), self.day(), self.hour())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_round_trip() {
        let ts = SimTimestamp::new(125, 180, 12);
        assert_eq!(ts.year(), 125);
        assert_eq!(ts.day(), 180);
        assert_eq!(ts.hour(), 12);
    }

    #[test]
    fn from_year_defaults() {
        let ts = SimTimestamp::from_year(500);
        assert_eq!(ts.year(), 500);
        assert_eq!(ts.day(), 1);
        assert_eq!(ts.hour(), 0);
    }

    #[test]
    fn from_raw_round_trip() {
        let ts = SimTimestamp::new(42, 100, 23);
        let raw = ts.as_u32();
        assert_eq!(SimTimestamp::from_raw(raw), ts);
    }

    #[test]
    fn chronological_ordering() {
        let a = SimTimestamp::new(100, 1, 0);
        let b = SimTimestamp::new(100, 1, 5);
        let c = SimTimestamp::new(100, 2, 0);
        let d = SimTimestamp::new(101, 1, 0);
        assert!(a < b);
        assert!(b < c);
        assert!(c < d);
    }

    #[test]
    fn month_derivation() {
        // Day 1 → month 1
        assert_eq!(SimTimestamp::new(1, 1, 0).month(), 1);
        assert_eq!(SimTimestamp::new(1, 1, 0).day_of_month(), 1);

        // Day 30 → month 1, day 30
        assert_eq!(SimTimestamp::new(1, 30, 0).month(), 1);
        assert_eq!(SimTimestamp::new(1, 30, 0).day_of_month(), 30);

        // Day 31 → month 2, day 1
        assert_eq!(SimTimestamp::new(1, 31, 0).month(), 2);
        assert_eq!(SimTimestamp::new(1, 31, 0).day_of_month(), 1);

        // Day 360 → month 12, day 30
        assert_eq!(SimTimestamp::new(1, 360, 0).month(), 12);
        assert_eq!(SimTimestamp::new(1, 360, 0).day_of_month(), 30);
    }

    #[test]
    fn serde_round_trip() {
        let ts = SimTimestamp::new(125, 45, 8);
        let json = serde_json::to_string(&ts).unwrap();
        let parsed: SimTimestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, parsed);
    }

    #[test]
    fn serde_shape() {
        let ts = SimTimestamp::new(125, 45, 8);
        let value = serde_json::to_value(ts).unwrap();
        assert_eq!(value["year"], 125);
        assert_eq!(value["day"], 45);
        assert_eq!(value["hour"], 8);
    }

    #[test]
    fn display_format() {
        let ts = SimTimestamp::new(125, 1, 0);
        assert_eq!(ts.to_string(), "Y125.D1.H0");
    }

    #[test]
    fn boundary_values() {
        // Max year: 2^18 - 1 = 262143
        let ts = SimTimestamp::new(262_143, 360, 23);
        assert_eq!(ts.year(), 262_143);
        assert_eq!(ts.day(), 360);
        assert_eq!(ts.hour(), 23);
    }
}
