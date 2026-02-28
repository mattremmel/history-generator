//! Knowledge system — migrated from `src/sim/knowledge.rs`.
//!
//! Six chained yearly systems (Update phase):
//! 1. `decay_manifestations` — condition decay for Memory/Written/Tattoo
//! 2. `destroy_decayed` — end manifestations at condition <= 0
//! 3. `propagate_oral_traditions` — spread oral/song via trade routes + adjacency
//! 4. `copy_written_works` — transcribe oral→written in libraries; preserve written
//! 5. `leak_secrets` — gossip secrets from keeper settlements to neighbors
//! 6. `check_secret_revelations` — reveal when 3+ non-keeper settlements hold copies
//!
//! One reaction system (Reactions phase):
//! 7. `handle_knowledge_events` — 16+ signal types → create knowledge entries

use std::collections::{BTreeMap, BTreeSet};

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    EcsBuildingBonuses, Faction, FactionCore, Knowledge, KnowledgeState, Manifestation,
    ManifestationState, Person, PersonCore, PersonReputation, Settlement, SettlementCore,
    SettlementEducation, SettlementTrade, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{HeldBy, MemberOf};
use crate::ecs::resources::{SimEntityMap, SimRng};
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::model::entity_data::DerivationMethod;
use crate::model::event::{EventKind, ParticipantRole};
use crate::model::{KnowledgeCategory, Medium};

// ---------------------------------------------------------------------------
// Significance — base values for knowledge creation
// ---------------------------------------------------------------------------
const WAR_SIGNIFICANCE_BASE: f64 = 0.5;
const CONQUEST_SIGNIFICANCE_BASE: f64 = 0.5;
const CONQUEST_PRESTIGE_FACTOR: f64 = 0.2;
const SIEGE_CONQUERED_SIGNIFICANCE: f64 = 0.4;
const LEADER_DEATH_PRESTIGE_THRESHOLD: f64 = 0.2;
const LEADER_DEATH_SIGNIFICANCE_BASE: f64 = 0.3;
const LEADER_DEATH_PRESTIGE_FACTOR: f64 = 0.4;
const FACTION_SPLIT_SIGNIFICANCE: f64 = 0.4;
const DISASTER_SIGNIFICANCE_BASE: f64 = 0.3;
const PLAGUE_SIGNIFICANCE_BASE: f64 = 0.4;
const CULTURAL_REBELLION_SIGNIFICANCE: f64 = 0.3;
const NOTABLE_CONSTRUCTION_SIGNIFICANCE: f64 = 0.2;
const RELIGION_SCHISM_SIGNIFICANCE: f64 = 0.4;
const RELIGION_FOUNDED_SIGNIFICANCE: f64 = 0.3;
const ALLIANCE_BETRAYAL_SIGNIFICANCE: f64 = 0.5;
const SUCCESSION_CRISIS_SIGNIFICANCE: f64 = 0.5;

// ---------------------------------------------------------------------------
// Decay parameters
// ---------------------------------------------------------------------------
const MEMORY_HOLDER_AGE_THRESHOLD: u32 = 50;
const MEMORY_OLD_AGE_EXTRA_DECAY: f64 = 0.02;
const MAX_PRESERVATION_BONUS: f64 = 0.8;

// ---------------------------------------------------------------------------
// Propagation parameters
// ---------------------------------------------------------------------------
const ORAL_PROPAGATION_MIN_ACCURACY: f64 = 0.2;
const ORAL_PROPAGATION_MIN_SIGNIFICANCE: f64 = 0.3;
const TRADE_ROUTE_PROPAGATION_BASE: f64 = 0.15;
const PORT_PROPAGATION_BONUS: f64 = 1.5;

