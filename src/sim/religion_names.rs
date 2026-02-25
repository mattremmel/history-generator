use rand::Rng;
use rand::RngCore;

// --- Religion name generation ---
// Pattern-based: "The Path of Iron", "The Verdant Faith", "Order of the Flame"

const RELIGION_PATTERNS: &[&str] = &[
    "The {adj} Faith",
    "The {adj} Path",
    "The {adj} Order",
    "Order of the {noun}",
    "Path of {noun}",
    "The {noun} Covenant",
    "Cult of the {noun}",
    "The {adj} Creed",
    "Children of the {noun}",
    "The {noun} Doctrine",
    "Keepers of the {noun}",
    "The {adj} Communion",
];

const RELIGION_ADJECTIVES: &[&str] = &[
    "Verdant",
    "Crimson",
    "Golden",
    "Silver",
    "Iron",
    "Eternal",
    "Sacred",
    "Ashen",
    "Radiant",
    "Silent",
    "Burning",
    "Twilight",
    "Ancient",
    "Celestial",
    "Hallowed",
    "Obsidian",
    "Ivory",
    "Hollow",
    "Sunlit",
    "Starborn",
];

const RELIGION_NOUNS: &[&str] = &[
    "Flame", "Storm", "Stone", "Sun", "Moon", "Stars", "Tide", "Mountain", "Serpent", "Eagle",
    "Wolf", "Oak", "Ash", "Dawn", "Dusk", "Thunder", "Frost", "Shadow", "Light", "Void",
];

/// Generate a religion name from pattern tables.
pub fn generate_religion_name(rng: &mut dyn RngCore) -> String {
    let pattern = RELIGION_PATTERNS[rng.random_range(0..RELIGION_PATTERNS.len())];
    let adj = RELIGION_ADJECTIVES[rng.random_range(0..RELIGION_ADJECTIVES.len())];
    let noun = RELIGION_NOUNS[rng.random_range(0..RELIGION_NOUNS.len())];
    pattern.replace("{adj}", adj).replace("{noun}", noun)
}

// --- Deity name generation ---
// Prefix + suffix from syllable tables: "Thalgoth", "Aesnor", "Nulmir"

const DEITY_PREFIXES: &[&str] = &[
    "Thal", "Aes", "Nul", "Vor", "Kel", "Zan", "Mor", "Ith", "Bal", "Sar", "Dra", "Fen", "Gul",
    "Orn", "Pyr", "Ael", "Cyr", "Hel", "Lur", "Myr", "Rhe", "Sel", "Val", "Xar", "Yth",
];

const DEITY_SUFFIXES: &[&str] = &[
    "goth", "nor", "mir", "oth", "ren", "vex", "thas", "iel", "nar", "kos", "zul", "dar", "mus",
    "phen", "ax", "ion", "us", "ra", "sha", "eth",
];

/// Generate a deity name from syllable tables.
pub fn generate_deity_name(rng: &mut dyn RngCore) -> String {
    let prefix = DEITY_PREFIXES[rng.random_range(0..DEITY_PREFIXES.len())];
    let suffix = DEITY_SUFFIXES[rng.random_range(0..DEITY_SUFFIXES.len())];
    format!("{prefix}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn religion_names_are_nonempty() {
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..50 {
            let name = generate_religion_name(&mut rng);
            assert!(!name.is_empty());
            assert!(!name.contains('{'));
        }
    }

    #[test]
    fn deity_names_are_nonempty() {
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..50 {
            let name = generate_deity_name(&mut rng);
            assert!(!name.is_empty());
            assert!(name.len() >= 4);
        }
    }
}
