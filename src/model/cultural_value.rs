use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum CulturalValue {
    Martial,
    Mercantile,
    Scholarly,
    Agrarian,
    Spiritual,
    Artistic,
    Seafaring,
    Isolationist,
    Custom(String),
}

string_enum_open!(CulturalValue, "cultural value", {
    Martial => "martial",
    Mercantile => "mercantile",
    Scholarly => "scholarly",
    Agrarian => "agrarian",
    Spiritual => "spiritual",
    Artistic => "artistic",
    Seafaring => "seafaring",
    Isolationist => "isolationist",
});

/// Opposing pairs: a culture cannot hold both values in a pair.
pub const OPPOSING_VALUE_PAIRS: [(CulturalValue, CulturalValue); 3] = [
    (CulturalValue::Martial, CulturalValue::Scholarly),
    (CulturalValue::Mercantile, CulturalValue::Isolationist),
    (CulturalValue::Seafaring, CulturalValue::Agrarian),
];

const ALL_VALUES: [CulturalValue; 8] = [
    CulturalValue::Martial,
    CulturalValue::Mercantile,
    CulturalValue::Scholarly,
    CulturalValue::Agrarian,
    CulturalValue::Spiritual,
    CulturalValue::Artistic,
    CulturalValue::Seafaring,
    CulturalValue::Isolationist,
];

fn opposite_value(v: &CulturalValue) -> Option<&'static CulturalValue> {
    for (a, b) in &OPPOSING_VALUE_PAIRS {
        if v == a {
            return Some(b);
        }
        if v == b {
            return Some(a);
        }
    }
    None
}

/// Generate `count` cultural values with no opposing pairs.
pub fn generate_cultural_values(rng: &mut dyn RngCore, count: usize) -> Vec<CulturalValue> {
    let mut chosen: Vec<CulturalValue> = Vec::with_capacity(count);

    for _ in 0..count {
        let mut candidates: Vec<&CulturalValue> = Vec::new();
        for v in &ALL_VALUES {
            if chosen.contains(v) {
                continue;
            }
            if let Some(opp) = opposite_value(v)
                && chosen.contains(opp)
            {
                continue;
            }
            candidates.push(v);
        }
        if candidates.is_empty() {
            break;
        }
        let idx = (rng.next_u32() as usize) % candidates.len();
        chosen.push(candidates[idx].clone());
    }

    chosen
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum NamingStyle {
    Nordic,
    Elvish,
    Desert,
    Steppe,
    Imperial,
    Sylvan,
    Custom(String),
}

impl NamingStyle {
    /// The 6 core naming styles, in a fixed order for deterministic cycling.
    pub const ALL: [NamingStyle; 6] = [
        NamingStyle::Nordic,
        NamingStyle::Elvish,
        NamingStyle::Desert,
        NamingStyle::Steppe,
        NamingStyle::Imperial,
        NamingStyle::Sylvan,
    ];
}

string_enum_open!(NamingStyle, "naming style", {
    Nordic => "nordic",
    Elvish => "elvish",
    Desert => "desert",
    Steppe => "steppe",
    Imperial => "imperial",
    Sylvan => "sylvan",
});

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn cultural_value_round_trip() {
        for v in &ALL_VALUES {
            let s: String = v.clone().into();
            let back = CulturalValue::try_from(s).unwrap();
            assert_eq!(&back, v);
        }
    }

    #[test]
    fn custom_cultural_value_round_trip() {
        let v = CulturalValue::Custom("nomadic".to_string());
        let s: String = v.clone().into();
        assert_eq!(s, "nomadic");
        let back = CulturalValue::try_from(s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn empty_cultural_value_fails() {
        assert!(CulturalValue::try_from(String::new()).is_err());
    }

    #[test]
    fn naming_style_round_trip() {
        for s in &NamingStyle::ALL {
            let string: String = s.clone().into();
            let back = NamingStyle::try_from(string).unwrap();
            assert_eq!(&back, s);
        }
    }

    #[test]
    fn custom_naming_style_round_trip() {
        let s = NamingStyle::Custom("dwarven".to_string());
        let string: String = s.clone().into();
        assert_eq!(string, "dwarven");
        let back = NamingStyle::try_from(string).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn empty_naming_style_fails() {
        assert!(NamingStyle::try_from(String::new()).is_err());
    }

    #[test]
    fn generate_values_respects_count() {
        let mut rng = SmallRng::seed_from_u64(42);
        for count in 1..=5 {
            let values = generate_cultural_values(&mut rng, count);
            assert!(values.len() <= count);
            assert!(!values.is_empty());
        }
    }

    #[test]
    fn generate_values_no_opposing_pairs() {
        let mut rng = SmallRng::seed_from_u64(99);
        for _ in 0..200 {
            let values = generate_cultural_values(&mut rng, 4);
            for (a, b) in &OPPOSING_VALUE_PAIRS {
                assert!(
                    !(values.contains(a) && values.contains(b)),
                    "opposing pair found: {a:?} and {b:?} in {values:?}"
                );
            }
        }
    }

    #[test]
    fn generate_values_no_duplicates() {
        let mut rng = SmallRng::seed_from_u64(123);
        for _ in 0..200 {
            let values = generate_cultural_values(&mut rng, 4);
            let unique: std::collections::HashSet<_> = values.iter().collect();
            assert_eq!(unique.len(), values.len(), "duplicate in {values:?}");
        }
    }
}
