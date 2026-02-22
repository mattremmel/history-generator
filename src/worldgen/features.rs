use rand::Rng;
use rand::RngCore;

use crate::model::{EntityKind, EventKind, RelationshipKind, SimTimestamp, World};

use super::config::WorldGenConfig;
use super::terrain::Terrain;

/// Generate geographic features (caves, passes, harbors, etc.) in regions.
pub fn generate_features(world: &mut World, _config: &WorldGenConfig, rng: &mut dyn RngCore) {
    let genesis_event = world.add_event(
        EventKind::Custom("world_genesis".to_string()),
        SimTimestamp::from_year(0),
        "Geographic features form across the land".to_string(),
    );

    // Collect region info
    let regions: Vec<(u64, Terrain, f64, f64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .map(|e| {
            let terrain_str = e.properties["terrain"].as_str().unwrap().to_string();
            let terrain = Terrain::try_from(terrain_str).expect("invalid terrain");
            let x = e.properties["x"].as_f64().unwrap();
            let y = e.properties["y"].as_f64().unwrap();
            (e.id, terrain, x, y)
        })
        .collect();

    // Check which land regions are adjacent to water (for harbors)
    let water_ids: std::collections::HashSet<u64> = world
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
        .map(|e| e.id)
        .collect();

    let coastal_regions: std::collections::HashSet<u64> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Region
                && !water_ids.contains(&e.id)
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::AdjacentTo
                        && water_ids.contains(&r.target_entity_id)
                })
        })
        .map(|e| e.id)
        .collect();

    for (region_id, terrain, rx, ry) in &regions {
        let feature_types = possible_features(*terrain, coastal_regions.contains(region_id));
        if feature_types.is_empty() {
            continue;
        }

        // Roll 0–2 features per region
        let num_features = match rng.random_range(0..10) {
            0..=5 => 0, // 60% no features
            6..=8 => 1, // 30% one feature
            _ => 2,     // 10% two features
        };

        let mut used_types: Vec<&str> = Vec::new();
        for _ in 0..num_features {
            // Pick a random feature type not already used in this region
            let available: Vec<&&str> = feature_types
                .iter()
                .filter(|t| !used_types.contains(t))
                .collect();
            if available.is_empty() {
                break;
            }

            let feature_type = available[rng.random_range(0..available.len())];
            used_types.push(feature_type);

            let name = generate_feature_name(feature_type, rng);
            let jitter_x = rng.random_range(-20.0..20.0);
            let jitter_y = rng.random_range(-20.0..20.0);
            let fx = rx + jitter_x;
            let fy = ry + jitter_y;

            let feature_id = world.add_entity(
                EntityKind::GeographicFeature,
                name,
                Some(SimTimestamp::from_year(0)),
                genesis_event,
            );
            world.set_property(
                feature_id,
                "feature_type".to_string(),
                serde_json::json!(feature_type),
                genesis_event,
            );
            world.set_property(
                feature_id,
                "x".to_string(),
                serde_json::json!(fx),
                genesis_event,
            );
            world.set_property(
                feature_id,
                "y".to_string(),
                serde_json::json!(fy),
                genesis_event,
            );

            world.add_relationship(
                feature_id,
                *region_id,
                RelationshipKind::LocatedIn,
                SimTimestamp::from_year(0),
                genesis_event,
            );
        }
    }
}

fn possible_features(terrain: Terrain, is_coastal: bool) -> Vec<&'static str> {
    let mut features = match terrain {
        Terrain::Mountains => vec!["cave", "mountain_pass"],
        Terrain::Forest | Terrain::Jungle => vec!["clearing", "grove"],
        Terrain::Swamp => vec!["sinkhole"],
        Terrain::Volcanic => vec!["hot_spring", "lava_tube"],
        _ => vec![],
    };

    if is_coastal {
        features.push("harbor");
    }

    features
}

fn generate_feature_name(feature_type: &str, rng: &mut dyn RngCore) -> String {
    let (adjectives, nouns) = match feature_type {
        "cave" => (
            &["Dark", "Crystal", "Echo", "Shadow"][..],
            &["Cave", "Cavern", "Grotto"][..],
        ),
        "mountain_pass" => (
            &["High", "Narrow", "Wind", "Storm"][..],
            &["Pass", "Gap", "Col"][..],
        ),
        "clearing" => (
            &["Sunlit", "Hidden", "Moonlit", "Sacred"][..],
            &["Clearing", "Glade", "Meadow"][..],
        ),
        "grove" => (
            &["Ancient", "Silver", "Elder", "Spirit"][..],
            &["Grove", "Copse", "Stand"][..],
        ),
        "sinkhole" => (
            &["Black", "Deep", "Murky", "Lost"][..],
            &["Sinkhole", "Pit", "Maw"][..],
        ),
        "hot_spring" => (
            &["Boiling", "Steaming", "Ember", "Fire"][..],
            &["Spring", "Pool", "Wells"][..],
        ),
        "lava_tube" => (
            &["Obsidian", "Molten", "Infernal", "Dragon"][..],
            &["Tube", "Tunnel", "Passage"][..],
        ),
        "harbor" => (
            &["Calm", "Sheltered", "Deep", "Storm"][..],
            &["Harbor", "Cove", "Anchorage"][..],
        ),
        _ => (&["The"][..], &["Feature"][..]),
    };

    let adj = adjectives[rng.random_range(0..adjectives.len())];
    let noun = nouns[rng.random_range(0..nouns.len())];
    format!("{adj} {noun}")
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
            num_regions: 25,
            water_fraction: 0.2,
            ..WorldGenConfig::default()
        };
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);
        (world, config)
    }

    #[test]
    fn features_have_located_in() {
        let (mut world, config) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 3);
        generate_features(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::GeographicFeature)
        {
            let located_in = entity
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::LocatedIn)
                .count();
            assert_eq!(
                located_in, 1,
                "feature '{}' should have exactly 1 LocatedIn, got {}",
                entity.name, located_in
            );
        }
    }

    #[test]
    fn features_have_required_properties() {
        let (mut world, config) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 3);
        generate_features(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::GeographicFeature)
        {
            assert!(
                entity.properties.contains_key("feature_type"),
                "feature '{}' missing feature_type",
                entity.name
            );
            assert!(
                entity.properties.contains_key("x"),
                "feature '{}' missing x",
                entity.name
            );
            assert!(
                entity.properties.contains_key("y"),
                "feature '{}' missing y",
                entity.name
            );
        }
    }
}
