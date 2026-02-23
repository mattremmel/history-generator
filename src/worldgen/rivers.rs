use rand::Rng;
use rand::RngCore;

use crate::model::{
    EntityData, EntityKind, EventKind, RelationshipKind, RiverData, SimTimestamp, World,
};

use super::terrain::{Terrain, TerrainTag};
use crate::worldgen::config::WorldGenConfig;

/// Maximum number of segments in a river path.
const MAX_RIVER_LENGTH: usize = 8;

/// Generate rivers that flow from highland sources toward water/map edges.
pub fn generate_rivers(world: &mut World, config: &WorldGenConfig, rng: &mut dyn RngCore) {
    let genesis_event = world.add_event(
        EventKind::Custom("world_genesis".to_string()),
        SimTimestamp::from_year(0),
        "Rivers carve through the land".to_string(),
    );

    // Collect region data: (id, terrain, index_in_list, adjacency_indices)
    let region_ids: Vec<u64> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .map(|e| e.id)
        .collect();

    if region_ids.is_empty() {
        return;
    }

    let region_terrains: Vec<Terrain> = region_ids
        .iter()
        .map(|&id| {
            let region = world.entities[&id].data.as_region().unwrap();
            Terrain::try_from(region.terrain.clone()).expect("invalid terrain")
        })
        .collect();

    // Build adjacency lookup by region_id -> list of region_ids
    let adjacency: Vec<Vec<usize>> = region_ids
        .iter()
        .map(|&id| {
            world.entities[&id]
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::AdjacentTo)
                .filter_map(|r| region_ids.iter().position(|&rid| rid == r.target_entity_id))
                .collect()
        })
        .collect();

    // Find source candidates: prefer Mountains, Hills, Tundra
    let mut source_candidates: Vec<usize> = (0..region_ids.len())
        .filter(|&i| {
            matches!(
                region_terrains[i],
                Terrain::Mountains | Terrain::Hills | Terrain::Tundra
            )
        })
        .collect();

    // If not enough highland sources, add any land region
    if source_candidates.len() < config.rivers.num_rivers as usize {
        for (i, terrain) in region_terrains.iter().enumerate() {
            if !terrain.is_water() && !source_candidates.contains(&i) {
                source_candidates.push(i);
            }
        }
    }

    // Shuffle and take num_rivers sources
    for i in (1..source_candidates.len()).rev() {
        let j = rng.random_range(0..=i);
        source_candidates.swap(i, j);
    }
    let num_rivers = (config.rivers.num_rivers as usize).min(source_candidates.len());

    let prefixes = &[
        "Silver", "Black", "Winding", "Swift", "Golden", "Crystal", "Serpent", "Iron",
    ];
    let suffixes = &["River", "Stream", "Run", "Waters", "Flow", "Brook"];

    for &source in source_candidates.iter().take(num_rivers) {
        // Random walk toward water or map edge
        let mut path: Vec<usize> = vec![source];
        let mut visited = std::collections::HashSet::new();
        visited.insert(source);

        for _ in 0..MAX_RIVER_LENGTH {
            let current = *path.last().unwrap();

            // If we reached water, stop
            if region_terrains[current].is_water() {
                break;
            }

            // Find unvisited neighbors, prefer water or downhill
            let neighbors: Vec<usize> = adjacency[current]
                .iter()
                .filter(|&&n| !visited.contains(&n))
                .copied()
                .collect();

            if neighbors.is_empty() {
                break;
            }

            // Prefer water neighbors, then any
            let next =
                if let Some(&water) = neighbors.iter().find(|&&n| region_terrains[n].is_water()) {
                    water
                } else {
                    neighbors[rng.random_range(0..neighbors.len())]
                };

            visited.insert(next);
            path.push(next);
        }

        // Need at least 2 regions for a meaningful river
        if path.len() < 2 {
            continue;
        }

        // Create river entity
        let prefix = prefixes[rng.random_range(0..prefixes.len())];
        let suffix = suffixes[rng.random_range(0..suffixes.len())];
        let name = format!("{prefix} {suffix}");

        let region_path: Vec<u64> = path.iter().map(|&i| region_ids[i]).collect();
        let river_length = path.len();
        let river_id = world.add_entity(
            EntityKind::River,
            name,
            Some(SimTimestamp::from_year(0)),
            EntityData::River(RiverData {
                region_path,
                length: river_length,
            }),
            genesis_event,
        );

        // FlowsThrough relationships (river → each region in path)
        for &region_idx in &path {
            world.add_relationship(
                river_id,
                region_ids[region_idx],
                RelationshipKind::FlowsThrough,
                SimTimestamp::from_year(0),
                genesis_event,
            );
        }

        // Add Riverine tag to traversed land regions
        for &region_idx in &path {
            if region_terrains[region_idx].is_water() {
                continue;
            }
            let region_id = region_ids[region_idx];
            add_terrain_tag(world, region_id, TerrainTag::Riverine, genesis_event);
        }
    }
}

