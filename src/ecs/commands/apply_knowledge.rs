use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use crate::ecs::components::common::SimEntity;
use crate::ecs::components::knowledge::{KnowledgeState, ManifestationState};
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::HeldBy;
use crate::ecs::spawn;
use crate::model::effect::StateChange;
use crate::model::entity_data::{DerivationMethod, KnowledgeCategory, Medium};

use super::applicator::ApplyCtx;

/// Create a new knowledge entity with an initial Memory manifestation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_create_knowledge(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    name: &str,
    settlement: Entity,
    category: KnowledgeCategory,
    significance: f64,
    ground_truth: &serde_json::Value,
    is_secret: bool,
    _secret_sensitivity: Option<f64>,
    _secret_motivation: Option<crate::model::secret::SecretMotivation>,
) {
    let settlement_sim_id = ctx.entity_map.get_sim(settlement).unwrap_or(0);

    // Spawn Knowledge entity
    let knowledge_id = ctx.id_gen.0.next_id();
    let knowledge_entity = spawn::spawn_knowledge(
        world,
        knowledge_id,
        name.to_string(),
        Some(ctx.clock_time),
        KnowledgeState {
            category,
            source_event_id: event_id,
            origin_settlement_id: settlement_sim_id,
            origin_time: ctx.clock_time,
            significance,
            ground_truth: ground_truth.clone(),
            revealed_at: if is_secret { None } else { Some(ctx.clock_time) },
        },
    );
    ctx.entity_map.insert(knowledge_id, knowledge_entity);

    ctx.record_effect(
        event_id,
        knowledge_entity,
        StateChange::EntityCreated {
            kind: crate::model::EntityKind::Knowledge,
            name: name.to_string(),
        },
    );

    ctx.emit(SimReactiveEvent::KnowledgeCreated {
        event_id,
        knowledge: knowledge_entity,
    });

    // Spawn initial Memory manifestation at the origin settlement
    let manifestation_id = ctx.id_gen.0.next_id();
    let manifestation_entity = spawn::spawn_manifestation(
        world,
        manifestation_id,
        format!("Memory of {name}"),
        Some(ctx.clock_time),
        ManifestationState {
            knowledge_id,
            medium: Medium::Memory,
            content: ground_truth.clone(),
            accuracy: 1.0,
            completeness: 1.0,
            distortions: Vec::new(),
            derived_from_id: None,
            derivation_method: DerivationMethod::Witnessed,
            condition: 1.0,
            created: ctx.clock_time,
        },
    );
    ctx.entity_map.insert(manifestation_id, manifestation_entity);

    // Manifestation is held by the settlement
    world
        .entity_mut(manifestation_entity)
        .insert(HeldBy(settlement));

    ctx.record_effect(
        event_id,
        manifestation_entity,
        StateChange::EntityCreated {
            kind: crate::model::EntityKind::Manifestation,
            name: format!("Memory of {name}"),
        },
    );

    ctx.emit(SimReactiveEvent::ManifestationCreated {
        event_id,
        manifestation: manifestation_entity,
    });
}

/// Create a new manifestation of existing knowledge.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_create_manifestation(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    knowledge: Entity,
    settlement: Entity,
    medium: Medium,
    content: &serde_json::Value,
    accuracy: f64,
    completeness: f64,
    distortions: &[serde_json::Value],
    derived_from_id: Option<u64>,
    derivation_method: &DerivationMethod,
) {
    let knowledge_sim_id = ctx.entity_map.get_sim(knowledge).unwrap_or(0);
    let knowledge_name = world
        .get::<SimEntity>(knowledge)
        .map(|s| s.name.clone())
        .unwrap_or_default();

    let manifestation_id = ctx.id_gen.0.next_id();
    let manif_name = format!("{medium} of {knowledge_name}");
    let manifestation_entity = spawn::spawn_manifestation(
        world,
        manifestation_id,
        manif_name.clone(),
        Some(ctx.clock_time),
        ManifestationState {
            knowledge_id: knowledge_sim_id,
            medium,
            content: content.clone(),
            accuracy,
            completeness,
            distortions: distortions.to_vec(),
            derived_from_id,
            derivation_method: derivation_method.clone(),
            condition: 1.0,
            created: ctx.clock_time,
        },
    );
    ctx.entity_map.insert(manifestation_id, manifestation_entity);

    // Manifestation is held by the settlement
    world
        .entity_mut(manifestation_entity)
        .insert(HeldBy(settlement));

    ctx.record_effect(
        event_id,
        manifestation_entity,
        StateChange::EntityCreated {
            kind: crate::model::EntityKind::Manifestation,
            name: manif_name,
        },
    );

    ctx.emit(SimReactiveEvent::ManifestationCreated {
        event_id,
        manifestation: manifestation_entity,
    });
}

/// Destroy a manifestation: mark it as ended.
pub(crate) fn apply_destroy_manifestation(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    manifestation: Entity,
) {
    if let Some(mut sim) = world.get_mut::<SimEntity>(manifestation)
        && sim.end.is_none()
    {
        sim.end = Some(ctx.clock_time);
        ctx.record_effect(event_id, manifestation, StateChange::EntityEnded);
    }
}

/// Reveal a secret: set revealed_at on the KnowledgeState.
pub(crate) fn apply_reveal_secret(
    ctx: &mut ApplyCtx,
    world: &mut World,
    event_id: u64,
    knowledge: Entity,
) {
    if let Some(mut state) = world.get_mut::<KnowledgeState>(knowledge)
        && state.revealed_at.is_none()
    {
        state.revealed_at = Some(ctx.clock_time);

        ctx.record_effect(
            event_id,
            knowledge,
            StateChange::PropertyChanged {
                field: "revealed_at".to_string(),
                old_value: serde_json::json!(null),
                new_value: serde_json::json!(ctx.clock_time.year()),
            },
        );

        ctx.emit(SimReactiveEvent::SecretRevealed {
            event_id,
            knowledge,
        });
    }
}
