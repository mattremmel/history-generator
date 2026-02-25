use std::collections::BTreeMap;

use rand::Rng;

use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};
use crate::sim::context::TickContext;
use crate::sim::helpers;
use crate::sim::signal::{Signal, SignalKind};

use super::{gather_settlements, resource_base_value};

pub(super) const MAX_TRADE_HOPS: usize = 6;
pub(super) const MAX_ROUTES_PER_SETTLEMENT: usize = 3;
pub(super) const TRADE_ROUTE_FORMATION_CHANCE: f64 = 0.15;

// Trade route value parameters
const TRADE_DISTANCE_DECAY_FACTOR: f64 = 0.15;
const TRADE_PRESTIGE_VALUE_BONUS: f64 = 0.15;
const TRADE_PRESTIGE_FORMATION_BONUS: f64 = 0.2;
const RIVER_TRADE_BONUS: f64 = 1.3;
const MARGINAL_DEMAND_NO_DEFICIT: f64 = 0.2;
const TRADE_DEFICIT_THRESHOLD: f64 = 0.1;

// Trade diplomacy parameters
const TRADE_HAPPINESS_PER_ROUTE: f64 = 0.01;
const TRADE_HAPPINESS_MAX: f64 = 0.05;
const MIN_ROUTES_FOR_ALLIANCE: usize = 2;
const TRADE_ALLIANCE_CHANCE: f64 = 0.03;

pub(super) fn factions_at_war(world: &World, a: u64, b: u64) -> bool {
    world
        .entities
        .get(&a)
        .map(|e| e.has_active_rel(RelationshipKind::AtWar, b))
        .unwrap_or(false)
}

fn region_has_hostile_settlement(world: &World, region_id: u64, hostile_factions: &[u64]) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::LocatedIn, region_id)
            && e.active_rel(RelationshipKind::MemberOf)
                .is_some_and(|fid| hostile_factions.contains(&fid))
    })
}

/// BFS from source to target region, returning full path of region IDs
/// (excluding source, including target). Returns None if unreachable
/// within max_hops or if path is blocked by hostile territory.
pub(super) fn find_trade_path(
    world: &World,
    source_region: u64,
    target_region: u64,
    max_hops: usize,
    hostile_factions: &[u64],
) -> Option<Vec<u64>> {
    use std::collections::VecDeque;

    if source_region == target_region {
        return Some(vec![]);
    }

    let mut parent: BTreeMap<u64, u64> = BTreeMap::new();
    parent.insert(source_region, source_region);
    let mut queue: VecDeque<(u64, usize)> = VecDeque::new();

    for adj in helpers::adjacent_regions(world, source_region) {
        if parent.contains_key(&adj) {
            continue;
        }
        if region_has_hostile_settlement(world, adj, hostile_factions) && adj != target_region {
            continue;
        }
        parent.insert(adj, source_region);
        queue.push_back((adj, 1));
    }

    while let Some((current, depth)) = queue.pop_front() {
        if current == target_region {
            let mut path = vec![current];
            let mut node = current;
            while parent[&node] != source_region {
                node = parent[&node];
                path.push(node);
            }
            path.reverse();
            return Some(path);
        }

        if depth >= max_hops {
            continue;
        }

        for adj in helpers::adjacent_regions(world, current) {
            if parent.contains_key(&adj) {
                continue;
            }
            if region_has_hostile_settlement(world, adj, hostile_factions) && adj != target_region {
                continue;
            }
            parent.insert(adj, current);
            queue.push_back((adj, depth + 1));
        }
    }

    None
}

/// Check if a region has a river flowing through it.
fn region_has_river(world: &World, region_id: u64) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::River
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::FlowsThrough, region_id)
    })
}

pub(super) fn count_active_outgoing_routes(world: &World, settlement_id: u64) -> usize {
    world
        .entities
        .get(&settlement_id)
        .map(|e| e.active_rels(RelationshipKind::TradeRoute).count())
        .unwrap_or(0)
}