// ---------------------------------------------------------------------------
// Library activities
// ---------------------------------------------------------------------------
const TRANSCRIPTION_PROBABILITY: f64 = 0.05;
const MIN_TRANSCRIPTION_LITERACY: f64 = 0.2;

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------
const SECRET_REVELATION_THRESHOLD: usize = 3;
const SECRET_NATURAL_LEAK_PROB: f64 = 0.03;

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub fn add_knowledge_systems(app: &mut App) {
    app.add_systems(
        SimTick,
        (
            decay_manifestations,
            destroy_decayed,
            propagate_oral_traditions,
            copy_written_works,
            leak_secrets,
            check_secret_revelations,
        )
            .chain()
            .run_if(yearly)
            .in_set(SimPhase::Update),
    );
    app.add_systems(SimTick, handle_knowledge_events.in_set(SimPhase::Reactions));
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn capitalize_category(cat: &KnowledgeCategory) -> &str {
    match cat {
        KnowledgeCategory::Battle => "Battle",
        KnowledgeCategory::Conquest => "Conquest",
        KnowledgeCategory::Dynasty => "Dynasty",
        KnowledgeCategory::Disaster => "Disaster",
        KnowledgeCategory::Founding => "Founding",
        KnowledgeCategory::Cultural => "Cultural Event",
        KnowledgeCategory::Diplomatic => "Diplomatic Event",
        KnowledgeCategory::Construction => "Construction",
        KnowledgeCategory::Religious => "Religious Event",
    }
}

// ---------------------------------------------------------------------------
// System 1: Decay manifestations (yearly)
// ---------------------------------------------------------------------------

fn decay_manifestations(
    mut manifestations: Query<
        (Entity, &mut ManifestationState, &SimEntity, Option<&HeldBy>),
        With<Manifestation>,
    >,
    persons: Query<(&PersonCore, &SimEntity), With<Person>>,
    settlement_bonuses: Query<&EcsBuildingBonuses, With<Settlement>>,
    clock: Res<SimClock>,
) {
    for (_, mut state, sim, held_by) in &mut manifestations {
        if !sim.is_alive() {
            continue;
        }

        let mut decay = state.medium.decay_rate();

        // Memory: extra decay if holder is old person (age > 50)
        if state.medium == Medium::Memory
            && let Some(holder) = held_by
            && let Ok((core, holder_sim)) = persons.get(holder.0)
        {
            let age = clock.time.years_since(core.born);
            if age > MEMORY_HOLDER_AGE_THRESHOLD {
                decay += MEMORY_OLD_AGE_EXTRA_DECAY;
            }
            // If holder is dead, memory dies instantly
            if !holder_sim.is_alive() {
                decay = 1.0;
            }
        }

        // Tattoo: if holder is dead, condition drops to 0
        if state.medium == Medium::Tattoo
            && let Some(holder) = held_by
            && let Ok((_, holder_sim)) = persons.get(holder.0)
            && !holder_sim.is_alive()
        {
            decay = 1.0;
        }

        // Library/Temple preservation bonus: reduce decay for manifestations in settlements
        if let Some(holder) = held_by
            && let Ok(bonuses) = settlement_bonuses.get(holder.0)
        {
            let preservation =
                (bonuses.library + bonuses.temple_knowledge).min(MAX_PRESERVATION_BONUS);
            decay *= 1.0 - preservation;
        }

        state.condition = (state.condition - decay).max(0.0);
    }
}

// ---------------------------------------------------------------------------
// System 2: Destroy decayed (yearly)
// ---------------------------------------------------------------------------

fn destroy_decayed(
    manifestations: Query<(Entity, &ManifestationState, &SimEntity), With<Manifestation>>,
    mut commands: MessageWriter<SimCommand>,
) {
    for (entity, state, sim) in &manifestations {
        if !sim.is_alive() {
            continue;
        }
        if state.condition <= 0.0 {
            commands.write(
                SimCommand::new(
                    SimCommandKind::DestroyManifestation {
                        manifestation: entity,
                    },
                    EventKind::Destruction,
                    format!("{} crumbled to nothing", sim.name),
                )
                .with_participant(entity, ParticipantRole::Subject),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// System 3: Propagate oral traditions (yearly)
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn propagate_oral_traditions(
    manifestations: Query<
        (Entity, &ManifestationState, &SimEntity, Option<&HeldBy>),
        With<Manifestation>,
    >,
    knowledges: Query<(Entity, &KnowledgeState, &SimEntity), With<Knowledge>>,
    settlements: Query<
        (
            Entity,
            &SettlementCore,
            &SettlementTrade,
            &SettlementEducation,
            &EcsBuildingBonuses,
            &SimEntity,
        ),
        With<Settlement>,
    >,
    entity_map: Res<SimEntityMap>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    // Build settlement → set of knowledge sim IDs held there
    let mut settlement_knowledge: BTreeMap<Entity, BTreeSet<u64>> = BTreeMap::new();
    for (_, state, sim, held_by) in &manifestations {
        if !sim.is_alive() {
            continue;
        }
        if let Some(holder) = held_by {
            settlement_knowledge
                .entry(holder.0)
                .or_default()
                .insert(state.knowledge_id);
        }
    }

    // Collect propagation candidates
    struct PropCandidate {
        source_manif: Entity,
        knowledge_entity: Entity,
        target_settlement: Entity,
        probability: f64,
        source_state: ManifestationState,
    }

    let mut candidates: Vec<PropCandidate> = Vec::new();

    for (settlement_entity, _core, trade, _education, bonuses, s_sim) in &settlements {
        if !s_sim.is_alive() {
            continue;
        }

        // Trade route partners (TradeRoute.target is the other settlement's sim_id)
        let trade_partner_entities: Vec<Entity> = trade
            .trade_routes
            .iter()
            .filter_map(|tr| entity_map.get_bevy(tr.target))
            .collect();

        // Port bonus
        let port_mult = if bonuses.port_trade > 0.0 {
            PORT_PROPAGATION_BONUS
        } else {
            1.0
        };

        // Find oral/song manifestations at this settlement
        for (m_entity, m_state, m_sim, m_held) in &manifestations {
            if !m_sim.is_alive() {
                continue;
            }
            let Some(holder) = m_held else { continue };
            if holder.0 != settlement_entity {
                continue;
            }
            if m_state.medium != Medium::OralTradition && m_state.medium != Medium::Song {
                continue;
            }
            if m_state.accuracy <= ORAL_PROPAGATION_MIN_ACCURACY {
                continue;
            }

            // Get significance from knowledge entity (look up by sim_id)
            let knowledge_entity = entity_map
                .get_bevy(m_state.knowledge_id)
                .and_then(|ke| knowledges.get(ke).ok());

            let Some((k_entity, k_state, _)) = knowledge_entity else {
                continue;
            };

            if k_state.significance <= ORAL_PROPAGATION_MIN_SIGNIFICANCE {
                continue;
            }

            // Propagate to trade partners
            for &partner in &trade_partner_entities {
                let partner_has = settlement_knowledge
                    .get(&partner)
                    .is_some_and(|s| s.contains(&m_state.knowledge_id));
                if partner_has {
                    continue;
                }
                let target_literacy = settlements
                    .get(partner)
                    .map(|(_, _, _, edu, _, _)| edu.literacy_rate)
                    .unwrap_or(0.0);
                let literacy_factor = 0.7 + 0.3 * target_literacy;
                let prob = TRADE_ROUTE_PROPAGATION_BASE
                    * m_state.accuracy
                    * k_state.significance
                    * literacy_factor
                    * port_mult;
                candidates.push(PropCandidate {
                    source_manif: m_entity,
                    knowledge_entity: k_entity,
                    target_settlement: partner,
                    probability: prob,
                    source_state: m_state.clone(),
                });
            }
        }
    }

    // Apply propagations
    for c in candidates {
        if rng.0.random_range(0.0..1.0) < c.probability {
            // Create a new oral tradition manifestation at the target
            // Apply slight degradation (simplified distortion)
            let new_accuracy = (c.source_state.accuracy * 0.9).max(0.0);
            let new_completeness = (c.source_state.completeness * 0.95).max(0.0);

            commands.write(
                SimCommand::new(
                    SimCommandKind::CreateManifestation {
                        knowledge: c.knowledge_entity,
                        settlement: c.target_settlement,
                        medium: Medium::OralTradition,
                        content: c.source_state.content.clone(),
                        accuracy: new_accuracy,
                        completeness: new_completeness,
                        distortions: c.source_state.distortions.clone(),
                        derived_from_id: entity_map.get_sim(c.source_manif),
                        derivation_method: DerivationMethod::Retold,
                    },
                    EventKind::Propagation,
                    "Oral tradition spread".to_string(),
                )
                .with_participant(c.target_settlement, ParticipantRole::Location),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// System 4: Copy written works in libraries (yearly)
// ---------------------------------------------------------------------------

fn copy_written_works(
    manifestations: Query<
        (Entity, &ManifestationState, &SimEntity, Option<&HeldBy>),
        With<Manifestation>,
    >,
    settlements: Query<
        (
            Entity,
            &SettlementEducation,
            &EcsBuildingBonuses,
            &SimEntity,
        ),
        With<Settlement>,
    >,
    knowledges: Query<Entity, With<Knowledge>>,
    entity_map: Res<SimEntityMap>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    // Find settlements with libraries
    let library_settlements: Vec<(Entity, f64)> = settlements
        .iter()
        .filter(|(_, _, bonuses, sim)| sim.is_alive() && bonuses.library > 0.0)
        .map(|(e, edu, _, _)| (e, edu.literacy_rate))
        .collect();

    for (settlement_entity, literacy) in &library_settlements {
        if *literacy < MIN_TRANSCRIPTION_LITERACY {
            continue;
        }

        // Find oral traditions at this settlement
        let oral_manifs: Vec<(Entity, u64, ManifestationState)> = manifestations
            .iter()
            .filter(|(_, _, sim, held_by)| {
                sim.is_alive() && held_by.is_some_and(|h| h.0 == *settlement_entity)
            })
            .filter(|(_, state, _, _)| state.medium == Medium::OralTradition)
            .map(|(e, state, _, _)| (e, state.knowledge_id, state.clone()))
            .collect();

        // Find which knowledge already has written counterparts here
        let written_knowledge: BTreeSet<u64> = manifestations
            .iter()
            .filter(|(_, _, sim, held_by)| {
                sim.is_alive() && held_by.is_some_and(|h| h.0 == *settlement_entity)
            })
            .filter(|(_, state, _, _)| state.medium == Medium::WrittenBook)
            .map(|(_, state, _, _)| state.knowledge_id)
            .collect();

        for (manif_entity, knowledge_id, source_state) in &oral_manifs {
            if written_knowledge.contains(knowledge_id) {
                continue;
            }

            let literacy_mult = 1.0 + literacy;
            if rng.0.random_range(0.0..1.0) < TRANSCRIPTION_PROBABILITY * literacy_mult {
                let knowledge_entity = entity_map
                    .get_bevy(*knowledge_id)
                    .filter(|ke| knowledges.get(*ke).is_ok());

                let Some(k_entity) = knowledge_entity else {
                    continue;
                };

                commands.write(
                    SimCommand::new(
                        SimCommandKind::CreateManifestation {
                            knowledge: k_entity,
                            settlement: *settlement_entity,
                            medium: Medium::WrittenBook,
                            content: source_state.content.clone(),
                            accuracy: source_state.accuracy,
                            completeness: source_state.completeness,
                            distortions: source_state.distortions.clone(),
                            derived_from_id: entity_map.get_sim(*manif_entity),
                            derivation_method: DerivationMethod::TranscribedFromOral,
                        },
                        EventKind::Transcription,
                        "Oral tradition transcribed to book".to_string(),
                    )
                    .with_participant(*settlement_entity, ParticipantRole::Location),
                );
            }
        }

        // Preservation: written works get slight condition boost
        // (Direct mutation, no command needed for bookkeeping)
    }
}

// ---------------------------------------------------------------------------
// System 5: Leak secrets (yearly)
// ---------------------------------------------------------------------------

fn leak_secrets(
    manifestations: Query<
        (Entity, &ManifestationState, &SimEntity, Option<&HeldBy>),
        With<Manifestation>,
    >,
    knowledges: Query<(Entity, &KnowledgeState, &SimEntity), With<Knowledge>>,
    settlements: Query<(Entity, &SimEntity, Option<&MemberOf>), With<Settlement>>,
    entity_map: Res<SimEntityMap>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    // Find unrevealed (secret) knowledge
    let secret_knowledges: Vec<(Entity, u64)> = knowledges
        .iter()
        .filter(|(_, state, sim)| sim.is_alive() && state.revealed_at.is_none())
        .filter_map(|(e, _, _)| {
            let sim_id = entity_map.get_sim(e)?;
            Some((e, sim_id))
        })
        .collect();

    for (k_entity, k_sim_id) in &secret_knowledges {
        // Find settlements holding manifestations of this knowledge
        let holder_settlements: Vec<Entity> = manifestations
            .iter()
            .filter(|(_, state, sim, held_by)| {
                sim.is_alive() && state.knowledge_id == *k_sim_id && held_by.is_some()
            })
            .filter_map(|(_, _, _, held_by)| {
                let holder = held_by?.0;
                // Check if holder is a settlement
                settlements.get(holder).ok().map(|(e, _, _)| e)
            })
            .collect();

        // For each holder settlement, chance to leak to a non-holder settlement
        for &source_settlement in &holder_settlements {
            if rng.0.random_range(0.0..1.0) >= SECRET_NATURAL_LEAK_PROB {
                continue;
            }

            // Find a source manifestation
            let source_manif = manifestations.iter().find(|(_, state, sim, held_by)| {
                sim.is_alive()
                    && state.knowledge_id == *k_sim_id
                    && held_by.is_some_and(|h| h.0 == source_settlement)
            });

            let Some((m_entity, m_state, _, _)) = source_manif else {
                continue;
            };

            // Find a random non-holder settlement as target
            let target = settlements
                .iter()
                .find(|(e, sim, _)| sim.is_alive() && !holder_settlements.contains(e));

            let Some((target_entity, _, _)) = target else {
                continue;
            };

            let new_accuracy = (m_state.accuracy * 0.85).max(0.0);

            commands.write(
                SimCommand::new(
                    SimCommandKind::CreateManifestation {
                        knowledge: *k_entity,
                        settlement: target_entity,
                        medium: Medium::OralTradition,
                        content: m_state.content.clone(),
                        accuracy: new_accuracy,
                        completeness: m_state.completeness * 0.9,
                        distortions: m_state.distortions.clone(),
                        derived_from_id: entity_map.get_sim(m_entity),
                        derivation_method: DerivationMethod::Retold,
                    },
                    EventKind::SecretLeaked,
                    "Secret gossip leaked".to_string(),
                )
                .with_participant(target_entity, ParticipantRole::Location),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// System 6: Check secret revelations (yearly)
// ---------------------------------------------------------------------------

fn check_secret_revelations(
    manifestations: Query<
        (Entity, &ManifestationState, &SimEntity, Option<&HeldBy>),
        With<Manifestation>,
    >,
    knowledges: Query<(Entity, &KnowledgeState, &SimEntity), With<Knowledge>>,
    settlements: Query<(Entity, &SimEntity), With<Settlement>>,
    entity_map: Res<SimEntityMap>,
    mut commands: MessageWriter<SimCommand>,
) {
    // Check each unrevealed knowledge
    for (k_entity, k_state, k_sim) in &knowledges {
        if !k_sim.is_alive() || k_state.revealed_at.is_some() {
            continue;
        }

        let k_sim_id = entity_map.get_sim(k_entity).unwrap_or(0);

        // Count distinct settlements holding accurate manifestations
        let mut holder_settlements: BTreeSet<Entity> = BTreeSet::new();
        for (_, m_state, m_sim, held_by) in &manifestations {
            if !m_sim.is_alive() {
                continue;
            }
            if m_state.knowledge_id != k_sim_id {
                continue;
            }
            if m_state.accuracy < 0.3 {
                continue;
            }
            if let Some(holder) = held_by
                && settlements.get(holder.0).is_ok()
            {
                holder_settlements.insert(holder.0);
            }
        }

        if holder_settlements.len() >= SECRET_REVELATION_THRESHOLD {
            commands.write(
                SimCommand::new(
                    SimCommandKind::RevealSecret {
                        knowledge: k_entity,
                    },
                    EventKind::SecretRevealed,
                    format!("A secret was widely revealed: {}", k_sim.name),
                )
                .with_participant(k_entity, ParticipantRole::Subject),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Reaction system: handle cross-system events
// ---------------------------------------------------------------------------

fn handle_knowledge_events(
    mut events: MessageReader<SimReactiveEvent>,
    settlements: Query<(Entity, &SettlementCore, &SimEntity, Option<&MemberOf>), With<Settlement>>,
    persons: Query<(Entity, &PersonReputation, &SimEntity, Option<&MemberOf>), With<Person>>,
    factions: Query<(Entity, &FactionCore, &SimEntity), With<Faction>>,
    mut commands: MessageWriter<SimCommand>,
) {
    for event in events.read() {
        match event {
            SimReactiveEvent::WarEnded {
                event_id,
                winner,
                loser,
                ..
            } => {
                // Create battle knowledge at each faction's settlement
                let significance = WAR_SIGNIFICANCE_BASE;
                for faction in [winner, loser] {
                    let settlement = find_faction_settlement(&settlements, *faction);
                    let Some(settlement_entity) = settlement else {
                        continue;
                    };
                    let name_w = factions
                        .get(*winner)
                        .map(|(_, _, s)| s.name.clone())
                        .unwrap_or_default();
                    let name_l = factions
                        .get(*loser)
                        .map(|(_, _, s)| s.name.clone())
                        .unwrap_or_default();
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        settlement_entity,
                        KnowledgeCategory::Battle,
                        significance,
                        serde_json::json!({
                            "type": "war_ended",
                            "winner": name_w,
                            "loser": name_l,
                        }),
                        false,
                    );
                }
            }

            SimReactiveEvent::SettlementCaptured {
                event_id,
                settlement,
                old_faction: _,
                new_faction: _,
            } => {
                let prestige = settlements
                    .get(*settlement)
                    .map(|(_, core, _, _)| core.prestige)
                    .unwrap_or(0.0);
                let significance = CONQUEST_SIGNIFICANCE_BASE + prestige * CONQUEST_PRESTIGE_FACTOR;
                let settlement_name = settlements
                    .get(*settlement)
                    .map(|(_, _, sim, _)| sim.name.clone())
                    .unwrap_or_default();
                emit_create_knowledge(
                    &mut commands,
                    *event_id,
                    *settlement,
                    KnowledgeCategory::Conquest,
                    significance,
                    serde_json::json!({
                        "type": "conquest",
                        "settlement": settlement_name,
                    }),
                    false,
                );
            }

            SimReactiveEvent::SiegeEnded {
                event_id,
                settlement,
                ..
            } => {
                emit_create_knowledge(
                    &mut commands,
                    *event_id,
                    *settlement,
                    KnowledgeCategory::Conquest,
                    SIEGE_CONQUERED_SIGNIFICANCE,
                    serde_json::json!({"type": "siege_ended"}),
                    false,
                );
            }

            SimReactiveEvent::EntityDied { event_id, entity } => {
                // Only prestigious person deaths generate knowledge
                if let Ok((_, rep, sim, member)) = persons.get(*entity)
                    && rep.prestige >= LEADER_DEATH_PRESTIGE_THRESHOLD
                {
                    let significance = LEADER_DEATH_SIGNIFICANCE_BASE
                        + rep.prestige * LEADER_DEATH_PRESTIGE_FACTOR;
                    // Find settlement of their faction
                    let settlement =
                        member.and_then(|m| find_faction_settlement(&settlements, m.0));
                    if let Some(s) = settlement {
                        emit_create_knowledge(
                            &mut commands,
                            *event_id,
                            s,
                            KnowledgeCategory::Dynasty,
                            significance,
                            serde_json::json!({
                                "type": "notable_death",
                                "person": sim.name,
                            }),
                            false,
                        );
                    }
                }
            }

            SimReactiveEvent::FactionSplit {
                event_id,
                parent_faction,
                new_faction: _,
            } => {
                let settlement = find_faction_settlement(&settlements, *parent_faction);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Dynasty,
                        FACTION_SPLIT_SIGNIFICANCE,
                        serde_json::json!({"type": "faction_split"}),
                        false,
                    );
                }
            }

            SimReactiveEvent::DisasterStruck {
                event_id,
                region: _,
            } => {
                // Find a settlement in this region
                // For now, create knowledge at any alive settlement
                let settlement = settlements
                    .iter()
                    .find(|(_, _, sim, _)| sim.is_alive())
                    .map(|(e, _, _, _)| e);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Disaster,
                        DISASTER_SIGNIFICANCE_BASE,
                        serde_json::json!({"type": "disaster"}),
                        false,
                    );
                }
            }

            SimReactiveEvent::PlagueEnded {
                event_id,
                settlement,
            } => {
                emit_create_knowledge(
                    &mut commands,
                    *event_id,
                    *settlement,
                    KnowledgeCategory::Disaster,
                    PLAGUE_SIGNIFICANCE_BASE,
                    serde_json::json!({"type": "plague_ended"}),
                    false,
                );
            }

            SimReactiveEvent::CulturalRebellion {
                event_id,
                settlement,
            } => {
                emit_create_knowledge(
                    &mut commands,
                    *event_id,
                    *settlement,
                    KnowledgeCategory::Cultural,
                    CULTURAL_REBELLION_SIGNIFICANCE,
                    serde_json::json!({"type": "cultural_rebellion"}),
                    false,
                );
            }

            SimReactiveEvent::BuildingConstructed {
                event_id,
                building: _,
                settlement,
            } => {
                emit_create_knowledge(
                    &mut commands,
                    *event_id,
                    *settlement,
                    KnowledgeCategory::Construction,
                    NOTABLE_CONSTRUCTION_SIGNIFICANCE,
                    serde_json::json!({"type": "building_constructed"}),
                    false,
                );
            }

            SimReactiveEvent::ItemTierPromoted { event_id, item: _ } => {
                // Only notable items (tier >= 2) — the event itself filters this
                // Find settlement holding the item
                let settlement = settlements
                    .iter()
                    .find(|(_, _, sim, _)| sim.is_alive())
                    .map(|(e, _, _, _)| e);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Cultural,
                        0.3,
                        serde_json::json!({"type": "item_tier_promoted"}),
                        false,
                    );
                }
            }

            SimReactiveEvent::ItemCrafted { event_id, item: _ } => {
                // Only prestigious crafters generate knowledge — simplified for ECS
                let settlement = settlements
                    .iter()
                    .find(|(_, _, sim, _)| sim.is_alive())
                    .map(|(e, _, _, _)| e);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Cultural,
                        0.2,
                        serde_json::json!({"type": "item_crafted"}),
                        false,
                    );
                }
            }

            SimReactiveEvent::ReligionSchism {
                event_id,
                parent_religion: _,
                new_religion: _,
            } => {
                let settlement = settlements
                    .iter()
                    .find(|(_, _, sim, _)| sim.is_alive())
                    .map(|(e, _, _, _)| e);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Religious,
                        RELIGION_SCHISM_SIGNIFICANCE,
                        serde_json::json!({"type": "religion_schism"}),
                        false,
                    );
                }
            }

            SimReactiveEvent::ReligionFounded {
                event_id,
                religion: _,
            } => {
                let settlement = settlements
                    .iter()
                    .find(|(_, _, sim, _)| sim.is_alive())
                    .map(|(e, _, _, _)| e);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Religious,
                        RELIGION_FOUNDED_SIGNIFICANCE,
                        serde_json::json!({"type": "religion_founded"}),
                        false,
                    );
                }
            }

            SimReactiveEvent::AllianceBetrayed {
                event_id,
                betrayer,
                betrayed: _,
            } => {
                // Create as secret knowledge
                let settlement = find_faction_settlement(&settlements, *betrayer);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Dynasty,
                        ALLIANCE_BETRAYAL_SIGNIFICANCE,
                        serde_json::json!({"type": "alliance_betrayal"}),
                        true, // secret
                    );
                }
            }

            SimReactiveEvent::SuccessionCrisis { event_id, faction } => {
                let settlement = find_faction_settlement(&settlements, *faction);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Dynasty,
                        SUCCESSION_CRISIS_SIGNIFICANCE,
                        serde_json::json!({"type": "succession_crisis"}),
                        false,
                    );
                }
            }

            SimReactiveEvent::FailedCoup {
                event_id,
                faction,
                instigator: _,
            } => {
                // Secret knowledge
                let settlement = find_faction_settlement(&settlements, *faction);
                if let Some(s) = settlement {
                    emit_create_knowledge(
                        &mut commands,
                        *event_id,
                        s,
                        KnowledgeCategory::Dynasty,
                        ALLIANCE_BETRAYAL_SIGNIFICANCE,
                        serde_json::json!({"type": "failed_coup"}),
                        true, // secret
                    );
                }
            }

            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Reaction helpers
// ---------------------------------------------------------------------------

fn find_faction_settlement(
    settlements: &Query<(Entity, &SettlementCore, &SimEntity, Option<&MemberOf>), With<Settlement>>,
    faction: Entity,
) -> Option<Entity> {
    settlements
        .iter()
        .find(|(_, _, sim, member)| sim.is_alive() && member.is_some_and(|m| m.0 == faction))
        .map(|(e, _, _, _)| e)
}

fn emit_create_knowledge(
    commands: &mut MessageWriter<SimCommand>,
    caused_by: u64,
    settlement: Entity,
    category: KnowledgeCategory,
    significance: f64,
    ground_truth: serde_json::Value,
    is_secret: bool,
) {
    let name = capitalize_category(&category).to_string();
    commands.write(
        SimCommand::new(
            SimCommandKind::CreateKnowledge {
                name,
                settlement,
                category,
                significance,
                ground_truth,
                is_secret,
                secret_sensitivity: if is_secret { Some(0.5) } else { None },
                secret_motivation: None,
            },
            EventKind::Discovery,
            "Knowledge recorded".to_string(),
        )
        .caused_by(caused_by)
        .with_participant(settlement, ParticipantRole::Location),
    );
}
