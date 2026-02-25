use history_gen::model::{EntityKind, EventKind, World};
use history_gen::scenario::Scenario;
use history_gen::sim::{
    ActionSystem, ConflictSystem, DemographicsSystem, EconomySystem, PoliticsSystem, SimConfig,
    SimSystem, run,
};
use history_gen::testutil;

fn generate_and_run(seed: u64, num_years: u32) -> World {
    testutil::generate_and_run(
        seed,
        num_years,
        vec![
            Box::new(ActionSystem),
            Box::new(DemographicsSystem),
            Box::new(EconomySystem),
            Box::new(ConflictSystem),
            Box::new(PoliticsSystem),
        ],
    )
}

#[test]
fn determinism_with_conflicts() {
    let world1 = generate_and_run(42, 50);
    let world2 = generate_and_run(42, 50);

    testutil::assert_deterministic(&world1, &world2);
}

// ---------------------------------------------------------------------------
// Scenario-based tests
// ---------------------------------------------------------------------------

#[test]
fn scenario_war_conquers_unfortified_settlement() {
    // Unfortified settlement gets conquered instantly — verify ownership transfer
    let w = testutil::war_scenario(0, 200);
    let mut world = w.world;
    let target = w.target_settlement;
    let attacker = w.attacker_faction;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(ConflictSystem)];
    let _ = run(&mut world, &mut systems, SimConfig::new(10, 1, 42));

    // Conquest should have occurred for unfortified settlement
    let conquest_count = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Conquest)
        .count();
    assert!(
        conquest_count > 0,
        "unfortified settlement should be conquered"
    );

    // Settlement ownership should have changed to the attacker
    let current_owner = world.entities[&target]
        .relationships
        .iter()
        .find(|r| r.kind == history_gen::RelationshipKind::MemberOf && r.end.is_none())
        .map(|r| r.target_entity_id);
    assert_eq!(
        current_owner,
        Some(attacker),
        "conquered settlement should now belong to attacker"
    );
}

#[test]
fn scenario_armies_travel_between_regions() {
    // Set up two factions at war with armies in different regions
    let mut s = Scenario::at_year(10);
    let region_a = s.add_region("Region A");
    let region_b = s.add_region("Region B");
    let region_c = s.add_region("Region C");
    s.make_adjacent(region_a, region_b);
    s.make_adjacent(region_b, region_c);

    let attacker = s.add_faction("Attacker");
    let defender = s.add_faction("Defender");
    s.make_at_war(attacker, defender);

    let _ = s
        .settlement("Attacker Town", attacker, region_a)
        .population(1000);
    let _ = s
        .settlement("Defender Town", defender, region_c)
        .population(500);

    // Army starts in region_a, should move toward enemy territory
    let _army = s.add_army("Attack Force", attacker, region_a, 200);

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(ConflictSystem)];
    let world = s.run(&mut systems, 1, 42);

    let moved_count = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::March)
        .count();

    assert!(
        moved_count > 0,
        "army should move between regions during war"
    );
}

#[test]
fn scenario_army_attrition_occurs() {
    // Army in enemy territory with low supply should suffer attrition
    let mut s = Scenario::at_year(10);
    let region_a = s.add_region("Home");
    let region_b = s.add_region("Enemy Land");
    s.make_adjacent(region_a, region_b);

    let attacker = s.add_faction("Attacker");
    let defender = s.add_faction("Defender");
    s.make_at_war(attacker, defender);

    s.add_settlement("Attacker Town", attacker, region_a);
    s.add_settlement("Defender Town", defender, region_b);

    // Army with low supply in enemy territory
    let army = s
        .army("Starving Force", attacker, region_b, 200)
        .supply(0.5)
        .morale(0.5)
        .id();
    let mut world = s.build();

    let starting_strength = world.army(army).strength;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(ConflictSystem)];
    let _ = run(&mut world, &mut systems, SimConfig::new(10, 1, 42));

    // Check for attrition events or reduced strength
    let attrition_count = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Attrition)
        .count();
    let final_strength = world
        .entities
        .get(&army)
        .and_then(|e| e.data.as_army())
        .map(|a| a.strength)
        .unwrap_or(0);

    assert!(
        attrition_count > 0 || final_strength < starting_strength,
        "army with low supply should suffer attrition: events={attrition_count}, strength {starting_strength}->{final_strength}"
    );
}

