use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::{FactionCore, PersonReputation, SettlementCore};
use crate::model::effect::StateChange;

use super::applicator::ApplyCtx;

/// Adjust prestige on any entity (person, faction, or settlement).
pub(crate) fn apply_adjust_prestige(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    entity: Entity,
    delta: f64,
) {
    // Try Person first
    if let Some(mut rep) = world.get_mut::<PersonReputation>(entity) {
        let old = rep.prestige;
        rep.prestige = (rep.prestige + delta).clamp(0.0, 1.0);
        ctx.record_effect(
            event_id,
            entity,
            StateChange::PropertyChanged {
                field: "prestige".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(rep.prestige),
            },
        );
        return;
    }

    // Try Faction
    if let Some(mut core) = world.get_mut::<FactionCore>(entity) {
        let old = core.prestige;
        core.prestige = (core.prestige + delta).clamp(0.0, 1.0);
        ctx.record_effect(
            event_id,
            entity,
            StateChange::PropertyChanged {
                field: "prestige".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(core.prestige),
            },
        );
        return;
    }

    // Try Settlement
    if let Some(mut core) = world.get_mut::<SettlementCore>(entity) {
        let old = core.prestige;
        core.prestige = (core.prestige + delta).clamp(0.0, 1.0);
        ctx.record_effect(
            event_id,
            entity,
            StateChange::PropertyChanged {
                field: "prestige".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(core.prestige),
            },
        );
    }
}

/// Update the prestige tier on any entity.
pub(crate) fn apply_update_prestige_tier(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    entity: Entity,
    new_tier: u8,
) {
    if let Some(mut rep) = world.get_mut::<PersonReputation>(entity) {
        let old = rep.prestige_tier;
        rep.prestige_tier = new_tier;
        ctx.record_effect(
            event_id,
            entity,
            StateChange::PropertyChanged {
                field: "prestige_tier".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(new_tier),
            },
        );
        return;
    }

    if let Some(mut core) = world.get_mut::<FactionCore>(entity) {
        let old = core.prestige_tier;
        core.prestige_tier = new_tier;
        ctx.record_effect(
            event_id,
            entity,
            StateChange::PropertyChanged {
                field: "prestige_tier".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(new_tier),
            },
        );
        return;
    }

    if let Some(mut core) = world.get_mut::<SettlementCore>(entity) {
        let old = core.prestige_tier;
        core.prestige_tier = new_tier;
        ctx.record_effect(
            event_id,
            entity,
            StateChange::PropertyChanged {
                field: "prestige_tier".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(new_tier),
            },
        );
    }
}
