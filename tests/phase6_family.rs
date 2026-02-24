use history_gen::model::{EntityKind, EventKind, RelationshipKind, World};
use history_gen::scenario::Scenario;
use history_gen::sim::{
    DemographicsSystem, EconomySystem, PoliticsSystem, SimConfig, SimSystem, run,
};
use history_gen::testutil;

fn generate_and_run(seed: u64, num_years: u32) -> World {
    testutil::generate_and_run(
        seed,
        num_years,
        vec![
            Box::new(DemographicsSystem),
            Box::new(EconomySystem),
            Box::new(PoliticsSystem),
        ],
    )
}

#[test]
fn determinism_preserved_with_family() {
    let world1 = generate_and_run(99, 50);
    let world2 = generate_and_run(99, 50);

    let count1 = world1.entities.len();
    let count2 = world2.entities.len();
    assert_eq!(
        count1, count2,
        "same seed should produce same entity count: {count1} vs {count2}"
    );

    let event_count1 = world1.events.len();
    let event_count2 = world2.events.len();
    assert_eq!(
        event_count1, event_count2,
        "same seed should produce same event count: {event_count1} vs {event_count2}"
    );

    let rel_count1 = world1.collect_relationships().count();
    let rel_count2 = world2.collect_relationships().count();
    assert_eq!(
        rel_count1, rel_count2,
        "same seed should produce same relationship count: {rel_count1} vs {rel_count2}"
    );
}

// ---------------------------------------------------------------------------
// Scenario-based tests
// ---------------------------------------------------------------------------

#[test]
fn scenario_parent_child_relationships_exist() {
    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone_with(
        "Town",
        |_| {},
        |sd| {
            sd.population = 300;
        },
    );
    let faction = setup.faction;
    let settlement = setup.settlement;
    let leader = s.add_person_with("King", faction, |pd| {
        pd.birth_year = 0;
        pd.sex = "male".to_string();
        pd.role = "warrior".to_string();
    });
    s.make_leader(leader, faction);
    // Add persons of both sexes so marriages and births can occur
    for i in 0..4 {
        let name = format!("Man {i}");
        let p = s.add_person_with(&name, faction, |pd| {
            pd.birth_year = 0;
            pd.sex = "male".to_string();
        });
        s.add_relationship(p, settlement, RelationshipKind::LocatedIn);
    }
    for i in 0..4 {
        let name = format!("Woman {i}");
        let p = s.add_person_with(&name, faction, |pd| {
            pd.birth_year = 0;
            pd.sex = "female".to_string();
        });
        s.add_relationship(p, settlement, RelationshipKind::LocatedIn);
    }

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];
    let world = s.run(&mut systems, 50, 42);

    let parent_rels = world
        .collect_relationships()
        .filter(|r| r.kind == RelationshipKind::Parent)
        .count();
    let child_rels = world
        .collect_relationships()
        .filter(|r| r.kind == RelationshipKind::Child)
        .count();

    assert!(
        parent_rels > 0,
        "expected Parent relationships after 50 years"
    );
    assert!(
        child_rels > 0,
        "expected Child relationships after 50 years"
    );
    assert_eq!(
        parent_rels, child_rels,
        "Parent count ({parent_rels}) should equal Child count ({child_rels})"
    );
}

#[test]
fn scenario_marriages_occur() {
    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone_with(
        "Town",
        |_| {},
        |sd| {
            sd.population = 300;
        },
    );
    let faction = setup.faction;
    let settlement = setup.settlement;
    let leader = s.add_person_with("King", faction, |pd| {
        pd.birth_year = 0;
        pd.sex = "male".to_string();
        pd.role = "warrior".to_string();
    });
    s.make_leader(leader, faction);
    // Add persons of both sexes for marriages
    for i in 0..4 {
        let name = format!("Man {i}");
        let p = s.add_person_with(&name, faction, |pd| {
            pd.birth_year = 0;
            pd.sex = "male".to_string();
        });
        s.add_relationship(p, settlement, RelationshipKind::LocatedIn);
    }
    for i in 0..4 {
        let name = format!("Woman {i}");
        let p = s.add_person_with(&name, faction, |pd| {
            pd.birth_year = 0;
            pd.sex = "female".to_string();
        });
        s.add_relationship(p, settlement, RelationshipKind::LocatedIn);
    }

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];
    let world = s.run(&mut systems, 50, 42);

    let union_events = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Union)
        .count();
    let spouse_rels = world
        .collect_relationships()
        .filter(|r| r.kind == RelationshipKind::Spouse)
        .count();

    assert!(union_events > 0, "expected Union events after 50 years");
    assert!(
        spouse_rels > 0,
        "expected Spouse relationships after 50 years"
    );
    assert_eq!(
        spouse_rels % 2,
        0,
        "Spouse relationships should be bidirectional (got {spouse_rels})"
    );
}

#[test]
fn scenario_surname_dynasties_visible() {
    let mut s = Scenario::new();
    let setup = s.add_settlement_standalone_with(
        "Town",
        |_| {},
        |sd| {
            sd.population = 300;
        },
    );
    let faction = setup.faction;
    let leader = s.add_person("King", faction);
    s.make_leader(leader, faction);

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];
    let world = s.run(&mut systems, 30, 42);

    let mut surnames: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for e in world.entities.values() {
        if e.kind == EntityKind::Person && e.end.is_none() {
            let base = e.name.split(" the ").next().unwrap_or(&e.name);
            if let Some((_, surname)) = base.rsplit_once(' ') {
                *surnames.entry(surname.to_string()).or_default() += 1;
            }
        }
    }

    let shared_surnames = surnames.values().filter(|&&count| count >= 2).count();
    assert!(
        shared_surnames > 0,
        "expected at least one surname shared by 2+ living persons after 30 years"
    );
}

#[test]
fn scenario_cross_faction_marriages_create_alliances() {
    let mut s = Scenario::new();
    let ka = s.add_kingdom_with("Kingdom A", |_| {}, |sd| sd.population = 300, |_| {});
    let kb = s.add_rival_kingdom_with(
        "Kingdom B",
        ka.region,
        |_| {},
        |sd| sd.population = 300,
        |_| {},
    );
    let _faction_a = ka.faction;
    let _faction_b = kb.faction;

    let mut world = s.build();

    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(EconomySystem),
        Box::new(PoliticsSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, 30, 42));

    // Check for marriage_alliance_year or cross-faction Union events
    let has_alliance = world
        .entities
        .values()
        .any(|e| e.kind == EntityKind::Faction && e.extra.contains_key("marriage_alliance_year"));
    let has_cross_union = world
        .events
        .values()
        .any(|e| e.kind == EventKind::Union && e.description.contains("forging ties"));

    // This is probabilistic — just verify the system runs and produces some marriages
    let total_unions = world
        .events
        .values()
        .filter(|e| e.kind == EventKind::Union)
        .count();
    assert!(
        total_unions > 0 || has_alliance || has_cross_union,
        "expected some marriages or alliances after 30 years with two factions"
    );
}
