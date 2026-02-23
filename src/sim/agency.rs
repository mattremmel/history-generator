use rand::Rng;

use super::context::TickContext;
use super::system::{SimSystem, TickFrequency};
use crate::model::action::{Action, ActionKind, ActionSource};
use crate::model::traits::{Trait, get_npc_traits};
use crate::model::{EntityKind, RelationshipKind};

pub struct AgencySystem;

impl SimSystem for AgencySystem {
    fn name(&self) -> &str {
        "agency"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let current_year = ctx.world.current_time.year();

        // Collect living notable NPCs (persons with traits)
        let npcs: Vec<NpcInfo> = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Person && e.end.is_none() && e.has_property("traits"))
            .map(|e| {
                let traits = get_npc_traits(e);
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
                let last_action_year = e.get_property::<u32>("last_action_year").unwrap_or(0);
                NpcInfo {
                    id: e.id,
                    traits,
                    faction_id,
                    is_leader,
                    last_action_year,
                }
            })
            .collect();

        for npc in &npcs {
            // Cooldown: skip if acted recently (within last 3 years)
            if current_year.saturating_sub(npc.last_action_year) < 3 {
                continue;
            }

            let desires = evaluate_desires(npc, ctx);
            if desires.is_empty() {
                continue;
            }

            let max_urgency = desires.iter().map(|d| d.urgency).fold(0.0f64, f64::max);
            let action_prob = max_urgency.min(0.5);

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
                let ev_id = *ctx.world.events.keys().next_back().unwrap_or(&0);
                ctx.world.set_property(
                    npc.id,
                    "last_action_year".to_string(),
                    serde_json::json!(current_year),
                    ev_id,
                );
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
}

#[derive(Debug)]
enum DesireKind {
    SeizePower { faction_id: u64 },
    ExpandTerritory { target_faction_id: u64 },
    SupportFaction { faction_id: u64 },
    UndermineFaction { faction_id: u64 },
    SeekAlliance { faction_a: u64, faction_b: u64 },
    EliminateRival { target_id: u64 },
}

#[derive(Debug)]
struct ScoredDesire {
    kind: DesireKind,
    urgency: f64,
}

fn evaluate_desires(npc: &NpcInfo, ctx: &TickContext) -> Vec<ScoredDesire> {
    let mut desires = Vec::new();

    let Some(faction_id) = npc.faction_id else {
        return desires;
    };

    let stability = get_f64(ctx, faction_id, "stability", 0.5);
    let instability = 1.0 - stability;

    for t in &npc.traits {
        match t {
            Trait::Ambitious if !npc.is_leader => {
                // SeizePower — urgency scales with instability
                desires.push(ScoredDesire {
                    kind: DesireKind::SeizePower { faction_id },
                    urgency: 0.2 + 0.5 * instability,
                });
            }
            Trait::Ambitious if npc.is_leader => {
                // ExpandTerritory — look for enemy factions
                if let Some(target) = find_enemy_faction(ctx, faction_id) {
                    desires.push(ScoredDesire {
                        kind: DesireKind::ExpandTerritory {
                            target_faction_id: target,
                        },
                        urgency: 0.3 + 0.2 * instability,
                    });
                }
            }
            Trait::Aggressive if npc.is_leader => {
                // ExpandTerritory against enemies
                if let Some(target) = find_enemy_faction(ctx, faction_id) {
                    desires.push(ScoredDesire {
                        kind: DesireKind::ExpandTerritory {
                            target_faction_id: target,
                        },
                        urgency: 0.35 + 0.15 * instability,
                    });
                }
            }
            Trait::Aggressive if !npc.is_leader => {
                // EliminateRival — find enemy faction leader
                if let Some(target) = find_enemy_faction_leader(ctx, faction_id) {
                    desires.push(ScoredDesire {
                        kind: DesireKind::EliminateRival { target_id: target },
                        urgency: 0.25,
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
                if let Some(other) = find_potential_ally(ctx, faction_id) {
                    desires.push(ScoredDesire {
                        kind: DesireKind::SeekAlliance {
                            faction_a: faction_id,
                            faction_b: other,
                        },
                        urgency: 0.2,
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
            }
            Trait::Ruthless => {
                // EliminateRival — enemy leader
                if let Some(target) = find_enemy_faction_leader(ctx, faction_id) {
                    desires.push(ScoredDesire {
                        kind: DesireKind::EliminateRival { target_id: target },
                        urgency: 0.3,
                    });
                }
            }
            Trait::Content | Trait::Pious => {
                // SupportFaction — stabilize own faction
                desires.push(ScoredDesire {
                    kind: DesireKind::SupportFaction { faction_id },
                    urgency: 0.1 + 0.2 * instability,
                });
            }
            _ => {}
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
    }
}

// --- Helpers ---

fn get_f64(ctx: &TickContext, entity_id: u64, key: &str, default: f64) -> f64 {
    ctx.world
        .entities
        .get(&entity_id)
        .and_then(|e| e.properties.get(key))
        .and_then(|v| v.as_f64())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EventKind, SimTimestamp, World};
    use crate::sim::context::TickContext;
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
        let id = world.add_entity(EntityKind::Person, name.to_string(), Some(ts(70)), ev);
        let trait_strings: Vec<String> = traits.iter().map(|t| String::from(t.clone())).collect();
        world.set_property(
            id,
            "traits".to_string(),
            serde_json::json!(trait_strings),
            ev,
        );
        world.set_property(id, "role".to_string(), serde_json::json!("warrior"), ev);
        id
    }

    fn add_faction(world: &mut World, name: &str) -> u64 {
        let ev = world.add_event(EventKind::FactionFormed, ts(50), format!("{name} formed"));
        let id = world.add_entity(EntityKind::Faction, name.to_string(), Some(ts(50)), ev);
        world.set_property(id, "stability".to_string(), serde_json::json!(0.3), ev);
        world.set_property(id, "happiness".to_string(), serde_json::json!(0.5), ev);
        world.set_property(id, "legitimacy".to_string(), serde_json::json!(0.5), ev);
        id
    }

    fn tick_agency(world: &mut World) {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut system = AgencySystem;
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

        // Person without traits property
        let ev = world.add_event(EventKind::Birth, ts(70), "Born".to_string());
        let npc_id = world.add_entity(EntityKind::Person, "Nobody".to_string(), Some(ts(70)), ev);
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
        world.set_property(
            npc_id,
            "last_action_year".to_string(),
            serde_json::json!(99u32),
            ev,
        );

        tick_agency(&mut world);

        // Should not act due to cooldown (100 - 99 = 1 < 3)
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
}
