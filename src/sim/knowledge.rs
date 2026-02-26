use rand::{Rng, RngCore};

use super::context::TickContext;
use super::helpers::{self, entity_name};
use super::knowledge_derivation;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{
    BuildingType, EntityData, EntityKind, EventKind, KnowledgeCategory, KnowledgeData,
    ManifestationData, Medium, ParticipantRole, RelationshipKind, SecretDesire, SecretMotivation,
    SiegeOutcome, SimTimestamp,
};

// ---------------------------------------------------------------------------
// Significance — base values and scaling factors for knowledge creation
// ---------------------------------------------------------------------------

/// Base significance for knowledge from a war outcome.
const WAR_SIGNIFICANCE_BASE: f64 = 0.5;
/// Additional significance when the war outcome was decisive.
const WAR_DECISIVE_BONUS: f64 = 0.3;

/// Base significance for knowledge from a settlement conquest.
const CONQUEST_SIGNIFICANCE_BASE: f64 = 0.5;
/// Multiplier on settlement prestige added to conquest significance.
const CONQUEST_PRESTIGE_FACTOR: f64 = 0.2;

/// Significance assigned to a siege that ended in conquest.
const SIEGE_CONQUERED_SIGNIFICANCE: f64 = 0.4;

/// Minimum prestige a leader must have for their death to generate knowledge.
const LEADER_DEATH_PRESTIGE_THRESHOLD: f64 = 0.2;
/// Base significance for a notable leader's death.
const LEADER_DEATH_SIGNIFICANCE_BASE: f64 = 0.3;
/// Multiplier on prestige added to leader death significance.
const LEADER_DEATH_PRESTIGE_FACTOR: f64 = 0.4;

/// Significance assigned to a faction split event.
const FACTION_SPLIT_SIGNIFICANCE: f64 = 0.4;

/// Minimum disaster severity required to create knowledge.
const DISASTER_SEVERITY_THRESHOLD: f64 = 0.5;
/// Base significance for knowledge from a disaster.
const DISASTER_SIGNIFICANCE_BASE: f64 = 0.3;
/// Multiplier on severity added to disaster significance.
const DISASTER_SEVERITY_FACTOR: f64 = 0.4;

/// Minimum deaths for a plague to generate knowledge.
const PLAGUE_DEATH_THRESHOLD: u32 = 100;
/// Base significance for knowledge from a plague.
const PLAGUE_SIGNIFICANCE_BASE: f64 = 0.4;
/// Multiplier on normalized death count added to plague significance.
const PLAGUE_DEATH_FACTOR: f64 = 0.3;
/// Divisor for normalizing plague deaths into 0..1 range.
const PLAGUE_DEATH_NORMALIZATION: f64 = 1000.0;

/// Significance assigned to a cultural rebellion event.
const CULTURAL_REBELLION_SIGNIFICANCE: f64 = 0.3;

/// Significance assigned to temple/library construction.
const NOTABLE_CONSTRUCTION_SIGNIFICANCE: f64 = 0.2;

/// Significance assigned to a religious schism event.
const RELIGION_SCHISM_SIGNIFICANCE: f64 = 0.4;
/// Significance assigned to a religion founding event.
const RELIGION_FOUNDED_SIGNIFICANCE: f64 = 0.3;
/// Significance assigned to an alliance betrayal event.
const ALLIANCE_BETRAYAL_SIGNIFICANCE: f64 = 0.5;
const SUCCESSION_CRISIS_SIGNIFICANCE: f64 = 0.5;

// ---------------------------------------------------------------------------
// Decay — manifestation condition loss
// ---------------------------------------------------------------------------

/// Age (in years) after which a Memory holder's decay accelerates.
const MEMORY_HOLDER_AGE_THRESHOLD: u32 = 50;
/// Extra decay per year for memories held by people over the age threshold.
const MEMORY_OLD_AGE_EXTRA_DECAY: f64 = 0.02;

/// Maximum fraction of decay that can be prevented by library/temple preservation.
const MAX_PRESERVATION_BONUS: f64 = 0.8;

/// Minimum absolute condition change considered worth recording.
const CONDITION_CHANGE_EPSILON: f64 = 0.001;

// ---------------------------------------------------------------------------
// Propagation — oral tradition spread
// ---------------------------------------------------------------------------

/// Minimum accuracy a manifestation must have to be eligible for oral propagation.
const ORAL_PROPAGATION_MIN_ACCURACY: f64 = 0.2;
/// Minimum knowledge significance for oral propagation eligibility.
const ORAL_PROPAGATION_MIN_SIGNIFICANCE: f64 = 0.3;
/// Base propagation probability along trade routes (multiplied by accuracy * significance).
const TRADE_ROUTE_PROPAGATION_BASE: f64 = 0.15;
/// Base propagation probability to adjacent settlements (half the trade route rate).
const ADJACENT_PROPAGATION_BASE: f64 = 0.075;
/// Propagation probability multiplier for settlements with a port.
const PORT_PROPAGATION_BONUS: f64 = 1.5;

// ---------------------------------------------------------------------------
// Library activities — transcription and preservation
// ---------------------------------------------------------------------------

/// Annual probability that an oral tradition is transcribed into a written book.
const TRANSCRIPTION_PROBABILITY: f64 = 0.05;
/// Annual condition boost for written works in a library (preservation maintenance).
const LIBRARY_PRESERVATION_RATE: f64 = 0.001;
/// Minimum settlement literacy required for transcription.
const MIN_TRANSCRIPTION_LITERACY: f64 = 0.2;

// ---------------------------------------------------------------------------
// Secrets — suppression, leaking, and revelation
// ---------------------------------------------------------------------------

/// Propagation probability multiplier for keeper-controlled settlements (95% suppression).
const SECRET_KEEPER_PROPAGATION_FACTOR: f64 = 0.05;
/// Propagation probability multiplier for non-keeper settlements that still feel the chill.
const SECRET_NONKEEPER_PROPAGATION_FACTOR: f64 = 0.5;
/// Transcription probability multiplier — scribes reluctant to write down secrets.
const SECRET_TRANSCRIPTION_FACTOR: f64 = 0.1;
/// Number of non-keeper settlements with accurate manifestations needed to trigger revelation.
const SECRET_REVELATION_THRESHOLD: usize = 3;
/// Base probability per keeper-settlement per year that a secret leaks via gossip.
const SECRET_NATURAL_LEAK_PROB: f64 = 0.03;

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

        decay_manifestations(ctx, time, year_event);
        destroy_decayed(ctx, time, year_event);
        propagate_oral_traditions(ctx, time, year_event);
        copy_written_works(ctx, time, year_event);
        leak_secrets(ctx, time, year_event);
        check_secret_revelations(ctx, time, year_event);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let _year_event = ctx.world.add_event(
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
                } => handle_war_ended(
                    ctx,
                    time,
                    signal.event_id,
                    *winner_id,
                    *loser_id,
                    *decisive,
                    *reparations,
                ),
                SignalKind::SettlementCaptured {
                    settlement_id,
                    old_faction_id,
                    new_faction_id,
                } => handle_settlement_captured(
                    ctx,
                    time,
                    signal.event_id,
                    *settlement_id,
                    *old_faction_id,
                    *new_faction_id,
                ),
                SignalKind::SiegeEnded {
                    settlement_id,
                    outcome,
                    attacker_faction_id,
                    defender_faction_id,
                } => handle_siege_ended(
                    ctx,
                    time,
                    signal.event_id,
                    *settlement_id,
                    outcome,
                    *attacker_faction_id,
                    *defender_faction_id,
                ),
                SignalKind::EntityDied { entity_id } => {
                    handle_entity_died(ctx, time, signal.event_id, *entity_id);
                }
                SignalKind::FactionSplit {
                    old_faction_id,
                    new_faction_id,
                    settlement_id,
                } => handle_faction_split(
                    ctx,
                    time,
                    signal.event_id,
                    *old_faction_id,
                    *new_faction_id,
                    *settlement_id,
                ),
                SignalKind::DisasterStruck {
                    settlement_id,
                    disaster_type,
                    severity,
                    ..
                } => handle_disaster_struck(
                    ctx,
                    time,
                    signal.event_id,
                    *settlement_id,
                    disaster_type,
                    *severity,
                ),
                SignalKind::PlagueEnded {
                    settlement_id,
                    deaths,
                    disease_id,
                } => handle_plague_ended(
                    ctx,
                    time,
                    signal.event_id,
                    *settlement_id,
                    *deaths,
                    *disease_id,
                ),
                SignalKind::CulturalRebellion {
                    settlement_id,
                    faction_id,
                    culture_id,
                } => handle_cultural_rebellion(
                    ctx,
                    time,
                    signal.event_id,
                    *settlement_id,
                    *faction_id,
                    *culture_id,
                ),
                SignalKind::BuildingConstructed {
                    settlement_id,
                    building_type,
                    building_id,
                } => handle_building_constructed(
                    ctx,
                    time,
                    signal.event_id,
                    *settlement_id,
                    building_type,
                    *building_id,
                ),
                SignalKind::ItemTierPromoted {
                    item_id, new_tier, ..
                } if *new_tier >= 2 => {
                    handle_item_tier_promoted(ctx, time, signal.event_id, *item_id, *new_tier);
                }
                SignalKind::ItemCrafted {
                    item_id,
                    settlement_id,
                    crafter_id,
                    ..
                } => {
                    handle_item_crafted(
                        ctx,
                        time,
                        signal.event_id,
                        *item_id,
                        *settlement_id,
                        *crafter_id,
                    );
                }
                SignalKind::ReligionSchism {
                    parent_religion_id,
                    new_religion_id,
                    settlement_id,
                } => handle_religion_schism(
                    ctx,
                    time,
                    signal.event_id,
                    *parent_religion_id,
                    *new_religion_id,
                    *settlement_id,
                ),
                SignalKind::ReligionFounded {
                    religion_id,
                    settlement_id,
                    ..
                } => handle_religion_founded(
                    ctx,
                    time,
                    signal.event_id,
                    *religion_id,
                    *settlement_id,
                ),
                SignalKind::AllianceBetrayed {
                    betrayer_faction_id,
                    victim_faction_id,
                    betrayer_leader_id,
                } => handle_alliance_betrayal(
                    ctx,
                    time,
                    signal.event_id,
                    *betrayer_faction_id,
                    *victim_faction_id,
                    *betrayer_leader_id,
                ),
                SignalKind::SuccessionCrisis { faction_id, .. } => {
                    handle_succession_crisis(ctx, time, signal.event_id, *faction_id)
                }
                SignalKind::Custom { name, data } if name == "failed_coup" => {
                    handle_failed_coup(ctx, time, signal.event_id, data);
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Signal handlers — one per signal kind
// ---------------------------------------------------------------------------

fn handle_war_ended(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    winner_id: u64,
    loser_id: u64,
    decisive: bool,
    reparations: f64,
) {
    let significance = WAR_SIGNIFICANCE_BASE + if decisive { WAR_DECISIVE_BONUS } else { 0.0 };
    let capital = helpers::faction_capital_oldest(ctx.world, winner_id);
    if let Some(settlement_id) = capital {
        let winner_name = entity_name(ctx.world, winner_id);
        let loser_name = entity_name(ctx.world, loser_id);
        let (w_troops, l_troops) = get_faction_army_strengths(ctx.world, winner_id, loser_id);
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
            caused_by,
            KnowledgeCategory::Battle,
            significance,
            settlement_id,
            truth,
        );
    }
}

