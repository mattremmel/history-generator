use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::{
    DeityState, FactionCore, ReligionState, SettlementCore, SettlementCulture,
};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::LocatedIn;
use crate::ecs::spawn;
use crate::model::effect::StateChange;
use crate::model::entity_data::{DeityDomain, ReligiousTenet};

use super::applicator::ApplyCtx;

// -- Religion founding --
const FOUNDED_FERVOR: f64 = 0.5;
const FOUNDED_PROSELYTISM: f64 = 0.5;
const FOUNDED_ORTHODOXY: f64 = 0.5;
const FOUNDED_WORSHIP_STRENGTH: f64 = 0.5;

// -- Schism --
const SCHISM_FERVOR_BOOST: f64 = 0.1;
const SCHISM_ORTHODOXY_MULT: f64 = 0.8;
const SCHISM_DEFAULT_FERVOR: f64 = 0.6;
const SCHISM_DEFAULT_PROSELYTISM: f64 = 0.5;
const SCHISM_DEFAULT_ORTHODOXY: f64 = 0.4;
const SCHISM_SHARE_TRANSFER_FRAC: f64 = 0.3;

/// Found a new religion: spawn Religion + Deity entities.
pub(crate) fn apply_found_religion(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    founder: Entity,
    name: &str,
) {
    let religion_id = ctx.id_gen.0.next_id();
    let religion_entity = spawn::spawn_religion(
        world,
        religion_id,
        name.to_string(),
        Some(ctx.clock_time),
        ReligionState {
            fervor: FOUNDED_FERVOR,
            proselytism: FOUNDED_PROSELYTISM,
            orthodoxy: FOUNDED_ORTHODOXY,
            tenets: Vec::new(),
        },
    );
    ctx.entity_map.insert(religion_id, religion_entity);

    // Spawn a deity for this religion
    let deity_id = ctx.id_gen.0.next_id();
    let deity_name = format!("{name} God");
    let deity_entity = spawn::spawn_deity(
        world,
        deity_id,
        deity_name,
        Some(ctx.clock_time),
        DeityState {
            domain: DeityDomain::Sky,
            worship_strength: FOUNDED_WORSHIP_STRENGTH,
        },
    );
    ctx.entity_map.insert(deity_id, deity_entity);

    ctx.record_effect(
        event_id,
        founder,
        StateChange::PropertyChanged {
            field: "religion_founded".to_string(),
            old_value: serde_json::json!(null),
            new_value: serde_json::json!(religion_id),
        },
    );

    // Resolve founder's settlement for the event
    let founder_settlement = world.get::<LocatedIn>(founder).map(|l| l.0).unwrap_or(founder);

    ctx.emit(SimReactiveEvent::ReligionFounded {
        event_id,
        religion: religion_entity,
        settlement: founder_settlement,
    });
}

