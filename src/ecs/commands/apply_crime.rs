use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use rand::Rng;

use crate::ecs::components::common::SimEntity;
use crate::ecs::components::{
    ArmyState, FactionCore, FactionDiplomacy, FactionMilitary, PersonCore, PersonEducation,
    PersonReputation, PersonSocial, SettlementCore,
};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LeaderOf, LocatedIn, MemberOf};
use crate::ecs::spawn;
use crate::model::Sex;
use crate::model::effect::StateChange;
use crate::model::entity_data::{GovernmentType, Role};
use crate::model::traits::Trait;

use super::applicator::ApplyCtx;

// -- Bandit Raid --
const BANDIT_RAID_POP_LOSS_FRAC: f64 = 0.02;
const BANDIT_RAID_TREASURY_THEFT_FRAC: f64 = 0.1;
const BANDIT_RAID_TREASURY_THEFT_CAP: f64 = 5.0;

// ---------------------------------------------------------------------------
// Bandit name generation
// ---------------------------------------------------------------------------

const BANDIT_PREFIXES: &[&str] = &[
    "Shadow", "Black", "Blood", "Iron", "Red", "Gray", "Dark", "Storm", "Bone", "Ash",
];
const BANDIT_SUFFIXES: &[&str] = &[
    "Fangs",
    "Blades",
    "Brotherhood",
    "Wolves",
    "Marauders",
    "Reavers",
    "Claws",
    "Raiders",
    "Daggers",
    "Serpents",
];

