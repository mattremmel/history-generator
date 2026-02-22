use rand::Rng;
use rand::RngCore;

use crate::model::{EntityKind, EventKind, RelationshipKind, SimTimestamp, World};

use super::config::WorldGenConfig;
use super::terrain::{Terrain, TerrainProfile, TerrainTag};

/// Probability that a resource in a region spawns a deposit entity.
const DEPOSIT_SPAWN_CHANCE: f64 = 0.4;

/// Generate resource deposit entities in regions.
pub fn generate_deposits(world: &mut World, _config: &WorldGenConfig, rng: &mut dyn RngCore) {
    let genesis_event = world.add_event(
        EventKind::Custom("world_genesis".to_string()),
        SimTimestamp::from_year(0),
        "Resources form beneath the earth".to_string(),
    );

    // Collect region info
    let regions: Vec<(u64, TerrainProfile, f64, f64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .map(|e| {
            let terrain_str = e.properties["terrain"].as_str().unwrap().to_string();
            let terrain = Terrain::try_from(terrain_str).expect("invalid terrain");
            let tags: Vec<TerrainTag> = e
                .properties
                .get("terrain_tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            v.as_str()
                                .and_then(|s| TerrainTag::try_from(s.to_string()).ok())
                        })
                        .collect()
                })
                .unwrap_or_default();
            let x = e.properties["x"].as_f64().unwrap();
            let y = e.properties["y"].as_f64().unwrap();
            (e.id, TerrainProfile::new(terrain, tags), x, y)
        })
        .collect();

    for (region_id, profile, rx, ry) in &regions {
        let resources = profile.effective_resources();

        for resource in resources {
            if rng.random_range(0.0..1.0) >= DEPOSIT_SPAWN_CHANCE {
                continue;
            }

            let category = resource_category(resource);
            let (qty_min, qty_max) = category.quantity_range();
            let quantity = rng.random_range(qty_min..=qty_max);
            let quality: f64 = rng.random_range(0.1..=1.0);
            let discovered = rng.random_range(0.0..1.0) < category.discovery_chance();

            let jitter_x = rng.random_range(-15.0..15.0);
            let jitter_y = rng.random_range(-15.0..15.0);

            let name = format!("{} deposit", capitalize(resource));
            let deposit_id = world.add_entity(
                EntityKind::ResourceDeposit,
                name,
                Some(SimTimestamp::from_year(0)),
                genesis_event,
            );
            world.set_property(
                deposit_id,
                "resource_type".to_string(),
                serde_json::json!(resource),
                genesis_event,
            );
            world.set_property(
                deposit_id,
                "quantity".to_string(),
                serde_json::json!(quantity),
                genesis_event,
            );
            world.set_property(
                deposit_id,
                "quality".to_string(),
                serde_json::json!(quality),
                genesis_event,
            );
            world.set_property(
                deposit_id,
                "discovered".to_string(),
                serde_json::json!(discovered),
                genesis_event,
            );
            world.set_property(
                deposit_id,
                "x".to_string(),
                serde_json::json!(rx + jitter_x),
                genesis_event,
            );
            world.set_property(
                deposit_id,
                "y".to_string(),
                serde_json::json!(ry + jitter_y),
                genesis_event,
            );

            world.add_relationship(
                deposit_id,
                *region_id,
                RelationshipKind::LocatedIn,
                SimTimestamp::from_year(0),
                genesis_event,
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ResourceCategory {
    Agricultural,
    Mining,
    Luxury,
}

impl ResourceCategory {
    fn quantity_range(self) -> (u32, u32) {
        match self {
            ResourceCategory::Agricultural => (500, 2000),
            ResourceCategory::Mining => (50, 500),
            ResourceCategory::Luxury => (30, 200),
        }
    }

    fn discovery_chance(self) -> f64 {
        match self {
            ResourceCategory::Agricultural => 1.0,
            ResourceCategory::Mining => 0.70,
            ResourceCategory::Luxury => 0.50,
        }
    }
}

fn resource_category(resource: &str) -> ResourceCategory {
    match resource {
        // Agricultural/timber/game
        "grain" | "timber" | "game" | "horses" | "cattle" | "sheep" | "herbs" | "peat" | "furs"
        | "freshwater" => ResourceCategory::Agricultural,

        // Mining
        "iron" | "stone" | "copper" | "gold" | "gems" | "obsidian" | "sulfur" | "clay"
        | "glass" | "ivory" | "ore" => ResourceCategory::Mining,

        // Luxury
        "salt" | "pearls" | "spices" | "dyes" | "fish" | "whales" => ResourceCategory::Luxury,

        _ => ResourceCategory::Agricultural,
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
    use crate::worldgen::geography::generate_regions;

    fn make_world() -> (World, WorldGenConfig) {
        let config = WorldGenConfig {
            seed: 12345,
            num_regions: 20,
            ..WorldGenConfig::default()
        };
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);
        (world, config)
    }

    #[test]
    fn generates_deposits() {
        let (mut world, config) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 4);
        generate_deposits(&mut world, &config, &mut rng);

        let count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::ResourceDeposit)
            .count();
        assert!(count > 0, "should generate at least one deposit");
    }

    #[test]
    fn deposits_have_required_properties() {
        let (mut world, config) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 4);
        generate_deposits(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::ResourceDeposit)
        {
            assert!(entity.properties.contains_key("resource_type"));
            assert!(entity.properties.contains_key("quantity"));
            assert!(entity.properties.contains_key("quality"));
            assert!(entity.properties.contains_key("discovered"));
            assert!(entity.properties.contains_key("x"));
            assert!(entity.properties.contains_key("y"));

            let quality = entity.properties["quality"].as_f64().unwrap();
            assert!(
                (0.0..=1.0).contains(&quality),
                "quality should be 0.0-1.0, got {}",
                quality
            );
        }
    }

    #[test]
    fn deposits_have_located_in() {
        let (mut world, config) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 4);
        generate_deposits(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::ResourceDeposit)
        {
            let located_in = entity
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::LocatedIn)
                .count();
            assert_eq!(
                located_in, 1,
                "deposit '{}' should have exactly 1 LocatedIn",
                entity.name
            );
        }
    }
}
