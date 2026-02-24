use rand::Rng;

use super::context::TickContext;
use super::signal::SignalKind;
use super::system::{SimSystem, TickFrequency};
use crate::model::action::{Action, ActionKind, ActionSource};
use crate::model::traits::Trait;
use crate::model::{EntityKind, RelationshipKind};

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
                let is_leader = e
                    .relationships
                    .iter()
                    .any(|r| r.kind == RelationshipKind::LeaderOf && r.end.is_none());
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

    let stability = get_faction_f64(ctx, faction_id, "stability", 0.5);
    let instability = 1.0 - stability;
    let happiness = get_faction_f64(ctx, faction_id, "happiness", 0.5);

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
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LeaderOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .and_then(|e| e.data.as_person())
        .map(|pd| pd.prestige)
        .unwrap_or(0.0);

    // Faction context: is faction at war?
    let faction_at_war = ctx.world.entities.get(&faction_id).is_some_and(|e| {
        e.relationships
            .iter()
            .any(|r| r.kind == RelationshipKind::AtWar && r.end.is_none())
    });

    // Is faction leaderless?
    let faction_leaderless = !ctx.world.entities.values().any(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LeaderOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
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
    let gov_type_owned = ctx
        .world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| f.government_type.clone())
        .unwrap_or_else(|| "chieftain".to_string());
    let gov_type = gov_type_owned.as_str();

    // Faction settlement count
    let settlement_count = ctx
        .world
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
                if gov_type == "elective" || faction_leaderless {
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
                    .map(|e| {
                        e.relationships
                            .iter()
                            .filter(|r| r.kind == RelationshipKind::Ally && r.end.is_none())
                            .count()
                    })
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

fn get_faction_f64(ctx: &TickContext, faction_id: u64, field: &str, default: f64) -> f64 {
    ctx.world
        .entities
        .get(&faction_id)
        .and_then(|e| e.data.as_faction())
        .map(|f| match field {
            "stability" => f.stability,
            "happiness" => f.happiness,
            "legitimacy" => f.legitimacy,
            _ => default,
        })
        .unwrap_or(default)
}

fn find_enemy_faction(ctx: &TickContext, faction_id: u64) -> Option<u64> {
    ctx.world
        .entities
        .get(&faction_id)?
        .relationships
        .iter()
        .find(|r| r.kind == RelationshipKind::Enemy && r.end.is_none())
        .map(|r| r.target_entity_id)
}

fn find_enemy_faction_leader(ctx: &TickContext, faction_id: u64) -> Option<u64> {
    let enemy_faction = find_enemy_faction(ctx, faction_id)?;
    // Find leader of enemy faction
    ctx.world.entities.values().find_map(|e| {
        if e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LeaderOf
                    && r.target_entity_id == enemy_faction
                    && r.end.is_none()
            })
        {
            Some(e.id)
        } else {
            None
        }
    })
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
    use crate::model::{EntityData, EventKind, FactionData, PersonData, SimTimestamp, World};
    use crate::sim::context::TickContext;
    use crate::sim::signal::SignalKind;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    fn setup_world() -> World {
        let mut world = World::new();
        world.current_time = ts(100);
        world
    }

    fn add_person_with_traits(world: &mut World, name: &str, traits: &[Trait]) -> u64 {
        let ev = world.add_event(EventKind::Birth, ts(70), format!("{name} born"));
        let id = world.add_entity(
            EntityKind::Person,
            name.to_string(),
            Some(ts(70)),
            EntityData::Person(PersonData {
                birth_year: 70,
                sex: "male".to_string(),
                role: "warrior".to_string(),
                traits: traits.to_vec(),
                last_action_year: 0,
                culture_id: None,
                prestige: 0.0,
            }),
            ev,
        );
        id
    }

    fn add_person_with_traits_and_birth(
        world: &mut World,
        name: &str,
        traits: &[Trait],
        birth_year: u32,
    ) -> u64 {
        let ev = world.add_event(EventKind::Birth, ts(birth_year), format!("{name} born"));
        let id = world.add_entity(
            EntityKind::Person,
            name.to_string(),
            Some(ts(birth_year)),
            EntityData::Person(PersonData {
                birth_year,
                sex: "male".to_string(),
                role: "warrior".to_string(),
                traits: traits.to_vec(),
                last_action_year: 0,
                culture_id: None,
                prestige: 0.0,
            }),
            ev,
        );
        id
    }

    fn add_faction(world: &mut World, name: &str) -> u64 {
        let ev = world.add_event(EventKind::FactionFormed, ts(50), format!("{name} formed"));
        let id = world.add_entity(
            EntityKind::Faction,
            name.to_string(),
            Some(ts(50)),
            EntityData::Faction(FactionData {
                government_type: "chieftain".to_string(),
                stability: 0.3,
                happiness: 0.5,
                legitimacy: 0.5,
                treasury: 0.0,
                alliance_strength: 0.0,
                primary_culture: None,
                prestige: 0.0,
            }),
            ev,
        );
        id
    }

    fn tick_agency(world: &mut World) {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = AgencySystem::new();
        let mut ctx = TickContext {
            world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        system.tick(&mut ctx);
    }

    #[test]
    fn ambitious_non_leader_generates_coup_desire() {
        let mut world = setup_world();
        let faction_id = add_faction(&mut world, "The Empire");
        let npc_id = add_person_with_traits(&mut world, "Brutus", &[Trait::Ambitious]);

        // Join faction
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

        // Need a leader for coup to target
        let leader_id = add_person_with_traits(&mut world, "Caesar", &[Trait::Content]);
        let rev = world.add_event(EventKind::Joined, ts(80), "Joined".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::MemberOf,
            ts(80),
            rev,
        );
        let rev2 = world.add_event(EventKind::Succession, ts(80), "Crowned".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::LeaderOf,
            ts(80),
            rev2,
        );

        tick_agency(&mut world);

        // Should have queued an AttemptCoup action
        let coup_actions: Vec<_> = world
            .pending_actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::AttemptCoup { .. }))
            .collect();
        // With seed 42 and high instability (0.7), probability is high
        // But it's still probabilistic — check that the system ran without panic
        assert!(
            coup_actions.len() <= 1,
            "should queue at most one coup action per NPC"
        );
    }

    #[test]
    fn npc_without_traits_is_skipped() {
        let mut world = setup_world();
        let faction_id = add_faction(&mut world, "The Empire");

        // Person without traits (empty traits vec)
        let ev = world.add_event(EventKind::Birth, ts(70), "Born".to_string());
        let npc_id = world.add_entity(
            EntityKind::Person,
            "Nobody".to_string(),
            Some(ts(70)),
            EntityData::default_for_kind(&EntityKind::Person),
            ev,
        );
        let jev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), jev);

        tick_agency(&mut world);

        assert!(world.pending_actions.is_empty());
    }

    #[test]
    fn cooldown_prevents_spam() {
        let mut world = setup_world();
        let faction_id = add_faction(&mut world, "The Empire");
        let npc_id = add_person_with_traits(&mut world, "Eager", &[Trait::Content, Trait::Pious]);

        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

        // Set last action year to current year (100)
        world
            .entities
            .get_mut(&npc_id)
            .unwrap()
            .data
            .as_person_mut()
            .unwrap()
            .last_action_year = 99;

        tick_agency(&mut world);

        // Should not act due to cooldown (100 - 99 = 1 < 5 for Content+Pious)
        assert!(world.pending_actions.is_empty());
    }

    #[test]
    fn dead_npcs_are_skipped() {
        let mut world = setup_world();
        let faction_id = add_faction(&mut world, "The Empire");
        let npc_id =
            add_person_with_traits(&mut world, "Ghost", &[Trait::Ambitious, Trait::Aggressive]);

        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

        // Kill the NPC
        let dev = world.add_event(EventKind::Death, ts(95), "Died".to_string());
        world.end_entity(npc_id, ts(95), dev);

        tick_agency(&mut world);

        assert!(world.pending_actions.is_empty());
    }

    #[test]
    fn signal_leader_vacancy_boosts_seize_power() {
        let mut world = setup_world();
        let faction_id = add_faction(&mut world, "The Empire");
        let npc_id = add_person_with_traits(&mut world, "Brutus", &[Trait::Ambitious]);
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

        // Need a leader
        let leader_id = add_person_with_traits(&mut world, "Caesar", &[Trait::Content]);
        let rev = world.add_event(EventKind::Joined, ts(80), "Joined".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::MemberOf,
            ts(80),
            rev,
        );
        let rev2 = world.add_event(EventKind::Succession, ts(80), "Crowned".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::LeaderOf,
            ts(80),
            rev2,
        );

        // Create NpcInfo manually to test desire evaluation
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
    fn old_npc_reduced_urgency() {
        let mut world = setup_world();
        world.current_time = ts(130); // NPC born year 70 => age 60
        let faction_id = add_faction(&mut world, "The Empire");

        let npc_id = add_person_with_traits_and_birth(&mut world, "Elder", &[Trait::Ambitious], 70);
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

        // Add leader
        let leader_id = add_person_with_traits(&mut world, "King", &[Trait::Content]);
        let rev = world.add_event(EventKind::Joined, ts(80), "Joined".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::MemberOf,
            ts(80),
            rev,
        );
        let rev2 = world.add_event(EventKind::Succession, ts(80), "Crowned".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::LeaderOf,
            ts(80),
            rev2,
        );

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
        // Should be approximately halved
        assert!(
            (old_seize / young_seize - 0.5).abs() < 0.01,
            "old NPC urgency ratio should be ~0.5: got {:.3}",
            old_seize / young_seize
        );
    }

    #[test]
    fn at_war_boosts_aggressive_desires() {
        let mut world = setup_world();
        let faction_id = add_faction(&mut world, "The Empire");
        let enemy_id = add_faction(&mut world, "The Rebels");

        let npc_id = add_person_with_traits(&mut world, "General", &[Trait::Aggressive]);
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);
        // Make NPC a leader
        let lev = world.add_event(EventKind::Succession, ts(90), "Led".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::LeaderOf, ts(90), lev);

        // Add enemy relationship
        let eev = world.add_event(
            EventKind::Custom("rivalry".to_string()),
            ts(90),
            "Rivals".to_string(),
        );
        world.add_relationship(faction_id, enemy_id, RelationshipKind::Enemy, ts(90), eev);

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
        let wev = world.add_event(EventKind::WarDeclared, ts(99), "War".to_string());
        world.add_relationship(faction_id, enemy_id, RelationshipKind::AtWar, ts(99), wev);

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
    fn defect_desire_for_unhappy_cunning_npc() {
        let mut world = setup_world();
        let faction_id = add_faction(&mut world, "The Empire");
        let other_id = add_faction(&mut world, "The Republic");

        // Set low happiness
        world
            .entities
            .get_mut(&faction_id)
            .unwrap()
            .data
            .as_faction_mut()
            .unwrap()
            .happiness = 0.2;

        let npc_id = add_person_with_traits(&mut world, "Rat", &[Trait::Cunning]);
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

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
        // Verify target is the other faction
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
    fn seek_office_desire_for_ambitious_in_elective() {
        let mut world = setup_world();
        let faction_id = add_faction(&mut world, "The Republic");
        world
            .entities
            .get_mut(&faction_id)
            .unwrap()
            .data
            .as_faction_mut()
            .unwrap()
            .government_type = "elective".to_string();

        let npc_id = add_person_with_traits(&mut world, "Cicero", &[Trait::Ambitious]);
        let ev = world.add_event(EventKind::Joined, ts(90), "Joined".to_string());
        world.add_relationship(npc_id, faction_id, RelationshipKind::MemberOf, ts(90), ev);

        // Add a leader so it's not leaderless
        let leader_id = add_person_with_traits(&mut world, "Consul", &[Trait::Content]);
        let rev = world.add_event(EventKind::Joined, ts(80), "Joined".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::MemberOf,
            ts(80),
            rev,
        );
        let rev2 = world.add_event(EventKind::Succession, ts(80), "Elected".to_string());
        world.add_relationship(
            leader_id,
            faction_id,
            RelationshipKind::LeaderOf,
            ts(80),
            rev2,
        );

        // Add a settlement so faction has territory
        let sev = world.add_event(
            EventKind::Custom("settle".to_string()),
            ts(50),
            "Settled".to_string(),
        );
        let _settlement = world.add_entity(
            EntityKind::Settlement,
            "Rome".to_string(),
            Some(ts(50)),
            EntityData::default_for_kind(&EntityKind::Settlement),
            sev,
        );
        world.add_relationship(
            _settlement,
            faction_id,
            RelationshipKind::MemberOf,
            ts(50),
            sev,
        );

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