pub(super) fn manage_trade_routes(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    _year_event: u64,
) {
    let settlements = gather_settlements(ctx.world);

    // Build surplus/deficit maps
    struct TradeCandidate {
        source_id: u64,
        target_id: u64,
        source_region: u64,
        target_region: u64,
        source_faction: u64,
        target_faction: u64,
        resource: String,
        value: f64,
    }

    // Collect surpluses and deficits
    let mut surplus_settlements: Vec<(u64, u64, u64, String, f64)> = Vec::new(); // (id, region, faction, resource, surplus)
    let mut deficit_settlements: Vec<(u64, u64, u64, String, f64)> = Vec::new();

    for s in &settlements {
        let surplus_map = ctx
            .world
            .entities
            .get(&s.id)
            .and_then(|e| e.data.as_settlement())
            .map(|sd| &sd.surplus);

        if let Some(surplus_map) = surplus_map {
            for (resource, &v) in surplus_map {
                if v > 0.0 {
                    surplus_settlements.push((
                        s.id,
                        s.region_id,
                        s.faction_id,
                        resource.as_str().to_string(),
                        v,
                    ));
                } else if v < -TRADE_DEFICIT_THRESHOLD {
                    deficit_settlements.push((
                        s.id,
                        s.region_id,
                        s.faction_id,
                        resource.as_str().to_string(),
                        v,
                    ));
                }
            }
        }
    }

    // Build candidates: each surplus settlement tries to find a deficit settlement
    let mut candidates: Vec<TradeCandidate> = Vec::new();

    // Collect factions at war with each faction (for pathfinding)
    let faction_ids: Vec<u64> = settlements
        .iter()
        .map(|s| s.faction_id)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    for &(src_id, src_region, src_faction, ref resource, surplus_val) in &surplus_settlements {
        // Skip if already at route cap
        if count_active_outgoing_routes(ctx.world, src_id) >= MAX_ROUTES_PER_SETTLEMENT {
            continue;
        }

        // Check if already has a route for this resource
        let has_route_for_resource = ctx
            .world
            .entities
            .get(&src_id)
            .and_then(|e| e.data.as_settlement())
            .map(|sd| sd.trade_routes.iter().any(|r| r.resource == *resource))
            .unwrap_or(false);
        if has_route_for_resource {
            continue;
        }

        // Find hostile factions for pathfinding
        let hostile: Vec<u64> = faction_ids
            .iter()
            .filter(|&&fid| fid != src_faction && factions_at_war(ctx.world, src_faction, fid))
            .copied()
            .collect();

        for &(tgt_id, tgt_region, tgt_faction, ref def_resource, _deficit_val) in
            &deficit_settlements
        {
            if def_resource != resource {
                continue;
            }
            if src_id == tgt_id {
                continue;
            }
            // Don't trade with factions at war
            if factions_at_war(ctx.world, src_faction, tgt_faction) {
                continue;
            }
            // Check if a route already exists between these settlements
            let already_connected = ctx
                .world
                .entities
                .get(&src_id)
                .map(|e| e.has_active_rel(RelationshipKind::TradeRoute, tgt_id))
                .unwrap_or(false);
            if already_connected {
                continue;
            }

            // Apply port range bonus to max trade hops
            let port_range_bonus = ctx
                .world
                .entities
                .get(&src_id)
                .and_then(|e| e.data.as_settlement())
                .map(|sd| sd.building_bonuses.port_range)
                .unwrap_or(0.0) as usize;
            let effective_max_hops = MAX_TRADE_HOPS + port_range_bonus;

            // Pathfind
            if let Some(path) = find_trade_path(
                ctx.world,
                src_region,
                tgt_region,
                effective_max_hops,
                &hostile,
            ) {
                let distance = path.len();
                let src_prestige = ctx
                    .world
                    .entities
                    .get(&src_id)
                    .and_then(|e| e.data.as_settlement())
                    .map(|sd| sd.prestige)
                    .unwrap_or(0.0);
                let tgt_prestige = ctx
                    .world
                    .entities
                    .get(&tgt_id)
                    .and_then(|e| e.data.as_settlement())
                    .map(|sd| sd.prestige)
                    .unwrap_or(0.0);
                let avg_endpoint_prestige = (src_prestige + tgt_prestige) / 2.0;
                let value = surplus_val * resource_base_value(resource)
                    / (1.0 + TRADE_DISTANCE_DECAY_FACTOR * distance as f64)
                    * (1.0 + avg_endpoint_prestige * TRADE_PRESTIGE_VALUE_BONUS);

                candidates.push(TradeCandidate {
                    source_id: src_id,
                    target_id: tgt_id,
                    source_region: src_region,
                    target_region: tgt_region,
                    source_faction: src_faction,
                    target_faction: tgt_faction,
                    resource: resource.clone(),
                    value,
                });
            }
        }
    }

    // Sort by value descending
    candidates.sort_by(|a, b| {
        b.value
            .partial_cmp(&a.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Establish routes with probability check
    let mut routes_added: BTreeMap<u64, usize> = BTreeMap::new();

    for c in &candidates {
        let current_count = routes_added.get(&c.source_id).copied().unwrap_or(0)
            + count_active_outgoing_routes(ctx.world, c.source_id);
        if current_count >= MAX_ROUTES_PER_SETTLEMENT {
            continue;
        }

        let source_prestige = ctx
            .world
            .entities
            .get(&c.source_id)
            .and_then(|e| e.data.as_settlement())
            .map(|sd| sd.prestige)
            .unwrap_or(0.0);
        let formation_chance =
            TRADE_ROUTE_FORMATION_CHANCE * (1.0 + source_prestige * TRADE_PRESTIGE_FORMATION_BONUS);
        if ctx.rng.random_range(0.0..1.0) >= formation_chance {
            continue;
        }

        // Find the path again for storage
        let hostile: Vec<u64> = faction_ids
            .iter()
            .filter(|&&fid| {
                fid != c.source_faction && factions_at_war(ctx.world, c.source_faction, fid)
            })
            .copied()
            .collect();

        let path = match find_trade_path(
            ctx.world,
            c.source_region,
            c.target_region,
            MAX_TRADE_HOPS,
            &hostile,
        ) {
            Some(p) => p,
            None => continue,
        };

        let distance = path.len();

        // Add TradeRoute relationship
        let ev = ctx.world.add_event(
            EventKind::Custom("trade_route_established".to_string()),
            time,
            format!(
                "Trade route established for {} between settlements in year {current_year}",
                c.resource
            ),
        );
        ctx.world
            .add_event_participant(ev, c.source_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, c.target_id, ParticipantRole::Object);
        ctx.world.add_relationship(
            c.source_id,
            c.target_id,
            RelationshipKind::TradeRoute,
            time,
            ev,
        );

        // Store route metadata on the settlement
        let route_entry = crate::model::entity_data::TradeRoute {
            target: c.target_id,
            path,
            distance: distance as u32,
            resource: c.resource.clone(),
        };

        ctx.world
            .settlement_mut(c.source_id)
            .trade_routes
            .push(route_entry);

        // Emit signal
        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::TradeRouteEstablished {
                from_settlement: c.source_id,
                to_settlement: c.target_id,
                from_faction: c.source_faction,
                to_faction: c.target_faction,
            },
        });

        *routes_added.entry(c.source_id).or_insert(0) += 1;
    }
}

// ---------------------------------------------------------------------------
// Phase D: Trade Flows & Wealth
// ---------------------------------------------------------------------------

pub(super) fn calculate_trade_flows(ctx: &mut TickContext, _year_event: u64) {
    // For each settlement with trade routes, compute trade income
    struct TradeUpdate {
        settlement_id: u64,
        trade_income: f64,
    }

    let mut updates: Vec<TradeUpdate> = Vec::new();

    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| e.id)
        .collect();

    for &sid in &settlement_ids {
        let routes: Vec<crate::model::entity_data::TradeRoute> = ctx
            .world
            .entities
            .get(&sid)
            .and_then(|e| e.data.as_settlement())
            .map(|sd| sd.trade_routes.clone())
            .unwrap_or_default();

        let mut total_income = 0.0;

        for route in &routes {
            let resource = route.resource.as_str();
            let distance = route.distance.max(1) as f64;
            let path = &route.path;

            // Get surplus at source
            let resource_type: Option<crate::model::entity_data::ResourceType> =
                resource.to_string().try_into().ok();
            let surplus = resource_type
                .as_ref()
                .and_then(|rt| {
                    ctx.world
                        .entities
                        .get(&sid)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.surplus.get(rt).copied())
                })
                .unwrap_or(0.0)
                .max(0.0);

            if surplus <= 0.0 {
                continue;
            }

            // Get deficit at target
            let target_id = route.target;
            let target_deficit = resource_type
                .as_ref()
                .and_then(|rt| {
                    ctx.world
                        .entities
                        .get(&target_id)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.surplus.get(rt).copied())
                })
                .unwrap_or(0.0);

            // Only trade if target actually has a deficit
            let demand = if target_deficit < 0.0 {
                target_deficit.abs()
            } else {
                // Target no longer needs this — still some marginal value
                MARGINAL_DEMAND_NO_DEFICIT
            };

            let volume = surplus.min(demand);
            let distance_decay = 1.0 / (1.0 + TRADE_DISTANCE_DECAY_FACTOR * distance);

            // River bonus
            let river_bonus = if path.iter().any(|&rid| region_has_river(ctx.world, rid)) {
                RIVER_TRADE_BONUS
            } else {
                1.0
            };

            let value = volume * resource_base_value(resource) * distance_decay * river_bonus;
            total_income += value;
        }

        // Apply building bonuses: market (+% trade income), port (+% trade volume)
        let sd = ctx
            .world
            .entities
            .get(&sid)
            .and_then(|e| e.data.as_settlement());
        let market_bonus = sd.map(|s| s.building_bonuses.market).unwrap_or(0.0);
        let port_trade_bonus = sd.map(|s| s.building_bonuses.port_trade).unwrap_or(0.0);
        // Apply seasonal trade modifier (set by EnvironmentSystem)
        let season_trade_mod = sd.map(|s| s.seasonal.trade).unwrap_or(1.0);

        total_income *= (1.0 + market_bonus + port_trade_bonus) * season_trade_mod;

        // Scale to monthly
        total_income /= super::MONTHS_PER_YEAR;

        if total_income > 0.0 {
            updates.push(TradeUpdate {
                settlement_id: sid,
                trade_income: total_income,
            });
        }
    }

    for u in updates {
        ctx.world.settlement_mut(u.settlement_id).trade_income = u.trade_income;
    }
}

