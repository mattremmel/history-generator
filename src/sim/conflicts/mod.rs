mod siege;

use rand::Rng;
use serde::{Deserialize, Serialize};

use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::action::ActionKind;
use crate::model::population::PopulationBreakdown;
use crate::model::traits::{Trait, has_trait};
use crate::model::{
    EntityKind, EventKind, ParticipantRole, RelationshipKind, Role, SiegeOutcome, SimTimestamp,
    WarGoal, World,
};
use crate::sim::grievance as grv;
use crate::sim::helpers;
use crate::worldgen::terrain::Terrain;

// --- War Goals & Peace Terms ---

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeaceTerms {
    decisive: bool,
    territory_ceded: Vec<u64>,
    reparations: f64,
    tribute_per_year: f64,
    tribute_duration_years: u32,
}

// --- Constants ---

const WAR_DECLARATION_BASE_CHANCE: f64 = 0.04;
const RELIGIOUS_WAR_FERVOR_FACTOR: f64 = 0.05;
const RELIGIOUS_WAR_FERVOR_CAP: f64 = 0.10;
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

// Siege supply (used by apply_supply_and_attrition)
const SIEGE_SUPPLY_MULTIPLIER: f64 = 1.2;

// --- Grievance ---
const GRIEVANCE_TREATY_BROKEN: f64 = 0.30;
const GRIEVANCE_TERRITORY_CEDED: f64 = 0.25;

// Succession claim wars
const CLAIM_WAR_INDECISIVE_INSTALL_CHANCE: f64 = 0.5;
const CLAIM_LOSS_STRENGTH_PENALTY: f64 = 0.3;
const CLAIM_WAR_REGIME_STABILITY_HIT: f64 = -0.15;
const CLAIM_WAR_DEFENDER_REPARATIONS_FACTOR: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerritoryStatus {
    Friendly,
    Neutral,
    Enemy,
}

struct EnemyPair {
    a: u64,
    b: u64,
    avg_stability: f64,
    prestige_a: f64,
    prestige_b: f64,
}

struct PeaceOutcome {
    faction_a: u64,
    faction_b: u64,
    winner_id: u64,
    loser_id: u64,
    decisive: bool,
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
        let is_year_start = time.is_year_start();

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
        siege::start_sieges(ctx, time, current_year);
        siege::progress_sieges(ctx, time, current_year);

        // Yearly post-step: war endings (after monthly combat/conquest cycle)
        if is_year_start {
            check_war_endings(ctx, time, current_year);
        }
    }
}

// --- Step 1: War Declarations ---

fn collect_war_candidates(world: &World) -> Vec<EnemyPair> {
    let factions: Vec<(u64, f64, f64)> = world
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
            let fd = e.data.as_faction();
            let stability = fd.map(|f| f.stability).unwrap_or(0.5);
            let prestige = fd.map(|f| f.prestige).unwrap_or(0.0);
            (e.id, stability, prestige)
        })
        .collect();

    let mut enemy_pairs: Vec<EnemyPair> = Vec::new();
    for i in 0..factions.len() {
        for j in (i + 1)..factions.len() {
            let (a, stab_a, pres_a) = factions[i];
            let (b, stab_b, pres_b) = factions[j];

            // Check if they are enemies
            let is_enemy = helpers::has_active_rel_of_kind(world, a, b, RelationshipKind::Enemy);
            if !is_enemy {
                continue;
            }

            // Skip if already at war
            if helpers::has_active_rel_of_kind(world, a, b, RelationshipKind::AtWar) {
                continue;
            }

            // Check adjacency
            if !factions_are_adjacent(world, a, b) {
                continue;
            }

            enemy_pairs.push(EnemyPair {
                a,
                b,
                avg_stability: (stab_a + stab_b) / 2.0,
                prestige_a: pres_a,
                prestige_b: pres_b,
            });
        }
    }

    enemy_pairs
}

fn evaluate_war_chance(pair: &EnemyPair, ctx: &mut TickContext) -> f64 {
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
        return 0.0;
    }

    let instability_modifier = ((1.0 - pair.avg_stability) * 2.0).clamp(0.5, 2.0);
    let mut chance = WAR_DECLARATION_BASE_CHANCE * instability_modifier;

    // Economic war motivation
    for &fid in &[pair.a, pair.b] {
        let econ = ctx
            .world
            .entities
            .get(&fid)
            .and_then(|e| e.data.as_faction())
            .map(|fd| fd.economic_motivation)
            .unwrap_or(0.0);
        chance *= 1.0 + econ;
    }

    // Religious differences as war motivation
    let religion_a = ctx
        .world
        .entities
        .get(&pair.a)
        .and_then(|e| e.data.as_faction())
        .and_then(|fd| fd.primary_religion);
    let religion_b = ctx
        .world
        .entities
        .get(&pair.b)
        .and_then(|e| e.data.as_faction())
        .and_then(|fd| fd.primary_religion);
    if let (Some(ra), Some(rb)) = (religion_a, religion_b)
        && ra != rb
    {
        let fervor_a = ctx
            .world
            .entities
            .get(&ra)
            .and_then(|e| e.data.as_religion())
            .map(|rd| rd.fervor)
            .unwrap_or(0.0);
        let fervor_b = ctx
            .world
            .entities
            .get(&rb)
            .and_then(|e| e.data.as_religion())
            .map(|rd| rd.fervor)
            .unwrap_or(0.0);
        let avg_fervor = (fervor_a + fervor_b) / 2.0;
        let religious_bonus =
            (RELIGIOUS_WAR_FERVOR_FACTOR * avg_fervor).min(RELIGIOUS_WAR_FERVOR_CAP);
        chance += religious_bonus;
    }

    // Grievance factor: high grievances make war more likely
    let grievance_a = grv::get_grievance(ctx.world, pair.a, pair.b);
    let grievance_b = grv::get_grievance(ctx.world, pair.b, pair.a);
    let max_grievance = grievance_a.max(grievance_b);
    chance *= 1.0 + max_grievance; // up to 2x at max grievance

    // Leader traits influence war declaration chance
    for &fid in &[pair.a, pair.b] {
        if let Some(leader) = helpers::faction_leader_entity(ctx.world, fid) {
            if has_trait(leader, &Trait::Aggressive) {
                chance *= 1.5;
            } else if has_trait(leader, &Trait::Cautious) {
                chance *= 0.5;
            }
        }
    }

    // Prestige confidence: faction with more prestige is bolder about war
    let prestige_factor = 1.0 + (pair.prestige_a - pair.prestige_b).abs().min(0.3);
    chance *= prestige_factor;

    chance
}

