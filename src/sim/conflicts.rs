use std::collections::VecDeque;

use rand::Rng;

use super::context::TickContext;
use super::population::PopulationBreakdown;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::action::ActionKind;
use crate::model::traits::{Trait, has_trait};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};
use crate::worldgen::terrain::Terrain;

// --- Constants ---

const WAR_DECLARATION_BASE_CHANCE: f64 = 0.04;
const DRAFT_RATE: f64 = 0.15;
const MIN_ARMY_STRENGTH: u32 = 20;
const TERRAIN_BONUS_MOUNTAINS: f64 = 1.3;
const TERRAIN_BONUS_FOREST: f64 = 1.15;
const LOSER_CASUALTY_MIN: f64 = 0.25;
const LOSER_CASUALTY_MAX: f64 = 0.40;
const WINNER_CASUALTY_MIN: f64 = 0.10;
const WINNER_CASUALTY_MAX: f64 = 0.20;
const WAR_EXHAUSTION_START_YEAR: u32 = 5;
const PEACE_CHANCE_PER_YEAR: f64 = 0.15;
const WARRIOR_DEATH_CHANCE: f64 = 0.15;
const NON_WARRIOR_DEATH_CHANCE: f64 = 0.05;

// Movement & Supply
const STARTING_SUPPLY_MONTHS: f64 = 3.0;

// Forage rates (fraction of monthly consumption recovered)
const FORAGE_FRIENDLY: f64 = 0.8;
const FORAGE_NEUTRAL: f64 = 0.4;
const FORAGE_ENEMY: f64 = 0.15;

// Terrain forage multipliers
const FORAGE_PLAINS: f64 = 1.3;
const FORAGE_FOREST: f64 = 1.0;
const FORAGE_HILLS: f64 = 0.8;
const FORAGE_MOUNTAINS: f64 = 0.4;
const FORAGE_DESERT: f64 = 0.1;
const FORAGE_SWAMP: f64 = 0.6;
const FORAGE_TUNDRA: f64 = 0.2;
const FORAGE_JUNGLE: f64 = 0.7;
const FORAGE_DEFAULT: f64 = 0.5;
const FORAGE_COAST: f64 = 1.3;

// Disease attrition (fraction of strength lost per month)
const DISEASE_BASE: f64 = 0.005;
const DISEASE_SWAMP: f64 = 0.03;
const DISEASE_JUNGLE: f64 = 0.025;
const DISEASE_DESERT: f64 = 0.015;
const DISEASE_TUNDRA: f64 = 0.02;
const DISEASE_MOUNTAINS_RATE: f64 = 0.01;

// Starvation
const STARVATION_RATE: f64 = 0.15;

// Morale
const MORALE_DECAY_PER_MONTH: f64 = 0.02;
const HOME_TERRITORY_MORALE_BOOST: f64 = 0.05;
const STARVATION_MORALE_PENALTY: f64 = 0.10;

// Retreat thresholds
const RETREAT_MORALE_THRESHOLD: f64 = 0.2;
const RETREAT_STRENGTH_RATIO: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerritoryStatus {
    Friendly,
    Neutral,
    Enemy,
}

pub struct ConflictSystem;

impl SimSystem for ConflictSystem {
    fn name(&self) -> &str {
        "conflicts"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Monthly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();
        let is_year_start = time.day() == 1;

        // Yearly pre-steps: declarations and mustering
        if is_year_start {
            check_war_declarations(ctx, time, current_year);
            muster_armies(ctx, time, current_year);
        }

        // Monthly steps
        apply_supply_and_attrition(ctx, time, current_year);
        move_armies(ctx, time, current_year);
        resolve_battles(ctx, time, current_year);
        check_retreats(ctx, time, current_year);
        check_conquests(ctx, time, current_year);

        // Yearly post-step: war endings (after monthly combat/conquest cycle)
        if is_year_start {
            check_war_endings(ctx, time, current_year);
        }
    }
}

// --- Step 1: War Declarations ---

fn check_war_declarations(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect living faction pairs with active Enemy relationship
    struct EnemyPair {
        a: u64,
        b: u64,
        avg_stability: f64,
    }

    let factions: Vec<(u64, f64)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| {
            let stability = e
                .properties
                .get("stability")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            (e.id, stability)
        })
        .collect();

    let mut enemy_pairs: Vec<EnemyPair> = Vec::new();
    for i in 0..factions.len() {
        for j in (i + 1)..factions.len() {
            let (a, stab_a) = factions[i];
            let (b, stab_b) = factions[j];

            // Check if they are enemies
            let is_enemy = has_active_rel_of_kind(ctx.world, a, b, &RelationshipKind::Enemy);
            if !is_enemy {
                continue;
            }

            // Skip if already at war
            if has_active_rel_of_kind(ctx.world, a, b, &RelationshipKind::AtWar) {
                continue;
            }

            // Check adjacency
            if !factions_are_adjacent(ctx.world, a, b) {
                continue;
            }

            enemy_pairs.push(EnemyPair {
                a,
                b,
                avg_stability: (stab_a + stab_b) / 2.0,
            });
        }
    }

    for pair in enemy_pairs {
        // Dedup: skip if an NPC already queued DeclareWar between these factions
        let npc_war_queued = ctx.world.pending_actions.iter().any(|a| {
            if let ActionKind::DeclareWar { target_faction_id } = &a.kind {
                // Check if the actor's faction is one side and target is the other
                let actor_faction = ctx.world.entities.get(&a.actor_id).and_then(|e| {
                    e.relationships
                        .iter()
                        .find(|r| {
                            r.kind == RelationshipKind::MemberOf
                                && r.end.is_none()
                                && ctx
                                    .world
                                    .entities
                                    .get(&r.target_entity_id)
                                    .is_some_and(|t| t.kind == EntityKind::Faction)
                        })
                        .map(|r| r.target_entity_id)
                });
                if let Some(af) = actor_faction {
                    (af == pair.a && *target_faction_id == pair.b)
                        || (af == pair.b && *target_faction_id == pair.a)
                } else {
                    false
                }
            } else {
                false
            }
        });
        if npc_war_queued {
            continue;
        }

        let instability_modifier = ((1.0 - pair.avg_stability) * 2.0).clamp(0.5, 2.0);
        let mut chance = WAR_DECLARATION_BASE_CHANCE * instability_modifier;

        // Leader traits influence war declaration chance
        for &fid in &[pair.a, pair.b] {
            if let Some(leader) = find_faction_leader_entity(ctx.world, fid) {
                if has_trait(leader, &Trait::Aggressive) {
                    chance *= 1.5;
                } else if has_trait(leader, &Trait::Cautious) {
                    chance *= 0.5;
                }
            }
        }

        if ctx.rng.random_range(0.0..1.0) >= chance {
            continue;
        }

        // Pick attacker: lower stability is more aggressive
        let stab_a = get_f64_property(ctx.world, pair.a, "stability", 0.5);
        let stab_b = get_f64_property(ctx.world, pair.b, "stability", 0.5);
        let (attacker_id, defender_id) = if stab_a <= stab_b {
            (pair.a, pair.b)
        } else {
            (pair.b, pair.a)
        };

        let attacker_name = get_entity_name(ctx.world, attacker_id);
        let defender_name = get_entity_name(ctx.world, defender_id);

        let ev = ctx.world.add_event(
            EventKind::WarDeclared,
            time,
            format!("{attacker_name} declared war on {defender_name} in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, attacker_id, ParticipantRole::Attacker);
        ctx.world
            .add_event_participant(ev, defender_id, ParticipantRole::Defender);

        // Add bidirectional AtWar relationships
        ctx.world
            .add_relationship(attacker_id, defender_id, RelationshipKind::AtWar, time, ev);
        ctx.world
            .add_relationship(defender_id, attacker_id, RelationshipKind::AtWar, time, ev);

        // Set war_start_year on both factions
        ctx.world.set_property(
            attacker_id,
            "war_start_year".to_string(),
            serde_json::json!(current_year),
            ev,
        );
        ctx.world.set_property(
            defender_id,
            "war_start_year".to_string(),
            serde_json::json!(current_year),
            ev,
        );

        // End any active Ally relationship between them
        end_ally_relationship(ctx.world, attacker_id, defender_id, time, ev);

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::WarStarted {
                attacker_id,
                defender_id,
            },
        });
    }
}

