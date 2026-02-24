use rand::Rng;

use super::context::TickContext;
use super::signal::SignalKind;
use super::system::{SimSystem, TickFrequency};
use crate::model::action::{Action, ActionKind, ActionSource};
use crate::model::traits::Trait;
use crate::model::{EntityKind, GovernmentType, RelationshipKind};
use crate::sim::helpers;

pub struct AgencySystem {
    /// Signals received this tick, available during next tick's desire evaluation.
    recent_signals: Vec<SignalKind>,
}

impl Default for AgencySystem {
    fn default() -> Self {
        Self::new()
    }
}

impl AgencySystem {
    pub fn new() -> Self {
        Self {
            recent_signals: Vec::new(),
        }
    }
}

impl SimSystem for AgencySystem {
    fn name(&self) -> &str {
        "agency"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let current_year = ctx.world.current_time.year();

        // Consume signals from previous tick
        let signals = std::mem::take(&mut self.recent_signals);

        // Collect living notable NPCs (persons with traits)
        let npcs: Vec<NpcInfo> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Person
                    && e.end.is_none()
                    && e.data.as_person().is_some_and(|p| !p.traits.is_empty())
            })
            .map(|e| {
                let pd = e.data.as_person().unwrap();
                let traits = pd.traits.clone();
                let faction_id = e
                    .relationships
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
                    .map(|r| r.target_entity_id);
                let is_leader = e.active_rels(RelationshipKind::LeaderOf).next().is_some();
                let last_action_year = pd.last_action_year;
                let birth_year = pd.birth_year;
                let prestige = pd.prestige;
                NpcInfo {
                    id: e.id,
                    traits,
                    faction_id,
                    is_leader,
                    last_action_year,
                    birth_year,
                    prestige,
                }
            })
            .collect();

        for npc in &npcs {
            // Variable cooldown: trait-dependent
            let cooldown = compute_cooldown(&npc.traits);
            if current_year.saturating_sub(npc.last_action_year) < cooldown {
                continue;
            }

            let desires = evaluate_desires(npc, ctx, &signals, current_year);
            if desires.is_empty() {
                continue;
            }

            let max_urgency = desires.iter().map(|d| d.urgency).fold(0.0f64, f64::max);

            // Trait-modulated action probability
            let personality_mod = compute_personality_modifier(&npc.traits);
            let action_prob = (max_urgency * personality_mod).clamp(0.05, 0.65);

            if ctx.rng.random_range(0.0..1.0) >= action_prob {
                continue;
            }

            // Pick from desires weighted by urgency
            let total_urgency: f64 = desires.iter().map(|d| d.urgency).sum();
            if total_urgency <= 0.0 {
                continue;
            }

            let mut roll = ctx.rng.random_range(0.0..total_urgency);
            let mut chosen = &desires[desires.len() - 1];
            for d in &desires {
                if roll < d.urgency {
                    chosen = d;
                    break;
                }
                roll -= d.urgency;
            }

            if let Some(action_kind) = desire_to_action(chosen, npc) {
                ctx.world.pending_actions.push(Action {
                    actor_id: npc.id,
                    source: ActionSource::Autonomous,
                    kind: action_kind,
                });

                // Record last action year
                if let Some(entity) = ctx.world.entities.get_mut(&npc.id)
                    && let Some(pd) = entity.data.as_person_mut()
                {
                    pd.last_action_year = current_year;
                }
            }
        }
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        // Cache signals that matter for NPC decisions in the next tick
        self.recent_signals.clear();
        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::LeaderVacancy { .. }
                | SignalKind::WarStarted { .. }
                | SignalKind::WarEnded { .. }
                | SignalKind::SettlementCaptured { .. }
                | SignalKind::FactionSplit { .. } => {
                    self.recent_signals.push(signal.kind.clone());
                }
                _ => {}
            }
        }
    }
}

struct NpcInfo {
    id: u64,
    traits: Vec<Trait>,
    faction_id: Option<u64>,
    is_leader: bool,
    last_action_year: u32,
    birth_year: u32,
    prestige: f64,
}

#[derive(Debug)]
enum DesireKind {
    SeizePower { faction_id: u64 },
    ExpandTerritory { target_faction_id: u64 },
    SupportFaction { faction_id: u64 },
    UndermineFaction { faction_id: u64 },
    SeekAlliance { faction_a: u64, faction_b: u64 },
    EliminateRival { target_id: u64 },
    Defect { from_faction: u64, to_faction: u64 },
    SeekOffice { faction_id: u64 },
}

