use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::common::SimEntity;
use crate::ecs::components::{
    Person, PersonCore, PersonEducation, PersonReputation, PersonSocial, SettlementCore,
};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LeaderOf, LocatedIn, MemberOf, RelationshipGraph};
use crate::ecs::time::SimTime;
use crate::model::Sex;
use crate::model::effect::StateChange;
use crate::model::entity::EntityKind;
use crate::model::entity_data::Role;
use crate::model::traits::Trait;

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

    // End spouse relationship in the graph (widowing the surviving spouse)
    let spouse_to_widow: Option<Entity> = ctx
        .rel_graph
        .spouses
        .iter()
        .find(|((a, b), meta)| meta.is_active() && (*a == person || *b == person))
        .map(|((a, b), _)| if *a == person { *b } else { *a });

    if let Some(spouse) = spouse_to_widow {
        let pair = RelationshipGraph::canonical_pair(person, spouse);
        if let Some(meta) = ctx.rel_graph.spouses.get_mut(&pair) {
            meta.end = Some(ctx.clock_time);
        }
        // Set widowed_at on the surviving spouse
        if let Some(mut spouse_core) = world.get_mut::<PersonCore>(spouse) {
            spouse_core.widowed_at = Some(ctx.clock_time);
        }
    }

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
        ctx.emit(SimReactiveEvent::LeaderVacancy {
            event_id,
            faction,
            previous_leader: person,
        });
    }
}

/// Person born: spawn new person entity with full attributes, register in map, set up relationships.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_person_born(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    name: &str,
    faction: Entity,
    settlement: Entity,
    sex: Sex,
    role: &Role,
    traits: &[Trait],
    culture_id: Option<u64>,
    father: Option<Entity>,
    mother: Option<Entity>,
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
            PersonCore {
                born: ctx.clock_time,
                sex,
                role: role.clone(),
                traits: traits.to_vec(),
                last_action: SimTime::default(),
                culture_id,
                widowed_at: None,
            },
            PersonReputation::default(),
            PersonSocial::default(),
            PersonEducation::default(),
            MemberOf(faction),
            LocatedIn(settlement),
        ))
        .id();

    ctx.entity_map.insert(sim_id, entity);

    // Wire parent-child relationships in the graph
    if let Some(father_entity) = father {
        ctx.rel_graph
            .parent_child
            .entry(father_entity)
            .or_default()
            .push(entity);
    }
    if let Some(mother_entity) = mother {
        ctx.rel_graph
            .parent_child
            .entry(mother_entity)
            .or_default()
            .push(entity);
    }

    ctx.record_effect(
        event_id,
        entity,
        StateChange::EntityCreated {
            kind: EntityKind::Person,
            name: name.to_string(),
        },
    );
}

/// Grow population: update settlement population and breakdown.
pub(crate) fn apply_grow_population(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    new_total: u32,
) {
    let Some(mut core) = world.get_mut::<SettlementCore>(settlement) else {
        return;
    };
    let old_pop = core.population;
    core.population = new_total;
    // Scale the breakdown to match (keeps proportions)
    core.population_breakdown.scale_to(new_total);

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "population".to_string(),
            old_value: serde_json::json!(old_pop),
            new_value: serde_json::json!(new_total),
        },
    );
}

/// Marriage: insert spouse pair in RelationshipGraph.
pub(crate) fn apply_marriage(
    ctx: &mut ApplyCtx,
    _world: &mut World,
    event_id: u64,
    person_a: Entity,
    person_b: Entity,
) {
    use crate::ecs::relationships::{RelationshipGraph, RelationshipMeta};

    let pair = RelationshipGraph::canonical_pair(person_a, person_b);
    ctx.rel_graph
        .spouses
        .insert(pair, RelationshipMeta::new(ctx.clock_time));

    ctx.record_effect(
        event_id,
        person_a,
        StateChange::RelationshipStarted {
            target_entity_id: ctx.entity_map.get_sim(person_b).unwrap_or(0),
            kind: crate::model::RelationshipKind::Spouse,
        },
    );
    ctx.record_effect(
        event_id,
        person_b,
        StateChange::RelationshipStarted {
            target_entity_id: ctx.entity_map.get_sim(person_a).unwrap_or(0),
            kind: crate::model::RelationshipKind::Spouse,
        },
    );
}
