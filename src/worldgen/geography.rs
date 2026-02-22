use std::collections::VecDeque;

use rand::Rng;
use rand::RngCore;

use crate::model::{EntityKind, EventKind, RelationshipKind, SimTimestamp, World};

use super::config::WorldGenConfig;
use super::terrain::Terrain;

/// Minimum distance between region seed points (fraction of map diagonal).
const MIN_DISTANCE_FRACTION: f64 = 0.08;

/// Chance that a region's terrain differs from its nearest biome center.
const PERTURBATION_CHANCE: f64 = 0.15;

/// Generate regions with terrain, coordinates, and adjacency relationships.
pub fn generate_regions(world: &mut World, config: &WorldGenConfig, rng: &mut dyn RngCore) {
    let genesis_event = world.add_event(
        EventKind::Custom("world_genesis".to_string()),
        SimTimestamp::from_year(0),
        "The world takes shape".to_string(),
    );

    // 1. Scatter region seed points with min-distance rejection
    let min_dist = MIN_DISTANCE_FRACTION
        * (config.map_width * config.map_width + config.map_height * config.map_height).sqrt();
    let points = scatter_points(
        config.num_regions as usize,
        config.map_width,
        config.map_height,
        min_dist,
        rng,
    );

    // 2. Pick biome centers and assign each a random terrain
    let biome_centers = scatter_points(
        config.num_biome_centers as usize,
        config.map_width,
        config.map_height,
        0.0, // no min distance constraint for biome centers
        rng,
    );
    let biome_terrains: Vec<Terrain> = (0..biome_centers.len()).map(|_| rng.random()).collect();

    // 3. Assign terrain to each region based on nearest biome center
    let terrains: Vec<Terrain> = points
        .iter()
        .map(|&(x, y)| {
            let nearest_terrain = nearest_biome_terrain(x, y, &biome_centers, &biome_terrains);
            if rng.random_range(0.0..1.0) < PERTURBATION_CHANCE {
                rng.random()
            } else {
                nearest_terrain
            }
        })
        .collect();

    // 4. Create Region entities
    let mut region_ids: Vec<u64> = Vec::with_capacity(points.len());
    for (i, (&(x, y), &terrain)) in points.iter().zip(terrains.iter()).enumerate() {
        let name = generate_region_name(terrain, i, rng);
        let id = world.add_entity(
            EntityKind::Region,
            name,
            Some(SimTimestamp::from_year(0)),
            genesis_event,
        );
        world.set_property(
            id,
            "terrain".to_string(),
            serde_json::json!(terrain.as_str()),
            genesis_event,
        );
        world.set_property(id, "x".to_string(), serde_json::json!(x), genesis_event);
        world.set_property(id, "y".to_string(), serde_json::json!(y), genesis_event);
        let resources: Vec<&str> = terrain.resources().to_vec();
        world.set_property(
            id,
            "resources".to_string(),
            serde_json::json!(resources),
            genesis_event,
        );
        region_ids.push(id);
    }

    // 5. Compute K-nearest-neighbor adjacency
    let k = config.adjacency_k as usize;
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); points.len()];

    for i in 0..points.len() {
        let mut distances: Vec<(usize, f64)> = (0..points.len())
            .filter(|&j| j != i)
            .map(|j| (j, dist(points[i], points[j])))
            .collect();
        distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        for &(j, _) in distances.iter().take(k) {
            if !adjacency[i].contains(&j) {
                adjacency[i].push(j);
            }
            if !adjacency[j].contains(&i) {
                adjacency[j].push(i);
            }
        }
    }

    // 6. Ensure connectivity via BFS; add edges if disconnected
    ensure_connected(&points, &mut adjacency);

    // 7. Create bidirectional AdjacentTo relationships
    for i in 0..adjacency.len() {
        for &j in &adjacency[i] {
            if i < j {
                world.add_relationship(
                    region_ids[i],
                    region_ids[j],
                    RelationshipKind::AdjacentTo,
                    SimTimestamp::from_year(0),
                    genesis_event,
                );
                world.add_relationship(
                    region_ids[j],
                    region_ids[i],
                    RelationshipKind::AdjacentTo,
                    SimTimestamp::from_year(0),
                    genesis_event,
                );
            }
        }
    }
}

