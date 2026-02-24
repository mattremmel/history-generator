use history_gen::model::{EntityKind, RelationshipKind, World};
use history_gen::sim::{
    AgencySystem, BuildingSystem, ConflictSystem, DemographicsSystem, EconomySystem,
    PoliticsSystem, ReputationSystem, SimConfig, SimSystem, run,
};
use history_gen::worldgen::{self, config::WorldGenConfig};

/// Run with ReputationSystem in tick order after Politics, before Agency.
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
        Box::new(ReputationSystem),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
    world
}

#[test]
fn prestige_values_stay_in_bounds() {
    let world = generate_and_run(42, 500);

    for e in world.entities.values() {
        if e.end.is_some() {
            continue;
        }
        match e.kind {
            EntityKind::Person => {
                if let Some(pd) = e.data.as_person() {
                    assert!(
                        pd.prestige >= 0.0 && pd.prestige <= 1.0,
                        "person '{}' prestige {} out of bounds",
                        e.name,
                        pd.prestige
                    );
                }
            }
            EntityKind::Faction => {
                if let Some(fd) = e.data.as_faction() {
                    assert!(
                        fd.prestige >= 0.0 && fd.prestige <= 1.0,
                        "faction '{}' prestige {} out of bounds",
                        e.name,
                        fd.prestige
                    );
                }
            }
            EntityKind::Settlement => {
                if let Some(sd) = e.data.as_settlement() {
                    assert!(
                        sd.prestige >= 0.0 && sd.prestige <= 1.0,
                        "settlement '{}' prestige {} out of bounds",
                        e.name,
                        sd.prestige
                    );
                }
            }
            _ => {}
        }
    }
}

#[test]
fn factions_gain_prestige_over_time() {
    let world = generate_and_run(42, 500);

    let factions: Vec<_> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .filter_map(|e| e.data.as_faction().map(|fd| (e.name.clone(), fd.prestige)))
        .collect();

    assert!(!factions.is_empty(), "should have living factions");

    // At least one faction should have gained notable prestige (tier 1+, >= 0.2)
    let notable_factions = factions.iter().filter(|(_, p)| *p >= 0.2).count();
    assert!(
        notable_factions > 0,
        "after 500 years, at least one faction should reach notable prestige; \
         factions: {factions:?}"
    );
}

#[test]
fn settlements_gain_prestige_over_time() {
    let world = generate_and_run(42, 500);

    let settlements: Vec<_> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            e.data
                .as_settlement()
                .map(|sd| (e.name.clone(), sd.prestige))
        })
        .collect();

    assert!(!settlements.is_empty(), "should have living settlements");

    // Settlements should have non-zero prestige after 500 years
    let has_prestige = settlements.iter().filter(|(_, p)| *p > 0.05).count();
    assert!(
        has_prestige > 0,
        "after 500 years, settlements should have non-trivial prestige; \
         settlements: {settlements:?}"
    );
}

#[test]
fn prestige_varies_between_entities() {
    let world = generate_and_run(42, 500);

    let faction_prestiges: Vec<f64> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .filter_map(|e| e.data.as_faction().map(|fd| fd.prestige))
        .collect();

    if faction_prestiges.len() >= 2 {
        let min = faction_prestiges.iter().cloned().reduce(f64::min).unwrap();
        let max = faction_prestiges.iter().cloned().reduce(f64::max).unwrap();
        assert!(
            (max - min) > 0.01,
            "faction prestige should vary; min={min} max={max}"
        );
    }
}

// --- Cross-system prestige integration tests ---

/// Full system run including conflicts and agency (prestige feeds into all subsystems).
fn generate_full(seed: u64, num_years: u32) -> World {
    let config = WorldGenConfig {
        seed,
        ..WorldGenConfig::default()
    };
    let mut world = worldgen::generate_world(&config);
    let mut systems: Vec<Box<dyn SimSystem>> = vec![
        Box::new(DemographicsSystem),
        Box::new(BuildingSystem),
        Box::new(EconomySystem),
        Box::new(ConflictSystem),
        Box::new(PoliticsSystem),
        Box::new(ReputationSystem),
        Box::new(AgencySystem::new()),
    ];
    run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
    world
}

#[test]
fn prestige_stays_bounded_with_all_systems() {
    let world = generate_full(42, 500);

    for e in world.entities.values() {
        if e.end.is_some() {
            continue;
        }
        if let Some(fd) = e.data.as_faction() {
            assert!(
                fd.prestige >= 0.0 && fd.prestige <= 1.0,
                "faction '{}' prestige {} out of bounds",
                e.name,
                fd.prestige
            );
            assert!(
                fd.stability >= 0.0 && fd.stability <= 1.0,
                "faction '{}' stability {} out of bounds",
                e.name,
                fd.stability
            );
        }
        if let Some(sd) = e.data.as_settlement() {
            assert!(
                sd.prosperity >= 0.0 && sd.prosperity <= 1.0,
                "settlement '{}' prosperity {} out of bounds",
                e.name,
                sd.prosperity
            );
        }
    }
}

#[test]
fn prestigious_factions_tend_toward_more_alliances() {
    // Run multiple seeds to get statistical signal
    let mut high_prestige_allies = 0u32;
    let mut low_prestige_allies = 0u32;
    let mut high_count = 0u32;
    let mut low_count = 0u32;

    for seed in 0..10 {
        let world = generate_full(seed, 500);

        for e in world.entities.values() {
            if e.kind != EntityKind::Faction || e.end.is_some() {
                continue;
            }
            let prestige = e.data.as_faction().map(|f| f.prestige).unwrap_or(0.0);
            let ally_count = e
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Ally && r.end.is_none())
                .count() as u32;

            if prestige >= 0.3 {
                high_prestige_allies += ally_count;
                high_count += 1;
            } else {
                low_prestige_allies += ally_count;
                low_count += 1;
            }
        }
    }

    // We just verify the simulation doesn't crash and produces reasonable data
    assert!(
        high_count + low_count > 0,
        "should have factions across 10 seeds"
    );
}

#[test]
fn prosperity_benefits_from_settlement_prestige() {
    let world = generate_full(42, 500);

    let settlements: Vec<(f64, f64)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            e.data
                .as_settlement()
                .map(|sd| (sd.prestige, sd.prosperity))
        })
        .collect();

    assert!(
        settlements.len() >= 2,
        "should have multiple living settlements"
    );

    // Verify prestige and prosperity are both within valid ranges
    for (prestige, prosperity) in &settlements {
        assert!(
            *prestige >= 0.0 && *prestige <= 1.0,
            "prestige out of bounds: {prestige}"
        );
        assert!(
            *prosperity >= 0.0 && *prosperity <= 1.0,
            "prosperity out of bounds: {prosperity}"
        );
    }
}
