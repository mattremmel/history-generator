use rand::Rng;
use rand::RngCore;

use crate::model::{EntityKind, World};

pub(crate) const FIRST_PREFIXES: &[&str] = &[
    "Al", "Ar", "Bal", "Bel", "Bor", "Cal", "Cor", "Dar", "Del", "Dor", "El", "Er", "Fal", "Fen",
    "Gar", "Gil", "Hal", "Ith", "Kal", "Kel", "Lor", "Mal", "Mar", "Mor", "Nar", "Nor", "Or",
    "Pel", "Ral", "Ren", "Sal", "Sel", "Tar", "Tel", "Thal", "Tor", "Val", "Var", "Zan", "Zor",
];

pub(crate) const FIRST_SUFFIXES: &[&str] = &[
    "an", "ar", "as", "en", "er", "ia", "id", "il", "in", "ion", "is", "na", "on", "or", "ra",
    "ren", "ric", "rin", "us", "wen",
];

const SURNAMES: &[&str] = &[
    "Ashford",
    "Blackthorn",
    "Brightwater",
    "Coldwell",
    "Dunmere",
    "Fairwind",
    "Greymoor",
    "Hartwood",
    "Ironhand",
    "Kingsward",
    "Longbridge",
    "Mossbank",
    "Northgate",
    "Oakshield",
    "Pinehurst",
    "Ravencrest",
    "Silverleaf",
    "Stonemark",
    "Thornwall",
    "Whitevale",
];

/// Generate a random person name (first + surname).
pub fn generate_person_name(rng: &mut dyn RngCore) -> String {
    let prefix = FIRST_PREFIXES[rng.random_range(0..FIRST_PREFIXES.len())];
    let suffix = FIRST_SUFFIXES[rng.random_range(0..FIRST_SUFFIXES.len())];
    let surname = SURNAMES[rng.random_range(0..SURNAMES.len())];
    format!("{prefix}{suffix} {surname}")
}

pub(crate) const EPITHETS: &[&str] = &[
    "Elder", "Younger", "Bold", "Wise", "Fair", "Brave", "Stern", "Swift", "Tall", "Silent",
    "Fierce", "Gentle", "Dark", "Bright", "Grim",
];

/// Generate a person name that is unique among living persons in the world.
/// Falls back to adding an epithet after 5 attempts.
pub fn generate_unique_person_name(world: &World, rng: &mut dyn RngCore) -> String {
    for _ in 0..5 {
        let name = generate_person_name(rng);
        let is_taken = world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Person && e.end.is_none() && e.name == name);
        if !is_taken {
            return name;
        }
    }
    let base = generate_person_name(rng);
    let epithet = EPITHETS[rng.random_range(0..EPITHETS.len())];
    format!("{base} the {epithet}")
}

/// Extract the surname from a full name.
/// Handles "Aldric Ashford" → "Ashford" and "Aldric Ashford the Bold" → "Ashford".
pub fn extract_surname(name: &str) -> Option<&str> {
    let base = name.split(" the ").next().unwrap_or(name);
    base.rsplit_once(' ').map(|(_, surname)| surname)
}

/// Generate a random first name combined with the given surname.
/// Falls back to adding an epithet if the name collides with a living person.
pub fn generate_person_name_with_surname(
    world: &World,
    rng: &mut dyn RngCore,
    surname: &str,
) -> String {
    for _ in 0..5 {
        let prefix = FIRST_PREFIXES[rng.random_range(0..FIRST_PREFIXES.len())];
        let suffix = FIRST_SUFFIXES[rng.random_range(0..FIRST_SUFFIXES.len())];
        let name = format!("{prefix}{suffix} {surname}");
        let is_taken = world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Person && e.end.is_none() && e.name == name);
        if !is_taken {
            return name;
        }
    }
    let prefix = FIRST_PREFIXES[rng.random_range(0..FIRST_PREFIXES.len())];
    let suffix = FIRST_SUFFIXES[rng.random_range(0..FIRST_SUFFIXES.len())];
    let epithet = EPITHETS[rng.random_range(0..EPITHETS.len())];
    format!("{prefix}{suffix} {surname} the {epithet}")
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
        assert!(
            name.contains(' '),
            "name should have first and last: {name}"
        );
    }

    #[test]
    fn deterministic() {
        let mut rng1 = SmallRng::seed_from_u64(123);
        let mut rng2 = SmallRng::seed_from_u64(123);
        assert_eq!(
            generate_person_name(&mut rng1),
            generate_person_name(&mut rng2)
        );
    }

    #[test]
    fn extract_surname_simple() {
        assert_eq!(extract_surname("Aldric Ashford"), Some("Ashford"));
    }

    #[test]
    fn extract_surname_with_epithet() {
        assert_eq!(extract_surname("Aldric Ashford the Bold"), Some("Ashford"));
    }

    #[test]
    fn extract_surname_single_word() {
        assert_eq!(extract_surname("Aldric"), None);
    }

    #[test]
    fn generate_name_with_surname_uses_given_surname() {
        let world = World::new();
        let mut rng = SmallRng::seed_from_u64(42);
        let name = generate_person_name_with_surname(&world, &mut rng, "Ashford");
        assert!(
            name.contains("Ashford"),
            "name should contain surname: {name}"
        );
    }
}