// --- Step 2: Muster Armies ---

fn muster_armies(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Find factions at war that don't have a living Army
    let at_war_factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.relationships
                    .iter()
                    .any(|r| r.kind == RelationshipKind::AtWar && r.end.is_none())
        })
        .map(|e| e.id)
        .collect();

    for faction_id in at_war_factions {
        // Check if faction already has a living army
        let has_army = ctx.world.entities.values().any(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        });
        if has_army {
            continue;
        }

        // Sum able_bodied_men across faction settlements
        let mut total_able = 0u32;
        let settlement_ids: Vec<u64> = collect_faction_settlement_ids(ctx.world, faction_id);

        for &sid in &settlement_ids {
            if let Some(breakdown) = get_population_breakdown(ctx.world, sid) {
                total_able += breakdown.able_bodied_men();
            }
        }

        let draft_count = (total_able as f64 * DRAFT_RATE).round() as u32;
        if draft_count < MIN_ARMY_STRENGTH {
            continue;
        }

        // Create Army entity
        let faction_name = get_entity_name(ctx.world, faction_id);
        let ev = ctx.world.add_event(
            EventKind::Custom("army_mustered".to_string()),
            time,
            format!("{faction_name} mustered an army of {draft_count} in year {current_year}"),
        );

        let army_id = ctx.world.add_entity(
            EntityKind::Army,
            format!("Army of {faction_name}"),
            Some(time),
            ev,
        );
        ctx.world.set_property(
            army_id,
            "strength".to_string(),
            serde_json::json!(draft_count),
            ev,
        );
        ctx.world
            .set_property(army_id, "morale".to_string(), serde_json::json!(1.0), ev);
        ctx.world.set_property(
            army_id,
            "faction_id".to_string(),
            serde_json::json!(faction_id),
            ev,
        );
        ctx.world
            .add_relationship(army_id, faction_id, RelationshipKind::MemberOf, time, ev);
        ctx.world
            .add_event_participant(ev, army_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Object);

        // Set army location to faction's capital region
        if let Some((_settlement_id, region_id)) = find_faction_capital(ctx.world, faction_id) {
            ctx.world
                .add_relationship(army_id, region_id, RelationshipKind::LocatedIn, time, ev);
            ctx.world.set_property(
                army_id,
                "home_region_id".to_string(),
                serde_json::json!(region_id),
                ev,
            );
        }
        ctx.world.set_property(
            army_id,
            "starting_strength".to_string(),
            serde_json::json!(draft_count),
            ev,
        );
        ctx.world.set_property(
            army_id,
            "supply".to_string(),
            serde_json::json!(STARTING_SUPPLY_MONTHS),
            ev,
        );
        ctx.world.set_property(
            army_id,
            "months_campaigning".to_string(),
            serde_json::json!(0u32),
            ev,
        );

        // Reduce settlement populations proportionally
        apply_draft_to_settlements(ctx.world, &settlement_ids, draft_count, ev);
    }
}

fn apply_draft_to_settlements(
    world: &mut World,
    settlement_ids: &[u64],
    total_draft: u32,
    event_id: u64,
) {
    // Compute total able-bodied across all settlements for proportional distribution
    let mut settlement_able: Vec<(u64, u32)> = Vec::new();
    let mut grand_total = 0u32;
    for &sid in settlement_ids {
        let able = get_population_breakdown(world, sid)
            .map(|b| b.able_bodied_men())
            .unwrap_or(0);
        settlement_able.push((sid, able));
        grand_total += able;
    }

    if grand_total == 0 {
        return;
    }

    for (sid, able) in settlement_able {
        let proportion = able as f64 / grand_total as f64;
        let draft_from_here = (total_draft as f64 * proportion).round() as u32;
        if draft_from_here == 0 {
            continue;
        }

        if let Some(mut breakdown) = get_population_breakdown(world, sid) {
            apply_draft(&mut breakdown, draft_from_here);
            let new_pop = breakdown.total();
            world.set_property(
                sid,
                "population".to_string(),
                serde_json::json!(new_pop),
                event_id,
            );
            world.set_property(
                sid,
                "population_breakdown".to_string(),
                serde_json::to_value(&breakdown).unwrap(),
                event_id,
            );
        }
    }
}

/// Distribute draft from male brackets 2 (young_adult) and 3 (middle_age)
/// proportionally based on their relative sizes.
fn apply_draft(breakdown: &mut PopulationBreakdown, draft_count: u32) {
    let bracket2 = breakdown.male[2];
    let bracket3 = breakdown.male[3];
    let total = bracket2 + bracket3;
    if total == 0 {
        return;
    }

    let from2 =
        ((draft_count as f64 * bracket2 as f64 / total as f64).round() as u32).min(bracket2);
    let from3 = (draft_count.saturating_sub(from2)).min(bracket3);

    breakdown.male[2] = bracket2.saturating_sub(from2);
    breakdown.male[3] = bracket3.saturating_sub(from3);
}

// --- Supply & Attrition ---

