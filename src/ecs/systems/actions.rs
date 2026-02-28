//! Actions system -- migrated from `src/sim/actions.rs`.
//!
//! Drains `PendingActions` each year and processes each action by validating
//! preconditions via ECS queries, determining outcomes (RNG for coups,
//! elections), and emitting `SimCommand` messages. No direct state mutation;
//! all changes flow through the command pipeline.

use bevy_app::{App, Plugin};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    Faction, FactionCore, FactionDiplomacy, FactionMilitary, Person, PersonCore, PersonReputation,
    PersonSocial, Settlement, SettlementCore, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::relationships::{LeaderOf, LeaderOfSources, MemberOf, RelationshipGraph};
use crate::ecs::resources::{ActionResults, ActionsRng, PendingActions, SimEntityMap};
use crate::ecs::schedule::{DomainSet, SimTick};
use crate::model::WarGoal;
use crate::model::action::{ActionKind, ActionOutcome, ActionResult, ActionSource};
use crate::model::entity_data::GovernmentType;
use crate::model::event::{EventKind, ParticipantRole};
use crate::model::relationship::RelationshipKind;
use crate::model::traits::Trait;

// --- Support faction ---
const SUPPORT_STABILITY_BOOST: f64 = 0.08;
const SUPPORT_HAPPINESS_BOOST: f64 = 0.06;

// --- Undermine faction ---
const UNDERMINE_STABILITY_PENALTY: f64 = 0.10;
const UNDERMINE_HAPPINESS_PENALTY: f64 = 0.08;
const UNDERMINE_LEGITIMACY_PENALTY: f64 = 0.06;

// --- Coup attempt ---
const COUP_ABLE_BODIED_DIVISOR: u32 = 4;
const COUP_MILITARY_NORMALIZATION: f64 = 200.0;
const COUP_RESISTANCE_BASE: f64 = 0.2;
const COUP_RESISTANCE_HAPPINESS_WEIGHT: f64 = 0.3;
const COUP_RESISTANCE_HAPPINESS_COMPLEMENT: f64 = 0.7;
const COUP_NOISE_RANGE: f64 = 0.1;
const COUP_POWER_BASE: f64 = 0.2;
const COUP_POWER_INSTABILITY_FACTOR: f64 = 0.3;
const COUP_SUCCESS_MIN: f64 = 0.1;
const COUP_SUCCESS_MAX: f64 = 0.9;
const COUP_FAILED_EXECUTION_CHANCE: f64 = 0.5;

// --- Defection ---
const DEFECT_STABILITY_PENALTY: f64 = 0.05;

// --- Seek office (election) ---
const ELECTION_BASE_CHANCE: f64 = 0.3;
const ELECTION_CHARISMATIC_BONUS: f64 = 0.2;
const ELECTION_INSTABILITY_BONUS: f64 = 0.1;
const ELECTION_INSTABILITY_THRESHOLD: f64 = 0.5;

// --- Betray ally ---
const BETRAYAL_STABILITY_PENALTY: f64 = 0.20;
const BETRAYAL_TRUST_PENALTY: f64 = 0.40;
const BETRAYAL_FACTION_PRESTIGE_PENALTY: f64 = 0.10;
const BETRAYAL_LEADER_PRESTIGE_PENALTY: f64 = 0.05;
const BETRAYAL_OTHER_ALLY_BREAK_CHANCE: f64 = 0.40;
const BETRAYAL_OTHER_ALLY_ENEMY_CHANCE: f64 = 0.25;
const BETRAYAL_VICTIM_ALLY_ENEMY_CHANCE: f64 = 0.50;

// ---------------------------------------------------------------------------
// System registration
// ---------------------------------------------------------------------------

pub struct ActionsPlugin;

impl Plugin for ActionsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ActionResults::default());
        app.add_systems(
            SimTick,
            process_actions.run_if(yearly).in_set(DomainSet::Actions),
        );
    }
}

