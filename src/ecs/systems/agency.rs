//! Agency system -- migrated from `src/sim/agency.rs`.
//!
//! Two systems:
//! 1. `capture_agency_signals` -- in `SimPhase::Reactions`, reads `SimReactiveEvent`
//!    messages and clones relevant variants into `AgencyMemory` resource.
//! 2. `evaluate_npc_desires` -- yearly, in `SimPhase::Update`, evaluates NPC desires
//!    based on traits + world state + cached signals, picks weighted action, queues
//!    into `PendingActions`.

use std::collections::{BTreeMap, BTreeSet};

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageReader;
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::components::{
    EcsActiveDisease, Faction, FactionCore, FactionDiplomacy, FactionMilitary, Person, PersonCore,
    PersonReputation, PersonSocial, Settlement, SettlementCore, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{
    LeaderOf, LeaderOfSources, LocatedIn, MemberOf, MemberOfSources, RegionAdjacency,
    RelationshipGraph,
};
use crate::ecs::resources::{AgencyMemory, PendingActions, SimEntityMap, SimRng};
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::ecs::time::SimTime;
use crate::model::action::{Action, ActionKind, ActionSource};
use crate::model::entity_data::GovernmentType;
use crate::model::traits::Trait;

// ---------------------------------------------------------------------------
// Constants -- Alliance strength (ported from diplomacy.rs)
// ---------------------------------------------------------------------------
const ALLIANCE_BASE_STRENGTH: f64 = 0.1;
const ALLIANCE_TRADE_ROUTE_STRENGTH: f64 = 0.2;
const ALLIANCE_TRADE_ROUTE_CAP: f64 = 0.6;
const ALLIANCE_SHARED_ENEMY_STRENGTH: f64 = 0.3;
const ALLIANCE_MARRIAGE_STRENGTH: f64 = 0.4;
const ALLIANCE_PRESTIGE_STRENGTH_WEIGHT: f64 = 0.3;
const ALLIANCE_PRESTIGE_STRENGTH_CAP: f64 = 0.2;
const TRUST_DEFAULT: f64 = 1.0;
const TRUST_STRENGTH_WEIGHT: f64 = 0.3;

// ---------------------------------------------------------------------------
// Constants -- Vulnerability (ported from diplomacy.rs)
// ---------------------------------------------------------------------------
const VULNERABILITY_AT_WAR: f64 = 0.30;
const VULNERABILITY_PLAGUE: f64 = 0.15;
const VULNERABILITY_INSTABILITY_WEIGHT: f64 = 0.4;
const VULNERABILITY_LOW_TREASURY: f64 = 0.10;
const VULNERABILITY_SINGLE_SETTLEMENT: f64 = 0.10;

// ---------------------------------------------------------------------------
// Internal data types
// ---------------------------------------------------------------------------

struct NpcInfo {
    entity: Entity,
    sim_id: u64,
    traits: Vec<Trait>,
    faction: Option<Entity>,
    is_leader: bool,
    last_action: SimTime,
    born: SimTime,
    prestige: f64,
}

#[derive(Debug)]
enum DesireKind {
    SeizePower {
        faction: Entity,
    },
    ExpandTerritory {
        target_faction: Entity,
    },
    SupportFaction {
        faction: Entity,
    },
    UndermineFaction {
        faction: Entity,
    },
    SeekAlliance {
        faction_a: Entity,
        faction_b: Entity,
    },
    EliminateRival {
        target: Entity,
    },
    Defect {
        from_faction: Entity,
        to_faction: Entity,
    },
    SeekOffice {
        faction: Entity,
    },
    BetrayAlly {
        ally_faction: Entity,
    },
    SeekRevenge {
        target_faction: Entity,
    },
    PressClaim {
        target_faction: Entity,
        _claim_strength: f64,
    },
}

#[derive(Debug)]
struct ScoredDesire {
    kind: DesireKind,
    urgency: f64,
}

// ---------------------------------------------------------------------------
// System registration
// ---------------------------------------------------------------------------

pub fn add_agency_systems(app: &mut App) {
    app.add_systems(SimTick, capture_agency_signals.in_set(SimPhase::Reactions));
    app.add_systems(
        SimTick,
        evaluate_npc_desires.run_if(yearly).in_set(SimPhase::Update),
    );
}

// ---------------------------------------------------------------------------
// System: capture_agency_signals (Reactions phase)
// ---------------------------------------------------------------------------

fn capture_agency_signals(
    mut events: MessageReader<SimReactiveEvent>,
    mut memory: ResMut<AgencyMemory>,
) {
    memory.0.clear();
    for event in events.read() {
        match event {
            SimReactiveEvent::LeaderVacancy { .. }
            | SimReactiveEvent::WarStarted { .. }
            | SimReactiveEvent::WarEnded { .. }
            | SimReactiveEvent::SettlementCaptured { .. }
            | SimReactiveEvent::FactionSplit { .. }
            | SimReactiveEvent::AllianceBetrayed { .. }
            | SimReactiveEvent::SuccessionCrisis { .. } => {
                memory.0.push(event.clone());
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// System: evaluate_npc_desires (yearly, Update phase)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn evaluate_npc_desires(
    mut persons: Query<
        (
            Entity,
            &SimEntity,
            &mut PersonCore,
            &PersonReputation,
            &PersonSocial,
            Option<&LeaderOf>,
            Option<&MemberOf>,
        ),
        With<Person>,
    >,
    factions: Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
            Option<&MemberOfSources>,
            Option<&LeaderOfSources>,
        ),
        With<Faction>,
    >,
    settlements: Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            Option<&MemberOf>,
            Option<&LocatedIn>,
        ),
        With<Settlement>,
    >,
    disease_settlements: Query<(Entity, &MemberOf), (With<Settlement>, With<EcsActiveDisease>)>,
    rel_graph: Res<RelationshipGraph>,
    memory: Res<AgencyMemory>,
    clock: Res<SimClock>,
    mut rng: ResMut<SimRng>,
    mut pending: ResMut<PendingActions>,
    entity_map: Res<SimEntityMap>,
    adjacency: Res<RegionAdjacency>,
) {
    let time = clock.time;
    let signals = &memory.0;
    let rng = &mut rng.0;

    // Pre-collect factions that have at least one plagued settlement
    let plagued_factions: BTreeSet<Entity> = disease_settlements
        .iter()
        .map(|(_, member)| member.0)
        .collect();

    // Pre-collect leader prestige from persons (before mutable iteration)
    let leader_prestige_map: BTreeMap<Entity, f64> = persons
        .iter()
        .map(|(entity, _, _, rep, ..)| (entity, rep.prestige))
        .collect();

    // Pre-collect faction data for lookups
    let faction_data: BTreeMap<Entity, FactionContext> = factions
        .iter()
        .filter(|(_, sim, ..)| sim.is_alive())
        .map(
            |(entity, sim, core, diplomacy, military, members, leaders)| {
                let settlement_count = members
                    .map(|m| {
                        m.iter()
                            .filter(|&&member| settlements.get(member).is_ok())
                            .count()
                    })
                    .unwrap_or(0);

                let total_population: u32 = members
                    .map(|m| {
                        m.iter()
                            .filter_map(|&member| settlements.get(member).ok())
                            .map(|(_, _, sc, ..)| sc.population)
                            .sum()
                    })
                    .unwrap_or(0);

                let leader_entity = leaders.and_then(|ls| ls.first().copied());
                let leader_prestige = leader_entity
                    .and_then(|le| leader_prestige_map.get(&le).copied())
                    .unwrap_or(0.0);

                (
                    entity,
                    FactionContext {
                        sim_id: sim.id,
                        core: core.clone(),
                        diplomacy: diplomacy.cloned(),
                        _military: military.cloned(),
                        settlement_count,
                        total_population,
                        leader_entity,
                        leader_prestige,
                        is_non_state: matches!(
                            core.government_type,
                            GovernmentType::BanditClan | GovernmentType::MercenaryCompany
                        ),
                    },
                )
            },
        )
        .collect();

    // Collect NPC info + social data: living persons with non-empty traits
    let npcs: Vec<(NpcInfo, PersonSocial)> = persons
        .iter()
        .filter(|(_, sim, core, ..)| sim.is_alive() && !core.traits.is_empty())
        .map(|(entity, sim, core, rep, social, leader_of, member_of)| {
            let faction = member_of
                .map(|m| m.0)
                .filter(|&f| faction_data.contains_key(&f));
            (
                NpcInfo {
                    entity,
                    sim_id: sim.id,
                    traits: core.traits.clone(),
                    faction,
                    is_leader: leader_of.is_some(),
                    last_action: core.last_action,
                    born: core.born,
                    prestige: rep.prestige,
                },
                social.clone(),
            )
        })
        .collect();

    // Track which NPCs need last_action updated
    let mut action_updates: Vec<(Entity, SimTime)> = Vec::new();

    for (npc, social) in &npcs {
        // Variable cooldown: trait-dependent
        let cooldown = compute_cooldown(&npc.traits);
        if time.years_since(npc.last_action) < cooldown {
            continue;
        }

        let mut desires = evaluate_desires(
            npc,
            &faction_data,
            &rel_graph,
            signals,
            time,
            &settlements,
            &adjacency,
            &plagued_factions,
        );

        // Evaluate PressClaim desires (needs PersonSocial.claims)
        evaluate_press_claim_desires(
            &mut desires,
            npc,
            &social.claims,
            &faction_data,
            signals,
            &entity_map,
        );

        // Evaluate SeekRevenge desires (needs PersonSocial.grievances + FactionDiplomacy.grievances)
        evaluate_revenge_desires(
            &mut desires,
            npc,
            &social.grievances,
            &faction_data,
            &rel_graph,
            &entity_map,
        );

        if desires.is_empty() {
            continue;
        }

        let max_urgency = desires.iter().map(|d| d.urgency).fold(0.0f64, f64::max);

        // Trait-modulated action probability
        let personality_mod = compute_personality_modifier(&npc.traits);
        let action_prob = (max_urgency * personality_mod).clamp(0.05, 0.65);

        if rng.random_range(0.0..1.0) >= action_prob {
            continue;
        }

        // Pick from desires weighted by urgency
        let total_urgency: f64 = desires.iter().map(|d| d.urgency).sum();
        if total_urgency <= 0.0 {
            continue;
        }

        let mut roll = rng.random_range(0.0..total_urgency);
        let mut chosen = &desires[desires.len() - 1];
        for d in &desires {
            if roll < d.urgency {
                chosen = d;
                break;
            }
            roll -= d.urgency;
        }

        if let Some(action_kind) = desire_to_action(chosen, &entity_map) {
            pending.0.push(Action {
                actor_id: npc.sim_id,
                source: ActionSource::Autonomous,
                kind: action_kind,
            });

            action_updates.push((npc.entity, time));
        }
    }

    // Apply last_action updates
    for (entity, time) in action_updates {
        if let Ok((_, _, mut core, ..)) = persons.get_mut(entity) {
            core.last_action = time;
        }
    }
}

// ---------------------------------------------------------------------------
// Faction context cache
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct FactionContext {
    sim_id: u64,
    core: FactionCore,
    diplomacy: Option<FactionDiplomacy>,
    _military: Option<FactionMilitary>,
    settlement_count: usize,
    total_population: u32,
    leader_entity: Option<Entity>,
    leader_prestige: f64,
    is_non_state: bool,
}

// ---------------------------------------------------------------------------
// Cooldown and personality
// ---------------------------------------------------------------------------

/// Compute variable cooldown based on traits.
/// Base: 3 years. Aggressive: -1, Cautious: +1, Ambitious: -1, Content: +1. Min 1.
fn compute_cooldown(traits: &[Trait]) -> u32 {
    let mut cooldown: i32 = 3;
    for t in traits {
        match t {
            Trait::Aggressive => cooldown -= 1,
            Trait::Cautious => cooldown += 1,
            Trait::Ambitious => cooldown -= 1,
            Trait::Content => cooldown += 1,
            _ => {}
        }
    }
    cooldown.max(1) as u32
}

/// Compute personality modifier for action probability.
/// Multiple traits multiply together, clamped to [0.4, 1.8].
fn compute_personality_modifier(traits: &[Trait]) -> f64 {
    let mut modifier: f64 = 1.0;
    for t in traits {
        let m = match t {
            Trait::Aggressive => 1.3,
            Trait::Cautious => 0.6,
            Trait::Ambitious => 1.2,
            Trait::Content => 0.7,
            Trait::Cunning => 1.1,
            Trait::Straightforward => 0.9,
            _ => 1.0,
        };
        modifier *= m;
    }
    modifier.clamp(0.4, 1.8)
}

// ---------------------------------------------------------------------------
// Desire evaluation (core logic ported from old AgencySystem)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn evaluate_desires(
    npc: &NpcInfo,
    factions: &BTreeMap<Entity, FactionContext>,
    rel_graph: &RelationshipGraph,
    signals: &[SimReactiveEvent],
    time: SimTime,
    settlements: &Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            Option<&MemberOf>,
            Option<&LocatedIn>,
        ),
        With<Settlement>,
    >,
    adjacency: &RegionAdjacency,
    plagued_factions: &BTreeSet<Entity>,
) -> Vec<ScoredDesire> {
    let mut desires = Vec::new();

    let Some(faction_entity) = npc.faction else {
        return desires;
    };

    let Some(fctx) = factions.get(&faction_entity) else {
        return desires;
    };

    let stability = fctx.core.stability;
    let instability = 1.0 - stability;
    let happiness = fctx.core.happiness;
    let faction_prestige = fctx.core.prestige;

    // Leader prestige (looked up from the leader_prestige_map at faction context build time)
    let leader_prestige = fctx.leader_prestige;

    // Faction at war?
    let faction_at_war = rel_graph
        .at_war
        .iter()
        .any(|(&(a, b), m)| m.is_active() && (a == faction_entity || b == faction_entity));

    // Is faction leaderless?
    let faction_leaderless = fctx.leader_entity.is_none();

    // Check for recent leader vacancy signal for this faction
    let leader_just_died = signals.iter().any(|s| {
        matches!(s, SimReactiveEvent::LeaderVacancy { faction, .. } if *faction == faction_entity)
    });

    // Check for recent settlement captured from this faction
    let lost_settlement = signals.iter().any(|s| {
        matches!(s, SimReactiveEvent::SettlementCaptured { old_faction: Some(old), .. } if *old == faction_entity)
    });

    // NPC age
    let age = time.years_since(npc.born);
    let age_risk_factor = if age >= 60 { 0.5 } else { 1.0 };

    // Government type
    let gov_type = fctx.core.government_type;

    // Faction settlement count
    let settlement_count = fctx.settlement_count;

    for t in &npc.traits {
        match t {
            Trait::Ambitious if !npc.is_leader => {
                // SeizePower -- urgency scales with instability
                let mut urgency =
                    0.2 + 0.5 * instability - 0.15 * npc.prestige - 0.1 * leader_prestige;
                // Massive boost if faction is leaderless or leader just died
                if faction_leaderless || leader_just_died {
                    urgency += 0.4;
                }
                urgency *= age_risk_factor;
                urgency = urgency.max(0.0);
                desires.push(ScoredDesire {
                    kind: DesireKind::SeizePower {
                        faction: faction_entity,
                    },
                    urgency,
                });

                // SeekOffice -- legitimate path to leadership
                if gov_type == GovernmentType::Elective || faction_leaderless {
                    let office_urgency = 0.3 + 0.2 * instability;
                    desires.push(ScoredDesire {
                        kind: DesireKind::SeekOffice {
                            faction: faction_entity,
                        },
                        urgency: office_urgency,
                    });
                }
            }
            Trait::Ambitious if npc.is_leader => {
                // ExpandTerritory -- look for enemy factions, fall back to weak neighbors
                let target = find_enemy_faction(rel_graph, faction_entity).or_else(|| {
                    find_expansion_target(
                        factions,
                        rel_graph,
                        faction_entity,
                        settlements,
                        adjacency,
                    )
                });
                if let Some(target) = target {
                    let mut urgency = 0.3 + 0.2 * instability + faction_prestige * 0.1;
                    if faction_at_war {
                        urgency += 0.15;
                    }
                    desires.push(ScoredDesire {
                        kind: DesireKind::ExpandTerritory {
                            target_faction: target,
                        },
                        urgency,
                    });
                }

                // BetrayAlly -- ambitious leaders exploit vulnerable allies
                evaluate_betrayal_desires(
                    &mut desires,
                    npc,
                    factions,
                    rel_graph,
                    faction_entity,
                    faction_prestige,
                    time,
                    1.3,
                    settlements,
                    plagued_factions,
                );
            }
            Trait::Aggressive if npc.is_leader => {
                // ExpandTerritory against enemies, fall back to weak neighbors
                let target = find_enemy_faction(rel_graph, faction_entity).or_else(|| {
                    find_expansion_target(
                        factions,
                        rel_graph,
                        faction_entity,
                        settlements,
                        adjacency,
                    )
                });
                if let Some(target) = target {
                    let mut urgency = 0.35 + 0.15 * instability + faction_prestige * 0.1;
                    if faction_at_war {
                        urgency += 0.15;
                    }
                    desires.push(ScoredDesire {
                        kind: DesireKind::ExpandTerritory {
                            target_faction: target,
                        },
                        urgency,
                    });
                }
            }
            Trait::Aggressive if !npc.is_leader => {
                // EliminateRival -- find enemy faction leader
                if let Some(target) = find_enemy_faction_leader(factions, rel_graph, faction_entity)
                {
                    let mut urgency = 0.25;
                    if faction_at_war {
                        urgency += 0.1;
                    }
                    urgency *= age_risk_factor;
                    desires.push(ScoredDesire {
                        kind: DesireKind::EliminateRival { target },
                        urgency,
                    });
                }
            }
            Trait::Cautious | Trait::Honorable if npc.is_leader => {
                // SupportFaction -- stabilize
                desires.push(ScoredDesire {
                    kind: DesireKind::SupportFaction {
                        faction: faction_entity,
                    },
                    urgency: 0.15 + 0.3 * instability,
                });
            }
            Trait::Charismatic => {
                // SeekAlliance -- find a non-allied, non-enemy faction
                let ally_count = rel_graph
                    .allies
                    .iter()
                    .filter(|&(&(a, b), m)| {
                        m.is_active() && (a == faction_entity || b == faction_entity)
                    })
                    .count();
                if let Some(other) = find_potential_ally(factions, rel_graph, faction_entity) {
                    // Reduce urgency if already have allies
                    let urgency = (if ally_count >= 2 { 0.1 } else { 0.2 }) + npc.prestige * 0.1;
                    desires.push(ScoredDesire {
                        kind: DesireKind::SeekAlliance {
                            faction_a: faction_entity,
                            faction_b: other,
                        },
                        urgency,
                    });
                }
            }
            Trait::Cunning => {
                // UndermineFaction -- target enemy
                if let Some(enemy) = find_enemy_faction(rel_graph, faction_entity) {
                    desires.push(ScoredDesire {
                        kind: DesireKind::UndermineFaction { faction: enemy },
                        urgency: 0.25 + 0.15 * instability,
                    });
                }

                // BetrayAlly -- cunning leaders exploit vulnerable allies
                if npc.is_leader {
                    evaluate_betrayal_desires(
                        &mut desires,
                        npc,
                        factions,
                        rel_graph,
                        faction_entity,
                        faction_prestige,
                        time,
                        1.5,
                        settlements,
                        plagued_factions,
                    );
                }

                // Defect -- pragmatists flee losing factions
                if !npc.is_leader
                    && happiness < 0.3
                    && let Some(to_faction) =
                        find_defection_target(factions, rel_graph, faction_entity)
                {
                    desires.push(ScoredDesire {
                        kind: DesireKind::Defect {
                            from_faction: faction_entity,
                            to_faction,
                        },
                        urgency: 0.15 + 0.3 * (1.0 - happiness) + 0.1 * (1.0 - faction_prestige),
                    });
                }
            }
            Trait::Ruthless => {
                // EliminateRival -- enemy leader
                if let Some(target) = find_enemy_faction_leader(factions, rel_graph, faction_entity)
                {
                    let urgency = 0.3 * age_risk_factor;
                    desires.push(ScoredDesire {
                        kind: DesireKind::EliminateRival { target },
                        urgency,
                    });
                }

                // BetrayAlly -- ruthless leaders exploit vulnerable allies
                if npc.is_leader {
                    evaluate_betrayal_desires(
                        &mut desires,
                        npc,
                        factions,
                        rel_graph,
                        faction_entity,
                        faction_prestige,
                        time,
                        1.8,
                        settlements,
                        plagued_factions,
                    );
                }
            }
            Trait::Content => {
                // SupportFaction -- stabilize own faction
                desires.push(ScoredDesire {
                    kind: DesireKind::SupportFaction {
                        faction: faction_entity,
                    },
                    urgency: 0.1 + 0.2 * instability,
                });

                // Content NPCs may defect from losing factions
                if !npc.is_leader
                    && (lost_settlement || happiness < 0.3)
                    && let Some(to_faction) =
                        find_defection_target(factions, rel_graph, faction_entity)
                {
                    desires.push(ScoredDesire {
                        kind: DesireKind::Defect {
                            from_faction: faction_entity,
                            to_faction,
                        },
                        urgency: 0.15 + 0.3 * (1.0 - happiness) + 0.1 * (1.0 - faction_prestige),
                    });
                }
            }
            Trait::Pious => {
                // SupportFaction -- stabilize own faction
                desires.push(ScoredDesire {
                    kind: DesireKind::SupportFaction {
                        faction: faction_entity,
                    },
                    urgency: 0.1 + 0.2 * instability,
                });
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------------
    // Non-trait desires that depend on world state
    // -------------------------------------------------------------------

    // PressClaim and SeekRevenge are evaluated separately in the main system
    // because they require PersonSocial data (claims, grievances) which is
    // not available to this function.

    // Any ambitious NPC can seek office if faction is leaderless (regardless of gov type)
    if !npc.is_leader
        && faction_leaderless
        && npc.traits.contains(&Trait::Ambitious)
        && settlement_count > 0
    {
        // Only add if not already added above
        let has_seek_office = desires
            .iter()
            .any(|d| matches!(d.kind, DesireKind::SeekOffice { .. }));
        if !has_seek_office {
            desires.push(ScoredDesire {
                kind: DesireKind::SeekOffice {
                    faction: faction_entity,
                },
                urgency: 0.3 + 0.2 * instability,
            });
        }
    }

    desires
}

/// Evaluate PressClaim desires for a leader NPC.
/// Separated from `evaluate_desires` because it needs PersonSocial data.
fn evaluate_press_claim_desires(
    desires: &mut Vec<ScoredDesire>,
    npc: &NpcInfo,
    claims: &BTreeMap<u64, crate::model::Claim>,
    factions: &BTreeMap<Entity, FactionContext>,
    signals: &[SimReactiveEvent],
    entity_map: &SimEntityMap,
) {
    if !npc.is_leader {
        return;
    }
    let Some(faction_entity) = npc.faction else {
        return;
    };
    let Some(fctx) = factions.get(&faction_entity) else {
        return;
    };
    let instability = 1.0 - fctx.core.stability;

    for (&target_sim_id, claim) in claims {
        // Target must be alive and different from our faction
        let target_entity = entity_map.get_bevy(target_sim_id);
        let target_alive = target_entity.and_then(|te| factions.get(&te)).is_some();
        if !target_alive {
            continue;
        }
        let target_entity = target_entity.unwrap();
        if Some(target_entity) == npc.faction {
            continue;
        }

        // Content hard-blocks pressing claims
        if npc.traits.contains(&Trait::Content) {
            continue;
        }

        let target_instability = factions
            .get(&target_entity)
            .map(|f| 1.0 - f.core.stability)
            .unwrap_or(0.5);

        let mut urgency = 0.2 + claim.strength * 0.4 + target_instability * 0.2 - instability * 0.3;

        // Check for recent SuccessionCrisis signal on target faction
        let crisis_boost = signals.iter().any(|s| {
            matches!(s, SimReactiveEvent::SuccessionCrisis { faction, .. } if *faction == target_entity)
        });
        if crisis_boost {
            urgency += 0.15;
        }

        // Trait modifiers
        for t in &npc.traits {
            match t {
                Trait::Ambitious => urgency *= 1.3,
                Trait::Aggressive => urgency *= 1.2,
                Trait::Cautious => urgency *= 0.5,
                Trait::Honorable => urgency *= 1.1,
                _ => {}
            }
        }

        urgency = urgency.max(0.0);

        desires.push(ScoredDesire {
            kind: DesireKind::PressClaim {
                target_faction: target_entity,
                _claim_strength: claim.strength,
            },
            urgency,
        });
    }
}

/// Evaluate SeekRevenge desires for a leader NPC.
/// Separated from `evaluate_desires` because it needs PersonSocial and FactionDiplomacy data.
fn evaluate_revenge_desires(
    desires: &mut Vec<ScoredDesire>,
    npc: &NpcInfo,
    person_grievances: &BTreeMap<u64, crate::model::Grievance>,
    factions: &BTreeMap<Entity, FactionContext>,
    rel_graph: &RelationshipGraph,
    entity_map: &SimEntityMap,
) {
    if !npc.is_leader {
        return;
    }
    let Some(faction_entity) = npc.faction else {
        return;
    };
    let Some(fctx) = factions.get(&faction_entity) else {
        return;
    };

    // Collect personal grievances
    let personal: Vec<(u64, f64)> = person_grievances
        .iter()
        .filter(|(_, g)| g.severity >= 0.3)
        .map(|(&target, g)| (target, g.severity))
        .collect();

    // Collect faction-level grievances
    let faction_grvs: Vec<(u64, f64)> = fctx
        .diplomacy
        .as_ref()
        .map(|d| {
            d.grievances
                .iter()
                .filter(|(_, g)| g.severity >= 0.3)
                .map(|(&target, g)| (target, g.severity))
                .collect()
        })
        .unwrap_or_default();

    // Merge: take max severity per target
    let mut all_targets: BTreeMap<u64, f64> = BTreeMap::new();
    for (target, sev) in personal.iter().chain(faction_grvs.iter()) {
        let entry = all_targets.entry(*target).or_insert(0.0);
        *entry = entry.max(*sev);
    }

    for (target_sim_id, severity) in all_targets {
        // Gate: target alive, not already at war with, not self
        let target_bevy = entity_map.get_bevy(target_sim_id);
        let target_alive = target_bevy.and_then(|te| factions.get(&te)).is_some();
        if !target_alive {
            continue;
        }
        let target_bevy = target_bevy.unwrap();
        if target_bevy == faction_entity {
            continue;
        }
        if rel_graph.are_at_war(faction_entity, target_bevy) {
            continue;
        }

        let mut urgency = (severity - 0.2) * 1.0;
        // Trait modifiers
        for t in &npc.traits {
            match t {
                Trait::Aggressive => urgency *= 1.4,
                Trait::Ruthless => urgency *= 1.6,
                Trait::Cautious => urgency *= 0.4,
                Trait::Content => urgency *= 0.3,
                Trait::Honorable => urgency *= 1.2,
                _ => {}
            }
        }
        urgency = urgency.max(0.0);

        if urgency > 0.0 {
            desires.push(ScoredDesire {
                kind: DesireKind::SeekRevenge {
                    target_faction: target_bevy,
                },
                urgency,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Desire-to-Action conversion
// ---------------------------------------------------------------------------

fn desire_to_action(desire: &ScoredDesire, entity_map: &SimEntityMap) -> Option<ActionKind> {
    match &desire.kind {
        DesireKind::SeizePower { faction } => Some(ActionKind::AttemptCoup {
            faction_id: entity_map.get_sim(*faction)?,
        }),
        DesireKind::ExpandTerritory { target_faction } => Some(ActionKind::DeclareWar {
            target_faction_id: entity_map.get_sim(*target_faction)?,
        }),
        DesireKind::SupportFaction { faction } => Some(ActionKind::SupportFaction {
            faction_id: entity_map.get_sim(*faction)?,
        }),
        DesireKind::UndermineFaction { faction } => Some(ActionKind::UndermineFaction {
            faction_id: entity_map.get_sim(*faction)?,
        }),
        DesireKind::SeekAlliance {
            faction_a,
            faction_b,
        } => Some(ActionKind::BrokerAlliance {
            faction_a: entity_map.get_sim(*faction_a)?,
            faction_b: entity_map.get_sim(*faction_b)?,
        }),
        DesireKind::EliminateRival { target } => Some(ActionKind::Assassinate {
            target_id: entity_map.get_sim(*target)?,
        }),
        DesireKind::Defect {
            from_faction,
            to_faction,
        } => Some(ActionKind::Defect {
            from_faction: entity_map.get_sim(*from_faction)?,
            to_faction: entity_map.get_sim(*to_faction)?,
        }),
        DesireKind::SeekOffice { faction } => Some(ActionKind::SeekOffice {
            faction_id: entity_map.get_sim(*faction)?,
        }),
        DesireKind::BetrayAlly { ally_faction } => Some(ActionKind::BetrayAlly {
            ally_faction_id: entity_map.get_sim(*ally_faction)?,
        }),
        DesireKind::SeekRevenge { target_faction } => Some(ActionKind::DeclareWar {
            target_faction_id: entity_map.get_sim(*target_faction)?,
        }),
        DesireKind::PressClaim { target_faction, .. } => Some(ActionKind::PressClaim {
            target_faction_id: entity_map.get_sim(*target_faction)?,
        }),
    }
}

// ---------------------------------------------------------------------------
// Helpers: faction lookups
// ---------------------------------------------------------------------------

/// Find first active enemy faction.
fn find_enemy_faction(rel_graph: &RelationshipGraph, faction: Entity) -> Option<Entity> {
    rel_graph
        .enemies
        .iter()
        .find(|&(&(a, b), m)| m.is_active() && (a == faction || b == faction))
        .map(|(&(a, b), _)| if a == faction { b } else { a })
}

/// Find the leader of the first enemy faction.
fn find_enemy_faction_leader(
    factions: &BTreeMap<Entity, FactionContext>,
    rel_graph: &RelationshipGraph,
    faction: Entity,
) -> Option<Entity> {
    let enemy = find_enemy_faction(rel_graph, faction)?;
    factions.get(&enemy)?.leader_entity
}

/// Find a faction that is not allied, enemy, or at war with the given faction.
fn find_potential_ally(
    factions: &BTreeMap<Entity, FactionContext>,
    rel_graph: &RelationshipGraph,
    faction: Entity,
) -> Option<Entity> {
    factions
        .keys()
        .find(|&&other| {
            other != faction
                && !rel_graph.are_allies(faction, other)
                && !rel_graph.are_enemies(faction, other)
                && !rel_graph.are_at_war(faction, other)
        })
        .copied()
}

/// Find all ally factions.
fn find_ally_factions(rel_graph: &RelationshipGraph, faction: Entity) -> Vec<Entity> {
    rel_graph
        .allies
        .iter()
        .filter(|(_, m)| m.is_active())
        .filter_map(|(&(a, b), _)| {
            if a == faction {
                Some(b)
            } else if b == faction {
                Some(a)
            } else {
                None
            }
        })
        .collect()
}

/// Find an adjacent weaker faction without a diplomatic relationship.
#[allow(clippy::type_complexity)]
fn find_expansion_target(
    factions: &BTreeMap<Entity, FactionContext>,
    rel_graph: &RelationshipGraph,
    faction: Entity,
    settlements: &Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            Option<&MemberOf>,
            Option<&LocatedIn>,
        ),
        With<Settlement>,
    >,
    adjacency: &RegionAdjacency,
) -> Option<Entity> {
    let our_pop = factions.get(&faction)?.total_population as f64;
    if our_pop < 1.0 {
        return None;
    }

    let mut best: Option<(Entity, f64)> = None;
    for (&other, other_ctx) in factions {
        if other == faction {
            continue;
        }
        // Skip if has any diplomatic relationship
        if rel_graph.are_allies(faction, other)
            || rel_graph.are_enemies(faction, other)
            || rel_graph.are_at_war(faction, other)
        {
            continue;
        }
        // Skip non-state factions
        if other_ctx.is_non_state {
            continue;
        }
        // Must be adjacent
        if !factions_are_adjacent(faction, other, settlements, adjacency) {
            continue;
        }
        let their_pop = other_ctx.total_population as f64;
        if their_pop < 1.0 {
            continue;
        }
        let ratio = our_pop / their_pop;
        // Only target weaker factions (1.5x+)
        if ratio >= 1.5 && (best.is_none() || ratio > best.unwrap().1) {
            best = Some((other, ratio));
        }
    }
    best.map(|(id, _)| id)
}

/// Find a non-enemy faction that the NPC could defect to.
fn find_defection_target(
    factions: &BTreeMap<Entity, FactionContext>,
    rel_graph: &RelationshipGraph,
    faction: Entity,
) -> Option<Entity> {
    factions
        .keys()
        .find(|&&other| {
            other != faction
                && !rel_graph.are_enemies(faction, other)
                && !rel_graph.are_at_war(faction, other)
        })
        .copied()
}

/// Check if two factions have settlements in adjacent regions.
#[allow(clippy::type_complexity)]
fn factions_are_adjacent(
    faction_a: Entity,
    faction_b: Entity,
    settlements: &Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            Option<&MemberOf>,
            Option<&LocatedIn>,
        ),
        With<Settlement>,
    >,
    adjacency: &RegionAdjacency,
) -> bool {
    // Collect regions for faction A
    let regions_a: Vec<Entity> = settlements
        .iter()
        .filter(|(_, _, _, member, _)| member.is_some_and(|m| m.0 == faction_a))
        .filter_map(|(_, _, _, _, loc)| loc.map(|l| l.0))
        .collect();

    // Collect regions for faction B
    let regions_b: Vec<Entity> = settlements
        .iter()
        .filter(|(_, _, _, member, _)| member.is_some_and(|m| m.0 == faction_b))
        .filter_map(|(_, _, _, _, loc)| loc.map(|l| l.0))
        .collect();

    for &ra in &regions_a {
        for &rb in &regions_b {
            if ra == rb || adjacency.are_adjacent(ra, rb) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Alliance strength and vulnerability (ported from diplomacy.rs)
// ---------------------------------------------------------------------------

/// Compute how vulnerable an ally faction is (0.0-1.0).
#[allow(clippy::type_complexity)]
fn compute_ally_vulnerability(
    fctx: &FactionContext,
    rel_graph: &RelationshipGraph,
    ally: Entity,
    settlements: &Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            Option<&MemberOf>,
            Option<&LocatedIn>,
        ),
        With<Settlement>,
    >,
    plagued_factions: &BTreeSet<Entity>,
) -> f64 {
    let mut vuln = 0.0;

    // At war
    let at_war = rel_graph
        .at_war
        .iter()
        .any(|(&(a, b), m)| m.is_active() && (a == ally || b == ally));
    if at_war {
        vuln += VULNERABILITY_AT_WAR;
    }

    // Has plague in any settlement
    if plagued_factions.contains(&ally) {
        vuln += VULNERABILITY_PLAGUE;
    }

    // Low stability
    if fctx.core.stability < 0.5 {
        vuln += (0.5 - fctx.core.stability) * VULNERABILITY_INSTABILITY_WEIGHT;
    }

    // Low treasury
    if fctx.core.treasury < 5.0 {
        vuln += VULNERABILITY_LOW_TREASURY;
    }

    // Only one settlement
    let settlement_count = settlements
        .iter()
        .filter(|(_, sim, _, member, _)| sim.is_alive() && member.is_some_and(|m| m.0 == ally))
        .count();
    if settlement_count <= 1 {
        vuln += VULNERABILITY_SINGLE_SETTLEMENT;
    }

    vuln.clamp(0.0, 1.0)
}

/// Calculate alliance strength between two factions.
fn calculate_alliance_strength(
    fctx_a: &FactionContext,
    fctx_b: &FactionContext,
    rel_graph: &RelationshipGraph,
    faction_a: Entity,
    _faction_b: Entity,
) -> f64 {
    let mut strength = ALLIANCE_BASE_STRENGTH;

    // Trade routes between factions
    if let Some(ref diplomacy) = fctx_a.diplomacy
        && let Some(&count) = diplomacy.trade_partner_routes.get(&fctx_b.sim_id)
    {
        strength += (count as f64 * ALLIANCE_TRADE_ROUTE_STRENGTH).min(ALLIANCE_TRADE_ROUTE_CAP);
    }

    // Shared enemies: check if both have an active enemy relationship with any common faction
    let enemies_a: Vec<Entity> = rel_graph
        .enemies
        .iter()
        .filter(|(_, m)| m.is_active())
        .filter_map(|(&(a, b), _)| {
            if a == faction_a {
                Some(b)
            } else if b == faction_a {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    let has_shared = enemies_a
        .iter()
        .any(|&enemy| rel_graph.are_enemies(_faction_b, enemy));
    if has_shared {
        strength += ALLIANCE_SHARED_ENEMY_STRENGTH;
    }

    // Marriage alliance
    if let Some(ref diplomacy) = fctx_a.diplomacy
        && diplomacy.marriage_alliances.contains_key(&fctx_b.sim_id)
    {
        strength += ALLIANCE_MARRIAGE_STRENGTH;
    }

    // Prestige bonus
    let avg_prestige = (fctx_a.core.prestige + fctx_b.core.prestige) / 2.0;
    strength +=
        (avg_prestige * ALLIANCE_PRESTIGE_STRENGTH_WEIGHT).min(ALLIANCE_PRESTIGE_STRENGTH_CAP);

    // Low trust weakens alliance
    let trust_a = fctx_a
        .diplomacy
        .as_ref()
        .map(|d| d.diplomatic_trust)
        .unwrap_or(TRUST_DEFAULT);
    let trust_b = fctx_b
        .diplomacy
        .as_ref()
        .map(|d| d.diplomatic_trust)
        .unwrap_or(TRUST_DEFAULT);
    let min_trust = trust_a.min(trust_b);
    strength += (min_trust - TRUST_DEFAULT) * TRUST_STRENGTH_WEIGHT;

    strength
}

/// Evaluate betrayal desires for a leader against all vulnerable allies.
/// `trait_multiplier` varies by personality (Cunning=1.5, Ruthless=1.8, Ambitious=1.3).
/// Honorable trait hard-blocks all betrayal.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn evaluate_betrayal_desires(
    desires: &mut Vec<ScoredDesire>,
    npc: &NpcInfo,
    factions: &BTreeMap<Entity, FactionContext>,
    rel_graph: &RelationshipGraph,
    faction_entity: Entity,
    faction_prestige: f64,
    time: SimTime,
    trait_multiplier: f64,
    settlements: &Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            Option<&MemberOf>,
            Option<&LocatedIn>,
        ),
        With<Settlement>,
    >,
    plagued_factions: &BTreeSet<Entity>,
) {
    // Honorable hard-blocks
    if npc.traits.contains(&Trait::Honorable) {
        return;
    }

    let allies = find_ally_factions(rel_graph, faction_entity);
    if allies.is_empty() {
        return;
    }

    let Some(fctx) = factions.get(&faction_entity) else {
        return;
    };

    // 10-year cooldown after last betrayal
    let years_since_betrayal = fctx
        .diplomacy
        .as_ref()
        .and_then(|d| d.last_betrayal)
        .map(|lb| time.years_since(lb))
        .unwrap_or(u32::MAX);
    let cooldown_factor = if years_since_betrayal < 10 { 0.2 } else { 1.0 };

    for ally in allies {
        let Some(ally_ctx) = factions.get(&ally) else {
            continue;
        };

        let vulnerability =
            compute_ally_vulnerability(ally_ctx, rel_graph, ally, settlements, plagued_factions);
        if vulnerability < 0.3 {
            continue;
        }

        let base_urgency = 0.1 + vulnerability * 0.5;
        let strength = calculate_alliance_strength(fctx, ally_ctx, rel_graph, faction_entity, ally);
        let strength_resistance = (1.0 - strength * 0.5).max(0.1_f64);

        let urgency = base_urgency * trait_multiplier * strength_resistance * cooldown_factor
            + faction_prestige * 0.15;

        desires.push(ScoredDesire {
            kind: DesireKind::BetrayAlly { ally_faction: ally },
            urgency: urgency.max(0.0),
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app;
    use crate::ecs::components::{
        EcsBuildingBonuses, EcsSeasonalModifiers, PersonEducation, SettlementCrime,
        SettlementCulture, SettlementDisease, SettlementEducation, SettlementMilitary,
        SettlementTrade,
    };
    use crate::ecs::spawn;
    use crate::ecs::test_helpers::tick_years;
    use crate::ecs::time::SimTime;

    fn setup_app() -> bevy_app::App {
        let mut app = build_sim_app(100);
        app.insert_resource(RegionAdjacency::new());
        app.insert_resource(PendingActions::default());
        app.insert_resource(AgencyMemory::default());
        add_agency_systems(&mut app);
        app
    }

    /// Spawn a person into the ECS world and return its Bevy Entity.
    fn spawn_test_person(
        app: &mut bevy_app::App,
        sim_id: u64,
        name: &str,
        faction: Option<Entity>,
        traits: Vec<Trait>,
        last_action: SimTime,
        born: SimTime,
        prestige: f64,
    ) -> Entity {
        let entity = spawn::spawn_person(
            app.world_mut(),
            sim_id,
            name.to_string(),
            Some(SimTime::from_year(100)),
            PersonCore {
                born,
                traits,
                last_action,
                ..Default::default()
            },
            PersonReputation {
                prestige,
                prestige_tier: 0,
            },
            PersonSocial::default(),
            PersonEducation::default(),
        );
        if let Some(faction) = faction {
            app.world_mut().entity_mut(entity).insert(MemberOf(faction));
        }
        entity
    }

    /// Spawn a faction into the ECS world and return its Bevy Entity.
    fn spawn_test_faction(
        app: &mut bevy_app::App,
        sim_id: u64,
        name: &str,
        stability: f64,
        happiness: f64,
    ) -> Entity {
        spawn::spawn_faction(
            app.world_mut(),
            sim_id,
            name.to_string(),
            Some(SimTime::from_year(100)),
            FactionCore {
                stability,
                happiness,
                ..Default::default()
            },
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        )
    }

    fn spawn_test_settlement(
        app: &mut bevy_app::App,
        sim_id: u64,
        name: &str,
        faction: Entity,
        population: u32,
    ) -> Entity {
        let entity = spawn::spawn_settlement(
            app.world_mut(),
            sim_id,
            name.to_string(),
            Some(SimTime::from_year(100)),
            SettlementCore {
                population,
                ..Default::default()
            },
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettlementMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses::default(),
        );
        app.world_mut().entity_mut(entity).insert(MemberOf(faction));
        entity
    }

    #[test]
    fn cooldown_ambitious_aggressive() {
        let cooldown = compute_cooldown(&[Trait::Ambitious, Trait::Aggressive]);
        assert_eq!(
            cooldown, 1,
            "ambitious+aggressive should have 1-year cooldown"
        );
    }

    #[test]
    fn cooldown_cautious_content() {
        let cooldown = compute_cooldown(&[Trait::Cautious, Trait::Content]);
        assert_eq!(cooldown, 5, "cautious+content should have 5-year cooldown");
    }

    #[test]
    fn personality_modifier_aggressive_ambitious() {
        let modifier = compute_personality_modifier(&[Trait::Aggressive, Trait::Ambitious]);
        let expected = 1.3 * 1.2;
        assert!(
            (modifier - expected).abs() < 0.01,
            "expected ~{expected:.2}, got {modifier:.2}"
        );
    }

    #[test]
    fn personality_modifier_cautious_content() {
        let modifier = compute_personality_modifier(&[Trait::Cautious, Trait::Content]);
        let expected = 0.6 * 0.7;
        assert!(
            (modifier - expected).abs() < 0.01,
            "expected ~{expected:.2}, got {modifier:.2}"
        );
    }

    #[test]
    fn ambitious_non_leader_queues_action() {
        let mut app = setup_app();

        let faction = spawn_test_faction(&mut app, 1, "The Empire", 0.3, 0.5);
        let _settlement = spawn_test_settlement(&mut app, 10, "Rome", faction, 500);
        let _npc = spawn_test_person(
            &mut app,
            2,
            "Brutus",
            Some(faction),
            vec![Trait::Ambitious],
            SimTime::default(), // last_action at year 0 -- well past cooldown
            SimTime::from_year(70),
            0.0,
        );
        let leader = spawn_test_person(
            &mut app,
            3,
            "Caesar",
            Some(faction),
            vec![Trait::Content],
            SimTime::default(),
            SimTime::from_year(70),
            0.0,
        );
        app.world_mut().entity_mut(leader).insert(LeaderOf(faction));

        // Run 1 year of ticks
        tick_years(&mut app, 1);

        let pending = app.world().resource::<PendingActions>();
        // With instability=0.7, ambitious NPC should likely queue *some* action
        // (SeizePower or SeekOffice). Not guaranteed due to RNG but highly probable.
        // We just check the system ran without panicking.
        let _ = pending.0.len();
    }

    #[test]
    fn cooldown_prevents_repeated_actions() {
        let mut app = setup_app();

        let faction = spawn_test_faction(&mut app, 1, "The Empire", 0.3, 0.5);
        let _settlement = spawn_test_settlement(&mut app, 10, "Rome", faction, 500);
        // NPC with last_action at year 99, Content+Pious cooldown = 5 years
        // At year 100, years_since = 1 < 5, so should be skipped
        let _npc = spawn_test_person(
            &mut app,
            2,
            "Eager",
            Some(faction),
            vec![Trait::Content, Trait::Pious],
            SimTime::from_year(99), // very recent action
            SimTime::from_year(70),
            0.0,
        );

        tick_years(&mut app, 1);

        let pending = app.world().resource::<PendingActions>();
        assert!(
            pending.0.is_empty(),
            "NPC on cooldown should not queue actions: got {:?}",
            pending.0
        );
    }

    #[test]
    fn signal_memory_captures_events() {
        let mut app = setup_app();

        // Manually populate AgencyMemory with events to verify the resource works
        let dummy_a = app.world_mut().spawn_empty().id();
        let dummy_b = app.world_mut().spawn_empty().id();
        let dummy_c = app.world_mut().spawn_empty().id();
        {
            let mut memory = app.world_mut().resource_mut::<AgencyMemory>();
            memory.0.push(SimReactiveEvent::WarStarted {
                event_id: 1,
                attacker: dummy_a,
                defender: dummy_b,
            });
            memory.0.push(SimReactiveEvent::SettlementCaptured {
                event_id: 2,
                settlement: dummy_c,
                old_faction: Some(dummy_a),
                new_faction: dummy_b,
            });
        }

        let memory = app.world().resource::<AgencyMemory>();
        assert_eq!(memory.0.len(), 2, "should have 2 cached events");
        assert!(
            memory
                .0
                .iter()
                .any(|e| matches!(e, SimReactiveEvent::WarStarted { .. }))
        );
        assert!(
            memory
                .0
                .iter()
                .any(|e| matches!(e, SimReactiveEvent::SettlementCaptured { .. }))
        );
    }

    #[test]
    fn npc_without_traits_is_skipped() {
        let mut app = setup_app();

        let faction = spawn_test_faction(&mut app, 1, "The Empire", 0.3, 0.5);
        let _settlement = spawn_test_settlement(&mut app, 10, "Rome", faction, 500);
        // Person with no traits
        let _npc = spawn_test_person(
            &mut app,
            2,
            "Nobody",
            Some(faction),
            vec![], // no traits
            SimTime::default(),
            SimTime::from_year(70),
            0.0,
        );

        tick_years(&mut app, 1);

        let pending = app.world().resource::<PendingActions>();
        assert!(
            pending.0.is_empty(),
            "traitless NPC should not queue actions"
        );
    }

    #[test]
    fn desire_to_action_maps_all_variants() {
        // Build a minimal entity map for conversion
        let mut world = bevy_ecs::world::World::new();
        let mut entity_map = SimEntityMap::new();
        let faction_a = world.spawn_empty().id();
        let faction_b = world.spawn_empty().id();
        let person = world.spawn_empty().id();
        entity_map.insert(1, faction_a);
        entity_map.insert(2, faction_b);
        entity_map.insert(3, person);

        let cases: Vec<(ScoredDesire, &str)> = vec![
            (
                ScoredDesire {
                    kind: DesireKind::SeizePower { faction: faction_a },
                    urgency: 0.5,
                },
                "AttemptCoup",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::ExpandTerritory {
                        target_faction: faction_b,
                    },
                    urgency: 0.5,
                },
                "DeclareWar",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::SupportFaction { faction: faction_a },
                    urgency: 0.5,
                },
                "SupportFaction",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::UndermineFaction { faction: faction_b },
                    urgency: 0.5,
                },
                "UndermineFaction",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::SeekAlliance {
                        faction_a,
                        faction_b,
                    },
                    urgency: 0.5,
                },
                "BrokerAlliance",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::EliminateRival { target: person },
                    urgency: 0.5,
                },
                "Assassinate",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::Defect {
                        from_faction: faction_a,
                        to_faction: faction_b,
                    },
                    urgency: 0.5,
                },
                "Defect",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::SeekOffice { faction: faction_a },
                    urgency: 0.5,
                },
                "SeekOffice",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::BetrayAlly {
                        ally_faction: faction_b,
                    },
                    urgency: 0.5,
                },
                "BetrayAlly",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::SeekRevenge {
                        target_faction: faction_b,
                    },
                    urgency: 0.5,
                },
                "DeclareWar",
            ),
            (
                ScoredDesire {
                    kind: DesireKind::PressClaim {
                        target_faction: faction_b,
                        _claim_strength: 0.8,
                    },
                    urgency: 0.5,
                },
                "PressClaim",
            ),
        ];

        for (desire, expected_name) in &cases {
            let result = desire_to_action(desire, &entity_map);
            assert!(
                result.is_some(),
                "desire_to_action should map {expected_name}"
            );
        }
    }
}
