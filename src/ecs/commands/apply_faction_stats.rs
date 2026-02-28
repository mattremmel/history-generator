use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::{FactionCore, FactionDiplomacy};
use crate::model::WarGoal;
use crate::model::effect::StateChange;

use super::applicator::ApplyCtx;

/// Apply delta adjustments to faction stats (stability, happiness, legitimacy, trust, prestige).
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_adjust_faction_stats(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    faction: Entity,
    stability_delta: f64,
    happiness_delta: f64,
    legitimacy_delta: f64,
    trust_delta: f64,
    prestige_delta: f64,
) {
    if let Some(mut core) = world.get_mut::<FactionCore>(faction) {
        if stability_delta != 0.0 {
            let old = core.stability;
            core.stability = (core.stability + stability_delta).clamp(0.0, 1.0);
            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "stability".to_string(),
                    old_value: serde_json::json!(old),
                    new_value: serde_json::json!(core.stability),
                },
            );
        }
        if happiness_delta != 0.0 {
            let old = core.happiness;
            core.happiness = (core.happiness + happiness_delta).clamp(0.0, 1.0);
            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "happiness".to_string(),
                    old_value: serde_json::json!(old),
                    new_value: serde_json::json!(core.happiness),
                },
            );
        }
        if legitimacy_delta != 0.0 {
            let old = core.legitimacy;
            core.legitimacy = (core.legitimacy + legitimacy_delta).clamp(0.0, 1.0);
            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "legitimacy".to_string(),
                    old_value: serde_json::json!(old),
                    new_value: serde_json::json!(core.legitimacy),
                },
            );
        }
        if prestige_delta != 0.0 {
            let old = core.prestige;
            core.prestige = (core.prestige + prestige_delta).clamp(0.0, 1.0);
            ctx.record_effect(
                event_id,
                faction,
                StateChange::PropertyChanged {
                    field: "prestige".to_string(),
                    old_value: serde_json::json!(old),
                    new_value: serde_json::json!(core.prestige),
                },
            );
        }
    }

    if trust_delta != 0.0
        && let Some(mut diplomacy) = world.get_mut::<FactionDiplomacy>(faction)
    {
        let old = diplomacy.diplomatic_trust;
        diplomacy.diplomatic_trust = (diplomacy.diplomatic_trust + trust_delta).clamp(0.0, 1.0);
        ctx.record_effect(
            event_id,
            faction,
            StateChange::PropertyChanged {
                field: "diplomatic_trust".to_string(),
                old_value: serde_json::json!(old),
                new_value: serde_json::json!(diplomacy.diplomatic_trust),
            },
        );
    }
}

/// Set a war goal on a faction's diplomacy for a target faction.
pub(crate) fn apply_set_war_goal(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    faction: Entity,
    target_faction: Entity,
    goal: &WarGoal,
) {
    let target_sim = ctx.entity_map.get_sim(target_faction).unwrap_or(0);

    if let Some(mut diplomacy) = world.get_mut::<FactionDiplomacy>(faction) {
        diplomacy.war_goals.insert(target_sim, goal.clone());
        ctx.record_effect(
            event_id,
            faction,
            StateChange::PropertyChanged {
                field: "war_goals".to_string(),
                old_value: serde_json::Value::Null,
                new_value: serde_json::json!({
                    "target_faction_id": target_sim,
                    "goal": goal,
                }),
            },
        );
    }
}
