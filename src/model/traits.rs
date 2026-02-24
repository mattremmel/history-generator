use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::entity::Entity;
use super::entity_data::Role;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Trait {
    Ambitious,
    Content,
    Aggressive,
    Cautious,
    Charismatic,
    Reclusive,
    Honorable,
    Ruthless,
    Pious,
    Skeptical,
    Cunning,
    Straightforward,
    Custom(String),
}

string_enum_open!(Trait, "trait", {
    Ambitious => "ambitious",
    Content => "content",
    Aggressive => "aggressive",
    Cautious => "cautious",
    Charismatic => "charismatic",
    Reclusive => "reclusive",
    Honorable => "honorable",
    Ruthless => "ruthless",
    Pious => "pious",
    Skeptical => "skeptical",
    Cunning => "cunning",
    Straightforward => "straightforward",
});

/// Opposing pairs: an NPC cannot have both traits in a pair.
pub const OPPOSING_PAIRS: [(Trait, Trait); 6] = [
    (Trait::Ambitious, Trait::Content),
    (Trait::Aggressive, Trait::Cautious),
    (Trait::Charismatic, Trait::Reclusive),
    (Trait::Honorable, Trait::Ruthless),
    (Trait::Pious, Trait::Skeptical),
    (Trait::Cunning, Trait::Straightforward),
];

/// All core traits in order, used for weighted selection.
const ALL_TRAITS: [Trait; 12] = [
    Trait::Ambitious,
    Trait::Content,
    Trait::Aggressive,
    Trait::Cautious,
    Trait::Charismatic,
    Trait::Reclusive,
    Trait::Honorable,
    Trait::Ruthless,
    Trait::Pious,
    Trait::Skeptical,
    Trait::Cunning,
    Trait::Straightforward,
];

fn role_weight(role: &Role, t: &Trait) -> u32 {
    match role {
        Role::Warrior => match t {
            Trait::Aggressive => 4,
            Trait::Ambitious => 3,
            Trait::Honorable => 2,
            Trait::Cautious => 1,
            Trait::Cunning => 1,
            _ => 1,
        },
        Role::Scholar => match t {
            Trait::Cunning => 4,
            Trait::Cautious => 3,
            Trait::Skeptical => 2,
            Trait::Pious => 2,
            _ => 1,
        },
        Role::Elder => match t {
            Trait::Pious => 4,
            Trait::Honorable => 3,
            Trait::Content => 2,
            Trait::Cautious => 2,
            _ => 1,
        },
        Role::Merchant => match t {
            Trait::Cunning => 3,
            Trait::Charismatic => 3,
            Trait::Ambitious => 2,
            Trait::Ruthless => 2,
            _ => 1,
        },
        Role::Artisan => match t {
            Trait::Content => 3,
            Trait::Straightforward => 2,
            Trait::Pious => 2,
            _ => 1,
        },
        // Common and custom roles: uniform
        _ => 1,
    }
}

fn opposite_of(t: &Trait) -> Option<&'static Trait> {
    for (a, b) in &OPPOSING_PAIRS {
        if t == a {
            return Some(b);
        }
        if t == b {
            return Some(a);
        }
    }
    None
}

/// Generate 2-4 traits for an NPC based on role, respecting opposing constraints.
pub fn generate_traits(role: &Role, rng: &mut dyn RngCore) -> Vec<Trait> {
    // Decide count: 2 (50%), 3 (35%), 4 (15%)
    let roll: u32 = rng.next_u32() % 100;
    let count = if roll < 50 {
        2
    } else if roll < 85 {
        3
    } else {
        4
    };

    let mut chosen: Vec<Trait> = Vec::with_capacity(count);

    for _ in 0..count {
        // Build candidate weights excluding already-chosen and their opposites
        let mut candidates: Vec<(&Trait, u32)> = Vec::new();
        for t in &ALL_TRAITS {
            if chosen.contains(t) {
                continue;
            }
            if let Some(opp) = opposite_of(t)
                && chosen.contains(opp)
            {
                continue;
            }
            candidates.push((t, role_weight(role, t)));
        }
        if candidates.is_empty() {
            break;
        }

        let total: u32 = candidates.iter().map(|(_, w)| w).sum();
        let mut roll = rng.next_u32() % total;
        let mut picked_idx = candidates.len() - 1;
        for (i, (_, w)) in candidates.iter().enumerate() {
            if roll < *w {
                picked_idx = i;
                break;
            }
            roll -= w;
        }
        chosen.push(candidates[picked_idx].0.clone());
    }

    chosen
}

/// Read an NPC's traits from its typed PersonData.
pub fn get_npc_traits(entity: &Entity) -> Vec<Trait> {
    entity
        .data
        .as_person()
        .map(|p| p.traits.clone())
        .unwrap_or_default()
}

