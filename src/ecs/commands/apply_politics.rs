use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::common::SimEntity;
use crate::ecs::components::{FactionCore, FactionDiplomacy, FactionMilitary, Person};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LeaderOf, LeaderOfSources, MemberOf, RelationshipGraph, RelationshipMeta};
use crate::ecs::spawn;
use crate::model::effect::StateChange;
use crate::model::relationship::RelationshipKind;

use super::applicator::ApplyCtx;

/// Succeed leader: remove old LeaderOf, insert new LeaderOf, apply stability hit.
pub(crate) fn apply_succeed_leader(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    faction: Entity,
    new_leader: Entity,
) {
    let faction_sim = ctx.entity_map.get_sim(faction).unwrap_or(0);
    let new_leader_sim = ctx.entity_map.get_sim(new_leader).unwrap_or(0);

    // Find and remove old leader's LeaderOf component
    let old_leader = world
        .get::<LeaderOfSources>(faction)
        .and_then(|sources| sources.iter().next().copied());

    if let Some(old) = old_leader {
        world.entity_mut(old).remove::<LeaderOf>();
        let old_sim = ctx.entity_map.get_sim(old).unwrap_or(0);
        ctx.record_effect(
            event_id,
            old,
            StateChange::RelationshipEnded {
                target_entity_id: faction_sim,
                kind: RelationshipKind::LeaderOf,
            },
        );
        let _ = old_sim;
    }

    // Insert new leader
    world.entity_mut(new_leader).insert(LeaderOf(faction));
    ctx.record_effect(
        event_id,
        new_leader,
        StateChange::RelationshipStarted {
            target_entity_id: faction_sim,
            kind: RelationshipKind::LeaderOf,
        },
    );

    // Apply succession stability hit
    if let Some(mut core) = world.get_mut::<FactionCore>(faction) {
        let old_stability = core.stability;
        let hit = 0.12 * (1.0 - core.prestige * 0.5);
        core.stability = (core.stability - hit).clamp(0.0, 1.0);
        ctx.record_effect(
            event_id,
            faction,
            StateChange::PropertyChanged {
                field: "stability".to_string(),
                old_value: serde_json::json!(old_stability),
                new_value: serde_json::json!(core.stability),
            },
        );
    }

    let _ = new_leader_sim;
}

/// Attempt coup: swap leader if successful, apply stability/legitimacy changes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_attempt_coup(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    faction: Entity,
    instigator: Entity,
    succeeded: bool,
    execute_instigator: bool,
) {
    let faction_sim = ctx.entity_map.get_sim(faction).unwrap_or(0);
    let instigator_sim = ctx.entity_map.get_sim(instigator).unwrap_or(0);

    if succeeded {
        // Remove old leader
        let old_leader = world
            .get::<LeaderOfSources>(faction)
            .and_then(|sources| sources.iter().next().copied());

        if let Some(old) = old_leader {
            world.entity_mut(old).remove::<LeaderOf>();
            ctx.record_effect(
                event_id,
                old,
                StateChange::RelationshipEnded {
                    target_entity_id: faction_sim,
                    kind: RelationshipKind::LeaderOf,
                },
            );
        }

        // Install instigator as new leader
        world.entity_mut(instigator).insert(LeaderOf(faction));
        ctx.record_effect(
            event_id,
            instigator,
            StateChange::RelationshipStarted {
                target_entity_id: faction_sim,
                kind: RelationshipKind::LeaderOf,
            },
        );

        // Update faction state after successful coup
        if let Some(mut core) = world.get_mut::<FactionCore>(faction) {
            let old_stability = core.stability;
            let old_legitimacy = core.legitimacy;
            let old_happiness = core.happiness;

            // Stability: base 0.3 + unhappiness bonus + illegitimacy bonus, clamped 0.2-0.65
            let unhappiness_bonus = (1.0 - old_happiness) * 0.15;
            let illegitimacy_bonus = (1.0 - old_legitimacy) * 0.10;
            core.stability = (0.3 + unhappiness_bonus + illegitimacy_bonus).clamp(0.2, 0.65);

            // Legitimacy depends on whether it was a "liberation" or "power grab"
            if old_happiness < 0.35 {
                // Liberation coup — higher legitimacy
                core.legitimacy = (0.4 + 0.3 * (1.0 - old_happiness)).clamp(0.0, 1.0);
            } else {
                // Power grab — lower legitimacy
                core.legitimacy = (0.15 + 0.15 * (1.0 - old_happiness)).clamp(0.0, 1.0);
            }

            // Happiness hit
            core.happiness =
                (old_happiness - 0.05 - 0.10 * old_happiness).clamp(0.0, 1.0);

            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "stability".to_string(),
                    old_value: serde_json::json!(old_stability),
                    new_value: serde_json::json!(core.stability),
                },
            );
            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "legitimacy".to_string(),
                    old_value: serde_json::json!(old_legitimacy),
                    new_value: serde_json::json!(core.legitimacy),
                },
            );
            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "happiness".to_string(),
                    old_value: serde_json::json!(old_happiness),
                    new_value: serde_json::json!(core.happiness),
                },
            );
        }
    } else {
        // Failed coup
        if let Some(mut core) = world.get_mut::<FactionCore>(faction) {
            let old_stability = core.stability;
            let old_legitimacy = core.legitimacy;

            core.stability = (core.stability - 0.05).max(0.0);
            core.legitimacy = (core.legitimacy + 0.10).min(1.0);

            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "stability".to_string(),
                    old_value: serde_json::json!(old_stability),
                    new_value: serde_json::json!(core.stability),
                },
            );
            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "legitimacy".to_string(),
                    old_value: serde_json::json!(old_legitimacy),
                    new_value: serde_json::json!(core.legitimacy),
                },
            );
        }

        // Possibly execute instigator
        if execute_instigator {
            if let Some(mut sim) = world.get_mut::<SimEntity>(instigator)
                && sim.end.is_none()
            {
                sim.end = Some(ctx.clock_time);
                ctx.record_effect(event_id, instigator, StateChange::EntityEnded);
                ctx.emit(SimReactiveEvent::EntityDied {
                    event_id,
                    entity: instigator,
                });
            }
            // Clean up structural relationships
            world.entity_mut(instigator).remove::<MemberOf>();
        }

        ctx.emit(SimReactiveEvent::FailedCoup {
            event_id,
            faction,
            instigator,
        });
    }

    let _ = instigator_sim;
}

