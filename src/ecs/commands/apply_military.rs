use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use rand::Rng;

use crate::ecs::components::common::SimEntity;
use crate::ecs::components::{
    ArmyState, FactionCore, FactionDiplomacy, FactionMilitary, PersonCore, PersonEducation,
    PersonReputation, PersonSocial,
};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{
    HiredBy, LocatedIn, MemberOf, RelationshipGraph, RelationshipMeta,
};
use crate::ecs::spawn;
use crate::model::Sex;
use crate::model::effect::StateChange;
use crate::model::entity_data::{GovernmentType, Role};
use crate::model::relationship::RelationshipKind;
use crate::model::traits::Trait;

use super::applicator::ApplyCtx;

/// Declare war: insert AtWar into RelationshipGraph, emit WarStarted.
pub(crate) fn apply_declare_war(
    ctx: &mut ApplyCtx,
    event_id: u64,
    attacker: Entity,
    defender: Entity,
) {
    let pair = RelationshipGraph::canonical_pair(attacker, defender);
    if !ctx.rel_graph.are_at_war(attacker, defender) {
        ctx.rel_graph
            .at_war
            .insert(pair, RelationshipMeta::new(ctx.clock_time));
    }

    let attacker_sim = ctx.entity_map.get_sim(attacker).unwrap_or(0);
    let defender_sim = ctx.entity_map.get_sim(defender).unwrap_or(0);

    ctx.record_effect(
        event_id,
        attacker,
        StateChange::RelationshipStarted {
            target_entity_id: defender_sim,
            kind: RelationshipKind::AtWar,
        },
    );
    ctx.record_effect(
        event_id,
        defender,
        StateChange::RelationshipStarted {
            target_entity_id: attacker_sim,
            kind: RelationshipKind::AtWar,
        },
    );

    ctx.emit(SimReactiveEvent::WarStarted {
        event_id,
        attacker,
        defender,
    });
}

/// Capture settlement: change MemberOf from old faction to new, emit SettlementCaptured.
pub(crate) fn apply_capture_settlement(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    new_faction: Entity,
) {
    let old_faction = world.get::<MemberOf>(settlement).map(|m| m.0);

    // Update the MemberOf relationship
    world.entity_mut(settlement).insert(MemberOf(new_faction));

    let settlement_sim = ctx.entity_map.get_sim(settlement).unwrap_or(0);
    let new_faction_sim = ctx.entity_map.get_sim(new_faction).unwrap_or(0);

    // Record effect: old membership ended
    if let Some(old) = old_faction {
        let old_sim = ctx.entity_map.get_sim(old).unwrap_or(0);
        ctx.record_effect(
            event_id,
            settlement,
            StateChange::RelationshipEnded {
                target_entity_id: old_sim,
                kind: RelationshipKind::MemberOf,
            },
        );
    }

    // Record effect: new membership started
    ctx.record_effect(
        event_id,
        settlement,
        StateChange::RelationshipStarted {
            target_entity_id: new_faction_sim,
            kind: RelationshipKind::MemberOf,
        },
    );

    ctx.emit(SimReactiveEvent::SettlementCaptured {
        event_id,
        settlement,
        old_faction: old_faction.unwrap_or(settlement), // fallback if no previous faction
        new_faction,
    });

    // Suppress unused variable warnings
    let _ = settlement_sim;
}

/// Muster army: spawn an Army entity in the given region, belonging to the faction.
pub(crate) fn apply_muster_army(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    faction: Entity,
    region: Entity,
) {
    let faction_sim = ctx.entity_map.get_sim(faction).unwrap_or(0);
    let region_sim = ctx.entity_map.get_sim(region).unwrap_or(0);
    let faction_name = world
        .get::<SimEntity>(faction)
        .map(|s| s.name.clone())
        .unwrap_or_default();

    let army_id = ctx.id_gen.0.next_id();
    let army_entity = spawn::spawn_army(
        world,
        army_id,
        format!("{faction_name} Army"),
        Some(ctx.clock_time),
        ArmyState {
            faction_id: faction_sim,
            home_region_id: region_sim,
            morale: 1.0,
            supply: 3.0,
            ..ArmyState::default()
        },
    );
    ctx.entity_map.insert(army_id, army_entity);
    world.entity_mut(army_entity).insert(LocatedIn(region));

    ctx.record_effect(
        event_id,
        army_entity,
        StateChange::EntityCreated {
            kind: crate::model::EntityKind::Army,
            name: format!("{faction_name} Army"),
        },
    );
}

/// March army: update LocatedIn to target region.
pub(crate) fn apply_march_army(
    _ctx: &mut ApplyCtx,
    world: &mut World,
    _event_id: u64,
    army: Entity,
    target_region: Entity,
) {
    world.entity_mut(army).insert(LocatedIn(target_region));
}

