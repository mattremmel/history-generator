use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::common::SimEntity;
use crate::ecs::components::dynamic::EcsActiveDisease;
use crate::ecs::components::{DiseaseState, SettlementDisease};
use crate::ecs::events::SimReactiveEvent;
use crate::model::effect::StateChange;
use crate::model::population::NUM_BRACKETS;

use super::applicator::ApplyCtx;

/// Start a plague: spawn Disease entity, insert EcsActiveDisease on settlement, emit PlagueStarted.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_start_plague(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    disease_name: &str,
    virulence: f64,
    lethality: f64,
    duration_years: u32,
    bracket_severity: &[f64; 8],
) {
    // Spawn Disease entity
    let disease_sim_id = ctx.id_gen.0.next_id();
    let mut severity_arr = [0.0f64; NUM_BRACKETS];
    severity_arr.copy_from_slice(bracket_severity);

    let disease_entity = world
        .spawn((
            SimEntity {
                id: disease_sim_id,
                name: disease_name.to_string(),
                origin: Some(ctx.clock_time),
                end: None,
            },
            crate::ecs::components::Disease,
            DiseaseState {
                virulence,
                lethality,
                duration_years,
                bracket_severity: severity_arr,
            },
        ))
        .id();
    ctx.entity_map.insert(disease_sim_id, disease_entity);

    // Insert EcsActiveDisease on settlement
    world.entity_mut(settlement).insert(EcsActiveDisease {
        disease_id: disease_sim_id,
        started: ctx.clock_time,
        infection_rate: virulence * 0.1, // initial infection rate
        peak_reached: false,
        total_deaths: 0,
    });

    // Clear disease risk markers
    if let Some(mut disease) = world.get_mut::<SettlementDisease>(settlement) {
        disease.disease_risk = Default::default();
    }

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "active_disease".to_string(),
            old_value: serde_json::json!(null),
            new_value: serde_json::json!(disease_name),
        },
    );

    ctx.emit(SimReactiveEvent::PlagueStarted {
        event_id,
        settlement,
    });
}

/// End a plague: remove EcsActiveDisease, grant immunity, emit PlagueEnded.
pub(crate) fn apply_end_plague(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
) {
    world.entity_mut(settlement).remove::<EcsActiveDisease>();

    // Grant post-plague immunity
    if let Some(mut disease) = world.get_mut::<SettlementDisease>(settlement) {
        disease.plague_immunity = 0.7;
    }

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "active_disease".to_string(),
            old_value: serde_json::json!("active"),
            new_value: serde_json::json!(null),
        },
    );

    ctx.emit(SimReactiveEvent::PlagueEnded {
        event_id,
        settlement,
    });
}

/// Spread a plague to a new settlement: insert EcsActiveDisease with initial infection rate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_spread_plague(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    to_settlement: Entity,
    disease_name: &str,
    virulence: f64,
    lethality: f64,
    duration_years: u32,
    bracket_severity: &[f64; 8],
) {
    // Only spread if target doesn't already have an active disease
    if world.get::<EcsActiveDisease>(to_settlement).is_some() {
        return;
    }

    // Spawn new Disease entity for this settlement's outbreak
    let disease_sim_id = ctx.id_gen.0.next_id();
    let mut severity_arr = [0.0f64; NUM_BRACKETS];
    severity_arr.copy_from_slice(bracket_severity);

    let disease_entity = world
        .spawn((
            SimEntity {
                id: disease_sim_id,
                name: disease_name.to_string(),
                origin: Some(ctx.clock_time),
                end: None,
            },
            crate::ecs::components::Disease,
            DiseaseState {
                virulence,
                lethality,
                duration_years,
                bracket_severity: severity_arr,
            },
        ))
        .id();
    ctx.entity_map.insert(disease_sim_id, disease_entity);

    // Insert EcsActiveDisease with reduced initial infection (transmission attenuation)
    world.entity_mut(to_settlement).insert(EcsActiveDisease {
        disease_id: disease_sim_id,
        started: ctx.clock_time,
        infection_rate: virulence * 0.05, // lower initial rate for spread
        peak_reached: false,
        total_deaths: 0,
    });

    ctx.record_effect(
        event_id,
        to_settlement,
        StateChange::PropertyChanged {
            field: "active_disease".to_string(),
            old_value: serde_json::json!(null),
            new_value: serde_json::json!(disease_name),
        },
    );

    ctx.emit(SimReactiveEvent::PlagueStarted {
        event_id,
        settlement: to_settlement,
    });
}