fn generate_bandit_name(rng: &mut dyn rand::RngCore) -> String {
    let prefix = BANDIT_PREFIXES[rng.random_range(0..BANDIT_PREFIXES.len())];
    let suffix = BANDIT_SUFFIXES[rng.random_range(0..BANDIT_SUFFIXES.len())];
    format!("The {prefix} {suffix}")
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BANDIT_MIN_STRENGTH: u32 = 15;
const BANDIT_MAX_STRENGTH: u32 = 30;

/// Form a bandit gang in the given region.
/// Spawns: Faction (BanditClan) + Person (leader) + Army.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_form_bandit_gang(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    region: Entity,
    rng: &mut dyn rand::RngCore,
) {
    let gang_name = generate_bandit_name(rng);

    // Spawn faction
    let faction_id = ctx.id_gen.0.next_id();
    let faction_entity = spawn::spawn_faction(
        world,
        faction_id,
        gang_name.clone(),
        Some(ctx.clock_time),
        FactionCore {
            government_type: GovernmentType::BanditClan,
            stability: 0.5,
            happiness: 0.3,
            legitimacy: 0.0,
            treasury: 0.0,
            ..FactionCore::default()
        },
        FactionDiplomacy::default(),
        FactionMilitary::default(),
    );

    // Register in entity map (already done by spawn)
    ctx.entity_map.insert(faction_id, faction_entity);

    // Spawn leader
    let leader_id = ctx.id_gen.0.next_id();
    let leader_entity = spawn::spawn_person(
        world,
        leader_id,
        format!("{gang_name} Leader"),
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

    // Set up leader relationships
    world.entity_mut(leader_entity).insert((
        MemberOf(faction_entity),
        LeaderOf(faction_entity),
        LocatedIn(region),
    ));

    // Spawn army
    let army_id = ctx.id_gen.0.next_id();
    let strength = rng.random_range(BANDIT_MIN_STRENGTH..=BANDIT_MAX_STRENGTH);
    let army_entity = spawn::spawn_army(
        world,
        army_id,
        format!("{gang_name} Warband"),
        Some(ctx.clock_time),
        ArmyState {
            strength,
            faction_id,
            home_region_id: ctx.entity_map.get_sim(region).unwrap_or(0),
            morale: 0.8,
            supply: 3.0,
            ..ArmyState::default()
        },
    );
    ctx.entity_map.insert(army_id, army_entity);

    // Place army in region
    world.entity_mut(army_entity).insert(LocatedIn(region));

    ctx.record_effect(
        event_id,
        faction_entity,
        StateChange::EntityCreated {
            kind: crate::model::EntityKind::Faction,
            name: gang_name,
        },
    );

    ctx.emit(SimReactiveEvent::BanditGangFormed { event_id, region });
}

/// Raid a settlement: reduce population and steal treasury.
pub(crate) fn apply_bandit_raid(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
) {
    if let Some(mut core) = world.get_mut::<SettlementCore>(settlement) {
        // Population loss: 1-3% (use midpoint 2%)
        let pop_loss = (core.population as f64 * BANDIT_RAID_POP_LOSS_FRAC) as u32;
        let old_pop = core.population;
        core.population = core.population.saturating_sub(pop_loss);
        let new_pop = core.population;
        core.population_breakdown.scale_to(new_pop);

        ctx.record_effect(
            event_id,
            settlement,
            StateChange::PropertyChanged {
                field: "population".to_string(),
                old_value: serde_json::json!(old_pop),
                new_value: serde_json::json!(new_pop),
            },
        );

        // Treasury theft
        let theft =
            (core.treasury * BANDIT_RAID_TREASURY_THEFT_FRAC).min(BANDIT_RAID_TREASURY_THEFT_CAP);
        let old_treasury = core.treasury;
        core.treasury = (core.treasury - theft).max(0.0);
        ctx.record_effect(
            event_id,
            settlement,
            StateChange::PropertyChanged {
                field: "treasury".to_string(),
                old_value: serde_json::json!(old_treasury),
                new_value: serde_json::json!(core.treasury),
            },
        );
    }

    ctx.emit(SimReactiveEvent::BanditRaid {
        event_id,
        settlement,
    });
}

/// Raid a trade route, optionally severing it.
pub(crate) fn apply_raid_trade_route(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    _bandit_faction: Entity,
    settlement_a: Entity,
    settlement_b: Entity,
    sever: bool,
) {
    if sever {
        // Sever by removing from relationship graph + settlement components
        super::apply_economy::apply_sever_trade_route(
            ctx,
            world,
            event_id,
            settlement_a,
            settlement_b,
        );
    } else {
        // Just emit the raided event
        ctx.emit(SimReactiveEvent::TradeRouteRaided {
            event_id,
            settlement_a,
            settlement_b,
        });
    }
}

/// Disband a bandit gang: end faction, army, and leader entities.
pub(crate) fn apply_disband_bandit_gang(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    faction: Entity,
) {
    // End the faction entity
    if let Some(mut sim) = world.get_mut::<SimEntity>(faction)
        && sim.end.is_none()
    {
        sim.end = Some(ctx.clock_time);
        ctx.record_effect(event_id, faction, StateChange::EntityEnded);
    }

    // Find and end all members (persons with MemberOf this faction)
    // Collect first to avoid borrow issues
    let members: Vec<Entity> = world
        .query::<(Entity, &MemberOf)>()
        .iter(world)
        .filter(|(_, m)| m.0 == faction)
        .map(|(e, _)| e)
        .collect();

    for member in members {
        if let Some(mut sim) = world.get_mut::<SimEntity>(member)
            && sim.end.is_none()
        {
            sim.end = Some(ctx.clock_time);
            ctx.record_effect(event_id, member, StateChange::EntityEnded);
        }
        world.entity_mut(member).remove::<MemberOf>();
    }

    // Find and end armies belonging to this faction
    let armies: Vec<Entity> = world
        .query::<(Entity, &ArmyState)>()
        .iter(world)
        .filter(|(_, a)| {
            ctx.entity_map
                .get_bevy(a.faction_id)
                .is_some_and(|f| f == faction)
        })
        .map(|(e, _)| e)
        .collect();

    for army in armies {
        if let Some(mut sim) = world.get_mut::<SimEntity>(army)
            && sim.end.is_none()
        {
            sim.end = Some(ctx.clock_time);
            ctx.record_effect(event_id, army, StateChange::EntityEnded);
        }
        world.entity_mut(army).remove::<LocatedIn>();
    }
}
