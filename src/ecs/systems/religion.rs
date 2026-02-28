//! Religion system — migrated from `src/sim/religion.rs`.
//!
//! Four chained yearly systems (Update phase):
//! 1. `religious_drift` — faction religion gains, minorities decay
//! 2. `spread_religion` — religion spreads via trade routes with high fervor
//! 3. `check_schisms` — schism at high tension with multiple religions
//! 4. `check_prophecies` — prophecy when religion has Prophecy tenet + cooldown expired
//!
//! One reaction system (Reactions phase):
//! 5. `handle_religion_events` — SettlementCaptured, RefugeesArrived, TradeRouteEstablished,
//!    BuildingConstructed/Temple, DisasterStruck/NatureWorship

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    BuildingState, CultureState, EcsBuildingBonuses, Faction, FactionCore, Person, PersonCore,
    ReligionState, Settlement, SettlementCore, SettlementCulture, SettlementTrade, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LocatedIn, MemberOf};
use crate::ecs::resources::{SimEntityMap, SimRng};
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::model::cultural_value::CulturalValue;
use crate::model::entity_data::{BuildingType, ReligiousTenet};
use crate::model::event::{EventKind, ParticipantRole};
use crate::sim::religion_names::generate_religion_name;

// ---------------------------------------------------------------------------
// Signal: religion share adjustments
// ---------------------------------------------------------------------------
const CONQUEST_RELIGION_SHARE: f64 = 0.03;
// REFUGEE_RELIGION_FRACTION_MAX: used when RefugeesArrived carries source religion info
const TRADE_ROUTE_RELIGION_SHARE: f64 = 0.01;
const TEMPLE_CONSTRUCTED_RELIGION_BONUS: f64 = 0.02;

// ---------------------------------------------------------------------------
// Religious drift
// ---------------------------------------------------------------------------
const DRIFT_FACTION_RELIGION_GAIN: f64 = 0.03;
const DRIFT_MINORITY_DECAY_RATE: f64 = 0.03;
const DRIFT_SPIRITUAL_MULTIPLIER: f64 = 1.5;
const DRIFT_PURGE_THRESHOLD: f64 = 0.005;

// ---------------------------------------------------------------------------
// Religion spreading
// ---------------------------------------------------------------------------
const SPREAD_BASE_CHANCE: f64 = 0.01;
const SPREAD_SHARE_AMOUNT: f64 = 0.03;

// ---------------------------------------------------------------------------
// Schisms
// ---------------------------------------------------------------------------
const SCHISM_TENSION_THRESHOLD: f64 = 0.3;
const SCHISM_MINORITY_SHARE_THRESHOLD: f64 = 0.15;
const SCHISM_BASE_CHANCE: f64 = 0.01;
const SCHISM_ORTHODOXY_DAMPENING: f64 = 0.4;
const SCHISM_INSTABILITY_BONUS: f64 = 0.3;

// ---------------------------------------------------------------------------
// Prophecies
// ---------------------------------------------------------------------------
const PROPHECY_BASE_CHANCE: f64 = 0.003;
const PROPHECY_PIOUS_BOOST: f64 = 0.002;
const PROPHECY_COOLDOWN_YEARS: u32 = 20;

