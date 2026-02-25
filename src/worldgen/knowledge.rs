use rand::{Rng, RngCore};

use crate::model::{
    DerivationMethod, EntityData, EntityKind, KnowledgeCategory, Medium, RelationshipKind,
    SimTimestamp, World,
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
    genesis_event: u64,
) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Settlement),
        "knowledge step requires settlements to exist"
    );

    // Collect settlements and their info
    struct SettlementInfo {
        id: u64,
        name: String,
        origin: SimTimestamp,
        adjacent: Vec<u64>,
    }

    let settlements: Vec<SettlementInfo> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| {
            // Find adjacent settlements via region adjacency:
            // settlement -> LocatedIn -> region -> AdjacentTo -> regions -> settlements
            let adjacent: Vec<u64> = e
                .active_rel(RelationshipKind::LocatedIn)
                .and_then(|region_id| world.entities.get(&region_id))
                .map(|region| {
                    region
                        .relationships
                        .iter()
                        .filter(|r| r.kind == RelationshipKind::AdjacentTo && r.end.is_none())
                        .flat_map(|r| {
                            world
                                .entities
                                .values()
                                .filter(move |e| {
                                    e.kind == EntityKind::Settlement
                                        && e.end.is_none()
                                        && e.has_active_rel(
                                            RelationshipKind::LocatedIn,
                                            r.target_entity_id,
                                        )
                                })
                                .map(|e| e.id)
                        })
                        .collect()
                })
                .unwrap_or_default();
            SettlementInfo {
                id: e.id,
                name: e.name.clone(),
                origin: e.origin.unwrap_or_default(),
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
            "year": s.origin.year(),
            "founder_name": null
        });

        let knowledge_name = format!("Founding of {}", s.name);
        let mut knowledge_data = EntityData::default_for_kind(EntityKind::Knowledge);
        if let EntityData::Knowledge(ref mut kd) = knowledge_data {
            kd.category = KnowledgeCategory::Founding;
            kd.source_event_id = genesis_event;
            kd.origin_settlement_id = s.id;
            kd.origin_time = s.origin;
            kd.significance = 0.4 + rng.random_range(0.0..0.2);
            kd.ground_truth = truth.clone();
        }
        let kid = world.add_entity(
            EntityKind::Knowledge,
            knowledge_name.clone(),
            Some(s.origin),
            knowledge_data,
            genesis_event,
        );

        // Memory manifestation at origin (accuracy 0.8-0.95, condition adjusted for "age")
        let memory_accuracy = rng.random_range(0.8..0.95);
        let memory_condition = rng.random_range(0.3..0.7); // old memories are partially faded
        let mut mem_data = EntityData::default_for_kind(EntityKind::Manifestation);
        if let EntityData::Manifestation(ref mut md) = mem_data {
            md.knowledge_id = kid;
            md.medium = Medium::Memory;
            md.content = truth.clone();
            md.accuracy = memory_accuracy;
            md.completeness = rng.random_range(0.6..0.9);
            md.derivation_method = DerivationMethod::Witnessed;
            md.condition = memory_condition;
            md.created = s.origin;
        }
        let mem_id = world.add_entity(
            EntityKind::Manifestation,
            format!("{knowledge_name} (memory)"),
            Some(s.origin),
            mem_data,
            genesis_event,
        );
        world.add_relationship(
            mem_id,
            s.id,
            RelationshipKind::HeldBy,
            s.origin,
            genesis_event,
        );

        // OralTradition manifestation at origin (accuracy 0.6-0.8)
        let oral_accuracy = rng.random_range(0.6..0.8);
        let mut oral_data = EntityData::default_for_kind(EntityKind::Manifestation);
        if let EntityData::Manifestation(ref mut md) = oral_data {
            md.knowledge_id = kid;
            md.medium = Medium::OralTradition;
            md.content = truth.clone();
            md.accuracy = oral_accuracy;
            md.completeness = rng.random_range(0.5..0.8);
            md.distortions = vec![serde_json::json!({"type": "oral_drift"})];
            md.derived_from_id = Some(mem_id);
            md.derivation_method = DerivationMethod::Retold;
            md.condition = rng.random_range(0.5..0.9);
            md.created = s.origin;
        }
        let oral_id = world.add_entity(
            EntityKind::Manifestation,
            format!("{knowledge_name} (oral tradition)"),
            Some(s.origin),
            oral_data,
            genesis_event,
        );
        world.add_relationship(
            oral_id,
            s.id,
            RelationshipKind::HeldBy,
            s.origin,
            genesis_event,
        );

        // Spread OralTradition to adjacent settlements (accuracy 0.4-0.6)
        for &adj_id in &s.adjacent {
            if rng.random_bool(0.5) {
                let spread_accuracy = rng.random_range(0.4..0.6);
                let mut spread_data = EntityData::default_for_kind(EntityKind::Manifestation);
                if let EntityData::Manifestation(ref mut md) = spread_data {
                    md.knowledge_id = kid;
                    md.medium = Medium::OralTradition;
                    md.content = truth.clone();
                    md.accuracy = spread_accuracy;
                    md.completeness = rng.random_range(0.3..0.6);
                    md.distortions = vec![
                        serde_json::json!({"type": "oral_drift"}),
                        serde_json::json!({"type": "distance_degradation"}),
                    ];
                    md.derived_from_id = Some(oral_id);
                    md.derivation_method = DerivationMethod::Retold;
                    md.condition = rng.random_range(0.4..0.8);
                    md.created = s.origin;
                }
                let spread_id = world.add_entity(
                    EntityKind::Manifestation,
                    format!("{knowledge_name} (distant oral tradition)"),
                    Some(s.origin),
                    spread_data,
                    genesis_event,
                );
                world.add_relationship(
                    spread_id,
                    adj_id,
                    RelationshipKind::HeldBy,
                    s.origin,
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

    fn make_world_with_settlements() -> (World, u64) {
        let config = WorldGenConfig::default();
        crate::worldgen::make_test_world(
            &config,
            &[
                crate::worldgen::geography::generate_regions,
                crate::worldgen::settlements::generate_settlements,
            ],
        )
    }

    #[test]
    fn founding_knowledge_created() {
        let (mut world, ev) = make_world_with_settlements();
        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_knowledge(&mut world, &config, &mut rng, ev);

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
        let (mut world, ev) = make_world_with_settlements();
        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_knowledge(&mut world, &config, &mut rng, ev);

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
        let (mut world, ev) = make_world_with_settlements();
        let config = WorldGenConfig::default();
        let mut rng = SmallRng::seed_from_u64(42);
        generate_knowledge(&mut world, &config, &mut rng, ev);

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