fn execute_war_declaration(
    ctx: &mut TickContext,
    pair: &EnemyPair,
    time: SimTimestamp,
    current_year: u32,
) {
    // Pick attacker: lower stability is more aggressive
    let stab_a = helpers::faction_stability(ctx.world, pair.a);
    let stab_b = helpers::faction_stability(ctx.world, pair.b);
    let (attacker_id, defender_id) = if stab_a <= stab_b {
        (pair.a, pair.b)
    } else {
        (pair.b, pair.a)
    };

    // --- Treaty-breaking detection ---
    let has_treaty = helpers::has_active_rel_of_kind(
        ctx.world,
        attacker_id,
        defender_id,
        RelationshipKind::Custom("treaty_with".to_string()),
    );
    if has_treaty {
        // End treaty relationships
        end_custom_relationship(ctx.world, attacker_id, defender_id, "treaty_with", time);
        end_custom_relationship(ctx.world, attacker_id, defender_id, "tribute_to", time);

        let attacker_name_tb = helpers::entity_name(ctx.world, attacker_id);
        let defender_name_tb = helpers::entity_name(ctx.world, defender_id);
        let treaty_broken_ev = ctx.world.add_event(
            EventKind::Custom("treaty_broken".to_string()),
            time,
            format!(
                "{attacker_name_tb} broke their treaty with {defender_name_tb} in year {current_year}"
            ),
        );
        ctx.world
            .add_event_participant(treaty_broken_ev, attacker_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(treaty_broken_ev, defender_id, ParticipantRole::Object);

        // Stability penalty for treaty breaker
        helpers::apply_stability_delta(ctx.world, attacker_id, -0.15, treaty_broken_ev);

        // Grievance: defender → attacker for breaking treaty
        grv::add_grievance(
            ctx.world,
            defender_id,
            attacker_id,
            GRIEVANCE_TREATY_BROKEN,
            "treaty_broken",
            time,
            treaty_broken_ev,
        );

        // Diplomatic trust penalty for treaty breaker
        {
            let fd = ctx.world.faction_mut(attacker_id);
            fd.diplomatic_trust = (fd.diplomatic_trust - 0.15).max(0.0);
        }

        // Remove tribute obligations between them
        remove_tribute_obligations(ctx.world, attacker_id, defender_id);
        remove_tribute_obligations(ctx.world, defender_id, attacker_id);

        // Third-party allies of the victim get a chance to become Enemy of the breaker
        let victim_allies: Vec<u64> = ctx
            .world
            .entities
            .get(&defender_id)
            .map(|e| {
                e.active_rels(RelationshipKind::Ally)
                    .filter(|&id| id != attacker_id)
                    .collect()
            })
            .unwrap_or_default();
        for ally_id in victim_allies {
            if ctx.rng.random_range(0.0..1.0) < 0.30 {
                ctx.world.add_relationship(
                    ally_id,
                    attacker_id,
                    RelationshipKind::Enemy,
                    time,
                    treaty_broken_ev,
                );
            }
        }
    }

    // --- Determine war goal ---
    let war_goal = determine_war_goal(ctx, attacker_id, defender_id, time);

    let attacker_name = helpers::entity_name(ctx.world, attacker_id);
    let defender_name = helpers::entity_name(ctx.world, defender_id);

    let goal_desc = match &war_goal {
        WarGoal::Territorial { target_settlements } => {
            format!(
                " seeking territorial expansion ({} settlements targeted)",
                target_settlements.len()
            )
        }
        WarGoal::Economic { reparation_demand } => {
            format!(" demanding economic reparations of {reparation_demand:.0} gold")
        }
        WarGoal::Punitive => " seeking punitive retribution".to_string(),
        WarGoal::SuccessionClaim { claimant_id } => {
            let claimant_name = helpers::entity_name(ctx.world, *claimant_id);
            format!(" pressing succession claim for {claimant_name}")
        }
    };

    let ev = ctx.world.add_event(
        EventKind::WarDeclared,
        time,
        format!(
            "{attacker_name} declared war on {defender_name}{goal_desc} in year {current_year}"
        ),
    );

    // Store war goal data on event
    if let Ok(goal_json) = serde_json::to_value(&war_goal) {
        ctx.world.events.get_mut(&ev).unwrap().data = goal_json;
    }

    ctx.world
        .add_event_participant(ev, attacker_id, ParticipantRole::Attacker);
    ctx.world
        .add_event_participant(ev, defender_id, ParticipantRole::Defender);

    // Store war goal on attacker faction for lookup at peace time
    ctx.world
        .faction_mut(attacker_id)
        .war_goals
        .insert(defender_id, war_goal);

    // Add bidirectional AtWar relationships
    ctx.world
        .add_relationship(attacker_id, defender_id, RelationshipKind::AtWar, time, ev);
    ctx.world
        .add_relationship(defender_id, attacker_id, RelationshipKind::AtWar, time, ev);

    // Set war_started on both factions
    ctx.world.faction_mut(attacker_id).war_started =
        Some(SimTimestamp::from_year(current_year));
    ctx.world.faction_mut(defender_id).war_started =
        Some(SimTimestamp::from_year(current_year));

    // End any active Ally relationship between them
    helpers::end_ally_relationship(ctx.world, attacker_id, defender_id, time, ev);

    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::WarStarted {
            attacker_id,
            defender_id,
        },
    });
}

fn check_war_declarations(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    let enemy_pairs = collect_war_candidates(ctx.world);
    for pair in enemy_pairs {
        let chance = evaluate_war_chance(&pair, ctx);
        if ctx.rng.random_range(0.0..1.0) >= chance {
            continue;
        }
        execute_war_declaration(ctx, &pair, time, current_year);
    }
}

