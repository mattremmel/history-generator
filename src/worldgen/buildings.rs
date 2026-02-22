use rand::RngCore;

use crate::model::{EntityKind, EventKind, RelationshipKind, SimTimestamp, World};

use super::config::WorldGenConfig;

/// Mining resources that can have mines built on them.
const MINING_RESOURCES: &[&str] = &[
    "iron", "stone", "copper", "gold", "gems", "obsidian", "sulfur", "clay", "ore",
];

/// Generate buildings (mines, ports) linked to deposits and features.
pub fn generate_buildings(world: &mut World, _config: &WorldGenConfig, _rng: &mut dyn RngCore) {
    let genesis_event = world.add_event(
        EventKind::Custom("world_genesis".to_string()),
        SimTimestamp::from_year(0),
        "Buildings rise across the world".to_string(),
    );

    // Collect settlements and their regions
    let settlements: Vec<(u64, u64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement)
        .map(|e| {
            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn)
                .expect("settlement should have LocatedIn")
                .target_entity_id;
            (e.id, region_id)
        })
        .collect();

    // Collect deposits by region: (deposit_id, resource_type, discovered, x, y)
    let deposits: Vec<(u64, u64, String, bool, f64, f64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::ResourceDeposit)
        .map(|e| {
            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn)
                .expect("deposit should have LocatedIn")
                .target_entity_id;
            let resource_type = e.properties["resource_type"].as_str().unwrap().to_string();
            let discovered = e.properties["discovered"].as_bool().unwrap_or(false);
            let x = e.properties["x"].as_f64().unwrap();
            let y = e.properties["y"].as_f64().unwrap();
            (e.id, region_id, resource_type, discovered, x, y)
        })
        .collect();

    // Collect harbor features by region
    let harbors: Vec<(u64, u64, f64, f64)> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::GeographicFeature
                && e.properties.get("feature_type").and_then(|v| v.as_str()) == Some("harbor")
        })
        .map(|e| {
            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn)
                .expect("feature should have LocatedIn")
                .target_entity_id;
            let x = e.properties["x"].as_f64().unwrap();
            let y = e.properties["y"].as_f64().unwrap();
            (e.id, region_id, x, y)
        })
        .collect();

    // For each settlement, create mines for discovered mining deposits in its region
    for &(_, region_id) in &settlements {
        for (deposit_id, dep_region, resource_type, discovered, dx, dy) in &deposits {
            if *dep_region != region_id || !discovered {
                continue;
            }
            if !MINING_RESOURCES.contains(&resource_type.as_str()) {
                continue;
            }

            let name = format!("{} Mine", capitalize(resource_type));
            let building_id = world.add_entity(
                EntityKind::Building,
                name,
                Some(SimTimestamp::from_year(0)),
                genesis_event,
            );
            world.set_property(
                building_id,
                "building_type".to_string(),
                serde_json::json!("mine"),
                genesis_event,
            );
            world.set_property(
                building_id,
                "output_resource".to_string(),
                serde_json::json!(resource_type),
                genesis_event,
            );
            world.set_property(
                building_id,
                "x".to_string(),
                serde_json::json!(dx),
                genesis_event,
            );
            world.set_property(
                building_id,
                "y".to_string(),
                serde_json::json!(dy),
                genesis_event,
            );

            // Exploits → deposit
            world.add_relationship(
                building_id,
                *deposit_id,
                RelationshipKind::Exploits,
                SimTimestamp::from_year(0),
                genesis_event,
            );

            // LocatedIn → region
            world.add_relationship(
                building_id,
                region_id,
                RelationshipKind::LocatedIn,
                SimTimestamp::from_year(0),
                genesis_event,
            );
        }

        // Create port for settlements in regions with harbors
        for (_, harbor_region, hx, hy) in &harbors {
            if *harbor_region != region_id {
                continue;
            }

            let building_id = world.add_entity(
                EntityKind::Building,
                "Port".to_string(),
                Some(SimTimestamp::from_year(0)),
                genesis_event,
            );
            world.set_property(
                building_id,
                "building_type".to_string(),
                serde_json::json!("port"),
                genesis_event,
            );
            world.set_property(
                building_id,
                "x".to_string(),
                serde_json::json!(hx),
                genesis_event,
            );
            world.set_property(
                building_id,
                "y".to_string(),
                serde_json::json!(hy),
                genesis_event,
            );

            world.add_relationship(
                building_id,
                region_id,
                RelationshipKind::LocatedIn,
                SimTimestamp::from_year(0),
                genesis_event,
            );
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    use crate::model::World;
    use crate::worldgen::config::WorldGenConfig;
    use crate::worldgen::deposits::generate_deposits;
    use crate::worldgen::features::generate_features;
    use crate::worldgen::geography::generate_regions;
    use crate::worldgen::settlements::generate_settlements;

    fn make_full_world() -> (World, WorldGenConfig) {
        let config = WorldGenConfig {
            seed: 12345,
            num_regions: 25,
            ..WorldGenConfig::default()
        };
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);
        generate_settlements(&mut world, config.map_width, config.map_height, &mut rng);
        generate_features(&mut world, &config, &mut rng);
        generate_deposits(&mut world, &config, &mut rng);
        (world, config)
    }

    #[test]
    fn mines_have_exploits_relationship() {
        let (mut world, config) = make_full_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 5);
        generate_buildings(&mut world, &config, &mut rng);

        let mines: Vec<_> = world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Building
                    && e.properties.get("building_type").and_then(|v| v.as_str()) == Some("mine")
            })
            .collect();

        for mine in &mines {
            let exploits = mine
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Exploits)
                .count();
            assert_eq!(
                exploits, 1,
                "mine '{}' should have exactly 1 Exploits relationship",
                mine.name
            );
        }
    }

    #[test]
    fn buildings_have_located_in() {
        let (mut world, config) = make_full_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 5);
        generate_buildings(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Building)
        {
            let located_in = entity
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::LocatedIn)
                .count();
            assert_eq!(
                located_in, 1,
                "building '{}' should have exactly 1 LocatedIn",
                entity.name
            );
        }
    }
}