fn handle_settlement_captured(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    settlement_id: u64,
    old_faction_id: u64,
    new_faction_id: u64,
) {
    let settlement_prestige = get_settlement_prestige(ctx.world, settlement_id);
    let significance = CONQUEST_SIGNIFICANCE_BASE + CONQUEST_PRESTIGE_FACTOR * settlement_prestige;
    let settlement_name = entity_name(ctx.world, settlement_id);
    let old_name = entity_name(ctx.world, old_faction_id);
    let new_name = entity_name(ctx.world, new_faction_id);
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
        caused_by,
        KnowledgeCategory::Conquest,
        significance,
        settlement_id,
        truth,
    );

    // Conquest frees secrets: find manifestations at the captured settlement
    // whose knowledge was secret to the old faction, and spread them to the captor's capital.
    let captor_capital = helpers::faction_capital_oldest(ctx.world, new_faction_id);
    if let Some(captor_capital) = captor_capital {
        // Collect old faction's secret knowledge IDs
        let old_secrets: Vec<u64> = ctx
            .world
            .entities
            .get(&old_faction_id)
            .and_then(|e| e.data.as_faction())
            .map(|fd| fd.secrets.keys().copied().collect())
            .unwrap_or_default();

        if !old_secrets.is_empty() {
            // Find manifestations at the captured settlement for secret knowledge
            let secret_manifs: Vec<u64> = ctx
                .world
                .entities
                .values()
                .filter(|e| {
                    e.kind == EntityKind::Manifestation
                        && e.end.is_none()
                        && e.has_active_rel(RelationshipKind::HeldBy, settlement_id)
                })
                .filter_map(|e| {
                    let md = e.data.as_manifestation()?;
                    if old_secrets.contains(&md.knowledge_id) && md.accuracy >= 0.3 {
                        Some(e.id)
                    } else {
                        None
                    }
                })
                .collect();

            for manif_id in secret_manifs {
                let ev = ctx.world.add_caused_event(
                    EventKind::SecretCaptured,
                    time,
                    format!(
                        "Secret knowledge captured at {} by {}",
                        settlement_name,
                        entity_name(ctx.world, new_faction_id)
                    ),
                    caused_by,
                );
                if let Some(new_id) = knowledge_derivation::derive(
                    ctx.world,
                    ctx.rng,
                    manif_id,
                    Medium::OralTradition,
                    captor_capital,
                    time,
                    ev,
                    None,
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
                            settlement_id: captor_capital,
                            medium: Medium::OralTradition,
                        },
                    });
                }
            }
        }
    }
}

fn handle_siege_ended(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    settlement_id: u64,
    outcome: &SiegeOutcome,
    attacker_faction_id: u64,
    defender_faction_id: u64,
) {
    if *outcome == SiegeOutcome::Conquered {
        let truth = serde_json::json!({
            "event_type": "conquest",
            "settlement_id": settlement_id,
            "settlement_name": entity_name(ctx.world, settlement_id),
            "attacker_faction_name": entity_name(ctx.world, attacker_faction_id),
            "defender_faction_name": entity_name(ctx.world, defender_faction_id),
            "outcome": outcome,
            "year": time.year()
        });
        create_knowledge(
            ctx,
            time,
            caused_by,
            KnowledgeCategory::Conquest,
            SIEGE_CONQUERED_SIGNIFICANCE,
            settlement_id,
            truth,
        );
    }
}

fn handle_entity_died(ctx: &mut TickContext, time: SimTimestamp, caused_by: u64, entity_id: u64) {
    if let Some(entity) = ctx.world.entities.get(&entity_id)
        && entity.kind == EntityKind::Person
    {
        let prestige = entity.data.as_person().map(|p| p.prestige).unwrap_or(0.0);
        if prestige > LEADER_DEATH_PRESTIGE_THRESHOLD {
            let person_name = entity.name.clone();
            let faction_id = entity.active_rel(RelationshipKind::LeaderOf);
            if let Some(fid) = faction_id
                && let Some(sid) = helpers::faction_capital_oldest(ctx.world, fid)
            {
                let faction_name = entity_name(ctx.world, fid);
                let significance =
                    LEADER_DEATH_SIGNIFICANCE_BASE + LEADER_DEATH_PRESTIGE_FACTOR * prestige;
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
                    caused_by,
                    KnowledgeCategory::Dynasty,
                    significance,
                    sid,
                    truth,
                );
            }
        }
    }
}

fn handle_faction_split(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    old_faction_id: u64,
    new_faction_id: Option<u64>,
    settlement_id: u64,
) {
    let new_id = new_faction_id.unwrap_or(0);
    let truth = serde_json::json!({
        "event_type": "faction_split",
        "old_faction_id": old_faction_id,
        "old_faction_name": entity_name(ctx.world, old_faction_id),
        "new_faction_id": new_id,
        "new_faction_name": entity_name(ctx.world, new_id),
        "settlement_id": settlement_id,
        "year": time.year()
    });
    create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Dynasty,
        FACTION_SPLIT_SIGNIFICANCE,
        settlement_id,
        truth,
    );
}

fn handle_disaster_struck(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    settlement_id: u64,
    disaster_type: &crate::model::DisasterType,
    severity: f64,
) {
    if severity > DISASTER_SEVERITY_THRESHOLD {
        let settlement_name = entity_name(ctx.world, settlement_id);
        let significance = DISASTER_SIGNIFICANCE_BASE + DISASTER_SEVERITY_FACTOR * severity;
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
            caused_by,
            KnowledgeCategory::Disaster,
            significance,
            settlement_id,
            truth,
        );
    }
}

fn handle_plague_ended(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    settlement_id: u64,
    deaths: u32,
    disease_id: u64,
) {
    if deaths > PLAGUE_DEATH_THRESHOLD {
        let settlement_name = entity_name(ctx.world, settlement_id);
        let significance = PLAGUE_SIGNIFICANCE_BASE
            + PLAGUE_DEATH_FACTOR * (deaths as f64 / PLAGUE_DEATH_NORMALIZATION).min(1.0);
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
            caused_by,
            KnowledgeCategory::Disaster,
            significance,
            settlement_id,
            truth,
        );
    }
}

fn handle_cultural_rebellion(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    settlement_id: u64,
    faction_id: u64,
    culture_id: u64,
) {
    let truth = serde_json::json!({
        "event_type": "cultural_rebellion",
        "settlement_id": settlement_id,
        "settlement_name": entity_name(ctx.world, settlement_id),
        "faction_id": faction_id,
        "culture_id": culture_id,
        "year": time.year()
    });
    create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Cultural,
        CULTURAL_REBELLION_SIGNIFICANCE,
        settlement_id,
        truth,
    );
}

fn handle_building_constructed(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    settlement_id: u64,
    building_type: &BuildingType,
    building_id: u64,
) {
    if *building_type == BuildingType::Temple || *building_type == BuildingType::Library {
        let truth = serde_json::json!({
            "event_type": "construction",
            "settlement_id": settlement_id,
            "settlement_name": entity_name(ctx.world, settlement_id),
            "building_type": building_type,
            "building_id": building_id,
            "year": time.year()
        });
        create_knowledge(
            ctx,
            time,
            caused_by,
            KnowledgeCategory::Construction,
            NOTABLE_CONSTRUCTION_SIGNIFICANCE,
            settlement_id,
            truth,
        );
    }
}

fn handle_item_tier_promoted(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    item_id: u64,
    new_tier: u8,
) {
    // Create knowledge about this legendary item
    let item_name = entity_name(ctx.world, item_id);
    let settlement_id = ctx
        .world
        .entities
        .get(&item_id)
        .and_then(|e| e.active_rel(RelationshipKind::HeldBy))
        .and_then(|holder_id| {
            let holder = ctx.world.entities.get(&holder_id)?;
            match holder.kind {
                EntityKind::Settlement => Some(holder_id),
                EntityKind::Person => {
                    holder.active_rel(RelationshipKind::LocatedIn).or_else(|| {
                        holder
                            .active_rel(RelationshipKind::MemberOf)
                            .and_then(|fid| {
                                helpers::faction_settlements(ctx.world, fid)
                                    .into_iter()
                                    .next()
                            })
                    })
                }
                _ => None,
            }
        });

    let Some(sid) = settlement_id else { return };

    let tier_name = match new_tier {
        2 => "Renowned",
        3 => "Legendary",
        _ => "Notable",
    };

    let truth = serde_json::json!({
        "event_type": "item_renowned",
        "item_id": item_id,
        "item_name": item_name,
        "tier": new_tier,
        "tier_name": tier_name,
        "year": time.year()
    });

    let significance = if new_tier >= 3 { 0.6 } else { 0.3 };
    create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Cultural,
        significance,
        sid,
        truth,
    );
}

fn handle_item_crafted(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    item_id: u64,
    settlement_id: u64,
    crafter_id: Option<u64>,
) {
    // Only create knowledge for notable crafters (prestige > 0.3)
    let crafter_notable = crafter_id.is_some_and(|cid| {
        ctx.world
            .entities
            .get(&cid)
            .and_then(|e| e.data.as_person())
            .is_some_and(|pd| pd.prestige > 0.3)
    });

    if !crafter_notable {
        return;
    }

    let item_name = entity_name(ctx.world, item_id);
    let crafter_name = crafter_id
        .map(|cid| entity_name(ctx.world, cid))
        .unwrap_or_default();

    let truth = serde_json::json!({
        "event_type": "notable_crafting",
        "item_id": item_id,
        "item_name": item_name,
        "crafter_id": crafter_id,
        "crafter_name": crafter_name,
        "settlement_id": settlement_id,
        "year": time.year()
    });

    create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Cultural,
        0.2,
        settlement_id,
        truth,
    );
}

