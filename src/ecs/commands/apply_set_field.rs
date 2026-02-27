use bevy_ecs::entity::Entity;

use crate::model::effect::StateChange;

use super::applicator::ApplyCtx;

/// Record a property change effect. No Event entry. No reactive event.
pub(crate) fn apply_set_field(
    ctx: &mut ApplyCtx,
    event_id: u64,
    entity: Entity,
    field: &str,
    old_value: &serde_json::Value,
    new_value: &serde_json::Value,
) {
    ctx.record_effect(
        event_id,
        entity,
        StateChange::PropertyChanged {
            field: field.to_string(),
            old_value: old_value.clone(),
            new_value: new_value.clone(),
        },
    );
}
