use rand::Rng;

use super::context::TickContext;
use super::knowledge_derivation;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{
    EntityData, EntityKind, EventKind, KnowledgeCategory, KnowledgeData, ManifestationData, Medium,
    ParticipantRole, RelationshipKind, SimTimestamp,
};

pub struct KnowledgeSystem;

impl SimSystem for KnowledgeSystem {
    fn name(&self) -> &str {
        "knowledge"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        let year_event = ctx.world.add_event(
            EventKind::Custom("knowledge_tick".to_string()),
            time,
            format!("Knowledge activity in year {current_year}"),
        );

        decay_manifestations(ctx, time, current_year, year_event);
        destroy_decayed(ctx, time, year_event);
        propagate_oral_traditions(ctx, time, year_event);
        copy_written_works(ctx, time, year_event);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let year_event = ctx.world.add_event(
            EventKind::Custom("knowledge_signal".to_string()),
            time,
            format!("Knowledge signal processing in year {}", time.year()),
        );

        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::WarEnded {
                    winner_id,
                    loser_id,
                    decisive,
                    reparations,
                    ..
                } => {
                    let significance = 0.5 + if *decisive { 0.3 } else { 0.0 };
                    let capital = find_faction_capital(ctx.world, *winner_id);
                    if let Some(settlement_id) = capital {
                        let winner_name = entity_name(ctx.world, *winner_id);
                        let loser_name = entity_name(ctx.world, *loser_id);
                        let (w_troops, l_troops) =
                            get_faction_army_strengths(ctx.world, *winner_id, *loser_id);
                        let truth = serde_json::json!({
                            "event_type": "battle",
                            "name": format!("War between {} and {}", winner_name, loser_name),
                            "year": time.year(),
                            "attacker": { "faction_id": winner_id, "faction_name": winner_name, "troops": w_troops },
                            "defender": { "faction_id": loser_id, "faction_name": loser_name, "troops": l_troops },
                            "outcome": "attacker_victory",
                            "decisive": decisive,
                            "reparations": reparations,
                            "notable_details": []
                        });
                        create_knowledge(
                            ctx,
                            time,
                            year_event,
                            signal.event_id,
                            KnowledgeCategory::Battle,
                            significance,
                            settlement_id,
                            truth,
                        );
                    }
                }
                SignalKind::SettlementCaptured {
                    settlement_id,
                    old_faction_id,
                    new_faction_id,
                } => {
                    let settlement_prestige = get_settlement_prestige(ctx.world, *settlement_id);
                    let significance = 0.5 + 0.2 * settlement_prestige;
                    let settlement_name = entity_name(ctx.world, *settlement_id);
                    let old_name = entity_name(ctx.world, *old_faction_id);
                    let new_name = entity_name(ctx.world, *new_faction_id);
                    let truth = serde_json::json!({
                        "event_type": "conquest",
                        "settlement_id": settlement_id,
                        "settlement_name": settlement_name,
                        "old_faction_id": old_faction_id,
                        "old_faction_name": old_name,
                        "new_faction_id": new_faction_id,
                        "new_faction_name": new_name,
                        "year": time.year()
                    });
                    create_knowledge(
                        ctx,
                        time,
                        year_event,
                        signal.event_id,
                        KnowledgeCategory::Conquest,
                        significance,
                        *settlement_id,
                        truth,
                    );
                }
                SignalKind::SiegeEnded {
                    settlement_id,
                    outcome,
                    attacker_faction_id,
                    defender_faction_id,
                } => {
                    if outcome == "conquered" {
                        let truth = serde_json::json!({
                            "event_type": "conquest",
                            "settlement_id": settlement_id,
                            "settlement_name": entity_name(ctx.world, *settlement_id),
                            "attacker_faction_name": entity_name(ctx.world, *attacker_faction_id),
                            "defender_faction_name": entity_name(ctx.world, *defender_faction_id),
                            "outcome": outcome,
                            "year": time.year()
                        });
                        create_knowledge(
                            ctx,
                            time,
                            year_event,
                            signal.event_id,
                            KnowledgeCategory::Conquest,
                            0.4,
                            *settlement_id,
                            truth,
                        );
                    }
                }
                SignalKind::EntityDied { entity_id } => {
                    if let Some(entity) = ctx.world.entities.get(entity_id)
                        && entity.kind == EntityKind::Person
                    {
                        let prestige = entity.data.as_person().map(|p| p.prestige).unwrap_or(0.0);
                        if prestige > 0.2 {
                            let person_name = entity.name.clone();
                            let faction_id = entity
                                .relationships
                                .iter()
                                .find(|r| r.kind == RelationshipKind::LeaderOf && r.end.is_none())
                                .map(|r| r.target_entity_id);
                            if let Some(fid) = faction_id
                                && let Some(sid) = find_faction_capital(ctx.world, fid)
                            {
                                let faction_name = entity_name(ctx.world, fid);
                                let significance = 0.3 + 0.4 * prestige;
                                let truth = serde_json::json!({
                                    "event_type": "leader_death",
                                    "person_id": entity_id,
                                    "person_name": person_name,
                                    "faction_id": fid,
                                    "faction_name": faction_name,
                                    "year": time.year()
                                });
                                create_knowledge(
                                    ctx,
                                    time,
                                    year_event,
                                    signal.event_id,
                                    KnowledgeCategory::Dynasty,
                                    significance,
                                    sid,
                                    truth,
                                );
                            }
                        }
                    }
                }
                SignalKind::FactionSplit {
                    old_faction_id,
                    new_faction_id,
                    settlement_id,
                } => {
                    let truth = serde_json::json!({
                        "event_type": "faction_split",
                        "old_faction_id": old_faction_id,
                        "old_faction_name": entity_name(ctx.world, *old_faction_id),
                        "new_faction_id": new_faction_id,
                        "new_faction_name": entity_name(ctx.world, *new_faction_id),
                        "settlement_id": settlement_id,
                        "year": time.year()
                    });
                    create_knowledge(
                        ctx,
                        time,
                        year_event,
                        signal.event_id,
                        KnowledgeCategory::Dynasty,
                        0.4,
                        *settlement_id,
                        truth,
                    );
                }
                SignalKind::DisasterStruck {
                    settlement_id,
                    disaster_type,
                    severity,
                    ..
                } => {
                    if *severity > 0.5 {
                        let settlement_name = entity_name(ctx.world, *settlement_id);
                        let significance = 0.3 + 0.4 * severity;
                        let truth = serde_json::json!({
                            "event_type": "disaster",
                            "settlement_id": settlement_id,
                            "settlement_name": settlement_name,
                            "disaster_type": disaster_type,
                            "severity": severity,
                            "year": time.year()
                        });
                        create_knowledge(
                            ctx,
                            time,
                            year_event,
                            signal.event_id,
                            KnowledgeCategory::Disaster,
                            significance,
                            *settlement_id,
                            truth,
                        );
                    }
                }
                SignalKind::PlagueEnded {
                    settlement_id,
                    deaths,
                    disease_id,
                } => {
                    if *deaths > 100 {
                        let settlement_name = entity_name(ctx.world, *settlement_id);
                        let significance = 0.4 + 0.3 * (*deaths as f64 / 1000.0).min(1.0);
                        let truth = serde_json::json!({
                            "event_type": "plague",
                            "settlement_id": settlement_id,
                            "settlement_name": settlement_name,
                            "disease_id": disease_id,
                            "deaths": deaths,
                            "year": time.year()
                        });
                        create_knowledge(
                            ctx,
                            time,
                            year_event,
                            signal.event_id,
                            KnowledgeCategory::Disaster,
                            significance,
                            *settlement_id,
                            truth,
                        );
                    }
                }
                SignalKind::CulturalRebellion {
                    settlement_id,
                    faction_id,
                    culture_id,
                } => {
                    let truth = serde_json::json!({
                        "event_type": "cultural_rebellion",
                        "settlement_id": settlement_id,
                        "settlement_name": entity_name(ctx.world, *settlement_id),
                        "faction_id": faction_id,
                        "culture_id": culture_id,
                        "year": time.year()
                    });
                    create_knowledge(
                        ctx,
                        time,
                        year_event,
                        signal.event_id,
                        KnowledgeCategory::Cultural,
                        0.3,
                        *settlement_id,
                        truth,
                    );
                }
                SignalKind::BuildingConstructed {
                    settlement_id,
                    building_type,
                    building_id,
                } => {
                    if building_type == "temple" || building_type == "library" {
                        let truth = serde_json::json!({
                            "event_type": "construction",
                            "settlement_id": settlement_id,
                            "settlement_name": entity_name(ctx.world, *settlement_id),
                            "building_type": building_type,
                            "building_id": building_id,
                            "year": time.year()
                        });
                        create_knowledge(
                            ctx,
                            time,
                            year_event,
                            signal.event_id,
                            KnowledgeCategory::Construction,
                            0.2,
                            *settlement_id,
                            truth,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Knowledge creation helper
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn create_knowledge(
    ctx: &mut TickContext,
    time: SimTimestamp,
    _year_event: u64,
    caused_by: u64,
    category: KnowledgeCategory,
    significance: f64,
    settlement_id: u64,
    ground_truth: serde_json::Value,
) {
    let category_str = category.as_str().to_string();
    let knowledge_name = format!(
        "{} at {}",
        capitalize_category(&category),
        entity_name(ctx.world, settlement_id)
    );

    // Create knowledge entity
    let ev = ctx.world.add_caused_event(
        EventKind::Custom("knowledge_created".to_string()),
        time,
        format!("Knowledge recorded: {knowledge_name}"),
        caused_by,
    );

    let kid = ctx.world.add_entity(
        EntityKind::Knowledge,
        knowledge_name.clone(),
        Some(time),
        EntityData::Knowledge(KnowledgeData {
            category,
            source_event_id: caused_by,
            origin_settlement_id: settlement_id,
            origin_year: time.year(),
            significance,
            ground_truth: ground_truth.clone(),
        }),
        ev,
    );
    ctx.world
        .add_event_participant(ev, kid, ParticipantRole::Subject);

    // Create initial eyewitness Memory manifestation at origin settlement
    let manif_name = format!("{knowledge_name} (memory)");
    let mid = ctx.world.add_entity(
        EntityKind::Manifestation,
        manif_name,
        Some(time),
        EntityData::Manifestation(ManifestationData {
            knowledge_id: kid,
            medium: Medium::Memory,
            content: ground_truth,
            accuracy: 1.0,
            completeness: 1.0,
            distortions: serde_json::json!([]),
            derived_from_id: None,
            derivation_method: "witnessed".to_string(),
            condition: 1.0,
            created_year: time.year(),
        }),
        ev,
    );

    // HeldBy -> origin settlement
    ctx.world
        .add_relationship(mid, settlement_id, RelationshipKind::HeldBy, time, ev);

    // Emit signals
    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::KnowledgeCreated {
            knowledge_id: kid,
            settlement_id,
            category: category_str.clone(),
            significance,
        },
    });
    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::ManifestationCreated {
            manifestation_id: mid,
            knowledge_id: kid,
            settlement_id,
            medium: "memory".to_string(),
        },
    });
}

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
    }
}