pub(super) fn sever_faction_trade_routes(
    ctx: &mut TickContext,
    faction_a: u64,
    faction_b: u64,
    time: SimTimestamp,
    caused_by: u64,
) {
    // Find all trade routes between settlements of these two factions
    let mut to_sever: Vec<(u64, u64)> = Vec::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        let Some(my_faction) = e.active_rel(RelationshipKind::MemberOf) else {
            continue;
        };

        if my_faction != faction_a && my_faction != faction_b {
            continue;
        }

        for target in e.active_rels(RelationshipKind::TradeRoute) {
            if let Some(tf) = helpers::settlement_faction(ctx.world, target)
                && ((my_faction == faction_a && tf == faction_b)
                    || (my_faction == faction_b && tf == faction_a))
            {
                to_sever.push((e.id, target));
            }
        }
    }

    for (source, target) in to_sever {
        sever_route(ctx, source, target, time, caused_by);
    }
}

pub(crate) fn sever_settlement_trade_routes(
    ctx: &mut TickContext,
    settlement_id: u64,
    _old_faction_id: u64,
    time: SimTimestamp,
    caused_by: u64,
) {
    // Sever all routes from/to this settlement that were with the old faction's trade partners
    let mut to_sever: Vec<(u64, u64)> = Vec::new();

    if let Some(e) = ctx.world.entities.get(&settlement_id) {
        for target in e.active_rels(RelationshipKind::TradeRoute) {
            to_sever.push((settlement_id, target));
        }
    }

    // Also find incoming routes to this settlement
    for e in ctx.world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.id != settlement_id
            && e.has_active_rel(RelationshipKind::TradeRoute, settlement_id)
        {
            to_sever.push((e.id, settlement_id));
        }
    }

    for (source, target) in to_sever {
        sever_route(ctx, source, target, time, caused_by);
    }
}