fn determine_war_goal(
    ctx: &mut TickContext,
    attacker_id: u64,
    defender_id: u64,
    time: SimTimestamp,
) -> WarGoal {
    let econ_motivation = ctx
        .world
        .entities
        .get(&attacker_id)
        .and_then(|e| e.data.as_faction())
        .map(|fd| fd.economic_motivation)
        .unwrap_or(0.0);

    // Economic goal: high economic war motivation
    if econ_motivation > 0.3 {
        let defender_treasury = ctx
            .world
            .entities
            .get(&defender_id)
            .and_then(|e| e.data.as_faction())
            .map(|f| f.treasury)
            .unwrap_or(0.0);
        let demand = (defender_treasury * 0.5).max(10.0);
        return WarGoal::Economic {
            reparation_demand: demand,
        };
    }

    // Punitive: high grievance (>0.5) against defender
    let attacker_grievance = grv::get_grievance(ctx.world, attacker_id, defender_id);
    if attacker_grievance > 0.5 {
        return WarGoal::Punitive;
    }

    // Punitive: attacker recently lost a settlement to defender (Conquest events in last ~20 years)
    let recently_lost = ctx.world.events.values().any(|e| {
        e.kind == EventKind::Conquest
            && time.years_since(e.timestamp) <= 20
            && ctx.world.event_participants.iter().any(|p| {
                p.event_id == e.id
                    && p.entity_id == defender_id
                    && p.role == ParticipantRole::Attacker
            })
            && ctx.world.event_participants.iter().any(|p| {
                p.event_id == e.id
                    && p.entity_id == attacker_id
                    && p.role == ParticipantRole::Defender
            })
    });
    if recently_lost {
        return WarGoal::Punitive;
    }

    // Default: Territorial — target defender settlements in regions adjacent to attacker
    let attacker_regions = collect_faction_region_ids(ctx.world, attacker_id);
    let mut target_settlements = Vec::new();
    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Settlement || e.end.is_some() {
            continue;
        }
        if !e.has_active_rel(RelationshipKind::MemberOf, defender_id) {
            continue;
        }
        let settlement_region = e.active_rel(RelationshipKind::LocatedIn);
        if let Some(region) = settlement_region {
            // Check if this region is adjacent to any attacker region
            let adjacent =
                attacker_regions.iter().any(|&ar| {
                    ar == region
                        || ctx.world.entities.get(&ar).is_some_and(|re| {
                            re.has_active_rel(RelationshipKind::AdjacentTo, region)
                        })
                });
            if adjacent {
                target_settlements.push(e.id);
            }
        }
    }
    WarGoal::Territorial { target_settlements }
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
                && e.active_rel(RelationshipKind::AtWar).is_some()
        })
        .map(|e| e.id)
        .collect();

    for faction_id in at_war_factions {
        // Check if faction already has a living army
        let has_army = ctx.world.entities.values().any(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        });
        if has_army {
            continue;
        }

        // Sum able_bodied_men across faction settlements
        let mut total_able = 0u32;
        let settlement_ids: Vec<u64> = helpers::faction_settlements(ctx.world, faction_id);

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
        let faction_name = helpers::entity_name(ctx.world, faction_id);
        let ev = ctx.world.add_event(
            EventKind::Custom("army_mustered".to_string()),
            time,
            format!("{faction_name} mustered an army of {draft_count} in year {current_year}"),
        );

        // Determine home region before creating army
        let home_region = helpers::faction_capital_largest(ctx.world, faction_id);

        use crate::model::entity_data::{ArmyData, EntityData};
        let army_id = ctx.world.add_entity(
            EntityKind::Army,
            format!("Army of {faction_name}"),
            Some(time),
            EntityData::Army(ArmyData {
                strength: draft_count,
                morale: 1.0,
                supply: STARTING_SUPPLY_MONTHS,
                faction_id,
                home_region_id: home_region.map(|(_, r)| r).unwrap_or(0),
                besieging_settlement_id: None,
                months_campaigning: 0,
                starting_strength: draft_count,
            }),
            ev,
        );
        ctx.world
            .add_relationship(army_id, faction_id, RelationshipKind::MemberOf, time, ev);
        ctx.world
            .add_event_participant(ev, army_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, faction_id, ParticipantRole::Object);

        // Set army location to faction's capital region
        if let Some((_settlement_id, region_id)) = home_region {
            ctx.world
                .add_relationship(army_id, region_id, RelationshipKind::LocatedIn, time, ev);
        }

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

        let changes = {
            let Some(entity) = world.entities.get_mut(&sid) else {
                continue;
            };
            let Some(sd) = entity.data.as_settlement_mut() else {
                continue;
            };
            let old_pop = sd.population;
            apply_draft(&mut sd.population_breakdown, draft_from_here);
            sd.population = sd.population_breakdown.total();
            let new_pop = sd.population;
            let new_breakdown = sd.population_breakdown.clone();
            Some((old_pop, new_pop, new_breakdown))
        };
        if let Some((old_pop, new_pop, new_breakdown)) = changes {
            world.record_change(
                sid,
                event_id,
                "population",
                serde_json::json!(old_pop),
                serde_json::json!(new_pop),
            );
            world.record_change(
                sid,
                event_id,
                "population_breakdown",
                serde_json::json!(old_pop),
                serde_json::to_value(&new_breakdown).unwrap(),
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
            let faction_id = e.data.as_army().map(|ad| ad.faction_id).unwrap_or(0);
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

        // Seasonal army modifier from environment system (lower in winter/harsh climates)
        let season_army_mod = find_region_season_army_modifier(ctx.world, region_id);

        // Consume supply (besieging armies consume at higher rate)
        let mut supply = army_supply(ctx.world, army_id);
        let old_supply = supply;
        let is_besieging = ctx
            .world
            .entities
            .get(&army_id)
            .and_then(|e| e.data.as_army())
            .is_some_and(|ad| ad.besieging_settlement_id.is_some());
        let supply_rate = if is_besieging {
            SIEGE_SUPPLY_MULTIPLIER
        } else {
            1.0
        };
        supply -= supply_rate;

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
        // Seasonal modifier affects forage (winter = harder to forage)
        supply = (supply + forage_base * terrain_mod * season_army_mod).min(STARTING_SUPPLY_MONTHS);

        // Disease
        let strength = army_strength(ctx.world, army_id);
        if strength == 0 {
            continue;
        }
        let disease_rate = terrain
            .as_ref()
            .map(disease_rate_for_terrain)
            .unwrap_or(DISEASE_BASE);
        // Harsh seasons increase attrition (invert modifier: low season_army_mod = more losses)
        let season_attrition = if season_army_mod < 1.0 {
            1.0 + (1.0 - season_army_mod) * 0.5
        } else {
            1.0
        };
        let disease_losses =
            (strength as f64 * disease_rate * season_attrition * ctx.rng.random_range(0.5..1.5))
                .round() as u32;

        // Starvation
        let starvation_losses = if supply <= 0.0 {
            (strength as f64 * STARVATION_RATE * ctx.rng.random_range(0.7..1.3)).round() as u32
        } else {
            0
        };

        let total_losses = disease_losses + starvation_losses;

        // Morale
        let mut morale = army_morale(ctx.world, army_id);
        let old_morale_val = morale;
        let home_region = ctx
            .world
            .entities
            .get(&army_id)
            .and_then(|e| e.data.as_army())
            .map(|ad| ad.home_region_id)
            .filter(|&id| id != 0);
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
        let months = ctx
            .world
            .entities
            .get(&army_id)
            .and_then(|e| e.data.as_army())
            .map(|ad| ad.months_campaigning)
            .unwrap_or(0);

        if total_losses > 0 {
            let new_strength = strength.saturating_sub(total_losses);
            let army_name = helpers::entity_name(ctx.world, army_id);
            let ev = ctx.world.add_event(
                EventKind::Custom("army_attrition".to_string()),
                time,
                format!(
                    "{army_name} lost {total_losses} troops to attrition in year {current_year}"
                ),
            );
            ctx.world
                .add_event_participant(ev, army_id, ParticipantRole::Subject);
            {
                let entity = ctx.world.entities.get_mut(&army_id).unwrap();
                let ad = entity.data.as_army_mut().unwrap();
                ad.strength = new_strength;
                ad.supply = supply;
                ad.morale = morale;
            }
            ctx.world.record_change(
                army_id,
                ev,
                "strength",
                serde_json::json!(strength),
                serde_json::json!(new_strength),
            );
            ctx.world.record_change(
                army_id,
                ev,
                "supply",
                serde_json::json!(old_supply),
                serde_json::json!(supply),
            );
            ctx.world.record_change(
                army_id,
                ev,
                "morale",
                serde_json::json!(old_morale_val),
                serde_json::json!(morale),
            );
            ctx.world.army_mut(army_id).months_campaigning = months + 1;

            if new_strength == 0 {
                ctx.world.end_entity(army_id, time, ev);
            }
        } else {
            // No event, but still update supply/morale/months via a dummy mechanism
            // Only update if values actually changed meaningfully
            // old_supply and old_morale_val already captured above
            if (supply - old_supply).abs() > 0.001 || (morale - old_morale_val).abs() > 0.001 {
                // Create a minimal bookkeeping event
                let ev = ctx.world.add_event(
                    EventKind::Custom("army_status_update".to_string()),
                    time,
                    String::new(),
                );
                {
                    let entity = ctx.world.entities.get_mut(&army_id).unwrap();
                    let ad = entity.data.as_army_mut().unwrap();
                    ad.supply = supply;
                    ad.morale = morale;
                }
                ctx.world.record_change(
                    army_id,
                    ev,
                    "supply",
                    serde_json::json!(old_supply),
                    serde_json::json!(supply),
                );
                ctx.world.record_change(
                    army_id,
                    ev,
                    "morale",
                    serde_json::json!(old_morale_val),
                    serde_json::json!(morale),
                );
                ctx.world.army_mut(army_id).months_campaigning = months + 1;
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
        .filter(|e| {
            e.data
                .as_army()
                .is_none_or(|ad| ad.besieging_settlement_id.is_none())
        })
        .filter_map(|e| {
            let ad = e.data.as_army()?;
            let faction_id = ad.faction_id;
            if faction_id == 0 {
                return None;
            }
            let current_region = e.active_rel(RelationshipKind::LocatedIn)?;
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

        let Some(next_region) = helpers::bfs_next_step(ctx.world, c.current_region, target_region)
        else {
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
                    && helpers::has_active_rel_of_kind(ctx.world, fi, fj, RelationshipKind::AtWar)
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
        let army_name = helpers::entity_name(ctx.world, mv.army_id);
        let origin_name = helpers::entity_name(ctx.world, mv.from);
        let dest_name = helpers::entity_name(ctx.world, mv.to);
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
            .end_relationship(mv.army_id, mv.from, RelationshipKind::LocatedIn, time, ev);
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
            let ad = e.data.as_army()?;
            let faction_id = ad.faction_id;
            if faction_id == 0 {
                return None;
            }
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;
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
            if !helpers::has_active_rel_of_kind(
                ctx.world,
                a.faction_id,
                b.faction_id,
                RelationshipKind::AtWar,
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

        let str_a = army_strength(ctx.world, army_a_id);
        let str_b = army_strength(ctx.world, army_b_id);
        if str_a == 0 || str_b == 0 {
            continue;
        }

        let terrain_bonus = get_terrain_defense_bonus(ctx.world, region_id).unwrap_or(1.0);

        // Determine attacker/defender: army farther from home is attacker
        let home_a = ctx
            .world
            .entities
            .get(&army_a_id)
            .and_then(|e| e.data.as_army())
            .map(|ad| ad.home_region_id)
            .filter(|&id| id != 0);
        let home_b = ctx
            .world
            .entities
            .get(&army_b_id)
            .and_then(|e| e.data.as_army())
            .map(|ad| ad.home_region_id)
            .filter(|&id| id != 0);
        let a_is_home = home_a == Some(region_id);
        let b_is_home = home_b == Some(region_id);

        let (attacker_army, attacker_faction, defender_army, defender_faction) =
            if a_is_home && !b_is_home {
                (army_b_id, faction_b, army_a_id, faction_a)
            } else {
                (army_a_id, faction_a, army_b_id, faction_b)
            };

        let att_str = army_strength(ctx.world, attacker_army);
        let def_str = army_strength(ctx.world, defender_army);
        let att_morale = army_morale(ctx.world, attacker_army);
        let def_morale = army_morale(ctx.world, defender_army);

        let att_faction_prestige = get_faction_prestige(ctx.world, attacker_faction);
        let def_faction_prestige = get_faction_prestige(ctx.world, defender_faction);
        let attacker_power = att_str as f64 * att_morale * (1.0 + att_faction_prestige * 0.1);
        let defender_power =
            def_str as f64 * def_morale * terrain_bonus * (1.0 + def_faction_prestige * 0.1);

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

        let winner_str = army_strength(ctx.world, winner_army);
        let loser_str = army_strength(ctx.world, loser_army);

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

        let winner_name = helpers::entity_name(ctx.world, winner_faction);
        let loser_name = helpers::entity_name(ctx.world, loser_faction);
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

        // Update winner army
        let (old_winner_morale, new_winner_morale) = {
            let entity = ctx.world.entities.get_mut(&winner_army).unwrap();
            let ad = entity.data.as_army_mut().unwrap();
            ad.strength = new_winner_str;
            let old_morale = ad.morale;
            ad.morale = (old_morale * 1.1).clamp(0.0, 1.0);
            (old_morale, ad.morale)
        };
        ctx.world.record_change(
            winner_army,
            battle_ev,
            "strength",
            serde_json::json!(winner_str),
            serde_json::json!(new_winner_str),
        );
        ctx.world.record_change(
            winner_army,
            battle_ev,
            "morale",
            serde_json::json!(old_winner_morale),
            serde_json::json!(new_winner_morale),
        );

        // Update loser army
        let (old_loser_morale, new_loser_morale) = {
            let entity = ctx.world.entities.get_mut(&loser_army).unwrap();
            let ad = entity.data.as_army_mut().unwrap();
            ad.strength = new_loser_str;
            let old_morale = ad.morale;
            ad.morale = (old_morale * 0.7).clamp(0.0, 1.0);
            (old_morale, ad.morale)
        };
        ctx.world.record_change(
            loser_army,
            battle_ev,
            "strength",
            serde_json::json!(loser_str),
            serde_json::json!(new_loser_str),
        );
        ctx.world.record_change(
            loser_army,
            battle_ev,
            "morale",
            serde_json::json!(old_loser_morale),
            serde_json::json!(new_loser_morale),
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
    let members: Vec<(u64, Role)> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        })
        .map(|e| {
            let role = e
                .data
                .as_person()
                .map(|p| p.role.clone())
                .unwrap_or(Role::Common);
            (e.id, role)
        })
        .collect();

    let mut to_kill: Vec<u64> = Vec::new();
    for (person_id, role) in &members {
        let base_chance = if *role == Role::Warrior {
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
        let person_name = helpers::entity_name(ctx.world, person_id);

        // Check if this person is a leader before ending relationships
        let leader_of_faction: Option<u64> = ctx
            .world
            .entities
            .get(&person_id)
            .and_then(|e| e.active_rel(RelationshipKind::LeaderOf));

        let death_ev = ctx.world.add_caused_event(
            EventKind::Death,
            time,
            format!("{person_name} was killed in battle in year {current_year}"),
            battle_ev,
        );
        ctx.world
            .add_event_participant(death_ev, person_id, ParticipantRole::Subject);

        // End all active relationships
        helpers::end_all_person_relationships(ctx.world, person_id, time, death_ev);

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
            let ad = e.data.as_army();
            let home = ad.map(|a| a.home_region_id).filter(|&id| id != 0);
            let starting = ad.map(|a| a.starting_strength).unwrap_or(1);
            (e.id, starting as u64, home)
        })
        .collect();

    for (army_id, starting_strength, home_region) in armies {
        let morale = army_morale(ctx.world, army_id);
        let strength = army_strength(ctx.world, army_id);
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

        let next_step = helpers::bfs_next_step(ctx.world, current_region, home);
        let Some(next_region) = next_step else {
            continue;
        };

        // Clear any siege this army was conducting
        let besieging = ctx
            .world
            .entities
            .get(&army_id)
            .and_then(|e| e.data.as_army())
            .and_then(|ad| ad.besieging_settlement_id);
        if let Some(siege_settlement_id) = besieging {
            let defender_faction = ctx
                .world
                .entities
                .get(&siege_settlement_id)
                .and_then(|e| e.active_rel(RelationshipKind::MemberOf))
                .unwrap_or(0);
            let attacker_faction = ctx
                .world
                .entities
                .get(&army_id)
                .and_then(|e| e.data.as_army())
                .map(|ad| ad.faction_id)
                .unwrap_or(0);
            siege::clear_siege(
                ctx,
                siege::SiegeClearParams {
                    settlement_id: siege_settlement_id,
                    army_id,
                    attacker_faction_id: attacker_faction,
                    defender_faction_id: defender_faction,
                    outcome: SiegeOutcome::Abandoned,
                },
                time,
                current_year,
            );
        }

        let army_name = helpers::entity_name(ctx.world, army_id);
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
            RelationshipKind::LocatedIn,
            time,
            ev,
        );
        ctx.world
            .add_relationship(army_id, next_region, RelationshipKind::LocatedIn, time, ev);

        // Small morale recovery from retreating
        let new_morale = (morale + 0.05).clamp(0.0, 1.0);
        {
            let entity = ctx.world.entities.get_mut(&army_id).unwrap();
            let ad = entity.data.as_army_mut().unwrap();
            ad.morale = new_morale;
        }
        ctx.world.record_change(
            army_id,
            ev,
            "morale",
            serde_json::json!(morale),
            serde_json::json!(new_morale),
        );
    }
}

// --- Step 5: War Endings ---

fn determine_peace_terms(
    world: &World,
    winner_id: u64,
    loser_id: u64,
    decisive: bool,
    war_goal: &WarGoal,
    rng: &mut dyn rand::RngCore,
) -> PeaceTerms {
    let loser_settlement_count = helpers::faction_settlements(world, loser_id).len() as f64;
    let estimated_income = loser_settlement_count * 5.0;

    // Prestigious winners extract harsher terms
    let winner_prestige = get_faction_prestige(world, winner_id);
    let prestige_bonus = if winner_prestige > 0.5 {
        (winner_prestige - 0.5) * 2.0 // 0.0-1.0 scale above threshold
    } else {
        0.0
    };

    // Grievance makes peace terms harsher: +50% reparations, +1 tribute year
    let winner_grievance = grv::get_grievance(world, winner_id, loser_id);
    let grievance_reparation_mult = if winner_grievance > 0.4 { 1.5 } else { 1.0 };
    let grievance_tribute_bonus: u32 = if winner_grievance > 0.4 { 1 } else { 0 };

    match (decisive, war_goal) {
        (true, WarGoal::Territorial { target_settlements }) => PeaceTerms {
            decisive: true,
            territory_ceded: target_settlements.clone(),
            reparations: 0.0,
            tribute_per_year: 0.0,
            tribute_duration_years: 0,
        },
        (true, WarGoal::Economic { reparation_demand }) => {
            let tribute_years = rng.random_range(5..=10)
                + (prestige_bonus * 2.0).round() as u32
                + grievance_tribute_bonus;
            PeaceTerms {
                decisive: true,
                territory_ceded: Vec::new(),
                reparations: *reparation_demand
                    * (1.0 + prestige_bonus * 0.2)
                    * grievance_reparation_mult,
                tribute_per_year: estimated_income * 0.15 * (1.0 + prestige_bonus * 0.1),
                tribute_duration_years: tribute_years,
            }
        }
        (true, WarGoal::Punitive) => PeaceTerms {
            decisive: true,
            territory_ceded: Vec::new(),
            reparations: estimated_income
                * 2.0
                * (1.0 + prestige_bonus * 0.2)
                * grievance_reparation_mult,
            tribute_per_year: 0.0,
            tribute_duration_years: 0,
        },
        (false, WarGoal::Territorial { .. }) => {
            // Status quo — settlements conquered during war stay
            PeaceTerms {
                decisive: false,
                territory_ceded: Vec::new(),
                reparations: 0.0,
                tribute_per_year: 0.0,
                tribute_duration_years: 0,
            }
        }
        (false, WarGoal::Economic { reparation_demand }) => {
            let tribute_years = rng.random_range(3..=5)
                + (prestige_bonus * 2.0).round() as u32
                + grievance_tribute_bonus;
            PeaceTerms {
                decisive: false,
                territory_ceded: Vec::new(),
                reparations: reparation_demand
                    * 0.5
                    * (1.0 + prestige_bonus * 0.2)
                    * grievance_reparation_mult,
                tribute_per_year: estimated_income * 0.10 * (1.0 + prestige_bonus * 0.1),
                tribute_duration_years: tribute_years,
            }
        }
        (false, WarGoal::Punitive) => PeaceTerms {
            decisive: false,
            territory_ceded: Vec::new(),
            reparations: 0.0,
            tribute_per_year: 0.0,
            tribute_duration_years: 0,
        },
        // Succession claim: the prize is the throne, not territory/reparations
        (true, WarGoal::SuccessionClaim { .. }) => PeaceTerms {
            decisive: true,
            territory_ceded: Vec::new(),
            reparations: 0.0,
            tribute_per_year: 0.0,
            tribute_duration_years: 0,
        },
        (false, WarGoal::SuccessionClaim { .. }) => {
            // Non-decisive: small reparations from the losing side
            PeaceTerms {
                decisive: false,
                territory_ceded: Vec::new(),
                reparations: loser_settlement_count * CLAIM_WAR_DEFENDER_REPARATIONS_FACTOR,
                tribute_per_year: 0.0,
                tribute_duration_years: 0,
            }
        }
    }
}

fn evaluate_peace_conditions(
    ctx: &mut TickContext,
    faction_a: u64,
    faction_b: u64,
    current_year: u32,
) -> Option<PeaceOutcome> {
    let army_a = find_faction_army(ctx.world, faction_a);
    let army_b = find_faction_army(ctx.world, faction_b);

    // Army destroyed → surrender (decisive)
    let (winner_id, loser_id, decisive) = match (army_a, army_b) {
        (None, Some(_)) => (faction_b, faction_a, true),
        (Some(_), None) => (faction_a, faction_b, true),
        // Both armies destroyed - draw (not decisive)
        (None, None) => (faction_a, faction_b, false),
        // Both alive — check exhaustion (not decisive)
        (Some(army_a_id), Some(army_b_id)) => {
            let war_start = get_war_start_year(ctx.world, faction_a).unwrap_or(current_year);
            let war_duration = current_year.saturating_sub(war_start);
            if war_duration < WAR_EXHAUSTION_START_YEAR {
                return None;
            }
            let peace_chance = (PEACE_CHANCE_PER_YEAR
                * (war_duration - WAR_EXHAUSTION_START_YEAR + 1) as f64)
                .min(0.8);
            if ctx.rng.random_range(0.0..1.0) >= peace_chance {
                return None;
            }
            let str_a = army_strength(ctx.world, army_a_id) as f64;
            let str_b = army_strength(ctx.world, army_b_id) as f64;
            if str_a >= str_b {
                (faction_a, faction_b, false)
            } else {
                (faction_b, faction_a, false)
            }
        }
    };

    Some(PeaceOutcome {
        faction_a,
        faction_b,
        winner_id,
        loser_id,
        decisive,
    })
}

fn execute_peace_terms(
    ctx: &mut TickContext,
    outcome: &PeaceOutcome,
    time: SimTimestamp,
    current_year: u32,
) {
    let winner_id = outcome.winner_id;
    let loser_id = outcome.loser_id;
    let decisive = outcome.decisive;

    // Look up war goal — check winner's extra first, then loser's (original attacker may be loser)
    let war_goal: WarGoal = ctx
        .world
        .faction(winner_id)
        .war_goals
        .get(&loser_id)
        .cloned()
        .or_else(|| {
            ctx.world
                .faction(loser_id)
                .war_goals
                .get(&winner_id)
                .cloned()
        })
        .unwrap_or(WarGoal::Territorial {
            target_settlements: Vec::new(),
        });

    let terms = determine_peace_terms(ctx.world, winner_id, loser_id, decisive, &war_goal, ctx.rng);

    let winner_name = helpers::entity_name(ctx.world, winner_id);
    let loser_name = helpers::entity_name(ctx.world, loser_id);

    // Build treaty description
    let mut terms_desc = Vec::new();
    if decisive {
        terms_desc.push("decisive victory".to_string());
    } else {
        terms_desc.push("exhaustion peace".to_string());
    }
    if !terms.territory_ceded.is_empty() {
        terms_desc.push(format!("{} settlements ceded", terms.territory_ceded.len()));
    }
    if terms.reparations > 0.0 {
        terms_desc.push(format!("{:.0} gold reparations", terms.reparations));
    }
    if terms.tribute_duration_years > 0 {
        terms_desc.push(format!(
            "{:.0} gold/year tribute for {} years",
            terms.tribute_per_year, terms.tribute_duration_years
        ));
    }
    let terms_text = terms_desc.join(", ");

    // Create Treaty event
    let treaty_ev = ctx.world.add_event(
        EventKind::Treaty,
        time,
        format!(
            "Treaty between {winner_name} and {loser_name} in year {current_year}: {terms_text}"
        ),
    );

    // Store peace terms as event data
    if let Ok(terms_json) = serde_json::to_value(&terms) {
        ctx.world.events.get_mut(&treaty_ev).unwrap().data = terms_json;
    }

    ctx.world
        .add_event_participant(treaty_ev, winner_id, ParticipantRole::Subject);
    ctx.world
        .add_event_participant(treaty_ev, loser_id, ParticipantRole::Object);

    // End bidirectional AtWar relationships
    end_at_war_relationship(
        ctx.world,
        outcome.faction_a,
        outcome.faction_b,
        time,
        treaty_ev,
    );

    // --- Apply peace terms ---

    // 1. Cede territory: transfer settlements not already conquered
    for &settlement_id in &terms.territory_ceded {
        let current_owner = ctx
            .world
            .entities
            .get(&settlement_id)
            .and_then(|e| e.active_rel(RelationshipKind::MemberOf));
        if current_owner == Some(loser_id) {
            // Transfer settlement
            ctx.world.end_relationship(
                settlement_id,
                loser_id,
                RelationshipKind::MemberOf,
                time,
                treaty_ev,
            );
            ctx.world.add_relationship(
                settlement_id,
                winner_id,
                RelationshipKind::MemberOf,
                time,
                treaty_ev,
            );

            // Transfer NPCs
            helpers::transfer_settlement_npcs(
                ctx.world,
                settlement_id,
                loser_id,
                winner_id,
                time,
                treaty_ev,
            );

            ctx.signals.push(Signal {
                event_id: treaty_ev,
                kind: SignalKind::SettlementCaptured {
                    settlement_id,
                    old_faction_id: loser_id,
                    new_faction_id: winner_id,
                },
            });

            // Grievance: loser → winner for territory ceded in peace
            grv::add_grievance(
                ctx.world,
                loser_id,
                winner_id,
                GRIEVANCE_TERRITORY_CEDED,
                "territory_ceded",
                time,
                treaty_ev,
            );
        }
    }

    // 2. Reparations: transfer from loser treasury to winner
    if terms.reparations > 0.0 {
        let loser_treasury = ctx
            .world
            .entities
            .get(&loser_id)
            .and_then(|e| e.data.as_faction())
            .map(|f| f.treasury)
            .unwrap_or(0.0);
        let transfer = terms.reparations.min(loser_treasury);
        if transfer > 0.0 {
            {
                let entity = ctx.world.entities.get_mut(&loser_id).unwrap();
                let fd = entity.data.as_faction_mut().unwrap();
                fd.treasury -= transfer;
            }
            {
                let entity = ctx.world.entities.get_mut(&winner_id).unwrap();
                let fd = entity.data.as_faction_mut().unwrap();
                fd.treasury += transfer;
            }
            ctx.world.record_change(
                loser_id,
                treaty_ev,
                "treasury",
                serde_json::json!(loser_treasury),
                serde_json::json!(loser_treasury - transfer),
            );
        }
    }

    // 3. Tribute setup
    if terms.tribute_duration_years > 0 && terms.tribute_per_year > 0.0 {
        ctx.world.faction_mut(loser_id).tributes.insert(
            winner_id,
            crate::model::TributeObligation {
                amount: terms.tribute_per_year,
                years_remaining: terms.tribute_duration_years,
                treaty_event_id: treaty_ev,
            },
        );
        ctx.world.add_relationship(
            loser_id,
            winner_id,
            RelationshipKind::Custom("tribute_to".to_string()),
            time,
            treaty_ev,
        );
    }

    // 4. Treaty tracking: bidirectional treaty_with relationships
    ctx.world.add_relationship(
        winner_id,
        loser_id,
        RelationshipKind::Custom("treaty_with".to_string()),
        time,
        treaty_ev,
    );
    ctx.world.add_relationship(
        loser_id,
        winner_id,
        RelationshipKind::Custom("treaty_with".to_string()),
        time,
        treaty_ev,
    );


    // Clean up war goals
    ctx.world
        .faction_mut(winner_id)
        .war_goals
        .remove(&loser_id);
    ctx.world
        .faction_mut(loser_id)
        .war_goals
        .remove(&winner_id);

    // --- Succession Claim resolution ---
    if let WarGoal::SuccessionClaim { claimant_id } = &war_goal {
        let claimant_id = *claimant_id;
        // Determine the target faction (the one whose throne is being claimed).
        // The claimant's faction attacked the target, so the target is the other faction.
        let claimant_faction = ctx.world.entities.get(&claimant_id).and_then(|e| {
            e.active_rels(RelationshipKind::MemberOf).find(|&target| {
                ctx.world
                    .entities
                    .get(&target)
                    .is_some_and(|t| t.kind == EntityKind::Faction)
            })
        });
        let target_faction_id = if claimant_faction == Some(outcome.faction_a) {
            outcome.faction_b
        } else {
            outcome.faction_a
        };
        let attacker_won = winner_id != target_faction_id;

        if attacker_won {
            let should_install = if decisive {
                true
            } else {
                // Non-decisive: 50% chance claimant installs
                ctx.rng.random_range(0.0..1.0) < CLAIM_WAR_INDECISIVE_INSTALL_CHANCE
            };

            if should_install
                && ctx
                    .world
                    .entities
                    .get(&claimant_id)
                    .is_some_and(|e| e.end.is_none())
            {
                // Read claim strength before mutation
                let claim_strength = ctx
                    .world
                    .entities
                    .get(&claimant_id)
                    .and_then(|e| e.data.as_person())
                    .and_then(|pd| pd.claims.get(&target_faction_id))
                    .map(|c| c.strength)
                    .unwrap_or(0.5);

                // End claimant's LeaderOf on their current faction
                if let Some(old_faction) = ctx
                    .world
                    .entities
                    .get(&claimant_id)
                    .and_then(|e| e.active_rel(RelationshipKind::LeaderOf))
                {
                    ctx.world.end_relationship(
                        claimant_id,
                        old_faction,
                        RelationshipKind::LeaderOf,
                        time,
                        treaty_ev,
                    );
                }

                // End claimant's MemberOf on their current faction
                if let Some(old_faction) = claimant_faction {
                    ctx.world.end_relationship(
                        claimant_id,
                        old_faction,
                        RelationshipKind::MemberOf,
                        time,
                        treaty_ev,
                    );
                }

                // End current target leader's LeaderOf
                if let Some(current_leader) = helpers::faction_leader(ctx.world, target_faction_id)
                {
                    ctx.world.end_relationship(
                        current_leader,
                        target_faction_id,
                        RelationshipKind::LeaderOf,
                        time,
                        treaty_ev,
                    );
                }

                // Add claimant MemberOf + LeaderOf on target faction
                ctx.world.add_relationship(
                    claimant_id,
                    target_faction_id,
                    RelationshipKind::MemberOf,
                    time,
                    treaty_ev,
                );
                ctx.world.add_relationship(
                    claimant_id,
                    target_faction_id,
                    RelationshipKind::LeaderOf,
                    time,
                    treaty_ev,
                );

                // Create succession event
                let claimant_name = helpers::entity_name(ctx.world, claimant_id);
                let target_name = helpers::entity_name(ctx.world, target_faction_id);
                let succ_ev = ctx.world.add_caused_event(
                    EventKind::Succession,
                    time,
                    format!(
                        "{claimant_name} claimed the throne of {target_name} in year {current_year}"
                    ),
                    treaty_ev,
                );
                ctx.world
                    .add_event_participant(succ_ev, claimant_id, ParticipantRole::Subject);
                ctx.world.add_event_participant(
                    succ_ev,
                    target_faction_id,
                    ParticipantRole::Object,
                );

                // Remove claim from claimant
                ctx.world
                    .person_mut(claimant_id)
                    .claims
                    .remove(&target_faction_id);

                // Stability hit on target faction (regime change)
                helpers::apply_stability_delta(
                    ctx.world,
                    target_faction_id,
                    CLAIM_WAR_REGIME_STABILITY_HIT,
                    treaty_ev,
                );

                // Set legitimacy based on claim strength
                if let Some(fd) = ctx
                    .world
                    .entities
                    .get_mut(&target_faction_id)
                    .and_then(|e| e.data.as_faction_mut())
                {
                    fd.legitimacy = (claim_strength * 0.8).clamp(0.2, 0.9);
                }
            } else {
                // Attacker won but claimant not installed (dead or indecisive roll)
                reduce_claim_strength(
                    ctx.world,
                    claimant_id,
                    target_faction_id,
                    CLAIM_LOSS_STRENGTH_PENALTY,
                );
            }
        } else {
            // Defender won: reduce claim strength
            reduce_claim_strength(
                ctx.world,
                claimant_id,
                target_faction_id,
                CLAIM_LOSS_STRENGTH_PENALTY,
            );
        }
    }

    // Disband armies and return soldiers to settlements
    for &fid in &[outcome.faction_a, outcome.faction_b] {
        if let Some(army_id) = find_faction_army(ctx.world, fid) {
            let remaining_str = army_strength(ctx.world, army_id);
            if ctx
                .world
                .entities
                .get(&army_id)
                .is_some_and(|e| e.end.is_none())
            {
                ctx.world.end_entity(army_id, time, treaty_ev);
            }
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
            decisive,
            reparations: terms.reparations,
            tribute_years: terms.tribute_duration_years,
        },
    });
}

fn check_war_endings(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    let war_pairs = collect_war_pairs(ctx.world);
    for (faction_a, faction_b) in war_pairs {
        if let Some(outcome) = evaluate_peace_conditions(ctx, faction_a, faction_b, current_year) {
            execute_peace_terms(ctx, &outcome, time, current_year);
        }
    }
}

fn return_soldiers_to_settlements(
    world: &mut World,
    faction_id: u64,
    total_soldiers: u32,
    event_id: u64,
) {
    let settlement_ids = helpers::faction_settlements(world, faction_id);
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

        let changes = {
            let Some(entity) = world.entities.get_mut(&sid) else {
                continue;
            };
            let Some(sd) = entity.data.as_settlement_mut() else {
                continue;
            };
            let old_pop = sd.population;
            // Add returning soldiers to male brackets 2 and 3
            let half = soldiers / 2;
            sd.population_breakdown.male[2] += half;
            sd.population_breakdown.male[3] += soldiers - half;
            sd.population = sd.population_breakdown.total();
            let new_pop = sd.population;
            let new_breakdown = sd.population_breakdown.clone();
            Some((old_pop, new_pop, new_breakdown))
        };
        if let Some((old_pop, new_pop, new_breakdown)) = changes {
            world.record_change(
                sid,
                event_id,
                "population",
                serde_json::json!(old_pop),
                serde_json::json!(new_pop),
            );
            world.record_change(
                sid,
                event_id,
                "population_breakdown",
                serde_json::json!(old_pop),
                serde_json::to_value(&new_breakdown).unwrap(),
            );
        }
    }
}

// --- Helpers ---

pub(crate) fn army_strength(world: &World, army_id: u64) -> u32 {
    world.army(army_id).strength
}

pub(crate) fn army_morale(world: &World, army_id: u64) -> f64 {
    world.army(army_id).morale
}

pub(crate) fn army_supply(world: &World, army_id: u64) -> f64 {
    world.army(army_id).supply
}

/// Reduce a person's claim strength by the given penalty, removing if below threshold.
fn reduce_claim_strength(world: &mut World, person_id: u64, faction_id: u64, penalty: f64) {
    let Some(entity) = world.entities.get(&person_id) else {
        return;
    };
    let Some(claim) = entity.data.as_person().and_then(|pd| pd.claims.get(&faction_id)) else {
        return;
    };
    let new_strength = claim.strength - penalty;
    if new_strength < 0.1 {
        world.person_mut(person_id).claims.remove(&faction_id);
    } else {
        world.person_mut(person_id).claims.get_mut(&faction_id).unwrap().strength = new_strength;
    }
}

fn get_faction_prestige(world: &World, faction_id: u64) -> f64 {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.prestige)
        .unwrap_or(0.0)
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
            if world
                .entities
                .get(&ra)
                .is_some_and(|entity| entity.has_active_rel(RelationshipKind::AdjacentTo, rb))
            {
                return true;
            }
        }
    }
    false
}

