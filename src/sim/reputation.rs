use rand::Rng;

use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::traits::Trait;
use crate::model::{EntityKind, EventKind, RelationshipKind, SiegeOutcome, SimTimestamp};
use crate::sim::helpers;

pub struct ReputationSystem;

impl SimSystem for ReputationSystem {
    fn name(&self) -> &str {
        "reputation"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let year_event = ctx.world.add_event(
            EventKind::Custom("reputation_tick".to_string()),
            time,
            format!("Reputation update in year {}", time.year()),
        );

        update_person_prestige(ctx, time, year_event);
        update_faction_prestige(ctx, time, year_event);
        update_settlement_prestige(ctx, time, year_event);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let year_event = ctx.world.add_event(
            EventKind::Custom("reputation_signal".to_string()),
            time,
            format!("Reputation signal processing in year {}", time.year()),
        );

        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::WarEnded {
                    winner_id,
                    loser_id,
                    decisive,
                    ..
                } => {
                    if *decisive {
                        apply_faction_prestige_delta(ctx.world, *winner_id, 0.15, year_event);
                        apply_faction_prestige_delta(ctx.world, *loser_id, -0.15, year_event);
                        // Boost/penalize faction leaders
                        if let Some(leader_id) = helpers::faction_leader(ctx.world, *winner_id) {
                            apply_person_prestige_delta(ctx.world, leader_id, 0.10, year_event);
                        }
                        if let Some(leader_id) = helpers::faction_leader(ctx.world, *loser_id) {
                            apply_person_prestige_delta(ctx.world, leader_id, -0.05, year_event);
                        }
                    } else {
                        apply_faction_prestige_delta(ctx.world, *winner_id, 0.05, year_event);
                        apply_faction_prestige_delta(ctx.world, *loser_id, -0.05, year_event);
                    }
                }
                SignalKind::SettlementCaptured {
                    new_faction_id,
                    old_faction_id,
                    ..
                } => {
                    apply_faction_prestige_delta(ctx.world, *new_faction_id, 0.03, year_event);
                    apply_faction_prestige_delta(ctx.world, *old_faction_id, -0.05, year_event);
                }
                SignalKind::SiegeEnded {
                    attacker_faction_id,
                    defender_faction_id,
                    outcome,
                    ..
                } => {
                    if *outcome == SiegeOutcome::Conquered {
                        apply_faction_prestige_delta(
                            ctx.world,
                            *attacker_faction_id,
                            0.05,
                            year_event,
                        );
                    } else if *outcome == SiegeOutcome::Lifted {
                        apply_faction_prestige_delta(
                            ctx.world,
                            *defender_faction_id,
                            0.05,
                            year_event,
                        );
                    }
                }
                SignalKind::BuildingConstructed { settlement_id, .. } => {
                    apply_settlement_prestige_delta(ctx.world, *settlement_id, 0.02, year_event);
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *settlement_id) {
                        apply_faction_prestige_delta(ctx.world, fid, 0.01, year_event);
                    }
                }
                SignalKind::BuildingUpgraded { settlement_id, .. } => {
                    apply_settlement_prestige_delta(ctx.world, *settlement_id, 0.03, year_event);
                    if let Some(fid) = helpers::settlement_faction(ctx.world, *settlement_id) {
                        apply_faction_prestige_delta(ctx.world, fid, 0.01, year_event);
                    }
                }
                SignalKind::TradeRouteEstablished {
                    from_settlement,
                    to_settlement,
                    from_faction,
                    to_faction,
                    ..
                } => {
                    apply_settlement_prestige_delta(ctx.world, *from_settlement, 0.01, year_event);
                    apply_settlement_prestige_delta(ctx.world, *to_settlement, 0.01, year_event);
                    apply_faction_prestige_delta(ctx.world, *from_faction, 0.005, year_event);
                    apply_faction_prestige_delta(ctx.world, *to_faction, 0.005, year_event);
                }
                SignalKind::PlagueEnded { settlement_id, .. } => {
                    apply_settlement_prestige_delta(ctx.world, *settlement_id, 0.02, year_event);
                }
                SignalKind::FactionSplit { old_faction_id, .. } => {
                    apply_faction_prestige_delta(ctx.world, *old_faction_id, -0.10, year_event);
                }
                SignalKind::CulturalRebellion { faction_id, .. } => {
                    apply_faction_prestige_delta(ctx.world, *faction_id, -0.05, year_event);
                }
                SignalKind::TreasuryDepleted { faction_id } => {
                    apply_faction_prestige_delta(ctx.world, *faction_id, -0.05, year_event);
                }
                SignalKind::EntityDied { entity_id } => {
                    // If a leader died, penalize their faction
                    let faction_ids: Vec<u64> = ctx
                        .world
                        .entities
                        .get(entity_id)
                        .filter(|e| e.kind == EntityKind::Person)
                        .map(|e| {
                            e.relationships
                                .iter()
                                .filter(|r| r.kind == RelationshipKind::LeaderOf && r.end.is_none())
                                .map(|r| r.target_entity_id)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for fid in faction_ids {
                        apply_faction_prestige_delta(ctx.world, fid, -0.03, year_event);
                    }
                }
                SignalKind::DisasterStruck {
                    settlement_id,
                    severity,
                    ..
                } => {
                    // Disaster reduces settlement prestige based on severity
                    apply_settlement_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        -0.05 * severity,
                        year_event,
                    );
                    // Large disasters also affect the owning faction
                    if *severity > 0.5
                        && let Some(faction_id) = helpers::settlement_faction(ctx.world, *settlement_id)
                    {
                        apply_faction_prestige_delta(ctx.world, faction_id, -0.03, year_event);
                    }
                }
                SignalKind::DisasterEnded { settlement_id, .. } => {
                    // Surviving a disaster shows resilience
                    apply_settlement_prestige_delta(ctx.world, *settlement_id, 0.02, year_event);
                }
                SignalKind::KnowledgeCreated {
                    settlement_id,
                    significance,
                    ..
                } => {
                    // Knowledge creation gives small prestige to origin settlement
                    apply_settlement_prestige_delta(
                        ctx.world,
                        *settlement_id,
                        0.01 * significance,
                        year_event,
                    );
                }
                _ => {}
            }
        }

        // Check for tier changes and emit threshold signals
        emit_threshold_signals(ctx, year_event);
    }
}

