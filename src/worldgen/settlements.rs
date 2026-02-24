use rand::Rng;
use rand::RngCore;
use rand::seq::SliceRandom;

use crate::model::PopulationBreakdown;
use crate::model::{
    EntityData, EntityKind, EventKind, RelationshipKind, SettlementData, SimTimestamp, World,
};

use super::terrain::{Terrain, TerrainProfile};
use crate::worldgen::config::WorldGenConfig;

/// Coordinate jitter range (fraction of map size) for settlement placement.
const JITTER_FRACTION: f64 = 0.03;

/// Generate settlements in regions based on terrain probability.
pub fn generate_settlements(world: &mut World, config: &WorldGenConfig, rng: &mut dyn RngCore) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Region),
        "settlements step requires regions to exist"
    );
    let map_width = config.map.width;
    let map_height = config.map.height;
    let founding_event = world.add_event(
        EventKind::Custom("world_genesis".to_string()),
        SimTimestamp::from_year(0),
        "Settlements emerge across the world".to_string(),
    );

    // Collect region info before mutating world
    struct RegionInfo {
        id: u64,
        profile: TerrainProfile,
        x: f64,
        y: f64,
        resources: Vec<String>,
    }
    let regions: Vec<RegionInfo> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .map(|e| {
            let region = e
                .data
                .as_region()
                .expect("region entity missing RegionData");
            RegionInfo {
                id: e.id,
                profile: TerrainProfile::new(region.terrain, region.terrain_tags.clone()),
                x: region.x,
                y: region.y,
                resources: region.resources.clone(),
            }
        })
        .collect();

    for region in &regions {
        let profile = &region.profile;

        // Roll against settlement probability
        if rng.random_range(0.0..1.0) >= profile.effective_settlement_probability() {
            continue;
        }

        // Population from terrain-based range
        let (pop_min, pop_max) = profile.effective_population_range();
        if pop_max == 0 {
            continue;
        }
        let population = rng.random_range(pop_min..=pop_max);

        // Coordinates near region center with jitter
        let jitter_x = map_width * JITTER_FRACTION;
        let jitter_y = map_height * JITTER_FRACTION;
        let sx = (region.x + rng.random_range(-jitter_x..jitter_x)).clamp(0.0, map_width);
        let sy = (region.y + rng.random_range(-jitter_y..jitter_y)).clamp(0.0, map_height);

        // Assign a subset of region resources (at least 1)
        let num_resources = if region.resources.is_empty() {
            0
        } else {
            rng.random_range(1..=region.resources.len())
        };
        let mut settlement_resources = region.resources.clone();
        settlement_resources.shuffle(rng);
        settlement_resources.truncate(num_resources);

        // Generate settlement name
        let name = generate_settlement_name(profile.base, rng);

        let breakdown = PopulationBreakdown::from_total(population);
        let prosperity = rng.random_range(0.4..0.7);
        let prestige = (population as f64 / 1000.0).clamp(0.05, 0.15);

        let settlement_id = world.add_entity(
            EntityKind::Settlement,
            name,
            Some(SimTimestamp::from_year(0)),
            EntityData::Settlement(SettlementData {
                population,
                population_breakdown: breakdown,
                x: sx,
                y: sy,
                resources: settlement_resources,
                prosperity,
                treasury: 0.0,
                dominant_culture: None,
                culture_makeup: std::collections::BTreeMap::new(),
                cultural_tension: 0.0,
                active_disease: None,
                plague_immunity: 0.0,
                fortification_level: 0,
                active_siege: None,
                prestige,
                active_disaster: None,
            }),
            founding_event,
        );

        // LocatedIn relationship
        world.add_relationship(
            settlement_id,
            region.id,
            RelationshipKind::LocatedIn,
            SimTimestamp::from_year(0),
            founding_event,
        );
    }
}

