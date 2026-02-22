pub mod config;
pub mod geography;
pub mod settlements;
pub mod terrain;

use rand::SeedableRng;
use rand::rngs::SmallRng;

use crate::model::World;

pub use config::WorldGenConfig;
pub use terrain::Terrain;

/// Generate a complete world with regions, terrain, and settlements.
pub fn generate_world(config: &WorldGenConfig) -> World {
    let mut world = World::new();
    let mut rng = SmallRng::seed_from_u64(config.seed);

    geography::generate_regions(&mut world, config, &mut rng);
    settlements::generate_settlements(&mut world, config.map_width, config.map_height, &mut rng);

    world
}