fn handle_religion_schism(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    parent_religion_id: u64,
    new_religion_id: u64,
    settlement_id: u64,
) {
    let truth = serde_json::json!({
        "event_type": "religion_schism",
        "parent_religion_id": parent_religion_id,
        "parent_religion_name": entity_name(ctx.world, parent_religion_id),
        "new_religion_id": new_religion_id,
        "new_religion_name": entity_name(ctx.world, new_religion_id),
        "settlement_id": settlement_id,
        "year": time.year()
    });
    create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Religious,
        RELIGION_SCHISM_SIGNIFICANCE,
        settlement_id,
        truth,
    );
}

fn handle_religion_founded(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    religion_id: u64,
    settlement_id: u64,
) {
    let truth = serde_json::json!({
        "event_type": "religion_founded",
        "religion_id": religion_id,
        "religion_name": entity_name(ctx.world, religion_id),
        "settlement_id": settlement_id,
        "year": time.year()
    });
    create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Religious,
        RELIGION_FOUNDED_SIGNIFICANCE,
        settlement_id,
        truth,
    );
}

fn handle_succession_crisis(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    faction_id: u64,
) {
    let settlement_id = helpers::faction_capital_oldest(ctx.world, faction_id);
    let Some(settlement_id) = settlement_id else {
        return;
    };

    let truth = serde_json::json!({
        "event_type": "succession_crisis",
        "faction_id": faction_id,
        "faction_name": entity_name(ctx.world, faction_id),
        "year": time.year()
    });
    create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Dynasty,
        SUCCESSION_CRISIS_SIGNIFICANCE,
        settlement_id,
        truth,
    );
}

fn handle_alliance_betrayal(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    betrayer_faction_id: u64,
    victim_faction_id: u64,
    betrayer_leader_id: u64,
) {
    // Create knowledge at victim's capital
    let settlement_id = helpers::faction_capital_oldest(ctx.world, victim_faction_id)
        .or_else(|| helpers::faction_capital_oldest(ctx.world, betrayer_faction_id));
    let Some(settlement_id) = settlement_id else {
        return;
    };

    let truth = serde_json::json!({
        "event_type": "alliance_betrayal",
        "betrayer_faction_id": betrayer_faction_id,
        "betrayer_faction_name": entity_name(ctx.world, betrayer_faction_id),
        "victim_faction_id": victim_faction_id,
        "victim_faction_name": entity_name(ctx.world, victim_faction_id),
        "betrayer_leader_id": betrayer_leader_id,
        "betrayer_leader_name": entity_name(ctx.world, betrayer_leader_id),
        "year": time.year()
    });
    let knowledge_id = create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Dynasty,
        ALLIANCE_BETRAYAL_SIGNIFICANCE,
        settlement_id,
        truth,
    );

    // Betrayer faction wants to suppress knowledge of their treachery
    if let Some(entity) = ctx.world.entities.get_mut(&betrayer_faction_id)
        && let Some(fd) = entity.data.as_faction_mut()
    {
        fd.secrets.insert(
            knowledge_id,
            SecretDesire {
                motivation: SecretMotivation::Shameful,
                sensitivity: 0.7,
                accuracy_threshold: 0.3,
                created: time,
            },
        );
    }
}

fn handle_failed_coup(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    data: &serde_json::Value,
) {
    let faction_id = data["faction_id"].as_u64().unwrap_or(0);
    let actor_id = data["actor_id"].as_u64().unwrap_or(0);
    let leader_id = data["leader_id"].as_u64().unwrap_or(0);

    let settlement_id = helpers::faction_capital_oldest(ctx.world, faction_id);
    let Some(settlement_id) = settlement_id else {
        return;
    };

    let truth = serde_json::json!({
        "event_type": "failed_coup",
        "faction_id": faction_id,
        "faction_name": entity_name(ctx.world, faction_id),
        "actor_id": actor_id,
        "actor_name": entity_name(ctx.world, actor_id),
        "leader_id": leader_id,
        "leader_name": entity_name(ctx.world, leader_id),
        "year": time.year()
    });
    let knowledge_id = create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Dynasty,
        0.4,
        settlement_id,
        truth,
    );

    // The faction wants to hide its internal instability
    if let Some(entity) = ctx.world.entities.get_mut(&faction_id)
        && let Some(fd) = entity.data.as_faction_mut()
    {
        fd.secrets.insert(
            knowledge_id,
            SecretDesire {
                motivation: SecretMotivation::Shameful,
                sensitivity: 0.8,
                accuracy_threshold: 0.3,
                created: time,
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Knowledge creation helper
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn create_knowledge(
    ctx: &mut TickContext,
    time: SimTimestamp,
    caused_by: u64,
    category: KnowledgeCategory,
    significance: f64,
    settlement_id: u64,
    ground_truth: serde_json::Value,
) -> u64 {
    let signal_category = category;
    let knowledge_name = format!(
        "{} at {}",
        capitalize_category(&category),
        entity_name(ctx.world, settlement_id)
    );

    // Create knowledge entity
    let ev = ctx.world.add_caused_event(
        EventKind::Discovery,
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
            origin_time: time,
            significance,
            ground_truth: ground_truth.clone(),
            revealed_at: None,
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
            distortions: Vec::new(),
            derived_from_id: None,
            derivation_method: crate::model::DerivationMethod::Witnessed,
            condition: 1.0,
            created: time,
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
            category: signal_category,
            significance,
        },
    });
    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::ManifestationCreated {
            manifestation_id: mid,
            knowledge_id: kid,
            settlement_id,
            medium: Medium::Memory,
        },
    });

    kid
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
        KnowledgeCategory::Religious => "Religious Event",
    }
}

// ---------------------------------------------------------------------------
// Tick phase 1: Decay manifestations
// ---------------------------------------------------------------------------

