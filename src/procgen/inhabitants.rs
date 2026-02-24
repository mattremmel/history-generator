use rand::Rng;

use crate::model::population::{BRACKET_LABELS, BRACKET_WIDTHS, NUM_BRACKETS};
use crate::sim::names::generate_person_name;

use super::seed::{PROCGEN_ID_BASE, make_rng};
use super::tables::select_occupation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
}

#[derive(Debug, Clone)]
pub struct GeneratedPerson {
    pub id: u64,
    pub name: String,
    pub age: u32,
    pub sex: Sex,
    pub occupation: &'static str,
    pub bracket_label: &'static str,
}

/// Cumulative base age for each bracket (start of the bracket's age range).
fn bracket_base_ages() -> [u32; NUM_BRACKETS] {
    let mut bases = [0u32; NUM_BRACKETS];
    for i in 1..NUM_BRACKETS {
        bases[i] = bases[i - 1] + BRACKET_WIDTHS[i - 1];
    }
    bases
}

/// Max age within a bracket.
fn bracket_max_age(bracket: usize) -> u32 {
    let bases = bracket_base_ages();
    if bracket == NUM_BRACKETS - 1 {
        // Centenarians: cap at 110
        110
    } else {
        bases[bracket] + BRACKET_WIDTHS[bracket] - 1
    }
}

pub fn generate_inhabitants(
    snapshot: &super::SettlementSnapshot,
    config: &super::ProcGenConfig,
) -> Vec<GeneratedPerson> {
    let total = snapshot.population.total();
    if total == 0 {
        return Vec::new();
    }

    let target = ((total as f64 * config.inhabitant_sample_rate).ceil() as usize)
        .max(1)
        .min(config.max_inhabitants);

    let mut rng = make_rng(snapshot.settlement_id, snapshot.year, "inhabitants");
    let bases = bracket_base_ages();
    let mut people = Vec::with_capacity(target);
    let mut id_counter = 0u64;

    // Distribute target across brackets proportionally
    let mut slots: Vec<(usize, Sex, u32)> = Vec::new(); // (bracket, sex, count)
    let mut assigned = 0usize;

    for bracket in 0..NUM_BRACKETS {
        for (sex, counts) in [
            (Sex::Male, snapshot.population.male[bracket]),
            (Sex::Female, snapshot.population.female[bracket]),
        ] {
            let proportion = counts as f64 / total as f64;
            let slot_count = if assigned < target {
                let raw = (target as f64 * proportion).round() as usize;
                raw.min(target - assigned)
            } else {
                0
            };
            assigned += slot_count;
            if slot_count > 0 {
                slots.push((bracket, sex, slot_count as u32));
            }
        }
    }

    // If rounding left us short, add remainder to largest bracket
    if assigned < target && !slots.is_empty() {
        let remaining = target - assigned;
        slots[0].2 += remaining as u32;
    }

    for (bracket, sex, count) in slots {
        let min_age = bases[bracket];
        let max_age = bracket_max_age(bracket);

        for _ in 0..count {
            let age = rng.random_range(min_age..=max_age);
            let name = generate_person_name(&mut rng);

            let occupation = if bracket <= 1 {
                "child"
            } else if bracket >= 5 {
                if rng.random_bool(0.3) {
                    select_occupation(&snapshot.resources, &mut rng)
                } else {
                    "elder"
                }
            } else {
                select_occupation(&snapshot.resources, &mut rng)
            };

            people.push(GeneratedPerson {
                id: PROCGEN_ID_BASE + id_counter,
                name,
                age,
                sex: sex.clone(),
                occupation,
                bracket_label: BRACKET_LABELS[bracket],
            });
            id_counter += 1;
        }
    }

    people
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PopulationBreakdown;
    use crate::procgen::ProcGenConfig;
    use crate::procgen::SettlementSnapshot;

    fn test_snapshot(population: u32) -> SettlementSnapshot {
        SettlementSnapshot {
            settlement_id: 42,
            name: "Testhold".to_string(),
            year: 500,
            founded_year: 0,
            population: PopulationBreakdown::from_total(population),
            resources: vec!["iron".to_string(), "timber".to_string()],
            terrain: Some("plains".to_string()),
            terrain_tags: vec![],
            notable_events: vec![],
        }
    }

    #[test]
    fn deterministic() {
        let snapshot = test_snapshot(500);
        let config = ProcGenConfig::default();
        let a = generate_inhabitants(&snapshot, &config);
        let b = generate_inhabitants(&snapshot, &config);
        assert_eq!(a.len(), b.len());
        for (pa, pb) in a.iter().zip(b.iter()) {
            assert_eq!(pa.name, pb.name);
            assert_eq!(pa.age, pb.age);
            assert_eq!(pa.sex, pb.sex);
            assert_eq!(pa.occupation, pb.occupation);
        }
    }

    #[test]
    fn empty_population_empty_result() {
        let snapshot = test_snapshot(0);
        let config = ProcGenConfig::default();
        let result = generate_inhabitants(&snapshot, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn respects_max_cap() {
        let snapshot = test_snapshot(10_000);
        let config = ProcGenConfig {
            max_inhabitants: 10,
            ..ProcGenConfig::default()
        };
        let result = generate_inhabitants(&snapshot, &config);
        assert!(result.len() <= 10);
    }

    #[test]
    fn ages_within_bracket_range() {
        let snapshot = test_snapshot(1000);
        let config = ProcGenConfig::default();
        let bases = bracket_base_ages();
        let result = generate_inhabitants(&snapshot, &config);

        for person in &result {
            let bracket_idx = BRACKET_LABELS
                .iter()
                .position(|&l| l == person.bracket_label)
                .unwrap();
            let min_age = bases[bracket_idx];
            let max_age = bracket_max_age(bracket_idx);
            assert!(
                person.age >= min_age && person.age <= max_age,
                "person age {} not in bracket {} range [{}, {}]",
                person.age,
                person.bracket_label,
                min_age,
                max_age
            );
        }
    }

    #[test]
    fn ids_in_procgen_range() {
        let snapshot = test_snapshot(200);
        let config = ProcGenConfig::default();
        let result = generate_inhabitants(&snapshot, &config);
        for person in &result {
            assert!(person.id >= PROCGEN_ID_BASE);
        }
    }

    #[test]
    fn children_have_child_occupation() {
        let snapshot = test_snapshot(1000);
        let config = ProcGenConfig::default();
        let result = generate_inhabitants(&snapshot, &config);
        for person in &result {
            if person.bracket_label == "infant" || person.bracket_label == "child" {
                assert_eq!(person.occupation, "child");
            }
        }
    }
}