fn apply_supply_and_attrition(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    let armies: Vec<(u64, u64)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .map(|e| {
            let faction_id = e
                .properties
                .get("faction_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            (e.id, faction_id)
        })
        .collect();

    for (army_id, faction_id) in armies {
        let region_id = match get_army_region(ctx.world, army_id) {
            Some(r) => r,
            None => continue,
        };

        let terrain = get_region_terrain(ctx.world, region_id);
        let territory = get_territory_status(ctx.world, region_id, faction_id);

        // Consume supply
        let mut supply = get_f64_property(ctx.world, army_id, "supply", STARTING_SUPPLY_MONTHS);
        supply -= 1.0;

        // Forage
        let forage_base = match territory {
            TerritoryStatus::Friendly => FORAGE_FRIENDLY,
            TerritoryStatus::Neutral => FORAGE_NEUTRAL,
            TerritoryStatus::Enemy => FORAGE_ENEMY,
        };
        let terrain_mod = terrain
            .as_ref()
            .map(forage_terrain_modifier)
            .unwrap_or(FORAGE_DEFAULT);
        supply = (supply + forage_base * terrain_mod).min(STARTING_SUPPLY_MONTHS);

        // Disease
        let strength = get_f64_property(ctx.world, army_id, "strength", 0.0) as u32;
        if strength == 0 {
            continue;
        }
        let disease_rate = terrain
            .as_ref()
            .map(disease_rate_for_terrain)
            .unwrap_or(DISEASE_BASE);
        let disease_losses =
            (strength as f64 * disease_rate * ctx.rng.random_range(0.5..1.5)).round() as u32;

        // Starvation
        let starvation_losses = if supply <= 0.0 {
            (strength as f64 * STARVATION_RATE * ctx.rng.random_range(0.7..1.3)).round() as u32
        } else {
            0
        };

        let total_losses = disease_losses + starvation_losses;

        // Morale
        let mut morale = get_f64_property(ctx.world, army_id, "morale", 1.0);
        let home_region = ctx
            .world
            .entities
            .get(&army_id)
            .and_then(|e| e.properties.get("home_region_id"))
            .and_then(|v| v.as_u64());
        if home_region == Some(region_id) {
            morale += HOME_TERRITORY_MORALE_BOOST;
        } else {
            morale -= MORALE_DECAY_PER_MONTH;
        }
        if supply <= 0.0 {
            morale -= STARVATION_MORALE_PENALTY;
        }
        morale = morale.clamp(0.0, 1.0);

        // Increment months_campaigning
        let months = get_f64_property(ctx.world, army_id, "months_campaigning", 0.0) as u32;

        if total_losses > 0 {
            let new_strength = strength.saturating_sub(total_losses);
            let army_name = get_entity_name(ctx.world, army_id);
            let ev = ctx.world.add_event(
                EventKind::Custom("army_attrition".to_string()),
                time,
                format!(
                    "{army_name} lost {total_losses} troops to attrition in year {current_year}"
                ),
            );
            ctx.world
                .add_event_participant(ev, army_id, ParticipantRole::Subject);
            ctx.world.set_property(
                army_id,
                "strength".to_string(),
                serde_json::json!(new_strength),
                ev,
            );
            ctx.world
                .set_property(army_id, "supply".to_string(), serde_json::json!(supply), ev);
            ctx.world
                .set_property(army_id, "morale".to_string(), serde_json::json!(morale), ev);
            ctx.world.set_property(
                army_id,
                "months_campaigning".to_string(),
                serde_json::json!(months + 1),
                ev,
            );

            if new_strength == 0 {
                ctx.world.end_entity(army_id, time, ev);
            }
        } else {
            // No event, but still update supply/morale/months via a dummy mechanism
            // Only update if values actually changed meaningfully
            let old_supply = get_f64_property(ctx.world, army_id, "supply", STARTING_SUPPLY_MONTHS);
            let old_morale = get_f64_property(ctx.world, army_id, "morale", 1.0);
            if (supply - old_supply).abs() > 0.001 || (morale - old_morale).abs() > 0.001 {
                // Create a minimal bookkeeping event
                let ev = ctx.world.add_event(
                    EventKind::Custom("army_status_update".to_string()),
                    time,
                    String::new(),
                );
                ctx.world.set_property(
                    army_id,
                    "supply".to_string(),
                    serde_json::json!(supply),
                    ev,
                );
                ctx.world.set_property(
                    army_id,
                    "morale".to_string(),
                    serde_json::json!(morale),
                    ev,
                );
                ctx.world.set_property(
                    army_id,
                    "months_campaigning".to_string(),
                    serde_json::json!(months + 1),
                    ev,
                );
            }
        }
    }
}

// --- Movement ---
// TODO: Army decision logic (movement targets, retreat decisions, when to engage)
// should eventually be driven by the army's general NPC once that system exists.
// The general's traits and goals would influence targeting priorities, risk
// tolerance, retreat thresholds, and whether to pursue or consolidate.

fn move_armies(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect army info: (army_id, faction_id, current_region)
    struct MoveCandidate {
        army_id: u64,
        faction_id: u64,
        current_region: u64,
    }

    let candidates: Vec<MoveCandidate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e.properties.get("faction_id")?.as_u64()?;
            let current_region = e.relationships.iter().find_map(|r| {
                if r.kind == RelationshipKind::LocatedIn && r.end.is_none() {
                    Some(r.target_entity_id)
                } else {
                    None
                }
            })?;
            Some(MoveCandidate {
                army_id: e.id,
                faction_id,
                current_region,
            })
        })
        .collect();

    // Compute intended moves
    struct IntendedMove {
        army_id: u64,
        from: u64,
        to: u64,
    }

    let mut moves: Vec<IntendedMove> = Vec::new();
    for c in &candidates {
        let enemies = collect_war_enemies(ctx.world, c.faction_id);
        if enemies.is_empty() {
            continue;
        }

        // Priority 1: move toward nearest enemy army
        let enemy_army_region =
            find_nearest_enemy_army_region(ctx.world, c.current_region, &enemies);
        // Priority 2: move toward nearest enemy settlement
        let enemy_settlement_region =
            find_nearest_enemy_region(ctx.world, c.current_region, &enemies);

        // Pick whichever target is closer (army takes priority if equal)
        let target = enemy_army_region.or(enemy_settlement_region);
        let Some(target_region) = target else {
            continue;
        };

        if c.current_region == target_region {
            continue;
        }

        let Some(next_region) = bfs_next_step(ctx.world, c.current_region, target_region) else {
            continue;
        };

        moves.push(IntendedMove {
            army_id: c.army_id,
            from: c.current_region,
            to: next_region,
        });
    }

    // Detect crossings: if army A goes R1→R2 and army B goes R2→R1 and they're at war,
    // cancel both moves (they'll fight in their current regions next tick — or
    // move one of them so they end up co-located)
    let mut cancelled: Vec<usize> = Vec::new();
    for i in 0..moves.len() {
        if cancelled.contains(&i) {
            continue;
        }
        for j in (i + 1)..moves.len() {
            if cancelled.contains(&j) {
                continue;
            }
            // Check if they swap: A.from == B.to && A.to == B.from
            if moves[i].from == moves[j].to && moves[i].to == moves[j].from {
                // Check if they're hostile
                let faction_i = candidates
                    .iter()
                    .find(|c| c.army_id == moves[i].army_id)
                    .map(|c| c.faction_id);
                let faction_j = candidates
                    .iter()
                    .find(|c| c.army_id == moves[j].army_id)
                    .map(|c| c.faction_id);
                if let (Some(fi), Some(fj)) = (faction_i, faction_j)
                    && has_active_rel_of_kind(ctx.world, fi, fj, &RelationshipKind::AtWar)
                {
                    // Cancel the second army's move so they meet at army j's current pos
                    cancelled.push(j);
                }
            }
        }
    }

    // Execute moves
    for (idx, mv) in moves.iter().enumerate() {
        if cancelled.contains(&idx) {
            continue;
        }
        let army_name = get_entity_name(ctx.world, mv.army_id);
        let origin_name = get_entity_name(ctx.world, mv.from);
        let dest_name = get_entity_name(ctx.world, mv.to);
        let ev = ctx.world.add_event(
            EventKind::Custom("army_moved".to_string()),
            time,
            format!("{army_name} marched from {origin_name} to {dest_name} in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, mv.army_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, mv.from, ParticipantRole::Origin);
        ctx.world
            .add_event_participant(ev, mv.to, ParticipantRole::Destination);

        ctx.world
            .end_relationship(mv.army_id, mv.from, &RelationshipKind::LocatedIn, time, ev);
        ctx.world
            .add_relationship(mv.army_id, mv.to, RelationshipKind::LocatedIn, time, ev);
    }
}

// --- Resolve Battles ---

fn resolve_battles(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect all living armies with location
    struct ArmyInfo {
        army_id: u64,
        faction_id: u64,
        region_id: u64,
    }

    let army_infos: Vec<ArmyInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e.properties.get("faction_id")?.as_u64()?;
            let region_id = e.relationships.iter().find_map(|r| {
                if r.kind == RelationshipKind::LocatedIn && r.end.is_none() {
                    Some(r.target_entity_id)
                } else {
                    None
                }
            })?;
            Some(ArmyInfo {
                army_id: e.id,
                faction_id,
                region_id,
            })
        })
        .collect();

    // Find pairs of hostile armies in the same region
    let mut battle_pairs: Vec<(u64, u64, u64, u64, u64)> = Vec::new(); // (army_a, faction_a, army_b, faction_b, region)
    for i in 0..army_infos.len() {
        for j in (i + 1)..army_infos.len() {
            let a = &army_infos[i];
            let b = &army_infos[j];
            if a.region_id != b.region_id {
                continue;
            }
            // Check if factions are at war
            if !has_active_rel_of_kind(
                ctx.world,
                a.faction_id,
                b.faction_id,
                &RelationshipKind::AtWar,
            ) {
                continue;
            }
            battle_pairs.push((
                a.army_id,
                a.faction_id,
                b.army_id,
                b.faction_id,
                a.region_id,
            ));
        }
    }

    for (army_a_id, faction_a, army_b_id, faction_b, region_id) in battle_pairs {
        // Skip if either army already ended this tick
        if ctx
            .world
            .entities
            .get(&army_a_id)
            .is_some_and(|e| e.end.is_some())
        {
            continue;
        }
        if ctx
            .world
            .entities
            .get(&army_b_id)
            .is_some_and(|e| e.end.is_some())
        {
            continue;
        }

        let str_a = get_f64_property(ctx.world, army_a_id, "strength", 0.0) as u32;
        let str_b = get_f64_property(ctx.world, army_b_id, "strength", 0.0) as u32;
        if str_a == 0 || str_b == 0 {
            continue;
        }

        let terrain_bonus = get_terrain_defense_bonus(ctx.world, region_id).unwrap_or(1.0);

        // Determine attacker/defender: army farther from home is attacker
        let home_a = ctx
            .world
            .entities
            .get(&army_a_id)
            .and_then(|e| e.properties.get("home_region_id"))
            .and_then(|v| v.as_u64());
        let home_b = ctx
            .world
            .entities
            .get(&army_b_id)
            .and_then(|e| e.properties.get("home_region_id"))
            .and_then(|v| v.as_u64());
        let a_is_home = home_a == Some(region_id);
        let b_is_home = home_b == Some(region_id);

        let (attacker_army, attacker_faction, defender_army, defender_faction) =
            if a_is_home && !b_is_home {
                (army_b_id, faction_b, army_a_id, faction_a)
            } else {
                (army_a_id, faction_a, army_b_id, faction_b)
            };

        let att_str = get_f64_property(ctx.world, attacker_army, "strength", 0.0) as u32;
        let def_str = get_f64_property(ctx.world, defender_army, "strength", 0.0) as u32;
        let att_morale = get_f64_property(ctx.world, attacker_army, "morale", 1.0);
        let def_morale = get_f64_property(ctx.world, defender_army, "morale", 1.0);

        let attacker_power = att_str as f64 * att_morale;
        let defender_power = def_str as f64 * def_morale * terrain_bonus;

        let (winner_faction, loser_faction, winner_army, loser_army) =
            if attacker_power >= defender_power {
                (
                    attacker_faction,
                    defender_faction,
                    attacker_army,
                    defender_army,
                )
            } else {
                (
                    defender_faction,
                    attacker_faction,
                    defender_army,
                    attacker_army,
                )
            };

        let winner_str = get_f64_property(ctx.world, winner_army, "strength", 0.0) as u32;
        let loser_str = get_f64_property(ctx.world, loser_army, "strength", 0.0) as u32;

        let loser_casualties = (loser_str as f64
            * ctx.rng.random_range(LOSER_CASUALTY_MIN..LOSER_CASUALTY_MAX))
        .round() as u32;
        let winner_casualties = (winner_str as f64
            * ctx
                .rng
                .random_range(WINNER_CASUALTY_MIN..WINNER_CASUALTY_MAX))
        .round() as u32;

        let new_loser_str = loser_str.saturating_sub(loser_casualties);
        let new_winner_str = winner_str.saturating_sub(winner_casualties);

        let winner_name = get_entity_name(ctx.world, winner_faction);
        let loser_name = get_entity_name(ctx.world, loser_faction);
        let battle_ev = ctx.world.add_event(
            EventKind::Battle,
            time,
            format!("Battle between {winner_name} and {loser_name} in year {current_year}"),
        );
        ctx.world
            .add_event_participant(battle_ev, winner_faction, ParticipantRole::Attacker);
        ctx.world
            .add_event_participant(battle_ev, loser_faction, ParticipantRole::Defender);
        ctx.world
            .add_event_participant(battle_ev, region_id, ParticipantRole::Location);

        ctx.world.set_property(
            winner_army,
            "strength".to_string(),
            serde_json::json!(new_winner_str),
            battle_ev,
        );
        ctx.world.set_property(
            loser_army,
            "strength".to_string(),
            serde_json::json!(new_loser_str),
            battle_ev,
        );

        let new_loser_morale =
            (get_f64_property(ctx.world, loser_army, "morale", 1.0) * 0.7).clamp(0.0, 1.0);
        let new_winner_morale =
            (get_f64_property(ctx.world, winner_army, "morale", 1.0) * 1.1).clamp(0.0, 1.0);
        ctx.world.set_property(
            loser_army,
            "morale".to_string(),
            serde_json::json!(new_loser_morale),
            battle_ev,
        );
        ctx.world.set_property(
            winner_army,
            "morale".to_string(),
            serde_json::json!(new_winner_morale),
            battle_ev,
        );

        kill_battle_npcs(ctx, loser_faction, battle_ev, time, current_year, false);
        kill_battle_npcs(ctx, winner_faction, battle_ev, time, current_year, true);

        if new_loser_str == 0 {
            ctx.world.end_entity(loser_army, time, battle_ev);
        }
        if new_winner_str == 0 {
            ctx.world.end_entity(winner_army, time, battle_ev);
        }
    }
}