fn collect_faction_region_ids(world: &World, faction_id: u64) -> Vec<u64> {
    let mut seen = std::collections::BTreeSet::new();
    for e in world.entities.values() {
        if e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
            && let Some(region_id) = e.active_rel(RelationshipKind::LocatedIn)
        {
            seen.insert(region_id);
        }
    }
    seen.into_iter().collect()
}

fn get_population_breakdown(world: &World, settlement_id: u64) -> Option<PopulationBreakdown> {
    world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.data.as_settlement())
        .map(|s| s.population_breakdown.clone())
}

fn collect_war_pairs(world: &World) -> Vec<(u64, u64)> {
    let mut seen = std::collections::BTreeSet::new();
    for e in world.entities.values() {
        if e.kind != EntityKind::Faction || e.end.is_some() {
            continue;
        }
        for b in e.active_rels(RelationshipKind::AtWar) {
            let a = e.id;
            let pair = if a < b { (a, b) } else { (b, a) };
            seen.insert(pair);
        }
    }
    seen.into_iter().collect()
}

fn find_faction_army(world: &World, faction_id: u64) -> Option<u64> {
    world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        })
        .map(|e| e.id)
}

pub(crate) fn get_army_region(world: &World, army_id: u64) -> Option<u64> {
    world
        .entities
        .get(&army_id)
        .and_then(|e| e.active_rel(RelationshipKind::LocatedIn))
}