/// Form alliance: insert into RelationshipGraph.allies.
pub(crate) fn apply_form_alliance(
    ctx: &mut ApplyCtx,
    _world: &mut World,
    event_id: u64,
    faction_a: Entity,
    faction_b: Entity,
) {
    let pair = RelationshipGraph::canonical_pair(faction_a, faction_b);

    // Idempotent: skip if already allies
    if !ctx.rel_graph.are_allies(faction_a, faction_b) {
        ctx.rel_graph
            .allies
            .insert(pair, RelationshipMeta::new(ctx.clock_time));
    }

    let a_sim = ctx.entity_map.get_sim(faction_a).unwrap_or(0);
    let b_sim = ctx.entity_map.get_sim(faction_b).unwrap_or(0);

    ctx.record_effect(
        event_id,
        faction_a,
        StateChange::RelationshipStarted {
            target_entity_id: b_sim,
            kind: RelationshipKind::Ally,
        },
    );
    ctx.record_effect(
        event_id,
        faction_b,
        StateChange::RelationshipStarted {
            target_entity_id: a_sim,
            kind: RelationshipKind::Ally,
        },
    );
}

/// Betray alliance: end ally relationship, add grievance, possibly create enemy relationship.
pub(crate) fn apply_betray_alliance(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    betrayer: Entity,
    betrayed: Entity,
) {
    let betrayer_sim = ctx.entity_map.get_sim(betrayer).unwrap_or(0);
    let betrayed_sim = ctx.entity_map.get_sim(betrayed).unwrap_or(0);

    // End the ally relationship
    let pair = RelationshipGraph::canonical_pair(betrayer, betrayed);
    if let Some(meta) = ctx.rel_graph.allies.get_mut(&pair) {
        meta.end = Some(ctx.clock_time);
    }

    ctx.record_effect(
        event_id,
        betrayer,
        StateChange::RelationshipEnded {
            target_entity_id: betrayed_sim,
            kind: RelationshipKind::Ally,
        },
    );
    ctx.record_effect(
        event_id,
        betrayed,
        StateChange::RelationshipEnded {
            target_entity_id: betrayer_sim,
            kind: RelationshipKind::Ally,
        },
    );

    // Update betrayer's diplomacy
    if let Some(mut diplomacy) = world.get_mut::<FactionDiplomacy>(betrayer) {
        diplomacy.betrayal_count += 1;
        diplomacy.last_betrayal = Some(ctx.clock_time);
    }

    // Update betrayed's diplomacy: add grievance
    if let Some(mut diplomacy) = world.get_mut::<FactionDiplomacy>(betrayed) {
        diplomacy.last_betrayed_by = Some(betrayer_sim);

        use crate::model::Grievance;
        use crate::model::SimTimestamp;
        let grievance = diplomacy
            .grievances
            .entry(betrayer_sim)
            .or_insert(Grievance {
                severity: 0.0,
                sources: Vec::new(),
                peak: 0.0,
                updated: SimTimestamp::default(),
            });
        grievance.severity = (grievance.severity + 0.50).min(1.0);
        if grievance.severity > grievance.peak {
            grievance.peak = grievance.severity;
        }
        if grievance.sources.len() < 5 {
            grievance.sources.push("alliance_betrayal".to_string());
        }
    }

    // Create enemy relationship (always — the system decides whether to call this)
    if !ctx.rel_graph.are_enemies(betrayer, betrayed) {
        ctx.rel_graph
            .enemies
            .insert(pair, RelationshipMeta::new(ctx.clock_time));
    }

    ctx.emit(SimReactiveEvent::AllianceBetrayed {
        event_id,
        betrayer,
        betrayed,
    });
}

