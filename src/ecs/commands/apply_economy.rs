use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::SettlementTrade;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{RelationshipGraph, TradeRouteData};
use crate::model::effect::StateChange;

use super::applicator::ApplyCtx;

/// Establish a trade route between two settlements.
pub(crate) fn apply_establish_trade_route(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement_a: Entity,
    settlement_b: Entity,
) {
    let pair = RelationshipGraph::canonical_pair(settlement_a, settlement_b);

    // Don't duplicate existing active route
    if ctx.rel_graph.trade_routes.contains_key(&pair) {
        return;
    }

    // Insert into RelationshipGraph
    ctx.rel_graph.trade_routes.insert(
        pair,
        TradeRouteData {
            path: vec![settlement_a, settlement_b],
            distance: 1,
            resource: String::new(),
            start: ctx.clock_time,
        },
    );

    // Add TradeRoute to both settlements' SettlementTrade component
    let sim_id_a = ctx.entity_map.get_sim(settlement_a).unwrap_or(0);
    let sim_id_b = ctx.entity_map.get_sim(settlement_b).unwrap_or(0);

    if let Some(mut trade) = world.get_mut::<SettlementTrade>(settlement_a) {
        use crate::model::TradeRoute;
        if !trade.trade_routes.iter().any(|r| r.target == sim_id_b) {
            trade.trade_routes.push(TradeRoute {
                target: sim_id_b,
                path: vec![],
                distance: 1,
                resource: String::new(),
            });
        }
    }
    if let Some(mut trade) = world.get_mut::<SettlementTrade>(settlement_b) {
        use crate::model::TradeRoute;
        if !trade.trade_routes.iter().any(|r| r.target == sim_id_a) {
            trade.trade_routes.push(TradeRoute {
                target: sim_id_a,
                path: vec![],
                distance: 1,
                resource: String::new(),
            });
        }
    }

    ctx.record_effect(
        event_id,
        settlement_a,
        StateChange::RelationshipStarted {
            target_entity_id: sim_id_b,
            kind: crate::model::RelationshipKind::TradeRoute,
        },
    );

    ctx.emit(SimReactiveEvent::TradeRouteEstablished {
        event_id,
        settlement_a,
        settlement_b,
    });
}

/// Sever a trade route between two settlements.
pub(crate) fn apply_sever_trade_route(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement_a: Entity,
    settlement_b: Entity,
) {
    let pair = RelationshipGraph::canonical_pair(settlement_a, settlement_b);
    ctx.rel_graph.trade_routes.remove(&pair);

    // Remove from both settlements' SettlementTrade component
    let sim_id_a = ctx.entity_map.get_sim(settlement_a).unwrap_or(0);
    let sim_id_b = ctx.entity_map.get_sim(settlement_b).unwrap_or(0);

    if let Some(mut trade) = world.get_mut::<SettlementTrade>(settlement_a) {
        trade.trade_routes.retain(|r| r.target != sim_id_b);
    }
    if let Some(mut trade) = world.get_mut::<SettlementTrade>(settlement_b) {
        trade.trade_routes.retain(|r| r.target != sim_id_a);
    }

    ctx.record_effect(
        event_id,
        settlement_a,
        StateChange::RelationshipEnded {
            target_entity_id: sim_id_b,
            kind: crate::model::RelationshipKind::TradeRoute,
        },
    );

    ctx.emit(SimReactiveEvent::TradeRouteRaided {
        event_id,
        settlement_a,
        settlement_b,
    });
}
