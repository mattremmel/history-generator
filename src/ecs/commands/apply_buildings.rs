use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::{Building, BuildingState, FactionCore, SimEntity};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::LocatedIn;
use crate::model::effect::StateChange;
use crate::model::entity_data::BuildingType;

use super::applicator::ApplyCtx;

fn capitalize_building_type(bt: &BuildingType) -> &str {
    match bt {
        BuildingType::Mine => "Mine",
        BuildingType::Port => "Port",
        BuildingType::Market => "Market",
        BuildingType::Granary => "Granary",
        BuildingType::Temple => "Temple",
        BuildingType::Workshop => "Workshop",
        BuildingType::Aqueduct => "Aqueduct",
        BuildingType::Library => "Library",
        BuildingType::ScholarGuild => "Scholar Guild",
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_construct_building(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    faction: Entity,
    building_type: BuildingType,
    cost: f64,
    x: f64,
    y: f64,
) {
    // Deduct from faction treasury
    if let Some(mut fc) = world.get_mut::<FactionCore>(faction) {
        let old_treasury = fc.treasury;
        fc.treasury -= cost;
        ctx.record_effect(
            event_id,
            faction,
            StateChange::PropertyChanged {
                field: "treasury".to_string(),
                old_value: serde_json::json!(old_treasury),
                new_value: serde_json::json!(old_treasury - cost),
            },
        );
    }

    // Get settlement name for the building name
    let settlement_name = world
        .get::<SimEntity>(settlement)
        .map(|s| s.name.clone())
        .unwrap_or_default();
    let building_name = format!(
        "{} {}",
        settlement_name,
        capitalize_building_type(&building_type)
    );

    // Spawn building entity (inline — SimEntityMap is extracted from world into ctx)
    let building_id = ctx.id_gen.0.next_id();
    let building_entity = world
        .spawn((
            SimEntity {
                id: building_id,
                name: building_name,
                origin: Some(ctx.clock_time),
                end: None,
            },
            Building,
            BuildingState {
                building_type,
                output_resource: None,
                x,
                y,
                condition: 1.0,
                level: 0,
                constructed: ctx.clock_time,
            },
            LocatedIn(settlement),
        ))
        .id();
    ctx.entity_map.insert(building_id, building_entity);

    ctx.emit(SimReactiveEvent::BuildingConstructed {
        event_id,
        building: building_entity,
        settlement,
    });
}

pub(crate) fn apply_damage_building(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    building: Entity,
    damage: f64,
) {
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
            ctx.record_effect(event_id, building, StateChange::EntityEnded);
        }
    }
}

pub(crate) fn apply_upgrade_building(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    building: Entity,
    new_level: u8,
    cost: f64,
    faction: Entity,
) {
    // Deduct from faction treasury
    if let Some(mut fc) = world.get_mut::<FactionCore>(faction) {
        let old_treasury = fc.treasury;
        fc.treasury -= cost;
        ctx.record_effect(
            event_id,
            faction,
            StateChange::PropertyChanged {
                field: "treasury".to_string(),
                old_value: serde_json::json!(old_treasury),
                new_value: serde_json::json!(old_treasury - cost),
            },
        );
    }

    // Upgrade building
    if let Some(mut bs) = world.get_mut::<BuildingState>(building) {
        let old_level = bs.level;
        bs.level = new_level;
        bs.condition = 1.0; // Restore condition on upgrade
        ctx.record_effect(
            event_id,
            building,
            StateChange::PropertyChanged {
                field: "level".to_string(),
                old_value: serde_json::json!(old_level),
                new_value: serde_json::json!(new_level),
            },
        );
    }

    ctx.emit(SimReactiveEvent::BuildingUpgraded { event_id, building });
}
