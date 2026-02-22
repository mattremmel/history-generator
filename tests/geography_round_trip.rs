mod common;

use history_gen::flush::flush_to_jsonl;
use history_gen::model::*;
use history_gen::worldgen::{WorldGenConfig, generate_world};

#[test]
fn generate_world_deterministic() {
    let config = WorldGenConfig {
        seed: 99,
        map: history_gen::worldgen::config::MapConfig {
            num_regions: 20,
            ..Default::default()
        },
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
        region_count, config.map.num_regions as usize,
        "should have {} regions",
        config.map.num_regions
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
fn water_regions_exist() {
    let config = WorldGenConfig {
        map: history_gen::worldgen::config::MapConfig {
            num_regions: 30,
            ..Default::default()
        },
        terrain: history_gen::worldgen::config::TerrainConfig {
            water_fraction: 0.3,
        },
        ..WorldGenConfig::default()
    };
    let world = generate_world(&config);

    let water_count = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Region
                && e.properties
                    .get("terrain")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "shallow_water" || s == "deep_water")
                    .unwrap_or(false)
        })
        .count();

    assert!(
        water_count > 0,
        "should have at least one water region with water_fraction=0.3"
    );
}

#[test]
fn terrain_tags_present_on_regions() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    for entity in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
    {
        assert!(
            entity.properties.contains_key("terrain_tags"),
            "region '{}' should have terrain_tags property",
            entity.name
        );
        assert!(
            entity.properties["terrain_tags"].is_array(),
            "terrain_tags should be an array"
        );
    }
}

#[test]
fn river_entities_have_flows_through() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    let rivers: Vec<_> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::River)
        .collect();

    assert!(!rivers.is_empty(), "should have at least one river");

    for river in &rivers {
        let flows = river
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::FlowsThrough)
            .count();
        assert!(
            flows >= 2,
            "river '{}' should flow through at least 2 regions",
            river.name
        );
        assert!(
            river.properties.contains_key("region_path"),
            "river '{}' should have region_path",
            river.name
        );
    }
}

#[test]
fn deposits_have_required_properties() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    let deposits: Vec<_> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::ResourceDeposit)
        .collect();

    assert!(!deposits.is_empty(), "should have at least one deposit");

    for deposit in &deposits {
        assert!(deposit.properties.contains_key("resource_type"));
        assert!(deposit.properties.contains_key("quantity"));
        assert!(deposit.properties.contains_key("quality"));
        assert!(deposit.properties.contains_key("discovered"));

        let located_in = deposit
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::LocatedIn)
            .count();
        assert_eq!(located_in, 1, "deposit should have exactly 1 LocatedIn");
    }
}

#[test]
fn buildings_exploit_deposits() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    let mines: Vec<_> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Building
                && e.properties.get("building_type").and_then(|v| v.as_str()) == Some("mine")
        })
        .collect();

    // Mines may not exist in every seed, but if they do, they should exploit a deposit
    for mine in &mines {
        let exploits = mine
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Exploits)
            .count();
        assert_eq!(
            exploits, 1,
            "mine '{}' should exploit exactly 1 deposit",
            mine.name
        );

        let located_in = mine
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::LocatedIn)
            .count();
        assert_eq!(
            located_in, 1,
            "mine '{}' should have exactly 1 LocatedIn",
            mine.name
        );
    }
}

#[test]
fn every_located_entity_has_exactly_one_located_in() {
    let config = WorldGenConfig::default();
    let world = generate_world(&config);

    let locatable_kinds = [
        EntityKind::Settlement,
        EntityKind::GeographicFeature,
        EntityKind::ResourceDeposit,
        EntityKind::Building,
    ];

    for entity in world.entities.values() {
        if !locatable_kinds.contains(&entity.kind) {
            continue;
        }

        let located_in = entity
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::LocatedIn)
            .count();
        assert_eq!(
            located_in, 1,
            "{:?} '{}' should have exactly 1 LocatedIn, got {}",
            entity.kind, entity.name, located_in
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

    // Helper to check kind exists
    let has_kind = |lines: &[String], kind: &str| -> bool {
        lines.iter().any(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["kind"] == kind
        })
    };

    // Entity kinds
    assert!(has_kind(&entities_lines, "region"));
    assert!(has_kind(&entities_lines, "settlement"));
    assert!(has_kind(&entities_lines, "river"));
    assert!(has_kind(&entities_lines, "resource_deposit"));

    // Relationship kinds
    assert!(has_kind(&rels_lines, "adjacent_to"));
    assert!(has_kind(&rels_lines, "located_in"));
    assert!(has_kind(&rels_lines, "flows_through"));

    // Region entities should have terrain and terrain_tags
    let region_line = entities_lines
        .iter()
        .find(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["kind"] == "region"
        })
        .unwrap();
    let region: serde_json::Value = serde_json::from_str(region_line).unwrap();
    assert!(region["properties"]["terrain"].is_string());
    assert!(region["properties"]["x"].is_number());
    assert!(region["properties"]["y"].is_number());
    assert!(region["properties"]["terrain_tags"].is_array());

    // Settlement entities should have population
    let settlement_line = entities_lines
        .iter()
        .find(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["kind"] == "settlement"
        })
        .unwrap();
    let settlement: serde_json::Value = serde_json::from_str(settlement_line).unwrap();
    assert!(settlement["properties"]["population"].is_number());

    // River entities should have region_path
    let river_line = entities_lines
        .iter()
        .find(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["kind"] == "river"
        })
        .unwrap();
    let river: serde_json::Value = serde_json::from_str(river_line).unwrap();
    assert!(river["properties"]["region_path"].is_array());

    // Deposit entities should have resource_type
    let deposit_line = entities_lines
        .iter()
        .find(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            v["kind"] == "resource_deposit"
        })
        .unwrap();
    let deposit: serde_json::Value = serde_json::from_str(deposit_line).unwrap();
    assert!(deposit["properties"]["resource_type"].is_string());
    assert!(deposit["properties"]["quantity"].is_number());
}