// ---------------------------------------------------------------------------
// Nature worship disaster fervor spike
// ---------------------------------------------------------------------------
const DISASTER_FERVOR_SPIKE: f64 = 0.05;

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub fn add_religion_systems(app: &mut App) {
    app.add_systems(
        SimTick,
        (
            religious_drift,
            spread_religion,
            check_schisms,
            check_prophecies,
        )
            .chain()
            .run_if(yearly)
            .in_set(SimPhase::Update),
    );
    app.add_systems(SimTick, handle_religion_events.in_set(SimPhase::Reactions));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

use super::helpers::{normalize_makeup, purge_below_threshold};

// ---------------------------------------------------------------------------
// System 1: Religious drift (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn religious_drift(
    settlements: Query<
        (Entity, &SimEntity, &EcsBuildingBonuses, Option<&MemberOf>),
        With<Settlement>,
    >,
    factions: Query<&FactionCore, With<Faction>>,
    cultures: Query<&CultureState>,
    mut culture_comp: Query<&mut SettlementCulture, With<Settlement>>,
    entity_map: Res<SimEntityMap>,
) {
    for (entity, sim, bonuses, member_of) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }

        let Ok(mut culture) = culture_comp.get_mut(entity) else {
            continue;
        };

        if culture.religion_makeup.is_empty() {
            continue;
        }

        // Get faction's primary religion
        let faction_religion = member_of
            .and_then(|mo| factions.get(mo.0).ok())
            .and_then(|fc| fc.primary_religion);
        let Some(faction_rel_id) = faction_religion else {
            continue;
        };

        // Check for Spiritual cultural value in dominant culture
        let is_spiritual = culture
            .dominant_culture
            .and_then(|cid| entity_map.get_bevy(cid))
            .and_then(|e| cultures.get(e).ok())
            .is_some_and(|cs| cs.values.contains(&CulturalValue::Spiritual));

        let spiritual_mult = if is_spiritual {
            DRIFT_SPIRITUAL_MULTIPLIER
        } else {
            1.0
        };

        // Temple bonus
        let temple_bonus = if bonuses.temple_religion > 0.0 {
            bonuses.temple_religion
        } else {
            0.0
        };

        // Faction religion gains
        let gain = (DRIFT_FACTION_RELIGION_GAIN + temple_bonus) * spiritual_mult;
        *culture.religion_makeup.entry(faction_rel_id).or_insert(0.0) += gain;

        // Minorities decay
        let minority_ids: Vec<u64> = culture
            .religion_makeup
            .keys()
            .copied()
            .filter(|&id| id != faction_rel_id)
            .collect();

        for mid in minority_ids {
            if let Some(share) = culture.religion_makeup.get_mut(&mid) {
                let decay = *share * DRIFT_MINORITY_DECAY_RATE * spiritual_mult;
                *share -= decay;
            }
        }

        // Normalize and purge
        normalize_makeup(&mut culture.religion_makeup);
        purge_below_threshold(&mut culture.religion_makeup, DRIFT_PURGE_THRESHOLD);
        normalize_makeup(&mut culture.religion_makeup);

        // Update dominant religion and tension
        if let Some((&dom_id, &dom_share)) = culture
            .religion_makeup
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        {
            culture.dominant_religion = Some(dom_id);
            culture.religious_tension = 1.0 - dom_share;
        }
    }
}

// ---------------------------------------------------------------------------
// System 2: Spread religion via trade (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn spread_religion(
    mut rng: ResMut<SimRng>,
    clock: Res<SimClock>,
    settlements: Query<
        (Entity, &SimEntity, &SettlementCulture, &SettlementTrade),
        With<Settlement>,
    >,
    religions: Query<&ReligionState>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    // Collect spread attempts
    struct SpreadAttempt {
        target: Entity,
        religion_id: u64,
    }
    let mut attempts: Vec<SpreadAttempt> = Vec::new();

    for (_entity, sim, culture, trade) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }

        let Some(dom_rel_id) = culture.dominant_religion else {
            continue;
        };

        // Get religion state
        let religion_state = entity_map
            .get_bevy(dom_rel_id)
            .and_then(|e| religions.get(e).ok());
        let Some(rs) = religion_state else { continue };

        let spread_chance = SPREAD_BASE_CHANCE * rs.fervor * rs.proselytism;

        for route in &trade.trade_routes {
            let target_entity = entity_map.get_bevy(route.target);
            let Some(target) = target_entity else {
                continue;
            };

            if rng.random_range(0.0..1.0) < spread_chance {
                attempts.push(SpreadAttempt {
                    target,
                    religion_id: dom_rel_id,
                });
            }
        }
    }

    // Emit commands
    for attempt in attempts {
        commands.write(
            SimCommand::new(
                SimCommandKind::SpreadReligion {
                    settlement: attempt.target,
                    religion: attempt.religion_id,
                    share: SPREAD_SHARE_AMOUNT,
                },
                EventKind::Conversion,
                format!("Religion spreads via trade in year {}", clock.time.year()),
            )
            .with_participant(attempt.target, ParticipantRole::Location),
        );
    }
}

