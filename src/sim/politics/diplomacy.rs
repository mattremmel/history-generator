use rand::Rng;

use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};
use crate::sim::context::TickContext;
use crate::sim::grievance as grv;

use crate::sim::helpers::entity_name;

// --- Diplomacy ---
const ALLIANCE_DISSOLUTION_BASE_CHANCE: f64 = 0.03;
const ENEMY_DISSOLUTION_CHANCE: f64 = 0.03;
const ALLIANCE_SOFT_CAP_THRESHOLD: u32 = 2;
const ALLIANCE_CAP_RATE: f64 = 0.5;
const ALLIANCE_FORMATION_BASE_RATE: f64 = 0.008;
const ALLIANCE_SHARED_ENEMY_MULTIPLIER: f64 = 2.0;
const ALLIANCE_HAPPINESS_WEIGHT: f64 = 0.5;
const ALLIANCE_PRESTIGE_BONUS_WEIGHT: f64 = 0.3;
const RIVALRY_FORMATION_BASE_RATE: f64 = 0.006;
const RIVALRY_INSTABILITY_WEIGHT: f64 = 0.5;

// --- Alliance Strength ---
const ALLIANCE_BASE_STRENGTH: f64 = 0.1;
const ALLIANCE_TRADE_ROUTE_STRENGTH: f64 = 0.2;
const ALLIANCE_TRADE_ROUTE_CAP: f64 = 0.6;
const ALLIANCE_SHARED_ENEMY_STRENGTH: f64 = 0.3;
const ALLIANCE_MARRIAGE_STRENGTH: f64 = 0.4;
const ALLIANCE_PRESTIGE_STRENGTH_WEIGHT: f64 = 0.3;
const ALLIANCE_PRESTIGE_STRENGTH_CAP: f64 = 0.2;

// --- Diplomatic Trust ---
const TRUST_DEFAULT: f64 = 1.0;
const TRUST_RECOVERY_RATE: f64 = 0.02;
const TRUST_LOW_THRESHOLD: f64 = 0.3;
const TRUST_DISSOLUTION_WEIGHT: f64 = 0.02;
const TRUST_STRENGTH_WEIGHT: f64 = 0.3;

// --- Vulnerability ---
const VULNERABILITY_AT_WAR: f64 = 0.30;
const VULNERABILITY_PLAGUE: f64 = 0.15;
const VULNERABILITY_INSTABILITY_WEIGHT: f64 = 0.4;
const VULNERABILITY_LOW_TREASURY: f64 = 0.10;
const VULNERABILITY_SINGLE_SETTLEMENT: f64 = 0.10;

use super::STABILITY_DEFAULT;

