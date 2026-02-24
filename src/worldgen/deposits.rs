use rand::Rng;
use rand::RngCore;

use crate::model::{
    EntityData, EntityKind, RelationshipKind, ResourceDepositData, SimTimestamp, World,
};

use super::terrain::TerrainProfile;
use crate::worldgen::config::WorldGenConfig;

/// Probability that a resource in a region spawns a deposit entity.
const DEPOSIT_SPAWN_CHANCE: f64 = 0.4;

/// Generate resource deposit entities in regions.
pub fn generate_deposits(
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
        "deposits step requires regions to exist"
    );

    // Collect region info
    let regions: Vec<(u64, TerrainProfile, f64, f64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .map(|e| {
            let region = e.data.as_region().unwrap();
            (
                e.id,
                TerrainProfile::new(region.terrain, region.terrain_tags.clone()),
                region.x,
                region.y,
            )
        })
        .collect();

    for (region_id, profile, rx, ry) in &regions {
        let resources = profile.effective_resources();

        for resource in resources {
            if rng.random_range(0.0..1.0) >= DEPOSIT_SPAWN_CHANCE {
                continue;
            }

            let resource_str = resource.as_str();
            let category = resource_category(resource_str);
            let (qty_min, qty_max) = category.quantity_range();
            let quantity = rng.random_range(qty_min..=qty_max);
            let quality: f64 = rng.random_range(0.1..=1.0);
            let discovered = rng.random_range(0.0..1.0) < category.discovery_chance();

            let jitter_x = rng.random_range(-15.0..15.0);
            let jitter_y = rng.random_range(-15.0..15.0);

            let name = format!("{} deposit", super::capitalize(resource_str));
            let deposit_id = world.add_entity(
                EntityKind::ResourceDeposit,
                name,
                Some(SimTimestamp::from_year(0)),
                EntityData::ResourceDeposit(ResourceDepositData {
                    resource_type: resource,
                    quantity,
                    quality,
                    discovered,
                    x: rx + jitter_x,
                    y: ry + jitter_y,
                }),
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
        use crate::worldgen::config::MapConfig;
        let config = WorldGenConfig {
            seed: 12345,
            map: MapConfig {
                num_regions: 20,
                ..MapConfig::default()
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
    fn generates_deposits() {
        let (mut world, config, ev) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 4);
        generate_deposits(&mut world, &config, &mut rng, ev);

        let count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::ResourceDeposit)
            .count();
        assert!(count > 0, "should generate at least one deposit");
    }

    #[test]
    fn deposits_have_required_properties() {
        let (mut world, config, ev) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 4);
        generate_deposits(&mut world, &config, &mut rng, ev);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::ResourceDeposit)
        {
            let deposit = entity.data.as_resource_deposit().unwrap_or_else(|| {
                panic!("deposit '{}' should have ResourceDepositData", entity.name)
            });
            assert!(
                !deposit.resource_type.as_str().is_empty(),
                "deposit '{}' missing resource_type",
                entity.name
            );
            assert!(
                (0.0..=1.0).contains(&deposit.quality),
                "quality should be 0.0-1.0, got {}",
                deposit.quality
            );
        }
    }

    #[test]
    fn deposits_have_located_in() {
        let (mut world, config, ev) = make_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 4);
        generate_deposits(&mut world, &config, &mut rng, ev);

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