// ---------------------------------------------------------------------------
// Prestige tiers
// ---------------------------------------------------------------------------

fn prestige_tier(prestige: f64) -> u8 {
    match prestige {
        p if p >= 0.8 => 4,
        p if p >= 0.6 => 3,
        p if p >= 0.4 => 2,
        p if p >= 0.2 => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Person prestige convergence
// ---------------------------------------------------------------------------

fn update_person_prestige(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    struct PersonInfo {
        id: u64,
        old_prestige: f64,
        target: f64,
        convergence_rate: f64,
    }

    let current_year = time.year();

    // Collect person info
    let persons: Vec<PersonInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
        .filter_map(|e| {
            let pd = e.data.as_person()?;
            // Only track prestige for notable NPCs (those with traits)
            if pd.traits.is_empty() {
                return None;
            }

            let mut base_target = 0.05;

            // Leadership bonus
            let leader_faction = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LeaderOf && r.end.is_none());
            if let Some(lr) = leader_faction {
                base_target += 0.15;
                // Count settlements belonging to their faction
                let settlement_count =
                    helpers::faction_settlements(ctx.world, lr.target_entity_id).len();
                if settlement_count >= 3 {
                    base_target += 0.10;
                }
                if settlement_count >= 6 {
                    base_target += 0.10;
                }
            }

            // Role bonus
            match pd.role.as_str() {
                "warrior" => base_target += 0.05,
                "elder" => base_target += 0.04,
                "scholar" => base_target += 0.03,
                _ => {}
            }

            // Longevity bonus (age >= 50)
            if current_year > pd.birth_year {
                let age = current_year - pd.birth_year;
                if age >= 50 {
                    base_target += 0.02 * ((age - 50) as f64 / 30.0).min(1.0);
                }
            }

            let target = base_target.clamp(0.0, 0.85);

            // Trait-based convergence rate modifier
            let mut trait_mult = 1.0;
            for t in &pd.traits {
                match t {
                    Trait::Ambitious => trait_mult *= 1.3,
                    Trait::Charismatic => trait_mult *= 1.2,
                    Trait::Content => trait_mult *= 0.7,
                    Trait::Reclusive => trait_mult *= 0.5,
                    _ => {}
                }
            }

            Some(PersonInfo {
                id: e.id,
                old_prestige: pd.prestige,
                target,
                convergence_rate: 0.10 * trait_mult,
            })
        })
        .collect();

    // Apply
    for p in persons {
        let noise = ctx.rng.random_range(-0.01..0.01);
        let new_prestige =
            (p.old_prestige + (p.target - p.old_prestige) * p.convergence_rate + noise)
                .clamp(0.0, 1.0);

        if let Some(entity) = ctx.world.entities.get_mut(&p.id)
            && let Some(pd) = entity.data.as_person_mut()
        {
            pd.prestige = new_prestige;
        }
        ctx.world.record_change(
            p.id,
            year_event,
            "prestige",
            serde_json::json!(p.old_prestige),
            serde_json::json!(new_prestige),
        );
    }
}

// ---------------------------------------------------------------------------
// Faction prestige convergence
// ---------------------------------------------------------------------------

fn update_faction_prestige(ctx: &mut TickContext, _time: SimTimestamp, year_event: u64) {
    struct FactionInfo {
        id: u64,
        old_prestige: f64,
        target: f64,
    }

    let factions: Vec<FactionInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .filter_map(|e| {
            let fd = e.data.as_faction()?;
            let faction_id = e.id;

            let mut base_target = 0.10;

            // Territory size
            let settlement_count = helpers::faction_settlements(ctx.world, faction_id).len();
            base_target += (settlement_count as f64 * 0.05).min(0.30);

            // Average settlement prosperity
            let avg_prosperity = avg_faction_prosperity(ctx.world, faction_id);
            base_target += avg_prosperity * 0.15;

            // Trade routes
            let trade_count = count_faction_trade_routes(ctx.world, faction_id);
            base_target += (trade_count as f64 * 0.02).min(0.10);

            // Infrastructure (buildings)
            let building_count = count_faction_buildings(ctx.world, faction_id);
            base_target += (building_count as f64 * 0.01).min(0.10);

            // Governance
            base_target += fd.stability * 0.05 + fd.legitimacy * 0.05;

            // Leader prestige contribution
            if let Some(leader_prestige) = get_leader_prestige(ctx.world, faction_id) {
                base_target += leader_prestige * 0.10;
            }

            let target = base_target.clamp(0.0, 0.90);

            Some(FactionInfo {
                id: faction_id,
                old_prestige: fd.prestige,
                target,
            })
        })
        .collect();

    // Apply
    for f in factions {
        let noise = ctx.rng.random_range(-0.02..0.02);
        let new_prestige =
            (f.old_prestige + (f.target - f.old_prestige) * 0.12 + noise).clamp(0.0, 1.0);

        if let Some(entity) = ctx.world.entities.get_mut(&f.id)
            && let Some(fd) = entity.data.as_faction_mut()
        {
            fd.prestige = new_prestige;
        }
        ctx.world.record_change(
            f.id,
            year_event,
            "prestige",
            serde_json::json!(f.old_prestige),
            serde_json::json!(new_prestige),
        );
    }
}

// ---------------------------------------------------------------------------
// Settlement prestige convergence
// ---------------------------------------------------------------------------

fn update_settlement_prestige(ctx: &mut TickContext, _time: SimTimestamp, year_event: u64) {
    struct SettlementInfo {
        id: u64,
        old_prestige: f64,
        target: f64,
    }

    let settlements: Vec<SettlementInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            let settlement_id = e.id;

            let mut base_target = 0.05;

            // Population milestones
            let pop = sd.population;
            if pop >= 100 {
                base_target += 0.05;
            }
            if pop >= 500 {
                base_target += 0.10;
            }
            if pop >= 1000 {
                base_target += 0.10;
            }
            if pop >= 2000 {
                base_target += 0.05;
            }

            // Prosperity
            base_target += sd.prosperity * 0.10;

            // Buildings
            let building_count = helpers::settlement_building_count(ctx.world, settlement_id);
            base_target += (building_count as f64 * 0.03).min(0.15);

            // Fortifications
            base_target += sd.fortification_level as f64 * 0.02;

            // Trade routes
            let trade_count = count_settlement_trade_routes(e);
            base_target += (trade_count as f64 * 0.03).min(0.10);

            // Written manifestations (knowledge/library prestige)
            let written_count = count_settlement_written_manifestations(ctx.world, settlement_id);
            if written_count > 30 {
                base_target += 0.05;
            } else if written_count > 15 {
                base_target += 0.03;
            } else if written_count > 5 {
                base_target += 0.02;
            }

            // Siege penalty
            if sd.active_siege.is_some() {
                base_target -= 0.10;
            }

            let target = base_target.clamp(0.0, 0.85);

            Some(SettlementInfo {
                id: settlement_id,
                old_prestige: sd.prestige,
                target,
            })
        })
        .collect();

    // Apply
    for s in settlements {
        let noise = ctx.rng.random_range(-0.01..0.01);
        let new_prestige =
            (s.old_prestige + (s.target - s.old_prestige) * 0.08 + noise).clamp(0.0, 1.0);

        if let Some(entity) = ctx.world.entities.get_mut(&s.id)
            && let Some(sd) = entity.data.as_settlement_mut()
        {
            sd.prestige = new_prestige;
        }
        ctx.world.record_change(
            s.id,
            year_event,
            "prestige",
            serde_json::json!(s.old_prestige),
            serde_json::json!(new_prestige),
        );
    }
}