fn sever_route(
    ctx: &mut TickContext,
    source: u64,
    target: u64,
    time: SimTimestamp,
    caused_by: u64,
) {
    // End the TradeRoute relationship
    if let Some(e) = ctx.world.entities.get_mut(&source) {
        for r in &mut e.relationships {
            if r.kind == RelationshipKind::TradeRoute
                && r.target_entity_id == target
                && r.end.is_none()
            {
                r.end = Some(time);
            }
        }
    }

    // Remove from trade_routes struct field
    if let Some(e) = ctx.world.entities.get_mut(&source)
        && let Some(sd) = e.data.as_settlement_mut()
    {
        sd.trade_routes.retain(|r| r.target != target);
    }

    // Emit signal
    ctx.signals.push(Signal {
        event_id: caused_by,
        kind: SignalKind::TradeRouteSevered {
            from_settlement: source,
            to_settlement: target,
        },
    });
}

pub(super) fn check_trade_diplomacy(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    _year_event: u64,
) {
    // Count cross-faction trade routes and compute trade happiness bonuses
    let factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    // Count trade routes between each faction pair (BTreeMap for deterministic iteration)
    let mut faction_pair_routes: std::collections::BTreeMap<(u64, u64), usize> =
        std::collections::BTreeMap::new();
    let mut faction_trade_partner_count: std::collections::BTreeMap<u64, usize> =
        std::collections::BTreeMap::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        let Some(my_faction) = e.active_rel(RelationshipKind::MemberOf) else {
            continue;
        };

        for target in e.active_rels(RelationshipKind::TradeRoute) {
            if let Some(target_faction) = helpers::settlement_faction(ctx.world, target)
                && target_faction != my_faction
            {
                let key = if my_faction < target_faction {
                    (my_faction, target_faction)
                } else {
                    (target_faction, my_faction)
                };
                *faction_pair_routes.entry(key).or_insert(0) += 1;
            }
        }
    }

    // Compute trade happiness bonus per settlement (cross-faction route count)
    struct TradeHappinessUpdate {
        settlement_id: u64,
        bonus: f64,
    }
    let mut trade_happiness_updates: Vec<TradeHappinessUpdate> = Vec::new();
    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        let Some(my_faction) = e.active_rel(RelationshipKind::MemberOf) else {
            continue;
        };

        let mut cross_faction_route_count = 0usize;
        for target in e.active_rels(RelationshipKind::TradeRoute) {
            if let Some(target_faction) = helpers::settlement_faction(ctx.world, target)
                && target_faction != my_faction
            {
                cross_faction_route_count += 1;
            }
        }

        let bonus = (cross_faction_route_count as f64 * TRADE_HAPPINESS_PER_ROUTE)
            .min(TRADE_HAPPINESS_MAX);
        trade_happiness_updates.push(TradeHappinessUpdate {
            settlement_id: e.id,
            bonus,
        });
    }
    for u in trade_happiness_updates {
        ctx.world.settlement_mut(u.settlement_id).trade_happiness_bonus = u.bonus;
    }

    // Compute per-faction partner route counts for alliance logic
    for &fid in &factions {
        let mut partner_route_count = 0usize;
        for (&(a, b), &count) in &faction_pair_routes {
            if a == fid || b == fid {
                partner_route_count += count;
            }
        }
        *faction_trade_partner_count.entry(fid).or_insert(0) = partner_route_count;
    }

    // Store per-faction trade route counts with each partner for alliance strength calculation
    for &fid in &factions {
        let mut partner_routes: BTreeMap<u64, u32> = BTreeMap::new();
        for (&(a, b), &count) in &faction_pair_routes {
            if count == 0 {
                continue;
            }
            if a == fid {
                partner_routes.insert(b, count as u32);
            } else if b == fid {
                partner_routes.insert(a, count as u32);
            }
        }
        ctx.world.faction_mut(fid).trade_partner_routes = partner_routes;
    }

    // Trade-to-alliance: factions with trade routes may form alliances
    for (&(fa, fb), &route_count) in &faction_pair_routes {
        if route_count == 0 {
            continue;
        }

        // Check if at war
        if factions_at_war(ctx.world, fa, fb) {
            continue;
        }

        // 3% chance per year if trading (low enough to not suppress all wars)
        if route_count >= MIN_ROUTES_FOR_ALLIANCE
            && ctx.rng.random_range(0.0..1.0) < TRADE_ALLIANCE_CHANCE
        {
            let ev = ctx.world.add_event(
                EventKind::Custom("trade_alliance".to_string()),
                time,
                format!("Trade partnership led to alliance in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, fa, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, fb, ParticipantRole::Subject);
            ctx.world
                .add_relationship(fa, fb, RelationshipKind::Ally, time, ev);
            ctx.world
                .add_relationship(fb, fa, RelationshipKind::Ally, time, ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;

    #[test]
    fn scenario_trade_path_direct_neighbor() {
        let mut s = Scenario::new();
        let r1 = s.add_region("R1");
        let r2 = s.add_region("R2");
        s.make_adjacent(r1, r2);
        let world = s.build();

        let path = find_trade_path(&world, r1, r2, 6, &[]);
        assert_eq!(path, Some(vec![r2]));
    }

    #[test]
    fn scenario_trade_path_multi_hop() {
        let mut s = Scenario::new();
        let r1 = s.add_region("R1");
        let r2 = s.add_region("R2");
        let r3 = s.add_region("R3");
        let r4 = s.add_region("R4");
        s.make_adjacent(r1, r2);
        s.make_adjacent(r2, r3);
        s.make_adjacent(r3, r4);
        let world = s.build();

        let path = find_trade_path(&world, r1, r4, 6, &[]);
        assert_eq!(path, Some(vec![r2, r3, r4]));
    }

    #[test]
    fn scenario_trade_path_respects_max_hops() {
        let mut s = Scenario::new();
        let r1 = s.add_region("R1");
        let r2 = s.add_region("R2");
        let r3 = s.add_region("R3");
        let r4 = s.add_region("R4");
        s.make_adjacent(r1, r2);
        s.make_adjacent(r2, r3);
        s.make_adjacent(r3, r4);
        let world = s.build();

        // Max 2 hops: can't reach R4 from R1 (need 3 hops)
        assert_eq!(find_trade_path(&world, r1, r4, 2, &[]), None);
        // Max 3 hops: can reach R4
        assert!(find_trade_path(&world, r1, r4, 3, &[]).is_some());
    }

    #[test]
    fn scenario_trade_path_blocks_hostile_regions() {
        let mut s = Scenario::new();
        let r1 = s.add_region("R1");
        let r2 = s.add_region("R2");
        let r3 = s.add_region("R3");
        s.make_adjacent(r1, r2);
        s.make_adjacent(r2, r3);

        // Place an enemy settlement in R2
        let enemy = s.add_faction("Enemy");
        s.add_settlement("EnemyTown", enemy, r2);
        let world = s.build();

        // Without hostile factions: path exists
        assert!(find_trade_path(&world, r1, r3, 6, &[]).is_some());
        // With hostile factions: path blocked
        assert_eq!(find_trade_path(&world, r1, r3, 6, &[enemy]), None);
    }

    #[test]
    fn scenario_trade_path_same_region() {
        let mut s = Scenario::new();
        let r1 = s.add_region("R1");
        let world = s.build();

        assert_eq!(find_trade_path(&world, r1, r1, 6, &[]), Some(vec![]));
    }
}