// ---------------------------------------------------------------------------
// Tick phase 1: Decay manifestations
// ---------------------------------------------------------------------------

fn decay_manifestations(
    ctx: &mut TickContext,
    _time: SimTimestamp,
    current_year: u32,
    year_event: u64,
) {
    struct DecayInfo {
        id: u64,
        old_condition: f64,
        decay_rate: f64,
    }

    let mut decays: Vec<DecayInfo> = Vec::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Manifestation || e.end.is_some() {
            continue;
        }
        let Some(md) = e.data.as_manifestation() else {
            continue;
        };

        let mut decay = md.medium.decay_rate();

        // Memory: extra decay if holder is old person (age > 50)
        if md.medium == Medium::Memory {
            let holder_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none())
                .map(|r| r.target_entity_id);
            if let Some(hid) = holder_id
                && let Some(holder) = ctx.world.entities.get(&hid)
            {
                if let Some(pd) = holder.data.as_person()
                    && current_year > pd.birth_year
                {
                    let age = current_year - pd.birth_year;
                    if age > 50 {
                        decay += 0.02;
                    }
                }
                // If holder is dead, memory dies instantly
                if holder.end.is_some() {
                    decay = 1.0;
                }
            }
        }

        // Tattoo: if holder is dead, condition drops to 0
        if md.medium == Medium::Tattoo {
            let holder_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none())
                .map(|r| r.target_entity_id);
            if let Some(hid) = holder_id
                && let Some(holder) = ctx.world.entities.get(&hid)
                && holder.end.is_some()
            {
                decay = 1.0;
            }
        }

        // Library/Temple bonus: reduce decay for manifestations in settlements with these buildings
        let settlement_id = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none())
            .map(|r| r.target_entity_id)
            .and_then(|hid| {
                let holder = ctx.world.entities.get(&hid)?;
                if holder.kind == EntityKind::Settlement {
                    Some(hid)
                } else {
                    // Check if holder (person) is in a settlement
                    holder
                        .relationships
                        .iter()
                        .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                        .and_then(|r| {
                            let faction = ctx.world.entities.get(&r.target_entity_id)?;
                            if faction.kind == EntityKind::Faction {
                                // Find a settlement in this faction — simplification
                                None
                            } else {
                                None
                            }
                        })
                }
            });

        if let Some(sid) = settlement_id {
            let library_bonus = ctx
                .world
                .entities
                .get(&sid)
                .and_then(|e| e.extra.get("building_library_bonus"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let temple_bonus = ctx
                .world
                .entities
                .get(&sid)
                .and_then(|e| e.extra.get("building_temple_knowledge_bonus"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let preservation = (library_bonus + temple_bonus).min(0.8);
            decay *= 1.0 - preservation;
        }

        decays.push(DecayInfo {
            id: e.id,
            old_condition: md.condition,
            decay_rate: decay,
        });
    }

    for d in decays {
        let new_condition = (d.old_condition - d.decay_rate).max(0.0);
        if let Some(entity) = ctx.world.entities.get_mut(&d.id)
            && let Some(md) = entity.data.as_manifestation_mut()
        {
            md.condition = new_condition;
        }

        if (d.old_condition - new_condition).abs() > 0.001 {
            ctx.world.record_change(
                d.id,
                year_event,
                "condition",
                serde_json::json!(d.old_condition),
                serde_json::json!(new_condition),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tick phase 2: Destroy decayed manifestations
// ---------------------------------------------------------------------------

fn destroy_decayed(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    let to_destroy: Vec<(u64, u64, u64)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Manifestation && e.end.is_none())
        .filter_map(|e| {
            let md = e.data.as_manifestation()?;
            if md.condition <= 0.0 {
                let settlement_id = e
                    .relationships
                    .iter()
                    .find(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none())
                    .map(|r| r.target_entity_id)
                    .unwrap_or(0);
                Some((e.id, md.knowledge_id, settlement_id))
            } else {
                None
            }
        })
        .collect();

    for (manif_id, knowledge_id, settlement_id) in to_destroy {
        let manif_name = ctx
            .world
            .entities
            .get(&manif_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let ev = ctx.world.add_caused_event(
            EventKind::Custom("manifestation_destroyed".to_string()),
            time,
            format!("{manif_name} crumbled to nothing"),
            year_event,
        );
        ctx.world
            .add_event_participant(ev, manif_id, ParticipantRole::Subject);
        ctx.world.end_entity(manif_id, time, ev);

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::ManifestationDestroyed {
                manifestation_id: manif_id,
                knowledge_id,
                settlement_id,
                cause: "decay".to_string(),
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Tick phase 3: Propagate oral traditions along trade routes
// ---------------------------------------------------------------------------

fn propagate_oral_traditions(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    // Collect settlement trade route adjacency
    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .map(|e| e.id)
        .collect();

    // Collect existing knowledge-per-settlement: (settlement_id -> set of knowledge_ids)
    let mut settlement_knowledge: std::collections::HashMap<u64, std::collections::HashSet<u64>> =
        std::collections::HashMap::new();

    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Manifestation || e.end.is_some() {
            continue;
        }
        let Some(md) = e.data.as_manifestation() else {
            continue;
        };
        let holder_id = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none())
            .map(|r| r.target_entity_id);
        if let Some(sid) = holder_id {
            settlement_knowledge
                .entry(sid)
                .or_default()
                .insert(md.knowledge_id);
        }
    }

    // Collect propagation candidates: (source_manifestation_id, target_settlement_id, probability)
    struct PropCandidate {
        source_manif_id: u64,
        target_settlement_id: u64,
        probability: f64,
    }

    let mut candidates: Vec<PropCandidate> = Vec::new();

    for &sid in &settlement_ids {
        let Some(settlement) = ctx.world.entities.get(&sid) else {
            continue;
        };

        // Trade route partners
        let trade_partners: Vec<u64> = settlement
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::TradeRoute && r.end.is_none())
            .map(|r| r.target_entity_id)
            .collect();

        // Adjacent settlements (via region adjacency)
        let adjacent_settlements: Vec<u64> = settlement
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::AdjacentTo && r.end.is_none())
            .map(|r| r.target_entity_id)
            .filter(|id| {
                ctx.world
                    .entities
                    .get(id)
                    .is_some_and(|e| e.kind == EntityKind::Settlement && e.end.is_none())
            })
            .collect();

        // Find oral/song manifestations in this settlement
        let oral_manifests: Vec<(u64, u64, f64, f64)> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Manifestation
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::HeldBy
                            && r.target_entity_id == sid
                            && r.end.is_none()
                    })
            })
            .filter_map(|e| {
                let md = e.data.as_manifestation()?;
                if (md.medium == Medium::OralTradition || md.medium == Medium::Song)
                    && md.accuracy > 0.2
                {
                    // Get significance from knowledge
                    let significance = ctx
                        .world
                        .entities
                        .get(&md.knowledge_id)
                        .and_then(|k| k.data.as_knowledge())
                        .map(|kd| kd.significance)
                        .unwrap_or(0.0);
                    if significance > 0.3 {
                        Some((e.id, md.knowledge_id, md.accuracy, significance))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        for (manif_id, knowledge_id, accuracy, significance) in &oral_manifests {
            // Trade route partners
            for &partner in &trade_partners {
                let partner_has = settlement_knowledge
                    .get(&partner)
                    .is_some_and(|s| s.contains(knowledge_id));
                if !partner_has {
                    let prob = 0.15 * accuracy * significance;
                    candidates.push(PropCandidate {
                        source_manif_id: *manif_id,
                        target_settlement_id: partner,
                        probability: prob,
                    });
                }
            }

            // Adjacent settlements (half probability)
            for &adj in &adjacent_settlements {
                if trade_partners.contains(&adj) {
                    continue; // already handled
                }
                let adj_has = settlement_knowledge
                    .get(&adj)
                    .is_some_and(|s| s.contains(knowledge_id));
                if !adj_has {
                    let prob = 0.075 * accuracy * significance;
                    candidates.push(PropCandidate {
                        source_manif_id: *manif_id,
                        target_settlement_id: adj,
                        probability: prob,
                    });
                }
            }
        }
    }

    // Apply propagations
    for c in candidates {
        if ctx.rng.random_range(0.0..1.0) < c.probability {
            let ev = ctx.world.add_caused_event(
                EventKind::Custom("knowledge_propagated".to_string()),
                time,
                format!(
                    "Oral tradition spread to {}",
                    entity_name(ctx.world, c.target_settlement_id)
                ),
                year_event,
            );

            if let Some(new_id) = knowledge_derivation::derive(
                ctx.world,
                ctx.rng,
                c.source_manif_id,
                Medium::OralTradition,
                c.target_settlement_id,
                time,
                ev,
            ) {
                let knowledge_id = ctx
                    .world
                    .entities
                    .get(&new_id)
                    .and_then(|e| e.data.as_manifestation())
                    .map(|md| md.knowledge_id)
                    .unwrap_or(0);
                ctx.signals.push(Signal {
                    event_id: ev,
                    kind: SignalKind::ManifestationCreated {
                        manifestation_id: new_id,
                        knowledge_id,
                        settlement_id: c.target_settlement_id,
                        medium: "oral_tradition".to_string(),
                    },
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tick phase 4: Copy written works in libraries
// ---------------------------------------------------------------------------

fn copy_written_works(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    // Find settlements with libraries
    let library_settlements: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter(|e| {
            e.extra
                .get("building_library_bonus")
                .and_then(|v| v.as_f64())
                .is_some_and(|v| v > 0.0)
        })
        .map(|e| e.id)
        .collect();

    // Collect existing knowledge per settlement
    let mut settlement_knowledge: std::collections::HashMap<u64, std::collections::HashSet<u64>> =
        std::collections::HashMap::new();
    for e in ctx.world.entities.values() {
        if e.kind != EntityKind::Manifestation || e.end.is_some() {
            continue;
        }
        let Some(md) = e.data.as_manifestation() else {
            continue;
        };
        let holder_id = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::HeldBy && r.end.is_none())
            .map(|r| r.target_entity_id);
        if let Some(sid) = holder_id {
            settlement_knowledge
                .entry(sid)
                .or_default()
                .insert(md.knowledge_id);
        }
    }

    struct TranscriptionCandidate {
        source_manif_id: u64,
        settlement_id: u64,
    }

    struct PreservationCandidate {
        manif_id: u64,
        old_condition: f64,
    }

    let mut transcriptions: Vec<TranscriptionCandidate> = Vec::new();
    let mut preservations: Vec<PreservationCandidate> = Vec::new();

    for &sid in &library_settlements {
        // Find oral traditions without a written counterpart
        let oral_manifs: Vec<(u64, u64)> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Manifestation
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::HeldBy
                            && r.target_entity_id == sid
                            && r.end.is_none()
                    })
            })
            .filter_map(|e| {
                let md = e.data.as_manifestation()?;
                if md.medium == Medium::OralTradition {
                    Some((e.id, md.knowledge_id))
                } else {
                    None
                }
            })
            .collect();

        let written_knowledge: std::collections::HashSet<u64> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Manifestation
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::HeldBy
                            && r.target_entity_id == sid
                            && r.end.is_none()
                    })
            })
            .filter_map(|e| {
                let md = e.data.as_manifestation()?;
                if md.medium == Medium::WrittenBook {
                    Some(md.knowledge_id)
                } else {
                    None
                }
            })
            .collect();

        for (manif_id, knowledge_id) in &oral_manifs {
            if !written_knowledge.contains(knowledge_id) {
                transcriptions.push(TranscriptionCandidate {
                    source_manif_id: *manif_id,
                    settlement_id: sid,
                });
            }
        }

        // Preservation: written works get slight condition boost
        let written_manifs: Vec<(u64, f64)> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Manifestation
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::HeldBy
                            && r.target_entity_id == sid
                            && r.end.is_none()
                    })
            })
            .filter_map(|e| {
                let md = e.data.as_manifestation()?;
                if md.medium == Medium::WrittenBook && md.condition < 1.0 {
                    Some((e.id, md.condition))
                } else {
                    None
                }
            })
            .collect();

        for (mid, cond) in written_manifs {
            preservations.push(PreservationCandidate {
                manif_id: mid,
                old_condition: cond,
            });
        }
    }

    // Apply transcriptions (5% chance each)
    for tc in transcriptions {
        if ctx.rng.random_range(0.0..1.0) < 0.05 {
            let ev = ctx.world.add_caused_event(
                EventKind::Custom("knowledge_transcribed".to_string()),
                time,
                format!(
                    "Oral tradition transcribed to book at {}",
                    entity_name(ctx.world, tc.settlement_id)
                ),
                year_event,
            );
            if let Some(new_id) = knowledge_derivation::derive(
                ctx.world,
                ctx.rng,
                tc.source_manif_id,
                Medium::WrittenBook,
                tc.settlement_id,
                time,
                ev,
            ) {
                let knowledge_id = ctx
                    .world
                    .entities
                    .get(&new_id)
                    .and_then(|e| e.data.as_manifestation())
                    .map(|md| md.knowledge_id)
                    .unwrap_or(0);
                ctx.signals.push(Signal {
                    event_id: ev,
                    kind: SignalKind::ManifestationCreated {
                        manifestation_id: new_id,
                        knowledge_id,
                        settlement_id: tc.settlement_id,
                        medium: "written_book".to_string(),
                    },
                });
            }
        }
    }

    // Apply preservation (+0.001/yr maintenance)
    for p in preservations {
        let new_condition = (p.old_condition + 0.001).min(1.0);
        if let Some(entity) = ctx.world.entities.get_mut(&p.manif_id)
            && let Some(md) = entity.data.as_manifestation_mut()
        {
            md.condition = new_condition;
        }
    }
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