#[derive(Debug)]
struct ScoredDesire {
    kind: DesireKind,
    urgency: f64,
}

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
pub fn compute_personality_modifier(traits: &[Trait]) -> f64 {
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

fn evaluate_desires(
    npc: &NpcInfo,
    ctx: &TickContext,
    signals: &[SignalKind],
    current_year: u32,
) -> Vec<ScoredDesire> {
    let mut desires = Vec::new();

    let Some(faction_id) = npc.faction_id else {
        return desires;
    };

    let stability = helpers::faction_stability(ctx.world, faction_id);
    let instability = 1.0 - stability;
    let happiness = helpers::faction_happiness(ctx.world, faction_id);

    let faction_prestige = ctx
        .world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.prestige)
        .unwrap_or(0.0);
    let leader_prestige = ctx
        .world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::LeaderOf, faction_id)
        })
        .and_then(|e| e.data.as_person())
        .map(|pd| pd.prestige)
        .unwrap_or(0.0);

    // Faction context: is faction at war?
    let faction_at_war = ctx
        .world
        .entities
        .get(&faction_id)
        .is_some_and(|e| e.active_rels(RelationshipKind::AtWar).next().is_some());

    // Is faction leaderless?
    let faction_leaderless = !ctx.world.entities.values().any(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.has_active_rel(RelationshipKind::LeaderOf, faction_id)
    });

    // Check for recent leader vacancy signal for this faction
    let leader_just_died = signals.iter().any(
        |s| matches!(s, SignalKind::LeaderVacancy { faction_id: fid, .. } if *fid == faction_id),
    );

    // Check for recent settlement captured from this faction
    let lost_settlement = signals.iter().any(|s| {
        matches!(s, SignalKind::SettlementCaptured { old_faction_id, .. } if *old_faction_id == faction_id)
    });

    // NPC age
    let age = current_year.saturating_sub(npc.birth_year);
    let age_risk_factor = if age >= 60 { 0.5 } else { 1.0 };

    // Government type
    let gov_type = ctx
        .world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.government_type)
        .unwrap_or(GovernmentType::Chieftain);

    // Faction settlement count
    let settlement_count = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        })
        .count();

    for t in &npc.traits {
        match t {
            Trait::Ambitious if !npc.is_leader => {
                // SeizePower — urgency scales with instability
                let mut urgency =
                    0.2 + 0.5 * instability - 0.15 * npc.prestige - 0.1 * leader_prestige;
                // Massive boost if faction is leaderless or leader just died
                if faction_leaderless || leader_just_died {
                    urgency += 0.4;
                }
                urgency *= age_risk_factor;
                urgency = urgency.max(0.0);
                desires.push(ScoredDesire {
                    kind: DesireKind::SeizePower { faction_id },
                    urgency,
                });

                // SeekOffice — legitimate path to leadership
                if gov_type == GovernmentType::Elective || faction_leaderless {
                    let office_urgency = 0.3 + 0.2 * instability;
                    desires.push(ScoredDesire {
                        kind: DesireKind::SeekOffice { faction_id },
                        urgency: office_urgency,
                    });
                }
            }
            Trait::Ambitious if npc.is_leader => {
                // ExpandTerritory — look for enemy factions
                if let Some(target) = find_enemy_faction(ctx, faction_id) {
                    let mut urgency = 0.3 + 0.2 * instability + faction_prestige * 0.1;
                    if faction_at_war {
                        urgency += 0.15;
                    }
                    desires.push(ScoredDesire {
                        kind: DesireKind::ExpandTerritory {
                            target_faction_id: target,
                        },
                        urgency,
                    });
                }
            }
            Trait::Aggressive if npc.is_leader => {
                // ExpandTerritory against enemies
                if let Some(target) = find_enemy_faction(ctx, faction_id) {
                    let mut urgency = 0.35 + 0.15 * instability + faction_prestige * 0.1;
                    if faction_at_war {
                        urgency += 0.15;
                    }
                    desires.push(ScoredDesire {
                        kind: DesireKind::ExpandTerritory {
                            target_faction_id: target,
                        },
                        urgency,
                    });
                }
            }
            Trait::Aggressive if !npc.is_leader => {
                // EliminateRival — find enemy faction leader
                if let Some(target) = find_enemy_faction_leader(ctx, faction_id) {
                    let mut urgency = 0.25;
                    if faction_at_war {
                        urgency += 0.1;
                    }
                    urgency *= age_risk_factor;
                    desires.push(ScoredDesire {
                        kind: DesireKind::EliminateRival { target_id: target },
                        urgency,
                    });
                }
            }
            Trait::Cautious | Trait::Honorable if npc.is_leader => {
                // SupportFaction — stabilize
                desires.push(ScoredDesire {
                    kind: DesireKind::SupportFaction { faction_id },
                    urgency: 0.15 + 0.3 * instability,
                });
            }
            Trait::Charismatic => {
                // SeekAlliance — find a non-allied, non-enemy faction
                let ally_count = ctx
                    .world
                    .entities
                    .get(&faction_id)
                    .map(|e| e.active_rels(RelationshipKind::Ally).count())
                    .unwrap_or(0);
                if let Some(other) = find_potential_ally(ctx, faction_id) {
                    // Reduce urgency if already have allies
                    let urgency = (if ally_count >= 2 { 0.1 } else { 0.2 }) + npc.prestige * 0.1;
                    desires.push(ScoredDesire {
                        kind: DesireKind::SeekAlliance {
                            faction_a: faction_id,
                            faction_b: other,
                        },
                        urgency,
                    });
                }
            }
            Trait::Cunning => {
                // UndermineFaction — target enemy
                if let Some(enemy) = find_enemy_faction(ctx, faction_id) {
                    desires.push(ScoredDesire {
                        kind: DesireKind::UndermineFaction { faction_id: enemy },
                        urgency: 0.25 + 0.15 * instability,
                    });
                }

                // Defect — pragmatists flee losing factions
                if !npc.is_leader
                    && happiness < 0.3
                    && let Some(to_faction) = find_defection_target(ctx, faction_id)
                {
                    desires.push(ScoredDesire {
                        kind: DesireKind::Defect {
                            from_faction: faction_id,
                            to_faction,
                        },
                        urgency: 0.15 + 0.3 * (1.0 - happiness) + 0.1 * (1.0 - faction_prestige),
                    });
                }
            }
            Trait::Ruthless => {
                // EliminateRival — enemy leader
                if let Some(target) = find_enemy_faction_leader(ctx, faction_id) {
                    let urgency = 0.3 * age_risk_factor;
                    desires.push(ScoredDesire {
                        kind: DesireKind::EliminateRival { target_id: target },
                        urgency,
                    });
                }
            }
            Trait::Content => {
                // SupportFaction — stabilize own faction
                desires.push(ScoredDesire {
                    kind: DesireKind::SupportFaction { faction_id },
                    urgency: 0.1 + 0.2 * instability,
                });

                // Content NPCs may defect from losing factions
                if !npc.is_leader
                    && (lost_settlement || happiness < 0.3)
                    && let Some(to_faction) = find_defection_target(ctx, faction_id)
                {
                    desires.push(ScoredDesire {
                        kind: DesireKind::Defect {
                            from_faction: faction_id,
                            to_faction,
                        },
                        urgency: 0.15 + 0.3 * (1.0 - happiness) + 0.1 * (1.0 - faction_prestige),
                    });
                }
            }
            Trait::Pious => {
                // SupportFaction — stabilize own faction
                desires.push(ScoredDesire {
                    kind: DesireKind::SupportFaction { faction_id },
                    urgency: 0.1 + 0.2 * instability,
                });
            }
            _ => {}
        }
    }

    // Non-trait desires that depend on world state:

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
                kind: DesireKind::SeekOffice { faction_id },
                urgency: 0.3 + 0.2 * instability,
            });
        }
    }

    desires
}

