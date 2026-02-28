use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::SettlementCore;
use crate::ecs::components::common::SimEntity;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LocatedIn, MemberOf};
use crate::model::effect::StateChange;
use crate::model::relationship::RelationshipKind;

use super::applicator::ApplyCtx;

/// Migrate population: subtract from source, add to destination.
pub(crate) fn apply_migrate_population(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    from_settlement: Entity,
    to_settlement: Entity,
    count: u32,
) {
    // Subtract from source
    if let Some(mut core) = world.get_mut::<SettlementCore>(from_settlement) {
        let old_pop = core.population;
        let new_pop = old_pop.saturating_sub(count);
        core.population = new_pop;
        core.population_breakdown.scale_to(new_pop);
        ctx.record_effect(
            event_id,
            from_settlement,
            StateChange::PropertyChanged {
                field: "population".to_string(),
                old_value: serde_json::json!(old_pop),
                new_value: serde_json::json!(new_pop),
            },
        );
    }

    // Add to destination
    if let Some(mut core) = world.get_mut::<SettlementCore>(to_settlement) {
        let old_pop = core.population;
        let new_pop = old_pop + count;
        core.population = new_pop;
        core.population_breakdown.scale_to(new_pop);
        ctx.record_effect(
            event_id,
            to_settlement,
            StateChange::PropertyChanged {
                field: "population".to_string(),
                old_value: serde_json::json!(old_pop),
                new_value: serde_json::json!(new_pop),
            },
        );
    }

    ctx.emit(SimReactiveEvent::RefugeesArrived {
        event_id,
        settlement: to_settlement,
        count,
    });
}

/// Relocate person: update LocatedIn, optionally switch faction to match destination.
pub(crate) fn apply_relocate_person(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    person: Entity,
    to_settlement: Entity,
) {
    // Verify person is alive
    let is_alive = world
        .get::<SimEntity>(person)
        .is_some_and(|s| s.end.is_none());
    if !is_alive {
        return;
    }

    let person_sim = ctx.entity_map.get_sim(person).unwrap_or(0);

    // Record old location ending
    if let Some(old_loc) = world.get::<LocatedIn>(person) {
        let old_sim = ctx.entity_map.get_sim(old_loc.0).unwrap_or(0);
        ctx.record_effect(
            event_id,
            person,
            StateChange::RelationshipEnded {
                target_entity_id: old_sim,
                kind: RelationshipKind::LocatedIn,
            },
        );
    }

    // Set new location
    world.entity_mut(person).insert(LocatedIn(to_settlement));
    let to_sim = ctx.entity_map.get_sim(to_settlement).unwrap_or(0);
    ctx.record_effect(
        event_id,
        person,
        StateChange::RelationshipStarted {
            target_entity_id: to_sim,
            kind: RelationshipKind::LocatedIn,
        },
    );

    // Switch faction if destination settlement belongs to a different faction
    let dest_faction = world.get::<MemberOf>(to_settlement).map(|m| m.0);
    let person_faction = world.get::<MemberOf>(person).map(|m| m.0);

    if let (Some(new_fid), Some(old_fid)) = (dest_faction, person_faction)
        && old_fid != new_fid
    {
        let old_faction_sim = ctx.entity_map.get_sim(old_fid).unwrap_or(0);
        let new_faction_sim = ctx.entity_map.get_sim(new_fid).unwrap_or(0);

        world.entity_mut(person).insert(MemberOf(new_fid));
        ctx.record_effect(
            event_id,
            person,
            StateChange::RelationshipEnded {
                target_entity_id: old_faction_sim,
                kind: RelationshipKind::MemberOf,
            },
        );
        ctx.record_effect(
            event_id,
            person,
            StateChange::RelationshipStarted {
                target_entity_id: new_faction_sim,
                kind: RelationshipKind::MemberOf,
            },
        );
    }

    let _ = person_sim;
}

/// Abandon settlement: end the settlement entity.
pub(crate) fn apply_abandon_settlement(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
) {
    let Some(mut sim_entity) = world.get_mut::<SimEntity>(settlement) else {
        return;
    };
    if sim_entity.end.is_some() {
        return;
    }

    sim_entity.end = Some(ctx.clock_time);
    ctx.record_effect(event_id, settlement, StateChange::EntityEnded);
    ctx.emit(SimReactiveEvent::EntityDied {
        event_id,
        entity: settlement,
    });
}
