//! Culture system — migrated from `src/sim/culture.rs`.
//!
//! Three chained yearly systems (Update phase):
//! 1. `cultural_drift` — ruling culture gains, minorities decay, tension update
//! 2. `cultural_blending` — two qualifying cultures blend after coexistence timer
//! 3. `rebellion_check` — cultural rebellion at high tension + low stability
//!
//! One reaction system (Reactions phase):
//! 4. `handle_culture_events` — SettlementCaptured, RefugeesArrived, TradeRouteEstablished

use bevy_app::{App, Plugin};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    CultureState, Faction, FactionCore, Settlement, SettlementCore, SettlementCulture,
    SettlementTrade, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::MemberOf;
use crate::ecs::resources::{CultureRng, SimEntityMap};
use crate::ecs::schedule::{DomainSet, SimPhase, SimTick};
use crate::model::event::{EventKind, ParticipantRole};
use crate::sim::culture_names::generate_culture_entity_name;

// ---------------------------------------------------------------------------
// Signal: culture share adjustments
// ---------------------------------------------------------------------------
const CONQUEST_CULTURE_SHARE: f64 = 0.05;
// REFUGEE_CULTURE_FRACTION_MAX: used when RefugeesArrived carries source culture info
const REFUGEE_CULTURE_FRACTION_DEFAULT: f64 = 0.05;
const TRADE_ROUTE_CULTURE_SHARE: f64 = 0.01;

// ---------------------------------------------------------------------------
// Cultural drift
// ---------------------------------------------------------------------------
const DRIFT_BASE_MINORITY_LOSS: f64 = 0.02;
const DRIFT_TRADE_BONUS_MULTIPLIER: f64 = 0.005;
const DRIFT_PROSPERITY_THRESHOLD: f64 = 0.6;
const DRIFT_PROSPERITY_BONUS: f64 = 0.005;
const DRIFT_PURGE_THRESHOLD: f64 = 0.03;
const DOMINANT_CULTURE_MIN_FRACTION: f64 = 0.5;

// ---------------------------------------------------------------------------
// Cultural blending
// ---------------------------------------------------------------------------
const BLEND_QUALIFYING_SHARE: f64 = 0.30;
const BLEND_TIMER_THRESHOLD: u32 = 50;
const BLEND_CHANCE_PER_YEAR: f64 = 0.05;

// ---------------------------------------------------------------------------
// Cultural rebellion
// ---------------------------------------------------------------------------
const REBELLION_TENSION_THRESHOLD: f64 = 0.35;
const REBELLION_STABILITY_THRESHOLD: f64 = 0.5;
const REBELLION_BASE_CHANCE: f64 = 0.03;
const REBELLION_BASE_SUCCESS_CHANCE: f64 = 0.40;
const REBELLION_HIGH_TENSION_THRESHOLD: f64 = 0.6;
const REBELLION_HIGH_TENSION_BONUS: f64 = 0.20;
const REBELLION_LOW_STABILITY_THRESHOLD: f64 = 0.3;
const REBELLION_LOW_STABILITY_BONUS: f64 = 0.10;

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub struct CulturePlugin;

impl Plugin for CulturePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SimTick,
            (cultural_drift, cultural_blending, rebellion_check)
                .chain()
                .run_if(yearly)
                .in_set(DomainSet::Culture),
        );
        app.add_systems(SimTick, handle_culture_events.in_set(SimPhase::Reactions));
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

use super::helpers::{normalize_makeup, purge_below_threshold};

