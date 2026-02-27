use std::fmt;

// Calendar constants (same fantasy calendar as old SimTimestamp).
pub const MINUTES_PER_HOUR: u32 = 60;
pub const HOURS_PER_DAY: u32 = 24;
pub const DAYS_PER_MONTH: u32 = 30;
pub const MONTHS_PER_YEAR: u32 = 12;
pub const DAYS_PER_YEAR: u32 = 360;

pub const MINUTES_PER_DAY: u32 = MINUTES_PER_HOUR * HOURS_PER_DAY; // 1,440
pub const MINUTES_PER_WEEK: u32 = MINUTES_PER_DAY * 7; // 10,080
pub const MINUTES_PER_MONTH: u32 = MINUTES_PER_DAY * DAYS_PER_MONTH; // 43,200
pub const MINUTES_PER_YEAR: u32 = MINUTES_PER_DAY * DAYS_PER_YEAR; // 518,400

/// Simulation time as total elapsed minutes since year 0.
///
/// A plain `u32` wrapper — no bit packing, just minutes. All calendar
/// accessors (year, month, day, hour, minute) are derived via
/// division/modulo. Natural `u32` ordering equals chronological ordering.
///
/// Max representable: ~8,285 years (`u32::MAX / MINUTES_PER_YEAR`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimTime(u32);

impl SimTime {
    /// Create from a raw minute count.
    pub fn from_minutes(minutes: u32) -> Self {
        Self(minutes)
    }

    /// Start of a year (day 1, 00:00).
    pub fn from_year(year: u32) -> Self {
        Self(year * MINUTES_PER_YEAR)
    }

    /// First minute of a month (1-indexed) within a year.
    pub fn from_year_month(year: u32, month: u32) -> Self {
        debug_assert!(
            (1..=MONTHS_PER_YEAR).contains(&month),
            "month out of range: {month}"
        );
        Self(year * MINUTES_PER_YEAR + (month - 1) * MINUTES_PER_MONTH)
    }

    /// Full specification: year, day-of-year (1–360), hour (0–23), minute (0–59).
    pub fn new(year: u32, day: u32, hour: u32, minute: u32) -> Self {
        debug_assert!(
            (1..=DAYS_PER_YEAR).contains(&day),
            "day out of range: {day}"
        );
        debug_assert!(hour < HOURS_PER_DAY, "hour out of range: {hour}");
        debug_assert!(minute < MINUTES_PER_HOUR, "minute out of range: {minute}");
        Self(
            year * MINUTES_PER_YEAR
                + (day - 1) * MINUTES_PER_DAY
                + hour * MINUTES_PER_HOUR
                + minute,
        )
    }

    /// The inner minute count.
    pub fn as_minutes(self) -> u32 {
        self.0
    }

    pub fn year(self) -> u32 {
        self.0 / MINUTES_PER_YEAR
    }

    /// Day of year (1–360).
    pub fn day(self) -> u32 {
        (self.0 % MINUTES_PER_YEAR) / MINUTES_PER_DAY + 1
    }

    /// Month of year (1–12).
    pub fn month(self) -> u32 {
        (self.day() - 1) / DAYS_PER_MONTH + 1
    }

    /// Day within the month (1–30).
    pub fn day_of_month(self) -> u32 {
        (self.day() - 1) % DAYS_PER_MONTH + 1
    }

    /// Hour of day (0–23).
    pub fn hour(self) -> u32 {
        (self.0 % MINUTES_PER_DAY) / MINUTES_PER_HOUR
    }

    /// Minute of hour (0–59).
    pub fn minute(self) -> u32 {
        self.0 % MINUTES_PER_HOUR
    }

    /// True at the first minute of a year.
    pub fn is_year_start(self) -> bool {
        self.0.is_multiple_of(MINUTES_PER_YEAR)
    }

    /// True at the first minute of a month.
    pub fn is_month_start(self) -> bool {
        self.0.is_multiple_of(MINUTES_PER_MONTH)
    }

    /// Whole years elapsed between `earlier` and `self` (saturating).
    pub fn years_since(self, earlier: SimTime) -> u32 {
        self.year().saturating_sub(earlier.year())
    }

    /// Whole months elapsed between `earlier` and `self` (saturating).
    pub fn months_since(self, earlier: SimTime) -> u32 {
        (self.0 / MINUTES_PER_MONTH).saturating_sub(earlier.0 / MINUTES_PER_MONTH)
    }
}

impl Default for SimTime {
    fn default() -> Self {
        Self::from_year(0)
    }
}