fn kill_battle_npcs(
    ctx: &mut TickContext,
    faction_id: u64,
    battle_ev: u64,
    time: SimTimestamp,
    current_year: u32,
    is_winner: bool,
) {
    // Collect faction members who are warriors or other roles
    let members: Vec<(u64, String)> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .map(|e| {
            let role = e
                .properties
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("common")
                .to_string();
            (e.id, role)
        })
        .collect();

    let mut to_kill: Vec<u64> = Vec::new();
    for (person_id, role) in &members {
        let base_chance = if role == "warrior" {
            WARRIOR_DEATH_CHANCE
        } else {
            NON_WARRIOR_DEATH_CHANCE
        };
        let chance = if is_winner {
            base_chance * 0.5
        } else {
            base_chance
        };
        if ctx.rng.random_range(0.0..1.0) < chance {
            to_kill.push(*person_id);
        }
    }

    for person_id in to_kill {
        let person_name = get_entity_name(ctx.world, person_id);

        // Check if this person is a leader before ending relationships
        let leader_of_faction: Option<u64> = ctx.world.entities.get(&person_id).and_then(|e| {
            e.relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LeaderOf && r.end.is_none())
                .map(|r| r.target_entity_id)
        });

        let death_ev = ctx.world.add_caused_event(
            EventKind::Death,
            time,
            format!("{person_name} was killed in battle in year {current_year}"),
            battle_ev,
        );
        ctx.world
            .add_event_participant(death_ev, person_id, ParticipantRole::Subject);

        // End all active relationships
        end_person_relationships(ctx.world, person_id, time, death_ev);

        ctx.world.end_entity(person_id, time, death_ev);

        ctx.signals.push(Signal {
            event_id: death_ev,
            kind: SignalKind::EntityDied {
                entity_id: person_id,
            },
        });

        if let Some(fid) = leader_of_faction {
            ctx.signals.push(Signal {
                event_id: death_ev,
                kind: SignalKind::LeaderVacancy {
                    faction_id: fid,
                    previous_leader_id: person_id,
                },
            });
        }
    }
}

// --- Retreats ---