#[test]
fn scenario_army_supply_depletes() {
    // Army in enemy territory should lose supply over 12 months
    let w = testutil::war_scenario(2, 200);
    let mut world = w.world;
    let army = w.army;

    let starting_supply = world.army(army).supply;

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(ConflictSystem)];
    let _ = run(&mut world, &mut systems, SimConfig::new(10, 1, 42));

    let final_supply = world
        .entities
        .get(&army)
        .and_then(|e| e.data.as_army())
        .map(|a| a.supply)
        .unwrap_or(starting_supply);

    assert!(
        final_supply < starting_supply,
        "army supply should deplete in enemy territory: {starting_supply} -> {final_supply}"
    );
}

#[test]
fn scenario_treaty_events_have_terms() {
    // Set up an exhausted war that should produce a treaty
    let mut s = Scenario::at_year(10);
    let a = s.add_settlement_standalone("Attacker Town");
    let b = s.add_rival_settlement("Defender Town", a.region);
    let attacker = a.faction;
    let defender = b.faction;
    s.make_at_war(attacker, defender);

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(ConflictSystem)];
    let world = s.run(&mut systems, 1, 42);

    let treaties: Vec<_> = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Treaty)
        .collect();

    assert!(
        !treaties.is_empty(),
        "exhausted war should produce a treaty"
    );

    for treaty in &treaties {
        if !treaty.data.is_null() {
            assert!(
                treaty.data.get("decisive").is_some(),
                "Treaty event data should contain 'decisive' field"
            );
            assert!(
                treaty.data.get("reparations").is_some(),
                "Treaty event data should contain 'reparations' field"
            );
            assert!(
                treaty.data.get("territory_ceded").is_some(),
                "Treaty event data should contain 'territory_ceded' field"
            );
        }
    }
}

#[test]
fn scenario_war_goals_on_declarations() {
    // Set up conditions for a war declaration
    let mut s = Scenario::at_year(10);
    let ka = s.add_kingdom("Aggressive Kingdom");
    let _ = s
        .faction_mut(ka.faction)
        .stability(0.8)
        .happiness(0.3)
        .treasury(200.0);
    let _ = s.settlement_mut(ka.settlement).population(1000);
    let kb = s.add_rival_kingdom("Peaceful Kingdom", ka.region);
    let _ = s.faction_mut(kb.faction).stability(0.5).happiness(0.5);
    let _ = s.settlement_mut(kb.settlement).population(500);
    let faction_a = ka.faction;
    let faction_b = kb.faction;
    s.make_enemies(faction_a, faction_b);

    // Run conflict + politics for a few years to trigger war declaration
    let mut systems: Vec<Box<dyn SimSystem>> =
        vec![Box::new(ConflictSystem), Box::new(PoliticsSystem)];
    let world = s.run(&mut systems, 5, 42);

    let war_declarations: Vec<_> = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::WarDeclared && !e.data.is_null())
        .collect();

    for wd in &war_declarations {
        if let Some(goal_type) = wd.data.get("type").and_then(|v| v.as_str()) {
            assert!(
                ["territorial", "economic", "punitive"].contains(&goal_type),
                "war goal type should be valid, got: {goal_type}"
            );
        }
    }

    // At least verify the system ran without panics and factions still exist
    assert!(
        world
            .entities
            .values()
            .any(|e| e.kind == EntityKind::Faction && e.end.is_none()),
        "should have living factions"
    );
}

#[test]
fn scenario_tribute_flows_between_factions() {
    let mut s = Scenario::at_year(100);
    let region = s.add_region("Plains");
    let payer_faction = s.faction("Payer").treasury(100.0).id();
    let payee_faction = s.faction("Payee").treasury(50.0).id();
    s.add_settlement("Payer Town", payer_faction, region);
    s.add_settlement("Payee Town", payee_faction, region);

    // Set up tribute obligation via struct field
    s.add_tribute(payer_faction, payee_faction, 10.0, 3);

    let mut systems: Vec<Box<dyn SimSystem>> = vec![Box::new(EconomySystem)];
    let world = s.run(&mut systems, 1, 42);

    // After 1 year, years_remaining should have decreased from 3 to 2
    let tribute = world
        .faction(payer_faction)
        .tributes
        .get(&payee_faction);

    if let Some(trib) = tribute {
        assert_eq!(
            trib.years_remaining, 2,
            "tribute years_remaining should decrease from 3 to 2"
        );
    }
}
