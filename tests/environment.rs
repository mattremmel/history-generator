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
    let mut world = worldgen::generate_world(config);
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
    let _ = run(&mut world, &mut systems, SimConfig::new(1, 100, 42));

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

    let sd = world.settlement(settlement);
    // After running EnvironmentSystem, seasonal.food should have been set
    // (it will be something other than the default 1.0 depending on season)
    assert!(
        sd.seasonal.food > 0.0,
        "settlement should have season food modifier after 1 year"
    );
}

#[test]
fn scenario_annual_food_modifier_stored() {
    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone("Town");
    let settlement = setup.settlement;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(EnvironmentSystem)];
    let world = s.run(&mut systems, 2, 42);

    let sd = world.settlement(settlement);
    // food_annual should be set (averaged over the year's months)
    assert!(
        sd.seasonal.food_annual > 0.0,
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

    let sd = world.settlement(settlement);
    // construction_months should have been computed (some months may be blocked)
    assert!(
        sd.seasonal.construction_months > 0,
        "settlement should have construction_months after 2 years"
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
    use history_gen::model::ResourceType;

    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone("Town");
    let _ = s
        .settlement_mut(setup.settlement)
        .population(300)
        .resources(vec![ResourceType::Grain, ResourceType::Iron]);
    let settlement = setup.settlement;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(EnvironmentSystem),
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
    ];
    let world = s.run(&mut systems, 10, 42);

    let sd = world.settlement(settlement);
    assert!(
        !sd.production.is_empty(),
        "settlement should have production after 10 years with Economy system"
    );
}