// ---------------------------------------------------------------------------
// Delta helpers
// ---------------------------------------------------------------------------

fn apply_faction_prestige_delta(
    world: &mut crate::model::World,
    faction_id: u64,
    delta: f64,
    event_id: u64,
) {
    let Some(entity) = world.entities.get_mut(&faction_id) else {
        return;
    };
    let Some(fd) = entity.data.as_faction_mut() else {
        return;
    };
    let old = fd.prestige;
    fd.prestige = (fd.prestige + delta).clamp(0.0, 1.0);
    let new = fd.prestige;
    world.record_change(
        faction_id,
        event_id,
        "prestige",
        serde_json::json!(old),
        serde_json::json!(new),
    );
}

fn apply_person_prestige_delta(
    world: &mut crate::model::World,
    person_id: u64,
    delta: f64,
    event_id: u64,
) {
    let Some(entity) = world.entities.get_mut(&person_id) else {
        return;
    };
    let Some(pd) = entity.data.as_person_mut() else {
        return;
    };
    let old = pd.prestige;
    pd.prestige = (pd.prestige + delta).clamp(0.0, 1.0);
    let new = pd.prestige;
    world.record_change(
        person_id,
        event_id,
        "prestige",
        serde_json::json!(old),
        serde_json::json!(new),
    );
}

