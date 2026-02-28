use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::ItemState;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::HeldBy;
use crate::ecs::spawn;
use crate::model::ItemType;
use crate::model::effect::StateChange;

use super::applicator::ApplyCtx;

/// Craft a new item: spawn Item entity with HeldBy relationship to crafter.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_craft_item(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    crafter: Entity,
    settlement: Entity,
    name: &str,
    item_type: ItemType,
    material: &str,
) {
    let item_id = ctx.id_gen.0.next_id();
    let item_entity = spawn::spawn_item(
        world,
        item_id,
        name.to_string(),
        Some(ctx.clock_time),
        ItemState {
            item_type,
            material: material.to_string(),
            resonance: 0.0,
            resonance_tier: 0,
            condition: 1.0,
            created: ctx.clock_time,
            last_transferred: None,
        },
    );
    ctx.entity_map.insert(item_id, item_entity);

    // Item is held by its crafter
    world.entity_mut(item_entity).insert(HeldBy(crafter));

    ctx.record_effect(
        event_id,
        item_entity,
        StateChange::EntityCreated {
            kind: crate::model::EntityKind::Item,
            name: name.to_string(),
        },
    );

    // Also record the settlement where it was crafted
    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "item_crafted".to_string(),
            old_value: serde_json::json!(null),
            new_value: serde_json::json!(item_id),
        },
    );

    ctx.emit(SimReactiveEvent::ItemCrafted {
        event_id,
        item: item_entity,
    });
}

/// Transfer item ownership to a new holder.
pub(crate) fn apply_transfer_item(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    item: Entity,
    new_holder: Entity,
) {
    let old_holder = world.get::<HeldBy>(item).map(|h| h.0);

    // Update or insert the HeldBy component
    world.entity_mut(item).insert(HeldBy(new_holder));

    // Update last_transferred timestamp
    if let Some(mut state) = world.get_mut::<ItemState>(item) {
        state.last_transferred = Some(ctx.clock_time);
    }

    ctx.record_effect(
        event_id,
        item,
        StateChange::PropertyChanged {
            field: "held_by".to_string(),
            old_value: serde_json::json!(old_holder.and_then(|e| ctx.entity_map.get_sim(e))),
            new_value: serde_json::json!(ctx.entity_map.get_sim(new_holder)),
        },
    );
}