pub(super) fn update_diplomacy(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Drift diplomatic trust toward 1.0
    drift_diplomatic_trust(ctx, time);

    // Collect living factions with their properties
    struct FactionDiplo {
        id: u64,
        happiness: f64,
        stability: f64,
        ally_count: u32,
        prestige: f64,
    }

    let factions: Vec<FactionDiplo> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && !e.data.as_faction().is_some_and(|fd| {
                    fd.government_type == crate::model::GovernmentType::BanditClan
                })
        })
        .map(|e| {
            let ally_count = e.active_rels(RelationshipKind::Ally).count() as u32;
            let fd = e.data.as_faction();
            FactionDiplo {
                id: e.id,
                happiness: fd.map(|f| f.happiness).unwrap_or(STABILITY_DEFAULT),
                stability: fd.map(|f| f.stability).unwrap_or(STABILITY_DEFAULT),
                ally_count,
                prestige: fd.map(|f| f.prestige).unwrap_or(0.0),
            }
        })
        .collect();

    let faction_ids: Vec<u64> = factions.iter().map(|f| f.id).collect();

    // Check for dissolution of existing relationships
    struct EndAction {
        source_id: u64,
        target_id: u64,
        kind: RelationshipKind,
    }
    let mut ends: Vec<EndAction> = Vec::new();

    for &fid in &faction_ids {
        if let Some(entity) = ctx.world.entities.get(&fid) {
            for rel in &entity.relationships {
                if rel.end.is_some() {
                    continue;
                }
                match &rel.kind {
                    RelationshipKind::Ally => {
                        // Calculate alliance strength from all sources
                        let target = rel.target_entity_id;
                        let strength = calculate_alliance_strength(ctx.world, fid, target);

                        // Low trust increases dissolution chance
                        let trust = get_diplomatic_trust(ctx.world, fid);
                        let trust_penalty = (1.0 - trust) * TRUST_DISSOLUTION_WEIGHT;

                        // Decay rate modulated by strength: at 1.0+ strength, no decay
                        let dissolution_chance = (ALLIANCE_DISSOLUTION_BASE_CHANCE + trust_penalty)
                            * (1.0 - strength).max(0.0);
                        if ctx.rng.random_range(0.0..1.0) < dissolution_chance {
                            ends.push(EndAction {
                                source_id: fid,
                                target_id: target,
                                kind: RelationshipKind::Ally,
                            });
                        }
                    }
                    RelationshipKind::Enemy => {
                        // Grievance slows enemy dissolution
                        let enemy_grievance =
                            grv::get_grievance(ctx.world, fid, rel.target_entity_id)
                                .max(grv::get_grievance(ctx.world, rel.target_entity_id, fid));
                        let effective_dissolution =
                            ENEMY_DISSOLUTION_CHANCE * (1.0 - enemy_grievance).max(0.1);
                        if ctx.rng.random_range(0.0..1.0) < effective_dissolution {
                            ends.push(EndAction {
                                source_id: fid,
                                target_id: rel.target_entity_id,
                                kind: RelationshipKind::Enemy,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    for end in ends {
        let name_a = entity_name(ctx.world, end.source_id);
        let name_b = entity_name(ctx.world, end.target_id);
        let rel_type = match &end.kind {
            RelationshipKind::Ally => "alliance",
            RelationshipKind::Enemy => "rivalry",
            _ => "relation",
        };
        let ev = ctx.world.add_event(
            EventKind::Dissolution,
            time,
            format!("The {rel_type} between {name_a} and {name_b} ended in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, end.source_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, end.target_id, ParticipantRole::Object);
        ctx.world
            .end_relationship(end.source_id, end.target_id, end.kind.clone(), time, ev);
    }

    // Check for new relationships between unrelated pairs
    struct NewRelAction {
        source_id: u64,
        target_id: u64,
        kind: RelationshipKind,
    }
    let mut new_rels: Vec<NewRelAction> = Vec::new();

    for i in 0..factions.len() {
        for j in (i + 1)..factions.len() {
            let a = &factions[i];
            let b = &factions[j];

            if has_active_diplomatic_rel(ctx.world, a.id, b.id) {
                continue;
            }

            // Check for shared enemies (boosts alliance chance)
            let shared_enemies = has_shared_enemy(ctx.world, a.id, b.id);

            // Alliance soft cap: halve rate if either has 2+ alliances
            let alliance_cap = if a.ally_count >= ALLIANCE_SOFT_CAP_THRESHOLD
                || b.ally_count >= ALLIANCE_SOFT_CAP_THRESHOLD
            {
                ALLIANCE_CAP_RATE
            } else {
                1.0
            };

            let avg_happiness = (a.happiness + b.happiness) / 2.0;
            let avg_prestige = (a.prestige + b.prestige) / 2.0;
            let shared_enemy_mult = if shared_enemies {
                ALLIANCE_SHARED_ENEMY_MULTIPLIER
            } else {
                1.0
            };

            // Trust gates alliance formation
            let trust_a = get_diplomatic_trust(ctx.world, a.id);
            let trust_b = get_diplomatic_trust(ctx.world, b.id);
            let min_trust = trust_a.min(trust_b);

            // Mutual grievance dampens alliance formation and boosts rivalry
            let mutual_grievance = grv::get_grievance(ctx.world, a.id, b.id)
                .max(grv::get_grievance(ctx.world, b.id, a.id));
            let grievance_alliance_factor = if mutual_grievance > 0.15 {
                (1.0 - mutual_grievance).max(0.0)
            } else {
                1.0
            };

            let alliance_rate = if min_trust < TRUST_LOW_THRESHOLD {
                0.0 // Too untrustworthy for alliance
            } else {
                ALLIANCE_FORMATION_BASE_RATE
                    * shared_enemy_mult
                    * (ALLIANCE_HAPPINESS_WEIGHT + ALLIANCE_HAPPINESS_WEIGHT * avg_happiness)
                    * alliance_cap
                    * (1.0 + avg_prestige * ALLIANCE_PRESTIGE_BONUS_WEIGHT)
                    * min_trust
                    * grievance_alliance_factor
            };

            let avg_instability = (1.0 - a.stability + 1.0 - b.stability) / 2.0;
            let grievance_rivalry_boost = mutual_grievance * 0.08; // up to +8%
            let rivalry_rate = RIVALRY_FORMATION_BASE_RATE
                * (RIVALRY_INSTABILITY_WEIGHT + RIVALRY_INSTABILITY_WEIGHT * avg_instability)
                + grievance_rivalry_boost;

            let roll: f64 = ctx.rng.random_range(0.0..1.0);
            if roll < alliance_rate {
                new_rels.push(NewRelAction {
                    source_id: a.id,
                    target_id: b.id,
                    kind: RelationshipKind::Ally,
                });
            } else if roll < alliance_rate + rivalry_rate {
                new_rels.push(NewRelAction {
                    source_id: a.id,
                    target_id: b.id,
                    kind: RelationshipKind::Enemy,
                });
            }
        }
    }

    for rel in new_rels {
        let name_a = entity_name(ctx.world, rel.source_id);
        let name_b = entity_name(ctx.world, rel.target_id);
        let (desc, event_kind) = match &rel.kind {
            RelationshipKind::Ally => (
                format!("{name_a} and {name_b} formed an alliance in year {current_year}"),
                EventKind::Treaty,
            ),
            RelationshipKind::Enemy => (
                format!("{name_a} and {name_b} became rivals in year {current_year}"),
                EventKind::Custom("rivalry".to_string()),
            ),
            _ => unreachable!(),
        };
        let ev = ctx.world.add_event(event_kind, time, desc);
        ctx.world
            .add_event_participant(ev, rel.source_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, rel.target_id, ParticipantRole::Object);
        ctx.world
            .add_relationship(rel.source_id, rel.target_id, rel.kind, time, ev);
    }
}

fn has_shared_enemy(world: &World, a: u64, b: u64) -> bool {
    let enemies_a: Vec<u64> = world
        .entities
        .get(&a)
        .map(|e| e.active_rels(RelationshipKind::Enemy).collect())
        .unwrap_or_default();

    if enemies_a.is_empty() {
        return false;
    }

    world
        .entities
        .get(&b)
        .map(|e| {
            e.active_rels(RelationshipKind::Enemy)
                .any(|target_id| enemies_a.contains(&target_id))
        })
        .unwrap_or(false)
}

fn has_active_diplomatic_rel(world: &World, a: u64, b: u64) -> bool {
    if let Some(entity) = world.entities.get(&a) {
        for rel in &entity.relationships {
            if rel.end.is_some() {
                continue;
            }
            if rel.target_entity_id == b
                && (rel.kind == RelationshipKind::Ally
                    || rel.kind == RelationshipKind::Enemy
                    || rel.kind == RelationshipKind::AtWar)
            {
                return true;
            }
        }
    }
    if let Some(entity) = world.entities.get(&b) {
        for rel in &entity.relationships {
            if rel.end.is_some() {
                continue;
            }
            if rel.target_entity_id == a
                && (rel.kind == RelationshipKind::Ally
                    || rel.kind == RelationshipKind::Enemy
                    || rel.kind == RelationshipKind::AtWar)
            {
                return true;
            }
        }
    }
    false
}

/// Calculate the strength of an alliance between two factions based on all
/// active reasons for being allies. Strength >= 1.0 prevents decay entirely.
///
/// Sources:
/// - Trade routes: min(route_count * ALLIANCE_TRADE_ROUTE_STRENGTH, ALLIANCE_TRADE_ROUTE_CAP)
/// - Shared enemies: ALLIANCE_SHARED_ENEMY_STRENGTH
/// - Marriage alliance: ALLIANCE_MARRIAGE_STRENGTH
/// - Base (existing alliance): ALLIANCE_BASE_STRENGTH
pub(crate) fn calculate_alliance_strength(world: &World, faction_a: u64, faction_b: u64) -> f64 {
    let mut strength = ALLIANCE_BASE_STRENGTH;

    // Trade routes between these factions (set by economy system)
    if let Some(entity) = world.entities.get(&faction_a)
        && let Some(fd) = entity.data.as_faction()
        && let Some(&count) = fd.trade_partner_routes.get(&faction_b)
    {
        strength +=
            (count as f64 * ALLIANCE_TRADE_ROUTE_STRENGTH).min(ALLIANCE_TRADE_ROUTE_CAP);
    }

    // Shared enemies
    if has_shared_enemy(world, faction_a, faction_b) {
        strength += ALLIANCE_SHARED_ENEMY_STRENGTH;
    }

    // Marriage alliance (pair-specific)
    let has_marriage_alliance = world
        .entities
        .get(&faction_a)
        .and_then(|e| e.data.as_faction())
        .is_some_and(|fd| fd.marriage_alliances.contains_key(&faction_b));
    if has_marriage_alliance {
        strength += ALLIANCE_MARRIAGE_STRENGTH;
    }

    // Prestige bonus
    let prestige_a = world
        .entities
        .get(&faction_a)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.prestige)
        .unwrap_or(0.0);
    let prestige_b = world
        .entities
        .get(&faction_b)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.prestige)
        .unwrap_or(0.0);
    let avg_prestige = (prestige_a + prestige_b) / 2.0;
    strength +=
        (avg_prestige * ALLIANCE_PRESTIGE_STRENGTH_WEIGHT).min(ALLIANCE_PRESTIGE_STRENGTH_CAP);

    // Low trust weakens alliance
    let min_trust =
        get_diplomatic_trust(world, faction_a).min(get_diplomatic_trust(world, faction_b));
    strength += (min_trust - TRUST_DEFAULT) * TRUST_STRENGTH_WEIGHT;

    strength
}

/// Get the diplomatic trust of a faction (default 1.0).
pub(crate) fn get_diplomatic_trust(world: &World, faction_id: u64) -> f64 {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|fd| fd.diplomatic_trust)
        .unwrap_or(TRUST_DEFAULT)
}

/// Compute how vulnerable an ally faction is (0.0-1.0).
/// Values >= VULNERABILITY_THRESHOLD make betrayal worth considering.
pub(crate) fn compute_ally_vulnerability(world: &World, ally_id: u64) -> f64 {
    let Some(entity) = world.entities.get(&ally_id) else {
        return 0.0;
    };
    let Some(fd) = entity.data.as_faction() else {
        return 0.0;
    };

    let mut vuln = 0.0;

    // At war
    if entity.active_rels(RelationshipKind::AtWar).next().is_some() {
        vuln += VULNERABILITY_AT_WAR;
    }

    // Has plague in any settlement
    let has_plague = world.entities.values().any(|e| {
        e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::MemberOf, ally_id)
            && e.data
                .as_settlement()
                .is_some_and(|s| s.active_disease.is_some())
    });
    if has_plague {
        vuln += VULNERABILITY_PLAGUE;
    }

    // Low stability
    if fd.stability < 0.5 {
        vuln += (0.5 - fd.stability) * VULNERABILITY_INSTABILITY_WEIGHT;
    }

    // Low treasury
    if fd.treasury < 5.0 {
        vuln += VULNERABILITY_LOW_TREASURY;
    }

    // Only one settlement
    let settlement_count = world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, ally_id)
        })
        .count();
    if settlement_count <= 1 {
        vuln += VULNERABILITY_SINGLE_SETTLEMENT;
    }

    vuln.clamp(0.0, 1.0)
}

/// Drift diplomatic trust toward 1.0 at TRUST_RECOVERY_RATE per year.
fn drift_diplomatic_trust(ctx: &mut TickContext, time: SimTimestamp) {
    let faction_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.data
                    .as_faction()
                    .is_some_and(|fd| fd.diplomatic_trust < TRUST_DEFAULT)
        })
        .map(|e| e.id)
        .collect();

    for fid in faction_ids {
        let current = get_diplomatic_trust(ctx.world, fid);
        let new_trust = (current + TRUST_RECOVERY_RATE).min(TRUST_DEFAULT);
        let ev = ctx.world.add_event(
            EventKind::Custom("trust_recovery".to_string()),
            time,
            format!("Diplomatic trust recovering for faction {fid}"),
        );
        ctx.world.record_change(
            fid,
            ev,
            "diplomatic_trust",
            serde_json::json!(current),
            serde_json::json!(new_trust),
        );
        ctx.world.faction_mut(fid).diplomatic_trust = new_trust;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use crate::sim::politics::PoliticsSystem;
    use crate::testutil;

    #[test]
    fn scenario_diplomatic_trust_recovers_over_time() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        s.set_diplomatic_trust(setup.faction, 0.5);
        let mut world = s.build();

        // Run politics for a few years so trust drifts
        for _ in 0..5 {
            testutil::tick_system(&mut world, &mut PoliticsSystem, 100, 42);
        }

        let trust = get_diplomatic_trust(&world, setup.faction);
        // 0.5 + 5 * 0.02 = 0.60
        assert!(trust > 0.5, "trust should recover over time, got {trust}");
        assert!(trust <= 1.0, "trust should not exceed 1.0, got {trust}");
    }

    #[test]
    fn scenario_diplomatic_trust_reduces_alliance_formation() {
        // With low trust, alliance formation rate should be 0 (blocked below 0.3)
        let mut s = Scenario::at_year(100);
        let setup_a = s.add_settlement_standalone("Town A");
        let setup_b = s.add_settlement_standalone("Town B");
        s.set_diplomatic_trust(setup_a.faction, 0.1); // Below threshold
        let mut world = s.build();

        // Run many ticks — alliance should never form due to low trust
        for _ in 0..50 {
            testutil::tick_system(&mut world, &mut PoliticsSystem, 100, 42);
        }

        let has_alliance = world.entities[&setup_a.faction]
            .active_rels(RelationshipKind::Ally)
            .any(|id| id == setup_b.faction);
        assert!(!has_alliance, "low-trust faction should not form alliances");
    }

    #[test]
    fn scenario_compute_ally_vulnerability() {
        let mut s = Scenario::at_year(100);
        // Healthy faction with settlement
        let healthy_setup = s.add_settlement_standalone_with(
            "Strong Town",
            |f| {
                f.stability = 0.8;
                f.treasury = 100.0;
            },
            |_| {},
        );

        // Weak faction: low stability, low treasury, single settlement
        let weak_setup = s.add_settlement_standalone_with(
            "Weak Town",
            |f| {
                f.stability = 0.2;
                f.treasury = 2.0;
            },
            |_| {},
        );

        let world = s.build();

        let healthy_vuln = compute_ally_vulnerability(&world, healthy_setup.faction);
        let weak_vuln = compute_ally_vulnerability(&world, weak_setup.faction);

        assert!(
            healthy_vuln < 0.3,
            "healthy faction should have low vulnerability: {healthy_vuln}"
        );
        assert!(
            weak_vuln >= 0.3,
            "weak faction should be vulnerable: {weak_vuln}"
        );
    }
}