// ---------------------------------------------------------------------------
// Main dispatch
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_actions(
    mut pending: ResMut<PendingActions>,
    mut results: ResMut<ActionResults>,
    persons: Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
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
        ),
        With<Faction>,
    >,
    settlements: Query<(Entity, &SimEntity, &SettlementCore, Option<&MemberOf>), With<Settlement>>,
    leader_sources: Query<&LeaderOfSources>,
    rel_graph: Res<RelationshipGraph>,
    entity_map: Res<SimEntityMap>,
    clock: Res<SimClock>,
    mut rng: ResMut<ActionsRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    let actions = std::mem::take(&mut pending.0);

    for action in actions {
        let source = action.source.clone();
        let outcome = match action.kind {
            ActionKind::Assassinate { target_id } => process_assassinate(
                &entity_map,
                &persons,
                target_id,
                action.actor_id,
                &action.source,
                &clock,
                &mut commands,
            ),
            ActionKind::SupportFaction { faction_id } => process_support_faction(
                &entity_map,
                &persons,
                &factions,
                action.actor_id,
                &action.source,
                faction_id,
                &clock,
                &mut commands,
            ),
            ActionKind::UndermineFaction { faction_id } => process_undermine_faction(
                &entity_map,
                &persons,
                &factions,
                action.actor_id,
                &action.source,
                faction_id,
                &clock,
                &mut commands,
            ),
            ActionKind::BrokerAlliance {
                faction_a,
                faction_b,
            } => process_broker_alliance(
                &entity_map,
                &persons,
                &factions,
                &rel_graph,
                action.actor_id,
                &action.source,
                faction_a,
                faction_b,
                &clock,
                &mut commands,
            ),
            ActionKind::DeclareWar { target_faction_id } => process_declare_war(
                &entity_map,
                &persons,
                &factions,
                &rel_graph,
                action.actor_id,
                &action.source,
                target_faction_id,
                &clock,
                &mut commands,
            ),
            ActionKind::AttemptCoup { faction_id } => process_attempt_coup(
                &entity_map,
                &persons,
                &factions,
                &settlements,
                &leader_sources,
                action.actor_id,
                &action.source,
                faction_id,
                &clock,
                &mut rng,
                &mut commands,
            ),
            ActionKind::Defect {
                from_faction,
                to_faction,
            } => process_defect(
                &entity_map,
                &persons,
                &factions,
                &settlements,
                action.actor_id,
                &action.source,
                from_faction,
                to_faction,
                &clock,
                &mut commands,
            ),
            ActionKind::SeekOffice { faction_id } => process_seek_office(
                &entity_map,
                &persons,
                &factions,
                &leader_sources,
                action.actor_id,
                &action.source,
                faction_id,
                &clock,
                &mut rng,
                &mut commands,
            ),
            ActionKind::BetrayAlly { ally_faction_id } => process_betray_ally(
                &entity_map,
                &persons,
                &factions,
                &rel_graph,
                action.actor_id,
                &action.source,
                ally_faction_id,
                &clock,
                &mut rng,
                &mut commands,
            ),
            ActionKind::PressClaim { target_faction_id } => process_press_claim(
                &entity_map,
                &persons,
                &factions,
                &rel_graph,
                action.actor_id,
                &action.source,
                target_faction_id,
                &clock,
                &mut commands,
            ),
        };
        results.0.push(ActionResult {
            actor_id: action.actor_id,
            source,
            outcome,
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a sim ID to a Bevy entity.
fn resolve(entity_map: &SimEntityMap, sim_id: u64) -> Option<Entity> {
    entity_map.get_bevy(sim_id)
}

/// Validate that an entity exists and is alive in the ECS.
fn validate_alive(entity_map: &SimEntityMap, sim_id: u64, label: &str) -> Result<Entity, String> {
    let Some(bevy_entity) = resolve(entity_map, sim_id) else {
        return Err(format!("{label} {sim_id} does not exist"));
    };
    Ok(bevy_entity)
}

/// Check that a Bevy entity's SimEntity is alive (end.is_none()).
fn is_alive_check(sim_entity: &SimEntity, sim_id: u64, kind_label: &str) -> Result<(), String> {
    if sim_entity.end.is_some() {
        Err(format!(
            "target {sim_id} does not exist or is not a living {kind_label}"
        ))
    } else {
        Ok(())
    }
}

/// Find the actor's faction by checking MemberOf on person entities.
#[allow(clippy::type_complexity)]
fn find_actor_faction_ecs(
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    actor_bevy: Entity,
) -> Option<Entity> {
    persons
        .get(actor_bevy)
        .ok()
        .and_then(|(_, _, _, _, _, member, _)| member.map(|m| m.0))
}

/// Check if actor is leader of a given faction.
#[allow(clippy::type_complexity)]
fn is_leader_of(
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    actor_bevy: Entity,
    faction_bevy: Entity,
) -> bool {
    persons
        .get(actor_bevy)
        .ok()
        .and_then(|(_, _, _, _, _, _, leader)| leader.map(|l| l.0 == faction_bevy))
        .unwrap_or(false)
}

/// Get entity name from the person query or faction query.
#[allow(clippy::type_complexity)]
fn person_name(
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    entity: Entity,
) -> String {
    persons
        .get(entity)
        .map(|(_, sim, _, _, _, _, _)| sim.name.clone())
        .unwrap_or_else(|_| "Unknown".to_string())
}

#[allow(clippy::type_complexity)]
fn faction_name(
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    entity: Entity,
) -> String {
    factions
        .get(entity)
        .map(|(_, sim, _, _, _)| sim.name.clone())
        .unwrap_or_else(|_| "Unknown".to_string())
}

/// Validate a person sim_id resolves and is alive.
#[allow(clippy::type_complexity)]
fn validate_living_person(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    sim_id: u64,
    label: &str,
) -> Result<Entity, String> {
    let bevy_entity = validate_alive(entity_map, sim_id, label)?;
    let (_, sim, _, _, _, _, _) = persons
        .get(bevy_entity)
        .map_err(|_| format!("{label} {sim_id} does not exist or is not a living person"))?;
    is_alive_check(sim, sim_id, "person")?;
    Ok(bevy_entity)
}

/// Validate a faction sim_id resolves and is alive.
#[allow(clippy::type_complexity)]
fn validate_living_faction(
    entity_map: &SimEntityMap,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    sim_id: u64,
    label: &str,
) -> Result<Entity, String> {
    let bevy_entity = validate_alive(entity_map, sim_id, label)?;
    let (_, sim, _, _, _) = factions
        .get(bevy_entity)
        .map_err(|_| format!("{label} {sim_id} does not exist or is not a living faction"))?;
    is_alive_check(sim, sim_id, "faction")?;
    Ok(bevy_entity)
}

// ---------------------------------------------------------------------------
// Action processors
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_assassinate(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    target_id: u64,
    actor_id: u64,
    source: &ActionSource,
    clock: &SimClock,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    // Validate target exists and is a living person
    let target_bevy = match validate_living_person(entity_map, persons, target_id, "target") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };

    let actor_bevy = resolve(entity_map, actor_id);
    let actor_name = actor_bevy
        .map(|e| person_name(persons, e))
        .unwrap_or_else(|| format!("Entity {actor_id}"));
    let target_name = person_name(persons, target_bevy);

    // Emit PersonDied command (the applicator handles ending relationships, leader vacancy, etc.)
    let mut cmd = SimCommand::new(
        SimCommandKind::PersonDied {
            person: target_bevy,
        },
        EventKind::Assassination,
        format!("{actor_name} assassinated {target_name} in year {year}"),
    )
    .with_participant(target_bevy, ParticipantRole::Object)
    .with_data(serde_json::to_value(source).unwrap_or_default());

    if let Some(actor_e) = actor_bevy {
        cmd = cmd.with_participant(actor_e, ParticipantRole::Instigator);
    }

    commands.write(cmd);

    ActionOutcome::Success { event_id: 0 }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_support_faction(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    actor_id: u64,
    source: &ActionSource,
    faction_id: u64,
    clock: &SimClock,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    let faction_bevy = match validate_living_faction(entity_map, factions, faction_id, "faction") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };

    let actor_bevy = resolve(entity_map, actor_id);
    let actor_name = actor_bevy
        .map(|e| person_name(persons, e))
        .unwrap_or_else(|| format!("Entity {actor_id}"));
    let fname = faction_name(factions, faction_bevy);

    // Main event: adjust faction stats
    let mut cmd = SimCommand::new(
        SimCommandKind::AdjustFactionStats {
            faction: faction_bevy,
            stability_delta: SUPPORT_STABILITY_BOOST,
            happiness_delta: SUPPORT_HAPPINESS_BOOST,
            legitimacy_delta: 0.0,
            trust_delta: 0.0,
            prestige_delta: 0.0,
        },
        EventKind::Intrigue,
        format!("{actor_name} bolstered {fname} in year {year}"),
    )
    .with_participant(faction_bevy, ParticipantRole::Object)
    .with_data(serde_json::to_value(source).unwrap_or_default());

    if let Some(actor_e) = actor_bevy {
        cmd = cmd.with_participant(actor_e, ParticipantRole::Instigator);
    }
    commands.write(cmd);

    ActionOutcome::Success { event_id: 0 }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_undermine_faction(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    actor_id: u64,
    source: &ActionSource,
    faction_id: u64,
    clock: &SimClock,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    let faction_bevy = match validate_living_faction(entity_map, factions, faction_id, "faction") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };

    let actor_bevy = resolve(entity_map, actor_id);
    let actor_name = actor_bevy
        .map(|e| person_name(persons, e))
        .unwrap_or_else(|| format!("Entity {actor_id}"));
    let fname = faction_name(factions, faction_bevy);

    // Main event: adjust faction stats (negative deltas)
    let mut cmd = SimCommand::new(
        SimCommandKind::AdjustFactionStats {
            faction: faction_bevy,
            stability_delta: -UNDERMINE_STABILITY_PENALTY,
            happiness_delta: -UNDERMINE_HAPPINESS_PENALTY,
            legitimacy_delta: -UNDERMINE_LEGITIMACY_PENALTY,
            trust_delta: 0.0,
            prestige_delta: 0.0,
        },
        EventKind::Intrigue,
        format!("{actor_name} undermined {fname} in year {year}"),
    )
    .with_participant(faction_bevy, ParticipantRole::Object)
    .with_data(serde_json::to_value(source).unwrap_or_default());

    if let Some(actor_e) = actor_bevy {
        cmd = cmd.with_participant(actor_e, ParticipantRole::Instigator);
    }
    commands.write(cmd);

    ActionOutcome::Success { event_id: 0 }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_broker_alliance(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    rel_graph: &RelationshipGraph,
    actor_id: u64,
    source: &ActionSource,
    faction_a_id: u64,
    faction_b_id: u64,
    clock: &SimClock,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    if faction_a_id == faction_b_id {
        return ActionOutcome::Failed {
            reason: "cannot broker alliance between a faction and itself".to_string(),
        };
    }

    let fa_bevy = match validate_living_faction(entity_map, factions, faction_a_id, "faction_a") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };
    let fb_bevy = match validate_living_faction(entity_map, factions, faction_b_id, "faction_b") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };

    // Check for existing diplomatic relationships
    if rel_graph.are_allies(fa_bevy, fb_bevy) {
        return ActionOutcome::Failed {
            reason: "factions are already allied".to_string(),
        };
    }
    if rel_graph.are_enemies(fa_bevy, fb_bevy) {
        return ActionOutcome::Failed {
            reason: "factions are currently enemies".to_string(),
        };
    }
    if rel_graph.are_at_war(fa_bevy, fb_bevy) {
        return ActionOutcome::Failed {
            reason: "factions already have an active diplomatic relationship".to_string(),
        };
    }

    let actor_bevy = resolve(entity_map, actor_id);
    let actor_name = actor_bevy
        .map(|e| person_name(persons, e))
        .unwrap_or_else(|| format!("Entity {actor_id}"));
    let name_a = faction_name(factions, fa_bevy);
    let name_b = faction_name(factions, fb_bevy);

    let mut cmd = SimCommand::new(
        SimCommandKind::FormAlliance {
            faction_a: fa_bevy,
            faction_b: fb_bevy,
        },
        EventKind::Alliance,
        format!("{actor_name} brokered an alliance between {name_a} and {name_b} in year {year}"),
    )
    .with_participant(fa_bevy, ParticipantRole::Subject)
    .with_participant(fb_bevy, ParticipantRole::Object)
    .with_data(serde_json::to_value(source).unwrap_or_default());

    if let Some(actor_e) = actor_bevy {
        cmd = cmd.with_participant(actor_e, ParticipantRole::Instigator);
    }
    commands.write(cmd);

    ActionOutcome::Success { event_id: 0 }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_declare_war(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    rel_graph: &RelationshipGraph,
    actor_id: u64,
    source: &ActionSource,
    target_faction_id: u64,
    clock: &SimClock,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    let target_bevy =
        match validate_living_faction(entity_map, factions, target_faction_id, "faction") {
            Ok(e) => e,
            Err(reason) => return ActionOutcome::Failed { reason },
        };

    // Find actor's faction
    let actor_bevy = match resolve(entity_map, actor_id) {
        Some(e) => e,
        None => {
            return ActionOutcome::Failed {
                reason: "actor does not exist".to_string(),
            };
        }
    };

    let Some(actor_faction_bevy) = find_actor_faction_ecs(persons, actor_bevy) else {
        return ActionOutcome::Failed {
            reason: "actor does not belong to any faction".to_string(),
        };
    };

    // Get actor's faction sim_id for comparison
    let actor_faction_sim_id = entity_map.get_sim(actor_faction_bevy).unwrap_or(0);
    if actor_faction_sim_id == target_faction_id {
        return ActionOutcome::Failed {
            reason: "cannot declare war on own faction".to_string(),
        };
    }

    // Check not already at war
    if rel_graph.are_at_war(actor_faction_bevy, target_bevy) {
        return ActionOutcome::Failed {
            reason: "factions are already at war".to_string(),
        };
    }

    let attacker_name = faction_name(factions, actor_faction_bevy);
    let defender_name = faction_name(factions, target_bevy);
    let actor_name = person_name(persons, actor_bevy);

    // End any existing ally relationship via EndRelationship commands
    if rel_graph.are_allies(actor_faction_bevy, target_bevy) {
        commands.write(SimCommand::bookkeeping(SimCommandKind::EndRelationship {
            source: actor_faction_bevy,
            target: target_bevy,
            kind: RelationshipKind::Ally,
        }));
    }

    let cmd = SimCommand::new(
        SimCommandKind::DeclareWar {
            attacker: actor_faction_bevy,
            defender: target_bevy,
        },
        EventKind::WarDeclared,
        format!("{actor_name} of {attacker_name} declared war on {defender_name} in year {year}"),
    )
    .with_participant(actor_bevy, ParticipantRole::Instigator)
    .with_participant(actor_faction_bevy, ParticipantRole::Attacker)
    .with_participant(target_bevy, ParticipantRole::Defender)
    .with_data(serde_json::to_value(source).unwrap_or_default());

    commands.write(cmd);

    ActionOutcome::Success { event_id: 0 }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_attempt_coup(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    settlements: &Query<(Entity, &SimEntity, &SettlementCore, Option<&MemberOf>), With<Settlement>>,
    leader_sources: &Query<&LeaderOfSources>,
    actor_id: u64,
    source: &ActionSource,
    faction_id: u64,
    clock: &SimClock,
    rng: &mut ActionsRng,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    let faction_bevy = match validate_living_faction(entity_map, factions, faction_id, "faction") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };

    let actor_bevy = match resolve(entity_map, actor_id) {
        Some(e) => e,
        None => {
            return ActionOutcome::Failed {
                reason: "actor does not exist".to_string(),
            };
        }
    };

    // Find current leader
    let leader_entity = leader_sources
        .get(faction_bevy)
        .ok()
        .and_then(|sources| sources.iter().next().copied());
    let Some(leader_bevy) = leader_entity else {
        return ActionOutcome::Failed {
            reason: "faction has no leader to overthrow".to_string(),
        };
    };

    if actor_bevy == leader_bevy {
        return ActionOutcome::Failed {
            reason: "cannot coup yourself".to_string(),
        };
    }

    // Read faction stats
    let (_, _, fc, _, _) = factions.get(faction_bevy).unwrap();
    let stability = fc.stability;
    let happiness = fc.happiness;
    let legitimacy = fc.legitimacy;
    let instability = 1.0 - stability;

    // Military strength from faction settlements (only living settlements)
    let mut able_bodied = 0u32;
    for (_, sim, sc, member) in settlements.iter() {
        if sim.end.is_none()
            && let Some(m) = member
            && m.0 == faction_bevy
        {
            able_bodied += sc.population / COUP_ABLE_BODIED_DIVISOR;
        }
    }
    let military = (able_bodied as f64 / COUP_MILITARY_NORMALIZATION).clamp(0.0, 1.0);
    let resistance = COUP_RESISTANCE_BASE
        + military
            * legitimacy
            * (COUP_RESISTANCE_HAPPINESS_WEIGHT + COUP_RESISTANCE_HAPPINESS_COMPLEMENT * happiness);
    let noise: f64 = rng.0.random_range(-COUP_NOISE_RANGE..COUP_NOISE_RANGE);
    let coup_power =
        (COUP_POWER_BASE + COUP_POWER_INSTABILITY_FACTOR * instability + noise).max(0.0);
    let success_chance =
        (coup_power / (coup_power + resistance)).clamp(COUP_SUCCESS_MIN, COUP_SUCCESS_MAX);

    let succeeded = rng.0.random_range(0.0..1.0) < success_chance;
    let execute_instigator = !succeeded && rng.0.random_bool(COUP_FAILED_EXECUTION_CHANCE);

    let actor_name = person_name(persons, actor_bevy);
    let leader_name = person_name(persons, leader_bevy);
    let fname = faction_name(factions, faction_bevy);

    let description = if succeeded {
        format!("{actor_name} overthrew {leader_name} of {fname} in year {year}")
    } else {
        format!("{actor_name} failed to overthrow {leader_name} of {fname} in year {year}")
    };

    let event_kind = if succeeded {
        EventKind::Coup
    } else {
        EventKind::FailedCoup
    };

    let cmd = SimCommand::new(
        SimCommandKind::AttemptCoup {
            faction: faction_bevy,
            instigator: actor_bevy,
            succeeded,
            execute_instigator,
        },
        event_kind,
        description,
    )
    .with_participant(actor_bevy, ParticipantRole::Instigator)
    .with_participant(leader_bevy, ParticipantRole::Subject)
    .with_participant(faction_bevy, ParticipantRole::Object)
    .with_data(serde_json::to_value(source).unwrap_or_default());

    commands.write(cmd);

    if succeeded {
        // Stability/legitimacy adjustments are handled by the AttemptCoup applicator
        ActionOutcome::Success { event_id: 0 }
    } else {
        ActionOutcome::Failed {
            reason: "coup attempt failed".to_string(),
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_defect(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    settlements: &Query<(Entity, &SimEntity, &SettlementCore, Option<&MemberOf>), With<Settlement>>,
    actor_id: u64,
    source: &ActionSource,
    from_faction_id: u64,
    to_faction_id: u64,
    clock: &SimClock,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    // Validate both factions alive
    let from_bevy =
        match validate_living_faction(entity_map, factions, from_faction_id, "from_faction") {
            Ok(e) => e,
            Err(reason) => return ActionOutcome::Failed { reason },
        };
    let to_bevy = match validate_living_faction(entity_map, factions, to_faction_id, "to_faction") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };

    let actor_bevy = match resolve(entity_map, actor_id) {
        Some(e) => e,
        None => {
            return ActionOutcome::Failed {
                reason: "actor does not exist".to_string(),
            };
        }
    };

    // Validate actor is member of from_faction
    let is_member = persons
        .get(actor_bevy)
        .ok()
        .and_then(|(_, _, _, _, _, member, _)| member.map(|m| m.0 == from_bevy))
        .unwrap_or(false);
    if !is_member {
        return ActionOutcome::Failed {
            reason: "actor is not a member of the source faction".to_string(),
        };
    }

    // Leaders can't defect
    if is_leader_of(persons, actor_bevy, from_bevy) {
        return ActionOutcome::Failed {
            reason: "leaders cannot defect".to_string(),
        };
    }

    let actor_name = person_name(persons, actor_bevy);
    let from_name = faction_name(factions, from_bevy);
    let to_name = faction_name(factions, to_bevy);

    // End MemberOf with old faction, then add new MemberOf
    commands.write(SimCommand::bookkeeping(SimCommandKind::EndRelationship {
        source: actor_bevy,
        target: from_bevy,
        kind: RelationshipKind::MemberOf,
    }));
    commands.write(SimCommand::bookkeeping(SimCommandKind::AddRelationship {
        source: actor_bevy,
        target: to_bevy,
        kind: RelationshipKind::MemberOf,
    }));

    // Find a settlement in the new faction to relocate to
    let new_settlement: Option<Entity> = settlements
        .iter()
        .find(|(_, sim, _, member)| sim.end.is_none() && member.is_some_and(|m| m.0 == to_bevy))
        .map(|(e, _, _, _)| e);

    // Main event: relocate person (also records the event in the log)
    if let Some(settlement_bevy) = new_settlement {
        let cmd = SimCommand::new(
            SimCommandKind::RelocatePerson {
                person: actor_bevy,
                to_settlement: settlement_bevy,
            },
            EventKind::Defection,
            format!("{actor_name} defected from {from_name} to {to_name} in year {year}"),
        )
        .with_participant(actor_bevy, ParticipantRole::Instigator)
        .with_participant(from_bevy, ParticipantRole::Origin)
        .with_participant(to_bevy, ParticipantRole::Destination)
        .with_data(serde_json::to_value(source).unwrap_or_default());
        commands.write(cmd);
    } else {
        // No settlement found; still record the defection event via a stats adjustment
        let cmd = SimCommand::new(
            SimCommandKind::AdjustFactionStats {
                faction: from_bevy,
                stability_delta: -DEFECT_STABILITY_PENALTY,
                happiness_delta: 0.0,
                legitimacy_delta: 0.0,
                trust_delta: 0.0,
                prestige_delta: 0.0,
            },
            EventKind::Defection,
            format!("{actor_name} defected from {from_name} to {to_name} in year {year}"),
        )
        .with_participant(actor_bevy, ParticipantRole::Instigator)
        .with_participant(from_bevy, ParticipantRole::Origin)
        .with_participant(to_bevy, ParticipantRole::Destination)
        .with_data(serde_json::to_value(source).unwrap_or_default());
        commands.write(cmd);
        return ActionOutcome::Success { event_id: 0 };
    }

    // Stability hit to old faction (bookkeeping)
    commands.write(SimCommand::bookkeeping(
        SimCommandKind::AdjustFactionStats {
            faction: from_bevy,
            stability_delta: -DEFECT_STABILITY_PENALTY,
            happiness_delta: 0.0,
            legitimacy_delta: 0.0,
            trust_delta: 0.0,
            prestige_delta: 0.0,
        },
    ));

    ActionOutcome::Success { event_id: 0 }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_seek_office(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    leader_sources: &Query<&LeaderOfSources>,
    actor_id: u64,
    source: &ActionSource,
    faction_id: u64,
    clock: &SimClock,
    rng: &mut ActionsRng,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    let faction_bevy = match validate_living_faction(entity_map, factions, faction_id, "faction") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };

    let actor_bevy = match resolve(entity_map, actor_id) {
        Some(e) => e,
        None => {
            return ActionOutcome::Failed {
                reason: "actor does not exist".to_string(),
            };
        }
    };

    // Validate actor is a member
    let is_member = persons
        .get(actor_bevy)
        .ok()
        .and_then(|(_, _, _, _, _, member, _)| member.map(|m| m.0 == faction_bevy))
        .unwrap_or(false);
    if !is_member {
        return ActionOutcome::Failed {
            reason: "actor is not a member of the faction".to_string(),
        };
    }

    // Check if already leader
    if is_leader_of(persons, actor_bevy, faction_bevy) {
        return ActionOutcome::Failed {
            reason: "actor is already the leader".to_string(),
        };
    }

    // Check if faction has a leader
    let current_leader = leader_sources
        .get(faction_bevy)
        .ok()
        .and_then(|sources| sources.iter().next().copied());

    let actor_name = person_name(persons, actor_bevy);
    let fname = faction_name(factions, faction_bevy);

    if current_leader.is_none() {
        // Leaderless faction -- auto-succeed
        let cmd = SimCommand::new(
            SimCommandKind::SucceedLeader {
                faction: faction_bevy,
                new_leader: actor_bevy,
            },
            EventKind::Succession,
            format!("{actor_name} claimed leadership of {fname} in year {year}"),
        )
        .with_participant(actor_bevy, ParticipantRole::Instigator)
        .with_participant(faction_bevy, ParticipantRole::Object)
        .with_data(serde_json::to_value(source).unwrap_or_default());

        commands.write(cmd);
        return ActionOutcome::Success { event_id: 0 };
    }

    // Faction has leader -- check government type
    let (_, _, fc, _, _) = factions.get(faction_bevy).unwrap();
    let gov_type = fc.government_type;

    if gov_type != GovernmentType::Elective {
        return ActionOutcome::Failed {
            reason: "faction government is not elective".to_string(),
        };
    }

    // Elective faction: probabilistic success
    let stability = fc.stability;
    let mut success_chance = ELECTION_BASE_CHANCE;

    // Check for Charismatic trait
    let has_charismatic = persons
        .get(actor_bevy)
        .ok()
        .is_some_and(|(_, _, core, _, _, _, _)| core.traits.contains(&Trait::Charismatic));
    if has_charismatic {
        success_chance += ELECTION_CHARISMATIC_BONUS;
    }

    // Instability bonus
    if stability < ELECTION_INSTABILITY_THRESHOLD {
        success_chance += ELECTION_INSTABILITY_BONUS
            * ((ELECTION_INSTABILITY_THRESHOLD - stability) / ELECTION_INSTABILITY_THRESHOLD);
    }

    let leader_bevy = current_leader.unwrap();
    let leader_name = person_name(persons, leader_bevy);

    if rng.0.random_range(0.0..1.0) < success_chance {
        // Success: replace leader
        let cmd = SimCommand::new(
            SimCommandKind::SucceedLeader {
                faction: faction_bevy,
                new_leader: actor_bevy,
            },
            EventKind::Succession,
            format!(
                "{actor_name} was elected to lead {fname}, replacing {leader_name} in year {year}"
            ),
        )
        .with_participant(actor_bevy, ParticipantRole::Instigator)
        .with_participant(leader_bevy, ParticipantRole::Subject)
        .with_participant(faction_bevy, ParticipantRole::Object)
        .with_data(serde_json::to_value(source).unwrap_or_default());

        commands.write(cmd);
        ActionOutcome::Success { event_id: 0 }
    } else {
        // Failed election
        let cmd = SimCommand::new(
            SimCommandKind::SetField {
                entity: faction_bevy,
                field: "election_failed".to_string(),
                old_value: serde_json::Value::Null,
                new_value: serde_json::Value::Null,
            },
            EventKind::Election,
            format!("{actor_name} failed to win election in {fname} in year {year}"),
        )
        .with_participant(actor_bevy, ParticipantRole::Instigator)
        .with_participant(faction_bevy, ParticipantRole::Object)
        .with_data(serde_json::to_value(source).unwrap_or_default());

        commands.write(cmd);
        ActionOutcome::Failed {
            reason: "election attempt failed".to_string(),
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_betray_ally(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    rel_graph: &RelationshipGraph,
    actor_id: u64,
    source: &ActionSource,
    ally_faction_id: u64,
    clock: &SimClock,
    rng: &mut ActionsRng,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    // Find actor's faction
    let actor_bevy = match resolve(entity_map, actor_id) {
        Some(e) => e,
        None => {
            return ActionOutcome::Failed {
                reason: "actor does not exist".to_string(),
            };
        }
    };

    let Some(actor_faction_bevy) = find_actor_faction_ecs(persons, actor_bevy) else {
        return ActionOutcome::Failed {
            reason: "actor does not belong to any faction".to_string(),
        };
    };

    // Validate actor is leader
    if !is_leader_of(persons, actor_bevy, actor_faction_bevy) {
        return ActionOutcome::Failed {
            reason: "only faction leaders can betray allies".to_string(),
        };
    }

    // Validate ally faction exists
    let ally_bevy =
        match validate_living_faction(entity_map, factions, ally_faction_id, "ally faction") {
            Ok(e) => e,
            Err(reason) => return ActionOutcome::Failed { reason },
        };

    // Validate alliance exists
    if !rel_graph.are_allies(actor_faction_bevy, ally_bevy) {
        return ActionOutcome::Failed {
            reason: "no active alliance with target faction".to_string(),
        };
    }

    let betrayer_name = faction_name(factions, actor_faction_bevy);
    let victim_name = faction_name(factions, ally_bevy);
    let actor_name = person_name(persons, actor_bevy);

    // Emit BetrayAlliance command (handles ending ally, adding enemy, grievance)
    let cmd = SimCommand::new(
        SimCommandKind::BetrayAlliance {
            betrayer: actor_faction_bevy,
            betrayed: ally_bevy,
        },
        EventKind::Betrayal,
        format!(
            "{actor_name} of {betrayer_name} betrayed the alliance with {victim_name} in year {year}"
        ),
    )
    .with_participant(actor_bevy, ParticipantRole::Instigator)
    .with_participant(actor_faction_bevy, ParticipantRole::Attacker)
    .with_participant(ally_bevy, ParticipantRole::Defender)
    .with_data(serde_json::to_value(source).unwrap_or_default());

    commands.write(cmd);

    // Declare war between betrayer and betrayed
    commands.write(SimCommand::bookkeeping(SimCommandKind::DeclareWar {
        attacker: actor_faction_bevy,
        defender: ally_bevy,
    }));

    // Stability + trust + faction prestige penalties on betrayer (all in one command)
    commands.write(SimCommand::bookkeeping(
        SimCommandKind::AdjustFactionStats {
            faction: actor_faction_bevy,
            stability_delta: -BETRAYAL_STABILITY_PENALTY,
            happiness_delta: 0.0,
            legitimacy_delta: 0.0,
            trust_delta: -BETRAYAL_TRUST_PENALTY,
            prestige_delta: -BETRAYAL_FACTION_PRESTIGE_PENALTY,
        },
    ));

    // Leader prestige penalty
    if persons.get(actor_bevy).is_ok() {
        commands.write(SimCommand::bookkeeping(SimCommandKind::AdjustPrestige {
            entity: actor_bevy,
            delta: -BETRAYAL_LEADER_PRESTIGE_PENALTY,
        }));
    }

    // Third-party reactions: betrayer's other allies
    let mut betrayer_other_allies = Vec::new();
    for (&pair, meta) in &rel_graph.allies {
        if !meta.is_active() {
            continue;
        }
        if pair.0 == actor_faction_bevy && pair.1 != ally_bevy {
            betrayer_other_allies.push(pair.1);
        } else if pair.1 == actor_faction_bevy && pair.0 != ally_bevy {
            betrayer_other_allies.push(pair.0);
        }
    }

    for other_ally in &betrayer_other_allies {
        let roll: f64 = rng.0.random_range(0.0..1.0);
        if roll < BETRAYAL_OTHER_ALLY_BREAK_CHANCE {
            // Break alliance
            commands.write(SimCommand::bookkeeping(SimCommandKind::EndRelationship {
                source: actor_faction_bevy,
                target: *other_ally,
                kind: RelationshipKind::Ally,
            }));
            if roll < BETRAYAL_OTHER_ALLY_ENEMY_CHANCE {
                // Also become enemy
                commands.write(SimCommand::bookkeeping(SimCommandKind::AddRelationship {
                    source: actor_faction_bevy,
                    target: *other_ally,
                    kind: RelationshipKind::Enemy,
                }));
                commands.write(SimCommand::bookkeeping(SimCommandKind::AddRelationship {
                    source: *other_ally,
                    target: actor_faction_bevy,
                    kind: RelationshipKind::Enemy,
                }));
            }
        }
    }

    // Victim's allies may become enemy of betrayer
    let mut victim_allies = Vec::new();
    for (&pair, meta) in &rel_graph.allies {
        if !meta.is_active() {
            continue;
        }
        if pair.0 == ally_bevy && pair.1 != actor_faction_bevy {
            victim_allies.push(pair.1);
        } else if pair.1 == ally_bevy && pair.0 != actor_faction_bevy {
            victim_allies.push(pair.0);
        }
    }

    for victim_ally in &victim_allies {
        if rng.0.random_range(0.0..1.0) < BETRAYAL_VICTIM_ALLY_ENEMY_CHANCE {
            // End any existing alliance with betrayer
            commands.write(SimCommand::bookkeeping(SimCommandKind::EndRelationship {
                source: actor_faction_bevy,
                target: *victim_ally,
                kind: RelationshipKind::Ally,
            }));
            commands.write(SimCommand::bookkeeping(SimCommandKind::AddRelationship {
                source: *victim_ally,
                target: actor_faction_bevy,
                kind: RelationshipKind::Enemy,
            }));
            commands.write(SimCommand::bookkeeping(SimCommandKind::AddRelationship {
                source: actor_faction_bevy,
                target: *victim_ally,
                kind: RelationshipKind::Enemy,
            }));
        }
    }

    ActionOutcome::Success { event_id: 0 }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn process_press_claim(
    entity_map: &SimEntityMap,
    persons: &Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            Option<&PersonReputation>,
            Option<&PersonSocial>,
            Option<&MemberOf>,
            Option<&LeaderOf>,
        ),
        With<Person>,
    >,
    factions: &Query<
        (
            Entity,
            &SimEntity,
            &FactionCore,
            Option<&FactionDiplomacy>,
            Option<&FactionMilitary>,
        ),
        With<Faction>,
    >,
    rel_graph: &RelationshipGraph,
    actor_id: u64,
    source: &ActionSource,
    target_faction_id: u64,
    clock: &SimClock,
    commands: &mut MessageWriter<SimCommand>,
) -> ActionOutcome {
    let year = clock.time.year();

    // Validate actor is alive
    let actor_bevy = match validate_living_person(entity_map, persons, actor_id, "actor") {
        Ok(e) => e,
        Err(reason) => return ActionOutcome::Failed { reason },
    };

    // Find actor's faction
    let Some(actor_faction_bevy) = find_actor_faction_ecs(persons, actor_bevy) else {
        return ActionOutcome::Failed {
            reason: "actor does not belong to any faction".to_string(),
        };
    };

    // Validate actor is leader
    if !is_leader_of(persons, actor_bevy, actor_faction_bevy) {
        return ActionOutcome::Failed {
            reason: "only faction leaders can press claims".to_string(),
        };
    }

    // Validate target faction exists
    let target_bevy =
        match validate_living_faction(entity_map, factions, target_faction_id, "target faction") {
            Ok(e) => e,
            Err(reason) => return ActionOutcome::Failed { reason },
        };

    // Can't press claim on own faction
    let actor_faction_sim_id = entity_map.get_sim(actor_faction_bevy).unwrap_or(0);
    if actor_faction_sim_id == target_faction_id {
        return ActionOutcome::Failed {
            reason: "cannot press claim on own faction".to_string(),
        };
    }

    // Validate actor has a claim on target faction
    let has_claim = persons
        .get(actor_bevy)
        .ok()
        .and_then(|(_, _, _, _, social, _, _)| {
            social.and_then(|s| {
                s.claims
                    .get(&target_faction_id)
                    .filter(|c| c.strength >= 0.1)
            })
        })
        .is_some();
    if !has_claim {
        return ActionOutcome::Failed {
            reason: "actor has no valid claim on target faction".to_string(),
        };
    }

    // Check not already at war
    if rel_graph.are_at_war(actor_faction_bevy, target_bevy) {
        return ActionOutcome::Failed {
            reason: "already at war with target faction".to_string(),
        };
    }

    let actor_name = person_name(persons, actor_bevy);
    let attacker_name = faction_name(factions, actor_faction_bevy);
    let target_name = faction_name(factions, target_bevy);

    // End any existing ally relationship
    if rel_graph.are_allies(actor_faction_bevy, target_bevy) {
        commands.write(SimCommand::bookkeeping(SimCommandKind::EndRelationship {
            source: actor_faction_bevy,
            target: target_bevy,
            kind: RelationshipKind::Ally,
        }));
    }

    // Add Enemy relationship (bidirectional)
    commands.write(SimCommand::bookkeeping(SimCommandKind::AddRelationship {
        source: actor_faction_bevy,
        target: target_bevy,
        kind: RelationshipKind::Enemy,
    }));

    // Declare war
    let cmd = SimCommand::new(
        SimCommandKind::DeclareWar {
            attacker: actor_faction_bevy,
            defender: target_bevy,
        },
        EventKind::WarDeclared,
        format!(
            "{actor_name} of {attacker_name} pressed their claim on the throne of {target_name} in year {year}"
        ),
    )
    .with_participant(actor_bevy, ParticipantRole::Instigator)
    .with_participant(actor_faction_bevy, ParticipantRole::Attacker)
    .with_participant(target_bevy, ParticipantRole::Defender)
    .with_data(serde_json::to_value(source).unwrap_or_default());

    commands.write(cmd);

    // Set war goal (SuccessionClaim) on the attacker faction's diplomacy
    commands.write(SimCommand::bookkeeping(SimCommandKind::SetWarGoal {
        faction: actor_faction_bevy,
        target_faction: target_bevy,
        goal: WarGoal::SuccessionClaim {
            claimant_id: actor_id,
        },
    }));

    ActionOutcome::Success { event_id: 0 }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app;
    use crate::ecs::components::{
        EcsBuildingBonuses, EcsSeasonalModifiers, FactionCore, FactionDiplomacy, FactionMilitary,
        PersonCore, PersonEducation, PersonReputation, PersonSocial, SettlementCore,
        SettlementCrime, SettlementCulture, SettlementDisease, SettlementEducation,
        SettlementMilitary as SettMilitary, SettlementTrade,
    };
    use crate::ecs::relationships::{LeaderOf, MemberOf, RelationshipGraph, RelationshipMeta};
    use crate::ecs::resources::PendingActions;
    use crate::ecs::schedule::SimTick;
    use crate::ecs::spawn::{spawn_faction, spawn_person, spawn_settlement};
    use crate::ecs::time::SimTime;
    use crate::model::action::{Action, ActionKind, ActionOutcome, ActionSource};

    fn setup_app() -> bevy_app::App {
        let mut app = build_sim_app(100);
        app.insert_resource(PendingActions::default());
        app.add_plugins(ActionsPlugin);
        app
    }

    fn spawn_test_person(app: &mut bevy_app::App, id: u64, name: &str) -> Entity {
        spawn_person(
            app.world_mut(),
            id,
            name.to_string(),
            Some(SimTime::from_year(80)),
            PersonCore::default(),
            PersonReputation::default(),
            PersonSocial::default(),
            PersonEducation::default(),
        )
    }

    fn spawn_test_faction(
        app: &mut bevy_app::App,
        id: u64,
        name: &str,
        stability: f64,
        happiness: f64,
        legitimacy: f64,
    ) -> Entity {
        spawn_faction(
            app.world_mut(),
            id,
            name.to_string(),
            Some(SimTime::from_year(50)),
            FactionCore {
                stability,
                happiness,
                legitimacy,
                ..FactionCore::default()
            },
            FactionDiplomacy::default(),
            FactionMilitary::default(),
        )
    }

    fn spawn_test_settlement(
        app: &mut bevy_app::App,
        id: u64,
        name: &str,
        faction: Entity,
        population: u32,
    ) -> Entity {
        let e = spawn_settlement(
            app.world_mut(),
            id,
            name.to_string(),
            Some(SimTime::from_year(50)),
            SettlementCore {
                population,
                ..SettlementCore::default()
            },
            SettlementCulture::default(),
            SettlementDisease::default(),
            SettlementTrade::default(),
            SettMilitary::default(),
            SettlementCrime::default(),
            SettlementEducation::default(),
            EcsSeasonalModifiers::default(),
            EcsBuildingBonuses::default(),
        );
        app.world_mut().entity_mut(e).insert(MemberOf(faction));
        e
    }

    fn queue_action(app: &mut bevy_app::App, action: Action) {
        app.world_mut()
            .resource_mut::<PendingActions>()
            .0
            .push(action);
    }

    fn tick(app: &mut bevy_app::App) {
        // The yearly condition fires at year-start. Clock starts at year 100 minute 0,
        // so the first tick is a year-start.
        app.world_mut().run_schedule(SimTick);
    }

    fn get_results(app: &bevy_app::App) -> Vec<ActionResult> {
        app.world().resource::<ActionResults>().0.clone()
    }

    // -----------------------------------------------------------------------
    // Test: Assassinate emits PersonDied command
    // -----------------------------------------------------------------------

    #[test]
    fn assassinate_emits_person_died() {
        let mut app = setup_app();

        let _actor = spawn_test_person(&mut app, 1, "Assassin");
        let _target = spawn_test_person(&mut app, 2, "Victim");

        queue_action(
            &mut app,
            Action {
                actor_id: 1,
                source: ActionSource::Player,
                kind: ActionKind::Assassinate { target_id: 2 },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0].outcome, ActionOutcome::Success { .. }),
            "expected success, got {:?}",
            results[0].outcome
        );
    }

    // -----------------------------------------------------------------------
    // Test: Coup attempt emits AttemptCoup command
    // -----------------------------------------------------------------------

    #[test]
    fn coup_attempt_emits_command() {
        let mut app = setup_app();

        // Unstable faction for higher coup success chance
        let faction = spawn_test_faction(&mut app, 1, "Kingdom", 0.1, 0.1, 0.1);
        let leader = spawn_test_person(&mut app, 2, "Old King");
        app.world_mut()
            .entity_mut(leader)
            .insert((MemberOf(faction), LeaderOf(faction)));
        let actor = spawn_test_person(&mut app, 3, "Rebel");
        app.world_mut().entity_mut(actor).insert(MemberOf(faction));
        let _settlement = spawn_test_settlement(&mut app, 4, "Town", faction, 100);

        queue_action(
            &mut app,
            Action {
                actor_id: 3,
                source: ActionSource::Player,
                kind: ActionKind::AttemptCoup { faction_id: 1 },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        // Either success or failed is valid -- the important thing is it processed
        match &results[0].outcome {
            ActionOutcome::Success { .. } => {}
            ActionOutcome::Failed { reason } => {
                assert_eq!(reason, "coup attempt failed");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Test: Invalid target returns Failed
    // -----------------------------------------------------------------------

    #[test]
    fn invalid_target_returns_failed() {
        let mut app = setup_app();

        let _actor = spawn_test_person(&mut app, 1, "Assassin");

        queue_action(
            &mut app,
            Action {
                actor_id: 1,
                source: ActionSource::Player,
                kind: ActionKind::Assassinate { target_id: 99999 },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0].outcome,
                ActionOutcome::Failed { reason } if reason.contains("does not exist")
            ),
            "expected failure about missing target, got {:?}",
            results[0].outcome
        );
    }

    // -----------------------------------------------------------------------
    // Test: Declare war emits DeclareWar command
    // -----------------------------------------------------------------------

    #[test]
    fn declare_war_emits_command() {
        let mut app = setup_app();

        let faction_a = spawn_test_faction(&mut app, 1, "Kingdom A", 0.5, 0.5, 0.5);
        let _faction_b = spawn_test_faction(&mut app, 2, "Kingdom B", 0.5, 0.5, 0.5);
        let actor = spawn_test_person(&mut app, 3, "Warlord");
        app.world_mut()
            .entity_mut(actor)
            .insert(MemberOf(faction_a));

        queue_action(
            &mut app,
            Action {
                actor_id: 3,
                source: ActionSource::Player,
                kind: ActionKind::DeclareWar {
                    target_faction_id: 2,
                },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0].outcome, ActionOutcome::Success { .. }),
            "expected success, got {:?}",
            results[0].outcome
        );
    }

    // -----------------------------------------------------------------------
    // Test: Support faction produces Success
    // -----------------------------------------------------------------------

    #[test]
    fn support_faction_succeeds() {
        let mut app = setup_app();

        let _faction = spawn_test_faction(&mut app, 1, "Alliance", 0.5, 0.5, 0.5);
        let _actor = spawn_test_person(&mut app, 2, "Supporter");

        queue_action(
            &mut app,
            Action {
                actor_id: 2,
                source: ActionSource::Player,
                kind: ActionKind::SupportFaction { faction_id: 1 },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0].outcome, ActionOutcome::Success { .. }));
    }

    // -----------------------------------------------------------------------
    // Test: Undermine faction produces Success
    // -----------------------------------------------------------------------

    #[test]
    fn undermine_faction_succeeds() {
        let mut app = setup_app();

        let _faction = spawn_test_faction(&mut app, 1, "Empire", 0.5, 0.5, 0.5);
        let _actor = spawn_test_person(&mut app, 2, "Saboteur");

        queue_action(
            &mut app,
            Action {
                actor_id: 2,
                source: ActionSource::Player,
                kind: ActionKind::UndermineFaction { faction_id: 1 },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0].outcome, ActionOutcome::Success { .. }));
    }

    // -----------------------------------------------------------------------
    // Test: Broker alliance succeeds for valid factions
    // -----------------------------------------------------------------------

    #[test]
    fn broker_alliance_succeeds() {
        let mut app = setup_app();

        let _fa = spawn_test_faction(&mut app, 1, "Faction A", 0.5, 0.5, 0.5);
        let _fb = spawn_test_faction(&mut app, 2, "Faction B", 0.5, 0.5, 0.5);
        let _actor = spawn_test_person(&mut app, 3, "Diplomat");

        queue_action(
            &mut app,
            Action {
                actor_id: 3,
                source: ActionSource::Player,
                kind: ActionKind::BrokerAlliance {
                    faction_a: 1,
                    faction_b: 2,
                },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0].outcome, ActionOutcome::Success { .. }));
    }

    // -----------------------------------------------------------------------
    // Test: Broker alliance fails for already allied factions
    // -----------------------------------------------------------------------

    #[test]
    fn broker_alliance_fails_if_allied() {
        let mut app = setup_app();

        let fa = spawn_test_faction(&mut app, 1, "Faction A", 0.5, 0.5, 0.5);
        let fb = spawn_test_faction(&mut app, 2, "Faction B", 0.5, 0.5, 0.5);
        let _actor = spawn_test_person(&mut app, 3, "Diplomat");

        // Pre-establish alliance
        let pair = RelationshipGraph::canonical_pair(fa, fb);
        app.world_mut()
            .resource_mut::<RelationshipGraph>()
            .allies
            .insert(pair, RelationshipMeta::new(SimTime::from_year(90)));

        queue_action(
            &mut app,
            Action {
                actor_id: 3,
                source: ActionSource::Player,
                kind: ActionKind::BrokerAlliance {
                    faction_a: 1,
                    faction_b: 2,
                },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0].outcome,
                ActionOutcome::Failed { reason } if reason.contains("already allied")
            ),
            "expected allied failure, got {:?}",
            results[0].outcome
        );
    }

    // -----------------------------------------------------------------------
    // Test: Seek office auto-succeeds for leaderless faction
    // -----------------------------------------------------------------------

    #[test]
    fn seek_office_leaderless_auto_succeeds() {
        let mut app = setup_app();

        let faction = spawn_test_faction(&mut app, 1, "Leaderless", 0.5, 0.5, 0.5);
        let actor = spawn_test_person(&mut app, 2, "Aspirant");
        app.world_mut().entity_mut(actor).insert(MemberOf(faction));

        queue_action(
            &mut app,
            Action {
                actor_id: 2,
                source: ActionSource::Autonomous,
                kind: ActionKind::SeekOffice { faction_id: 1 },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0].outcome, ActionOutcome::Success { .. }),
            "expected success for leaderless faction, got {:?}",
            results[0].outcome
        );
    }

    // -----------------------------------------------------------------------
    // Test: Defect fails for leader
    // -----------------------------------------------------------------------

    #[test]
    fn defect_fails_for_leader() {
        let mut app = setup_app();

        let from_faction = spawn_test_faction(&mut app, 1, "Old", 0.5, 0.5, 0.5);
        let _to_faction = spawn_test_faction(&mut app, 2, "New", 0.5, 0.5, 0.5);
        let actor = spawn_test_person(&mut app, 3, "Leader");
        app.world_mut()
            .entity_mut(actor)
            .insert((MemberOf(from_faction), LeaderOf(from_faction)));

        queue_action(
            &mut app,
            Action {
                actor_id: 3,
                source: ActionSource::Autonomous,
                kind: ActionKind::Defect {
                    from_faction: 1,
                    to_faction: 2,
                },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0].outcome,
                ActionOutcome::Failed { reason } if reason.contains("leaders cannot defect")
            ),
            "expected leader-defection failure, got {:?}",
            results[0].outcome
        );
    }

    // -----------------------------------------------------------------------
    // Test: Dead target returns failed
    // -----------------------------------------------------------------------

    #[test]
    fn dead_target_returns_failed() {
        let mut app = setup_app();

        let _actor = spawn_test_person(&mut app, 1, "Assassin");
        let target = spawn_test_person(&mut app, 2, "DeadGuy");
        // Mark as dead
        app.world_mut().get_mut::<SimEntity>(target).unwrap().end = Some(SimTime::from_year(90));

        queue_action(
            &mut app,
            Action {
                actor_id: 1,
                source: ActionSource::Player,
                kind: ActionKind::Assassinate { target_id: 2 },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0].outcome,
                ActionOutcome::Failed { reason } if reason.contains("not a living person")
            ),
            "expected dead-target failure, got {:?}",
            results[0].outcome
        );
    }

    // -----------------------------------------------------------------------
    // Test: Actions are cleared after processing
    // -----------------------------------------------------------------------

    #[test]
    fn actions_cleared_after_tick() {
        let mut app = setup_app();

        let _actor = spawn_test_person(&mut app, 1, "Actor");
        let _fa = spawn_test_faction(&mut app, 2, "Faction A", 0.5, 0.5, 0.5);
        let _fb = spawn_test_faction(&mut app, 3, "Faction B", 0.5, 0.5, 0.5);

        queue_action(
            &mut app,
            Action {
                actor_id: 1,
                source: ActionSource::Player,
                kind: ActionKind::SupportFaction { faction_id: 2 },
            },
        );
        queue_action(
            &mut app,
            Action {
                actor_id: 1,
                source: ActionSource::Player,
                kind: ActionKind::SupportFaction { faction_id: 3 },
            },
        );

        assert_eq!(
            app.world().resource::<PendingActions>().0.len(),
            2,
            "should have 2 pending actions"
        );

        tick(&mut app);

        assert!(
            app.world().resource::<PendingActions>().0.is_empty(),
            "pending actions should be drained after tick"
        );
        assert_eq!(get_results(&app).len(), 2, "should have 2 action results");
    }

    // -----------------------------------------------------------------------
    // Test: Press claim fails without claim
    // -----------------------------------------------------------------------

    #[test]
    fn press_claim_fails_without_claim() {
        let mut app = setup_app();

        let fa = spawn_test_faction(&mut app, 1, "Kingdom A", 0.5, 0.5, 0.5);
        let _fb = spawn_test_faction(&mut app, 2, "Kingdom B", 0.5, 0.5, 0.5);
        let actor = spawn_test_person(&mut app, 3, "King");
        app.world_mut()
            .entity_mut(actor)
            .insert((MemberOf(fa), LeaderOf(fa)));

        queue_action(
            &mut app,
            Action {
                actor_id: 3,
                source: ActionSource::Player,
                kind: ActionKind::PressClaim {
                    target_faction_id: 2,
                },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0].outcome,
                ActionOutcome::Failed { reason } if reason.contains("no valid claim")
            ),
            "expected no-claim failure, got {:?}",
            results[0].outcome
        );
    }

    // -----------------------------------------------------------------------
    // Test: Betray ally fails without alliance
    // -----------------------------------------------------------------------

    #[test]
    fn betray_ally_fails_without_alliance() {
        let mut app = setup_app();

        let fa = spawn_test_faction(&mut app, 1, "Kingdom A", 0.5, 0.5, 0.5);
        let _fb = spawn_test_faction(&mut app, 2, "Kingdom B", 0.5, 0.5, 0.5);
        let actor = spawn_test_person(&mut app, 3, "Leader");
        app.world_mut()
            .entity_mut(actor)
            .insert((MemberOf(fa), LeaderOf(fa)));

        queue_action(
            &mut app,
            Action {
                actor_id: 3,
                source: ActionSource::Player,
                kind: ActionKind::BetrayAlly { ally_faction_id: 2 },
            },
        );

        tick(&mut app);

        let results = get_results(&app);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0].outcome,
                ActionOutcome::Failed { reason } if reason.contains("no active alliance")
            ),
            "expected no-alliance failure, got {:?}",
            results[0].outcome
        );
    }
}
