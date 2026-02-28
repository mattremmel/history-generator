//! Crime system — migrated from `src/sim/crime.rs`.
//!
//! Six chained yearly systems (Update phase):
//! 1. `update_crime_rates` — converge crime_rate toward target
//! 2. `update_guard_strength` — compute guard strength from treasury/fortifications
//! 3. `form_bandit_gangs` — spawn bandit factions in high-crime regions
//! 4. `raid_trade_routes` — bandits intercept nearby trade routes
//! 5. `raid_settlements` — bandits raid poorly-defended settlements
//! 6. `update_bandit_lifecycle` — bandit growth, disband, threat propagation
//!
//! One reaction system (Reactions phase):
//! 7. `handle_crime_events` — SettlementCaptured, WarEnded, PlagueEnded → crime spikes

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
    ArmyState, Faction, FactionCore, Settlement, SettlementCore, SettlementCrime,
    SettlementMilitary, SettlementTrade, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LocatedIn, MemberOf, RegionAdjacency};
use crate::ecs::resources::{SimEntityMap, SimRng};
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::model::entity_data::GovernmentType;
use crate::model::event::{EventKind, ParticipantRole};

// ---------------------------------------------------------------------------
// Crime rate computation
// ---------------------------------------------------------------------------
const CRIME_POVERTY_WEIGHT: f64 = 0.3;
const CRIME_OVERCROWDING_WEIGHT: f64 = 0.2;
const CRIME_INSTABILITY_WEIGHT: f64 = 0.2;
const CRIME_BANDIT_THREAT_WEIGHT: f64 = 0.2;
const CRIME_GUARD_REDUCTION: f64 = 0.5;
const CRIME_CONVERGENCE_RATE: f64 = 0.3;
const CRIME_PORT_BONUS: f64 = 0.1;

// ---------------------------------------------------------------------------
// Guard strength computation
// ---------------------------------------------------------------------------
const GUARD_COST_PER_SETTLEMENT: f64 = 2.0;
const GUARD_BASE_STRENGTH: f64 = 0.1;
const GUARD_TREASURY_FACTOR: f64 = 0.3;
const GUARD_FORTIFICATION_BONUS: f64 = 0.1;

// ---------------------------------------------------------------------------
// Bandit formation
// ---------------------------------------------------------------------------
const BANDIT_FORMATION_CRIME_THRESHOLD: f64 = 0.5;
const BANDIT_FORMATION_CHANCE: f64 = 0.08;

// ---------------------------------------------------------------------------
// Trade route raiding
// ---------------------------------------------------------------------------
const RAID_TRADE_BASE_CHANCE: f64 = 0.15;
const RAID_TRADE_STRENGTH_SCALE: f64 = 30.0;
const RAID_TRADE_MAX_CHANCE: f64 = 0.3;
const RAID_TRADE_SEVER_STRENGTH: u32 = 50;

// ---------------------------------------------------------------------------
// Settlement raiding
// ---------------------------------------------------------------------------
const RAID_SETTLEMENT_BASE_CHANCE: f64 = 0.10;
const RAID_SETTLEMENT_STRENGTH_SCALE: f64 = 30.0;
const RAID_SETTLEMENT_GUARD_THRESHOLD: f64 = 0.3;

// ---------------------------------------------------------------------------
// Bandit lifecycle
// ---------------------------------------------------------------------------
const BANDIT_GROWTH_CHANCE: f64 = 0.15;
const BANDIT_GROWTH_MIN: u32 = 5;
const BANDIT_GROWTH_MAX: u32 = 10;
const BANDIT_MAX_ARMY_STRENGTH: u32 = 80;
const BANDIT_DISBAND_CHANCE: f64 = 0.10;
const BANDIT_THREAT_PER_STRENGTH: f64 = 1.0 / 80.0;