/// Check if an entity has a specific trait.
pub fn has_trait(entity: &Entity, t: &Trait) -> bool {
    entity
        .data
        .as_person()
        .map(|p| p.traits.contains(t))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;
    use std::collections::HashMap;

    fn make_person_with_traits(traits: &[Trait]) -> Entity {
        use crate::model::entity_data::{EntityData, PersonData, Role, Sex};
        Entity {
            id: 1,
            kind: crate::model::entity::EntityKind::Person,
            name: "Test".to_string(),
            origin: None,
            end: None,
            data: EntityData::Person(PersonData {
                birth_year: 0,
                sex: Sex::Male,
                role: Role::Common,
                traits: traits.to_vec(),
                last_action_year: 0,
                culture_id: None,
                prestige: 0.0,
            }),
            extra: HashMap::new(),
            relationships: vec![],
        }
    }

    #[test]
    fn trait_string_round_trip() {
        for t in &ALL_TRAITS {
            let s: String = t.clone().into();
            let back = Trait::try_from(s).unwrap();
            assert_eq!(&back, t);
        }
    }

    #[test]
    fn custom_trait_round_trip() {
        let t = Trait::Custom("berserker".to_string());
        let s: String = t.clone().into();
        assert_eq!(s, "berserker");
        let back = Trait::try_from(s).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn empty_string_fails() {
        assert!(Trait::try_from(String::new()).is_err());
    }

    #[test]
    fn generate_respects_count_range() {
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..100 {
            let traits = generate_traits(&Role::Common, &mut rng);
            assert!(
                traits.len() >= 2 && traits.len() <= 4,
                "got {}",
                traits.len()
            );
        }
    }

    #[test]
    fn generate_no_opposing_pairs() {
        let mut rng = SmallRng::seed_from_u64(99);
        for _ in 0..200 {
            let traits = generate_traits(&Role::Warrior, &mut rng);
            for (a, b) in &OPPOSING_PAIRS {
                assert!(
                    !(traits.contains(a) && traits.contains(b)),
                    "opposing pair found: {a:?} and {b:?} in {traits:?}"
                );
            }
        }
    }

    #[test]
    fn generate_no_duplicates() {
        let mut rng = SmallRng::seed_from_u64(123);
        for _ in 0..200 {
            let traits = generate_traits(&Role::Scholar, &mut rng);
            let unique: std::collections::HashSet<_> = traits.iter().collect();
            assert_eq!(unique.len(), traits.len(), "duplicate in {traits:?}");
        }
    }

    #[test]
    fn warrior_skews_aggressive() {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut counts: HashMap<String, u32> = HashMap::new();
        for _ in 0..500 {
            let traits = generate_traits(&Role::Warrior, &mut rng);
            for t in traits {
                *counts.entry(String::from(t)).or_default() += 1;
            }
        }
        // Aggressive should appear more than content for warriors
        let aggressive = counts.get("aggressive").copied().unwrap_or(0);
        let content = counts.get("content").copied().unwrap_or(0);
        assert!(
            aggressive > content,
            "aggressive={aggressive} should exceed content={content} for warriors"
        );
    }

    #[test]
    fn scholar_skews_cunning() {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut counts: HashMap<String, u32> = HashMap::new();
        for _ in 0..500 {
            let traits = generate_traits(&Role::Scholar, &mut rng);
            for t in traits {
                *counts.entry(String::from(t)).or_default() += 1;
            }
        }
        let cunning = counts.get("cunning").copied().unwrap_or(0);
        let straightforward = counts.get("straightforward").copied().unwrap_or(0);
        assert!(
            cunning > straightforward,
            "cunning={cunning} should exceed straightforward={straightforward} for scholars"
        );
    }

    #[test]
    fn get_npc_traits_reads_properties() {
        let entity = make_person_with_traits(&[Trait::Ambitious, Trait::Cunning]);
        let traits = get_npc_traits(&entity);
        assert_eq!(traits, vec![Trait::Ambitious, Trait::Cunning]);
    }

    #[test]
    fn has_trait_works() {
        let entity = make_person_with_traits(&[Trait::Aggressive, Trait::Pious]);
        assert!(has_trait(&entity, &Trait::Aggressive));
        assert!(has_trait(&entity, &Trait::Pious));
        assert!(!has_trait(&entity, &Trait::Cunning));
    }

    #[test]
    fn get_npc_traits_empty_when_no_person_data() {
        use crate::model::entity_data::EntityData;
        let entity = Entity {
            id: 1,
            kind: crate::model::entity::EntityKind::Faction,
            name: "Test".to_string(),
            origin: None,
            end: None,
            data: EntityData::None,
            extra: HashMap::new(),
            relationships: vec![],
        };
        assert!(get_npc_traits(&entity).is_empty());
    }
}
