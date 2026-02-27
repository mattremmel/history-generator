use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::relationships::{
    Exploits, FlowsThrough, HeldBy, HiredBy, LeaderOf, LocatedIn, MemberOf, RelationshipMeta,
};
use crate::model::effect::StateChange;
use crate::model::relationship::RelationshipKind;

use super::applicator::ApplyCtx;

/// Add a relationship between two entities.
///
/// Structural relationships (LocatedIn, MemberOf, LeaderOf, HeldBy, HiredBy,
/// FlowsThrough, Exploits) are stored as Bevy components on the source entity.
/// Graph relationships (Ally, Enemy, AtWar, Spouse, TradeRoute, Parent, Child)
/// are stored in `RelationshipGraph`.
pub(crate) fn apply_add_relationship(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    source: Entity,
    target: Entity,
    kind: &RelationshipKind,
) {
    match kind {
        // Structural: insert component on source
        RelationshipKind::LocatedIn => {
            world.entity_mut(source).insert(LocatedIn(target));
        }
        RelationshipKind::MemberOf => {
            world.entity_mut(source).insert(MemberOf(target));
        }
        RelationshipKind::LeaderOf => {
            world.entity_mut(source).insert(LeaderOf(target));
        }
        RelationshipKind::HeldBy => {
            world.entity_mut(source).insert(HeldBy(target));
        }
        RelationshipKind::HiredBy => {
            world.entity_mut(source).insert(HiredBy(target));
        }
        RelationshipKind::FlowsThrough => {
            world.entity_mut(source).insert(FlowsThrough(target));
        }
        RelationshipKind::Exploits => {
            world.entity_mut(source).insert(Exploits(target));
        }

        // Graph: insert into RelationshipGraph (idempotent)
        RelationshipKind::Ally => {
            let pair = crate::ecs::relationships::RelationshipGraph::canonical_pair(source, target);
            if !ctx.rel_graph.are_allies(source, target) {
                ctx.rel_graph
                    .allies
                    .insert(pair, RelationshipMeta::new(ctx.clock_time));
            }
        }
        RelationshipKind::Enemy => {
            let pair = crate::ecs::relationships::RelationshipGraph::canonical_pair(source, target);
            if !ctx.rel_graph.are_enemies(source, target) {
                ctx.rel_graph
                    .enemies
                    .insert(pair, RelationshipMeta::new(ctx.clock_time));
            }
        }
        RelationshipKind::AtWar => {
            let pair = crate::ecs::relationships::RelationshipGraph::canonical_pair(source, target);
            if !ctx.rel_graph.are_at_war(source, target) {
                ctx.rel_graph
                    .at_war
                    .insert(pair, RelationshipMeta::new(ctx.clock_time));
            }
        }
        RelationshipKind::Spouse => {
            let pair = crate::ecs::relationships::RelationshipGraph::canonical_pair(source, target);
            if !ctx.rel_graph.are_spouses(source, target) {
                ctx.rel_graph
                    .spouses
                    .insert(pair, RelationshipMeta::new(ctx.clock_time));
            }
        }
        RelationshipKind::Parent => {
            ctx.rel_graph
                .parent_child
                .entry(source)
                .or_default()
                .push(target);
        }
        RelationshipKind::Child => {
            // Child is the inverse of Parent: target is the parent
            ctx.rel_graph
                .parent_child
                .entry(target)
                .or_default()
                .push(source);
        }

        // AdjacentTo and TradeRoute are handled elsewhere (adjacency resource, trade data)
        RelationshipKind::AdjacentTo | RelationshipKind::TradeRoute => {
            tracing::warn!("AddRelationship for {:?} not handled via applicator", kind);
        }
        RelationshipKind::Custom(_) => {}
    }

    let source_sim = ctx.entity_map.get_sim(source).unwrap_or(0);
    let target_sim = ctx.entity_map.get_sim(target).unwrap_or(0);
    ctx.record_effect(
        event_id,
        source,
        StateChange::RelationshipStarted {
            target_entity_id: target_sim,
            kind: kind.clone(),
        },
    );
    // Suppress unused variable warning
    let _ = source_sim;
}

/// End a relationship between two entities.
pub(crate) fn apply_end_relationship(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    source: Entity,
    target: Entity,
    kind: &RelationshipKind,
) {
    match kind {
        // Structural: remove component
        RelationshipKind::LocatedIn => {
            world.entity_mut(source).remove::<LocatedIn>();
        }
        RelationshipKind::MemberOf => {
            world.entity_mut(source).remove::<MemberOf>();
        }
        RelationshipKind::LeaderOf => {
            world.entity_mut(source).remove::<LeaderOf>();
        }
        RelationshipKind::HeldBy => {
            world.entity_mut(source).remove::<HeldBy>();
        }
        RelationshipKind::HiredBy => {
            world.entity_mut(source).remove::<HiredBy>();
        }
        RelationshipKind::FlowsThrough => {
            world.entity_mut(source).remove::<FlowsThrough>();
        }
        RelationshipKind::Exploits => {
            world.entity_mut(source).remove::<Exploits>();
        }

        // Graph: set end time
        RelationshipKind::Ally => {
            let pair = crate::ecs::relationships::RelationshipGraph::canonical_pair(source, target);
            if let Some(meta) = ctx.rel_graph.allies.get_mut(&pair) {
                meta.end = Some(ctx.clock_time);
            }
        }
        RelationshipKind::Enemy => {
            let pair = crate::ecs::relationships::RelationshipGraph::canonical_pair(source, target);
            if let Some(meta) = ctx.rel_graph.enemies.get_mut(&pair) {
                meta.end = Some(ctx.clock_time);
            }
        }
        RelationshipKind::AtWar => {
            let pair = crate::ecs::relationships::RelationshipGraph::canonical_pair(source, target);
            if let Some(meta) = ctx.rel_graph.at_war.get_mut(&pair) {
                meta.end = Some(ctx.clock_time);
            }
        }
        RelationshipKind::Spouse => {
            let pair = crate::ecs::relationships::RelationshipGraph::canonical_pair(source, target);
            if let Some(meta) = ctx.rel_graph.spouses.get_mut(&pair) {
                meta.end = Some(ctx.clock_time);
            }
        }

        RelationshipKind::Parent
        | RelationshipKind::Child
        | RelationshipKind::AdjacentTo
        | RelationshipKind::TradeRoute => {
            tracing::warn!("EndRelationship for {:?} not handled via applicator", kind);
        }
        RelationshipKind::Custom(_) => {}
    }

    let target_sim = ctx.entity_map.get_sim(target).unwrap_or(0);
    ctx.record_effect(
        event_id,
        source,
        StateChange::RelationshipEnded {
            target_entity_id: target_sim,
            kind: kind.clone(),
        },
    );
}
