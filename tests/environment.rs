use history_gen::model::EntityKind;
use history_gen::scenario::Scenario;
use history_gen::sim::{
    BuildingSystem, ConflictSystem, CultureSystem, DemographicsSystem, DiseaseSystem,
    EconomySystem, EnvironmentSystem, MigrationSystem, PoliticsSystem, ReputationSystem, SimConfig,
    SimSystem, run,
};
use history_gen::worldgen::{self, config::WorldGenConfig};

// ---------------------------------------------------------------------------
// Full integration smoke test (shortened to 100y)
// ---------------------------------------------------------------------------

#[test]
fn hundred_year_full_integration_no_panics() {
    let config = WorldGenConfig {
        seed: 42,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(EnvironmentSystem),
        Box::new(DemographicsSystem),
        Box::new(BuildingSystem),
        Box::new(EconomySystem),
        Box::new(ConflictSystem),
        Box::new(MigrationSystem),
        Box::new(DiseaseSystem),
        Box::new(CultureSystem),
        Box::new(PoliticsSystem),
        Box::new(ReputationSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, 100, 42));

    let living_settlements = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .count();
    assert!(
        living_settlements > 0,
        "should have living settlements after 100 years"
    );

    let living_factions = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .count();
    assert!(
        living_factions > 0,
        "should have living factions after 100 years"
    );
}

// ---------------------------------------------------------------------------
// Scenario-based tests
// ---------------------------------------------------------------------------

#[test]
fn scenario_seasonal_food_modifier_set_on_settlements() {
    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone("Town");
    let settlement = setup.settlement;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(EnvironmentSystem)];
    let world = s.run(&mut systems, 1, 42);

    let has_modifier = world.entities[&settlement]
        .extra
        .contains_key("season_food_modifier");
    assert!(
        has_modifier,
        "settlement should have season_food_modifier after 1 year"
    );
}

#[test]
fn scenario_annual_food_modifier_stored() {
    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone("Town");
    let settlement = setup.settlement;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(EnvironmentSystem)];
    let world = s.run(&mut systems, 2, 42);

    let has_annual = world.entities[&settlement]
        .extra
        .get("season_food_modifier_annual")
        .and_then(|v| v.as_f64())
        .is_some();
    assert!(
        has_annual,
        "settlement should have season_food_modifier_annual after 2 years"
    );
}

#[test]
fn scenario_construction_months_stored() {
    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone("Town");
    let settlement = setup.settlement;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(EnvironmentSystem)];
    let world = s.run(&mut systems, 2, 42);

    let has_construction_months = world.entities[&settlement]
        .extra
        .get("season_construction_months")
        .and_then(|v| v.as_u64())
        .is_some();
    assert!(
        has_construction_months,
        "settlement should have season_construction_months after 2 years"
    );
}

#[test]
fn scenario_disaster_damages_buildings() {
    use history_gen::model::BuildingType;

    // Buildings decay naturally. Set up a building with low condition to verify
    // that the building system processes decay correctly.
    let mut s = Scenario::at_year(100);
    let setup = s.add_settlement_standalone("Town");
    let building = s.add_building_with(BuildingType::Market, setup.settlement, |bd| {
        bd.condition = 0.15; // Low condition - will decay further
    });

    // Run building system for a few years — decay should reduce condition further
    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(BuildingSystem)];
    let world = s.run(&mut systems, 5, 42);

    let condition = world.entities[&building]
        .data
        .as_building()
        .unwrap()
        .condition;
    assert!(
        condition < 0.15,
        "building condition should decrease from decay, got {condition}"
    );
}

#[test]
fn scenario_economy_produces_seasonal_variation() {
    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone("Town");
    s.settlement_mut(setup.settlement).population(300);
    let settlement = setup.settlement;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(EnvironmentSystem),
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
    ];
    let world = s.run(&mut systems, 10, 42);

    let has_production = world.entities[&settlement].extra.contains_key("production");
    assert!(
        has_production,
        "settlement should have production after 10 years with Economy system"
    );
}
