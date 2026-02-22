use rand::Rng;

use crate::sim::names::generate_person_name;

use super::seed::{PROCGEN_ID_BASE, make_rng};
use super::tables::{
    PROCLAMATION_TEMPLATES, TOMBSTONE_TEMPLATES, TRADE_RECORD_TEMPLATES, select_occupation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingCategory {
    Tombstone,
    TradeRecord,
    Proclamation,
}

#[derive(Debug, Clone)]
pub struct GeneratedWriting {
    pub id: u64,
    pub category: WritingCategory,
    pub text: String,
    pub year_written: u32,
}

pub fn generate_writings(
    snapshot: &super::SettlementSnapshot,
    config: &super::ProcGenConfig,
    id_offset: u64,
) -> Vec<GeneratedWriting> {
    let settlement_age = snapshot.year.saturating_sub(snapshot.founded_year);
    let population = snapshot.population.total();
    if settlement_age == 0 || population == 0 {
        return Vec::new();
    }

    let mut rng = make_rng(snapshot.settlement_id, snapshot.year, "writings");

    // Determine counts per category
    let tombstone_count = ((population as f64 * 0.01).ceil() as usize + settlement_age as usize / 20)
        .min(config.max_writings / 2)
        .max(1);
    let trade_count = if snapshot.resources.is_empty() {
        0
    } else {
        (snapshot.resources.len()).min(config.max_writings / 4).max(1)
    };
    let proclamation_count = (settlement_age as usize / 50).max(1).min(config.max_writings / 4);

    let total_target = (tombstone_count + trade_count + proclamation_count).min(config.max_writings);

    let mut writings = Vec::with_capacity(total_target);
    let mut id_counter = 0u64;

    // Tombstones
    let actual_tombstones = tombstone_count.min(total_target);
    for _ in 0..actual_tombstones {
        let template = TOMBSTONE_TEMPLATES[rng.random_range(0..TOMBSTONE_TEMPLATES.len())];
        let name = generate_person_name(&mut rng);
        let occupation = select_occupation(&snapshot.resources, &mut rng);
        let age = rng.random_range(20..=85);
        let year_written = snapshot.founded_year + rng.random_range(0..=settlement_age);

        let text = template
            .replace("{name}", &name)
            .replace("{occupation}", occupation)
            .replace("{settlement}", &snapshot.name)
            .replace("{age}", &age.to_string())
            .replace("{year}", &year_written.to_string());

        writings.push(GeneratedWriting {
            id: PROCGEN_ID_BASE + id_offset + id_counter,
            category: WritingCategory::Tombstone,
            text,
            year_written,
        });
        id_counter += 1;
    }

    // Trade records
    let remaining = total_target.saturating_sub(writings.len());
    let actual_trade = trade_count.min(remaining);
    for _ in 0..actual_trade {
        let template = TRADE_RECORD_TEMPLATES[rng.random_range(0..TRADE_RECORD_TEMPLATES.len())];
        let resource = if snapshot.resources.is_empty() {
            "goods"
        } else {
            &snapshot.resources[rng.random_range(0..snapshot.resources.len())]
        };
        let name = generate_person_name(&mut rng);
        let quantity = rng.random_range(10..=500);
        let years = rng.random_range(1..=10);
        let year_written = snapshot.founded_year + rng.random_range(0..=settlement_age);

        let text = template
            .replace("{name}", &name)
            .replace("{settlement}", &snapshot.name)
            .replace("{resource}", resource)
            .replace("{quantity}", &quantity.to_string())
            .replace("{years}", &years.to_string())
            .replace("{year}", &year_written.to_string());

        writings.push(GeneratedWriting {
            id: PROCGEN_ID_BASE + id_offset + id_counter,
            category: WritingCategory::TradeRecord,
            text,
            year_written,
        });
        id_counter += 1;
    }

    // Proclamations
    let remaining = total_target.saturating_sub(writings.len());
    let actual_proclamations = proclamation_count.min(remaining);
    for _ in 0..actual_proclamations {
        let template =
            PROCLAMATION_TEMPLATES[rng.random_range(0..PROCLAMATION_TEMPLATES.len())];
        let occupation = select_occupation(&snapshot.resources, &mut rng);
        let terrain = snapshot.terrain.as_deref().unwrap_or("lands");
        let resource = if snapshot.resources.is_empty() {
            "harvest"
        } else {
            &snapshot.resources[rng.random_range(0..snapshot.resources.len())]
        };
        let year_written = snapshot.founded_year + rng.random_range(0..=settlement_age);

        let text = template
            .replace("{settlement}", &snapshot.name)
            .replace("{occupation}", occupation)
            .replace("{terrain}", terrain)
            .replace("{resource}", resource)
            .replace("{year}", &year_written.to_string());

        writings.push(GeneratedWriting {
            id: PROCGEN_ID_BASE + id_offset + id_counter,
            category: WritingCategory::Proclamation,
            text,
            year_written,
        });
        id_counter += 1;
    }

    writings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procgen::{ProcGenConfig, SettlementSnapshot};
    use crate::sim::PopulationBreakdown;

    fn test_snapshot() -> SettlementSnapshot {
        SettlementSnapshot {
            settlement_id: 42,
            name: "Testhold".to_string(),
            year: 500,
            founded_year: 0,
            population: PopulationBreakdown::from_total(500),
            resources: vec!["iron".to_string(), "grain".to_string()],
            terrain: Some("plains".to_string()),
            terrain_tags: vec![],
            notable_events: vec![],
        }
    }

    #[test]
    fn deterministic() {
        let snapshot = test_snapshot();
        let config = ProcGenConfig::default();
        let a = generate_writings(&snapshot, &config, 0);
        let b = generate_writings(&snapshot, &config, 0);
        assert_eq!(a.len(), b.len());
        for (wa, wb) in a.iter().zip(b.iter()) {
            assert_eq!(wa.text, wb.text);
            assert_eq!(wa.category, wb.category);
            assert_eq!(wa.year_written, wb.year_written);
        }
    }

    #[test]
    fn no_raw_placeholders() {
        let snapshot = test_snapshot();
        let config = ProcGenConfig::default();
        let result = generate_writings(&snapshot, &config, 0);
        for writing in &result {
            assert!(
                !writing.text.contains('{'),
                "writing contains raw placeholder: {}",
                writing.text
            );
        }
    }

    #[test]
    fn respects_max_cap() {
        let snapshot = test_snapshot();
        let config = ProcGenConfig {
            max_writings: 3,
            ..ProcGenConfig::default()
        };
        let result = generate_writings(&snapshot, &config, 0);
        assert!(result.len() <= 3);
    }

    #[test]
    fn empty_when_new() {
        let snapshot = SettlementSnapshot {
            year: 0,
            founded_year: 0,
            ..test_snapshot()
        };
        let config = ProcGenConfig::default();
        let result = generate_writings(&snapshot, &config, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn has_multiple_categories() {
        let snapshot = test_snapshot();
        let config = ProcGenConfig {
            max_writings: 20,
            ..ProcGenConfig::default()
        };
        let result = generate_writings(&snapshot, &config, 0);
        let has_tombstone = result.iter().any(|w| w.category == WritingCategory::Tombstone);
        let has_trade = result.iter().any(|w| w.category == WritingCategory::TradeRecord);
        let has_proclamation = result.iter().any(|w| w.category == WritingCategory::Proclamation);
        assert!(has_tombstone, "should have tombstones");
        assert!(has_trade, "should have trade records");
        assert!(has_proclamation, "should have proclamations");
    }

    #[test]
    fn ids_in_procgen_range() {
        let snapshot = test_snapshot();
        let config = ProcGenConfig::default();
        let result = generate_writings(&snapshot, &config, 0);
        for writing in &result {
            assert!(writing.id >= PROCGEN_ID_BASE);
        }
    }
}
