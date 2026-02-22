use std::hash::{Hash, Hasher};

use rand::SeedableRng;
use rand::rngs::SmallRng;

/// Base offset for procgen IDs — upper half of u64, never collides with simulation IDs.
pub const PROCGEN_ID_BASE: u64 = 1 << 63;

/// Create a deterministic seed from settlement context + category discriminator.
pub fn make_seed(settlement_id: u64, year: u32, discriminator: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    settlement_id.hash(&mut hasher);
    year.hash(&mut hasher);
    discriminator.hash(&mut hasher);
    hasher.finish()
}

/// Create a seeded RNG for a specific generation category.
pub fn make_rng(settlement_id: u64, year: u32, discriminator: &str) -> SmallRng {
    SmallRng::seed_from_u64(make_seed(settlement_id, year, discriminator))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_same_seed() {
        let a = make_seed(42, 500, "inhabitants");
        let b = make_seed(42, 500, "inhabitants");
        assert_eq!(a, b);
    }

    #[test]
    fn different_discriminator_different_seed() {
        let a = make_seed(42, 500, "inhabitants");
        let b = make_seed(42, 500, "artifacts");
        assert_ne!(a, b);
    }

    #[test]
    fn different_settlement_different_seed() {
        let a = make_seed(1, 500, "inhabitants");
        let b = make_seed(2, 500, "inhabitants");
        assert_ne!(a, b);
    }

    #[test]
    fn different_year_different_seed() {
        let a = make_seed(42, 100, "inhabitants");
        let b = make_seed(42, 200, "inhabitants");
        assert_ne!(a, b);
    }

    #[test]
    fn make_rng_deterministic() {
        use rand::Rng;
        let mut rng1 = make_rng(42, 500, "test");
        let mut rng2 = make_rng(42, 500, "test");
        let vals1: Vec<u32> = (0..10).map(|_| rng1.random()).collect();
        let vals2: Vec<u32> = (0..10).map(|_| rng2.random()).collect();
        assert_eq!(vals1, vals2);
    }

    #[test]
    fn procgen_id_base_above_normal_range() {
        // Simulation IDs start at 1 and count up. Even after millions of entities,
        // they'll never reach 2^63.
        assert!(PROCGEN_ID_BASE > 1_000_000_000);
    }
}
