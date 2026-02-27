use history_gen::model::entity_data::ResourceType;
use history_gen::model::population::PopulationBreakdown;
use history_gen::model::{BuildingType, EntityKind, Terrain};
use history_gen::scenario::Scenario;
use history_gen::sim::{
    BuildingSystem, ConflictSystem, DemographicsSystem, EconomySystem, EnvironmentSystem,
    MigrationSystem, SimConfig, SimSystem, run,
};

// ---------------------------------------------------------------------------
// Building: Port requires coastal settlement
// ---------------------------------------------------------------------------

#[test]
fn port_construction_requires_coastal() {
    // Coastal settlement should be able to build a port, inland should not.
    let mut s = Scenario::at_year(1);
    let f = s.add_faction("Kingdom");
    s.modify_faction(f, |fd| fd.treasury = 10000.0);

    let r = s.add_region("Coast");
    let coastal = s.add_settlement_with("PortTown", f, r, |sd| {
        sd.population_breakdown = PopulationBreakdown::from_total(500);
        sd.is_coastal = true;
        sd.prosperity = 0.9;
    });
    // Pre-build Granary so Port is next in BUILDING_SPECS priority
    s.add_building(BuildingType::Granary, coastal);

    let r2 = s.add_region("Plains");
    s.make_adjacent(r, r2);
    let inland = s.add_settlement_with("InlandTown", f, r2, |sd| {
        sd.population_breakdown = PopulationBreakdown::from_total(500);
        sd.prosperity = 0.9;
    });
    s.add_building(BuildingType::Granary, inland);

    let mut world = s.build();
    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(BuildingSystem)];
    let _ = run(&mut world, &mut systems, SimConfig::new(1, 100, 42));

    let coastal_has_port = world.entities.values().any(|e| {
        e.kind == EntityKind::Building
            && e.end.is_none()
            && e.data
                .as_building()
                .is_some_and(|b| b.building_type == BuildingType::Port)
            && e.has_active_rel(history_gen::model::RelationshipKind::LocatedIn, coastal)
    });
    let inland_has_port = world.entities.values().any(|e| {
        e.kind == EntityKind::Building
            && e.end.is_none()
            && e.data
                .as_building()
                .is_some_and(|b| b.building_type == BuildingType::Port)
            && e.has_active_rel(history_gen::model::RelationshipKind::LocatedIn, inland)
    });

    assert!(
        coastal_has_port,
        "coastal settlement should have built a port"
    );
    assert!(
        !inland_has_port,
        "inland settlement should not have built a port"
    );
}

// ---------------------------------------------------------------------------
// Demographics: coastal + port settlements get higher capacity
// ---------------------------------------------------------------------------

#[test]
fn coastal_port_increases_capacity() {
    let mut s = Scenario::at_year(1);
    let f = s.add_faction("Kingdom");
    let r = s.add_region("Coast");
    let coastal_port = s.add_settlement_with("PortCity", f, r, |sd| {
        sd.population_breakdown = PopulationBreakdown::from_total(500);
        sd.is_coastal = true;
    });
    s.add_building(BuildingType::Port, coastal_port);

    let r2 = s.add_region("Plains");
    s.make_adjacent(r, r2);
    let inland = s.add_settlement_with("InlandCity", f, r2, |sd| {
        sd.population_breakdown = PopulationBreakdown::from_total(500);
    });

    let mut world = s.build();
    let mut systems: Vec<Box<dyn SimSystem>> =
        vec![Box::new(BuildingSystem), Box::new(DemographicsSystem)];
    let _ = run(&mut world, &mut systems, SimConfig::new(1, 1, 42));

    let port_cap = world.settlement(coastal_port).capacity;
    let inland_cap = world.settlement(inland).capacity;

    assert!(
        port_cap > inland_cap,
        "port city capacity ({port_cap}) should exceed inland ({inland_cap})"
    );
}

// ---------------------------------------------------------------------------
// Fishing bonus: port + Fish resource boosts fish production
// ---------------------------------------------------------------------------

#[test]
fn fishing_bonus_boosts_fish_production() {
    let mut s = Scenario::at_year(1);
    let f = s.add_faction("Kingdom");
    s.modify_faction(f, |fd| fd.treasury = 100.0);

    let r = s.add_region("Coast");
    let fishing_port = s.add_settlement_with("FishPort", f, r, |sd| {
        sd.is_coastal = true;
        sd.resources = vec![ResourceType::Fish, ResourceType::Salt];
    });
    s.add_building(BuildingType::Port, fishing_port);

    let r2 = s.add_region("Plains");
    s.make_adjacent(r, r2);
    let no_port = s.add_settlement_with("FishVillage", f, r2, |sd| {
        sd.resources = vec![ResourceType::Fish, ResourceType::Salt];
    });

    let mut world = s.build();
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(EnvironmentSystem),
        Box::new(BuildingSystem),
        Box::new(EconomySystem),
    ];
    let _ = run(&mut world, &mut systems, SimConfig::new(1, 1, 42));

    let port_fish = world
        .settlement(fishing_port)
        .production
        .get(&ResourceType::Fish)
        .copied()
        .unwrap_or(0.0);
    let village_fish = world
        .settlement(no_port)
        .production
        .get(&ResourceType::Fish)
        .copied()
        .unwrap_or(0.0);

    assert!(
        port_fish > village_fish,
        "port settlement fish production ({port_fish}) should exceed no-port ({village_fish})"
    );
}

