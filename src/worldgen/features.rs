use rand::Rng;
use rand::RngCore;

use crate::model::{
    EntityData, EntityKind, FeatureType, GeographicFeatureData, RelationshipKind, SimTimestamp,
    World,
};

use super::terrain::Terrain;
use crate::worldgen::config::WorldGenConfig;

/// Generate geographic features (caves, passes, harbors, etc.) in regions.
pub fn generate_features(
    world: &mut World,
    _config: &WorldGenConfig,
    rng: &mut dyn RngCore,
    genesis_event: u64,
) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Region),
        "features step requires regions to exist"
    );

    // Collect region info
    let regions: Vec<(u64, Terrain, f64, f64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .map(|e| {
            let region = e.data.as_region().unwrap();
            (e.id, region.terrain, region.x, region.y)
        })
        .collect();

    // Check which land regions are adjacent to water (for harbors)
    let water_ids: std::collections::HashSet<u64> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Region
                && e.data
                    .as_region()
                    .map(|r| r.terrain == Terrain::ShallowWater || r.terrain == Terrain::DeepWater)
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
                && e.active_rels(RelationshipKind::AdjacentTo)
                    .any(|id| water_ids.contains(&id))
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

        let mut used_types: Vec<FeatureType> = Vec::new();
        for _ in 0..num_features {
            // Pick a random feature type not already used in this region
            let available: Vec<&FeatureType> = feature_types
                .iter()
                .filter(|t| !used_types.contains(t))
                .collect();
            if available.is_empty() {
                break;
            }

            let feature_type = available[rng.random_range(0..available.len())].clone();
            used_types.push(feature_type.clone());

            let name = generate_feature_name(&feature_type, rng);
            let jitter_x = rng.random_range(-20.0..20.0);
            let jitter_y = rng.random_range(-20.0..20.0);
            let fx = rx + jitter_x;
            let fy = ry + jitter_y;

            let feature_id = world.add_entity(
                EntityKind::GeographicFeature,
                name,
                Some(SimTimestamp::from_year(0)),
                EntityData::GeographicFeature(GeographicFeatureData {
                    feature_type,
                    x: fx,
                    y: fy,
                }),
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

fn possible_features(terrain: Terrain, is_coastal: bool) -> Vec<FeatureType> {
    let mut features = match terrain {
        Terrain::Mountains => vec![FeatureType::Cave, FeatureType::MountainPass],
        Terrain::Forest | Terrain::Jungle => vec![FeatureType::Clearing, FeatureType::Grove],
        Terrain::Swamp => vec![FeatureType::Sinkhole],
        Terrain::Volcanic => vec![FeatureType::HotSpring, FeatureType::LavaTube],
        _ => vec![],
    };

    if is_coastal {
        features.push(FeatureType::Harbor);
    }

    features
}

fn generate_feature_name(feature_type: &FeatureType, rng: &mut dyn RngCore) -> String {
    let (adjectives, nouns) = match feature_type {
        FeatureType::Cave => (
            &["Dark", "Crystal", "Echo", "Shadow"][..],
            &["Cave", "Cavern", "Grotto"][..],
        ),
        FeatureType::MountainPass => (
            &["High", "Narrow", "Wind", "Storm"][..],
            &["Pass", "Gap", "Col"][..],
        ),
        FeatureType::Clearing => (
            &["Sunlit", "Hidden", "Moonlit", "Sacred"][..],
            &["Clearing", "Glade", "Meadow"][..],
        ),
        FeatureType::Grove => (
            &["Ancient", "Silver", "Elder", "Spirit"][..],
            &["Grove", "Copse", "Stand"][..],
        ),
        FeatureType::Sinkhole => (
            &["Black", "Deep", "Murky", "Lost"][..],
            &["Sinkhole", "Pit", "Maw"][..],
        ),
        FeatureType::HotSpring => (
            &["Boiling", "Steaming", "Ember", "Fire"][..],
            &["Spring", "Pool", "Wells"][..],
        ),
        FeatureType::LavaTube => (
            &["Obsidian", "Molten", "Infernal", "Dragon"][..],
            &["Tube", "Tunnel", "Passage"][..],
        ),
        FeatureType::Harbor => (
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

    use crate::model::{EventKind, SimTimestamp, World};
    use crate::worldgen::config::WorldGenConfig;
    use crate::worldgen::geography::generate_regions;

    fn genesis_event(world: &mut World) -> u64 {
        world.add_event(
            EventKind::Custom("world_genesis".to_string()),
            SimTimestamp::from_year(0),
            "test genesis".to_string(),
        )
    }

    fn make_world() -> (World, WorldGenConfig, u64) {
        use crate::worldgen::config::{MapConfig, TerrainConfig};
        let config = WorldGenConfig {
            seed: 12345,
            map: MapConfig {
                num_regions: 25,
                ..MapConfig::default()
            },
            terrain: TerrainConfig {
                water_fraction: 0.2,
            },
            ..WorldGenConfig::default()
        };
        let mut world = World::new();
        let ev = genesis_event(&mut world);
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng, ev);
        (world, config, ev)
    }

    #[test]
    fn features_have_located_in() {
        let (mut world, config, ev) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 3);
        generate_features(&mut world, &config, &mut rng, ev);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::GeographicFeature)
        {
            let located_in = entity.active_rels(RelationshipKind::LocatedIn).count();
            assert_eq!(
                located_in, 1,
                "feature '{}' should have exactly 1 LocatedIn, got {}",
                entity.name, located_in
            );
        }
    }

    #[test]
    fn features_have_required_properties() {
        let (mut world, config, ev) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 3);
        generate_features(&mut world, &config, &mut rng, ev);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::GeographicFeature)
        {
            let feature = entity.data.as_geographic_feature().unwrap_or_else(|| {
                panic!(
                    "feature '{}' should have GeographicFeatureData",
                    entity.name
                )
            });
            assert!(
                !feature.feature_type.as_str().is_empty(),
                "feature '{}' missing feature_type",
                entity.name
            );
            // x and y are always present in the typed struct
        }
    }
}