fn check_retreats(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    let armies: Vec<(u64, u64, Option<u64>)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .map(|e| {
            let home = e.properties.get("home_region_id").and_then(|v| v.as_u64());
            let starting = e
                .properties
                .get("starting_strength")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            (e.id, starting as u64, home)
        })
        .collect();

    for (army_id, starting_strength, home_region) in armies {
        let morale = get_f64_property(ctx.world, army_id, "morale", 1.0);
        let strength = get_f64_property(ctx.world, army_id, "strength", 0.0) as u32;
        let starting = starting_strength.max(1) as u32;

        let should_retreat = morale < RETREAT_MORALE_THRESHOLD
            || (strength as f64 / starting as f64) < RETREAT_STRENGTH_RATIO;

        if !should_retreat {
            continue;
        }

        let current_region = match get_army_region(ctx.world, army_id) {
            Some(r) => r,
            None => continue,
        };

        let Some(home) = home_region else {
            continue;
        };

        // Already at home
        if current_region == home {
            continue;
        }

        let next_step = bfs_next_step(ctx.world, current_region, home);
        let Some(next_region) = next_step else {
            continue;
        };

        let army_name = get_entity_name(ctx.world, army_id);
        let ev = ctx.world.add_event(
            EventKind::Custom("army_retreated".to_string()),
            time,
            format!("{army_name} retreated toward home in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, army_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, current_region, ParticipantRole::Origin);
        ctx.world
            .add_event_participant(ev, next_region, ParticipantRole::Destination);

        ctx.world.end_relationship(
            army_id,
            current_region,
            &RelationshipKind::LocatedIn,
            time,
            ev,
        );
        ctx.world
            .add_relationship(army_id, next_region, RelationshipKind::LocatedIn, time, ev);

        // Small morale recovery from retreating
        let new_morale = (morale + 0.05).clamp(0.0, 1.0);
        ctx.world.set_property(
            army_id,
            "morale".to_string(),
            serde_json::json!(new_morale),
            ev,
        );
    }
}

// --- Conquests ---

fn check_conquests(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Find armies in regions containing enemy settlements, with no opposing army present
    struct ConquestCandidate {
        army_id: u64,
        army_faction: u64,
        region_id: u64,
    }

    let candidates: Vec<ConquestCandidate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e.properties.get("faction_id")?.as_u64()?;
            let region_id = e.relationships.iter().find_map(|r| {
                if r.kind == RelationshipKind::LocatedIn && r.end.is_none() {
                    Some(r.target_entity_id)
                } else {
                    None
                }
            })?;
            Some(ConquestCandidate {
                army_id: e.id,
                army_faction: faction_id,
                region_id,
            })
        })
        .collect();

    for candidate in &candidates {
        // Check no opposing army in same region
        let has_opposition = candidates.iter().any(|other| {
            other.army_id != candidate.army_id
                && other.region_id == candidate.region_id
                && has_active_rel_of_kind(
                    ctx.world,
                    candidate.army_faction,
                    other.army_faction,
                    &RelationshipKind::AtWar,
                )
        });
        if has_opposition {
            continue;
        }

        // Find enemy settlements in this region
        let enemy_settlements: Vec<(u64, u64)> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Settlement
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::LocatedIn
                            && r.target_entity_id == candidate.region_id
                            && r.end.is_none()
                    })
            })
            .filter_map(|e| {
                let owner = e.relationships.iter().find_map(|r| {
                    if r.kind == RelationshipKind::MemberOf && r.end.is_none() {
                        Some(r.target_entity_id)
                    } else {
                        None
                    }
                })?;
                // Must belong to an enemy faction
                if owner != candidate.army_faction
                    && has_active_rel_of_kind(
                        ctx.world,
                        candidate.army_faction,
                        owner,
                        &RelationshipKind::AtWar,
                    )
                {
                    Some((e.id, owner))
                } else {
                    None
                }
            })
            .collect();

        for (settlement_id, loser_faction) in enemy_settlements {
            let winner_faction = candidate.army_faction;
            let winner_name = get_entity_name(ctx.world, winner_faction);
            let loser_name = get_entity_name(ctx.world, loser_faction);
            let settlement_name = get_entity_name(ctx.world, settlement_id);

            let siege_ev = ctx.world.add_event(
                EventKind::Siege,
                time,
                format!(
                    "{winner_name} besieged {settlement_name} of {loser_name} in year {current_year}"
                ),
            );
            ctx.world
                .add_event_participant(siege_ev, winner_faction, ParticipantRole::Attacker);
            ctx.world
                .add_event_participant(siege_ev, settlement_id, ParticipantRole::Object);

            let conquest_ev = ctx.world.add_caused_event(
                EventKind::Conquest,
                time,
                format!(
                    "{winner_name} conquered {settlement_name} from {loser_name} in year {current_year}"
                ),
                siege_ev,
            );
            ctx.world
                .add_event_participant(conquest_ev, winner_faction, ParticipantRole::Attacker);
            ctx.world
                .add_event_participant(conquest_ev, loser_faction, ParticipantRole::Defender);
            ctx.world
                .add_event_participant(conquest_ev, settlement_id, ParticipantRole::Object);

            // Transfer settlement
            ctx.world.end_relationship(
                settlement_id,
                loser_faction,
                &RelationshipKind::MemberOf,
                time,
                conquest_ev,
            );
            ctx.world.add_relationship(
                settlement_id,
                winner_faction,
                RelationshipKind::MemberOf,
                time,
                conquest_ev,
            );

            // Transfer NPCs
            let npc_transfers: Vec<u64> = ctx
                .world
                .entities
                .values()
                .filter(|e| {
                    e.kind == EntityKind::Person
                        && e.end.is_none()
                        && e.relationships.iter().any(|r| {
                            r.kind == RelationshipKind::LocatedIn
                                && r.target_entity_id == settlement_id
                                && r.end.is_none()
                        })
                        && e.relationships.iter().any(|r| {
                            r.kind == RelationshipKind::MemberOf
                                && r.target_entity_id == loser_faction
                                && r.end.is_none()
                        })
                })
                .map(|e| e.id)
                .collect();

            for npc_id in npc_transfers {
                ctx.world.end_relationship(
                    npc_id,
                    loser_faction,
                    &RelationshipKind::MemberOf,
                    time,
                    conquest_ev,
                );
                ctx.world.add_relationship(
                    npc_id,
                    winner_faction,
                    RelationshipKind::MemberOf,
                    time,
                    conquest_ev,
                );
            }

            ctx.signals.push(Signal {
                event_id: conquest_ev,
                kind: SignalKind::SettlementCaptured {
                    settlement_id,
                    old_faction_id: loser_faction,
                    new_faction_id: winner_faction,
                },
            });
        }
    }
}

// --- Step 5: War Endings ---

fn check_war_endings(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    let war_pairs = collect_war_pairs(ctx.world);

    for (faction_a, faction_b) in war_pairs {
        let army_a = find_faction_army(ctx.world, faction_a);
        let army_b = find_faction_army(ctx.world, faction_b);

        let mut end_war = false;
        let mut winner_id = faction_a;
        let mut loser_id = faction_b;

        // Army destroyed → surrender
        match (army_a, army_b) {
            (None, Some(_)) => {
                // faction_a has no army, faction_b wins
                winner_id = faction_b;
                loser_id = faction_a;
                end_war = true;
            }
            (Some(_), None) => {
                // faction_b has no army, faction_a wins
                winner_id = faction_a;
                loser_id = faction_b;
                end_war = true;
            }
            (None, None) => {
                // Both armies destroyed - draw, pick faction_a as nominal winner
                end_war = true;
            }
            (Some(army_a_id), Some(army_b_id)) => {
                // Both alive — check exhaustion
                let war_start = get_war_start_year(ctx.world, faction_a).unwrap_or(current_year);
                let war_duration = current_year.saturating_sub(war_start);
                if war_duration >= WAR_EXHAUSTION_START_YEAR {
                    let peace_chance = (PEACE_CHANCE_PER_YEAR
                        * (war_duration - WAR_EXHAUSTION_START_YEAR + 1) as f64)
                        .min(0.8);
                    if ctx.rng.random_range(0.0..1.0) < peace_chance {
                        // Draw — faction with more strength is nominal winner
                        let str_a = get_f64_property(ctx.world, army_a_id, "strength", 0.0);
                        let str_b = get_f64_property(ctx.world, army_b_id, "strength", 0.0);
                        if str_a >= str_b {
                            winner_id = faction_a;
                            loser_id = faction_b;
                        } else {
                            winner_id = faction_b;
                            loser_id = faction_a;
                        }
                        end_war = true;
                    }
                }
            }
        }

        if !end_war {
            continue;
        }

        let winner_name = get_entity_name(ctx.world, winner_id);
        let loser_name = get_entity_name(ctx.world, loser_id);

        // Create Treaty event
        let treaty_ev = ctx.world.add_event(
            EventKind::Treaty,
            time,
            format!(
                "Treaty between {winner_name} and {loser_name} ending war in year {current_year}"
            ),
        );
        ctx.world
            .add_event_participant(treaty_ev, winner_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(treaty_ev, loser_id, ParticipantRole::Object);

        // End bidirectional AtWar relationships
        end_at_war_relationship(ctx.world, faction_a, faction_b, time, treaty_ev);

        // Disband armies and return soldiers to settlements
        for &fid in &[faction_a, faction_b] {
            if let Some(army_id) = find_faction_army(ctx.world, fid) {
                let remaining_str = get_f64_property(ctx.world, army_id, "strength", 0.0) as u32;
                // End army entity
                if ctx
                    .world
                    .entities
                    .get(&army_id)
                    .is_some_and(|e| e.end.is_none())
                {
                    ctx.world.end_entity(army_id, time, treaty_ev);
                }

                // Return soldiers to faction settlements
                if remaining_str > 0 {
                    return_soldiers_to_settlements(ctx.world, fid, remaining_str, treaty_ev);
                }
            }
        }

        ctx.signals.push(Signal {
            event_id: treaty_ev,
            kind: SignalKind::WarEnded {
                winner_id,
                loser_id,
            },
        });
    }
}

fn return_soldiers_to_settlements(
    world: &mut World,
    faction_id: u64,
    total_soldiers: u32,
    event_id: u64,
) {
    let settlement_ids = collect_faction_settlement_ids(world, faction_id);
    if settlement_ids.is_empty() {
        return;
    }

    let per_settlement = total_soldiers / settlement_ids.len() as u32;
    let remainder = total_soldiers % settlement_ids.len() as u32;

    for (i, &sid) in settlement_ids.iter().enumerate() {
        let extra = if (i as u32) < remainder { 1 } else { 0 };
        let soldiers = per_settlement + extra;
        if soldiers == 0 {
            continue;
        }

        if let Some(mut breakdown) = get_population_breakdown(world, sid) {
            // Add returning soldiers to male brackets 2 and 3
            let half = soldiers / 2;
            breakdown.male[2] += half;
            breakdown.male[3] += soldiers - half;
            let new_pop = breakdown.total();
            world.set_property(
                sid,
                "population".to_string(),
                serde_json::json!(new_pop),
                event_id,
            );
            world.set_property(
                sid,
                "population_breakdown".to_string(),
                serde_json::to_value(&breakdown).unwrap(),
                event_id,
            );
        }
    }
}

// --- Helpers ---

fn find_faction_leader_entity(world: &World, faction_id: u64) -> Option<&crate::model::Entity> {
    world.entities.values().find(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LeaderOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
    })
}