// ---------------------------------------------------------------------------
// Naval movement: army crosses water via port regions
// ---------------------------------------------------------------------------

#[test]
fn army_crosses_water_via_port_regions() {
    // Set up two coastal kingdoms separated by water, at war.
    // Pre-set building bonuses so the port is recognized immediately.
    let mut s = Scenario::at_year(1);
    let f1 = s.add_faction("Attackers");
    s.modify_faction(f1, |fd| fd.treasury = 500.0);
    let r1 = s.add_region_with("Coast1", |rd| rd.terrain = Terrain::Coast);
    let port1 = s.add_settlement_with("Port1", f1, r1, |sd| {
        sd.population_breakdown = PopulationBreakdown::from_total(500);
        sd.is_coastal = true;
    });
    s.add_building(BuildingType::Port, port1);

    let water = s.add_region_with("Sea", |rd| rd.terrain = Terrain::ShallowWater);
    s.make_adjacent(r1, water);

    let f2 = s.add_faction("Defenders");
    s.modify_faction(f2, |fd| fd.treasury = 500.0);
    let r3 = s.add_region_with("Coast2", |rd| rd.terrain = Terrain::Coast);
    s.make_adjacent(water, r3);
    let coastal2 = s.add_settlement_with("Port2", f2, r3, |sd| {
        sd.population_breakdown = PopulationBreakdown::from_total(300);
        sd.is_coastal = true;
    });
    s.add_building(BuildingType::Port, coastal2);

    // Create war
    s.make_at_war(f1, f2);

    // Create armies — both sides need armies to prevent immediate "no army" treaty
    s.add_army("Invasion Force", f1, r1, 200);
    s.add_army("Garrison", f2, r3, 100);

    let mut world = s.build();
    let mut systems: Vec<Box<dyn SimSystem>> =
        vec![Box::new(BuildingSystem), Box::new(ConflictSystem)];
    // Run for enough time for the army to move across
    let _ = run(&mut world, &mut systems, SimConfig::new(1, 20, 42));

    // Check if any conquest or battle events occurred (army had to cross water to reach defender)
    let had_military_contact = world.events.values().any(|ev| {
        matches!(
            ev.kind,
            history_gen::model::EventKind::Conquest | history_gen::model::EventKind::Battle
        )
    });

    // Check if the defender settlement was conquered (ownership transferred to attacker)
    let was_conquered = world
        .entities
        .get(&coastal2)
        .map(|e| {
            e.relationships.iter().any(|r| {
                r.kind == history_gen::model::RelationshipKind::MemberOf
                    && r.target_entity_id == f1
                    && r.end.is_none()
            })
        })
        .unwrap_or(false);

    // The army should have been able to cross the water
    assert!(
        had_military_contact || was_conquered,
        "army should cross water via port regions and engage defender"
    );
}

// ---------------------------------------------------------------------------
// Integration: 200-year run with coastal kingdoms
// ---------------------------------------------------------------------------

#[test]
fn maritime_integration_200_years() {
    let mut s = Scenario::at_year(1);
    let f1 = s.add_faction("CoastalKingdom");
    s.modify_faction(f1, |fd| fd.treasury = 200.0);
    let r1 = s.add_region_with("Coast1", |rd| {
        rd.terrain = Terrain::Coast;
        rd.resources = vec![ResourceType::Fish, ResourceType::Salt];
    });
    let c1 = s.add_settlement_with("PortCity1", f1, r1, |sd| {
        sd.population_breakdown = PopulationBreakdown::from_total(500);
        sd.is_coastal = true;
        sd.resources = vec![ResourceType::Fish, ResourceType::Salt];
    });

    let f2 = s.add_faction("InlandKingdom");
    s.modify_faction(f2, |fd| fd.treasury = 200.0);
    let r2 = s.add_region("Plains");
    s.make_adjacent(r1, r2);
    let _c2 = s.add_settlement_with("InlandCity", f2, r2, |sd| {
        sd.population_breakdown = PopulationBreakdown::from_total(500);
        sd.resources = vec![ResourceType::Grain, ResourceType::Cattle];
    });

    let mut world = s.build();
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(EnvironmentSystem),
        Box::new(DemographicsSystem),
        Box::new(BuildingSystem),
        Box::new(EconomySystem),
        Box::new(MigrationSystem),
        Box::new(ConflictSystem),
    ];
    let _ = run(&mut world, &mut systems, SimConfig::new(1, 200, 42));

    // Verify no panics and coastal settlement still alive
    let coastal_pop = world.settlement(c1).population;
    assert!(
        coastal_pop > 0,
        "coastal port city should survive 200 years"
    );
}