fn get_region_terrain(world: &World, region_id: u64) -> Option<Terrain> {
    Some(world.entities.get(&region_id)?.data.as_region()?.terrain)
}

/// Look up the `season_army_modifier` from any settlement in this region.
/// Falls back to 1.0 if no settlement is found or the extra is not set.
fn find_region_season_army_modifier(world: &World, region_id: u64) -> f64 {
    world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::LocatedIn, region_id)
        })
        .map(|e| e.data.as_settlement().map_or(1.0, |sd| sd.seasonal.army))
        .unwrap_or(1.0)
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
        if !e.has_active_rel(RelationshipKind::LocatedIn, region_id) {
            continue;
        }
        if let Some(fid) = e.active_rel(RelationshipKind::MemberOf) {
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

fn collect_war_enemies(world: &World, faction_id: u64) -> Vec<u64> {
    let mut enemies = Vec::new();
    if let Some(e) = world.entities.get(&faction_id) {
        for target in e.active_rels(RelationshipKind::AtWar) {
            if !enemies.contains(&target) {
                enemies.push(target);
            }
        }
    }
    enemies
}

/// BFS from `start` to find the nearest region containing an enemy settlement.
fn find_nearest_enemy_region(world: &World, start: u64, enemies: &[u64]) -> Option<u64> {
    helpers::bfs_nearest(world, start, |r| {
        region_has_enemy_settlement(world, r, enemies)
    })
}

/// BFS from `start` to find the nearest region containing a hostile army.
fn find_nearest_enemy_army_region(world: &World, start: u64, enemies: &[u64]) -> Option<u64> {
    helpers::bfs_nearest(world, start, |region_id| {
        world.entities.values().any(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.data
                    .as_army()
                    .is_some_and(|ad| enemies.contains(&ad.faction_id))
                && e.has_active_rel(RelationshipKind::LocatedIn, region_id)
        })
    })
}

