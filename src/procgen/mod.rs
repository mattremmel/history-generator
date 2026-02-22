pub mod artifacts;
pub mod inhabitants;
pub mod seed;
pub mod tables;
pub mod writings;

pub use artifacts::GeneratedArtifact;
pub use inhabitants::{GeneratedPerson, Sex};
pub use writings::{GeneratedWriting, WritingCategory};

use crate::model::{EntityKind, ParticipantRole, RelationshipKind, World};
use crate::sim::PopulationBreakdown;

/// Minimal snapshot of settlement state needed for procedural generation.
/// Can be constructed from a live World or from deserialized checkpoint data.
pub struct SettlementSnapshot {
    pub settlement_id: u64,
    pub name: String,
    pub year: u32,
    pub founded_year: u32,
    pub population: PopulationBreakdown,
    pub resources: Vec<String>,
    pub terrain: Option<String>,
    pub terrain_tags: Vec<String>,
    pub notable_events: Vec<EventSummary>,
}

/// Simplified event summary for writing generation.
#[derive(Debug, Clone)]
pub struct EventSummary {
    pub year: u32,
    pub kind: String,
    pub description: String,
}

/// Configuration for procedural generation.
pub struct ProcGenConfig {
    pub max_inhabitants: usize,
    pub max_artifacts: usize,
    pub max_writings: usize,
    pub inhabitant_sample_rate: f64,
}

impl Default for ProcGenConfig {
    fn default() -> Self {
        Self {
            max_inhabitants: 200,
            max_artifacts: 50,
            max_writings: 20,
            inhabitant_sample_rate: 0.05,
        }
    }
}

/// All generated content for a settlement at a given point in time.
pub struct SettlementDetails {
    pub inhabitants: Vec<GeneratedPerson>,
    pub artifacts: Vec<GeneratedArtifact>,
    pub writings: Vec<GeneratedWriting>,
}

/// Construct a SettlementSnapshot from a live World.
pub fn snapshot_from_world(
    world: &World,
    settlement_id: u64,
    year: u32,
) -> Option<SettlementSnapshot> {
    let entity = world.entities.get(&settlement_id)?;
    if entity.kind != EntityKind::Settlement {
        return None;
    }

    let population = entity
        .properties
        .get("population")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let breakdown = entity
        .properties
        .get("population_breakdown")
        .and_then(|v| serde_json::from_value::<PopulationBreakdown>(v.clone()).ok())
        .unwrap_or_else(|| PopulationBreakdown::from_total(population));

    let resources: Vec<String> = entity
        .properties
        .get("resources")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let founded_year = entity
        .origin
        .map(|t| t.year())
        .unwrap_or(0);

    // Follow LocatedIn to find region for terrain/tags
    let region_id = entity
        .relationships
        .iter()
        .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
        .map(|r| r.target_entity_id);

    let (terrain, terrain_tags) = region_id
        .and_then(|rid| world.entities.get(&rid))
        .map(|region| {
            let terrain = region
                .properties
                .get("terrain")
                .and_then(|v| v.as_str())
                .map(String::from);
            let tags: Vec<String> = region
                .properties
                .get("terrain_tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            (terrain, tags)
        })
        .unwrap_or((None, vec![]));

    // Collect events where this settlement participated
    let settlement_event_ids: Vec<u64> = world
        .event_participants
        .iter()
        .filter(|ep| {
            ep.entity_id == settlement_id
                && matches!(ep.role, ParticipantRole::Location | ParticipantRole::Subject)
        })
        .map(|ep| ep.event_id)
        .collect();

    let notable_events: Vec<EventSummary> = settlement_event_ids
        .iter()
        .filter_map(|eid| world.events.get(eid))
        .filter(|e| e.timestamp.year() <= year)
        .map(|e| EventSummary {
            year: e.timestamp.year(),
            kind: String::from(e.kind.clone()),
            description: e.description.clone(),
        })
        .collect();

    Some(SettlementSnapshot {
        settlement_id,
        name: entity.name.clone(),
        year,
        founded_year,
        population: breakdown,
        resources,
        terrain,
        terrain_tags,
        notable_events,
    })
}

/// Generate all settlement details at once.
pub fn generate_settlement_details(
    snapshot: &SettlementSnapshot,
    config: &ProcGenConfig,
) -> SettlementDetails {
    let inhabitants_result = inhabitants::generate_inhabitants(snapshot, config);
    let inhabitant_count = inhabitants_result.len() as u64;
    let artifacts_result = artifacts::generate_artifacts(snapshot, config, inhabitant_count);
    let artifact_count = artifacts_result.len() as u64;
    let writings_result =
        writings::generate_writings(snapshot, config, inhabitant_count + artifact_count);

    SettlementDetails {
        inhabitants: inhabitants_result,
        artifacts: artifacts_result,
        writings: writings_result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn generate_all_produces_content() {
        let snapshot = test_snapshot();
        let config = ProcGenConfig::default();
        let details = generate_settlement_details(&snapshot, &config);
        assert!(!details.inhabitants.is_empty());
        assert!(!details.artifacts.is_empty());
        assert!(!details.writings.is_empty());
    }

    #[test]
    fn no_id_collisions() {
        let snapshot = test_snapshot();
        let config = ProcGenConfig::default();
        let details = generate_settlement_details(&snapshot, &config);

        let mut all_ids: Vec<u64> = Vec::new();
        all_ids.extend(details.inhabitants.iter().map(|p| p.id));
        all_ids.extend(details.artifacts.iter().map(|a| a.id));
        all_ids.extend(details.writings.iter().map(|w| w.id));

        let unique_count = {
            let mut sorted = all_ids.clone();
            sorted.sort();
            sorted.dedup();
            sorted.len()
        };
        assert_eq!(
            all_ids.len(),
            unique_count,
            "all generated IDs should be unique"
        );
    }

    #[test]
    fn default_config_values() {
        let config = ProcGenConfig::default();
        assert_eq!(config.max_inhabitants, 200);
        assert_eq!(config.max_artifacts, 50);
        assert_eq!(config.max_writings, 20);
        assert!((config.inhabitant_sample_rate - 0.05).abs() < f64::EPSILON);
    }
}