// ---------------------------------------------------------------------------
// System 1: Cultural drift (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn cultural_drift(
    settlements: Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            &SettlementTrade,
            Option<&MemberOf>,
        ),
        With<Settlement>,
    >,
    factions: Query<&FactionCore, With<Faction>>,
    cultures: Query<&CultureState>,
    mut culture_comp: Query<&mut SettlementCulture, With<Settlement>>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    for (entity, sim, core, trade, member_of) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }

        let Ok(mut culture) = culture_comp.get_mut(entity) else {
            continue;
        };

        if culture.culture_makeup.is_empty() {
            continue;
        }

        // Get ruling culture from faction
        let ruling_culture = member_of
            .and_then(|mo| factions.get(mo.0).ok())
            .and_then(|fc| fc.primary_culture);
        let Some(ruling_id) = ruling_culture else {
            continue;
        };

        // Count trade connections to same-culture settlements (simplified)
        let trade_connections = trade.trade_routes.len();

        // Drift: minorities lose, ruling gains
        let mut total_gain = 0.0;
        let minority_ids: Vec<u64> = culture
            .culture_makeup
            .keys()
            .copied()
            .filter(|&id| id != ruling_id)
            .collect();

        for mid in minority_ids {
            let share = culture.culture_makeup.get(&mid).copied().unwrap_or(0.0);

            // Look up culture resistance
            let resistance = entity_map
                .get_bevy(mid)
                .and_then(|e| cultures.get(e).ok())
                .map(|cs| cs.resistance)
                .unwrap_or(0.0);

            let trade_bonus = trade_connections as f64 * DRIFT_TRADE_BONUS_MULTIPLIER;
            let prosperity_bonus = if core.prosperity > DRIFT_PROSPERITY_THRESHOLD {
                DRIFT_PROSPERITY_BONUS
            } else {
                0.0
            };

            let loss =
                DRIFT_BASE_MINORITY_LOSS * (1.0 - resistance) + trade_bonus + prosperity_bonus;
            let actual_loss = loss.min(share);
            total_gain += actual_loss;

            if let Some(s) = culture.culture_makeup.get_mut(&mid) {
                *s -= actual_loss;
            }
        }

        // Add gain to ruling culture
        *culture.culture_makeup.entry(ruling_id).or_insert(0.0) += total_gain;

        // Normalize
        normalize_makeup(&mut culture.culture_makeup);

        // Purge below threshold
        purge_below_threshold(&mut culture.culture_makeup, DRIFT_PURGE_THRESHOLD);
        normalize_makeup(&mut culture.culture_makeup);

        // Update dominant culture
        let old_dominant = culture.dominant_culture;
        if let Some((&dom_id, &dom_share)) = culture
            .culture_makeup
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        {
            if dom_share >= DOMINANT_CULTURE_MIN_FRACTION {
                culture.dominant_culture = Some(dom_id);
            }
            culture.cultural_tension = 1.0 - dom_share;
        }

        // Emit cultural shift if dominant changed
        if culture.dominant_culture != old_dominant
            && let Some(new_dom_id) = culture.dominant_culture
            && let Some(new_dom_entity) = entity_map.get_bevy(new_dom_id)
        {
            commands.write(
                SimCommand::new(
                    SimCommandKind::CulturalShift {
                        settlement: entity,
                        new_culture: new_dom_entity,
                    },
                    EventKind::CulturalShift,
                    "Dominant culture changed",
                )
                .with_participant(entity, ParticipantRole::Location),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// System 2: Cultural blending (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn cultural_blending(
    mut rng: ResMut<CultureRng>,
    clock: Res<SimClock>,
    mut settlements: Query<
        (Entity, &SimEntity, &mut SettlementCore, &SettlementCulture),
        With<Settlement>,
    >,
    cultures: Query<&CultureState>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (entity, sim, mut core, culture) in settlements.iter_mut() {
        if !sim.is_alive() {
            continue;
        }

        // Find cultures with >= BLEND_QUALIFYING_SHARE
        let qualifying: Vec<(u64, f64)> = culture
            .culture_makeup
            .iter()
            .filter(|&(_, &share)| share >= BLEND_QUALIFYING_SHARE)
            .map(|(&id, &share)| (id, share))
            .collect();

        if qualifying.len() < 2 {
            core.blend_timer = 0;
            continue;
        }

        core.blend_timer += 1;
        if core.blend_timer < BLEND_TIMER_THRESHOLD {
            continue;
        }

        if rng.random_range(0.0..1.0) >= BLEND_CHANCE_PER_YEAR {
            continue;
        }

        // Take first two qualifying cultures
        let (id_a, _) = qualifying[0];
        let (id_b, _) = qualifying[1];

        // Get culture data for blending
        let state_a = entity_map.get_bevy(id_a).and_then(|e| cultures.get(e).ok());
        let state_b = entity_map.get_bevy(id_b).and_then(|e| cultures.get(e).ok());

        let (values, naming_style, resistance) = if let (Some(a), Some(b)) = (state_a, state_b) {
            // Pick one value from each parent
            let mut values = Vec::new();
            if let Some(v) = a.values.first() {
                values.push(v.clone());
            }
            if let Some(v) = b.values.last()
                && !values.contains(v)
            {
                values.push(v.clone());
            }
            (
                values,
                a.naming_style.clone(),
                (a.resistance + b.resistance) / 2.0,
            )
        } else {
            (Vec::new(), crate::model::NamingStyle::Nordic, 0.3)
        };

        let new_name = generate_culture_entity_name(rng);

        commands.write(
            SimCommand::new(
                SimCommandKind::BlendCultures {
                    settlement: entity,
                    parent_culture_a: id_a,
                    parent_culture_b: id_b,
                    new_name: new_name.clone(),
                    values,
                    naming_style,
                    resistance,
                },
                EventKind::CulturalShift,
                format!("{new_name} culture emerges in year {}", clock.time.year()),
            )
            .with_participant(entity, ParticipantRole::Location),
        );

        core.blend_timer = 0;
    }
}

// ---------------------------------------------------------------------------
// System 3: Rebellion check (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn rebellion_check(
    mut rng: ResMut<CultureRng>,
    clock: Res<SimClock>,
    settlements: Query<
        (Entity, &SimEntity, &SettlementCulture, Option<&MemberOf>),
        With<Settlement>,
    >,
    factions: Query<&FactionCore, With<Faction>>,
    cultures: Query<&CultureState>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;

    for (entity, sim, culture, member_of) in settlements.iter() {
        if !sim.is_alive() {
            continue;
        }
        if culture.cultural_tension < REBELLION_TENSION_THRESHOLD {
            continue;
        }

        let Some(mo) = member_of else { continue };
        let Ok(fcore) = factions.get(mo.0) else {
            continue;
        };

        if fcore.stability >= REBELLION_STABILITY_THRESHOLD {
            continue;
        }

        // Check if dominant culture differs from faction's
        let dom_differs = culture
            .dominant_culture
            .is_some_and(|dom| fcore.primary_culture.is_none_or(|pc| pc != dom));
        if !dom_differs {
            continue;
        }

        // Get rebel culture resistance
        let rebel_culture_id = culture.dominant_culture.unwrap_or(0);
        let resistance = entity_map
            .get_bevy(rebel_culture_id)
            .and_then(|e| cultures.get(e).ok())
            .map(|cs| cs.resistance)
            .unwrap_or(0.5);

        let rebellion_chance =
            REBELLION_BASE_CHANCE * culture.cultural_tension * (1.0 - fcore.stability) * resistance;

        if rng.random_range(0.0..1.0) >= rebellion_chance {
            continue;
        }

        // Determine success
        let mut success_chance = REBELLION_BASE_SUCCESS_CHANCE;
        if culture.cultural_tension > REBELLION_HIGH_TENSION_THRESHOLD {
            success_chance += REBELLION_HIGH_TENSION_BONUS;
        }
        if fcore.stability < REBELLION_LOW_STABILITY_THRESHOLD {
            success_chance += REBELLION_LOW_STABILITY_BONUS;
        }
        let succeeded = rng.random_range(0.0..1.0) < success_chance;

        commands.write(
            SimCommand::new(
                SimCommandKind::CulturalRebellion {
                    settlement: entity,
                    rebel_culture: rebel_culture_id,
                    succeeded,
                    new_faction_name: None,
                },
                EventKind::CulturalShift,
                format!(
                    "Cultural rebellion in year {} ({})",
                    clock.time.year(),
                    if succeeded { "succeeded" } else { "failed" }
                ),
            )
            .with_participant(entity, ParticipantRole::Location),
        );
    }
}

