mod common;

use history_gen::flush::flush_to_jsonl;
use history_gen::model::*;
use history_gen::worldgen::{WorldGenConfig, generate_world};

#[test]
fn generate_world_deterministic() {
    let config = WorldGenConfig {
        seed: 99,
        num_regions: 20,
        ..WorldGenConfig::default()
    };

    let world1 = generate_world(&config);
    let world2 = generate_world(&config);

    let names1: Vec<&str> = world1.entities.values().map(|e| e.name.as_str()).collect();
    let names2: Vec<&str> = world2.entities.values().map(|e| e.name.as_str()).collect();
    assert_eq!(names1, names2, "same seed should produce same world");
}

#[test]
fn generated_world_has_regions_and_settlements() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    let region_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .count();
    let settlement_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement)
        .count();

    assert_eq!(
        region_count, config.num_regions as usize,
        "should have {} regions",
        config.num_regions
    );
    assert!(settlement_count > 0, "should have at least one settlement");
}

#[test]
fn all_regions_reachable_via_adjacency() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    let region_ids: Vec<u64> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .map(|e| e.id)
        .collect();

    // BFS from first region
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(region_ids[0]);
    visited.insert(region_ids[0]);

    while let Some(current) = queue.pop_front() {
        let entity = &world.entities[&current];
        for rel in &entity.relationships {
            if rel.kind == RelationshipKind::AdjacentTo && !visited.contains(&rel.target_entity_id)
            {
                visited.insert(rel.target_entity_id);
                queue.push_back(rel.target_entity_id);
            }
        }
    }

    assert_eq!(
        visited.len(),
        region_ids.len(),
        "all regions should be reachable via adjacency"
    );
}

#[test]
fn every_settlement_located_in_exactly_one_region() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    let region_ids: std::collections::HashSet<u64> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .map(|e| e.id)
        .collect();

    for entity in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement)
    {
        let located_in: Vec<&Relationship> = entity
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::LocatedIn)
            .collect();

        assert_eq!(
            located_in.len(),
            1,
            "settlement '{}' should have exactly 1 LocatedIn, got {}",
            entity.name,
            located_in.len()
        );

        assert!(
            region_ids.contains(&located_in[0].target_entity_id),
            "settlement '{}' LocatedIn target should be a region",
            entity.name
        );
    }
}

#[test]
fn flush_round_trip_includes_geography() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    let dir = tempfile::tempdir().unwrap();
    flush_to_jsonl(&world, dir.path()).unwrap();

    let entities_lines = common::read_lines(&dir.path().join("entities.jsonl"));
    let rels_lines = common::read_lines(&dir.path().join("relationships.jsonl"));

    // Should have entities
    assert!(
        !entities_lines.is_empty(),
        "entities file should not be empty"
    );

    // Should have regions
    let has_region = entities_lines.iter().any(|line| {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        v["kind"] == "region"
    });
    assert!(has_region, "should have region entities in JSONL");

    // Should have settlements
    let has_settlement = entities_lines.iter().any(|line| {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        v["kind"] == "settlement"
    });
    assert!(has_settlement, "should have settlement entities in JSONL");

    // Should have adjacent_to relationships
    let has_adjacent = rels_lines.iter().any(|line| {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        v["kind"] == "adjacent_to"
    });
    assert!(
        has_adjacent,
        "should have adjacent_to relationships in JSONL"
    );

    // Should have located_in relationships
    let has_located_in = rels_lines.iter().any(|line| {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        v["kind"] == "located_in"
    });
    assert!(
        has_located_in,
        "should have located_in relationships in JSONL"
    );

    // Region entities should have terrain property
    let region_line = entities_lines
        .iter()
        .find(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["kind"] == "region"
        })
        .unwrap();
    let region: serde_json::Value = serde_json::from_str(region_line).unwrap();
    assert!(
        region["properties"]["terrain"].is_string(),
        "region should have terrain property"
    );
    assert!(
        region["properties"]["x"].is_number(),
        "region should have x coordinate"
    );
    assert!(
        region["properties"]["y"].is_number(),
        "region should have y coordinate"
    );

    // Settlement entities should have population
    let settlement_line = entities_lines
        .iter()
        .find(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["kind"] == "settlement"
        })
        .unwrap();
    let settlement: serde_json::Value = serde_json::from_str(settlement_line).unwrap();
    assert!(
        settlement["properties"]["population"].is_number(),
        "settlement should have population"
    );
}