fn apply_settlement_prestige_delta(
    world: &mut crate::model::World,
    settlement_id: u64,
    delta: f64,
    event_id: u64,
) {
    let Some(entity) = world.entities.get_mut(&settlement_id) else {
        return;
    };
    let Some(sd) = entity.data.as_settlement_mut() else {
        return;
    };
    let old = sd.prestige;
    sd.prestige = (sd.prestige + delta).clamp(0.0, 1.0);
    let new = sd.prestige;
    world.record_change(
        settlement_id,
        event_id,
        "prestige",
        serde_json::json!(old),
        serde_json::json!(new),
    );
}

// ---------------------------------------------------------------------------
// Threshold signal emission
// ---------------------------------------------------------------------------

fn emit_threshold_signals(ctx: &mut TickContext, event_id: u64) {
    // We need to track old tiers — store them before tick in a pre-pass.
    // Since handle_signals runs after tick, the prestige values have already
    // been updated by both tick() and signal deltas. We check extras for
    // the previous tier, stored at end of each cycle.
    for e in ctx.world.entities.values() {
        if e.end.is_some() {
            continue;
        }

        let current_prestige = match e.kind {
            EntityKind::Person => e.data.as_person().map(|p| p.prestige),
            EntityKind::Faction => e.data.as_faction().map(|f| f.prestige),
            EntityKind::Settlement => e.data.as_settlement().map(|s| s.prestige),
            _ => None,
        };

        if let Some(prestige) = current_prestige {
            let new_tier = prestige_tier(prestige);
            let old_tier = e.extra_u64("prestige_tier").map(|v| v as u8).unwrap_or(0);

            if new_tier != old_tier {
                ctx.signals.push(Signal {
                    event_id,
                    kind: SignalKind::PrestigeThresholdCrossed {
                        entity_id: e.id,
                        old_tier,
                        new_tier,
                    },
                });
            }
        }
    }

    // Update stored tiers for next cycle
    let tier_updates: Vec<(u64, u8)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.end.is_none())
        .filter_map(|e| {
            let prestige = match e.kind {
                EntityKind::Person => e.data.as_person().map(|p| p.prestige),
                EntityKind::Faction => e.data.as_faction().map(|f| f.prestige),
                EntityKind::Settlement => e.data.as_settlement().map(|s| s.prestige),
                _ => None,
            }?;
            Some((e.id, prestige_tier(prestige)))
        })
        .collect();

    for (id, tier) in tier_updates {
        ctx.world.set_extra(
            id,
            "prestige_tier".to_string(),
            serde_json::json!(tier),
            event_id,
        );
    }
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Average prosperity of a faction's settlements.
fn avg_faction_prosperity(world: &crate::model::World, faction_id: u64) -> f64 {
    let mut sum = 0.0;
    let mut count = 0u32;
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
        {
            if let Some(sd) = e.data.as_settlement() {
                sum += sd.prosperity;
            }
            count += 1;
        }
    }
    if count > 0 { sum / count as f64 } else { 0.3 }
}