// ---------------------------------------------------------------------------
// Reaction system: Handle culture events
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn handle_culture_events(
    mut events: MessageReader<SimReactiveEvent>,
    mut cultures: Query<&mut SettlementCulture, With<Settlement>>,
    factions: Query<&FactionCore, With<Faction>>,
    membership: Query<&MemberOf, With<Settlement>>,
) {
    for event in events.read() {
        match event {
            SimReactiveEvent::SettlementCaptured {
                settlement,
                new_faction,
                ..
            } => {
                // Add conqueror's primary culture
                let conqueror_culture = factions
                    .get(*new_faction)
                    .ok()
                    .and_then(|fc| fc.primary_culture);
                if let Some(culture_id) = conqueror_culture
                    && let Ok(mut sc) = cultures.get_mut(*settlement)
                {
                    *sc.culture_makeup.entry(culture_id).or_insert(0.0) += CONQUEST_CULTURE_SHARE;
                    normalize_makeup(&mut sc.culture_makeup);
                }
            }

            SimReactiveEvent::RefugeesArrived {
                settlement,
                source_settlement,
                ..
            } => {
                // Spread source settlement's dominant culture to destination
                let source_culture = cultures
                    .get(*source_settlement)
                    .ok()
                    .and_then(|sc| sc.dominant_culture);
                if let Some(cid) = source_culture {
                    if let Ok(mut sc) = cultures.get_mut(*settlement) {
                        *sc.culture_makeup.entry(cid).or_insert(0.0) +=
                            REFUGEE_CULTURE_FRACTION_DEFAULT;
                        normalize_makeup(&mut sc.culture_makeup);
                    }
                } else if let Ok(mut sc) = cultures.get_mut(*settlement) {
                    // Fallback: use destination faction's primary culture
                    let culture_id = membership
                        .get(*settlement)
                        .ok()
                        .and_then(|mo| factions.get(mo.0).ok())
                        .and_then(|fc| fc.primary_culture);
                    if let Some(cid) = culture_id {
                        *sc.culture_makeup.entry(cid).or_insert(0.0) +=
                            REFUGEE_CULTURE_FRACTION_DEFAULT;
                        normalize_makeup(&mut sc.culture_makeup);
                    }
                }
            }

            SimReactiveEvent::TradeRouteEstablished {
                settlement_a,
                settlement_b,
                ..
            } => {
                // Get dominant culture from each settlement and add to the other
                let dom_a = cultures
                    .get(*settlement_a)
                    .ok()
                    .and_then(|sc| sc.dominant_culture);
                let dom_b = cultures
                    .get(*settlement_b)
                    .ok()
                    .and_then(|sc| sc.dominant_culture);

                if let Some(cid_a) = dom_a
                    && let Ok(mut sc_b) = cultures.get_mut(*settlement_b)
                {
                    *sc_b.culture_makeup.entry(cid_a).or_insert(0.0) += TRADE_ROUTE_CULTURE_SHARE;
                    normalize_makeup(&mut sc_b.culture_makeup);
                }
                if let Some(cid_b) = dom_b
                    && let Ok(mut sc_a) = cultures.get_mut(*settlement_a)
                {
                    *sc_a.culture_makeup.entry(cid_b).or_insert(0.0) += TRADE_ROUTE_CULTURE_SHARE;
                    normalize_makeup(&mut sc_a.culture_makeup);
                }
            }

            SimReactiveEvent::FactionSplit { .. } => {
                // New faction inherits parent's primary culture
                // This would require mutable access to FactionCore, which we handle
                // in the applicator instead. Skip here.
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
    use std::collections::BTreeMap;

    use super::*;
    use crate::ecs::app::build_sim_app_seeded;
    use crate::ecs::components::*;
    use crate::ecs::relationships::MemberOf;
    use crate::ecs::resources::SimEntityMap;
    use crate::ecs::test_helpers::tick_years;
    use crate::ecs::time::SimTime;
    use crate::model::NamingStyle;

    fn setup_app() -> App {
        let mut app = build_sim_app_seeded(100, 42);
        app.add_plugins(CulturePlugin);
        app
    }

    fn spawn_culture(app: &mut App, sim_id: u64, name: &str, resistance: f64) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Culture,
                CultureState {
                    values: vec![crate::model::CulturalValue::Martial],
                    naming_style: NamingStyle::Nordic,
                    resistance,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_faction(app: &mut App, sim_id: u64, culture_id: u64) -> Entity {
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
                    primary_culture: Some(culture_id),
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

    fn spawn_settlement_with_cultures(
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
                    culture_makeup: makeup,
                    dominant_culture: dominant,
                    cultural_tension: 0.0,
                    ..SettlementCulture::default()
                },
                SettlementDisease::default(),
                SettlementTrade::default(),
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
    fn ruling_culture_gains_share() {
        let mut app = setup_app();
        let _ruling = spawn_culture(&mut app, 1, "Northern", 0.0);
        let _minority = spawn_culture(&mut app, 2, "Southern", 0.0);
        let faction = spawn_faction(&mut app, 3, 1);

        let mut makeup = BTreeMap::new();
        makeup.insert(1, 0.6);
        makeup.insert(2, 0.4);
        let sett = spawn_settlement_with_cultures(&mut app, 4, faction, makeup);

        tick_years(&mut app, 10);

        let culture = app.world().get::<SettlementCulture>(sett).unwrap();
        let ruling_share = culture.culture_makeup.get(&1).copied().unwrap_or(0.0);
        let minority_share = culture.culture_makeup.get(&2).copied().unwrap_or(0.0);
        assert!(
            ruling_share > 0.6,
            "ruling culture should gain share, got {ruling_share}"
        );
        assert!(
            minority_share < 0.4,
            "minority should decay, got {minority_share}"
        );
    }

    #[test]
    fn minority_purged_below_threshold() {
        let mut app = setup_app();
        let _ruling = spawn_culture(&mut app, 1, "Northern", 0.0);
        let _minority = spawn_culture(&mut app, 2, "Southern", 0.0);
        let faction = spawn_faction(&mut app, 3, 1);

        let mut makeup = BTreeMap::new();
        makeup.insert(1, 0.95);
        makeup.insert(2, 0.05); // Just above purge threshold
        let sett = spawn_settlement_with_cultures(&mut app, 4, faction, makeup);

        tick_years(&mut app, 20);

        let culture = app.world().get::<SettlementCulture>(sett).unwrap();
        let minority_share = culture.culture_makeup.get(&2).copied().unwrap_or(0.0);
        assert!(
            minority_share < 0.05,
            "minority should have decayed, got {minority_share}"
        );
    }

    #[test]
    fn conquest_adds_conqueror_culture() {
        let mut app = setup_app();
        let _culture_a = spawn_culture(&mut app, 1, "Northern", 0.0);
        let _culture_b = spawn_culture(&mut app, 2, "Southern", 0.0);
        let faction_a = spawn_faction(&mut app, 3, 1);
        let faction_b = spawn_faction(&mut app, 4, 2);

        let mut makeup = BTreeMap::new();
        makeup.insert(1, 1.0);
        let sett = spawn_settlement_with_cultures(&mut app, 5, faction_a, makeup);

        // Inject SettlementCaptured event
        let event = SimReactiveEvent::SettlementCaptured {
            event_id: 1,
            settlement: sett,
            old_faction: Some(faction_a),
            new_faction: faction_b,
        };
        app.world_mut()
            .resource_mut::<bevy_ecs::message::Messages<SimReactiveEvent>>()
            .write(event);

        tick_years(&mut app, 1);

        let culture = app.world().get::<SettlementCulture>(sett).unwrap();
        let conqueror_share = culture.culture_makeup.get(&2).copied().unwrap_or(0.0);
        assert!(
            conqueror_share > 0.0,
            "conqueror culture should be added, got {conqueror_share}"
        );
    }
}