fn region_has_enemy_settlement(world: &World, region_id: u64, enemies: &[u64]) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Settlement
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::LocatedIn, region_id)
            && e.active_rel(RelationshipKind::MemberOf)
                .is_some_and(|owner| enemies.contains(&owner))
    })
}

pub fn get_terrain_defense_bonus(world: &World, region_id: u64) -> Option<f64> {
    let terrain = world.entities.get(&region_id)?.data.as_region()?.terrain;
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
        .data
        .as_faction()?
        .war_started
        .map(|ts| ts.year())
}

fn end_custom_relationship(
    world: &mut World,
    a: u64,
    b: u64,
    custom_name: &str,
    time: SimTimestamp,
) {
    let kind = RelationshipKind::Custom(custom_name.to_string());
    if let Some(entity) = world.entities.get_mut(&a) {
        for r in &mut entity.relationships {
            if r.target_entity_id == b && r.kind == kind && r.end.is_none() {
                r.end = Some(time);
            }
        }
    }
    if let Some(entity) = world.entities.get_mut(&b) {
        for r in &mut entity.relationships {
            if r.target_entity_id == a && r.kind == kind && r.end.is_none() {
                r.end = Some(time);
            }
        }
    }
}

fn remove_tribute_obligations(world: &mut World, payer_id: u64, payee_id: u64) {
    // Remove tribute obligation if it exists
    if let Some(e) = world.entities.get_mut(&payer_id)
        && let Some(fd) = e.data.as_faction_mut()
    {
        fd.tributes.remove(&payee_id);
    }
}

