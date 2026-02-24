use rand::{Rng, RngCore};

use crate::model::{
    EntityData, EntityKind, EventKind, KnowledgeCategory, KnowledgeData, ManifestationData, Medium,
    RelationshipKind, SimTimestamp, World,
};

/// Generate founding knowledge for each settlement.
///
/// Each settlement gets 1-2 Knowledge entities about its founding,
/// with Memory and OralTradition manifestations. Oral traditions
/// spread to adjacent settlements with reduced accuracy.
pub fn generate_knowledge(
    world: &mut World,
    _config: &crate::worldgen::config::WorldGenConfig,
    rng: &mut dyn RngCore,
) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Settlement),
        "knowledge step requires settlements to exist"
    );
    let genesis_event = world.add_event(
        EventKind::Custom("world_genesis_knowledge".to_string()),
        SimTimestamp::from_year(0),
        "Knowledge of the founding age".to_string(),
    );

    // Collect settlements and their info
    struct SettlementInfo {
        id: u64,
        name: String,
        origin_year: u32,
        adjacent: Vec<u64>,
    }

    let settlements: Vec<SettlementInfo> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| {
            let origin_year = e.origin.map(|t| t.year()).unwrap_or(0);
            let adjacent: Vec<u64> = e
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::AdjacentTo && r.end.is_none())
                .map(|r| r.target_entity_id)
                .filter(|id| {
                    world
                        .entities
                        .get(id)
                        .is_some_and(|e| e.kind == EntityKind::Settlement && e.end.is_none())
                })
                .collect();
            SettlementInfo {
                id: e.id,
                name: e.name.clone(),
                origin_year,
                adjacent,
            }
        })
        .collect();

    // For each settlement, create founding knowledge
    for s in &settlements {
        let truth = serde_json::json!({
            "event_type": "founding",
            "settlement_id": s.id,
            "settlement_name": s.name,
            "year": s.origin_year,
            "founder_name": null
        });

        let knowledge_name = format!("Founding of {}", s.name);
        let kid = world.add_entity(
            EntityKind::Knowledge,
            knowledge_name.clone(),
            Some(SimTimestamp::from_year(s.origin_year)),
            EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Founding,
                source_event_id: genesis_event,
                origin_settlement_id: s.id,
                origin_year: s.origin_year,
                significance: 0.4 + rng.random_range(0.0..0.2),
                ground_truth: truth.clone(),
            }),
            genesis_event,
        );

        // Memory manifestation at origin (accuracy 0.8-0.95, condition adjusted for "age")
        let memory_accuracy = rng.random_range(0.8..0.95);
        let memory_condition = rng.random_range(0.3..0.7); // old memories are partially faded
        let mem_id = world.add_entity(
            EntityKind::Manifestation,
            format!("{knowledge_name} (memory)"),
            Some(SimTimestamp::from_year(s.origin_year)),
            EntityData::Manifestation(ManifestationData {
                knowledge_id: kid,
                medium: Medium::Memory,
                content: truth.clone(),
                accuracy: memory_accuracy,
                completeness: rng.random_range(0.6..0.9),
                distortions: serde_json::json!([]),
                derived_from_id: None,
                derivation_method: "witnessed".to_string(),
                condition: memory_condition,
                created_year: s.origin_year,
            }),
            genesis_event,
        );
        world.add_relationship(
            mem_id,
            s.id,
            RelationshipKind::HeldBy,
            SimTimestamp::from_year(s.origin_year),
            genesis_event,
        );

        // OralTradition manifestation at origin (accuracy 0.6-0.8)
        let oral_accuracy = rng.random_range(0.6..0.8);
        let oral_id = world.add_entity(
            EntityKind::Manifestation,
            format!("{knowledge_name} (oral tradition)"),
            Some(SimTimestamp::from_year(s.origin_year)),
            EntityData::Manifestation(ManifestationData {
                knowledge_id: kid,
                medium: Medium::OralTradition,
                content: truth.clone(),
                accuracy: oral_accuracy,
                completeness: rng.random_range(0.5..0.8),
                distortions: serde_json::json!([{"type": "oral_drift"}]),
                derived_from_id: Some(mem_id),
                derivation_method: "retold".to_string(),
                condition: rng.random_range(0.5..0.9),
                created_year: s.origin_year,
            }),
            genesis_event,
        );
        world.add_relationship(
            oral_id,
            s.id,
            RelationshipKind::HeldBy,
            SimTimestamp::from_year(s.origin_year),
            genesis_event,
        );

        // Spread OralTradition to adjacent settlements (accuracy 0.4-0.6)
        for &adj_id in &s.adjacent {
            if rng.random_bool(0.5) {
                let spread_accuracy = rng.random_range(0.4..0.6);
                let spread_id = world.add_entity(
                    EntityKind::Manifestation,
                    format!("{knowledge_name} (distant oral tradition)"),
                    Some(SimTimestamp::from_year(s.origin_year)),
                    EntityData::Manifestation(ManifestationData {
                        knowledge_id: kid,
                        medium: Medium::OralTradition,
                        content: truth.clone(),
                        accuracy: spread_accuracy,
                        completeness: rng.random_range(0.3..0.6),
                        distortions: serde_json::json!([
                            {"type": "oral_drift"},
                            {"type": "distance_degradation"}
                        ]),
                        derived_from_id: Some(oral_id),
                        derivation_method: "retold".to_string(),
                        condition: rng.random_range(0.4..0.8),
                        created_year: s.origin_year,
                    }),
                    genesis_event,
                );
                world.add_relationship(
                    spread_id,
                    adj_id,
                    RelationshipKind::HeldBy,
                    SimTimestamp::from_year(s.origin_year),
                    genesis_event,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::config::WorldGenConfig;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn make_world_with_settlements() -> World {
        let config = WorldGenConfig::default();
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        crate::worldgen::geography::generate_regions(&mut world, &config, &mut rng);
        crate::worldgen::settlements::generate_settlements(&mut world, &config, &mut rng);
        world
    }

    #[test]
    fn founding_knowledge_created() {
        let mut world = make_world_with_settlements();
        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_knowledge(&mut world, &config, &mut rng);

        let knowledge_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert!(knowledge_count > 0, "should create knowledge entities");

        let manifestation_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Manifestation)
            .count();
        assert!(
            manifestation_count >= knowledge_count * 2,
            "each knowledge should have at least memory + oral tradition"
        );
    }

    #[test]
    fn manifestations_have_held_by() {
        let mut world = make_world_with_settlements();
        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_knowledge(&mut world, &config, &mut rng);

        for e in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Manifestation)
        {
            let has_held_by = e
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none());
            assert!(
                has_held_by,
                "manifestation '{}' should have HeldBy relationship",
                e.name
            );
        }
    }

    #[test]
    fn founding_knowledge_has_correct_category() {
        let mut world = make_world_with_settlements();
        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_knowledge(&mut world, &config, &mut rng);

        for e in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
        {
            let kd = e.data.as_knowledge().expect("should have KnowledgeData");
            assert_eq!(
                kd.category,
                KnowledgeCategory::Founding,
                "worldgen knowledge should be Founding category"
            );
        }
    }
}
