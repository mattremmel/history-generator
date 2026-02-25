use rand::Rng;

use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};
use crate::sim::context::TickContext;

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

use super::STABILITY_DEFAULT;

pub(super) fn update_diplomacy(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
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

                        // Decay rate modulated by strength: at 1.0+ strength, no decay
                        let dissolution_chance =
                            ALLIANCE_DISSOLUTION_BASE_CHANCE * (1.0 - strength).max(0.0);
                        if ctx.rng.random_range(0.0..1.0) < dissolution_chance {
                            ends.push(EndAction {
                                source_id: fid,
                                target_id: target,
                                kind: RelationshipKind::Ally,
                            });
                        }
                    }
                    RelationshipKind::Enemy => {
                        if ctx.rng.random_range(0.0..1.0) < ENEMY_DISSOLUTION_CHANCE {
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
            let alliance_rate = ALLIANCE_FORMATION_BASE_RATE
                * shared_enemy_mult
                * (ALLIANCE_HAPPINESS_WEIGHT + ALLIANCE_HAPPINESS_WEIGHT * avg_happiness)
                * alliance_cap
                * (1.0 + avg_prestige * ALLIANCE_PRESTIGE_BONUS_WEIGHT);

            let avg_instability = (1.0 - a.stability + 1.0 - b.stability) / 2.0;
            let rivalry_rate = RIVALRY_FORMATION_BASE_RATE
                * (RIVALRY_INSTABILITY_WEIGHT + RIVALRY_INSTABILITY_WEIGHT * avg_instability);

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
        // Use ensure_relationship: another system (economy) may have already
        // created this alliance in the same tick.
        ctx.world
            .ensure_relationship(rel.source_id, rel.target_id, rel.kind, time, ev);
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
fn calculate_alliance_strength(world: &World, faction_a: u64, faction_b: u64) -> f64 {
    let mut strength = ALLIANCE_BASE_STRENGTH;

    // Trade routes between these factions (set by economy system)
    if let Some(entity) = world.entities.get(&faction_a)
        && let Some(trade_map) = entity.extra.get("trade_partner_routes")
    {
        let key = faction_b.to_string();
        if let Some(count) = trade_map.get(&key).and_then(|v| v.as_u64()) {
            strength +=
                (count as f64 * ALLIANCE_TRADE_ROUTE_STRENGTH).min(ALLIANCE_TRADE_ROUTE_CAP);
        }
    }

    // Shared enemies
    if has_shared_enemy(world, faction_a, faction_b) {
        strength += ALLIANCE_SHARED_ENEMY_STRENGTH;
    }

    // Marriage alliance (pair-specific: faction has marriage_alliance_with_{other})
    let has_marriage_alliance = world.entities.get(&faction_a).is_some_and(|e| {
        e.extra
            .contains_key(&format!("marriage_alliance_with_{faction_b}"))
    });
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

    strength
}
