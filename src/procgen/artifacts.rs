use rand::Rng;

use super::seed::{PROCGEN_ID_BASE, make_rng};
use super::tables::{ARTIFACT_TYPES, available_materials};

#[derive(Debug, Clone)]
pub struct GeneratedArtifact {
    pub id: u64,
    pub name: String,
    pub artifact_type: &'static str,
    pub material: &'static str,
    pub age_years: u32,
    pub description: String,
}

pub fn generate_artifacts(
    snapshot: &super::SettlementSnapshot,
    config: &super::ProcGenConfig,
    id_offset: u64,
) -> Vec<GeneratedArtifact> {
    let settlement_age = snapshot.year.saturating_sub(snapshot.founded_year);
    let population = snapshot.population.total();
    if settlement_age == 0 || population == 0 {
        return Vec::new();
    }

    let raw_count = (settlement_age as f64).sqrt() * (population as f64 + 1.0).log2();
    let target = (raw_count.ceil() as usize).min(config.max_artifacts);
    if target == 0 {
        return Vec::new();
    }

    let mut rng = make_rng(snapshot.settlement_id, snapshot.year, "artifacts");
    let materials = available_materials(&snapshot.resources);
    let mut artifacts = Vec::with_capacity(target);

    for i in 0..target {
        let artifact_type = ARTIFACT_TYPES[rng.random_range(0..ARTIFACT_TYPES.len())];
        let material = materials[rng.random_range(0..materials.len())];
        let age_years = if settlement_age > 0 {
            rng.random_range(0..=settlement_age)
        } else {
            0
        };

        let age_desc = if age_years > 100 {
            "ancient"
        } else if age_years > 50 {
            "old"
        } else if age_years > 10 {
            "weathered"
        } else {
            "recent"
        };

        let name = format!("{material} {artifact_type}");
        let description = format!(
            "A {age_desc} {material} {artifact_type} from {settlement}, approximately {age_years} years old",
            settlement = snapshot.name,
        );

        artifacts.push(GeneratedArtifact {
            id: PROCGEN_ID_BASE + id_offset + i as u64,
            name,
            artifact_type,
            material,
            age_years,
            description,
        });
    }

    artifacts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PopulationBreakdown;
    use crate::procgen::{ProcGenConfig, SettlementSnapshot};

    fn test_snapshot(age: u32, pop: u32) -> SettlementSnapshot {
        SettlementSnapshot {
            settlement_id: 42,
            name: "Testhold".to_string(),
            year: age,
            founded_year: 0,
            population: PopulationBreakdown::from_total(pop),
            resources: vec!["iron".to_string(), "timber".to_string()],
            terrain: Some("plains".to_string()),
            terrain_tags: vec![],
            notable_events: vec![],
        }
    }

    #[test]
    fn deterministic() {
        let snapshot = test_snapshot(500, 200);
        let config = ProcGenConfig::default();
        let a = generate_artifacts(&snapshot, &config, 0);
        let b = generate_artifacts(&snapshot, &config, 0);
        assert_eq!(a.len(), b.len());
        for (aa, bb) in a.iter().zip(b.iter()) {
            assert_eq!(aa.name, bb.name);
            assert_eq!(aa.age_years, bb.age_years);
        }
    }

    #[test]
    fn empty_when_new() {
        let snapshot = test_snapshot(0, 100);
        let config = ProcGenConfig::default();
        let result = generate_artifacts(&snapshot, &config, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn age_within_settlement_age() {
        let snapshot = test_snapshot(200, 500);
        let config = ProcGenConfig::default();
        let result = generate_artifacts(&snapshot, &config, 0);
        for artifact in &result {
            assert!(
                artifact.age_years <= 200,
                "artifact age {} exceeds settlement age 200",
                artifact.age_years
            );
        }
    }

    #[test]
    fn respects_max_cap() {
        let snapshot = test_snapshot(1000, 5000);
        let config = ProcGenConfig {
            max_artifacts: 5,
            ..ProcGenConfig::default()
        };
        let result = generate_artifacts(&snapshot, &config, 0);
        assert!(result.len() <= 5);
    }

    #[test]
    fn materials_reflect_resources() {
        let snapshot = test_snapshot(100, 200);
        let config = ProcGenConfig::default();
        let result = generate_artifacts(&snapshot, &config, 0);
        let valid_materials = available_materials(&snapshot.resources);
        for artifact in &result {
            assert!(
                valid_materials.contains(&artifact.material),
                "artifact material {} not in available materials",
                artifact.material
            );
        }
    }

    #[test]
    fn ids_in_procgen_range() {
        let snapshot = test_snapshot(100, 200);
        let config = ProcGenConfig::default();
        let result = generate_artifacts(&snapshot, &config, 0);
        for artifact in &result {
            assert!(artifact.id >= PROCGEN_ID_BASE);
        }
    }
}