fn decay_manifestations(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
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
            let holder_id = e.active_rel(RelationshipKind::HeldBy);
            if let Some(hid) = holder_id
                && let Some(holder) = ctx.world.entities.get(&hid)
            {
                if let Some(pd) = holder.data.as_person() {
                    let age = time.years_since(pd.born);
                    if age > MEMORY_HOLDER_AGE_THRESHOLD {
                        decay += MEMORY_OLD_AGE_EXTRA_DECAY;
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
            let holder_id = e.active_rel(RelationshipKind::HeldBy);
            if let Some(hid) = holder_id
                && let Some(holder) = ctx.world.entities.get(&hid)
                && holder.end.is_some()
            {
                decay = 1.0;
            }
        }

        // Library/Temple bonus: reduce decay for manifestations in settlements with these buildings
        let settlement_id = e.active_rel(RelationshipKind::HeldBy).and_then(|hid| {
            let holder = ctx.world.entities.get(&hid)?;
            if holder.kind == EntityKind::Settlement {
                Some(hid)
            } else {
                // Check if holder (person) is in a settlement
                holder
                    .active_rel(RelationshipKind::MemberOf)
                    .and_then(|fid| {
                        let faction = ctx.world.entities.get(&fid)?;
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
            let entity = ctx.world.entities.get(&sid);
            let library_bonus = entity
                .and_then(|e| e.data.as_settlement())
                .map(|sd| sd.building_bonuses.library)
                .unwrap_or(0.0);
            let temple_bonus = entity
                .and_then(|e| e.data.as_settlement())
                .map(|sd| sd.building_bonuses.temple_knowledge)
                .unwrap_or(0.0);
            let preservation = (library_bonus + temple_bonus).min(MAX_PRESERVATION_BONUS);
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

        if (d.old_condition - new_condition).abs() > CONDITION_CHANGE_EPSILON {
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
                let settlement_id = e.active_rel(RelationshipKind::HeldBy).unwrap_or(0);
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
            EventKind::Destruction,
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
    let settlement_knowledge = build_settlement_knowledge_map(ctx.world);

    // Collect manifestations-per-settlement for merge candidate selection
    let settlement_manifests = build_settlement_manifestation_map(ctx.world);

    // Collect propagation candidates: (source_manifestation_id, target_settlement_id, probability)
    struct PropCandidate {
        source_manif_id: u64,
        source_knowledge_id: u64,
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
            .active_rels(RelationshipKind::TradeRoute)
            .collect();

        // Adjacent settlements (via region adjacency)
        let adjacent_settlements: Vec<u64> = settlement
            .active_rels(RelationshipKind::AdjacentTo)
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
                    && e.has_active_rel(RelationshipKind::HeldBy, sid)
            })
            .filter_map(|e| {
                let md = e.data.as_manifestation()?;
                if (md.medium == Medium::OralTradition || md.medium == Medium::Song)
                    && md.accuracy > ORAL_PROPAGATION_MIN_ACCURACY
                {
                    // Get significance from knowledge
                    let significance = ctx
                        .world
                        .entities
                        .get(&md.knowledge_id)
                        .and_then(|k| k.data.as_knowledge())
                        .map(|kd| kd.significance)
                        .unwrap_or(0.0);
                    if significance > ORAL_PROPAGATION_MIN_SIGNIFICANCE {
                        Some((e.id, md.knowledge_id, md.accuracy, significance))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        // Port cities spread knowledge faster
        let port_mult = if ctx.world.settlement(sid).building_bonuses.port_trade > 0.0 {
            PORT_PROPAGATION_BONUS
        } else {
            1.0
        };

        for (manif_id, knowledge_id, accuracy, significance) in &oral_manifests {
            // Secret suppression: reduce propagation from keeper-controlled settlements
            let secret_mult =
                secret_propagation_multiplier(ctx.world, *knowledge_id, *accuracy, sid);

            // Trade route partners
            for &partner in &trade_partners {
                let partner_has = settlement_knowledge
                    .get(&partner)
                    .is_some_and(|s| s.contains(knowledge_id));
                if !partner_has {
                    let target_literacy = helpers::settlement_literacy(ctx.world, partner);
                    let literacy_factor = 0.7 + 0.3 * target_literacy;
                    let prob = TRADE_ROUTE_PROPAGATION_BASE
                        * accuracy
                        * significance
                        * secret_mult
                        * literacy_factor
                        * port_mult;
                    candidates.push(PropCandidate {
                        source_manif_id: *manif_id,
                        source_knowledge_id: *knowledge_id,
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
                    let target_literacy = helpers::settlement_literacy(ctx.world, adj);
                    let literacy_factor = 0.7 + 0.3 * target_literacy;
                    let prob = ADJACENT_PROPAGATION_BASE
                        * accuracy
                        * significance
                        * secret_mult
                        * literacy_factor
                        * port_mult;
                    candidates.push(PropCandidate {
                        source_manif_id: *manif_id,
                        source_knowledge_id: *knowledge_id,
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
            // Select a merge candidate: a manifestation at the target settlement
            // with a different knowledge_id (represents the receiving community's
            // existing oral traditions contaminating the incoming story).
            let distortion_ctx = select_merge_candidate(
                &settlement_manifests,
                c.target_settlement_id,
                c.source_knowledge_id,
                ctx.rng,
            );

            let ev = ctx.world.add_caused_event(
                EventKind::Propagation,
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
                distortion_ctx.as_ref(),
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
                        medium: Medium::OralTradition,
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
            e.data
                .as_settlement()
                .is_some_and(|sd| sd.building_bonuses.library > 0.0)
        })
        .map(|e| e.id)
        .collect();

    struct TranscriptionCandidate {
        source_manif_id: u64,
        knowledge_id: u64,
        settlement_id: u64,
    }

    struct PreservationCandidate {
        manif_id: u64,
        old_condition: f64,
        settlement_id: u64,
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
                    && e.has_active_rel(RelationshipKind::HeldBy, sid)
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

        let written_knowledge: std::collections::BTreeSet<u64> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Manifestation
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::HeldBy, sid)
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
                    knowledge_id: *knowledge_id,
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
                    && e.has_active_rel(RelationshipKind::HeldBy, sid)
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
                settlement_id: sid,
            });
        }
    }

    // Apply transcriptions
    for tc in transcriptions {
        // Literacy barrier: settlements with low literacy cannot transcribe
        let literacy = helpers::settlement_literacy(ctx.world, tc.settlement_id);
        if literacy < MIN_TRANSCRIPTION_LITERACY {
            continue;
        }

        // Secret suppression: scribes are reluctant to write down known secrets
        let secret_mult = if faction_has_secret(ctx.world, tc.settlement_id, tc.knowledge_id, 0.3) {
            SECRET_TRANSCRIPTION_FACTOR
        } else {
            1.0
        };
        // Literacy boost: up to 2x at full literacy
        let literacy_mult = 1.0 + literacy;
        if ctx.rng.random_range(0.0..1.0) < TRANSCRIPTION_PROBABILITY * secret_mult * literacy_mult
        {
            let ev = ctx.world.add_caused_event(
                EventKind::Transcription,
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
                None,
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
                        medium: Medium::WrittenBook,
                    },
                });
            }
        }
    }

    // Apply preservation (slow annual condition boost from library maintenance)
    for p in preservations {
        let literacy = helpers::settlement_literacy(ctx.world, p.settlement_id);
        let new_condition =
            (p.old_condition + LIBRARY_PRESERVATION_RATE * (1.0 + literacy)).min(1.0);
        if let Some(entity) = ctx.world.entities.get_mut(&p.manif_id)
            && let Some(md) = entity.data.as_manifestation_mut()
        {
            md.condition = new_condition;
        }
    }
}

// ---------------------------------------------------------------------------
// Tick phase 5: Leak secrets via natural gossip
// ---------------------------------------------------------------------------

fn leak_secrets(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    // Collect faction secrets: (faction_id, knowledge_id, sensitivity, accuracy_threshold)
    struct LeakCandidate {
        sensitivity: f64,
        source_settlement_id: u64,
        target_settlement_id: u64,
        source_manif_id: u64,
    }

    let mut candidates: Vec<LeakCandidate> = Vec::new();

    // Gather faction-level secrets
    let faction_secrets: Vec<(u64, u64, f64, f64)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .flat_map(|e| {
            let fd = e.data.as_faction()?;
            Some(
                fd.secrets
                    .iter()
                    .map(move |(&kid, desire)| {
                        (e.id, kid, desire.sensitivity, desire.accuracy_threshold)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect();

    for (faction_id, knowledge_id, sensitivity, accuracy_threshold) in &faction_secrets {
        // Find keeper-controlled settlements that hold an accurate manifestation of this knowledge
        let keeper_settlements = helpers::faction_settlements(ctx.world, *faction_id);
        for &sid in &keeper_settlements {
            // Find a manifestation of this knowledge at this settlement with accuracy above threshold
            let manif = ctx.world.entities.values().find(|e| {
                e.kind == EntityKind::Manifestation
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::HeldBy, sid)
                    && e.data.as_manifestation().is_some_and(|md| {
                        md.knowledge_id == *knowledge_id && md.accuracy >= *accuracy_threshold
                    })
            });

            let Some(manif) = manif else { continue };
            let manif_id = manif.id;

            // Find adjacent non-keeper settlements to leak to
            let region_id = ctx
                .world
                .entities
                .get(&sid)
                .and_then(|e| e.active_rel(RelationshipKind::LocatedIn));
            let Some(region_id) = region_id else { continue };

            for adj_region in helpers::adjacent_regions(ctx.world, region_id) {
                // Find settlement in adjacent region
                for e in ctx.world.entities.values() {
                    if e.kind != EntityKind::Settlement || e.end.is_some() {
                        continue;
                    }
                    if !e.has_active_rel(RelationshipKind::LocatedIn, adj_region) {
                        continue;
                    }
                    // Skip if this settlement is also keeper-controlled
                    if helpers::settlement_faction(ctx.world, e.id)
                        .is_some_and(|fid| fid == *faction_id)
                    {
                        continue;
                    }
                    candidates.push(LeakCandidate {
                        sensitivity: *sensitivity,
                        source_settlement_id: sid,
                        target_settlement_id: e.id,
                        source_manif_id: manif_id,
                    });
                }
            }
        }
    }

    // Roll for each candidate
    for c in candidates {
        let prob = SECRET_NATURAL_LEAK_PROB * c.sensitivity;
        if ctx.rng.random_range(0.0..1.0) < prob {
            let ev = ctx.world.add_caused_event(
                EventKind::SecretLeaked,
                time,
                format!(
                    "Secret gossip leaked from {} to {}",
                    entity_name(ctx.world, c.source_settlement_id),
                    entity_name(ctx.world, c.target_settlement_id),
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
                None,
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
                        medium: Medium::OralTradition,
                    },
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tick phase 6: Check if secrets have been widely revealed
// ---------------------------------------------------------------------------

fn check_secret_revelations(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    // Collect all (entity_id, knowledge_id, desire) pairs from factions and persons
    struct SecretEntry {
        keeper_id: u64,
        knowledge_id: u64,
        motivation: SecretMotivation,
        sensitivity: f64,
        accuracy_threshold: f64,
        is_faction: bool,
    }

    let entries: Vec<SecretEntry> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.end.is_none())
        .flat_map(|e| {
            let mut out = Vec::new();
            if let Some(fd) = e.data.as_faction() {
                for (&kid, desire) in &fd.secrets {
                    out.push(SecretEntry {
                        keeper_id: e.id,
                        knowledge_id: kid,
                        motivation: desire.motivation,
                        sensitivity: desire.sensitivity,
                        accuracy_threshold: desire.accuracy_threshold,
                        is_faction: true,
                    });
                }
            }
            if let Some(pd) = e.data.as_person() {
                for (&kid, desire) in &pd.secrets {
                    out.push(SecretEntry {
                        keeper_id: e.id,
                        knowledge_id: kid,
                        motivation: desire.motivation,
                        sensitivity: desire.sensitivity,
                        accuracy_threshold: desire.accuracy_threshold,
                        is_faction: false,
                    });
                }
            }
            out
        })
        .collect();

    // For each secret, count non-keeper settlements with accurate manifestations
    struct Revelation {
        keeper_id: u64,
        knowledge_id: u64,
        motivation: SecretMotivation,
        sensitivity: f64,
    }

    let mut to_reveal: Vec<Revelation> = Vec::new();

    for entry in &entries {
        // Find keeper's settlements (faction → owned settlements, person → person's settlement)
        let keeper_settlement_ids: Vec<u64> = if entry.is_faction {
            helpers::faction_settlements(ctx.world, entry.keeper_id)
        } else {
            ctx.world
                .entities
                .get(&entry.keeper_id)
                .and_then(|e| e.active_rel(RelationshipKind::LocatedIn))
                .into_iter()
                .collect()
        };

        // Count distinct non-keeper settlements with accurate manifestations
        let mut non_keeper_count = 0usize;
        for e in ctx.world.entities.values() {
            if e.kind != EntityKind::Manifestation || e.end.is_some() {
                continue;
            }
            let Some(md) = e.data.as_manifestation() else {
                continue;
            };
            if md.knowledge_id != entry.knowledge_id {
                continue;
            }
            if md.accuracy < entry.accuracy_threshold {
                continue;
            }
            let Some(holder_id) = e.active_rel(RelationshipKind::HeldBy) else {
                continue;
            };
            // Check if holder is a non-keeper settlement
            let is_settlement = ctx
                .world
                .entities
                .get(&holder_id)
                .is_some_and(|h| h.kind == EntityKind::Settlement && h.end.is_none());
            if is_settlement && !keeper_settlement_ids.contains(&holder_id) {
                non_keeper_count += 1;
            }
        }

        if non_keeper_count >= SECRET_REVELATION_THRESHOLD {
            to_reveal.push(Revelation {
                keeper_id: entry.keeper_id,
                knowledge_id: entry.knowledge_id,
                motivation: entry.motivation,
                sensitivity: entry.sensitivity,
            });
        }
    }

    // Apply revelations
    for r in to_reveal {
        let ev = ctx.world.add_caused_event(
            EventKind::SecretRevealed,
            time,
            format!(
                "A secret of {} was widely revealed",
                entity_name(ctx.world, r.keeper_id),
            ),
            year_event,
        );

        // Mark the knowledge entity as revealed
        ctx.world.knowledge_mut(r.knowledge_id).revealed_at = Some(time);
        ctx.world.record_change(
            r.knowledge_id,
            ev,
            "revealed_at",
            serde_json::json!(null),
            serde_json::json!(time.year()),
        );

        // Remove the SecretDesire from the keeper
        if let Some(entity) = ctx.world.entities.get_mut(&r.keeper_id) {
            if let Some(fd) = entity.data.as_faction_mut() {
                fd.secrets.remove(&r.knowledge_id);
            }
            if let Some(pd) = entity.data.as_person_mut() {
                pd.secrets.remove(&r.knowledge_id);
            }
        }

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::SecretRevealed {
                knowledge_id: r.knowledge_id,
                keeper_id: r.keeper_id,
                motivation: r.motivation,
                sensitivity: r.sensitivity,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Settlement knowledge map — shared by propagation & transcription phases
// ---------------------------------------------------------------------------

/// Build a map of settlement_id -> set of knowledge IDs held at that settlement.
/// Used by both `propagate_oral_traditions` and `copy_written_works` to avoid
/// duplicate propagation.
fn build_settlement_knowledge_map(
    world: &crate::model::World,
) -> std::collections::BTreeMap<u64, std::collections::BTreeSet<u64>> {
    let mut map: std::collections::BTreeMap<u64, std::collections::BTreeSet<u64>> =
        std::collections::BTreeMap::new();
    for e in world.entities.values() {
        if e.kind != EntityKind::Manifestation || e.end.is_some() {
            continue;
        }
        let Some(md) = e.data.as_manifestation() else {
            continue;
        };
        if let Some(sid) = e.active_rel(RelationshipKind::HeldBy) {
            map.entry(sid).or_default().insert(md.knowledge_id);
        }
    }
    map
}

/// Build a map of settlement_id -> Vec of (manifestation_id, knowledge_id, content)
/// for oral/song manifestations. Used to find merge candidates for MergedWithOther.
fn build_settlement_manifestation_map(
    world: &crate::model::World,
) -> std::collections::BTreeMap<u64, Vec<(u64, u64, serde_json::Value)>> {
    let mut map: std::collections::BTreeMap<u64, Vec<(u64, u64, serde_json::Value)>> =
        std::collections::BTreeMap::new();
    for e in world.entities.values() {
        if e.kind != EntityKind::Manifestation || e.end.is_some() {
            continue;
        }
        let Some(md) = e.data.as_manifestation() else {
            continue;
        };
        if md.medium != Medium::OralTradition
            && md.medium != Medium::Song
            && md.medium != Medium::Memory
        {
            continue;
        }
        if let Some(sid) = e.active_rel(RelationshipKind::HeldBy) {
            map.entry(sid)
                .or_default()
                .push((e.id, md.knowledge_id, md.content.clone()));
        }
    }
    map
}

/// Pick a random manifestation at the target settlement with a different knowledge_id.
/// Returns a DistortionContext if a candidate is found, None otherwise.
fn select_merge_candidate(
    settlement_manifests: &std::collections::BTreeMap<u64, Vec<(u64, u64, serde_json::Value)>>,
    target_settlement_id: u64,
    source_knowledge_id: u64,
    rng: &mut dyn RngCore,
) -> Option<knowledge_derivation::DistortionContext> {
    let target_manifs = settlement_manifests.get(&target_settlement_id)?;
    let eligible: Vec<_> = target_manifs
        .iter()
        .filter(|(_, kid, _)| *kid != source_knowledge_id)
        .collect();
    if eligible.is_empty() {
        return None;
    }
    let idx = rng.random_range(0..eligible.len());
    let (mid, kid, content) = eligible[idx];
    Some(knowledge_derivation::DistortionContext {
        merge_source: Some(knowledge_derivation::MergeSource {
            manifestation_id: *mid,
            content: content.clone(),
            knowledge_id: *kid,
        }),
    })
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

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
        let faction_id = e.active_rel(RelationshipKind::MemberOf);
        let strength = e.data.as_army().map(|a| a.strength).unwrap_or(0);
        match faction_id {
            Some(id) if id == faction_a => a_troops += strength,
            Some(id) if id == faction_b => b_troops += strength,
            _ => {}
        }
    }
    (a_troops.max(100), b_troops.max(100))
}

// ---------------------------------------------------------------------------
// Secret suppression helpers
// ---------------------------------------------------------------------------

/// Returns a propagation multiplier based on whether the source settlement's
/// controlling faction or any person there wants to suppress this knowledge.
/// 1.0 = no suppression, lower = suppressed.
fn secret_propagation_multiplier(
    world: &crate::model::World,
    knowledge_id: u64,
    manifestation_accuracy: f64,
    source_settlement_id: u64,
) -> f64 {
    // Check owning faction's secrets
    if let Some(faction_id) = helpers::settlement_faction(world, source_settlement_id)
        && let Some(faction) = world.entities.get(&faction_id)
        && let Some(fd) = faction.data.as_faction()
        && let Some(desire) = fd.secrets.get(&knowledge_id)
        && manifestation_accuracy >= desire.accuracy_threshold
    {
        return SECRET_KEEPER_PROPAGATION_FACTOR;
    }

    // Check person-level secrets: any living person at this settlement
    for e in world.entities.values() {
        if e.kind != EntityKind::Person || e.end.is_some() {
            continue;
        }
        if !e.has_active_rel(RelationshipKind::LocatedIn, source_settlement_id) {
            continue;
        }
        if let Some(pd) = e.data.as_person()
            && let Some(desire) = pd.secrets.get(&knowledge_id)
            && manifestation_accuracy >= desire.accuracy_threshold
        {
            return SECRET_NONKEEPER_PROPAGATION_FACTOR;
        }
    }

    1.0
}

/// Check if a settlement's controlling faction has a SecretDesire for the given knowledge.
fn faction_has_secret(
    world: &crate::model::World,
    settlement_id: u64,
    knowledge_id: u64,
    accuracy: f64,
) -> bool {
    if let Some(faction_id) = helpers::settlement_faction(world, settlement_id)
        && let Some(faction) = world.entities.get(&faction_id)
        && let Some(fd) = faction.data.as_faction()
        && let Some(desire) = fd.secrets.get(&knowledge_id)
    {
        accuracy >= desire.accuracy_threshold
    } else {
        false
    }
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
        let faction = s.faction("TestFaction").treasury(500.0).id();
        let settlement = s
            .settlement("TestTown", faction, region)
            .population(500)
            .prosperity(0.7)
            .prestige(0.3)
            .id();
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
            EntityData::default_for_kind(EntityKind::Faction),
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
                origin_time: SimTimestamp::from_year(100),
                significance: 0.5,
                ground_truth: serde_json::json!({"event_type": "battle"}),
                revealed_at: None,
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
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: crate::model::DerivationMethod::Witnessed,
                condition: 0.5,
                created: SimTimestamp::from_year(100),
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

        decay_manifestations(&mut ctx, SimTimestamp::from_year(100), year_event);

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
                origin_time: SimTimestamp::from_year(100),
                significance: 0.3,
                ground_truth: serde_json::json!({}),
                revealed_at: None,
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
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: crate::model::DerivationMethod::Dreamed,
                condition: 0.0, // already at zero
                created: SimTimestamp::from_year(100),
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

    #[test]
    fn scenario_oral_propagation_with_merge() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();

        // Create a second settlement connected via trade route
        let region2 = world.add_entity(
            EntityKind::Region,
            "Region2".into(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Region),
            ev,
        );
        let settlement2 = world.add_entity(
            EntityKind::Settlement,
            "Fartown".into(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Settlement),
            ev,
        );
        world.add_relationship(
            settlement2,
            region2,
            RelationshipKind::LocatedIn,
            SimTimestamp::from_year(1),
            ev,
        );
        // Trade route between settlements
        world.add_relationship(
            settlement,
            settlement2,
            RelationshipKind::TradeRoute,
            SimTimestamp::from_year(1),
            ev,
        );

        // Create knowledge A at settlement 1 (high significance so it propagates)
        let kid_a = world.add_entity(
            EntityKind::Knowledge,
            "Battle of Ironhold".into(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Battle,
                source_event_id: ev,
                origin_settlement_id: settlement,
                origin_time: SimTimestamp::from_year(100),
                significance: 0.9,
                ground_truth: serde_json::json!({
                    "event_type": "battle",
                    "name": "Battle of Ironhold",
                    "year": 100,
                    "attacker": {"faction_name": "Northmen", "troops": 500},
                    "defender": {"faction_name": "Southfolk", "troops": 300},
                }),
                revealed_at: None,
            }),
            ev,
        );
        let mid_a = world.add_entity(
            EntityKind::Manifestation,
            "Battle of Ironhold (oral)".into(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Manifestation(ManifestationData {
                knowledge_id: kid_a,
                medium: Medium::OralTradition,
                content: serde_json::json!({
                    "event_type": "battle",
                    "name": "Battle of Ironhold",
                    "year": 100,
                    "attacker": {"faction_name": "Northmen", "troops": 500},
                    "defender": {"faction_name": "Southfolk", "troops": 300},
                }),
                accuracy: 0.9,
                completeness: 0.9,
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: crate::model::DerivationMethod::Witnessed,
                condition: 0.8,
                created: SimTimestamp::from_year(100),
            }),
            ev,
        );
        world.add_relationship(
            mid_a,
            settlement,
            RelationshipKind::HeldBy,
            SimTimestamp::from_year(100),
            ev,
        );

        // Create knowledge B at settlement 2 (the merge candidate)
        let kid_b = world.add_entity(
            EntityKind::Knowledge,
            "Founding of Fartown".into(),
            Some(SimTimestamp::from_year(50)),
            EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Founding,
                source_event_id: ev,
                origin_settlement_id: settlement2,
                origin_time: SimTimestamp::from_year(50),
                significance: 0.5,
                ground_truth: serde_json::json!({
                    "event_type": "founding",
                    "name": "Founding of Fartown",
                    "year": 50,
                    "attacker": {"faction_name": "Wanderers", "troops": 100},
                }),
                revealed_at: None,
            }),
            ev,
        );
        let mid_b = world.add_entity(
            EntityKind::Manifestation,
            "Founding of Fartown (oral)".into(),
            Some(SimTimestamp::from_year(50)),
            EntityData::Manifestation(ManifestationData {
                knowledge_id: kid_b,
                medium: Medium::OralTradition,
                content: serde_json::json!({
                    "event_type": "founding",
                    "name": "Founding of Fartown",
                    "year": 50,
                    "attacker": {"faction_name": "Wanderers", "troops": 100},
                }),
                accuracy: 0.8,
                completeness: 0.8,
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: crate::model::DerivationMethod::Witnessed,
                condition: 0.7,
                created: SimTimestamp::from_year(50),
            }),
            ev,
        );
        world.add_relationship(
            mid_b,
            settlement2,
            RelationshipKind::HeldBy,
            SimTimestamp::from_year(50),
            ev,
        );

        // Verify the settlement manifestation map finds the merge candidate
        let manif_map = build_settlement_manifestation_map(&world);
        assert!(
            manif_map.get(&settlement2).is_some_and(|v| !v.is_empty()),
            "settlement2 should have manifestations for merge"
        );

        // Verify merge candidate selection works
        let mut rng = SmallRng::seed_from_u64(42);
        let ctx = select_merge_candidate(&manif_map, settlement2, kid_a, &mut rng);
        assert!(
            ctx.is_some(),
            "should find a merge candidate (kid_b) at settlement2"
        );
        let ctx = ctx.unwrap();
        let ms = ctx.merge_source.as_ref().unwrap();
        assert_eq!(
            ms.knowledge_id, kid_b,
            "merge candidate should be knowledge B"
        );

        // Run propagation and check results
        let year_event = world.add_event(
            EventKind::Custom("tick".into()),
            SimTimestamp::from_year(101),
            "tick".into(),
        );
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        propagate_oral_traditions(&mut ctx, SimTimestamp::from_year(101), year_event);

        // Check if any new manifestations were created at settlement2
        let new_manifs: Vec<_> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Manifestation
                    && e.id != mid_a
                    && e.id != mid_b
                    && e.has_active_rel(RelationshipKind::HeldBy, settlement2)
            })
            .collect();

        // Propagation is probabilistic, so we just verify the system runs without panics.
        // If a new manifestation was created, verify it looks reasonable.
        for m in &new_manifs {
            let md = m.data.as_manifestation().unwrap();
            assert_eq!(md.knowledge_id, kid_a, "propagated knowledge should be A");
            assert!(md.accuracy >= 0.0 && md.accuracy <= 1.0);
        }
    }

    #[test]
    fn scenario_settlement_captured_creates_knowledge() {
        let (mut world, ev, faction, settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Create a second faction (the new owner)
        let new_faction = world.add_entity(
            EntityKind::Faction,
            "Conquerors".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );

        let signal = Signal {
            event_id: ev,
            kind: SignalKind::SettlementCaptured {
                settlement_id: settlement,
                old_faction_id: faction,
                new_faction_id: new_faction,
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

        let knowledge_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert!(
            knowledge_count > 0,
            "should create knowledge from SettlementCaptured"
        );

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
    fn scenario_entity_died_leader_creates_dynasty_knowledge() {
        let (mut world, ev, faction, _settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Create a person who is a faction leader with high prestige
        let leader = world.add_entity(
            EntityKind::Person,
            "King Aldric".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Person),
            ev,
        );
        // Set prestige above the threshold (0.2)
        if let Some(person) = world.entity_mut(leader).data.as_person_mut() {
            person.prestige = 0.5;
        }
        // Add LeaderOf relationship to the faction
        world.add_relationship(
            leader,
            faction,
            RelationshipKind::LeaderOf,
            SimTimestamp::from_year(1),
            ev,
        );
        // Add MemberOf relationship
        world.add_relationship(
            leader,
            faction,
            RelationshipKind::MemberOf,
            SimTimestamp::from_year(1),
            ev,
        );

        let signal = Signal {
            event_id: ev,
            kind: SignalKind::EntityDied { entity_id: leader },
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

        let knowledge_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert!(
            knowledge_count > 0,
            "should create dynasty knowledge from leader EntityDied"
        );

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
    fn scenario_disaster_struck_creates_knowledge() {
        use crate::model::entity_data::DisasterType;

        let (mut world, ev, _faction, settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Find the region the settlement is in
        let region = world
            .entity(settlement)
            .active_rel(RelationshipKind::LocatedIn)
            .expect("settlement should have a region");

        let signal = Signal {
            event_id: ev,
            kind: SignalKind::DisasterStruck {
                settlement_id: settlement,
                region_id: region,
                disaster_type: DisasterType::Earthquake,
                severity: 0.8,
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

        let knowledge_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert!(
            knowledge_count > 0,
            "should create knowledge from DisasterStruck"
        );

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
    fn scenario_building_constructed_creates_knowledge() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Create a building entity — only Temple and Library trigger knowledge
        let building = world.add_entity(
            EntityKind::Building,
            "Great Temple".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::default_for_kind(EntityKind::Building),
            ev,
        );

        let signal = Signal {
            event_id: ev,
            kind: SignalKind::BuildingConstructed {
                settlement_id: settlement,
                building_type: BuildingType::Temple,
                building_id: building,
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

        let knowledge_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert!(
            knowledge_count > 0,
            "should create knowledge from BuildingConstructed"
        );

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
    fn scenario_item_tier_promoted_creates_knowledge() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Create an item entity held by the settlement
        let item = world.add_entity(
            EntityKind::Item,
            "Ancient Sword".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::default_for_kind(EntityKind::Item),
            ev,
        );
        // Item must be held by a settlement so the handler can find a location
        world.add_relationship(
            item,
            settlement,
            RelationshipKind::HeldBy,
            SimTimestamp::from_year(100),
            ev,
        );

        let signal = Signal {
            event_id: ev,
            kind: SignalKind::ItemTierPromoted {
                item_id: item,
                old_tier: 1,
                new_tier: 2,
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

        let knowledge_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert!(
            knowledge_count > 0,
            "should create knowledge from ItemTierPromoted"
        );

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
    fn scenario_religion_schism_creates_knowledge() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Create parent and new religion entities
        let parent_religion = world.add_entity(
            EntityKind::Religion,
            "OldFaith".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Religion),
            ev,
        );
        let new_religion = world.add_entity(
            EntityKind::Religion,
            "NewFaith".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::default_for_kind(EntityKind::Religion),
            ev,
        );

        let signal = Signal {
            event_id: ev,
            kind: SignalKind::ReligionSchism {
                parent_religion_id: parent_religion,
                new_religion_id: new_religion,
                settlement_id: settlement,
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

        let knowledge_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert!(
            knowledge_count > 0,
            "should create knowledge from ReligionSchism"
        );

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
    fn scenario_alliance_betrayal_creates_knowledge() {
        let (mut world, ev, faction, _settlement) = knowledge_scenario();
        let mut rng = SmallRng::seed_from_u64(42);

        // Create a betrayer faction and its leader
        let betrayer = world.add_entity(
            EntityKind::Faction,
            "Betrayers".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );
        let betrayer_leader = world.add_entity(
            EntityKind::Person,
            "Lord Treachery".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Person),
            ev,
        );

        // Use the existing faction (which has a settlement via knowledge_scenario) as the victim
        let signal = Signal {
            event_id: ev,
            kind: SignalKind::AllianceBetrayed {
                betrayer_faction_id: betrayer,
                victim_faction_id: faction,
                betrayer_leader_id: betrayer_leader,
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

        let knowledge_count = ctx
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert!(
            knowledge_count > 0,
            "should create knowledge from AllianceBetrayed"
        );

        let kc_signals: Vec<_> = signals_out
            .iter()
            .filter(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. }))
            .collect();
        assert!(
            !kc_signals.is_empty(),
            "should emit KnowledgeCreated signal"
        );
    }

    // -----------------------------------------------------------------------
    // Signal handler tests using deliver_signals
    // -----------------------------------------------------------------------
    //
    // NOTE: KnowledgeCreated signal is NOT handled by the Knowledge system
    // (it falls into the `_ => {}` catch-all in handle_signals). The signal
    // is *emitted* by the Knowledge system for other systems to consume, but
    // the Knowledge system itself does not react to it. No test needed.
    // -----------------------------------------------------------------------

    #[test]
    fn scenario_siege_ended_creates_knowledge() {
        let (mut world, ev, faction, settlement) = knowledge_scenario();

        // Create an attacker faction
        let attacker = world.add_entity(
            EntityKind::Faction,
            "Invaders".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );

        // SiegeEnded with outcome Conquered should create Conquest knowledge
        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::SiegeEnded {
                    settlement_id: settlement,
                    attacker_faction_id: attacker,
                    defender_faction_id: faction,
                    outcome: SiegeOutcome::Conquered,
                },
            }],
            42,
        );

        let knowledge: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .collect();
        assert_eq!(
            knowledge.len(),
            1,
            "SiegeEnded(Conquered) should create one knowledge entity"
        );

        let kd = knowledge[0].data.as_knowledge().unwrap();
        assert_eq!(kd.category, KnowledgeCategory::Conquest);
        assert!((kd.significance - SIEGE_CONQUERED_SIGNIFICANCE).abs() < 0.001);
        assert_eq!(kd.origin_settlement_id, settlement);

        // Should also create a Memory manifestation at the settlement
        let manifs: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Manifestation)
            .collect();
        assert_eq!(manifs.len(), 1, "should create one manifestation");
        let md = manifs[0].data.as_manifestation().unwrap();
        assert_eq!(md.medium, Medium::Memory);
        assert!((md.accuracy - 1.0).abs() < 0.001);

        // Should emit KnowledgeCreated + ManifestationCreated signals
        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should emit KnowledgeCreated signal"
        );
        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::ManifestationCreated { .. })),
            "should emit ManifestationCreated signal"
        );
    }

    #[test]
    fn scenario_siege_ended_lifted_no_knowledge() {
        let (mut world, ev, faction, settlement) = knowledge_scenario();

        let attacker = world.add_entity(
            EntityKind::Faction,
            "Invaders".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );

        // SiegeEnded with outcome Lifted should NOT create knowledge
        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::SiegeEnded {
                    settlement_id: settlement,
                    attacker_faction_id: attacker,
                    defender_faction_id: faction,
                    outcome: SiegeOutcome::Lifted,
                },
            }],
            42,
        );

        let knowledge_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert_eq!(
            knowledge_count, 0,
            "SiegeEnded(Lifted) should not create knowledge"
        );
        assert!(
            !signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should not emit KnowledgeCreated for lifted siege"
        );
    }

    #[test]
    fn scenario_faction_split_creates_knowledge() {
        let (mut world, ev, faction, settlement) = knowledge_scenario();

        // Create a new faction (the split-off)
        let new_faction = world.add_entity(
            EntityKind::Faction,
            "Rebels".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );

        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::FactionSplit {
                    old_faction_id: faction,
                    new_faction_id: Some(new_faction),
                    settlement_id: settlement,
                },
            }],
            42,
        );

        let knowledge: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .collect();
        assert_eq!(
            knowledge.len(),
            1,
            "FactionSplit should create one knowledge entity"
        );

        let kd = knowledge[0].data.as_knowledge().unwrap();
        assert_eq!(kd.category, KnowledgeCategory::Dynasty);
        assert!((kd.significance - FACTION_SPLIT_SIGNIFICANCE).abs() < 0.001);
        assert_eq!(kd.origin_settlement_id, settlement);

        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should emit KnowledgeCreated signal"
        );
    }

    #[test]
    fn scenario_plague_ended_creates_knowledge() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();

        // Create a disease entity
        let disease = world.add_entity(
            EntityKind::Disease,
            "Red Plague".to_string(),
            Some(SimTimestamp::from_year(90)),
            EntityData::default_for_kind(EntityKind::Disease),
            ev,
        );

        // deaths > PLAGUE_DEATH_THRESHOLD (100) should create Disaster knowledge
        let deaths = 500u32;
        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::PlagueEnded {
                    settlement_id: settlement,
                    disease_id: disease,
                    deaths,
                },
            }],
            42,
        );

        let knowledge: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .collect();
        assert_eq!(
            knowledge.len(),
            1,
            "PlagueEnded with deaths > 100 should create knowledge"
        );

        let kd = knowledge[0].data.as_knowledge().unwrap();
        assert_eq!(kd.category, KnowledgeCategory::Disaster);
        // significance = 0.4 + 0.3 * (500/1000).min(1.0) = 0.4 + 0.15 = 0.55
        let expected_sig = PLAGUE_SIGNIFICANCE_BASE
            + PLAGUE_DEATH_FACTOR * (deaths as f64 / PLAGUE_DEATH_NORMALIZATION).min(1.0);
        assert!(
            (kd.significance - expected_sig).abs() < 0.001,
            "significance should be {expected_sig}, got {}",
            kd.significance
        );

        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should emit KnowledgeCreated signal"
        );
    }

    #[test]
    fn scenario_plague_ended_below_threshold_no_knowledge() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();

        let disease = world.add_entity(
            EntityKind::Disease,
            "Minor Cough".to_string(),
            Some(SimTimestamp::from_year(90)),
            EntityData::default_for_kind(EntityKind::Disease),
            ev,
        );

        // deaths <= PLAGUE_DEATH_THRESHOLD (100) should NOT create knowledge
        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::PlagueEnded {
                    settlement_id: settlement,
                    disease_id: disease,
                    deaths: 50,
                },
            }],
            42,
        );

        let knowledge_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert_eq!(
            knowledge_count, 0,
            "PlagueEnded with deaths <= 100 should not create knowledge"
        );
        assert!(
            !signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should not emit KnowledgeCreated for minor plague"
        );
    }

    #[test]
    fn scenario_cultural_rebellion_creates_knowledge() {
        let (mut world, ev, faction, settlement) = knowledge_scenario();

        // Create a culture entity
        let culture = world.add_entity(
            EntityKind::Culture,
            "Highland Culture".to_string(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(EntityKind::Culture),
            ev,
        );

        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::CulturalRebellion {
                    settlement_id: settlement,
                    faction_id: faction,
                    culture_id: culture,
                },
            }],
            42,
        );

        let knowledge: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .collect();
        assert_eq!(
            knowledge.len(),
            1,
            "CulturalRebellion should create one knowledge entity"
        );

        let kd = knowledge[0].data.as_knowledge().unwrap();
        assert_eq!(kd.category, KnowledgeCategory::Cultural);
        assert!((kd.significance - CULTURAL_REBELLION_SIGNIFICANCE).abs() < 0.001);
        assert_eq!(kd.origin_settlement_id, settlement);

        // Verify the ground truth JSON contains expected fields
        let truth = &kd.ground_truth;
        assert_eq!(truth["event_type"], "cultural_rebellion");
        assert_eq!(truth["settlement_id"], settlement);
        assert_eq!(truth["faction_id"], faction);
        assert_eq!(truth["culture_id"], culture);

        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should emit KnowledgeCreated signal"
        );
    }

    #[test]
    fn scenario_item_crafted_creates_knowledge() {
        let (mut world, ev, faction, settlement) = knowledge_scenario();

        // Create a person with prestige > 0.3 (the threshold for notable crafter)
        let crafter = world.add_entity(
            EntityKind::Person,
            "Master Smith".to_string(),
            Some(SimTimestamp::from_year(50)),
            EntityData::default_for_kind(EntityKind::Person),
            ev,
        );
        if let Some(pd) = world.entity_mut(crafter).data.as_person_mut() {
            pd.prestige = 0.5;
        }
        world.add_relationship(
            crafter,
            faction,
            RelationshipKind::MemberOf,
            SimTimestamp::from_year(50),
            ev,
        );

        // Create an item
        let item = world.add_entity(
            EntityKind::Item,
            "Steel Sword".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::default_for_kind(EntityKind::Item),
            ev,
        );

        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::ItemCrafted {
                    item_id: item,
                    settlement_id: settlement,
                    crafter_id: Some(crafter),
                    item_type: crate::model::ItemType::Weapon,
                },
            }],
            42,
        );

        let knowledge: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .collect();
        assert_eq!(
            knowledge.len(),
            1,
            "ItemCrafted with notable crafter should create knowledge"
        );

        let kd = knowledge[0].data.as_knowledge().unwrap();
        assert_eq!(kd.category, KnowledgeCategory::Cultural);
        assert!(
            (kd.significance - 0.2).abs() < 0.001,
            "significance should be 0.2"
        );
        assert_eq!(kd.origin_settlement_id, settlement);

        let truth = &kd.ground_truth;
        assert_eq!(truth["event_type"], "notable_crafting");
        assert_eq!(truth["crafter_name"], "Master Smith");

        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should emit KnowledgeCreated signal"
        );
    }

    #[test]
    fn scenario_item_crafted_low_prestige_no_knowledge() {
        let (mut world, ev, faction, settlement) = knowledge_scenario();

        // Create a person with prestige <= 0.3 (below threshold)
        let crafter = world.add_entity(
            EntityKind::Person,
            "Apprentice".to_string(),
            Some(SimTimestamp::from_year(80)),
            EntityData::default_for_kind(EntityKind::Person),
            ev,
        );
        if let Some(pd) = world.entity_mut(crafter).data.as_person_mut() {
            pd.prestige = 0.1;
        }
        world.add_relationship(
            crafter,
            faction,
            RelationshipKind::MemberOf,
            SimTimestamp::from_year(80),
            ev,
        );

        let item = world.add_entity(
            EntityKind::Item,
            "Wooden Bowl".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::default_for_kind(EntityKind::Item),
            ev,
        );

        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::ItemCrafted {
                    item_id: item,
                    settlement_id: settlement,
                    crafter_id: Some(crafter),
                    item_type: crate::model::ItemType::Pottery,
                },
            }],
            42,
        );

        let knowledge_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert_eq!(
            knowledge_count, 0,
            "ItemCrafted with low-prestige crafter should not create knowledge"
        );
        assert!(
            !signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should not emit KnowledgeCreated for low-prestige crafter"
        );
    }

    #[test]
    fn scenario_item_crafted_no_crafter_no_knowledge() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();

        let item = world.add_entity(
            EntityKind::Item,
            "Found Gem".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::default_for_kind(EntityKind::Item),
            ev,
        );

        // crafter_id = None should not create knowledge
        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::ItemCrafted {
                    item_id: item,
                    settlement_id: settlement,
                    crafter_id: None,
                    item_type: crate::model::ItemType::Jewelry,
                },
            }],
            42,
        );

        let knowledge_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .count();
        assert_eq!(
            knowledge_count, 0,
            "ItemCrafted with no crafter should not create knowledge"
        );
        assert!(
            !signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should not emit KnowledgeCreated without crafter"
        );
    }

    #[test]
    fn scenario_religion_founded_creates_knowledge() {
        let (mut world, ev, _faction, settlement) = knowledge_scenario();

        // Create a religion entity
        let religion = world.add_entity(
            EntityKind::Religion,
            "The Eternal Flame".to_string(),
            Some(SimTimestamp::from_year(100)),
            EntityData::default_for_kind(EntityKind::Religion),
            ev,
        );

        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::ReligionFounded {
                    religion_id: religion,
                    settlement_id: settlement,
                    founder_id: None,
                },
            }],
            42,
        );

        let knowledge: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .collect();
        assert_eq!(
            knowledge.len(),
            1,
            "ReligionFounded should create one knowledge entity"
        );

        let kd = knowledge[0].data.as_knowledge().unwrap();
        assert_eq!(kd.category, KnowledgeCategory::Religious);
        assert!((kd.significance - RELIGION_FOUNDED_SIGNIFICANCE).abs() < 0.001);
        assert_eq!(kd.origin_settlement_id, settlement);

        let truth = &kd.ground_truth;
        assert_eq!(truth["event_type"], "religion_founded");
        assert_eq!(truth["religion_name"], "The Eternal Flame");

        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should emit KnowledgeCreated signal"
        );
        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::ManifestationCreated { .. })),
            "should emit ManifestationCreated signal"
        );
    }

    #[test]
    fn scenario_succession_crisis_creates_knowledge() {
        let (mut world, ev, faction, settlement) = knowledge_scenario();

        // SuccessionCrisis needs the faction to have a capital (settlement owned by faction).
        // knowledge_scenario already sets that up via Scenario builder.

        // Create a new leader and claimant
        let new_leader = world.add_entity(
            EntityKind::Person,
            "Prince Heir".to_string(),
            Some(SimTimestamp::from_year(80)),
            EntityData::default_for_kind(EntityKind::Person),
            ev,
        );
        let claimant = world.add_entity(
            EntityKind::Person,
            "Duke Rival".to_string(),
            Some(SimTimestamp::from_year(75)),
            EntityData::default_for_kind(EntityKind::Person),
            ev,
        );

        let signals = crate::testutil::deliver_signals(
            &mut world,
            &mut KnowledgeSystem,
            &[Signal {
                event_id: ev,
                kind: SignalKind::SuccessionCrisis {
                    faction_id: faction,
                    new_leader_id: new_leader,
                    claimant_ids: vec![claimant],
                },
            }],
            42,
        );

        let knowledge: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Knowledge)
            .collect();
        assert_eq!(
            knowledge.len(),
            1,
            "SuccessionCrisis should create one knowledge entity"
        );

        let kd = knowledge[0].data.as_knowledge().unwrap();
        assert_eq!(kd.category, KnowledgeCategory::Dynasty);
        assert!((kd.significance - SUCCESSION_CRISIS_SIGNIFICANCE).abs() < 0.001);
        // Origin should be the faction's capital settlement
        assert_eq!(kd.origin_settlement_id, settlement);

        let truth = &kd.ground_truth;
        assert_eq!(truth["event_type"], "succession_crisis");
        assert_eq!(truth["faction_id"], faction);

        assert!(
            signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::KnowledgeCreated { .. })),
            "should emit KnowledgeCreated signal"
        );
    }

    // -----------------------------------------------------------------------
    // Secrets system tests
    // -----------------------------------------------------------------------

    /// Helper: create a two-settlement scenario with a knowledge + manifestation at settlement A,
    /// where faction A has a SecretDesire for that knowledge. Trade route connects them.
    fn secret_scenario() -> (World, u64, u64, u64, u64, u64) {
        let mut s = crate::scenario::Scenario::at_year(100);
        let r1 = s.add_region("Region1");
        let r2 = s.add_region("Region2");
        s.make_adjacent(r1, r2);
        let faction_a = s.faction("FactionA").treasury(500.0).id();
        let faction_b = s.faction("FactionB").treasury(500.0).id();
        let settlement_a = s
            .settlement("TownA", faction_a, r1)
            .population(500)
            .prosperity(0.7)
            .id();
        let settlement_b = s
            .settlement("TownB", faction_b, r2)
            .population(500)
            .prosperity(0.7)
            .id();

        // Trade route (bidirectional)
        s.make_trade_route(settlement_a, settlement_b);

        // Knowledge at settlement A
        let knowledge = s.add_knowledge_with(
            "Secret Intel",
            KnowledgeCategory::Dynasty,
            settlement_a,
            |kd| {
                kd.significance = 0.8;
            },
        );
        s.add_manifestation_with(
            "Secret Intel (oral)",
            knowledge,
            Medium::OralTradition,
            settlement_a,
            |md| {
                md.accuracy = 0.9;
                md.completeness = 0.9;
            },
        );

        // Faction A wants to suppress this knowledge
        s.add_secret(
            faction_a,
            knowledge,
            crate::model::SecretMotivation::Shameful,
            0.7,
        );

        let world = s.build();
        (
            world,
            faction_a,
            faction_b,
            settlement_a,
            settlement_b,
            knowledge,
        )
    }

    #[test]
    fn scenario_secret_suppresses_propagation() {
        // Test that the suppression multiplier is correctly applied
        let (world, _faction_a, _faction_b, settlement_a, _settlement_b, knowledge) =
            secret_scenario();

        // With the secret in place, multiplier should be very low
        let mult_with_secret = secret_propagation_multiplier(&world, knowledge, 0.9, settlement_a);
        assert!(
            (mult_with_secret - SECRET_KEEPER_PROPAGATION_FACTOR).abs() < f64::EPSILON,
            "keeper settlement should have strong suppression, got {mult_with_secret}"
        );
    }

    #[test]
    fn scenario_degraded_secret_not_suppressed() {
        // When the manifestation accuracy is below the threshold, it shouldn't be suppressed
        let (mut world, _faction_a, _faction_b, settlement_a, _settlement_b, knowledge) =
            secret_scenario();

        // Set the manifestation accuracy below the threshold (0.3)
        for e in world.entities.values_mut() {
            if e.kind == EntityKind::Manifestation
                && e.data
                    .as_manifestation()
                    .is_some_and(|md| md.knowledge_id == knowledge)
            {
                e.data.as_manifestation_mut().unwrap().accuracy = 0.1;
            }
        }

        // The secret_propagation_multiplier should return 1.0 for accuracy below threshold
        let mult = secret_propagation_multiplier(&world, knowledge, 0.1, settlement_a);
        assert!(
            (mult - 1.0).abs() < f64::EPSILON,
            "degraded manifestation should not be suppressed, got multiplier {mult}"
        );
    }

    #[test]
    fn scenario_secret_revelation_threshold() {
        // Place accurate manifestations at 3 non-keeper settlements — should trigger revelation
        let mut s = crate::scenario::Scenario::at_year(100);

        // Keeper faction and settlement
        let r_keeper = s.add_region("KeeperRegion");
        let faction_keeper = s.faction("Keeper").treasury(500.0).id();
        let s_keeper = s
            .settlement("KeeperTown", faction_keeper, r_keeper)
            .population(500)
            .id();

        // Knowledge
        let knowledge =
            s.add_knowledge_with("Big Secret", KnowledgeCategory::Dynasty, s_keeper, |kd| {
                kd.significance = 0.8;
            });
        s.add_secret(
            faction_keeper,
            knowledge,
            crate::model::SecretMotivation::Strategic,
            0.6,
        );

        // 3 non-keeper settlements, each with an accurate manifestation
        for i in 0..3 {
            let r = s.add_region(&format!("Region{i}"));
            s.make_adjacent(r_keeper, r);
            let f = s.faction(&format!("Faction{i}")).id();
            let sett = s.settlement(&format!("Town{i}"), f, r).population(200).id();
            s.add_manifestation_with(
                &format!("Leaked copy {i}"),
                knowledge,
                Medium::OralTradition,
                sett,
                |md| {
                    md.accuracy = 0.8;
                },
            );
        }

        let mut world = s.build();
        let signals = crate::testutil::tick_system(&mut world, &mut KnowledgeSystem, 101, 42);

        // Should have emitted SecretRevealed signal
        let revealed = signals
            .iter()
            .any(|s| matches!(s.kind, SignalKind::SecretRevealed { .. }));
        assert!(
            revealed,
            "should emit SecretRevealed when threshold reached"
        );

        // Secret should be removed from keeper
        let secrets = &world.faction(faction_keeper).secrets;
        assert!(
            !secrets.contains_key(&knowledge),
            "secret should be removed after revelation"
        );
    }

    #[test]
    fn scenario_secret_transcription_suppressed() {
        // Test that faction_has_secret correctly detects secrets and would apply
        // the SECRET_TRANSCRIPTION_FACTOR
        let mut s = crate::scenario::Scenario::at_year(100);
        let r = s.add_region("R");
        let faction = s.faction("F").treasury(500.0).id();
        let settlement = s.settlement("Town", faction, r).population(500).id();

        let knowledge = s.add_knowledge_with(
            "Secret Lore",
            KnowledgeCategory::Dynasty,
            settlement,
            |kd| {
                kd.significance = 0.8;
            },
        );

        // Add secret
        s.add_secret(
            faction,
            knowledge,
            crate::model::SecretMotivation::Sacred,
            0.8,
        );

        let world = s.build();

        // faction_has_secret should detect it at accuracy >= threshold (0.3)
        assert!(
            faction_has_secret(&world, settlement, knowledge, 0.5),
            "settlement's faction should have a secret for this knowledge at accuracy 0.5"
        );

        // With accuracy below threshold (0.3), should NOT detect it
        assert!(
            !faction_has_secret(&world, settlement, knowledge, 0.2),
            "accuracy 0.2 is below threshold 0.3, should not trigger"
        );
    }

    #[test]
    fn scenario_capture_frees_secrets() {
        let (mut world, faction_a, faction_b, settlement_a, settlement_b, knowledge) =
            secret_scenario();

        // Deliver SettlementCaptured signal — B captures A's settlement
        let ev = world.events.keys().next().copied().unwrap();
        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::SettlementCaptured {
                settlement_id: settlement_a,
                old_faction_id: faction_a,
                new_faction_id: faction_b,
            },
        }];
        crate::testutil::deliver_signals(&mut world, &mut KnowledgeSystem, &inbox, 42);

        // Captor's capital (settlement_b) should now have a manifestation of the secret knowledge
        let has_at_captor_capital = world.entities.values().any(|e| {
            e.kind == EntityKind::Manifestation
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::HeldBy, settlement_b)
                && e.data
                    .as_manifestation()
                    .is_some_and(|md| md.knowledge_id == knowledge)
        });
        assert!(
            has_at_captor_capital,
            "conquest should spread secret manifestations to captor's capital"
        );
    }

    #[test]
    fn scenario_betrayal_creates_secret_desire() {
        let mut s = crate::scenario::Scenario::at_year(100);
        let r1 = s.add_region("R1");
        let r2 = s.add_region("R2");
        let victim = s.faction("Victims").treasury(500.0).id();
        let _victim_town = s.settlement("VictimTown", victim, r1).population(500).id();
        let betrayer = s.faction("Betrayers").treasury(500.0).id();
        let _betrayer_town = s
            .settlement("BetrayerTown", betrayer, r2)
            .population(500)
            .id();
        let betrayer_leader = s.person("Treacherous Lord", betrayer).id();
        let mut world = s.build();

        let ev = world.events.keys().next().copied().unwrap();
        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::AllianceBetrayed {
                betrayer_faction_id: betrayer,
                victim_faction_id: victim,
                betrayer_leader_id: betrayer_leader,
            },
        }];
        crate::testutil::deliver_signals(&mut world, &mut KnowledgeSystem, &inbox, 42);

        // Betrayer faction should have a SecretDesire
        let fd = world.faction(betrayer);
        assert!(
            !fd.secrets.is_empty(),
            "betrayer faction should have secret desire after betrayal"
        );
        let (_, desire) = fd.secrets.iter().next().unwrap();
        assert_eq!(desire.motivation, crate::model::SecretMotivation::Shameful);
        assert!((desire.sensitivity - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn scenario_secret_revealed_prestige_penalty() {
        use crate::sim::reputation::ReputationSystem;

        let mut s = crate::scenario::Scenario::at_year(100);
        let r = s.add_region("R");
        let faction = s.faction("Keeper").prestige(0.5).id();
        let _settlement = s.settlement("Town", faction, r).population(500).id();
        let leader = s.person("Leader", faction).prestige(0.5).id();
        s.make_leader(leader, faction);
        let mut world = s.build();

        let ev = world.add_event(
            EventKind::Custom("test".into()),
            SimTimestamp::from_year(100),
            "test".into(),
        );
        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::SecretRevealed {
                knowledge_id: 999,
                keeper_id: faction,
                motivation: crate::model::SecretMotivation::Shameful,
                sensitivity: 1.0,
            },
        }];
        crate::testutil::deliver_signals(&mut world, &mut ReputationSystem, &inbox, 42);

        assert!(
            world.faction(faction).prestige < 0.5,
            "shameful revelation should reduce faction prestige, got {}",
            world.faction(faction).prestige
        );
        assert!(
            world.person(leader).prestige < 0.5,
            "shameful revelation should reduce leader prestige, got {}",
            world.person(leader).prestige
        );
    }

    #[test]
    fn scenario_secret_revealed_stability_hit() {
        use crate::sim::politics::PoliticsSystem;

        let mut s = crate::scenario::Scenario::at_year(100);
        let r = s.add_region("R");
        let faction = s.faction("Keeper").stability(0.8).happiness(0.8).id();
        let _settlement = s.settlement("Town", faction, r).population(500).id();
        let mut world = s.build();

        let ev = world.add_event(
            EventKind::Custom("test".into()),
            SimTimestamp::from_year(100),
            "test".into(),
        );
        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::SecretRevealed {
                knowledge_id: 999,
                keeper_id: faction,
                motivation: crate::model::SecretMotivation::Strategic,
                sensitivity: 1.0,
            },
        }];
        crate::testutil::deliver_signals(&mut world, &mut PoliticsSystem, &inbox, 42);

        assert!(
            world.faction(faction).stability < 0.8,
            "strategic revelation should reduce stability, got {}",
            world.faction(faction).stability
        );
    }
}