/// Scatter points with minimum distance rejection sampling.
fn scatter_points(
    count: usize,
    width: f64,
    height: f64,
    min_dist: f64,
    rng: &mut dyn RngCore,
) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(count);
    let max_attempts = count * 100;
    let mut attempts = 0;

    while points.len() < count && attempts < max_attempts {
        let x = rng.random_range(0.0..width);
        let y = rng.random_range(0.0..height);

        if min_dist > 0.0 && points.iter().any(|&p| dist(p, (x, y)) < min_dist) {
            attempts += 1;
            continue;
        }

        points.push((x, y));
        attempts += 1;
    }

    // If we couldn't place enough points with rejection, fill remaining randomly
    while points.len() < count {
        let x = rng.random_range(0.0..width);
        let y = rng.random_range(0.0..height);
        points.push((x, y));
    }

    points
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

fn nearest_biome_terrain(
    x: f64,
    y: f64,
    biome_centers: &[(f64, f64)],
    biome_terrains: &[Terrain],
) -> Terrain {
    biome_centers
        .iter()
        .zip(biome_terrains.iter())
        .min_by(|(a, _), (b, _)| dist(**a, (x, y)).partial_cmp(&dist(**b, (x, y))).unwrap())
        .map(|(_, &t)| t)
        .unwrap()
}