fn end_at_war_relationship(world: &mut World, a: u64, b: u64, time: SimTimestamp, event_id: u64) {
    let has_a_to_b = world
        .entities
        .get(&a)
        .is_some_and(|e| e.has_active_rel(RelationshipKind::AtWar, b));
    if has_a_to_b {
        world.end_relationship(a, b, RelationshipKind::AtWar, time, event_id);
    }

    let has_b_to_a = world
        .entities
        .get(&b)
        .is_some_and(|e| e.has_active_rel(RelationshipKind::AtWar, a));
    if has_b_to_a {
        world.end_relationship(b, a, RelationshipKind::AtWar, time, event_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entity_data::ActiveSiege;
    use crate::model::{SimTimestamp, World};
    use crate::scenario::Scenario;
    use crate::testutil::{has_signal, war_scenario};
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    #[test]
    fn scenario_factions_are_adjacent() {
        let mut s = Scenario::at_year(1);
        let region_a = s.add_region("Region A");
        let region_b = s.add_region("Region B");
        let region_c = s.add_region("Region C");
        s.make_adjacent(region_a, region_b);

        let faction_a = s.add_faction("Faction A");
        let faction_b = s.add_faction("Faction B");
        let faction_c = s.add_faction("Faction C");

        s.add_settlement("Town A", faction_a, region_a);
        s.add_settlement("Town B", faction_b, region_b);
        s.add_settlement("Town C", faction_c, region_c);
        let world = s.build();

        assert!(factions_are_adjacent(&world, faction_a, faction_b));
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
    fn scenario_find_faction_capital_returns_largest() {
        let mut s = Scenario::at_year(1);
        let region = s.add_region("Region");
        let region2 = s.add_region("Region2");
        let faction = s.add_faction("Faction");
        let _small = s
            .settlement("Small Town", faction, region)
            .population(100)
            .id();
        let big = s
            .settlement("Big City", faction, region2)
            .population(500)
            .id();
        let world = s.build();

        assert_eq!(
            helpers::faction_capital_largest(&world, faction),
            Some((big, region2))
        );
    }

    #[test]
    fn scenario_bfs_next_step_finds_shortest_path() {
        let mut s = Scenario::at_year(1);
        let r1 = s.add_region("R1");
        let r2 = s.add_region("R2");
        let r3 = s.add_region("R3");
        let r4 = s.add_region("R4");
        s.make_adjacent(r1, r2);
        s.make_adjacent(r2, r3);
        s.make_adjacent(r3, r4);
        let world = s.build();

        assert_eq!(helpers::bfs_next_step(&world, r1, r4), Some(r2));
        assert_eq!(helpers::bfs_next_step(&world, r1, r2), Some(r2));
        assert_eq!(helpers::bfs_next_step(&world, r1, r1), None);
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
    fn scenario_territory_status_detection() {
        let mut s = Scenario::at_year(1);
        let region = s.add_region("Region");
        let empty_region = s.add_region("Empty");
        let faction_a = s.add_faction("Faction A");
        let faction_b = s.add_faction("Faction B");
        s.add_settlement("Town", faction_a, region);
        let world = s.build();

        assert_eq!(
            get_territory_status(&world, region, faction_a),
            TerritoryStatus::Friendly
        );
        assert_eq!(
            get_territory_status(&world, region, faction_b),
            TerritoryStatus::Enemy
        );
        assert_eq!(
            get_territory_status(&world, empty_region, faction_a),
            TerritoryStatus::Neutral
        );
    }

    #[test]
    fn scenario_siege_lifts_when_army_destroyed() {
        let w = war_scenario(2, 200);
        let mut world = w.world;
        let army = w.army;
        let settlement = w.target_settlement;
        let attacker = w.attacker_faction;

        // Set up active siege
        {
            let entity = world.entities.get_mut(&settlement).unwrap();
            let sd = entity.data.as_settlement_mut().unwrap();
            sd.active_siege = Some(ActiveSiege {
                attacker_army_id: army,
                attacker_faction_id: attacker,
                started: SimTimestamp::from_year_month(10, 1),
                months_elapsed: 3,
                civilian_deaths: 0,
            });
        }

        // Kill army
        world.end_entity(army, ts(10), 1);

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        siege::progress_sieges(&mut ctx, ts(10), 10);

        assert!(ctx.world.settlement(settlement).active_siege.is_none());
        assert!(has_signal(&signals, |sk| matches!(
            sk,
            SignalKind::SiegeEnded {
                settlement_id: sid,
                outcome,
                ..
            } if *sid == settlement && *outcome == SiegeOutcome::Lifted
        )));
    }

    #[test]
    fn scenario_siege_starvation_reduces_population() {
        let w = war_scenario(1, 200);
        let mut world = w.world;
        let army = w.army;
        let settlement = w.target_settlement;
        let attacker = w.attacker_faction;

        // Set prosperity below starvation threshold and set up siege
        {
            let entity = world.entities.get_mut(&settlement).unwrap();
            let sd = entity.data.as_settlement_mut().unwrap();
            sd.prosperity = 0.15;
            sd.active_siege = Some(ActiveSiege {
                attacker_army_id: army,
                attacker_faction_id: attacker,
                started: SimTimestamp::from_year_month(10, 1),
                months_elapsed: 1,
                civilian_deaths: 0,
            });
        }
        world.army_mut(army).besieging_settlement_id = Some(settlement);

        let pop_before = world.settlement(settlement).population;

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        siege::progress_sieges(&mut ctx, ts(10), 10);

        let sd = ctx.world.settlement(settlement);
        assert!(sd.population < pop_before);
        assert!(sd.prosperity < 0.15);
        assert!(sd.active_siege.as_ref().unwrap().civilian_deaths > 0);
    }

    /// Helper used only by assault tests which need fresh World per RNG iteration.
    fn setup_siege_scenario(fort_level: u8) -> (World, u64, u64, u64, u64, u64) {
        let w = war_scenario(fort_level, 200);
        (
            w.world,
            w.army,
            w.target_settlement,
            w.attacker_faction,
            w.defender_faction,
            w.defender_region,
        )
    }

    #[test]
    fn assault_success_with_overwhelming_force() {
        let (mut world, army, settlement, attacker, _defender, _region) = setup_siege_scenario(1); // palisade

        // Give army huge strength (much greater than fort_level * pop * 0.05)
        {
            let entity = world.entities.get_mut(&army).unwrap();
            let ad = entity.data.as_army_mut().unwrap();
            ad.strength = 5000; // Overwhelmingly strong
            ad.morale = 1.0;
        }

        // Set up siege at month 3+ so assaults are possible
        {
            let entity = world.entities.get_mut(&settlement).unwrap();
            let sd = entity.data.as_settlement_mut().unwrap();
            sd.active_siege = Some(ActiveSiege {
                attacker_army_id: army,
                attacker_faction_id: attacker,
                started: SimTimestamp::from_year_month(10, 1),
                months_elapsed: 5, // Well past assault minimum
                civilian_deaths: 0,
            });
        }
        world.army_mut(army).besieging_settlement_id = Some(settlement);

        // Run many iterations: at least one should succeed via assault or surrender
        let mut conquered = false;
        for seed in 0..100 {
            // Reset world each time from scratch to avoid state accumulation
            let (mut w, a, s, att, _def, _r) = setup_siege_scenario(1);
            {
                let entity = w.entities.get_mut(&a).unwrap();
                let ad = entity.data.as_army_mut().unwrap();
                ad.strength = 5000;
                ad.morale = 1.0;
            }
            {
                let entity = w.entities.get_mut(&s).unwrap();
                let sd = entity.data.as_settlement_mut().unwrap();
                sd.active_siege = Some(ActiveSiege {
                    attacker_army_id: a,
                    attacker_faction_id: att,
                    started: SimTimestamp::from_year_month(10, 1),
                    months_elapsed: 5,
                    civilian_deaths: 0,
                });
            }
            w.army_mut(a).besieging_settlement_id = Some(s);

            let mut rng = SmallRng::seed_from_u64(seed);
            let mut signals = Vec::new();
            let mut ctx = TickContext {
                world: &mut w,
                rng: &mut rng,
                signals: &mut signals,
                inbox: &[],
            };

            siege::progress_sieges(&mut ctx, ts(10), 10);

            let owner = ctx
                .world
                .entities
                .get(&s)
                .unwrap()
                .active_rel(RelationshipKind::MemberOf);
            if owner == Some(att) {
                conquered = true;
                break;
            }
        }
        assert!(
            conquered,
            "overwhelming assault should succeed within 100 tries"
        );
    }

    #[test]
    fn failed_assault_costs_casualties() {
        use crate::model::entity_data::ActiveSiege;
        use rand::SeedableRng;
        use rand::rngs::SmallRng;

        // Use fortress level 3 with huge population to make assault fail
        let (mut world, army, settlement, attacker, _defender, _region) = setup_siege_scenario(3);

        // Make settlement very strong (large pop, high fort)
        {
            let entity = world.entities.get_mut(&settlement).unwrap();
            let sd = entity.data.as_settlement_mut().unwrap();
            sd.population = 10000;
            sd.population_breakdown = PopulationBreakdown::from_total(10000);
            sd.active_siege = Some(ActiveSiege {
                attacker_army_id: army,
                attacker_faction_id: attacker,
                started: SimTimestamp::from_year_month(10, 1),
                months_elapsed: 5,
                civilian_deaths: 0,
            });
        }

        // Army is weak compared to fort defense
        {
            let entity = world.entities.get_mut(&army).unwrap();
            let ad = entity.data.as_army_mut().unwrap();
            ad.strength = 100; // Too weak against fort_level=3 * 10000 * 0.05
            ad.morale = 0.8;
        }
        world.army_mut(army).besieging_settlement_id = Some(settlement);

        // Run many seeds looking for an assault attempt (which should always fail)
        let mut found_failed_assault = false;
        for seed in 0..200 {
            let (mut w, a, s, att, _def, _r) = setup_siege_scenario(3);
            {
                let entity = w.entities.get_mut(&s).unwrap();
                let sd = entity.data.as_settlement_mut().unwrap();
                sd.population = 10000;
                sd.population_breakdown = PopulationBreakdown::from_total(10000);
                sd.active_siege = Some(ActiveSiege {
                    attacker_army_id: a,
                    attacker_faction_id: att,
                    started: SimTimestamp::from_year_month(10, 1),
                    months_elapsed: 5,
                    civilian_deaths: 0,
                });
            }
            {
                let entity = w.entities.get_mut(&a).unwrap();
                let ad = entity.data.as_army_mut().unwrap();
                ad.strength = 100;
                ad.morale = 0.8;
            }
            w.army_mut(a).besieging_settlement_id = Some(s);

            let mut rng = SmallRng::seed_from_u64(seed);
            let mut signals = Vec::new();
            let mut ctx = TickContext {
                world: &mut w,
                rng: &mut rng,
                signals: &mut signals,
                inbox: &[],
            };

            let str_before = ctx
                .world
                .entities
                .get(&a)
                .unwrap()
                .data
                .as_army()
                .unwrap()
                .strength;

            siege::progress_sieges(&mut ctx, ts(10), 10);

            let str_after = ctx
                .world
                .entities
                .get(&a)
                .map(|e| e.data.as_army().map(|ad| ad.strength).unwrap_or(0))
                .unwrap_or(0);

            if str_after < str_before {
                found_failed_assault = true;
                break;
            }
        }
        assert!(
            found_failed_assault,
            "should find at least one failed assault causing casualties in 200 seeds"
        );
    }

    // -- Scenario-based tests (deterministic, no RNG loops) --

    #[test]
    fn scenario_unfortified_conquered_instantly() {
        use crate::testutil::{settlement_owner, war_scenario};

        let w = war_scenario(0, 200); // fort_level=0
        let mut world = w.world;
        let settlement = w.target_settlement;
        let attacker = w.attacker_faction;

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        siege::start_sieges(&mut ctx, ts(10), 10);

        assert_eq!(settlement_owner(ctx.world, settlement), Some(attacker));
        assert!(ctx.world.settlement(settlement).active_siege.is_none());
    }

    #[test]
    fn scenario_fortified_enters_siege() {
        use crate::testutil::{has_signal, settlement_owner, war_scenario};

        let w = war_scenario(2, 200); // stone walls
        let mut world = w.world;
        let army = w.army;
        let settlement = w.target_settlement;
        let attacker = w.attacker_faction;
        let defender = w.defender_faction;

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        siege::start_sieges(&mut ctx, ts(10), 10);

        // Still belongs to defender
        assert_eq!(settlement_owner(ctx.world, settlement), Some(defender));

        // Active siege exists
        let siege = ctx
            .world
            .settlement(settlement)
            .active_siege
            .as_ref()
            .expect("should have active siege");
        assert_eq!(siege.attacker_army_id, army);
        assert_eq!(siege.attacker_faction_id, attacker);
        assert_eq!(siege.months_elapsed, 0);

        // SiegeStarted signal emitted
        assert!(has_signal(&signals, |sk| matches!(
            sk,
            SignalKind::SiegeStarted {
                settlement_id: sid,
                attacker_faction_id: afid,
                ..
            } if *sid == settlement && *afid == attacker
        )));
    }

    #[test]
    fn scenario_siege_end_clears_besieging_field() {
        // Start a siege so the army has besieging_settlement_id set
        let setup = war_scenario(2, 100);
        let mut world = setup.world;
        let army = setup.army;
        let settlement = setup.target_settlement;

        // Set the besieging field
        world.army_mut(army).besieging_settlement_id = Some(settlement);
        assert_eq!(
            world.army(army).besieging_settlement_id,
            Some(settlement)
        );

        // Clear it via the function under test
        let ev = world.add_event(
            EventKind::Custom("siege_end".to_string()),
            ts(10),
            "Siege ended".to_string(),
        );
        siege::clear_besieging_extra(&mut world, army, ev);

        assert_eq!(world.army(army).besieging_settlement_id, None);
    }
}
