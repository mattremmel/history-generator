use rand::Rng;
use rand::RngCore;

use crate::model::{EntityKind, World};

const PREFIXES: &[&str] = &[
    "Iron", "Silver", "Golden", "Shadow", "Storm", "Crimson", "Ashen", "Frost", "Stone", "Dark",
    "Bright", "Ember", "Thorn", "Raven", "Amber", "Azure", "Obsidian", "Jade", "Scarlet", "Ivory",
    "Hollow", "Silent", "Verdant", "Gilded", "Pale", "Elder", "Sunken", "White", "Black", "Grey",
];

const TYPES: &[&str] = &[
    "Covenant", "March", "Kingdom", "Dominion", "League", "Order", "Compact", "Throne", "Banner",
    "Hold", "Concord", "Reach", "Accord", "Circle", "Crown",
];

/// Generate a random faction name: "The {Prefix} {Type}".
pub fn generate_faction_name(rng: &mut dyn RngCore) -> String {
    let prefix = PREFIXES[rng.random_range(0..PREFIXES.len())];
    let kind = TYPES[rng.random_range(0..TYPES.len())];
    format!("The {prefix} {kind}")
}

const SUFFIXES: &[&str] = &[
    "of the North",
    "of the South",
    "of the East",
    "of the West",
    "Reborn",
    "Ascendant",
    "Resurgent",
    "Renewed",
];

/// Generate a faction name unique among living factions.
/// Falls back to adding a suffix after 5 attempts.
pub fn generate_unique_faction_name(world: &World, rng: &mut dyn RngCore) -> String {
    for _ in 0..5 {
        let name = generate_faction_name(rng);
        let is_taken = world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Faction && e.end.is_none() && e.name == name);
        if !is_taken {
            return name;
        }
    }
    let base = generate_faction_name(rng);
    let suffix = SUFFIXES[rng.random_range(0..SUFFIXES.len())];
    format!("{base} {suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn generates_nonempty_name() {
        let mut rng = SmallRng::seed_from_u64(42);
        let name = generate_faction_name(&mut rng);
        assert!(!name.is_empty());
        assert!(
            name.starts_with("The "),
            "faction name should start with 'The ': {name}"
        );
        // Should have 3 words: "The X Y"
        assert_eq!(
            name.split_whitespace().count(),
            3,
            "expected 3 words: {name}"
        );
    }

    #[test]
    fn deterministic() {
        let mut rng1 = SmallRng::seed_from_u64(123);
        let mut rng2 = SmallRng::seed_from_u64(123);
        assert_eq!(
            generate_faction_name(&mut rng1),
            generate_faction_name(&mut rng2)
        );
    }
}
