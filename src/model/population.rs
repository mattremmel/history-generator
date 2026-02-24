use std::ops::AddAssign;

use rand::RngCore;
use serde::{Deserialize, Serialize};

pub const NUM_BRACKETS: usize = 8;

/// Width in years of each age bracket.
pub const BRACKET_WIDTHS: [u32; NUM_BRACKETS] = [6, 10, 25, 20, 15, 15, 9, u32::MAX];

/// Index of the young-adult bracket (16–40), used for rounding corrections,
/// fertility, and able-bodied counts.
pub const YOUNG_ADULT: usize = 2;
/// Index of the middle-age bracket (41–60).
pub const MIDDLE_AGE: usize = 3;

/// Annual mortality rate per bracket.
pub const BRACKET_MORTALITY: [f64; NUM_BRACKETS] =
    [0.03, 0.005, 0.008, 0.015, 0.04, 0.10, 0.25, 1.0];

pub const BRACKET_LABELS: [&str; NUM_BRACKETS] = [
    "infant",
    "child",
    "young_adult",
    "middle_age",
    "elder",
    "aged",
    "ancient",
    "centenarian",
];

/// Medieval-style age pyramid weights for initial distribution.
const PYRAMID_WEIGHTS: [f64; NUM_BRACKETS] = [0.12, 0.18, 0.30, 0.20, 0.12, 0.06, 0.02, 0.00];

/// Annual birth rate per fertile woman.
const BIRTH_RATE: f64 = 0.12;

/// Tracks population by 8 age brackets × 2 sexes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopulationBreakdown {
    pub male: [u32; NUM_BRACKETS],
    pub female: [u32; NUM_BRACKETS],
}

/// Stochastic rounding: values < 1.0 are probabilistically rounded to 0 or 1,
/// values >= 1.0 are rounded normally. Avoids systematic bias in small populations.
fn stochastic_round(exact: f64, rng: &mut dyn RngCore) -> u32 {
    use rand::Rng;
    if exact < 1.0 && exact > 0.0 {
        if rng.random_range(0.0..1.0) < exact {
            1
        } else {
            0
        }
    } else {
        exact.round() as u32
    }
}

impl Default for PopulationBreakdown {
    fn default() -> Self {
        Self::empty()
    }
}

impl AddAssign<&PopulationBreakdown> for PopulationBreakdown {
    fn add_assign(&mut self, other: &PopulationBreakdown) {
        for i in 0..NUM_BRACKETS {
            self.male[i] += other.male[i];
            self.female[i] += other.female[i];
        }
    }
}

impl PopulationBreakdown {
    pub fn empty() -> Self {
        Self {
            male: [0; NUM_BRACKETS],
            female: [0; NUM_BRACKETS],
        }
    }

    /// Distribute a total population across brackets using medieval age pyramid weights.
    /// 50/50 male/female split. Rounding remainder is fixed on young_adult bracket.
    pub fn from_total(total: u32) -> Self {
        let half = total / 2;
        let remainder = total - half * 2; // 0 or 1

        let mut male = [0u32; NUM_BRACKETS];
        let mut female = [0u32; NUM_BRACKETS];

        let mut male_sum = 0u32;
        let mut female_sum = 0u32;

        for i in 0..NUM_BRACKETS {
            male[i] = (half as f64 * PYRAMID_WEIGHTS[i]).round() as u32;
            female[i] = (half as f64 * PYRAMID_WEIGHTS[i]).round() as u32;
            male_sum += male[i];
            female_sum += female[i];
        }

        // Fix rounding errors on young_adult bracket
        if male_sum != half {
            male[YOUNG_ADULT] = male[YOUNG_ADULT].wrapping_add(half.wrapping_sub(male_sum));
        }
        if female_sum != (half + remainder) {
            female[YOUNG_ADULT] =
                female[YOUNG_ADULT].wrapping_add((half + remainder).wrapping_sub(female_sum));
        }

        Self { male, female }
    }

    pub fn total(&self) -> u32 {
        self.male.iter().sum::<u32>() + self.female.iter().sum::<u32>()
    }

    /// Count of fertile women (young_adult bracket, ages 16-40).
    pub fn fertile_women(&self) -> u32 {
        self.female[YOUNG_ADULT]
    }

    /// Count of able-bodied men (young_adult + middle_age, ages 16-60).
    pub fn able_bodied_men(&self) -> u32 {
        self.male[YOUNG_ADULT] + self.male[MIDDLE_AGE]
    }

    /// Total population (both sexes) for a given bracket index.
    pub fn bracket_total(&self, i: usize) -> u32 {
        self.male[i] + self.female[i]
    }

