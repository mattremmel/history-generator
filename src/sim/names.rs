use rand::RngCore;
use rand::Rng;

const FIRST_PREFIXES: &[&str] = &[
    "Al", "Ar", "Bal", "Bel", "Bor", "Cal", "Cor", "Dar", "Del", "Dor",
    "El", "Er", "Fal", "Fen", "Gar", "Gil", "Hal", "Ith", "Kal", "Kel",
    "Lor", "Mal", "Mar", "Mor", "Nar", "Nor", "Or", "Pel", "Ral", "Ren",
    "Sal", "Sel", "Tar", "Tel", "Thal", "Tor", "Val", "Var", "Zan", "Zor",
];

const FIRST_SUFFIXES: &[&str] = &[
    "an", "ar", "as", "en", "er", "ia", "id", "il", "in", "ion",
    "is", "na", "on", "or", "ra", "ren", "ric", "rin", "us", "wen",
];

const SURNAMES: &[&str] = &[
    "Ashford", "Blackthorn", "Brightwater", "Coldwell", "Dunmere",
    "Fairwind", "Greymoor", "Hartwood", "Ironhand", "Kingsward",
    "Longbridge", "Mossbank", "Northgate", "Oakshield", "Pinehurst",
    "Ravencrest", "Silverleaf", "Stonemark", "Thornwall", "Whitevale",
];

/// Generate a random person name (first + surname).
pub fn generate_person_name(rng: &mut dyn RngCore) -> String {
    let prefix = FIRST_PREFIXES[rng.random_range(0..FIRST_PREFIXES.len())];
    let suffix = FIRST_SUFFIXES[rng.random_range(0..FIRST_SUFFIXES.len())];
    let surname = SURNAMES[rng.random_range(0..SURNAMES.len())];
    format!("{prefix}{suffix} {surname}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn generates_nonempty_name() {
        let mut rng = SmallRng::seed_from_u64(42);
        let name = generate_person_name(&mut rng);
        assert!(!name.is_empty());
        assert!(name.contains(' '), "name should have first and last: {name}");
    }

    #[test]
    fn deterministic() {
        let mut rng1 = SmallRng::seed_from_u64(123);
        let mut rng2 = SmallRng::seed_from_u64(123);
        assert_eq!(generate_person_name(&mut rng1), generate_person_name(&mut rng2));
    }
}