/// Resolve battle: update army strength/morale based on casualties.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_resolve_battle(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    attacker_army: Entity,
    defender_army: Entity,
    attacker_casualties: u32,
    defender_casualties: u32,
    attacker_won: bool,
) {
    // Apply casualties to attacker
    if let Some(mut state) = world.get_mut::<ArmyState>(attacker_army) {
        let old_strength = state.strength;
        state.strength = state.strength.saturating_sub(attacker_casualties);
        if attacker_won {
            state.morale = (state.morale * 1.1).clamp(0.0, 1.0);
        } else {
            state.morale = (state.morale * 0.7).clamp(0.0, 1.0);
        }
        ctx.record_effect(
            event_id,
            attacker_army,
            StateChange::PropertyChanged {
                field: "strength".to_string(),
                old_value: serde_json::json!(old_strength),
                new_value: serde_json::json!(state.strength),
            },
        );
    }

    // Apply casualties to defender
    if let Some(mut state) = world.get_mut::<ArmyState>(defender_army) {
        let old_strength = state.strength;
        state.strength = state.strength.saturating_sub(defender_casualties);
        if !attacker_won {
            state.morale = (state.morale * 1.1).clamp(0.0, 1.0);
        } else {
            state.morale = (state.morale * 0.7).clamp(0.0, 1.0);
        }
        ctx.record_effect(
            event_id,
            defender_army,
            StateChange::PropertyChanged {
                field: "strength".to_string(),
                old_value: serde_json::json!(old_strength),
                new_value: serde_json::json!(state.strength),
            },
        );
    }

    // End army if destroyed (strength == 0)
    for army in [attacker_army, defender_army] {
        if let Some(state) = world.get::<ArmyState>(army)
            && state.strength == 0
            && let Some(mut sim) = world.get_mut::<SimEntity>(army)
            && sim.end.is_none()
        {
            sim.end = Some(ctx.clock_time);
            ctx.record_effect(event_id, army, StateChange::EntityEnded);
        }
    }
}

/// Begin siege: insert EcsActiveSiege component on settlement, emit SiegeStarted.
pub(crate) fn apply_begin_siege(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    army: Entity,
    settlement: Entity,
) {
    use crate::ecs::components::dynamic::EcsActiveSiege;

    let attacker_army_id = ctx.entity_map.get_sim(army).unwrap_or(0);
    let attacker_faction_id = world
        .get::<ArmyState>(army)
        .map(|s| s.faction_id)
        .unwrap_or(0);

    // Mark army as besieging
    if let Some(mut state) = world.get_mut::<ArmyState>(army) {
        state.besieging_settlement_id = Some(ctx.entity_map.get_sim(settlement).unwrap_or(0));
    }

    world.entity_mut(settlement).insert(EcsActiveSiege {
        attacker_army_id,
        attacker_faction_id,
        started: ctx.clock_time,
        months_elapsed: 0,
        civilian_deaths: 0,
    });

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "siege".to_string(),
            old_value: serde_json::json!(null),
            new_value: serde_json::json!("under_siege"),
        },
    );

    let attacker_entity = ctx.entity_map.get_bevy(attacker_faction_id).unwrap_or(army);
    ctx.emit(SimReactiveEvent::SiegeStarted {
        event_id,
        settlement,
        attacker: attacker_entity,
    });
}

/// Resolve assault: apply siege outcome, remove EcsActiveSiege if succeeded.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_resolve_assault(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    army: Entity,
    settlement: Entity,
    succeeded: bool,
    attacker_casualties: u32,
    _defender_casualties: u32,
) {
    use crate::ecs::components::dynamic::EcsActiveSiege;

    // Apply attacker casualties
    if let Some(mut state) = world.get_mut::<ArmyState>(army) {
        state.strength = state.strength.saturating_sub(attacker_casualties);
        if !succeeded {
            state.morale = (state.morale - 0.1).max(0.0);
        }
    }

    if succeeded {
        // Remove siege component
        world.entity_mut(settlement).remove::<EcsActiveSiege>();

        // Clear besieging field on army
        if let Some(mut state) = world.get_mut::<ArmyState>(army) {
            state.besieging_settlement_id = None;
        }

        let defender_faction = world.get::<MemberOf>(settlement).map(|m| m.0);
        ctx.emit(SimReactiveEvent::SiegeEnded {
            event_id,
            settlement,
            defender_faction: defender_faction.unwrap_or(settlement),
        });
    }
}

/// Sign treaty: end AtWar in RelationshipGraph, emit WarEnded (enriched).
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_sign_treaty(
    ctx: &mut ApplyCtx,
    _world: &mut World,
    event_id: u64,
    faction_a: Entity,
    faction_b: Entity,
    winner: Entity,
    loser: Entity,
    decisive: bool,
) {
    // End the AtWar relationship
    let pair = RelationshipGraph::canonical_pair(faction_a, faction_b);
    if let Some(meta) = ctx.rel_graph.at_war.get_mut(&pair) {
        meta.end = Some(ctx.clock_time);
    }

    let a_sim = ctx.entity_map.get_sim(faction_a).unwrap_or(0);
    let b_sim = ctx.entity_map.get_sim(faction_b).unwrap_or(0);

    ctx.record_effect(
        event_id,
        faction_a,
        StateChange::RelationshipEnded {
            target_entity_id: b_sim,
            kind: RelationshipKind::AtWar,
        },
    );
    ctx.record_effect(
        event_id,
        faction_b,
        StateChange::RelationshipEnded {
            target_entity_id: a_sim,
            kind: RelationshipKind::AtWar,
        },
    );

    ctx.emit(SimReactiveEvent::WarEnded {
        event_id,
        winner,
        loser,
        decisive,
    });
}