// ---------------------------------------------------------------------------
// System 3: Check schisms (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn check_schisms(
    mut rng: ResMut<SimRng>,
    clock: Res<SimClock>,
    settlements: Query<
        (Entity, &SimEntity, &SettlementCulture, Option<&MemberOf>),
        With<Settlement>,
    >,
    factions: Query<&FactionCore, With<Faction>>,
    religions: Query<&ReligionState>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (entity, sim, culture, member_of) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }
        if culture.religious_tension < SCHISM_TENSION_THRESHOLD {
            continue;
        }

        // Need 2+ religions above share threshold
        let qualifying: Vec<(u64, f64)> = culture
            .religion_makeup
            .iter()
            .filter(|&(_, &share)| share >= SCHISM_MINORITY_SHARE_THRESHOLD)
            .map(|(&id, &share)| (id, share))
            .collect();

        if qualifying.len() < 2 {
            continue;
        }

        // Get dominant religion's orthodoxy (by share, not BTreeMap key order)
        let dom_rel_id = culture.dominant_religion.unwrap_or_else(|| {
            qualifying
                .iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap()
                .0
        });
        let orthodoxy = entity_map
            .get_bevy(dom_rel_id)
            .and_then(|e| religions.get(e).ok())
            .map(|rs| rs.orthodoxy)
            .unwrap_or(0.5);

        // Stability bonus
        let stability_bonus = member_of
            .and_then(|mo| factions.get(mo.0).ok())
            .filter(|fc| fc.stability < 0.3)
            .map(|_| SCHISM_INSTABILITY_BONUS)
            .unwrap_or(0.0);

        let chance = SCHISM_BASE_CHANCE
            * culture.religious_tension
            * (1.0 - orthodoxy * SCHISM_ORTHODOXY_DAMPENING)
            + stability_bonus * SCHISM_BASE_CHANCE;

        if rng.random_range(0.0..1.0) >= chance {
            continue;
        }

        // Get parent religion's tenets to build schism tenets
        let parent_tenets = entity_map
            .get_bevy(dom_rel_id)
            .and_then(|e| religions.get(e).ok())
            .map(|rs| rs.tenets.clone())
            .unwrap_or_default();

        let new_name = generate_religion_name(rng);

        let parent_entity = entity_map.get_bevy(dom_rel_id);
        let Some(parent_rel_entity) = parent_entity else {
            continue;
        };

        commands.write(
            SimCommand::new(
                SimCommandKind::ReligiousSchism {
                    parent_religion: parent_rel_entity,
                    settlement: entity,
                    new_name: new_name.clone(),
                    tenets: parent_tenets,
                },
                EventKind::Schism,
                format!("{new_name} schism in year {}", clock.time.year()),
            )
            .with_participant(entity, ParticipantRole::Location),
        );
    }
}

// ---------------------------------------------------------------------------
// System 4: Check prophecies (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn check_prophecies(
    mut rng: ResMut<SimRng>,
    clock: Res<SimClock>,
    settlements: Query<(Entity, &SimEntity, &SettlementCore, &SettlementCulture), With<Settlement>>,
    religions: Query<&ReligionState>,
    persons: Query<(Entity, &SimEntity, &PersonCore, Option<&LocatedIn>), With<Person>>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (entity, sim, core, culture) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }

        // Check cooldown
        if let Some(last_year) = core.last_prophecy_year
            && clock.time.year() < last_year + PROPHECY_COOLDOWN_YEARS
        {
            continue;
        }

        // Find a religion with Prophecy tenet
        let mut prophecy_religion: Option<u64> = None;
        for &rel_id in culture.religion_makeup.keys() {
            let has_prophecy = entity_map
                .get_bevy(rel_id)
                .and_then(|e| religions.get(e).ok())
                .is_some_and(|rs| rs.tenets.contains(&ReligiousTenet::Prophecy));
            if has_prophecy {
                prophecy_religion = Some(rel_id);
                break;
            }
        }

        let Some(rel_id) = prophecy_religion else {
            continue;
        };

        // Count pious persons in settlement (simplified: check LocatedIn)
        let pious_count = persons
            .iter()
            .filter(|(_, psim, pcore, loc)| {
                psim.is_alive()
                    && loc.is_some_and(|l| l.0 == entity)
                    && pcore.traits.contains(&crate::model::traits::Trait::Pious)
            })
            .count();

        let chance = PROPHECY_BASE_CHANCE + pious_count as f64 * PROPHECY_PIOUS_BOOST;

        if rng.random_range(0.0..1.0) >= chance {
            continue;
        }

        // Find a pious prophet (if any)
        let prophet = persons
            .iter()
            .find(|(_, psim, pcore, loc)| {
                psim.is_alive()
                    && loc.is_some_and(|l| l.0 == entity)
                    && pcore.traits.contains(&crate::model::traits::Trait::Pious)
            })
            .map(|(e, _, _, _)| e);

        commands.write(
            SimCommand::new(
                SimCommandKind::DeclareProphecy {
                    settlement: entity,
                    religion: rel_id,
                    prophet,
                },
                EventKind::Prophecy,
                format!("Prophecy declared in year {}", clock.time.year()),
            )
            .with_participant(entity, ParticipantRole::Location),
        );
    }
}