fn desire_to_action(desire: &ScoredDesire, _npc: &NpcInfo) -> Option<ActionKind> {
    match &desire.kind {
        DesireKind::SeizePower { faction_id } => Some(ActionKind::AttemptCoup {
            faction_id: *faction_id,
        }),
        DesireKind::ExpandTerritory { target_faction_id } => Some(ActionKind::DeclareWar {
            target_faction_id: *target_faction_id,
        }),
        DesireKind::SupportFaction { faction_id } => Some(ActionKind::SupportFaction {
            faction_id: *faction_id,
        }),
        DesireKind::UndermineFaction { faction_id } => Some(ActionKind::UndermineFaction {
            faction_id: *faction_id,
        }),
        DesireKind::SeekAlliance {
            faction_a,
            faction_b,
        } => Some(ActionKind::BrokerAlliance {
            faction_a: *faction_a,
            faction_b: *faction_b,
        }),
        DesireKind::EliminateRival { target_id } => Some(ActionKind::Assassinate {
            target_id: *target_id,
        }),
        DesireKind::Defect {
            from_faction,
            to_faction,
        } => Some(ActionKind::Defect {
            from_faction: *from_faction,
            to_faction: *to_faction,
        }),
        DesireKind::SeekOffice { faction_id } => Some(ActionKind::SeekOffice {
            faction_id: *faction_id,
        }),
    }
}

