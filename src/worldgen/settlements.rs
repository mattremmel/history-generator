use rand::Rng;
use rand::RngCore;
use rand::seq::SliceRandom;

use crate::model::PopulationBreakdown;
use crate::model::entity_data::ResourceType;
use crate::model::{EntityData, EntityKind, RelationshipKind, SimTimestamp, World};

use super::terrain::{Terrain, TerrainProfile, TerrainTag};
use crate::worldgen::config::WorldGenConfig;

/// Coordinate jitter range (fraction of map size) for settlement placement.
const JITTER_FRACTION: f64 = 0.03;

/// Generate settlements in regions based on terrain probability.
pub fn generate_settlements(
    world: &mut World,
    config: &WorldGenConfig,
    rng: &mut dyn RngCore,
    founding_event: u64,
) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Region),
        "settlements step requires regions to exist"
    );
    let map_width = config.map.width;
    let map_height = config.map.height;

    // Collect region info before mutating world
    struct RegionInfo {
        id: u64,
        profile: TerrainProfile,
        x: f64,
        y: f64,
        resources: Vec<ResourceType>,
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

        let is_coastal = profile.base == Terrain::Coast
            || profile.tags.contains(&TerrainTag::Coastal)
            || profile.tags.contains(&TerrainTag::Riverine);

        let mut data = EntityData::default_for_kind(EntityKind::Settlement);
        if let EntityData::Settlement(ref mut sd) = data {
            sd.population = population;
            sd.population_breakdown = breakdown;
            sd.x = sx;
            sd.y = sy;
            sd.resources = settlement_resources;
            sd.prosperity = prosperity;
            sd.prestige = prestige;
            sd.is_coastal = is_coastal;
        }

        let settlement_id = world.add_entity(
            EntityKind::Settlement,
            name,
            Some(SimTimestamp::from_year(0)),
            data,
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

    use crate::worldgen::config::WorldGenConfig;
    use crate::worldgen::geography::generate_regions;

    fn make_world_with_regions() -> (World, WorldGenConfig, u64) {
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
        let (world, ev) = crate::worldgen::make_test_world(&config, &[generate_regions]);
        (world, config, ev)
    }

    #[test]
    fn generates_some_settlements() {
        let (mut world, config, ev) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world, &config, &mut rng, ev);

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
        let (mut world, config, ev) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world, &config, &mut rng, ev);

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
        let (mut world, config, ev) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world, &config, &mut rng, ev);

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
        let (mut world, config, ev) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world, &config, &mut rng, ev);

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
        let (mut world1, config, ev1) = make_world_with_regions();
        let mut rng1 = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world1, &config, &mut rng1, ev1);

        let (mut world2, _, ev2) = make_world_with_regions();
        let mut rng2 = SmallRng::seed_from_u64(config.seed + 1);
        generate_settlements(&mut world2, &config, &mut rng2, ev2);

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