/// Religious schism: spawn new Religion + Deity, redistribute shares.
pub(crate) fn apply_religious_schism(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    parent_religion: Entity,
    settlement: Entity,
    new_name: &str,
    tenets: &[ReligiousTenet],
) {
    let parent_sim_id = ctx.entity_map.get_sim(parent_religion).unwrap_or(0);

    // Get parent religion stats for the new religion
    let parent_state = world.get::<ReligionState>(parent_religion).cloned();

    let new_state = if let Some(ps) = parent_state {
        ReligionState {
            fervor: (ps.fervor + SCHISM_FERVOR_BOOST).min(1.0),
            proselytism: ps.proselytism,
            orthodoxy: ps.orthodoxy * SCHISM_ORTHODOXY_MULT,
            tenets: tenets.to_vec(),
        }
    } else {
        ReligionState {
            fervor: SCHISM_DEFAULT_FERVOR,
            proselytism: SCHISM_DEFAULT_PROSELYTISM,
            orthodoxy: SCHISM_DEFAULT_ORTHODOXY,
            tenets: tenets.to_vec(),
        }
    };

    // Spawn new religion
    let new_religion_id = ctx.id_gen.0.next_id();
    let new_religion_entity = spawn::spawn_religion(
        world,
        new_religion_id,
        new_name.to_string(),
        Some(ctx.clock_time),
        new_state,
    );
    ctx.entity_map.insert(new_religion_id, new_religion_entity);

    // Spawn deity for new religion
    let deity_id = ctx.id_gen.0.next_id();
    let deity_entity = spawn::spawn_deity(
        world,
        deity_id,
        format!("{new_name} Deity"),
        Some(ctx.clock_time),
        DeityState {
            domain: DeityDomain::Sky,
            worship_strength: FOUNDED_WORSHIP_STRENGTH,
        },
    );
    ctx.entity_map.insert(deity_id, deity_entity);

    // Transfer parent's share to new religion in settlement
    if let Some(mut culture) = world.get_mut::<SettlementCulture>(settlement) {
        let parent_share = culture
            .religion_makeup
            .get(&parent_sim_id)
            .copied()
            .unwrap_or(0.0);
        let transfer = parent_share * SCHISM_SHARE_TRANSFER_FRAC;

        if let Some(share) = culture.religion_makeup.get_mut(&parent_sim_id) {
            *share -= transfer;
        }
        culture.religion_makeup.insert(new_religion_id, transfer);

        // Normalize
        let total: f64 = culture.religion_makeup.values().sum();
        if total > 0.0 {
            for share in culture.religion_makeup.values_mut() {
                *share /= total;
            }
        }

        // Update dominant religion and tension
        if let Some((&dom_id, &dom_share)) = culture
            .religion_makeup
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1))
        {
            culture.dominant_religion = Some(dom_id);
            culture.religious_tension = 1.0 - dom_share;
        }
    }

    ctx.record_effect(
        event_id,
        parent_religion,
        StateChange::PropertyChanged {
            field: "religion_schism".to_string(),
            old_value: serde_json::json!(parent_sim_id),
            new_value: serde_json::json!(new_religion_id),
        },
    );

    ctx.emit(SimReactiveEvent::ReligionSchism {
        event_id,
        parent_religion,
        new_religion: new_religion_entity,
        settlement,
    });
}

/// Convert a faction's primary religion.
pub(crate) fn apply_convert_faction(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    faction: Entity,
    religion: Entity,
) {
    let religion_sim_id = ctx.entity_map.get_sim(religion).unwrap_or(0);

    if let Some(mut core) = world.get_mut::<FactionCore>(faction) {
        let old = core.primary_religion;
        core.primary_religion = Some(religion_sim_id);
        ctx.record_effect(
            event_id,
            faction,
            StateChange::PropertyChanged {
                field: "primary_religion".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(religion_sim_id),
            },
        );
    }
}

/// Declare a prophecy at a settlement.
pub(crate) fn apply_declare_prophecy(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    _religion: u64,
    prophet: Option<Entity>,
) {
    // Update settlement's last prophecy year
    if let Some(mut core) = world.get_mut::<SettlementCore>(settlement) {
        core.last_prophecy_year = Some(ctx.clock_time.year());
    }

    // Find deity entity for the reactive event (use a placeholder for now)
    let deity = prophet.unwrap_or(settlement);
    ctx.emit(SimReactiveEvent::ProphecyDeclared { event_id, deity });
}

/// Spread a religion to a settlement by adding share and normalizing.
pub(crate) fn apply_spread_religion(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    religion: u64,
    share: f64,
) {
    if let Some(mut culture) = world.get_mut::<SettlementCulture>(settlement) {
        *culture.religion_makeup.entry(religion).or_insert(0.0) += share;

        // Normalize
        let total: f64 = culture.religion_makeup.values().sum();
        if total > 0.0 {
            for s in culture.religion_makeup.values_mut() {
                *s /= total;
            }
        }

        // Update dominant religion and tension
        if let Some((&dom_id, &dom_share)) = culture
            .religion_makeup
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1))
        {
            culture.dominant_religion = Some(dom_id);
            culture.religious_tension = 1.0 - dom_share;
        }
    }

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "religion_makeup".to_string(),
            old_value: serde_json::json!(null),
            new_value: serde_json::json!(religion),
        },
    );
}