fn get_entity_name(world: &World, entity_id: u64) -> String {
    world
        .entities
        .get(&entity_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| format!("entity {entity_id}"))
}

fn get_f64_property(world: &World, entity_id: u64, key: &str, default: f64) -> f64 {
    world
        .entities
        .get(&entity_id)
        .and_then(|e| e.properties.get(key))
        .and_then(|v| v.as_f64())
        .unwrap_or(default)
}

fn has_active_rel_of_kind(world: &World, a: u64, b: u64, kind: &RelationshipKind) -> bool {
    let check = |source: u64, target: u64| -> bool {
        world.entities.get(&source).is_some_and(|e| {
            e.relationships
                .iter()
                .any(|r| r.target_entity_id == target && &r.kind == kind && r.end.is_none())
        })
    };
    check(a, b) || check(b, a)
}

/// Check if two factions have settlements in adjacent (or same) regions.
pub fn factions_are_adjacent(world: &World, a: u64, b: u64) -> bool {
    let regions_a = collect_faction_region_ids(world, a);
    let regions_b = collect_faction_region_ids(world, b);

    // Check if any region in A is adjacent to any region in B (or same region)
    for &ra in &regions_a {
        for &rb in &regions_b {
            if ra == rb {
                return true;
            }
            // Check AdjacentTo relationship
            if let Some(entity) = world.entities.get(&ra)
                && entity.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::AdjacentTo
                        && r.target_entity_id == rb
                        && r.end.is_none()
                })
            {
                return true;
            }
        }
    }
    false
}

fn collect_faction_region_ids(world: &World, faction_id: u64) -> Vec<u64> {
    let mut regions = Vec::new();
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
            && let Some(region_id) = e.relationships.iter().find_map(|r| {
                if r.kind == RelationshipKind::LocatedIn && r.end.is_none() {
                    Some(r.target_entity_id)
                } else {
                    None
                }
            })
            && !regions.contains(&region_id)
        {
            regions.push(region_id);
        }
    }
    regions
}

fn collect_faction_settlement_ids(world: &World, faction_id: u64) -> Vec<u64> {
    world
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
        .collect()
}

fn get_population_breakdown(world: &World, settlement_id: u64) -> Option<PopulationBreakdown> {
    world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.properties.get("population_breakdown"))
        .and_then(|v| serde_json::from_value::<PopulationBreakdown>(v.clone()).ok())
}

fn collect_war_pairs(world: &World) -> Vec<(u64, u64)> {
    let mut pairs: Vec<(u64, u64)> = Vec::new();
    for e in world.entities.values() {
        if e.kind != EntityKind::Faction || e.end.is_some() {
            continue;
        }
        for r in &e.relationships {
            if r.kind == RelationshipKind::AtWar && r.end.is_none() {
                let a = e.id;
                let b = r.target_entity_id;
                // Deduplicate: only keep (smaller, larger)
                let pair = if a < b { (a, b) } else { (b, a) };
                if !pairs.contains(&pair) {
                    pairs.push(pair);
                }
            }
        }
    }
    pairs
}

fn find_faction_army(world: &World, faction_id: u64) -> Option<u64> {
    world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .map(|e| e.id)
}

fn get_army_region(world: &World, army_id: u64) -> Option<u64> {
    world.entities.get(&army_id).and_then(|e| {
        e.relationships.iter().find_map(|r| {
            if r.kind == RelationshipKind::LocatedIn && r.end.is_none() {
                Some(r.target_entity_id)
            } else {
                None
            }
        })
    })
}

fn get_region_terrain(world: &World, region_id: u64) -> Option<Terrain> {
    let terrain_str = world
        .entities
        .get(&region_id)?
        .properties
        .get("terrain")?
        .as_str()?;
    Terrain::try_from(terrain_str.to_string()).ok()
}

fn forage_terrain_modifier(terrain: &Terrain) -> f64 {
    match terrain {
        Terrain::Plains => FORAGE_PLAINS,
        Terrain::Forest => FORAGE_FOREST,
        Terrain::Hills => FORAGE_HILLS,
        Terrain::Mountains => FORAGE_MOUNTAINS,
        Terrain::Desert => FORAGE_DESERT,
        Terrain::Swamp => FORAGE_SWAMP,
        Terrain::Tundra => FORAGE_TUNDRA,
        Terrain::Jungle => FORAGE_JUNGLE,
        Terrain::Coast => FORAGE_COAST,
        _ => FORAGE_DEFAULT,
    }
}

fn disease_rate_for_terrain(terrain: &Terrain) -> f64 {
    match terrain {
        Terrain::Swamp => DISEASE_SWAMP,
        Terrain::Jungle => DISEASE_JUNGLE,
        Terrain::Desert => DISEASE_DESERT,
        Terrain::Tundra => DISEASE_TUNDRA,
        Terrain::Mountains => DISEASE_MOUNTAINS_RATE,
        _ => DISEASE_BASE,
    }
}

fn get_territory_status(world: &World, region_id: u64, army_faction_id: u64) -> TerritoryStatus {
    // Check settlements in this region
    let mut has_friendly = false;
    let mut has_enemy = false;
    for e in world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        let in_region = e.relationships.iter().any(|r| {
            r.kind == RelationshipKind::LocatedIn
                && r.target_entity_id == region_id
                && r.end.is_none()
        });
        if !in_region {
            continue;
        }
        let faction_id = e.relationships.iter().find_map(|r| {
            if r.kind == RelationshipKind::MemberOf && r.end.is_none() {
                Some(r.target_entity_id)
            } else {
                None
            }
        });
        if let Some(fid) = faction_id {
            if fid == army_faction_id {
                has_friendly = true;
            } else {
                has_enemy = true;
            }
        }
    }
    if has_friendly {
        TerritoryStatus::Friendly
    } else if has_enemy {
        TerritoryStatus::Enemy
    } else {
        TerritoryStatus::Neutral
    }
}

fn find_faction_capital(world: &World, faction_id: u64) -> Option<(u64, u64)> {
    let mut best: Option<(u64, u64, u64)> = None; // (settlement_id, region_id, population)
    for e in world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        let belongs = e.relationships.iter().any(|r| {
            r.kind == RelationshipKind::MemberOf
                && r.target_entity_id == faction_id
                && r.end.is_none()
        });
        if !belongs {
            continue;
        }
        let region_id = e.relationships.iter().find_map(|r| {
            if r.kind == RelationshipKind::LocatedIn && r.end.is_none() {
                Some(r.target_entity_id)
            } else {
                None
            }
        });
        let Some(rid) = region_id else { continue };
        let pop = e
            .properties
            .get("population")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if best.is_none() || pop > best.unwrap().2 {
            best = Some((e.id, rid, pop));
        }
    }
    best.map(|(sid, rid, _)| (sid, rid))
}

fn collect_war_enemies(world: &World, faction_id: u64) -> Vec<u64> {
    let mut enemies = Vec::new();
    if let Some(e) = world.entities.get(&faction_id) {
        for r in &e.relationships {
            if r.kind == RelationshipKind::AtWar
                && r.end.is_none()
                && !enemies.contains(&r.target_entity_id)
            {
                enemies.push(r.target_entity_id);
            }
        }
    }
    enemies
}

fn get_adjacent_regions(world: &World, region_id: u64) -> Vec<u64> {
    world
        .entities
        .get(&region_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::AdjacentTo && r.end.is_none())
                .map(|r| r.target_entity_id)
                .collect()
        })
        .unwrap_or_default()
}

