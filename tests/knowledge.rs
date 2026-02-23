use history_gen::model::{BuildingType, EntityKind, RelationshipKind, World};
use history_gen::sim::{
    BuildingSystem, ConflictSystem, DemographicsSystem, EconomySystem, KnowledgeSystem,
    PoliticsSystem, ReputationSystem, SimConfig, SimSystem, run,
};
use history_gen::worldgen::{self, config::WorldGenConfig};

/// Run with KnowledgeSystem in the tick order.
fn generate_and_run(seed: u64, num_years: u32) -> World {
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(BuildingSystem),
        Box::new(EconomySystem),
        Box::new(ConflictSystem),
        Box::new(PoliticsSystem),
        Box::new(ReputationSystem),
        Box::new(KnowledgeSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
    world
}

#[test]
fn knowledge_entities_created_over_time() {
    let world = generate_and_run(42, 200);

    let knowledge_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Knowledge)
        .count();

    let manifestation_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Manifestation)
        .count();

    assert!(
        knowledge_count > 0,
        "should have knowledge entities after 200 years (worldgen creates founding knowledge)"
    );
    assert!(
        manifestation_count > 0,
        "should have manifestation entities after 200 years"
    );
    // Manifestations should outnumber knowledge (each knowledge has at least one)
    assert!(
        manifestation_count >= knowledge_count,
        "manifestations ({manifestation_count}) should be >= knowledge ({knowledge_count})"
    );
}

#[test]
fn manifestations_have_held_by_relationships() {
    let world = generate_and_run(42, 100);

    let living_manifestations: Vec<_> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Manifestation && e.end.is_none())
        .collect();

    if living_manifestations.is_empty() {
        return; // Nothing to check
    }

    let mut found_held_by = false;
    for e in &living_manifestations {
        let has_held_by = e
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none());
        if has_held_by {
            found_held_by = true;
            // Verify the target exists
            let target_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none())
                .unwrap()
                .target_entity_id;
            assert!(
                world.entities.contains_key(&target_id),
                "HeldBy target should exist in world"
            );
        }
    }

    assert!(
        found_held_by,
        "at least some living manifestations should have HeldBy relationships"
    );
}

#[test]
fn derivation_chains_form_valid_trees() {
    let world = generate_and_run(42, 200);

    for e in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Manifestation)
    {
        let md = e
            .data
            .as_manifestation()
            .expect("manifestation should have ManifestationData");

        // Verify knowledge_id points to a real knowledge entity
        assert!(
            world.entities.contains_key(&md.knowledge_id),
            "manifestation '{}' knowledge_id {} should exist",
            e.name,
            md.knowledge_id
        );
        let knowledge = world.entities.get(&md.knowledge_id).unwrap();
        assert_eq!(
            knowledge.kind,
            EntityKind::Knowledge,
            "knowledge_id should point to Knowledge entity"
        );

        // If derived, parent should exist and be a manifestation
        if let Some(parent_id) = md.derived_from_id {
            assert!(
                world.entities.contains_key(&parent_id),
                "derived_from_id {} should exist for '{}'",
                parent_id,
                e.name
            );
            let parent = world.entities.get(&parent_id).unwrap();
            assert_eq!(
                parent.kind,
                EntityKind::Manifestation,
                "derived_from_id should point to Manifestation entity"
            );
        }

        // Accuracy and completeness should be in valid ranges
        assert!(
            md.accuracy >= 0.0 && md.accuracy <= 1.0,
            "accuracy {} out of range for '{}'",
            md.accuracy,
            e.name
        );
        assert!(
            md.completeness >= 0.0 && md.completeness <= 1.0,
            "completeness {} out of range for '{}'",
            md.completeness,
            e.name
        );
        assert!(
            md.condition >= 0.0 && md.condition <= 1.0,
            "condition {} out of range for '{}'",
            md.condition,
            e.name
        );
    }
}

#[test]
fn content_diverges_from_origin() {
    let world = generate_and_run(42, 200);

    // Find manifestations that are derived (have a parent)
    let derived: Vec<_> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Manifestation && e.end.is_none())
        .filter(|e| {
            e.data
                .as_manifestation()
                .is_some_and(|md| md.derived_from_id.is_some())
        })
        .collect();

    if derived.is_empty() {
        // No derivations happened in this short run, that's OK
        return;
    }

    // At least some derived manifestations should have accuracy < 1.0
    let any_degraded = derived.iter().any(|e| {
        e.data
            .as_manifestation()
            .is_some_and(|md| md.accuracy < 1.0)
    });

    assert!(
        any_degraded,
        "derived manifestations should show accuracy degradation"
    );
}

#[test]
fn decay_destroys_manifestations_over_time() {
    let world = generate_and_run(42, 500);

    let destroyed_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Manifestation && e.end.is_some())
        .count();

    // After 500 years, some manifestations should have decayed and been destroyed
    // (especially Memory and Dream mediums with high decay rates)
    assert!(
        destroyed_count > 0,
        "some manifestations should be destroyed after 500 years of decay"
    );
}

#[test]
fn worldgen_creates_founding_knowledge() {
    let config = WorldGenConfig {
        seed: 42,
        ..WorldGenConfig::default()
    };
    let world = worldgen::generate_world(&config);

    let founding_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Knowledge)
        .filter(|e| {
            e.data
                .as_knowledge()
                .is_some_and(|kd| kd.category == history_gen::model::KnowledgeCategory::Founding)
        })
        .count();

    let settlement_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement)
        .count();

    assert!(
        founding_count > 0,
        "worldgen should create founding knowledge"
    );
    assert_eq!(
        founding_count, settlement_count,
        "each settlement should have a founding knowledge entry"
    );
}

#[test]
fn medium_types_in_valid_range() {
    let world = generate_and_run(42, 200);

    for e in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Manifestation)
    {
        let md = e.data.as_manifestation().unwrap();
        // Verify medium is a valid enum value by checking decay rate is non-negative
        assert!(
            md.medium.decay_rate() >= 0.0,
            "medium {:?} should have non-negative decay rate",
            md.medium
        );
    }
}

#[test]
#[ignore]
fn library_buildings_can_be_constructed() {
    // Run long enough for libraries to appear (need temples first)
    let mut any_library = false;
    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 500);

        let library_count = world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Building
                    && e.data
                        .as_building()
                        .is_some_and(|bd| bd.building_type == BuildingType::Library)
            })
            .count();

        if library_count > 0 {
            any_library = true;
            break;
        }
    }

    assert!(
        any_library,
        "at least one library should be constructed across 4 seeds after 500 years"
    );
}