impl fmt::Display for SimTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Y{}.D{} {:02}:{:02}",
            self.year(),
            self.day(),
            self.hour(),
            self.minute()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_new() {
        let t = SimTime::new(125, 180, 12, 45);
        assert_eq!(t.year(), 125);
        assert_eq!(t.day(), 180);
        assert_eq!(t.hour(), 12);
        assert_eq!(t.minute(), 45);
    }

    #[test]
    fn from_year_defaults() {
        let t = SimTime::from_year(500);
        assert_eq!(t.year(), 500);
        assert_eq!(t.day(), 1);
        assert_eq!(t.hour(), 0);
        assert_eq!(t.minute(), 0);
    }

    #[test]
    fn from_year_month_round_trip() {
        let t = SimTime::from_year_month(10, 7);
        assert_eq!(t.year(), 10);
        assert_eq!(t.month(), 7);
        assert_eq!(t.day_of_month(), 1);
        assert_eq!(t.hour(), 0);
        assert_eq!(t.minute(), 0);
    }

    #[test]
    fn from_minutes_round_trip() {
        let t = SimTime::new(42, 100, 23, 59);
        let raw = t.as_minutes();
        assert_eq!(SimTime::from_minutes(raw), t);
    }

    #[test]
    fn chronological_ordering() {
        let a = SimTime::new(100, 1, 0, 0);
        let b = SimTime::new(100, 1, 0, 30);
        let c = SimTime::new(100, 1, 5, 0);
        let d = SimTime::new(100, 2, 0, 0);
        let e = SimTime::new(101, 1, 0, 0);
        assert!(a < b);
        assert!(b < c);
        assert!(c < d);
        assert!(d < e);
    }

    #[test]
    fn month_derivation() {
        assert_eq!(SimTime::new(1, 1, 0, 0).month(), 1);
        assert_eq!(SimTime::new(1, 1, 0, 0).day_of_month(), 1);
        assert_eq!(SimTime::new(1, 30, 0, 0).month(), 1);
        assert_eq!(SimTime::new(1, 30, 0, 0).day_of_month(), 30);
        // Day 31 → month 2, day 1
        assert_eq!(SimTime::new(1, 31, 0, 0).month(), 2);
        assert_eq!(SimTime::new(1, 31, 0, 0).day_of_month(), 1);
        // Day 360 → month 12, day 30
        assert_eq!(SimTime::new(1, 360, 0, 0).month(), 12);
        assert_eq!(SimTime::new(1, 360, 0, 0).day_of_month(), 30);
    }

    #[test]
    fn is_year_start() {
        assert!(SimTime::from_year(100).is_year_start());
        assert!(!SimTime::new(100, 1, 0, 1).is_year_start());
        assert!(!SimTime::new(100, 1, 1, 0).is_year_start());
        assert!(!SimTime::new(100, 2, 0, 0).is_year_start());
    }

    #[test]
    fn is_month_start() {
        assert!(SimTime::from_year_month(100, 3).is_month_start());
        assert!(SimTime::from_year(100).is_month_start());
        assert!(!SimTime::new(100, 2, 0, 0).is_month_start()); // day 2 of month 1
        assert!(!SimTime::new(100, 31, 0, 1).is_month_start()); // month 2 day 1 but minute 1
    }

    #[test]
    fn years_since() {
        let a = SimTime::from_year(100);
        let b = SimTime::from_year(120);
        assert_eq!(b.years_since(a), 20);
        assert_eq!(a.years_since(b), 0); // saturates
        assert_eq!(a.years_since(a), 0);
    }

    #[test]
    fn months_since() {
        let a = SimTime::from_year_month(100, 3);
        let b = SimTime::from_year_month(100, 7);
        assert_eq!(b.months_since(a), 4);
        // Cross-year
        let c = SimTime::from_year_month(101, 2);
        assert_eq!(c.months_since(a), 11);
        assert_eq!(a.months_since(c), 0); // saturates
    }

    #[test]
    fn display_format() {
        assert_eq!(SimTime::from_year(125).to_string(), "Y125.D1 00:00");
        assert_eq!(SimTime::new(125, 180, 12, 5).to_string(), "Y125.D180 12:05");
    }

    #[test]
    fn default_is_year_zero() {
        let t = SimTime::default();
        assert_eq!(t.year(), 0);
        assert_eq!(t.day(), 1);
        assert_eq!(t.as_minutes(), 0);
    }

    #[test]
    fn boundary_max_year() {
        let max_year = u32::MAX / MINUTES_PER_YEAR;
        let t = SimTime::from_year(max_year);
        assert_eq!(t.year(), max_year);
    }

    #[test]
    fn constants_are_consistent() {
        assert_eq!(MINUTES_PER_DAY, MINUTES_PER_HOUR * HOURS_PER_DAY);
        assert_eq!(MINUTES_PER_MONTH, MINUTES_PER_DAY * DAYS_PER_MONTH);
        assert_eq!(MINUTES_PER_YEAR, MINUTES_PER_DAY * DAYS_PER_YEAR);
        assert_eq!(DAYS_PER_YEAR, DAYS_PER_MONTH * MONTHS_PER_YEAR);
        // Months divide evenly into years
        assert_eq!(MINUTES_PER_YEAR % MINUTES_PER_MONTH, 0);
    }
}