/// Disband army: end the army entity.
pub(crate) fn apply_disband_army(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    army: Entity,
) {
    if let Some(mut sim) = world.get_mut::<SimEntity>(army)
        && sim.end.is_none()
    {
        sim.end = Some(ctx.clock_time);
        ctx.record_effect(event_id, army, StateChange::EntityEnded);
    }
}

/// Create mercenary company: spawn faction (MercenaryCompany) + leader + army.
pub(crate) fn apply_create_mercenary_company(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    region: Entity,
    strength: u32,
    name: String,
    rng: &mut dyn rand::RngCore,
) {
    let region_sim = ctx.entity_map.get_sim(region).unwrap_or(0);

    // Spawn faction
    let faction_id = ctx.id_gen.0.next_id();
    let faction_entity = spawn::spawn_faction(
        world,
        faction_id,
        name.clone(),
        Some(ctx.clock_time),
        FactionCore {
            government_type: GovernmentType::MercenaryCompany,
            stability: 0.6,
            happiness: 0.5,
            legitimacy: 0.0,
            treasury: 0.0,
            ..FactionCore::default()
        },
        FactionDiplomacy::default(),
        FactionMilitary::default(),
    );
    ctx.entity_map.insert(faction_id, faction_entity);

    // Spawn leader
    let leader_id = ctx.id_gen.0.next_id();
    let leader_entity = spawn::spawn_person(
        world,
        leader_id,
        format!("{name} Captain"),
        Some(ctx.clock_time),
        PersonCore {
            born: ctx.clock_time,
            sex: if rng.random_bool(0.5) {
                Sex::Male
            } else {
                Sex::Female
            },
            role: Role::Warrior,
            traits: vec![Trait::Aggressive],
            ..PersonCore::default()
        },
        PersonReputation::default(),
        PersonSocial::default(),
        PersonEducation::default(),
    );
    ctx.entity_map.insert(leader_id, leader_entity);
    world
        .entity_mut(leader_entity)
        .insert(MemberOf(faction_entity));

    // Spawn army
    let army_id = ctx.id_gen.0.next_id();
    let army_entity = spawn::spawn_army(
        world,
        army_id,
        format!("{name} Company"),
        Some(ctx.clock_time),
        ArmyState {
            strength,
            faction_id,
            home_region_id: region_sim,
            morale: 0.8,
            supply: 3.0,
            is_mercenary: true,
            ..ArmyState::default()
        },
    );
    ctx.entity_map.insert(army_id, army_entity);
    world.entity_mut(army_entity).insert(LocatedIn(region));

    ctx.record_effect(
        event_id,
        faction_entity,
        StateChange::EntityCreated {
            kind: crate::model::EntityKind::Faction,
            name,
        },
    );
}

/// Hire mercenary: add HiredBy relationship from mercenary faction to employer.
pub(crate) fn apply_hire_mercenary(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    employer: Entity,
    mercenary: Entity,
    wage: f64,
) {
    world.entity_mut(mercenary).insert(HiredBy(employer));

    // Set wage on mercenary faction
    if let Some(mut mil) = world.get_mut::<FactionMilitary>(mercenary) {
        mil.mercenary_wage = wage;
    }

    let employer_sim = ctx.entity_map.get_sim(employer).unwrap_or(0);
    ctx.record_effect(
        event_id,
        mercenary,
        StateChange::RelationshipStarted {
            target_entity_id: employer_sim,
            kind: RelationshipKind::HiredBy,
        },
    );
}

/// End mercenary contract: remove HiredBy relationship.
pub(crate) fn apply_end_mercenary_contract(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    mercenary: Entity,
) {
    let employer = world.get::<HiredBy>(mercenary).map(|h| h.0);
    world.entity_mut(mercenary).remove::<HiredBy>();

    if let Some(emp) = employer {
        let employer_sim = ctx.entity_map.get_sim(emp).unwrap_or(0);
        ctx.record_effect(
            event_id,
            mercenary,
            StateChange::RelationshipEnded {
                target_entity_id: employer_sim,
                kind: RelationshipKind::HiredBy,
            },
        );
    }

    // Reset wage
    if let Some(mut mil) = world.get_mut::<FactionMilitary>(mercenary) {
        mil.mercenary_wage = 0.0;
        mil.unpaid_months = 0;
    }
}