    /// Remove a fraction of each bracket, returning the removed chunk.
    /// Uses stochastic rounding for small values to avoid systematic bias.
    pub fn subtract_fraction(
        &mut self,
        fraction: f64,
        rng: &mut dyn RngCore,
    ) -> PopulationBreakdown {
        let fraction = fraction.clamp(0.0, 1.0);
        let mut removed = PopulationBreakdown::empty();
        for i in 0..NUM_BRACKETS {
            for (src, dst) in [
                (&mut self.male, &mut removed.male),
                (&mut self.female, &mut removed.female),
            ] {
                let taken = stochastic_round(src[i] as f64 * fraction, rng).min(src[i]);
                src[i] -= taken;
                dst[i] = taken;
            }
        }
        removed
    }

    /// Scale all brackets proportionally so total() matches the given target.
    pub fn scale_to(&mut self, new_total: u32) {
        let current = self.total();
        if current == 0 || current == new_total {
            return;
        }
        let ratio = new_total as f64 / current as f64;
        let mut running = 0u32;
        for arr in [&mut self.male, &mut self.female] {
            for val in arr.iter_mut() {
                *val = (*val as f64 * ratio).round() as u32;
                running += *val;
            }
        }
        // Fix rounding difference on male young_adult bracket
        if running != new_total {
            let diff = new_total as i64 - running as i64;
            self.male[YOUNG_ADULT] = (self.male[YOUNG_ADULT] as i64 + diff).max(0) as u32;
        }
    }

    /// Apply extra disease mortality to each bracket.
    /// `rates[i]` is the additional death rate for bracket `i` this year.
    /// Returns the total number of deaths caused.
    pub fn apply_disease_mortality(
        &mut self,
        rates: &[f64; NUM_BRACKETS],
        rng: &mut dyn RngCore,
    ) -> u32 {
        let mut total_deaths = 0u32;
        for i in 0..NUM_BRACKETS {
            for counts in [&mut self.male, &mut self.female] {
                let rate = rates[i].clamp(0.0, 1.0);
                let deaths = stochastic_round(counts[i] as f64 * rate, rng).min(counts[i]);
                counts[i] -= deaths;
                total_deaths += deaths;
            }
        }
        total_deaths
    }