/// BFS to find the next step from `start` toward `goal` over region adjacency.
fn bfs_next_step(world: &World, start: u64, goal: u64) -> Option<u64> {
    if start == goal {
        return None;
    }
    let mut visited = vec![start];
    let mut queue: VecDeque<(u64, u64)> = VecDeque::new(); // (current, first_step)
    for adj in get_adjacent_regions(world, start) {
        if adj == goal {
            return Some(adj);
        }
        if !visited.contains(&adj) {
            visited.push(adj);
            queue.push_back((adj, adj));
        }
    }
    while let Some((current, first_step)) = queue.pop_front() {
        for adj in get_adjacent_regions(world, current) {
            if adj == goal {
                return Some(first_step);
            }
            if !visited.contains(&adj) {
                visited.push(adj);
                queue.push_back((adj, first_step));
            }
        }
    }
    None
}

/// BFS from `start` to find the nearest region containing an enemy settlement.
fn find_nearest_enemy_region(world: &World, start: u64, enemies: &[u64]) -> Option<u64> {
    // Check if start already has an enemy settlement
    if region_has_enemy_settlement(world, start, enemies) {
        return Some(start);
    }
    let mut visited = vec![start];
    let mut queue: VecDeque<u64> = VecDeque::new();
    for adj in get_adjacent_regions(world, start) {
        if !visited.contains(&adj) {
            visited.push(adj);
            queue.push_back(adj);
        }
    }
    while let Some(current) = queue.pop_front() {
        if region_has_enemy_settlement(world, current, enemies) {
            return Some(current);
        }
        for adj in get_adjacent_regions(world, current) {
            if !visited.contains(&adj) {
                visited.push(adj);
                queue.push_back(adj);
            }
        }
    }
    None
}

/// BFS from `start` to find the nearest region containing a hostile army.
fn find_nearest_enemy_army_region(world: &World, start: u64, enemies: &[u64]) -> Option<u64> {
    let check = |region_id: u64| -> bool {
        world.entities.values().any(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.properties
                    .get("faction_id")
                    .and_then(|v| v.as_u64())
                    .is_some_and(|fid| enemies.contains(&fid))
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LocatedIn
                        && r.target_entity_id == region_id
                        && r.end.is_none()
                })
        })
    };
    if check(start) {
        return Some(start);
    }
    let mut visited = vec![start];
    let mut queue: VecDeque<u64> = VecDeque::new();
    for adj in get_adjacent_regions(world, start) {
        if !visited.contains(&adj) {
            visited.push(adj);
            queue.push_back(adj);
        }
    }
    while let Some(current) = queue.pop_front() {
        if check(current) {
            return Some(current);
        }
        for adj in get_adjacent_regions(world, current) {
            if !visited.contains(&adj) {
                visited.push(adj);
                queue.push_back(adj);
            }
        }
    }
    None
}

fn region_has_enemy_settlement(world: &World, region_id: u64, enemies: &[u64]) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LocatedIn
                    && r.target_entity_id == region_id
                    && r.end.is_none()
            })
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::MemberOf
                    && r.end.is_none()
                    && enemies.contains(&r.target_entity_id)
            })
    })
}

pub fn get_terrain_defense_bonus(world: &World, region_id: u64) -> Option<f64> {
    let terrain_str = world
        .entities
        .get(&region_id)?
        .properties
        .get("terrain")?
        .as_str()?;
    let terrain = Terrain::try_from(terrain_str.to_string()).ok()?;
    Some(terrain_defense_bonus(&terrain))
}

fn terrain_defense_bonus(terrain: &Terrain) -> f64 {
    match terrain {
        Terrain::Mountains | Terrain::Hills => TERRAIN_BONUS_MOUNTAINS,
        Terrain::Forest | Terrain::Jungle => TERRAIN_BONUS_FOREST,
        _ => 1.0,
    }
}

fn get_war_start_year(world: &World, faction_id: u64) -> Option<u32> {
    world
        .entities
        .get(&faction_id)?
        .properties
        .get("war_start_year")?
        .as_u64()
        .map(|v| v as u32)
}

fn end_ally_relationship(world: &mut World, a: u64, b: u64, time: SimTimestamp, event_id: u64) {
    // End a->b Ally if exists
    let has_a_to_b = world.entities.get(&a).is_some_and(|e| {
        e.relationships
            .iter()
            .any(|r| r.target_entity_id == b && r.kind == RelationshipKind::Ally && r.end.is_none())
    });
    if has_a_to_b {
        world.end_relationship(a, b, &RelationshipKind::Ally, time, event_id);
    }

    // End b->a Ally if exists
    let has_b_to_a = world.entities.get(&b).is_some_and(|e| {
        e.relationships
            .iter()
            .any(|r| r.target_entity_id == a && r.kind == RelationshipKind::Ally && r.end.is_none())
    });
    if has_b_to_a {
        world.end_relationship(b, a, &RelationshipKind::Ally, time, event_id);
    }
}

fn end_at_war_relationship(world: &mut World, a: u64, b: u64, time: SimTimestamp, event_id: u64) {
    let has_a_to_b = world.entities.get(&a).is_some_and(|e| {
        e.relationships.iter().any(|r| {
            r.target_entity_id == b && r.kind == RelationshipKind::AtWar && r.end.is_none()
        })
    });
    if has_a_to_b {
        world.end_relationship(a, b, &RelationshipKind::AtWar, time, event_id);
    }

    let has_b_to_a = world.entities.get(&b).is_some_and(|e| {
        e.relationships.iter().any(|r| {
            r.target_entity_id == a && r.kind == RelationshipKind::AtWar && r.end.is_none()
        })
    });
    if has_b_to_a {
        world.end_relationship(b, a, &RelationshipKind::AtWar, time, event_id);
    }
}

