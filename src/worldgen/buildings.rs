use rand::RngCore;

use crate::model::{
    BuildingData, BuildingType, EntityData, EntityKind, FeatureType, RelationshipKind,
    ResourceType, SimTimestamp, World,
};

use crate::sim::helpers;
use crate::worldgen::config::WorldGenConfig;

/// Generate buildings (mines, ports) linked to deposits and features.
pub fn generate_buildings(
    world: &mut World,
    _config: &WorldGenConfig,
    _rng: &mut dyn RngCore,
    genesis_event: u64,
) {
    debug_assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Settlement),
        "buildings step requires settlements to exist"
    );

    // Collect settlements and their regions
    let settlements: Vec<(u64, u64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement)
        .map(|e| {
            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn)
                .expect("settlement should have LocatedIn")
                .target_entity_id;
            (e.id, region_id)
        })
        .collect();

    // Collect deposits by region: (deposit_id, region_id, resource_type, discovered, x, y)
    let deposits: Vec<(u64, u64, ResourceType, bool, f64, f64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::ResourceDeposit)
        .map(|e| {
            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn)
                .expect("deposit should have LocatedIn")
                .target_entity_id;
            let deposit = e.data.as_resource_deposit().unwrap();
            (
                e.id,
                region_id,
                deposit.resource_type.clone(),
                deposit.discovered,
                deposit.x,
                deposit.y,
            )
        })
        .collect();

    // Collect harbor features by region
    let harbors: Vec<(u64, u64, f64, f64)> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::GeographicFeature
                && e.data.as_geographic_feature().map(|f| &f.feature_type)
                    == Some(&FeatureType::Harbor)
        })
        .map(|e| {
            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn)
                .expect("feature should have LocatedIn")
                .target_entity_id;
            let feature = e.data.as_geographic_feature().unwrap();
            (e.id, region_id, feature.x, feature.y)
        })
        .collect();

    // For each settlement, create mines for discovered mining deposits in its region
    for &(settlement_id, region_id) in &settlements {
        for (deposit_id, dep_region, resource_type, discovered, dx, dy) in &deposits {
            if *dep_region != region_id || !discovered {
                continue;
            }
            if !helpers::MINING_RESOURCES.contains(resource_type) {
                continue;
            }

            let name = format!("{} Mine", super::capitalize(resource_type.as_str()));
            let building_id = world.add_entity(
                EntityKind::Building,
                name,
                Some(SimTimestamp::from_year(0)),
                EntityData::Building(BuildingData {
                    building_type: BuildingType::Mine,
                    output_resource: Some(resource_type.clone()),
                    x: *dx,
                    y: *dy,
                    condition: 1.0,
                    level: 0,
                    constructed: SimTimestamp::default(),
                }),
                genesis_event,
            );

            // Exploits -> deposit
            world.add_relationship(
                building_id,
                *deposit_id,
                RelationshipKind::Exploits,
                SimTimestamp::from_year(0),
                genesis_event,
            );

            // LocatedIn -> settlement (not region)
            world.add_relationship(
                building_id,
                settlement_id,
                RelationshipKind::LocatedIn,
                SimTimestamp::from_year(0),
                genesis_event,
            );
        }

        // Create port for settlements in regions with harbors
        for (_, harbor_region, hx, hy) in &harbors {
            if *harbor_region != region_id {
                continue;
            }

            let building_id = world.add_entity(
                EntityKind::Building,
                "Port".to_string(),
                Some(SimTimestamp::from_year(0)),
                EntityData::Building(BuildingData {
                    building_type: BuildingType::Port,
                    output_resource: None,
                    x: *hx,
                    y: *hy,
                    condition: 1.0,
                    level: 0,
                    constructed: SimTimestamp::default(),
                }),
                genesis_event,
            );

            // LocatedIn -> settlement (not region)
            world.add_relationship(
                building_id,
                settlement_id,
                RelationshipKind::LocatedIn,
                SimTimestamp::from_year(0),
                genesis_event,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    use crate::model::{EventKind, SimTimestamp, World};
    use crate::worldgen::config::WorldGenConfig;
    use crate::worldgen::deposits::generate_deposits;
    use crate::worldgen::features::generate_features;
    use crate::worldgen::geography::generate_regions;
    use crate::worldgen::settlements::generate_settlements;

    fn genesis_event(world: &mut World) -> u64 {
        world.add_event(
            EventKind::Custom("world_genesis".to_string()),
            SimTimestamp::from_year(0),
            "test genesis".to_string(),
        )
    }

    fn make_full_world() -> (World, WorldGenConfig, u64) {
        use crate::worldgen::config::MapConfig;
        let config = WorldGenConfig {
            seed: 12345,
            map: MapConfig {
                num_regions: 25,
                ..MapConfig::default()
            },
            ..WorldGenConfig::default()
        };
        let mut world = World::new();
        let ev = genesis_event(&mut world);
        let mut rng = SmallRng::seed_from_u64(config.seed);
        generate_regions(&mut world, &config, &mut rng, ev);
        generate_settlements(&mut world, &config, &mut rng, ev);
        generate_features(&mut world, &config, &mut rng, ev);
        generate_deposits(&mut world, &config, &mut rng, ev);
        (world, config, ev)
    }

    #[test]
    fn mines_have_exploits_relationship() {
        let (mut world, config, ev) = make_full_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 5);
        generate_buildings(&mut world, &config, &mut rng, ev);

        let mines: Vec<_> = world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Building
                    && e.data.as_building().map(|b| &b.building_type) == Some(&BuildingType::Mine)
            })
            .collect();

        for mine in &mines {
            let exploits = mine
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Exploits)
                .count();
            assert_eq!(
                exploits, 1,
                "mine '{}' should have exactly 1 Exploits relationship",
                mine.name
            );
        }
    }

    #[test]
    fn buildings_have_located_in() {
        let (mut world, config, ev) = make_full_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 5);
        generate_buildings(&mut world, &config, &mut rng, ev);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Building)
        {
            let located_in = entity
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::LocatedIn)
                .count();
            assert_eq!(
                located_in, 1,
                "building '{}' should have exactly 1 LocatedIn",
                entity.name
            );
        }
    }

    #[test]
    fn buildings_linked_to_settlements_not_regions() {
        let (mut world, config, ev) = make_full_world();
        let mut rng = SmallRng::seed_from_u64(config.seed + 5);
        generate_buildings(&mut world, &config, &mut rng, ev);

        for entity in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Building)
        {
            let target_id = entity
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn)
                .unwrap()
                .target_entity_id;
            let target_kind = world.entities.get(&target_id).map(|e| &e.kind);
            assert_eq!(
                target_kind,
                Some(&EntityKind::Settlement),
                "building '{}' should be LocatedIn a Settlement, not a Region",
                entity.name
            );
        }
    }
}
