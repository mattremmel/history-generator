pub mod buildings;
pub mod config;
pub mod deposits;
pub mod features;
pub mod geography;
pub mod rivers;
pub mod settlements;
pub mod terrain;

use rand::RngCore;
use rand::SeedableRng;
use rand::rngs::SmallRng;

use crate::model::World;

pub use config::{MapConfig, RiverConfig, TerrainConfig, WorldGenConfig};
pub use terrain::Terrain;

/// A single worldgen step: fn(&mut World, &WorldGenConfig, &mut dyn RngCore).
pub type WorldGenStep = fn(&mut World, &WorldGenConfig, &mut dyn RngCore);

pub struct WorldGenPipeline {
    steps: Vec<(&'static str, WorldGenStep)>,
    config: WorldGenConfig,
}

impl WorldGenPipeline {
    pub fn new(config: WorldGenConfig) -> Self {
        Self {
            steps: Vec::new(),
            config,
        }
    }

    /// Append a named step to the pipeline.
    pub fn step(mut self, name: &'static str, f: WorldGenStep) -> Self {
        self.steps.push((name, f));
        self
    }

    /// Run all steps in order with a seeded RNG.
    pub fn run(self) -> World {
        let mut world = World::new();
        let mut rng = SmallRng::seed_from_u64(self.config.seed);
        for (_name, step) in &self.steps {
            step(&mut world, &self.config, &mut rng);
        }
        world
    }
}

/// Build the default worldgen pipeline with all standard steps.
pub fn default_pipeline(config: WorldGenConfig) -> WorldGenPipeline {
    WorldGenPipeline::new(config)
        .step("regions", geography::generate_regions)
        .step("rivers", rivers::generate_rivers)
        .step("features", features::generate_features)
        .step("deposits", deposits::generate_deposits)
        .step("settlements", settlements::generate_settlements_step)
        .step("buildings", buildings::generate_buildings)
}

/// Generate a complete world with regions, terrain, and settlements.
pub fn generate_world(config: &WorldGenConfig) -> World {
    default_pipeline(config.clone()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityKind;

    #[test]
    fn custom_pipeline_without_rivers() {
        let config = WorldGenConfig::default();
        let world = WorldGenPipeline::new(config)
            .step("regions", geography::generate_regions)
            .step("features", features::generate_features)
            .step("deposits", deposits::generate_deposits)
            .step("settlements", settlements::generate_settlements_step)
            .step("buildings", buildings::generate_buildings)
            .run();

        let river_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::River)
            .count();
        assert_eq!(
            river_count, 0,
            "pipeline without rivers step should have no River entities"
        );

        // But should still have regions
        let region_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Region)
            .count();
        assert!(region_count > 0, "should still generate regions");
    }

    #[test]
    fn default_pipeline_matches_generate_world() {
        let config = WorldGenConfig {
            seed: 99,
            ..WorldGenConfig::default()
        };

        let world1 = generate_world(&config);
        let world2 = default_pipeline(config).run();

        assert_eq!(world1.entities.len(), world2.entities.len());
        assert_eq!(world1.events.len(), world2.events.len());
    }
}