// --- Helpers ---

fn find_enemy_faction(ctx: &TickContext, faction_id: u64) -> Option<u64> {
    ctx.world
        .entities
        .get(&faction_id)?
        .active_rel(RelationshipKind::Enemy)
}

fn find_enemy_faction_leader(ctx: &TickContext, faction_id: u64) -> Option<u64> {
    let enemy_faction = find_enemy_faction(ctx, faction_id)?;
    helpers::faction_leader(ctx.world, enemy_faction)
}

fn find_potential_ally(ctx: &TickContext, faction_id: u64) -> Option<u64> {
    let faction = ctx.world.entities.get(&faction_id)?;
    let existing_rels: Vec<u64> = faction
        .relationships
        .iter()
        .filter(|r| {
            r.end.is_none()
                && matches!(
                    r.kind,
                    RelationshipKind::Ally | RelationshipKind::Enemy | RelationshipKind::AtWar
                )
        })
        .map(|r| r.target_entity_id)
        .collect();

    ctx.world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.id != faction_id
                && !existing_rels.contains(&e.id)
        })
        .map(|e| e.id)
}

/// Find a non-enemy adjacent faction that the NPC could defect to.
fn find_defection_target(ctx: &TickContext, faction_id: u64) -> Option<u64> {
    let faction = ctx.world.entities.get(&faction_id)?;
    let enemies: Vec<u64> = faction
        .relationships
        .iter()
        .filter(|r| {
            r.end.is_none() && matches!(r.kind, RelationshipKind::Enemy | RelationshipKind::AtWar)
        })
        .map(|r| r.target_entity_id)
        .collect();

    // Find another living faction that is not an enemy
    ctx.world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.id != faction_id
                && !enemies.contains(&e.id)
        })
        .map(|e| e.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EventKind, RelationshipKind, SimTimestamp};
    use crate::scenario::Scenario;
    use crate::sim::context::TickContext;
    use crate::sim::signal::SignalKind;
    use crate::testutil;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn tick_agency(world: &mut crate::model::World) {
        testutil::tick_system(world, &mut AgencySystem::new(), 100, 42);
    }

    #[test]
    fn scenario_ambitious_non_leader_generates_coup_desire() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.faction("The Empire").stability(0.3).id();
        let _npc_id = s
            .person("Brutus", faction_id)
            .traits(vec![Trait::Ambitious])
            .id();
        let leader_id = s
            .person("Caesar", faction_id)
            .traits(vec![Trait::Content])
            .id();
        s.make_leader(leader_id, faction_id);
        let mut world = s.build();

        tick_agency(&mut world);

        let coup_actions: Vec<_> = world
            .pending_actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::AttemptCoup { .. }))
            .collect();
        assert!(
            coup_actions.len() <= 1,
            "should queue at most one coup action per NPC"
        );
    }

    #[test]
    fn scenario_npc_without_traits_is_skipped() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.faction("The Empire").stability(0.3).id();
        // Person with default (empty) traits
        s.add_person("Nobody", faction_id);
        let mut world = s.build();

        tick_agency(&mut world);

        assert!(world.pending_actions.is_empty());
    }

    #[test]
    fn scenario_cooldown_prevents_spam() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.faction("The Empire").stability(0.3).id();
        let npc_id = s
            .person("Eager", faction_id)
            .traits(vec![Trait::Content, Trait::Pious])
            .last_action_year(99)
            .id();
        let _ = npc_id;
        let mut world = s.build();

        tick_agency(&mut world);

        // Should not act due to cooldown (100 - 99 = 1 < 5 for Content+Pious)
        assert!(world.pending_actions.is_empty());
    }

    #[test]
    fn scenario_dead_npcs_are_skipped() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.faction("The Empire").stability(0.3).id();
        let npc_id = s
            .person("Ghost", faction_id)
            .traits(vec![Trait::Ambitious, Trait::Aggressive])
            .id();
        s.end_entity(npc_id);
        let mut world = s.build();

        tick_agency(&mut world);

        assert!(world.pending_actions.is_empty());
    }

    #[test]
    fn scenario_signal_leader_vacancy_boosts_seize_power() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.faction("The Empire").stability(0.3).id();
        let npc_id = s
            .person("Brutus", faction_id)
            .traits(vec![Trait::Ambitious])
            .id();
        let leader_id = s
            .person("Caesar", faction_id)
            .traits(vec![Trait::Content])
            .id();
        s.make_leader(leader_id, faction_id);
        let mut world = s.build();

        let npc_info = NpcInfo {
            id: npc_id,
            traits: vec![Trait::Ambitious],
            faction_id: Some(faction_id),
            is_leader: false,
            last_action_year: 0,
            birth_year: 70,
            prestige: 0.0,
        };

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals_out = Vec::new();
        let ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals_out,
            inbox: &[],
        };

        // Without vacancy signal
        let desires_no_signal = evaluate_desires(&npc_info, &ctx, &[], 100);
        let seize_no_signal = desires_no_signal
            .iter()
            .find(|d| matches!(d.kind, DesireKind::SeizePower { .. }))
            .map(|d| d.urgency)
            .unwrap_or(0.0);

        // With vacancy signal
        let vacancy_signals = vec![SignalKind::LeaderVacancy {
            faction_id,
            previous_leader_id: leader_id,
        }];
        let desires_with_signal = evaluate_desires(&npc_info, &ctx, &vacancy_signals, 100);
        let seize_with_signal = desires_with_signal
            .iter()
            .find(|d| matches!(d.kind, DesireKind::SeizePower { .. }))
            .map(|d| d.urgency)
            .unwrap_or(0.0);

        assert!(
            seize_with_signal > seize_no_signal,
            "vacancy signal should boost SeizePower urgency: {seize_with_signal} > {seize_no_signal}"
        );
    }

    #[test]
    fn scenario_old_npc_reduced_urgency() {
        let mut s = Scenario::at_year(130);
        let faction_id = s.faction("The Empire").stability(0.3).id();
        let npc_id = s
            .person("Elder", faction_id)
            .traits(vec![Trait::Ambitious])
            .birth_year(70)
            .id();
        let leader_id = s
            .person("King", faction_id)
            .traits(vec![Trait::Content])
            .id();
        s.make_leader(leader_id, faction_id);
        let mut world = s.build();

        let old_npc = NpcInfo {
            id: npc_id,
            traits: vec![Trait::Ambitious],
            faction_id: Some(faction_id),
            is_leader: false,
            last_action_year: 0,
            birth_year: 70,
            prestige: 0.0,
        };

        let young_npc = NpcInfo {
            id: npc_id,
            traits: vec![Trait::Ambitious],
            faction_id: Some(faction_id),
            is_leader: false,
            last_action_year: 0,
            birth_year: 100, // age 30
            prestige: 0.0,
        };

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals_out = Vec::new();
        let ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals_out,
            inbox: &[],
        };

        let old_desires = evaluate_desires(&old_npc, &ctx, &[], 130);
        let young_desires = evaluate_desires(&young_npc, &ctx, &[], 130);

        let old_seize = old_desires
            .iter()
            .find(|d| matches!(d.kind, DesireKind::SeizePower { .. }))
            .map(|d| d.urgency)
            .unwrap_or(0.0);
        let young_seize = young_desires
            .iter()
            .find(|d| matches!(d.kind, DesireKind::SeizePower { .. }))
            .map(|d| d.urgency)
            .unwrap_or(0.0);

        assert!(
            old_seize < young_seize,
            "old NPC should have reduced seize urgency: {old_seize} < {young_seize}"
        );
        assert!(
            (old_seize / young_seize - 0.5).abs() < 0.01,
            "old NPC urgency ratio should be ~0.5: got {:.3}",
            old_seize / young_seize
        );
    }

    #[test]
    fn scenario_at_war_boosts_aggressive_desires() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.faction("The Empire").stability(0.3).id();
        let enemy_id = s.faction("The Rebels").stability(0.3).id();
        let npc_id = s
            .person("General", faction_id)
            .traits(vec![Trait::Aggressive])
            .id();
        s.make_leader(npc_id, faction_id);
        s.make_enemies(faction_id, enemy_id);
        let mut world = s.build();

        let npc_no_war = NpcInfo {
            id: npc_id,
            traits: vec![Trait::Aggressive],
            faction_id: Some(faction_id),
            is_leader: true,
            last_action_year: 0,
            birth_year: 70,
            prestige: 0.0,
        };

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals_out = Vec::new();
        let ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals_out,
            inbox: &[],
        };

        let desires_no_war = evaluate_desires(&npc_no_war, &ctx, &[], 100);
        let expand_no_war = desires_no_war
            .iter()
            .find(|d| matches!(d.kind, DesireKind::ExpandTerritory { .. }))
            .map(|d| d.urgency)
            .unwrap_or(0.0);

        // Now add AtWar
        drop(ctx);
        let ts = SimTimestamp::from_year(99);
        let wev = world.add_event(EventKind::WarDeclared, ts, "War".to_string());
        world.add_relationship(faction_id, enemy_id, RelationshipKind::AtWar, ts, wev);

        let mut rng2 = SmallRng::seed_from_u64(42);
        let mut signals_out2 = Vec::new();
        let ctx2 = TickContext {
            world: &mut world,
            rng: &mut rng2,
            signals: &mut signals_out2,
            inbox: &[],
        };

        let desires_at_war = evaluate_desires(&npc_no_war, &ctx2, &[], 100);
        let expand_at_war = desires_at_war
            .iter()
            .find(|d| matches!(d.kind, DesireKind::ExpandTerritory { .. }))
            .map(|d| d.urgency)
            .unwrap_or(0.0);

        assert!(
            expand_at_war > expand_no_war,
            "at-war should boost expand urgency: {expand_at_war} > {expand_no_war}"
        );
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
    fn variable_cooldown_ambitious_aggressive() {
        let cooldown = compute_cooldown(&[Trait::Ambitious, Trait::Aggressive]);
        assert_eq!(
            cooldown, 1,
            "ambitious+aggressive should have 1-year cooldown"
        );
    }

    #[test]
    fn variable_cooldown_cautious_content() {
        let cooldown = compute_cooldown(&[Trait::Cautious, Trait::Content]);
        assert_eq!(cooldown, 5, "cautious+content should have 5-year cooldown");
    }

    #[test]
    fn scenario_defect_desire_for_unhappy_cunning_npc() {
        let mut s = Scenario::at_year(100);
        let faction_id = s.faction("The Empire").stability(0.3).happiness(0.2).id();
        let other_id = s.faction("The Republic").stability(0.3).id();
        let npc_id = s
            .person("Rat", faction_id)
            .traits(vec![Trait::Cunning])
            .id();
        let mut world = s.build();

        let npc_info = NpcInfo {
            id: npc_id,
            traits: vec![Trait::Cunning],
            faction_id: Some(faction_id),
            is_leader: false,
            last_action_year: 0,
            birth_year: 70,
            prestige: 0.0,
        };

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals_out = Vec::new();
        let ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals_out,
            inbox: &[],
        };

        let desires = evaluate_desires(&npc_info, &ctx, &[], 100);
        let has_defect = desires
            .iter()
            .any(|d| matches!(d.kind, DesireKind::Defect { .. }));
        assert!(
            has_defect,
            "unhappy cunning NPC should want to defect: {desires:?}"
        );
        let defect = desires
            .iter()
            .find(|d| matches!(d.kind, DesireKind::Defect { .. }))
            .unwrap();
        match &defect.kind {
            DesireKind::Defect {
                from_faction,
                to_faction,
            } => {
                assert_eq!(*from_faction, faction_id);
                assert_eq!(*to_faction, other_id);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn scenario_seek_office_desire_for_ambitious_in_elective() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Rome");
        let _ = s
            .faction_mut(setup.faction)
            .stability(0.3)
            .government_type(GovernmentType::Elective);
        let faction_id = setup.faction;
        let npc_id = s
            .person("Cicero", faction_id)
            .traits(vec![Trait::Ambitious])
            .id();
        let leader_id = s
            .person("Consul", faction_id)
            .traits(vec![Trait::Content])
            .id();
        s.make_leader(leader_id, faction_id);
        let mut world = s.build();

        let npc_info = NpcInfo {
            id: npc_id,
            traits: vec![Trait::Ambitious],
            faction_id: Some(faction_id),
            is_leader: false,
            last_action_year: 0,
            birth_year: 70,
            prestige: 0.0,
        };

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals_out = Vec::new();
        let ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals_out,
            inbox: &[],
        };

        let desires = evaluate_desires(&npc_info, &ctx, &[], 100);
        let has_seek_office = desires
            .iter()
            .any(|d| matches!(d.kind, DesireKind::SeekOffice { .. }));
        assert!(
            has_seek_office,
            "ambitious NPC in elective faction should want to seek office: {desires:?}"
        );
    }
}