fn end_person_relationships(world: &mut World, person_id: u64, time: SimTimestamp, event_id: u64) {
    let rels: Vec<(u64, RelationshipKind)> = world
        .entities
        .get(&person_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.end.is_none())
                .map(|r| (r.target_entity_id, r.kind.clone()))
                .collect()
        })
        .unwrap_or_default();

    for (target_id, kind) in rels {
        world.end_relationship(person_id, target_id, &kind, time, event_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SimTimestamp, World};

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    #[test]
    fn factions_are_adjacent_works() {
        let mut world = World::new();
        world.current_time = ts(1);

        // Create two regions
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            ts(1),
            "setup".to_string(),
        );
        let region_a = world.add_entity(EntityKind::Region, "Region A".to_string(), None, ev);
        let region_b = world.add_entity(EntityKind::Region, "Region B".to_string(), None, ev);
        let region_c = world.add_entity(EntityKind::Region, "Region C".to_string(), None, ev);

        // Make A adjacent to B
        world.add_relationship(region_a, region_b, RelationshipKind::AdjacentTo, ts(1), ev);

        // Create two factions
        let faction_a = world.add_entity(
            EntityKind::Faction,
            "Faction A".to_string(),
            Some(ts(1)),
            ev,
        );
        let faction_b = world.add_entity(
            EntityKind::Faction,
            "Faction B".to_string(),
            Some(ts(1)),
            ev,
        );
        let faction_c = world.add_entity(
            EntityKind::Faction,
            "Faction C".to_string(),
            Some(ts(1)),
            ev,
        );

        // Create settlements
        let settlement_a = world.add_entity(
            EntityKind::Settlement,
            "Town A".to_string(),
            Some(ts(1)),
            ev,
        );
        world.add_relationship(
            settlement_a,
            faction_a,
            RelationshipKind::MemberOf,
            ts(1),
            ev,
        );
        world.add_relationship(
            settlement_a,
            region_a,
            RelationshipKind::LocatedIn,
            ts(1),
            ev,
        );

        let settlement_b = world.add_entity(
            EntityKind::Settlement,
            "Town B".to_string(),
            Some(ts(1)),
            ev,
        );
        world.add_relationship(
            settlement_b,
            faction_b,
            RelationshipKind::MemberOf,
            ts(1),
            ev,
        );
        world.add_relationship(
            settlement_b,
            region_b,
            RelationshipKind::LocatedIn,
            ts(1),
            ev,
        );

        let settlement_c = world.add_entity(
            EntityKind::Settlement,
            "Town C".to_string(),
            Some(ts(1)),
            ev,
        );
        world.add_relationship(
            settlement_c,
            faction_c,
            RelationshipKind::MemberOf,
            ts(1),
            ev,
        );
        world.add_relationship(
            settlement_c,
            region_c,
            RelationshipKind::LocatedIn,
            ts(1),
            ev,
        );

        // A and B should be adjacent
        assert!(factions_are_adjacent(&world, faction_a, faction_b));
        // A and C should NOT be adjacent (C is in isolated region)
        assert!(!factions_are_adjacent(&world, faction_a, faction_c));
    }

    #[test]
    fn terrain_defense_bonus_values() {
        assert_eq!(
            terrain_defense_bonus(&Terrain::Mountains),
            TERRAIN_BONUS_MOUNTAINS
        );
        assert_eq!(
            terrain_defense_bonus(&Terrain::Hills),
            TERRAIN_BONUS_MOUNTAINS
        );
        assert_eq!(
            terrain_defense_bonus(&Terrain::Forest),
            TERRAIN_BONUS_FOREST
        );
        assert_eq!(
            terrain_defense_bonus(&Terrain::Jungle),
            TERRAIN_BONUS_FOREST
        );
        assert_eq!(terrain_defense_bonus(&Terrain::Plains), 1.0);
        assert_eq!(terrain_defense_bonus(&Terrain::Desert), 1.0);
    }

    #[test]
    fn apply_draft_reduces_population() {
        let mut bd = PopulationBreakdown::from_total(1000);
        let before_m2 = bd.male[2];
        let before_m3 = bd.male[3];
        let before_total = before_m2 + before_m3;

        apply_draft(&mut bd, 50);

        let after_m2 = bd.male[2];
        let after_m3 = bd.male[3];
        let after_total = after_m2 + after_m3;

        assert!(
            after_total < before_total,
            "draft should reduce male brackets 2+3: before={before_total}, after={after_total}"
        );
        assert_eq!(
            before_total - after_total,
            50,
            "should have drafted exactly 50"
        );
    }

    #[test]
    fn find_faction_capital_returns_largest() {
        let mut world = World::new();
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            ts(1),
            "setup".to_string(),
        );
        let region = world.add_entity(EntityKind::Region, "Region".to_string(), None, ev);
        let faction = world.add_entity(EntityKind::Faction, "Faction".to_string(), Some(ts(1)), ev);

        let small = world.add_entity(
            EntityKind::Settlement,
            "Small Town".to_string(),
            Some(ts(1)),
            ev,
        );
        world.add_relationship(small, faction, RelationshipKind::MemberOf, ts(1), ev);
        world.add_relationship(small, region, RelationshipKind::LocatedIn, ts(1), ev);
        world.set_property(small, "population".to_string(), serde_json::json!(100), ev);

        let region2 = world.add_entity(EntityKind::Region, "Region2".to_string(), None, ev);
        let big = world.add_entity(
            EntityKind::Settlement,
            "Big City".to_string(),
            Some(ts(1)),
            ev,
        );
        world.add_relationship(big, faction, RelationshipKind::MemberOf, ts(1), ev);
        world.add_relationship(big, region2, RelationshipKind::LocatedIn, ts(1), ev);
        world.set_property(big, "population".to_string(), serde_json::json!(500), ev);

        let result = find_faction_capital(&world, faction);
        assert_eq!(result, Some((big, region2)));
    }

    #[test]
    fn bfs_next_step_finds_shortest_path() {
        let mut world = World::new();
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            ts(1),
            "setup".to_string(),
        );
        // Create 4 regions in a line: R1 - R2 - R3 - R4
        let r1 = world.add_entity(EntityKind::Region, "R1".to_string(), None, ev);
        let r2 = world.add_entity(EntityKind::Region, "R2".to_string(), None, ev);
        let r3 = world.add_entity(EntityKind::Region, "R3".to_string(), None, ev);
        let r4 = world.add_entity(EntityKind::Region, "R4".to_string(), None, ev);

        world.add_relationship(r1, r2, RelationshipKind::AdjacentTo, ts(1), ev);
        world.add_relationship(r2, r1, RelationshipKind::AdjacentTo, ts(1), ev);
        world.add_relationship(r2, r3, RelationshipKind::AdjacentTo, ts(1), ev);
        world.add_relationship(r3, r2, RelationshipKind::AdjacentTo, ts(1), ev);
        world.add_relationship(r3, r4, RelationshipKind::AdjacentTo, ts(1), ev);
        world.add_relationship(r4, r3, RelationshipKind::AdjacentTo, ts(1), ev);

        // From R1 to R4: next step should be R2
        assert_eq!(bfs_next_step(&world, r1, r4), Some(r2));
        // From R1 to R2: next step should be R2
        assert_eq!(bfs_next_step(&world, r1, r2), Some(r2));
        // Already at goal
        assert_eq!(bfs_next_step(&world, r1, r1), None);
    }

    #[test]
    fn forage_terrain_modifier_values() {
        assert_eq!(forage_terrain_modifier(&Terrain::Plains), FORAGE_PLAINS);
        assert_eq!(forage_terrain_modifier(&Terrain::Forest), FORAGE_FOREST);
        assert_eq!(forage_terrain_modifier(&Terrain::Hills), FORAGE_HILLS);
        assert_eq!(
            forage_terrain_modifier(&Terrain::Mountains),
            FORAGE_MOUNTAINS
        );
        assert_eq!(forage_terrain_modifier(&Terrain::Desert), FORAGE_DESERT);
        assert_eq!(forage_terrain_modifier(&Terrain::Swamp), FORAGE_SWAMP);
        assert_eq!(forage_terrain_modifier(&Terrain::Tundra), FORAGE_TUNDRA);
        assert_eq!(forage_terrain_modifier(&Terrain::Jungle), FORAGE_JUNGLE);
        assert_eq!(forage_terrain_modifier(&Terrain::Coast), FORAGE_COAST);
        assert_eq!(forage_terrain_modifier(&Terrain::Volcanic), FORAGE_DEFAULT);
    }

    #[test]
    fn disease_rate_values() {
        assert_eq!(disease_rate_for_terrain(&Terrain::Swamp), DISEASE_SWAMP);
        assert_eq!(disease_rate_for_terrain(&Terrain::Jungle), DISEASE_JUNGLE);
        assert_eq!(disease_rate_for_terrain(&Terrain::Desert), DISEASE_DESERT);
        assert_eq!(disease_rate_for_terrain(&Terrain::Tundra), DISEASE_TUNDRA);
        assert_eq!(
            disease_rate_for_terrain(&Terrain::Mountains),
            DISEASE_MOUNTAINS_RATE
        );
        assert_eq!(disease_rate_for_terrain(&Terrain::Plains), DISEASE_BASE);
        assert_eq!(disease_rate_for_terrain(&Terrain::Forest), DISEASE_BASE);
    }

    #[test]
    fn territory_status_detection() {
        let mut world = World::new();
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            ts(1),
            "setup".to_string(),
        );
        let region = world.add_entity(EntityKind::Region, "Region".to_string(), None, ev);
        let faction_a = world.add_entity(
            EntityKind::Faction,
            "Faction A".to_string(),
            Some(ts(1)),
            ev,
        );
        let faction_b = world.add_entity(
            EntityKind::Faction,
            "Faction B".to_string(),
            Some(ts(1)),
            ev,
        );
        let empty_region = world.add_entity(EntityKind::Region, "Empty".to_string(), None, ev);

        // Settlement of faction_a in region
        let settlement =
            world.add_entity(EntityKind::Settlement, "Town".to_string(), Some(ts(1)), ev);
        world.add_relationship(settlement, faction_a, RelationshipKind::MemberOf, ts(1), ev);
        world.add_relationship(settlement, region, RelationshipKind::LocatedIn, ts(1), ev);

        // For faction_a's army, this is friendly territory
        assert_eq!(
            get_territory_status(&world, region, faction_a),
            TerritoryStatus::Friendly
        );
        // For faction_b's army, this is enemy territory
        assert_eq!(
            get_territory_status(&world, region, faction_b),
            TerritoryStatus::Enemy
        );
        // Empty region is neutral for everyone
        assert_eq!(
            get_territory_status(&world, empty_region, faction_a),
            TerritoryStatus::Neutral
        );
    }
}