    /// Advance one year: apply deaths, age cohorts, then compute births.
    pub fn tick_year(&mut self, carrying_capacity: u32, rng: &mut dyn RngCore) {
        use rand::Rng;

        // Phase 1: Deaths
        for i in 0..NUM_BRACKETS {
            for counts in [&mut self.male, &mut self.female] {
                if BRACKET_MORTALITY[i] >= 1.0 {
                    // Guaranteed death (centenarians)
                    counts[i] = 0;
                } else {
                    let noise: f64 = rng.random_range(0.85..1.15);
                    let deaths = (counts[i] as f64 * BRACKET_MORTALITY[i] * noise).round() as u32;
                    counts[i] = counts[i].saturating_sub(deaths);
                }
            }
        }

        // Phase 2: Aging — promote fraction from bracket i to i+1.
        // For small counts where count/width < 1, use a probabilistic roll
        // so the expected promotion rate stays correct (no artificial speedup).
        for i in 0..NUM_BRACKETS - 1 {
            let width = BRACKET_WIDTHS[i];
            for counts in [&mut self.male, &mut self.female] {
                if counts[i] == 0 {
                    continue;
                }
                let promoted = stochastic_round(counts[i] as f64 / width as f64, rng);
                counts[i] = counts[i].saturating_sub(promoted);
                counts[i + 1] += promoted;
            }
        }

        // Phase 3: Births
        let total = self.total();
        let capacity_factor = (1.0 - total as f64 / carrying_capacity.max(1) as f64).max(0.0);
        let noise: f64 = rng.random_range(0.85..1.15);
        let births =
            (self.fertile_women() as f64 * BIRTH_RATE * capacity_factor * noise).round() as u32;
        let male_births = births / 2;
        let female_births = births - male_births;
        self.male[0] += male_births;
        self.female[0] += female_births;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn from_total_sums_correctly() {
        for total in [0, 1, 50, 100, 500, 1000, 9999] {
            let bd = PopulationBreakdown::from_total(total);
            assert_eq!(
                bd.total(),
                total,
                "from_total({total}).total() should equal {total}, got {}",
                bd.total()
            );
        }
    }

    #[test]
    fn empty_is_zero() {
        let bd = PopulationBreakdown::empty();
        assert_eq!(bd.total(), 0);
        assert_eq!(bd.fertile_women(), 0);
        assert_eq!(bd.able_bodied_men(), 0);
    }

    #[test]
    fn tick_year_deaths_reduce_population() {
        let mut bd = PopulationBreakdown::from_total(1000);
        let mut rng = SmallRng::seed_from_u64(42);
        let before = bd.total();
        // Set carrying capacity very high so births don't compensate
        bd.tick_year(1_000_000, &mut rng);
        // Population should have changed (deaths happen)
        assert_ne!(bd.total(), before, "population should change after tick");
    }

    #[test]
    fn tick_year_births_grow_population() {
        let mut bd = PopulationBreakdown::from_total(100);
        let mut rng = SmallRng::seed_from_u64(42);
        // Run many ticks with high capacity to allow growth
        for _ in 0..50 {
            bd.tick_year(100_000, &mut rng);
        }
        assert!(
            bd.total() > 100,
            "population should grow with high capacity"
        );
    }

    #[test]
    fn centenarians_die() {
        let mut bd = PopulationBreakdown::empty();
        bd.male[7] = 100;
        bd.female[7] = 100;
        let mut rng = SmallRng::seed_from_u64(42);
        bd.tick_year(10_000, &mut rng);
        // Centenarian mortality is 100% (with noise 0.85-1.15, all should die)
        assert_eq!(bd.male[7], 0, "male centenarians should all die");
        assert_eq!(bd.female[7], 0, "female centenarians should all die");
    }

    #[test]
    fn collapse_when_all_elderly() {
        let mut bd = PopulationBreakdown::empty();
        // Only elderly, no fertile women — should collapse as they age
        // through to centenarian and die. Probabilistic aging means we
        // need enough ticks for the pipeline to drain.
        bd.male[4] = 50;
        bd.female[4] = 50;
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..300 {
            bd.tick_year(10_000, &mut rng);
        }
        assert_eq!(
            bd.total(),
            0,
            "population of only elderly should fully collapse, got {}",
            bd.total()
        );
    }

    #[test]
    fn small_settlement_survives() {
        // A settlement of 50 should be able to sustain itself, not collapse
        // due to artificially fast aging.
        let mut bd = PopulationBreakdown::from_total(50);
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..200 {
            bd.tick_year(500, &mut rng);
        }
        assert!(
            bd.total() >= 10,
            "small settlement of 50 should survive 200 years, got {}",
            bd.total()
        );
        assert!(
            bd.fertile_women() > 0,
            "should still have fertile women after 200 years"
        );
    }

    #[test]
    fn convenience_methods() {
        let bd = PopulationBreakdown::from_total(1000);
        assert!(bd.fertile_women() > 0);
        assert!(bd.able_bodied_men() > 0);
        assert_eq!(bd.bracket_total(2), bd.male[2] + bd.female[2]);
    }

    #[test]
    fn serde_round_trip() {
        let bd = PopulationBreakdown::from_total(500);
        let json = serde_json::to_value(&bd).unwrap();
        let deserialized: PopulationBreakdown = serde_json::from_value(json).unwrap();
        assert_eq!(bd, deserialized);
    }

    #[test]
    fn subtract_fraction_preserves_total() {
        let mut rng = SmallRng::seed_from_u64(42);
        let original = PopulationBreakdown::from_total(1000);
        let mut source = original.clone();
        let removed = source.subtract_fraction(0.25, &mut rng);
        assert_eq!(
            source.total() + removed.total(),
            original.total(),
            "source + removed should equal original"
        );
    }

    #[test]
    fn subtract_fraction_zero_removes_nothing() {
        let mut rng = SmallRng::seed_from_u64(42);
        let original = PopulationBreakdown::from_total(500);
        let mut source = original.clone();
        let removed = source.subtract_fraction(0.0, &mut rng);
        assert_eq!(removed.total(), 0);
        assert_eq!(source, original);
    }

    #[test]
    fn subtract_fraction_one_removes_all() {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut source = PopulationBreakdown::from_total(500);
        let removed = source.subtract_fraction(1.0, &mut rng);
        assert_eq!(source.total(), 0);
        assert_eq!(removed.total(), 500);
    }

    #[test]
    fn add_assign_sums_correctly() {
        let a = PopulationBreakdown::from_total(300);
        let b = PopulationBreakdown::from_total(200);
        let mut dest = a.clone();
        dest += &b;
        assert_eq!(dest.total(), a.total() + b.total());
        for i in 0..NUM_BRACKETS {
            assert_eq!(dest.male[i], a.male[i] + b.male[i]);
            assert_eq!(dest.female[i], a.female[i] + b.female[i]);
        }
    }
}