// ---------------------------------------------------------------------------
// Signal deltas
// ---------------------------------------------------------------------------
const CRIME_SPIKE_CONQUEST: f64 = 0.15;
const CRIME_SPIKE_PLAGUE: f64 = 0.08;
const CRIME_SPIKE_DISASTER: f64 = 0.05;

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub fn add_crime_systems(app: &mut App) {
    app.add_systems(
        SimTick,
        (
            update_crime_rates,
            update_guard_strength,
            form_bandit_gangs,
            raid_trade_routes,
            raid_settlements,
            update_bandit_lifecycle,
        )
            .chain()
            .run_if(yearly)
            .in_set(SimPhase::Update),
    );
    app.add_systems(SimTick, handle_crime_events.in_set(SimPhase::Reactions));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_bandit_faction(core: &FactionCore) -> bool {
    core.government_type == GovernmentType::BanditClan
}

// ---------------------------------------------------------------------------
// System 1: Update crime rates (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn update_crime_rates(
    settlements: Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            &SettlementTrade,
            &SettlementMilitary,
            Option<&MemberOf>,
        ),
        With<Settlement>,
    >,
    factions: Query<&FactionCore, With<Faction>>,
    mut crimes: Query<&mut SettlementCrime, With<Settlement>>,
    mut commands: MessageWriter<SimCommand>,
) {
    for (entity, sim, core, trade, military, member_of) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }

        // Skip bandit settlements
        if let Some(mo) = member_of
            && let Ok(fcore) = factions.get(mo.0)
            && is_bandit_faction(fcore)
        {
            continue;
        }

        let Ok(mut crime) = crimes.get_mut(entity) else {
            continue;
        };

        // Overcrowding
        let capacity = core.capacity.max(1) as f64;
        let cap_ratio = core.population as f64 / capacity;
        let overcrowding = ((cap_ratio - 0.8).max(0.0) / 0.2).min(1.0);

        // Stability from faction
        let stability = member_of
            .and_then(|mo| factions.get(mo.0).ok())
            .map(|fc| fc.stability)
            .unwrap_or(0.5);

        // Port bonus
        let port_bonus = if trade.is_coastal && trade.trade_routes.iter().any(|_| true) {
            CRIME_PORT_BONUS
        } else {
            0.0
        };

        let guard_factor = military.guard_strength.min(1.0) * CRIME_GUARD_REDUCTION;
        let target = ((1.0 - core.prosperity) * CRIME_POVERTY_WEIGHT
            + overcrowding * CRIME_OVERCROWDING_WEIGHT
            + (1.0 - stability) * CRIME_INSTABILITY_WEIGHT
            + crime.bandit_threat * CRIME_BANDIT_THREAT_WEIGHT
            + port_bonus
            - guard_factor)
            .clamp(0.0, 1.0);

        // Apply guard reduction
        let guard_reduction = if let Some(mo) = member_of {
            if let Ok(fcore) = factions.get(mo.0) {
                if !is_bandit_faction(fcore) {
                    // Get guard_strength from SettlementMilitary (not available in this query)
                    // We'll use a simplified version here
                    0.0 // Guard reduction applied via separate system
                } else {
                    0.0
                }
            } else {
                0.0
            }
        } else {
            0.0
        };

        let adjusted_target = (target - guard_reduction).clamp(0.0, 1.0);

        let old = crime.crime_rate;
        crime.crime_rate += (adjusted_target - crime.crime_rate) * CRIME_CONVERGENCE_RATE;
        crime.crime_rate = crime.crime_rate.clamp(0.0, 1.0);

        if (crime.crime_rate - old).abs() > f64::EPSILON {
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity,
                field: "crime_rate".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(crime.crime_rate),
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// System 2: Update guard strength (yearly)
// ---------------------------------------------------------------------------

fn update_guard_strength(
    mut settlements: Query<
        (
            Entity,
            &SimEntity,
            &mut SettlementMilitary,
            Option<&MemberOf>,
        ),
        With<Settlement>,
    >,
    factions: Query<&FactionCore, With<Faction>>,
    mut commands: MessageWriter<SimCommand>,
) {
    for (entity, sim, mut military, member_of) in settlements.iter_mut() {
        if !sim.is_alive() {
            continue;
        }

        let treasury_factor = member_of
            .and_then(|mo| factions.get(mo.0).ok())
            .map(|fc| {
                if fc.treasury >= GUARD_COST_PER_SETTLEMENT {
                    (fc.treasury / 50.0).min(1.0)
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

        let new_strength = (GUARD_BASE_STRENGTH
            + treasury_factor * GUARD_TREASURY_FACTOR
            + military.fortification_level as f64 * GUARD_FORTIFICATION_BONUS)
            .clamp(0.0, 1.0);

        let old = military.guard_strength;
        military.guard_strength = new_strength;

        if (military.guard_strength - old).abs() > f64::EPSILON {
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity,
                field: "guard_strength".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(military.guard_strength),
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// System 3: Form bandit gangs (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn form_bandit_gangs(
    mut rng: ResMut<SimRng>,
    clock: Res<SimClock>,
    settlements: Query<
        (Entity, &SimEntity, &SettlementCrime, Option<&LocatedIn>),
        With<Settlement>,
    >,
    factions: Query<(&SimEntity, &FactionCore), With<Faction>>,
    armies: Query<(&ArmyState, Option<&LocatedIn>)>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (entity, sim, crime, loc) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }
        if crime.crime_rate < BANDIT_FORMATION_CRIME_THRESHOLD {
            continue;
        }
        let Some(loc) = loc else { continue };
        let region = loc.0;

        // Check for existing bandit in this region
        let has_bandit = armies.iter().any(|(army_state, army_loc)| {
            if army_loc.is_none_or(|l| l.0 != region) {
                return false;
            }
            entity_map
                .get_bevy(army_state.faction_id)
                .and_then(|fe| factions.get(fe).ok())
                .is_some_and(|(fsim, fcore)| fsim.is_alive() && is_bandit_faction(fcore))
        });

        if has_bandit {
            continue;
        }

        if rng.random_range(0.0..1.0) < BANDIT_FORMATION_CHANCE {
            commands.write(
                SimCommand::new(
                    SimCommandKind::FormBanditGang { region },
                    EventKind::BanditFormed,
                    format!("Bandits form in year {}", clock.time.year()),
                )
                .with_participant(entity, ParticipantRole::Location),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// System 4: Raid trade routes (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn raid_trade_routes(
    mut rng: ResMut<SimRng>,
    clock: Res<SimClock>,
    armies: Query<(Entity, &SimEntity, &ArmyState, Option<&LocatedIn>)>,
    factions: Query<(&SimEntity, &FactionCore), With<Faction>>,
    settlements: Query<
        (Entity, &SimEntity, &SettlementTrade, Option<&LocatedIn>),
        With<Settlement>,
    >,
    adjacency: Res<RegionAdjacency>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (army_entity, army_sim, army_state, army_loc) in armies.iter() {
        if !army_sim.is_alive() {
            continue;
        }
        let Some(army_loc) = army_loc else { continue };
        let army_region = army_loc.0;

        // Check if bandit faction
        let is_bandit = entity_map
            .get_bevy(army_state.faction_id)
            .and_then(|fe| factions.get(fe).ok())
            .is_some_and(|(fsim, fcore)| fsim.is_alive() && is_bandit_faction(fcore));
        if !is_bandit {
            continue;
        }

        let raid_chance = (RAID_TRADE_BASE_CHANCE * army_state.strength as f64
            / RAID_TRADE_STRENGTH_SCALE)
            .min(RAID_TRADE_MAX_CHANCE);

        // Find settlements in region or adjacent regions with trade routes
        for (sett_entity, sett_sim, trade, sett_loc) in settlements.iter() {
            if !sett_sim.is_alive() {
                continue;
            }
            let Some(sett_loc) = sett_loc else { continue };

            // Must be in same or adjacent region
            if sett_loc.0 != army_region && !adjacency.are_adjacent(sett_loc.0, army_region) {
                continue;
            }

            if trade.trade_routes.is_empty() {
                continue;
            }

            if rng.random_range(0.0..1.0) < raid_chance {
                // Pick the first trade route
                let target_sim_id = trade.trade_routes[0].target;
                let target_entity = entity_map.get_bevy(target_sim_id);
                let Some(target) = target_entity else {
                    continue;
                };

                let sever = army_state.strength >= RAID_TRADE_SEVER_STRENGTH;
                let bandit_faction = entity_map
                    .get_bevy(army_state.faction_id)
                    .unwrap_or(army_entity);

                commands.write(
                    SimCommand::new(
                        SimCommandKind::RaidTradeRoute {
                            bandit_faction,
                            settlement_a: sett_entity,
                            settlement_b: target,
                            sever,
                        },
                        EventKind::Raid,
                        format!("Bandits raid trade route in year {}", clock.time.year()),
                    )
                    .with_participant(sett_entity, ParticipantRole::Location),
                );
                break; // One raid per bandit army per year
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System 5: Raid settlements (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn raid_settlements(
    mut rng: ResMut<SimRng>,
    clock: Res<SimClock>,
    armies: Query<(Entity, &SimEntity, &ArmyState, Option<&LocatedIn>)>,
    factions: Query<(&SimEntity, &FactionCore), With<Faction>>,
    settlements: Query<
        (Entity, &SimEntity, &SettlementMilitary, Option<&LocatedIn>),
        With<Settlement>,
    >,
    adjacency: Res<RegionAdjacency>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (_army_entity, army_sim, army_state, army_loc) in armies.iter() {
        if !army_sim.is_alive() {
            continue;
        }
        let Some(army_loc) = army_loc else { continue };
        let army_region = army_loc.0;

        let is_bandit = entity_map
            .get_bevy(army_state.faction_id)
            .and_then(|fe| factions.get(fe).ok())
            .is_some_and(|(fsim, fcore)| fsim.is_alive() && is_bandit_faction(fcore));
        if !is_bandit {
            continue;
        }

        let raid_chance = RAID_SETTLEMENT_BASE_CHANCE * army_state.strength as f64
            / RAID_SETTLEMENT_STRENGTH_SCALE;

        for (sett_entity, sett_sim, military, sett_loc) in settlements.iter() {
            if !sett_sim.is_alive() {
                continue;
            }
            let Some(sett_loc) = sett_loc else { continue };

            if sett_loc.0 != army_region && !adjacency.are_adjacent(sett_loc.0, army_region) {
                continue;
            }

            if military.guard_strength >= RAID_SETTLEMENT_GUARD_THRESHOLD {
                continue;
            }

            if rng.random_range(0.0..1.0) < raid_chance {
                commands.write(
                    SimCommand::new(
                        SimCommandKind::BanditRaid {
                            settlement: sett_entity,
                        },
                        EventKind::Raid,
                        format!("Bandits raid settlement in year {}", clock.time.year()),
                    )
                    .with_participant(sett_entity, ParticipantRole::Location),
                );
                break; // One raid per army per year
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System 6: Update bandit lifecycle (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn update_bandit_lifecycle(
    mut rng: ResMut<SimRng>,
    armies: Query<(Entity, &SimEntity, &ArmyState, Option<&LocatedIn>)>,
    factions: Query<(&SimEntity, &FactionCore), With<Faction>>,
    mut settlements_crime: Query<&mut SettlementCrime, With<Settlement>>,
    settlement_locs: Query<(Entity, Option<&LocatedIn>), With<Settlement>>,
    adjacency: Res<RegionAdjacency>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    // Reset all bandit threats to zero
    for mut crime in settlements_crime.iter_mut() {
        crime.bandit_threat = 0.0;
    }

    for (army_entity, army_sim, army_state, army_loc) in armies.iter() {
        if !army_sim.is_alive() {
            continue;
        }
        let Some(army_loc) = army_loc else { continue };
        let army_region = army_loc.0;

        let bandit_faction = entity_map
            .get_bevy(army_state.faction_id)
            .and_then(|fe| factions.get(fe).ok());
        let is_bandit =
            bandit_faction.is_some_and(|(fsim, fcore)| fsim.is_alive() && is_bandit_faction(fcore));
        if !is_bandit {
            continue;
        }

        // Growth
        if army_state.strength < BANDIT_MAX_ARMY_STRENGTH
            && rng.random_range(0.0..1.0) < BANDIT_GROWTH_CHANCE
        {
            let growth = rng.random_range(BANDIT_GROWTH_MIN..=BANDIT_GROWTH_MAX);
            let new_strength = (army_state.strength + growth).min(BANDIT_MAX_ARMY_STRENGTH);
            commands.write(SimCommand::bookkeeping(SimCommandKind::SetField {
                entity: army_entity,
                field: "strength".to_string(),
                old_value: serde_json::json!(army_state.strength),
                new_value: serde_json::json!(new_strength),
            }));
        }

        // Disband check
        if rng.random_range(0.0..1.0) < BANDIT_DISBAND_CHANCE {
            let faction_entity = entity_map.get_bevy(army_state.faction_id);
            if let Some(fe) = faction_entity {
                commands.write(SimCommand::bookkeeping(SimCommandKind::DisbandBanditGang {
                    faction: fe,
                }));
            }
            continue;
        }

        // Propagate threat to settlements in same/adjacent regions
        let threat = army_state.strength as f64 * BANDIT_THREAT_PER_STRENGTH;
        for (sett_entity, sett_loc) in settlement_locs.iter() {
            let Some(sett_loc) = sett_loc else { continue };
            if (sett_loc.0 == army_region || adjacency.are_adjacent(sett_loc.0, army_region))
                && let Ok(mut crime) = settlements_crime.get_mut(sett_entity)
            {
                crime.bandit_threat = (crime.bandit_threat + threat).min(1.0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reaction system: Handle crime events
// ---------------------------------------------------------------------------

fn handle_crime_events(
    mut events: MessageReader<SimReactiveEvent>,
    mut settlements: Query<&mut SettlementCrime, With<Settlement>>,
) {
    for event in events.read() {
        match event {
            SimReactiveEvent::SettlementCaptured { settlement, .. } => {
                if let Ok(mut crime) = settlements.get_mut(*settlement) {
                    crime.crime_rate = (crime.crime_rate + CRIME_SPIKE_CONQUEST).min(1.0);
                }
            }
            SimReactiveEvent::PlagueEnded { settlement, .. } => {
                if let Ok(mut crime) = settlements.get_mut(*settlement) {
                    crime.crime_rate = (crime.crime_rate + CRIME_SPIKE_PLAGUE).min(1.0);
                }
            }
            SimReactiveEvent::DisasterStruck { settlement, .. } => {
                if let Ok(mut crime) = settlements.get_mut(*settlement) {
                    crime.crime_rate = (crime.crime_rate + CRIME_SPIKE_DISASTER).min(1.0);
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
    use crate::ecs::relationships::{MemberOf, RegionAdjacency};
    use crate::ecs::resources::SimEntityMap;
    use crate::ecs::test_helpers::tick_years;
    use crate::ecs::time::SimTime;
    use crate::model::Terrain;

    fn setup_app() -> App {
        let mut app = build_sim_app_seeded(100, 42);
        app.insert_resource(RegionAdjacency::new());
        add_crime_systems(&mut app);
        app
    }

    fn spawn_region(app: &mut App, sim_id: u64) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Region".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState {
                    terrain: Terrain::Plains,
                    ..RegionState::default()
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_faction(app: &mut App, sim_id: u64) -> Entity {
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
                    stability: 0.3,
                    treasury: 10.0,
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

    fn spawn_settlement(
        app: &mut App,
        sim_id: u64,
        faction: Entity,
        region: Entity,
        population: u32,
        prosperity: f64,
    ) -> Entity {
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
                    population,
                    prosperity,
                    capacity: 500,
                    ..SettlementCore::default()
                },
                SettlementCulture::default(),
                SettlementDisease::default(),
                SettlementTrade::default(),
                SettlementMilitary::default(),
                SettlementCrime::default(),
                SettlementEducation::default(),
                EcsSeasonalModifiers::default(),
                EcsBuildingBonuses::default(),
            ))
            .id();
        app.world_mut()
            .entity_mut(entity)
            .insert((LocatedIn(region), MemberOf(faction)));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    #[test]
    fn crime_rate_converges_for_low_prosperity() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1);
        let faction = spawn_faction(&mut app, 2);
        let sett = spawn_settlement(&mut app, 3, faction, region, 200, 0.1);

        tick_years(&mut app, 5);

        let crime = app.world().get::<SettlementCrime>(sett).unwrap();
        assert!(
            crime.crime_rate > 0.0,
            "low prosperity should raise crime, got {}",
            crime.crime_rate
        );
    }

    #[test]
    fn guard_strength_from_treasury() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1);
        let faction = spawn_faction(&mut app, 2);
        let sett = spawn_settlement(&mut app, 3, faction, region, 200, 0.5);

        tick_years(&mut app, 1);

        let military = app.world().get::<SettlementMilitary>(sett).unwrap();
        assert!(
            military.guard_strength > GUARD_BASE_STRENGTH,
            "guard strength should exceed base with treasury, got {}",
            military.guard_strength
        );
    }

    #[test]
    fn conquest_spikes_crime() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1);
        let faction = spawn_faction(&mut app, 2);
        let sett = spawn_settlement(&mut app, 3, faction, region, 200, 0.5);

        let event = SimReactiveEvent::SettlementCaptured {
            event_id: 1,
            settlement: sett,
            old_faction: Some(faction),
            new_faction: faction,
        };
        app.world_mut()
            .resource_mut::<bevy_ecs::message::Messages<SimReactiveEvent>>()
            .write(event);

        tick_years(&mut app, 1);

        let crime = app.world().get::<SettlementCrime>(sett).unwrap();
        assert!(
            crime.crime_rate > 0.0,
            "conquest should spike crime, got {}",
            crime.crime_rate
        );
    }
}
