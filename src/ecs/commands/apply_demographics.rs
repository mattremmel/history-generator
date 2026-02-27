use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::common::SimEntity;
use crate::ecs::components::{Person, PersonCore, PersonEducation, PersonReputation, PersonSocial};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LeaderOf, MemberOf, LocatedIn};
use crate::model::effect::StateChange;
use crate::model::entity::EntityKind;

use super::applicator::ApplyCtx;

/// Person died: end entity, remove structural relationships, emit vacancy if leader.
pub(crate) fn apply_person_died(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    person: Entity,
) {
    // End the entity (idempotent)
    let Some(mut sim_entity) = world.get_mut::<SimEntity>(person) else {
        return;
    };
    if sim_entity.end.is_some() {
        return;
    }
    sim_entity.end = Some(ctx.clock_time);
    ctx.record_effect(event_id, person, StateChange::EntityEnded);

    // Check if leader before removing components
    let was_leader = world.get::<LeaderOf>(person).map(|lo| lo.0);

    // Remove structural relationships
    world.entity_mut(person).remove::<MemberOf>();
    world.entity_mut(person).remove::<LeaderOf>();
    world.entity_mut(person).remove::<LocatedIn>();

    ctx.emit(SimReactiveEvent::EntityDied {
        event_id,
        entity: person,
    });

    // If was a leader, emit vacancy
    if let Some(faction) = was_leader {
        ctx.emit(SimReactiveEvent::LeaderVacancy { event_id, faction });
    }
}

/// Person born: spawn new person entity, register in map, set up relationships.
pub(crate) fn apply_person_born(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    name: &str,
    faction: Entity,
    settlement: Entity,
) {
    let sim_id = ctx.id_gen.0.next_id();

    let entity = world
        .spawn((
            SimEntity {
                id: sim_id,
                name: name.to_string(),
                origin: Some(ctx.clock_time),
                end: None,
            },
            Person,
            PersonCore::default(),
            PersonReputation::default(),
            PersonSocial::default(),
            PersonEducation::default(),
            MemberOf(faction),
            LocatedIn(settlement),
        ))
        .id();

    ctx.entity_map.insert(sim_id, entity);

    ctx.record_effect(
        event_id,
        entity,
        StateChange::EntityCreated {
            kind: EntityKind::Person,
            name: name.to_string(),
        },
    );
}
