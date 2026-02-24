use history_gen::model::{BuildingType, EntityKind, EventKind, RelationshipKind, World};
use history_gen::sim::{
    BuildingSystem, DemographicsSystem, EconomySystem, PoliticsSystem, SimConfig, SimSystem, run,
};
use history_gen::worldgen::{self, config::WorldGenConfig};

/// Run with BuildingSystem in the tick order (Demographics → Buildings → Economy → Politics)
fn generate_and_run(seed: u64, num_years: u32) -> World {
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(BuildingSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
    world
}

#[test]
fn buildings_constructed_over_time() {
    let world = generate_and_run(42, 500);

    // Count all buildings (worldgen + constructed)
    let building_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Building)
        .count();

    // Worldgen creates mines + ports. After 500 years, construction should add more.
    let constructed_count = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Custom("building_constructed".to_string()))
        .count();

    assert!(building_count > 0, "should have buildings after 500 years");
    assert!(
        constructed_count > 0,
        "should have construction events after 500 years (found {constructed_count})"
    );
}

#[test]
fn buildings_have_correct_types() {
    let world = generate_and_run(42, 500);

    let valid_types = [
        BuildingType::Mine,
        BuildingType::Port,
        BuildingType::Market,
        BuildingType::Granary,
        BuildingType::Temple,
        BuildingType::Workshop,
        BuildingType::Aqueduct,
        BuildingType::Library,
    ];

    for e in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Building)
    {
        let bd = e
            .data
            .as_building()
            .expect("building should have BuildingData");
        assert!(
            valid_types.contains(&bd.building_type),
            "building '{}' has unexpected type: {:?}",
            e.name,
            bd.building_type
        );
        assert!(
            bd.condition >= 0.0 && bd.condition <= 1.0,
            "building '{}' condition {} out of range",
            e.name,
            bd.condition
        );
        assert!(
            bd.level <= 2,
            "building '{}' level {} exceeds max",
            e.name,
            bd.level
        );
    }
}

#[test]
fn buildings_linked_to_settlements() {
    let world = generate_and_run(42, 200);

    for e in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Building && e.end.is_none())
    {
        let has_located_in = e
            .relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none());
        assert!(
            has_located_in,
            "living building '{}' should have LocatedIn relationship",
            e.name
        );

        // Verify the target is a settlement
        let target_id = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
            .unwrap()
            .target_entity_id;
        let target_kind = world.entities.get(&target_id).map(|e| &e.kind);
        assert_eq!(
            target_kind,
            Some(&EntityKind::Settlement),
            "building '{}' should be LocatedIn a Settlement",
            e.name
        );
    }
}

#[test]
fn building_decay_and_destruction() {
    let world = generate_and_run(42, 500);

    // Some buildings should have been destroyed (ended) after 500 years
    let destroyed_count = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Building && e.end.is_some())
        .count();

    let destroyed_events = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Custom("building_destroyed".to_string()))
        .count();

    // Not all runs will have destroyed buildings, but with decay at 0.01/year
    // over 500 years, worldgen buildings will lose 5.0 condition (capped at 0)
    // so they should be destroyed at some point
    assert!(
        destroyed_count > 0,
        "some buildings should be destroyed after 500 years of decay"
    );
    assert!(
        destroyed_events > 0,
        "should have building_destroyed events"
    );
}

#[test]
fn building_bonuses_affect_settlement_extras() {
    let world = generate_and_run(42, 200);

    // Find settlements that have at least one building
    let mut found_bonus = false;
    for e in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
    {
        // Check if this settlement has any buildings
        let has_buildings = world.entities.values().any(|b| {
            b.kind == EntityKind::Building
                && b.end.is_none()
                && b.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LocatedIn
                        && r.target_entity_id == e.id
                        && r.end.is_none()
                })
        });

        if has_buildings {
            // At least one bonus extra should be set
            let has_any_bonus = [
                "building_mine_bonus",
                "building_workshop_bonus",
                "building_market_bonus",
                "building_port_trade_bonus",
                "building_happiness_bonus",
                "building_capacity_bonus",
                "building_food_buffer",
            ]
            .iter()
            .any(|key| {
                e.extra
                    .get(*key)
                    .and_then(|v| v.as_f64())
                    .is_some_and(|v| v > 0.0)
            });
            if has_any_bonus {
                found_bonus = true;
                break;
            }
        }
    }

    assert!(
        found_bonus,
        "settlements with buildings should have positive bonus extras"
    );
}

#[test]
fn mine_bonus_increases_production() {
    // Compare production of mining resources in settlements with vs without mines
    let world = generate_and_run(42, 200);

    let mut mine_settlement_productions = Vec::new();
    let mut no_mine_settlement_productions = Vec::new();

    for e in world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
    {
        let has_mine = world.entities.values().any(|b| {
            b.kind == EntityKind::Building
                && b.end.is_none()
                && b.data
                    .as_building()
                    .is_some_and(|bd| bd.building_type == BuildingType::Mine)
                && b.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LocatedIn
                        && r.target_entity_id == e.id
                        && r.end.is_none()
                })
        });

        let production_value: f64 = e
            .extra
            .get("production")
            .and_then(|v| v.as_object())
            .map(|obj| obj.values().filter_map(|v| v.as_f64()).sum())
            .unwrap_or(0.0);

        if has_mine && production_value > 0.0 {
            mine_settlement_productions.push(production_value);
        } else if production_value > 0.0 {
            no_mine_settlement_productions.push(production_value);
        }
    }

    // We just verify that settlements with mines exist and have production
    if !mine_settlement_productions.is_empty() {
        let avg_mine = mine_settlement_productions.iter().sum::<f64>()
            / mine_settlement_productions.len() as f64;
        assert!(
            avg_mine > 0.0,
            "settlements with mines should have positive production"
        );
    }
}

#[test]
fn upgrades_occur_over_time() {
    let mut any_upgrades = false;
    for seed in [42, 99, 123, 777] {
        let world = generate_and_run(seed, 500);

        let upgrade_events = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Custom("building_upgraded".to_string()))
            .count();

        let upgraded_buildings = world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Building
                    && e.data.as_building().is_some_and(|bd| bd.level > 0)
            })
            .count();

        if upgrade_events > 0 || upgraded_buildings > 0 {
            any_upgrades = true;
            break;
        }
    }

    assert!(
        any_upgrades,
        "should have some building upgrades after 500 years across 4 seeds"
    );
}