/// Add a terrain tag to a region if not already present.
fn add_terrain_tag(world: &mut World, region_id: u64, tag: TerrainTag, _event_id: u64) {
    let tag_str = tag.as_str().to_string();
    let entity = world.entities.get_mut(&region_id).unwrap();
    let region = entity.data.as_region_mut().unwrap();
    if !region.terrain_tags.contains(&tag_str) {
        region.terrain_tags.push(tag_str);
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

    fn make_world_with_regions() -> (World, WorldGenConfig) {
        use crate::worldgen::config::{MapConfig, RiverConfig, TerrainConfig};
        let config = WorldGenConfig {
            seed: 12345,
            map: MapConfig {
                num_regions: 20,
                ..MapConfig::default()
            },
            rivers: RiverConfig { num_rivers: 4 },
            terrain: TerrainConfig {
                water_fraction: 0.2,
            },
            ..WorldGenConfig::default()
        };
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);
        (world, config)
    }

    #[test]
    fn generates_rivers() {
        let (mut world, config) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 2);
        generate_rivers(&mut world, &config, &mut rng);

        let river_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::River)
            .count();
        assert!(river_count > 0, "should generate at least one river");
    }

    #[test]
    fn rivers_have_flows_through() {
        let (mut world, config) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 2);
        generate_rivers(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::River)
        {
            let flows_count = entity
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::FlowsThrough)
                .count();
            assert!(
                flows_count >= 2,
                "river '{}' should flow through at least 2 regions, got {}",
                entity.name,
                flows_count
            );
        }
    }

    #[test]
    fn rivers_have_region_path_property() {
        let (mut world, config) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 2);
        generate_rivers(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::River)
        {
            let river = entity.data.as_river().unwrap();
            assert!(
                river.region_path.len() >= 2,
                "region_path should have at least 2 entries"
            );
            assert!(river.length >= 2);
        }
    }

    #[test]
    fn riverine_tag_added_to_traversed_regions() {
        let (mut world, config) = make_world_with_regions();
        let mut rng = SmallRng::seed_from_u64(config.seed + 2);
        generate_rivers(&mut world, &config, &mut rng);

        // Find a river and check its traversed regions have Riverine tag
        let river = world
            .entities
            .values()
            .find(|e| e.kind == EntityKind::River)
            .expect("should have at least one river");

        let region_path = &river.data.as_river().unwrap().region_path;

        for &region_id in region_path {
            let region_entity = &world.entities[&region_id];
            let region = region_entity.data.as_region().unwrap();
            let terrain = Terrain::try_from(region.terrain.clone()).unwrap();
            if terrain.is_water() {
                continue;
            }
            let has_riverine = region.terrain_tags.iter().any(|t| t == "riverine");
            assert!(
                has_riverine,
                "land region '{}' traversed by river should have riverine tag",
                region_entity.name
            );
        }
    }
}