fn entity_name(world: &crate::model::World, id: u64) -> String {
    world
        .entities
        .get(&id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| format!("Entity#{id}"))
}

fn find_faction_capital(world: &crate::model::World, faction_id: u64) -> Option<u64> {
    // Return first (oldest) settlement belonging to this faction
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .min_by_key(|e| e.id)
        .map(|e| e.id)
}

fn get_settlement_prestige(world: &crate::model::World, settlement_id: u64) -> f64 {
    world
        .entities
        .get(&settlement_id)
        .and_then(|e| e.data.as_settlement())
        .map(|sd| sd.prestige)
        .unwrap_or(0.0)
}

fn get_faction_army_strengths(
    world: &crate::model::World,
    faction_a: u64,
    faction_b: u64,
) -> (u32, u32) {
    let mut a_troops = 0u32;
    let mut b_troops = 0u32;
    for e in world.entities.values() {
        if e.kind != EntityKind::Army || e.end.is_some() {
            continue;
        }
        let faction_id = e
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
            .map(|r| r.target_entity_id);
        let strength = e.data.as_army().map(|a| a.strength).unwrap_or(0);
        match faction_id {
            Some(id) if id == faction_a => a_troops += strength,
            Some(id) if id == faction_b => b_troops += strength,
            _ => {}
        }
    }
    (a_troops.max(100), b_troops.max(100))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::World;
    use crate::scenario::Scenario;
    use crate::sim::context::TickContext;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    /// Minimal knowledge test world: one region, one faction, one settlement.
    /// Returns `(world, setup_event, faction, settlement)`.
    fn knowledge_scenario() -> (World, u64, u64, u64) {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("TestRegion");
        let faction = s.add_faction_with("TestFaction", |fd| {
            fd.treasury = 500.0;
        });
        let settlement = s.add_settlement_with("TestTown", faction, region, |sd| {
            sd.population = 500;
            sd.prosperity = 0.7;
            sd.prestige = 0.3;
        });
        let ev = s.world().events.keys().next().copied().unwrap();
        (s.build(), ev, faction, settlement)
    }

    #[test]
    fn scenario_war_ended_creates_knowledge() {
        let (mut world, ev, faction, _settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Create a second faction
        let enemy = world.add_entity(
            EntityKind::Faction,
            "Enemy".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(&EntityKind::Faction),
            ev,
        );

        let signal = Signal {
            event_id: ev,
            kind: SignalKind::WarEnded {
                winner_id: faction,
                loser_id: enemy,
                decisive: true,
                reparations: 50.0,
                tribute_years: 0,
            },
        };

        let mut signals_out = vec![];
        let inbox = vec![signal];
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals_out,
            inbox: &inbox,
        };

        let mut system = KnowledgeSystem;
        system.handle_signals(&mut ctx);

        // Should have created Knowledge + Manifestation entities
        let knowledge_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        let manif_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Manifestation)
            .count();

        assert!(
            knowledge_count > 0,
            "should create knowledge entity from WarEnded"
        );
        assert!(
            manif_count > 0,
            "should create manifestation entity from WarEnded"
        );

        // Check KnowledgeCreated signal was emitted
        let kc_signals: Vec<_> = signals_out
            .iter()
            .filter(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. }))
            .collect();
        assert!(
            !kc_signals.is_empty(),
            "should emit KnowledgeCreated signal"
        );
    }

    #[test]
    fn scenario_decay_reduces_manifestation_condition() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Create a knowledge + manifestation
        let kid = world.add_entity(
            EntityKind::Knowledge,
            "Test Knowledge".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Battle,
                source_event_id: ev,
                origin_settlement_id: settlement,
                origin_year: 100,
                significance: 0.5,
                ground_truth: serde_json::json!({"event_type": "battle"}),
            }),
            ev,
        );

        let mid = world.add_entity(
            EntityKind::Manifestation,
            "Test Memory".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Manifestation(ManifestationData {
                knowledge_id: kid,
                medium: Medium::OralTradition,
                content: serde_json::json!({"event_type": "battle"}),
                accuracy: 1.0,
                completeness: 1.0,
                distortions: serde_json::json!([]),
                derived_from_id: None,
                derivation_method: "witnessed".to_string(),
                condition: 0.5,
                created_year: 100,
            }),
            ev,
        );
        world.add_relationship(
            mid,
            settlement,
            RelationshipKind::HeldBy,
            SimTimestamp::from_year(100),
            ev,
        );

        let year_event = world.add_event(
            EventKind::Custom("test".into()),
            world.current_time,
            "test".into(),
        );
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        decay_manifestations(&mut ctx, SimTimestamp::from_year(100), 100, year_event);

        let cond = ctx
            .world
            .entities
            .get(&mid)
            .and_then(|e| e.data.as_manifestation())
            .map(|md| md.condition)
            .unwrap_or(0.0);
        // OralTradition decay = 0.02, so 0.5 - 0.02 = 0.48
        assert!(
            (cond - 0.48).abs() < 0.01,
            "condition should be ~0.48 after decay, got {cond}"
        );
    }

    #[test]
    fn scenario_destroy_decayed_removes_manifestation() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        let kid = world.add_entity(
            EntityKind::Knowledge,
            "K".into(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Founding,
                source_event_id: ev,
                origin_settlement_id: settlement,
                origin_year: 100,
                significance: 0.3,
                ground_truth: serde_json::json!({}),
            }),
            ev,
        );

        let mid = world.add_entity(
            EntityKind::Manifestation,
            "M".into(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Manifestation(ManifestationData {
                knowledge_id: kid,
                medium: Medium::Dream,
                content: serde_json::json!({}),
                accuracy: 0.5,
                completeness: 0.5,
                distortions: serde_json::json!([]),
                derived_from_id: None,
                derivation_method: "dreamed".into(),
                condition: 0.0, // already at zero
                created_year: 100,
            }),
            ev,
        );
        world.add_relationship(
            mid,
            settlement,
            RelationshipKind::HeldBy,
            SimTimestamp::from_year(100),
            ev,
        );

        let year_event = world.add_event(
            EventKind::Custom("test".into()),
            world.current_time,
            "test".into(),
        );
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        destroy_decayed(&mut ctx, SimTimestamp::from_year(100), year_event);

        assert!(
            ctx.world.entities.get(&mid).unwrap().end.is_some(),
            "decayed manifestation should be ended"
        );
        assert!(
            !signals.is_empty(),
            "should emit ManifestationDestroyed signal"
        );
    }

    #[test]
    fn medium_decay_rates() {
        assert!((Medium::Memory.decay_rate() - 0.05).abs() < 0.001);
        assert!((Medium::OralTradition.decay_rate() - 0.02).abs() < 0.001);
        assert!((Medium::WrittenBook.decay_rate() - 0.005).abs() < 0.001);
        assert!((Medium::CarvedStone.decay_rate() - 0.001).abs() < 0.001);
        assert!((Medium::Dream.decay_rate() - 0.10).abs() < 0.001);
        assert!((Medium::MagicalImprint.decay_rate() - 0.0).abs() < 0.001);
    }
}
