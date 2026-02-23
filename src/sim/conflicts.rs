use rand::Rng;

use super::context::TickContext;
use super::population::PopulationBreakdown;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
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
const DECISIVE_STRENGTH_RATIO: f64 = 2.0;
const WARRIOR_DEATH_CHANCE: f64 = 0.15;
const NON_WARRIOR_DEATH_CHANCE: f64 = 0.05;

pub struct ConflictSystem;

impl SimSystem for ConflictSystem {
    fn name(&self) -> &str {
        "conflicts"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        check_war_declarations(ctx, time, current_year);
        muster_armies(ctx, time, current_year);
        resolve_battles(ctx, time, current_year);
        check_conquests(ctx, time, current_year);
        check_war_endings(ctx, time, current_year);
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
        let instability_modifier = ((1.0 - pair.avg_stability) * 2.0).clamp(0.5, 2.0);
        let chance = WAR_DECLARATION_BASE_CHANCE * instability_modifier;

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

// --- Step 3: Resolve Battles ---

fn resolve_battles(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Find at-war pairs
    let war_pairs = collect_war_pairs(ctx.world);

    for (faction_a, faction_b) in war_pairs {
        let army_a = find_faction_army(ctx.world, faction_a);
        let army_b = find_faction_army(ctx.world, faction_b);

        let (Some(army_a_id), Some(army_b_id)) = (army_a, army_b) else {
            continue;
        };

        let str_a = get_f64_property(ctx.world, army_a_id, "strength", 0.0) as u32;
        let str_b = get_f64_property(ctx.world, army_b_id, "strength", 0.0) as u32;
        if str_a == 0 || str_b == 0 {
            continue;
        }

        // Find border region for terrain
        let border_region = find_border_region(ctx.world, faction_a, faction_b);
        let terrain_bonus = border_region
            .and_then(|rid| get_terrain_defense_bonus(ctx.world, rid))
            .unwrap_or(1.0);

        let morale_a = get_f64_property(ctx.world, army_a_id, "morale", 1.0);
        let morale_b = get_f64_property(ctx.world, army_b_id, "morale", 1.0);

        // Attacker = faction_a, defender = faction_b (defender gets terrain bonus)
        let attacker_power = str_a as f64 * morale_a;
        let defender_power = str_b as f64 * morale_b * terrain_bonus;

        let (winner_faction, loser_faction, winner_army, loser_army) =
            if attacker_power >= defender_power {
                (faction_a, faction_b, army_a_id, army_b_id)
            } else {
                (faction_b, faction_a, army_b_id, army_a_id)
            };

        let winner_str = get_f64_property(ctx.world, winner_army, "strength", 0.0) as u32;
        let loser_str = get_f64_property(ctx.world, loser_army, "strength", 0.0) as u32;

        // Calculate casualties
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

        // Create Battle event
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
        if let Some(rid) = border_region {
            ctx.world
                .add_event_participant(battle_ev, rid, ParticipantRole::Location);
        }

        // Update army strengths
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

        // Adjust morale
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

        // Kill notable NPCs probabilistically
        kill_battle_npcs(ctx, loser_faction, battle_ev, time, current_year, false);
        kill_battle_npcs(ctx, winner_faction, battle_ev, time, current_year, true);

        // End armies with 0 strength
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

        // Check if this person is a ruler before ending relationships
        let ruler_of_faction: Option<u64> = ctx.world.entities.get(&person_id).and_then(|e| {
            e.relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::RulerOf && r.end.is_none())
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

        if let Some(fid) = ruler_of_faction {
            ctx.signals.push(Signal {
                event_id: death_ev,
                kind: SignalKind::RulerVacancy {
                    faction_id: fid,
                    previous_ruler_id: person_id,
                },
            });
        }
    }
}

// --- Step 4: Conquests ---

fn check_conquests(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    let war_pairs = collect_war_pairs(ctx.world);

    for (faction_a, faction_b) in war_pairs {
        let army_a = find_faction_army(ctx.world, faction_a);
        let army_b = find_faction_army(ctx.world, faction_b);

        let (Some(army_a_id), Some(army_b_id)) = (army_a, army_b) else {
            continue;
        };

        let str_a = get_f64_property(ctx.world, army_a_id, "strength", 0.0);
        let str_b = get_f64_property(ctx.world, army_b_id, "strength", 0.0);

        if str_a <= 0.0 || str_b <= 0.0 {
            continue;
        }

        // Check if one side has decisive advantage
        let (winner_faction, loser_faction) = if str_a > str_b * DECISIVE_STRENGTH_RATIO {
            (faction_a, faction_b)
        } else if str_b > str_a * DECISIVE_STRENGTH_RATIO {
            (faction_b, faction_a)
        } else {
            continue;
        };

        // Find a border settlement of the loser
        let border_settlement = find_border_settlement(ctx.world, loser_faction, winner_faction);
        let Some(settlement_id) = border_settlement else {
            continue;
        };

        let winner_name = get_entity_name(ctx.world, winner_faction);
        let loser_name = get_entity_name(ctx.world, loser_faction);
        let settlement_name = get_entity_name(ctx.world, settlement_id);

        // Siege event
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

        // Conquest event
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

        // Transfer settlement: end MemberOf to old faction, add MemberOf to new faction
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

        // Transfer NPCs in that settlement to new faction
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

fn find_border_region(world: &World, faction_a: u64, faction_b: u64) -> Option<u64> {
    let regions_a = collect_faction_region_ids(world, faction_a);
    let regions_b = collect_faction_region_ids(world, faction_b);

    for &ra in &regions_a {
        for &rb in &regions_b {
            if ra == rb {
                return Some(ra);
            }
            if let Some(entity) = world.entities.get(&ra)
                && entity.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::AdjacentTo
                        && r.target_entity_id == rb
                        && r.end.is_none()
                })
            {
                // Return the defender's region (terrain bonus applies to defender)
                return Some(rb);
            }
        }
    }
    None
}

fn find_border_settlement(world: &World, loser_faction: u64, winner_faction: u64) -> Option<u64> {
    let winner_regions = collect_faction_region_ids(world, winner_faction);

    for e in world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        let belongs_to_loser = e.relationships.iter().any(|r| {
            r.kind == RelationshipKind::MemberOf
                && r.target_entity_id == loser_faction
                && r.end.is_none()
        });
        if !belongs_to_loser {
            continue;
        }

        // Check if this settlement's region is adjacent to winner's territory
        let settlement_region = e.relationships.iter().find_map(|r| {
            if r.kind == RelationshipKind::LocatedIn && r.end.is_none() {
                Some(r.target_entity_id)
            } else {
                None
            }
        });

        if let Some(srid) = settlement_region {
            for &wrid in &winner_regions {
                if srid == wrid {
                    return Some(e.id);
                }
                if let Some(region) = world.entities.get(&srid)
                    && region.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::AdjacentTo
                            && r.target_entity_id == wrid
                            && r.end.is_none()
                    })
                {
                    return Some(e.id);
                }
            }
        }
    }
    None
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
}