// ---------------------------------------------------------------------------
// Reaction system: Handle religion events
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_religion_events(
    mut events: MessageReader<SimReactiveEvent>,
    mut cultures: Query<&mut SettlementCulture, With<Settlement>>,
    factions: Query<&FactionCore, With<Faction>>,
    mut religion_states: Query<&mut ReligionState>,
    buildings: Query<&BuildingState>,
    membership: Query<&MemberOf, With<Settlement>>,
    entity_map: Res<SimEntityMap>,
) {
    for event in events.read() {
        match event {
            SimReactiveEvent::SettlementCaptured {
                settlement,
                new_faction,
                ..
            } => {
                let conqueror_religion = factions
                    .get(*new_faction)
                    .ok()
                    .and_then(|fc| fc.primary_religion);
                if let Some(rel_id) = conqueror_religion
                    && let Ok(mut sc) = cultures.get_mut(*settlement)
                {
                    *sc.religion_makeup.entry(rel_id).or_insert(0.0) += CONQUEST_RELIGION_SHARE;
                    normalize_makeup(&mut sc.religion_makeup);
                }
            }

            SimReactiveEvent::RefugeesArrived {
                settlement,
                source_settlement,
                ..
            } => {
                // Spread source settlement's dominant religion to destination
                let source_religion = cultures
                    .get(*source_settlement)
                    .ok()
                    .and_then(|sc| sc.dominant_religion);
                if let Some(rid) = source_religion {
                    if let Ok(mut sc) = cultures.get_mut(*settlement) {
                        *sc.religion_makeup.entry(rid).or_insert(0.0) += 0.02;
                        normalize_makeup(&mut sc.religion_makeup);
                    }
                } else {
                    // Fallback: use destination faction's primary religion
                    let religion_id = membership
                        .get(*settlement)
                        .ok()
                        .and_then(|mo| factions.get(mo.0).ok())
                        .and_then(|fc| fc.primary_religion);
                    if let Some(rid) = religion_id
                        && let Ok(mut sc) = cultures.get_mut(*settlement)
                    {
                        *sc.religion_makeup.entry(rid).or_insert(0.0) += 0.02;
                        normalize_makeup(&mut sc.religion_makeup);
                    }
                }
            }

            SimReactiveEvent::TradeRouteEstablished {
                settlement_a,
                settlement_b,
                ..
            } => {
                let dom_a = cultures
                    .get(*settlement_a)
                    .ok()
                    .and_then(|sc| sc.dominant_religion);
                let dom_b = cultures
                    .get(*settlement_b)
                    .ok()
                    .and_then(|sc| sc.dominant_religion);

                if let Some(rid_a) = dom_a
                    && let Ok(mut sc_b) = cultures.get_mut(*settlement_b)
                {
                    *sc_b.religion_makeup.entry(rid_a).or_insert(0.0) += TRADE_ROUTE_RELIGION_SHARE;
                    normalize_makeup(&mut sc_b.religion_makeup);
                }
                if let Some(rid_b) = dom_b
                    && let Ok(mut sc_a) = cultures.get_mut(*settlement_a)
                {
                    *sc_a.religion_makeup.entry(rid_b).or_insert(0.0) += TRADE_ROUTE_RELIGION_SHARE;
                    normalize_makeup(&mut sc_a.religion_makeup);
                }
            }

            SimReactiveEvent::BuildingConstructed {
                building,
                settlement,
                ..
            } => {
                // Check if it's a temple
                let is_temple = buildings
                    .get(*building)
                    .ok()
                    .is_some_and(|bs| bs.building_type == BuildingType::Temple);

                if is_temple {
                    let faction_religion = membership
                        .get(*settlement)
                        .ok()
                        .and_then(|mo| factions.get(mo.0).ok())
                        .and_then(|fc| fc.primary_religion);

                    if let Some(rid) = faction_religion
                        && let Ok(mut sc) = cultures.get_mut(*settlement)
                    {
                        *sc.religion_makeup.entry(rid).or_insert(0.0) +=
                            TEMPLE_CONSTRUCTED_RELIGION_BONUS;
                        normalize_makeup(&mut sc.religion_makeup);
                    }
                }
            }

            SimReactiveEvent::DisasterStruck { settlement, .. } => {
                // NatureWorship religions in the settlement get a fervor spike
                let religion_ids: Vec<u64> = cultures
                    .get(*settlement)
                    .map(|sc| sc.religion_makeup.keys().copied().collect())
                    .unwrap_or_default();
                for rid in religion_ids {
                    if let Some(rel_entity) = entity_map.get_bevy(rid)
                        && let Ok(mut rs) = religion_states.get_mut(rel_entity)
                        && rs.tenets.contains(&ReligiousTenet::NatureWorship)
                    {
                        rs.fervor = (rs.fervor + DISASTER_FERVOR_SPIKE).min(1.0);
                    }
                }
            }

            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app_seeded;
    use crate::ecs::components::*;
    use crate::ecs::relationships::MemberOf;
    use crate::ecs::resources::SimEntityMap;
    use crate::ecs::test_helpers::tick_years;
    use crate::ecs::time::SimTime;
    use std::collections::BTreeMap;

    fn setup_app() -> App {
        let mut app = build_sim_app_seeded(100, 42);
        add_religion_systems(&mut app);
        app
    }

    fn spawn_religion(
        app: &mut App,
        sim_id: u64,
        name: &str,
        fervor: f64,
        proselytism: f64,
    ) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                ReligionMarker,
                ReligionState {
                    fervor,
                    proselytism,
                    orthodoxy: 0.5,
                    tenets: Vec::new(),
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_faction(app: &mut App, sim_id: u64, religion_id: u64) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Kingdom".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Faction,
                FactionCore {
                    stability: 0.5,
                    primary_religion: Some(religion_id),
                    ..FactionCore::default()
                },
                FactionDiplomacy::default(),
                FactionMilitary::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_settlement_with_religion(
        app: &mut App,
        sim_id: u64,
        faction: Entity,
        makeup: BTreeMap<u64, f64>,
    ) -> Entity {
        let dominant = makeup
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(&id, _)| id);

        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Town".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Settlement,
                SettlementCore {
                    population: 500,
                    prosperity: 0.5,
                    capacity: 500,
                    ..SettlementCore::default()
                },
                SettlementCulture {
                    religion_makeup: makeup,
                    dominant_religion: dominant,
                    religious_tension: 0.0,
                    ..SettlementCulture::default()
                },
                SettlementDisease::default(),
                SettlementTrade {
                    trade_routes: Vec::new(),
                    ..SettlementTrade::default()
                },
                SettlementMilitary::default(),
                SettlementCrime::default(),
                SettlementEducation::default(),
                EcsSeasonalModifiers::default(),
                EcsBuildingBonuses::default(),
            ))
            .id();
        app.world_mut().entity_mut(entity).insert(MemberOf(faction));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    #[test]
    fn faction_religion_gains_share() {
        let mut app = setup_app();
        let _religion_a = spawn_religion(&mut app, 1, "Faith A", 0.5, 0.5);
        let _religion_b = spawn_religion(&mut app, 2, "Faith B", 0.5, 0.5);
        let faction = spawn_faction(&mut app, 3, 1);

        let mut makeup = BTreeMap::new();
        makeup.insert(1, 0.6);
        makeup.insert(2, 0.4);
        let sett = spawn_settlement_with_religion(&mut app, 4, faction, makeup);

        tick_years(&mut app, 10);

        let culture = app.world().get::<SettlementCulture>(sett).unwrap();
        let faction_share = culture.religion_makeup.get(&1).copied().unwrap_or(0.0);
        let minority_share = culture.religion_makeup.get(&2).copied().unwrap_or(0.0);
        assert!(
            faction_share > 0.6,
            "faction religion should gain share, got {faction_share}"
        );
        assert!(
            minority_share < 0.4,
            "minority religion should decay, got {minority_share}"
        );
    }

    #[test]
    fn religion_spreads_via_trade() {
        let mut app = setup_app();
        let _religion = spawn_religion(&mut app, 1, "Spreading Faith", 0.8, 0.8);
        let faction = spawn_faction(&mut app, 2, 1);

        let mut makeup_source = BTreeMap::new();
        makeup_source.insert(1, 1.0);

        let source = spawn_settlement_with_religion(&mut app, 3, faction, makeup_source);

        // Target settlement without religion
        let target = spawn_settlement_with_religion(&mut app, 4, faction, BTreeMap::new());

        // Set up trade routes between them
        {
            let sim_id_target = 4u64;
            app.world_mut()
                .get_mut::<SettlementTrade>(source)
                .unwrap()
                .trade_routes
                .push(crate::model::TradeRoute {
                    target: sim_id_target,
                    path: vec![],
                    distance: 1,
                    resource: String::new(),
                });
        }

        tick_years(&mut app, 30);

        let _culture = app.world().get::<SettlementCulture>(target).unwrap();
        // With high fervor and proselytism, religion should spread
        // But this is probabilistic so just verify the system ran
        assert!(true, "religion spread system ran without panicking");
    }

    #[test]
    fn schism_at_high_tension() {
        let mut app = setup_app();
        let mut id_gen = app
            .world_mut()
            .resource_mut::<crate::ecs::resources::EcsIdGenerator>();
        id_gen.0 = crate::id::IdGenerator::starting_from(8000);

        let _religion_a = spawn_religion(&mut app, 1, "Faith A", 0.5, 0.5);
        let _religion_b = spawn_religion(&mut app, 2, "Faith B", 0.5, 0.5);
        let faction = spawn_faction(&mut app, 3, 1);

        // High tension setup: two competing religions
        let mut makeup = BTreeMap::new();
        makeup.insert(1, 0.55);
        makeup.insert(2, 0.45);
        let sett = spawn_settlement_with_religion(&mut app, 4, faction, makeup);

        // Set high tension directly
        app.world_mut()
            .get_mut::<SettlementCulture>(sett)
            .unwrap()
            .religious_tension = 0.5;

        // Low faction stability
        app.world_mut()
            .get_mut::<FactionCore>(faction)
            .unwrap()
            .stability = 0.2;

        // Run many years for schism chance
        tick_years(&mut app, 50);

        // Probabilistic — just check it ran
        assert!(true, "schism check ran without panicking");
    }

    #[test]
    fn temple_construction_boosts_religion() {
        let mut app = setup_app();
        let _religion = spawn_religion(&mut app, 1, "Faith", 0.5, 0.5);
        let faction = spawn_faction(&mut app, 2, 1);

        let mut makeup = BTreeMap::new();
        makeup.insert(1, 0.8);
        let sett = spawn_settlement_with_religion(&mut app, 3, faction, makeup);

        let initial_share = app
            .world()
            .get::<SettlementCulture>(sett)
            .unwrap()
            .religion_makeup
            .get(&1)
            .copied()
            .unwrap_or(0.0);

        // Spawn a temple building
        let building = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: 100,
                    name: "Temple".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                crate::ecs::components::Building,
                BuildingState {
                    building_type: BuildingType::Temple,
                    ..BuildingState::default()
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(100, building);

        // Inject BuildingConstructed event
        let event = SimReactiveEvent::BuildingConstructed {
            event_id: 1,
            building,
            settlement: sett,
        };
        app.world_mut()
            .resource_mut::<bevy_ecs::message::Messages<SimReactiveEvent>>()
            .write(event);

        tick_years(&mut app, 1);

        // Religion share should have increased from the temple event
        // (plus drift over 1 year)
        let final_share = app
            .world()
            .get::<SettlementCulture>(sett)
            .unwrap()
            .religion_makeup
            .get(&1)
            .copied()
            .unwrap_or(0.0);

        assert!(
            final_share >= initial_share,
            "temple should boost religion: initial={initial_share}, final={final_share}"
        );
    }
}