/// Split faction: spawn new faction, move settlement and its people.
pub(crate) fn apply_split_faction(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    parent_faction: Entity,
    new_faction_name: String,
    settlement: Entity,
) {
    let parent_sim = ctx.entity_map.get_sim(parent_faction).unwrap_or(0);

    // Read parent faction state for inheritance
    let (gov_type, happiness, prestige) = world
        .get::<FactionCore>(parent_faction)
        .map(|c| (c.government_type, c.happiness, c.prestige))
        .unwrap_or_default();

    // Spawn new faction
    let new_faction_id = ctx.id_gen.0.next_id();
    let new_faction_entity = spawn::spawn_faction(
        world,
        new_faction_id,
        new_faction_name.clone(),
        Some(ctx.clock_time),
        FactionCore {
            government_type: gov_type,
            stability: 0.5,
            happiness: (happiness + 0.1).min(1.0),
            legitimacy: 0.6,
            treasury: 0.0,
            prestige: prestige * 0.25,
            ..FactionCore::default()
        },
        FactionDiplomacy {
            diplomatic_trust: 1.0,
            ..FactionDiplomacy::default()
        },
        FactionMilitary::default(),
    );
    ctx.entity_map.insert(new_faction_id, new_faction_entity);

    ctx.record_effect(
        event_id,
        new_faction_entity,
        StateChange::EntityCreated {
            kind: crate::model::EntityKind::Faction,
            name: new_faction_name,
        },
    );

    // Move settlement to new faction
    world
        .entity_mut(settlement)
        .insert(MemberOf(new_faction_entity));

    let settlement_sim = ctx.entity_map.get_sim(settlement).unwrap_or(0);
    ctx.record_effect(
        event_id,
        settlement,
        StateChange::RelationshipEnded {
            target_entity_id: parent_sim,
            kind: RelationshipKind::MemberOf,
        },
    );
    ctx.record_effect(
        event_id,
        settlement,
        StateChange::RelationshipStarted {
            target_entity_id: new_faction_id,
            kind: RelationshipKind::MemberOf,
        },
    );

    // Move all people in the settlement that belong to the parent faction
    // Collect first to avoid borrow conflicts
    let people_to_move: Vec<Entity> = world
        .query_filtered::<(Entity, &MemberOf, &crate::ecs::relationships::LocatedIn), bevy_ecs::query::With<Person>>()
        .iter(world)
        .filter(|(_, member, loc)| member.0 == parent_faction && loc.0 == settlement)
        .map(|(e, _, _)| e)
        .collect();

    for person in &people_to_move {
        world
            .entity_mut(*person)
            .insert(MemberOf(new_faction_entity));
        ctx.record_effect(
            event_id,
            *person,
            StateChange::RelationshipEnded {
                target_entity_id: parent_sim,
                kind: RelationshipKind::MemberOf,
            },
        );
        ctx.record_effect(
            event_id,
            *person,
            StateChange::RelationshipStarted {
                target_entity_id: new_faction_id,
                kind: RelationshipKind::MemberOf,
            },
        );
    }

    // Apply stability hit to parent faction
    if let Some(mut core) = world.get_mut::<FactionCore>(parent_faction) {
        let old_stability = core.stability;
        core.stability = (core.stability - 0.15).max(0.0);
        ctx.record_effect(
            event_id,
            parent_faction,
            StateChange::PropertyChanged {
                field: "stability".to_string(),
                old_value: serde_json::json!(old_stability),
                new_value: serde_json::json!(core.stability),
            },
        );
    }

    ctx.emit(SimReactiveEvent::FactionSplit {
        event_id,
        parent_faction,
        new_faction: new_faction_entity,
    });

    let _ = settlement_sim;
}
