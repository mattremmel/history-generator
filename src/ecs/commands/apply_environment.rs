use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::{
    BuildingState, EcsActiveDisaster, GeographicFeature, GeographicFeatureState, SimEntity,
};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::LocatedIn;
use crate::model::effect::StateChange;
use crate::model::entity_data::DisasterType;

use super::applicator::ApplyCtx;

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_trigger_disaster(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    disaster_type: DisasterType,
    severity: f64,
    pop_loss_frac: f64,
    building_damage: f64,
    prosperity_hit: f64,
    sever_trade: bool,
    create_feature: &Option<(String, crate::model::FeatureType)>,
) {
    // Apply population loss
    if let Some(mut core) = world.get_mut::<crate::ecs::components::SettlementCore>(settlement) {
        let old_pop = core.population;
        let deaths = (core.population as f64 * pop_loss_frac) as u32;
        core.population = core.population.saturating_sub(deaths);
        let new_pop = core.population;
        let pop = core.population;
        core.population_breakdown.scale_to(pop);

        if old_pop != new_pop {
            ctx.record_effect(
                event_id,
                settlement,
                StateChange::PropertyChanged {
                    field: "population".to_string(),
                    old_value: serde_json::json!(old_pop),
                    new_value: serde_json::json!(new_pop),
                },
            );
        }

        // Prosperity hit
        let old_prosperity = core.prosperity;
        core.prosperity = (core.prosperity - prosperity_hit * severity).max(0.0);
        let new_prosperity = core.prosperity;

        if (old_prosperity - new_prosperity).abs() > f64::EPSILON {
            ctx.record_effect(
                event_id,
                settlement,
                StateChange::PropertyChanged {
                    field: "prosperity".to_string(),
                    old_value: serde_json::json!(old_prosperity),
                    new_value: serde_json::json!(new_prosperity),
                },
            );
        }
    }

    // Building damage — damage buildings of appropriate types in this settlement
    if building_damage > 0.0 {
        apply_building_damage_from_disaster(
            ctx,
            world,
            event_id,
            settlement,
            building_damage,
            &disaster_type,
        );
    }

    // Sever trade routes — actual SeverTradeRoute commands are emitted by
    // the environment system separately when needed.
    let _ = sever_trade;

    // Create geographic feature for severe disasters
    if let Some((name, feature_type)) = create_feature
        && let Some(located_in) = world.get::<LocatedIn>(settlement)
    {
        let region = located_in.0;
        let feature_id = ctx.id_gen.0.next_id();
        let feature_entity = world
            .spawn((
                SimEntity {
                    id: feature_id,
                    name: name.clone(),
                    origin: Some(ctx.clock_time),
                    end: None,
                },
                GeographicFeature,
                GeographicFeatureState {
                    feature_type: feature_type.clone(),
                    x: 0.0,
                    y: 0.0,
                },
                LocatedIn(region),
            ))
            .id();
        ctx.entity_map.insert(feature_id, feature_entity);
    }

    ctx.emit(SimReactiveEvent::DisasterStruck {
        event_id,
        region: settlement, // Use settlement entity as the target
    });
}

pub(crate) fn apply_start_persistent_disaster(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    disaster_type: DisasterType,
    severity: f64,
    months: u32,
) {
    world.entity_mut(settlement).insert(EcsActiveDisaster {
        disaster_type,
        severity,
        started: ctx.clock_time,
        months_remaining: months,
        total_deaths: 0,
    });

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "active_disaster".to_string(),
            old_value: serde_json::Value::Null,
            new_value: serde_json::json!(disaster_type.as_str()),
        },
    );

    ctx.emit(SimReactiveEvent::DisasterStarted {
        event_id,
        region: settlement,
    });
}

pub(crate) fn apply_end_disaster(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
) {
    let disaster_type = world
        .get::<EcsActiveDisaster>(settlement)
        .map(|ad| ad.disaster_type.as_str().to_string())
        .unwrap_or_default();

    world.entity_mut(settlement).remove::<EcsActiveDisaster>();

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "active_disaster".to_string(),
            old_value: serde_json::json!(disaster_type),
            new_value: serde_json::Value::Null,
        },
    );

    ctx.emit(SimReactiveEvent::DisasterEnded {
        event_id,
        region: settlement,
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_create_geographic_feature(
    ctx: &mut ApplyCtx,
    world: &mut World,
    _event_id: u64,
    name: &str,
    region: Entity,
    feature_type: &crate::model::FeatureType,
    x: f64,
    y: f64,
) {
    let feature_id = ctx.id_gen.0.next_id();
    let feature_entity = world
        .spawn((
            SimEntity {
                id: feature_id,
                name: name.to_string(),
                origin: Some(ctx.clock_time),
                end: None,
            },
            GeographicFeature,
            GeographicFeatureState {
                feature_type: feature_type.clone(),
                x,
                y,
            },
            LocatedIn(region),
        ))
        .id();
    ctx.entity_map.insert(feature_id, feature_entity);
}

/// Damage buildings in a settlement, filtering by disaster type.
fn apply_building_damage_from_disaster(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    damage: f64,
    disaster_type: &DisasterType,
) {
    // Collect building entities in this settlement
    let dt = *disaster_type;
    let buildings: Vec<Entity> = world
        .query::<(Entity, &SimEntity, &BuildingState, &LocatedIn)>()
        .iter(world)
        .filter(|(_, sim, bs, loc)| {
            sim.is_alive()
                && loc.0 == settlement
                && match dt {
                    DisasterType::Storm => {
                        matches!(
                            bs.building_type,
                            crate::model::BuildingType::Port | crate::model::BuildingType::Market
                        )
                    }
                    DisasterType::Flood => {
                        matches!(
                            bs.building_type,
                            crate::model::BuildingType::Granary
                                | crate::model::BuildingType::Workshop
                                | crate::model::BuildingType::Mine
                        )
                    }
                    DisasterType::Wildfire => {
                        matches!(
                            bs.building_type,
                            crate::model::BuildingType::Workshop
                                | crate::model::BuildingType::Granary
                                | crate::model::BuildingType::Market
                        )
                    }
                    _ => true,
                }
        })
        .map(|(e, _, _, _)| e)
        .collect();

    for building in buildings {
        if let Some(mut bs) = world.get_mut::<BuildingState>(building) {
            let old_condition = bs.condition;
            bs.condition = (bs.condition - damage).max(0.0);
            let new_condition = bs.condition;

            ctx.record_effect(
                event_id,
                building,
                StateChange::PropertyChanged {
                    field: "condition".to_string(),
                    old_value: serde_json::json!(old_condition),
                    new_value: serde_json::json!(new_condition),
                },
            );

            // If condition reached 0, end the building
            if new_condition <= 0.0
                && let Some(mut sim) = world.get_mut::<SimEntity>(building)
                && sim.end.is_none()
            {
                sim.end = Some(ctx.clock_time);
            }
        }
    }
}
