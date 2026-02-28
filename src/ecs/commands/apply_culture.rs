use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::{CultureState, FactionCore, SettlementCulture};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::spawn;
use crate::model::cultural_value::{CulturalValue, NamingStyle};
use crate::model::effect::StateChange;

use super::applicator::ApplyCtx;

// -- Cultural Rebellion --
const REBELLION_FAILED_STABILITY_PENALTY: f64 = 0.10;
const REBELLION_CRACKDOWN_CULTURE_BOOST: f64 = 0.10;

/// Blend two cultures in a settlement, spawning a new culture entity.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_blend_cultures(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    parent_culture_a: u64,
    parent_culture_b: u64,
    new_name: &str,
    values: &[CulturalValue],
    naming_style: NamingStyle,
    resistance: f64,
) {
    // Spawn new culture entity
    let culture_id = ctx.id_gen.0.next_id();
    let _culture_entity = spawn::spawn_culture(
        world,
        culture_id,
        new_name.to_string(),
        Some(ctx.clock_time),
        CultureState {
            values: values.to_vec(),
            naming_style,
            resistance,
        },
    );
    ctx.entity_map.insert(culture_id, _culture_entity);

    // Update settlement culture makeup: remove parents, add blended
    if let Some(mut culture) = world.get_mut::<SettlementCulture>(settlement) {
        let share_a = culture
            .culture_makeup
            .remove(&parent_culture_a)
            .unwrap_or(0.0);
        let share_b = culture
            .culture_makeup
            .remove(&parent_culture_b)
            .unwrap_or(0.0);
        let combined = share_a + share_b;
        culture.culture_makeup.insert(culture_id, combined);

        // Normalize
        let total: f64 = culture.culture_makeup.values().sum();
        if total > 0.0 {
            for share in culture.culture_makeup.values_mut() {
                *share /= total;
            }
        }

        // Update dominant
        if let Some((&dom_id, _)) = culture
            .culture_makeup
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1))
        {
            culture.dominant_culture = Some(dom_id);
        }
    }

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "culture_makeup".to_string(),
            old_value: serde_json::json!("blended"),
            new_value: serde_json::json!(culture_id),
        },
    );
}

/// Shift a settlement's dominant culture.
pub(crate) fn apply_cultural_shift(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    new_culture: Entity,
) {
    let new_culture_sim_id = ctx.entity_map.get_sim(new_culture).unwrap_or(0);

    if let Some(mut culture) = world.get_mut::<SettlementCulture>(settlement) {
        let old = culture.dominant_culture;
        culture.dominant_culture = Some(new_culture_sim_id);

        ctx.record_effect(
            event_id,
            settlement,
            StateChange::PropertyChanged {
                field: "dominant_culture".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(new_culture_sim_id),
            },
        );
    }
}

/// Process a cultural rebellion in a settlement.
pub(crate) fn apply_cultural_rebellion(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    settlement: Entity,
    _rebel_culture: u64,
    succeeded: bool,
    _new_faction_name: &Option<String>,
) {
    if !succeeded {
        // Failed: reduce stability, crackdown on rebel culture
        if let Some(member_of) = world.get::<crate::ecs::relationships::MemberOf>(settlement) {
            let faction = member_of.0;
            if let Some(mut core) = world.get_mut::<FactionCore>(faction) {
                core.stability = (core.stability - REBELLION_FAILED_STABILITY_PENALTY).max(0.0);
            }

            // Crackdown: boost ruling culture share
            let primary_culture = world
                .get::<FactionCore>(faction)
                .and_then(|c| c.primary_culture);
            if let Some(ruling_id) = primary_culture
                && let Some(mut culture) = world.get_mut::<SettlementCulture>(settlement)
            {
                *culture.culture_makeup.entry(ruling_id).or_insert(0.0) += REBELLION_CRACKDOWN_CULTURE_BOOST;
                // Normalize
                let total: f64 = culture.culture_makeup.values().sum();
                if total > 0.0 {
                    for share in culture.culture_makeup.values_mut() {
                        *share /= total;
                    }
                }
            }
        }
    }
    // If succeeded, the faction split would be handled by a separate SplitFaction command

    ctx.record_effect(
        event_id,
        settlement,
        StateChange::PropertyChanged {
            field: "cultural_rebellion".to_string(),
            old_value: serde_json::json!(null),
            new_value: serde_json::json!(succeeded),
        },
    );

    ctx.emit(SimReactiveEvent::CulturalRebellion {
        event_id,
        settlement,
    });
}