/// BFS connectivity check; connect disconnected components by adding edges.
fn ensure_connected(points: &[(f64, f64)], adjacency: &mut [Vec<usize>]) {
    if points.is_empty() {
        return;
    }

    let n = points.len();
    let mut visited = vec![false; n];
    let mut components: Vec<Vec<usize>> = Vec::new();

    // Find all connected components
    for start in 0..n {
        if visited[start] {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        visited[start] = true;
        while let Some(node) = queue.pop_front() {
            component.push(node);
            for &neighbor in &adjacency[node] {
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        components.push(component);
    }

    // Connect components by nearest pair between consecutive components
    for i in 1..components.len() {
        let mut best_dist = f64::MAX;
        let mut best_a = 0;
        let mut best_b = 0;

        for &a in &components[i - 1] {
            for &b in &components[i] {
                let d = dist(points[a], points[b]);
                if d < best_dist {
                    best_dist = d;
                    best_a = a;
                    best_b = b;
                }
            }
        }

        if !adjacency[best_a].contains(&best_b) {
            adjacency[best_a].push(best_b);
        }
        if !adjacency[best_b].contains(&best_a) {
            adjacency[best_b].push(best_a);
        }
    }
}

/// Generate a terrain-based region name.
fn generate_region_name(terrain: Terrain, index: usize, rng: &mut dyn RngCore) -> String {
    let prefixes = match terrain {
        Terrain::Plains => &["The Golden", "The Vast", "The Green", "The Wide"][..],
        Terrain::Forest => &["The Dark", "The Ancient", "The Whispering", "The Deep"][..],
        Terrain::Mountains => &["The Iron", "The Storm", "The Frozen", "The High"][..],
        Terrain::Hills => &["The Rolling", "The Amber", "The Windy", "The Gentle"][..],
        Terrain::Desert => &["The Burning", "The Endless", "The Red", "The Dust"][..],
        Terrain::Swamp => &["The Murky", "The Rotting", "The Black", "The Foggy"][..],
        Terrain::Coast => &["The Azure", "The Salt", "The Storm", "The Coral"][..],
        Terrain::Tundra => &["The Frozen", "The White", "The Bitter", "The Pale"][..],
        Terrain::Jungle => &["The Tangled", "The Verdant", "The Steaming", "The Wild"][..],
        Terrain::Volcanic => &["The Ashen", "The Burning", "The Molten", "The Ember"][..],
    };

    let suffixes = match terrain {
        Terrain::Plains => &["Plains", "Expanse", "Fields", "Steppe"][..],
        Terrain::Forest => &["Wood", "Forest", "Thicket", "Weald"][..],
        Terrain::Mountains => &["Peaks", "Mountains", "Heights", "Crags"][..],
        Terrain::Hills => &["Hills", "Downs", "Ridges", "Knolls"][..],
        Terrain::Desert => &["Wastes", "Sands", "Barrens", "Expanse"][..],
        Terrain::Swamp => &["Marsh", "Fen", "Mire", "Bog"][..],
        Terrain::Coast => &["Shore", "Coast", "Strand", "Bay"][..],
        Terrain::Tundra => &["Waste", "Reach", "Expanse", "Tundra"][..],
        Terrain::Jungle => &["Jungle", "Canopy", "Wilds", "Tangle"][..],
        Terrain::Volcanic => &["Caldera", "Wastes", "Forge", "Peaks"][..],
    };

    // Use index + rng to vary name selection
    let pi = (index + rng.random_range(0..prefixes.len())) % prefixes.len();
    let si = (index + rng.random_range(0..suffixes.len())) % suffixes.len();

    format!("{} {}", prefixes[pi], suffixes[si])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    use crate::model::World;

    use super::super::config::WorldGenConfig;

    fn test_config() -> WorldGenConfig {
        WorldGenConfig {
            seed: 12345,
            num_regions: 15,
            map_width: 500.0,
            map_height: 500.0,
            num_biome_centers: 4,
            adjacency_k: 3,
        }
    }

    #[test]
    fn generates_correct_number_of_regions() {
        let config = test_config();
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);

        let region_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
            .count();
        assert_eq!(region_count, config.num_regions as usize);
    }

    #[test]
    fn deterministic_with_same_seed() {
        let config = test_config();

        let mut world1 = World::new();
        let mut rng1 = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world1, &config, &mut rng1);

        let mut world2 = World::new();
        let mut rng2 = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world2, &config, &mut rng2);

        let names1: Vec<&str> = world1
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
            .map(|e| e.name.as_str())
            .collect();
        let names2: Vec<&str> = world2
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names1, names2);
    }

    #[test]
    fn all_regions_connected() {
        let config = test_config();
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);

        let region_ids: Vec<u64> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
            .map(|e| e.id)
            .collect();

        // BFS from first region
        let mut visited = std::collections::HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(region_ids[0]);
        visited.insert(region_ids[0]);

        while let Some(current) = queue.pop_front() {
            let entity = &world.entities[&current];
            for rel in &entity.relationships {
                if rel.kind == RelationshipKind::AdjacentTo
                    && !visited.contains(&rel.target_entity_id)
                {
                    visited.insert(rel.target_entity_id);
                    queue.push_back(rel.target_entity_id);
                }
            }
        }

        assert_eq!(
            visited.len(),
            region_ids.len(),
            "BFS should reach all {} regions, but only reached {}",
            region_ids.len(),
            visited.len()
        );
    }

    #[test]
    fn terrain_distribution_is_varied() {
        let config = WorldGenConfig {
            num_regions: 30,
            ..test_config()
        };
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);

        let terrains: std::collections::HashSet<String> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
            .filter_map(|e| e.properties.get("terrain"))
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        assert!(
            terrains.len() >= 2,
            "should have at least 2 terrain types, got {}",
            terrains.len()
        );
    }

    #[test]
    fn coordinates_within_bounds() {
        let config = test_config();
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
        {
            let x = entity.properties["x"].as_f64().unwrap();
            let y = entity.properties["y"].as_f64().unwrap();
            assert!(x >= 0.0 && x <= config.map_width, "x={} out of bounds", x);
            assert!(y >= 0.0 && y <= config.map_height, "y={} out of bounds", y);
        }
    }

    #[test]
    fn adjacency_is_bidirectional() {
        let config = test_config();
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
        {
            for rel in &entity.relationships {
                if rel.kind == RelationshipKind::AdjacentTo {
                    let target = &world.entities[&rel.target_entity_id];
                    let has_reverse = target.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::AdjacentTo && r.target_entity_id == entity.id
                    });
                    assert!(
                        has_reverse,
                        "AdjacentTo from {} to {} has no reverse",
                        entity.id, rel.target_entity_id
                    );
                }
            }
        }
    }
}