/// Count trade routes across all faction settlements.
fn count_faction_trade_routes(world: &crate::model::World, faction_id: u64) -> usize {
    let mut count = 0;
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
        {
            count += e
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::TradeRoute && r.end.is_none())
                .count();
        }
    }
    count
}

/// Count buildings belonging to a faction's settlements.
fn count_faction_buildings(world: &crate::model::World, faction_id: u64) -> usize {
    // Collect faction settlement IDs
    let settlement_ids: Vec<u64> = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .map(|e| e.id)
        .collect();

    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Building
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LocatedIn
                        && settlement_ids.contains(&r.target_entity_id)
                        && r.end.is_none()
                })
        })
        .count()
}

/// Count active trade routes on a settlement entity.
fn count_settlement_trade_routes(entity: &crate::model::Entity) -> usize {
    entity
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::TradeRoute && r.end.is_none())
        .count()
}

/// Get prestige of a faction's leader.
fn get_leader_prestige(world: &crate::model::World, faction_id: u64) -> Option<f64> {
    let leader_id = helpers::faction_leader(world, faction_id)?;
    let leader = world.entities.get(&leader_id)?;
    leader.data.as_person().map(|p| p.prestige)
}

/// Count written manifestations (books, scrolls) held by a settlement.
fn count_settlement_written_manifestations(
    world: &crate::model::World,
    settlement_id: u64,
) -> usize {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Manifestation
                && e.end.is_none()
                && e.data.as_manifestation().is_some_and(|md| {
                    matches!(
                        md.medium,
                        crate::model::Medium::WrittenBook
                            | crate::model::Medium::Scroll
                            | crate::model::Medium::EncodedCipher
                    )
                })
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::HeldBy
                        && r.target_entity_id == settlement_id
                        && r.end.is_none()
                })
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use crate::testutil::{
        PoliticalSetup, assert_approx, deliver_signals, get_faction, get_person, get_settlement,
        has_signal, political_scenario, tick_system,
    };
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn prestige_tier_thresholds() {
        assert_eq!(prestige_tier(0.0), 0);
        assert_eq!(prestige_tier(0.19), 0);
        assert_eq!(prestige_tier(0.2), 1);
        assert_eq!(prestige_tier(0.39), 1);
        assert_eq!(prestige_tier(0.4), 2);
        assert_eq!(prestige_tier(0.59), 2);
        assert_eq!(prestige_tier(0.6), 3);
        assert_eq!(prestige_tier(0.79), 3);
        assert_eq!(prestige_tier(0.8), 4);
        assert_eq!(prestige_tier(1.0), 4);
    }

    #[test]
    fn scenario_leader_prestige_converges_upward() {
        let PoliticalSetup {
            mut world, leader, ..
        } = political_scenario();

        for year in 100..120 {
            tick_system(&mut world, &mut ReputationSystem, year, 42);
        }

        let prestige = get_person(&world, leader).prestige;
        assert!(
            prestige > 0.15,
            "leader prestige should rise, got {prestige}"
        );
    }

    #[test]
    fn scenario_non_leader_prestige_stays_low() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let faction = setup.faction;
        let commoner = s.person("Commoner", faction).role("common").traits(vec![Trait::Content]).id();
        let mut world = s.build();

        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        for _ in 0..20 {
            let time = ctx.world.current_time;
            update_person_prestige(&mut ctx, time, year_event);
        }

        let prestige = get_person(ctx.world, commoner).prestige;
        assert!(
            prestige < 0.15,
            "commoner prestige should stay low, got {prestige}"
        );
    }

    #[test]
    fn scenario_faction_prestige_scales_with_territory() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let small_faction = s.add_faction("Small");
        s.add_settlement("Town", small_faction, region);

        let large_faction = s.add_faction("Large");
        for i in 0..5 {
            let r = s.add_region(&format!("Region{i}"));
            s.add_settlement(&format!("City{i}"), large_faction, r);
        }
        let mut world = s.build();

        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        for _ in 0..30 {
            let time = ctx.world.current_time;
            update_faction_prestige(&mut ctx, time, year_event);
        }

        let small_p = get_faction(ctx.world, small_faction).prestige;
        let large_p = get_faction(ctx.world, large_faction).prestige;
        assert!(
            large_p > small_p,
            "larger faction should have more prestige: large={large_p} small={small_p}"
        );
    }

    #[test]
    fn scenario_settlement_prestige_scales_with_population() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.add_faction("Kingdom");
        let village = s.settlement("Village", faction, region).population(50).id();
        let city = s.settlement("City", faction, region).population(1500).prosperity(0.7).fortification_level(2).id();
        let mut world = s.build();

        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        for _ in 0..30 {
            let time = ctx.world.current_time;
            update_settlement_prestige(&mut ctx, time, year_event);
        }

        let village_p = get_settlement(ctx.world, village).prestige;
        let city_p = get_settlement(ctx.world, city).prestige;
        assert!(
            city_p > village_p,
            "city should have more prestige: city={city_p} village={village_p}"
        );
    }

    #[test]
    fn scenario_war_victory_boosts_faction_prestige() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let winner = s.faction("Winners").prestige(0.3).id();
        s.add_settlement("Capital", winner, region);
        s.add_settlement("Town", winner, region);
        let loser = s.faction("Losers").prestige(0.3).id();
        s.add_settlement("Outpost", loser, region);
        s.add_settlement("Village", loser, region);
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::WarEnded {
                winner_id: winner,
                loser_id: loser,
                decisive: true,
                reparations: 0.0,
                tribute_years: 0,
            },
        }];

        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert_approx(
            get_faction(&world, winner).prestige,
            0.45,
            0.001,
            "winner +0.15",
        );
        assert_approx(
            get_faction(&world, loser).prestige,
            0.15,
            0.001,
            "loser -0.15",
        );
    }

    #[test]
    fn scenario_threshold_signal_emitted_on_tier_change() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.faction("Kingdom").prestige(0.19).id();
        s.add_settlement("Capital", faction, region);
        s.add_settlement("Town", faction, region);
        let mut world = s.build();

        let year_event = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );

        apply_faction_prestige_delta(&mut world, faction, 0.05, year_event);

        let prestige = get_faction(&world, faction).prestige;
        assert!(prestige >= 0.2, "prestige should cross 0.2, got {prestige}");

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals_out = vec![];
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals_out,
            inbox: &[],
        };

        emit_threshold_signals(&mut ctx, year_event);

        assert!(has_signal(&signals_out, |sk| matches!(
            sk,
            SignalKind::PrestigeThresholdCrossed {
                entity_id,
                old_tier: 0,
                new_tier: 1,
            } if *entity_id == faction
        )));
    }

    #[test]
    fn scenario_prestige_stays_bounded_after_extreme_signals() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let winner = s.faction("Winners").prestige(0.95).id();
        s.add_settlement("Capital", winner, region);
        let loser = s.faction("Losers").prestige(0.05).id();
        s.add_settlement("Outpost", loser, region);
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::WarEnded {
                winner_id: winner,
                loser_id: loser,
                decisive: true,
                reparations: 100.0,
                tribute_years: 5,
            },
        }];

        deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        let winner_prestige = get_faction(&world, winner).prestige;
        let loser_prestige = get_faction(&world, loser).prestige;
        assert!(
            winner_prestige <= 1.0,
            "winner prestige should be clamped to 1.0, got {winner_prestige}"
        );
        assert!(
            loser_prestige >= 0.0,
            "loser prestige should be clamped to 0.0, got {loser_prestige}"
        );
    }
}
