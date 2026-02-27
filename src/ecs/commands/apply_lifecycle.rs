use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::common::SimEntity;
use crate::ecs::events::SimReactiveEvent;
use crate::model::effect::StateChange;

use super::applicator::ApplyCtx;

/// End an entity: set `SimEntity.end = Some(time)`.
/// Idempotent: if already ended, no-op (no effect recorded).
pub(crate) fn apply_end_entity(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    entity: Entity,
) {
    let Some(mut sim_entity) = world.get_mut::<SimEntity>(entity) else {
        return;
    };

    // Idempotent: already ended → no-op
    if sim_entity.end.is_some() {
        return;
    }

    sim_entity.end = Some(ctx.clock_time);

    ctx.record_effect(event_id, entity, StateChange::EntityEnded);
    ctx.emit(SimReactiveEvent::EntityDied { event_id, entity });
}

/// Rename an entity: change `SimEntity.name`.
pub(crate) fn apply_rename_entity(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    entity: Entity,
    new_name: &str,
) {
    let Some(mut sim_entity) = world.get_mut::<SimEntity>(entity) else {
        return;
    };

    let old_name = sim_entity.name.clone();
    sim_entity.name = new_name.to_string();

    ctx.record_effect(
        event_id,
        entity,
        StateChange::NameChanged {
            old: old_name,
            new: new_name.to_string(),
        },
    );
}
