use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{MemberOf, RelationshipGraph, RelationshipMeta};
use crate::model::effect::StateChange;
use crate::model::relationship::RelationshipKind;

use super::applicator::ApplyCtx;

/// Declare war: insert AtWar into RelationshipGraph, emit WarStarted.
pub(crate) fn apply_declare_war(
    ctx: &mut ApplyCtx,
    event_id: u64,
    attacker: Entity,
    defender: Entity,
) {
    let pair = RelationshipGraph::canonical_pair(attacker, defender);
    if !ctx.rel_graph.are_at_war(attacker, defender) {
        ctx.rel_graph
            .at_war
            .insert(pair, RelationshipMeta::new(ctx.clock_time));
    }

    let attacker_sim = ctx.entity_map.get_sim(attacker).unwrap_or(0);
    let defender_sim = ctx.entity_map.get_sim(defender).unwrap_or(0);

    ctx.record_effect(
        event_id,
        attacker,
        StateChange::RelationshipStarted {
            target_entity_id: defender_sim,
            kind: RelationshipKind::AtWar,
        },
    );
    ctx.record_effect(
        event_id,
        defender,
        StateChange::RelationshipStarted {
            target_entity_id: attacker_sim,
            kind: RelationshipKind::AtWar,
        },
    );

    ctx.emit(SimReactiveEvent::WarStarted {
        event_id,
        attacker,
        defender,
    });
}

/// Capture settlement: change MemberOf from old faction to new, emit SettlementCaptured.
pub(crate) fn apply_capture_settlement(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    new_faction: Entity,
) {
    let old_faction = world.get::<MemberOf>(settlement).map(|m| m.0);

    // Update the MemberOf relationship
    world.entity_mut(settlement).insert(MemberOf(new_faction));

    let settlement_sim = ctx.entity_map.get_sim(settlement).unwrap_or(0);
    let new_faction_sim = ctx.entity_map.get_sim(new_faction).unwrap_or(0);

    // Record effect: old membership ended
    if let Some(old) = old_faction {
        let old_sim = ctx.entity_map.get_sim(old).unwrap_or(0);
        ctx.record_effect(
            event_id,
            settlement,
            StateChange::RelationshipEnded {
                target_entity_id: old_sim,
                kind: RelationshipKind::MemberOf,
            },
        );
    }

    // Record effect: new membership started
    ctx.record_effect(
        event_id,
        settlement,
        StateChange::RelationshipStarted {
            target_entity_id: new_faction_sim,
            kind: RelationshipKind::MemberOf,
        },
    );

    ctx.emit(SimReactiveEvent::SettlementCaptured {
        event_id,
        settlement,
        old_faction: old_faction.unwrap_or(settlement), // fallback if no previous faction
        new_faction,
    });

    // Suppress unused variable warnings
    let _ = settlement_sim;
}
