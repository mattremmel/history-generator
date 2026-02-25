use rand::{Rng, RngCore};

use super::context::TickContext;
use super::extra_keys as K;
use super::helpers::{self, entity_name};
use super::knowledge_derivation;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{
    BuildingType, EntityData, EntityKind, EventKind, KnowledgeCategory, KnowledgeData,
    ManifestationData, Medium, ParticipantRole, RelationshipKind, SiegeOutcome, SimTimestamp,
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

// ---------------------------------------------------------------------------
// Library activities — transcription and preservation
// ---------------------------------------------------------------------------

/// Annual probability that an oral tradition is transcribed into a written book.
const TRANSCRIPTION_PROBABILITY: f64 = 0.05;
/// Annual condition boost for written works in a library (preservation maintenance).
const LIBRARY_PRESERVATION_RATE: f64 = 0.001;

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
                SignalKind::SuccessionCrisis {
                    faction_id, ..
                } => handle_succession_crisis(ctx, time, signal.event_id, *faction_id),
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
    create_knowledge(
        ctx,
        time,
        caused_by,
        KnowledgeCategory::Dynasty,
        ALLIANCE_BETRAYAL_SIGNIFICANCE,
        settlement_id,
        truth,
    );
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
) {
    let signal_category = category;
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
            distortions: Vec::new(),
            derived_from_id: None,
            derivation_method: crate::model::DerivationMethod::Witnessed,
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
            let holder_id = e.active_rel(RelationshipKind::HeldBy);
            if let Some(hid) = holder_id
                && let Some(holder) = ctx.world.entities.get(&hid)
            {
                if let Some(pd) = holder.data.as_person()
                    && current_year > pd.birth_year
                {
                    let age = current_year - pd.birth_year;
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
                .map(|e| e.extra_f64_or(K::BUILDING_LIBRARY_BONUS, 0.0))
                .unwrap_or(0.0);
            let temple_bonus = entity
                .map(|e| e.extra_f64_or(K::BUILDING_TEMPLE_KNOWLEDGE_BONUS, 0.0))
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

        for (manif_id, knowledge_id, accuracy, significance) in &oral_manifests {
            // Trade route partners
            for &partner in &trade_partners {
                let partner_has = settlement_knowledge
                    .get(&partner)
                    .is_some_and(|s| s.contains(knowledge_id));
                if !partner_has {
                    let prob = TRADE_ROUTE_PROPAGATION_BASE * accuracy * significance;
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
                    let prob = ADJACENT_PROPAGATION_BASE * accuracy * significance;
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
            e.extra_f64(K::BUILDING_LIBRARY_BONUS)
                .is_some_and(|v| v > 0.0)
        })
        .map(|e| e.id)
        .collect();

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
            });
        }
    }

    // Apply transcriptions
    for tc in transcriptions {
        if ctx.rng.random_range(0.0..1.0) < TRANSCRIPTION_PROBABILITY {
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
        let new_condition = (p.old_condition + LIBRARY_PRESERVATION_RATE).min(1.0);
        if let Some(entity) = ctx.world.entities.get_mut(&p.manif_id)
            && let Some(md) = entity.data.as_manifestation_mut()
        {
            md.condition = new_condition;
        }
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
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: crate::model::DerivationMethod::Witnessed,
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
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: crate::model::DerivationMethod::Dreamed,
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
                origin_year: 100,
                significance: 0.9,
                ground_truth: serde_json::json!({
                    "event_type": "battle",
                    "name": "Battle of Ironhold",
                    "year": 100,
                    "attacker": {"faction_name": "Northmen", "troops": 500},
                    "defender": {"faction_name": "Southfolk", "troops": 300},
                }),
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
                created_year: 100,
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
                origin_year: 50,
                significance: 0.5,
                ground_truth: serde_json::json!({
                    "event_type": "founding",
                    "name": "Founding of Fartown",
                    "year": 50,
                    "attacker": {"faction_name": "Wanderers", "troops": 100},
                }),
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
                created_year: 50,
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
}