fn generate_settlement_name(terrain: Terrain, rng: &mut dyn RngCore) -> String {
    let prefixes = match terrain {
        Terrain::Plains => &["Wheat", "Gold", "Green", "Wind", "Sun"][..],
        Terrain::Forest => &["Oak", "Elm", "Thorn", "Moss", "Pine"][..],
        Terrain::Mountains => &["Iron", "Stone", "Eagle", "Frost", "Storm"][..],
        Terrain::Hills => &["Copper", "Amber", "Shepherd", "Mill", "Ridge"][..],
        Terrain::Desert => &["Sand", "Sun", "Oasis", "Dust", "Mirage"][..],
        Terrain::Swamp => &["Bog", "Reed", "Fog", "Marsh", "Mud"][..],
        Terrain::Coast => &["Port", "Anchor", "Tide", "Shell", "Wave"][..],
        Terrain::Tundra => &["Frost", "Ice", "Snow", "White", "Pale"][..],
        Terrain::Jungle => &["Vine", "Fern", "Parrot", "Orchid", "Canopy"][..],
        Terrain::Volcanic => &["Ash", "Ember", "Cinder", "Slag", "Flame"][..],
        Terrain::ShallowWater => &["Reef", "Shoal", "Tide", "Shell", "Pearl"][..],
        Terrain::DeepWater => &["Abyss", "Deep", "Dark", "Storm", "Wave"][..],
    };

    let suffixes = &[
        "hold", "haven", "ford", "stead", "gate", "bury", "well", "ton", "march", "dale",
    ];

    let prefix = prefixes[rng.random_range(0..prefixes.len())];
    let suffix = suffixes[rng.random_range(0..suffixes.len())];

    format!("{prefix}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    use crate::model::World;
    use crate::worldgen::config::WorldGenConfig;
    use crate::worldgen::geography::generate_regions;

    fn make_world_with_regions() -> (World, WorldGenConfig) {
        use crate::worldgen::config::MapConfig;
        let config = WorldGenConfig {
            seed: 12345,
            map: MapConfig {
                num_regions: 15,
                width: 500.0,
                height: 500.0,
                num_biome_centers: 4,
                adjacency_k: 3,
            },
            ..WorldGenConfig::default()
        };
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);
        (world, config)
    }

    #[test]
    fn generates_some_settlements() {
        let (mut world, config) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world, &config, &mut rng);

        let settlement_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement)
            .count();
        assert!(
            settlement_count > 0,
            "should generate at least one settlement"
        );
    }

    #[test]
    fn every_settlement_has_located_in() {
        let (mut world, config) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement)
        {
            let located_in_count = entity
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::LocatedIn)
                .count();
            assert_eq!(
                located_in_count, 1,
                "settlement {} should have exactly 1 LocatedIn, got {}",
                entity.name, located_in_count
            );
        }
    }

    #[test]
    fn settlement_coordinates_within_bounds() {
        let (mut world, config) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement)
        {
            let sd = entity.data.as_settlement().unwrap();
            let x = sd.x;
            let y = sd.y;
            assert!(
                x >= 0.0 && x <= config.map.width,
                "settlement x={} out of bounds",
                x
            );
            assert!(
                y >= 0.0 && y <= config.map.height,
                "settlement y={} out of bounds",
                y
            );
        }
    }

    #[test]
    fn settlements_have_population() {
        let (mut world, config) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement)
        {
            let pop = entity.data.as_settlement().unwrap().population;
            assert!(pop > 0, "settlement {} has zero population", entity.name);
        }
    }

    #[test]
    fn deterministic_settlements() {
        let (mut world1, config) = make_world_with_regions();
        let mut rng1 = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world1, &config, &mut rng1);

        let (mut world2, _) = make_world_with_regions();
        let mut rng2 = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world2, &config, &mut rng2);

        let names1: Vec<&str> = world1
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement)
            .map(|e| e.name.as_str())
            .collect();
        let names2: Vec<&str> = world2
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names1, names2);
    }
}
