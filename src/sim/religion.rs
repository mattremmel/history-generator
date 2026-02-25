use rand::Rng;

use super::context::TickContext;
use super::extra_keys as K;
use super::helpers;
use super::religion_names::{generate_deity_name, generate_religion_name};
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::cultural_value::CulturalValue;
use crate::model::entity_data::{
    BuildingType, DeityData, DeityDomain, KnowledgeCategory, KnowledgeData, ManifestationData,
    Medium, ReligionData, ReligiousTenet,
};
use crate::model::{
    DerivationMethod, EntityData, EntityKind, EventKind, ParticipantRole, RelationshipKind,
};

// --- Signal: religion share adjustments ---
const CONQUEST_RELIGION_SHARE: f64 = 0.03;
const REFUGEE_RELIGION_FRACTION_MAX: f64 = 0.15;
const TRADE_ROUTE_RELIGION_SHARE: f64 = 0.01;
const TEMPLE_CONSTRUCTED_RELIGION_BONUS: f64 = 0.02;
const FACTION_SPLIT_INHERIT_RELIGION: bool = true;

// --- Religious drift ---
const DRIFT_FACTION_RELIGION_GAIN: f64 = 0.03;
const DRIFT_MINORITY_DECAY_RATE: f64 = 0.03; // proportional: 3% of current share per year
const DRIFT_SPIRITUAL_MULTIPLIER: f64 = 1.5;
const DRIFT_PURGE_THRESHOLD: f64 = 0.005;

// --- Religion spreading ---
const SPREAD_BASE_CHANCE: f64 = 0.01;
const SPREAD_SHARE_AMOUNT: f64 = 0.03;

// --- Schisms ---
const SCHISM_TENSION_THRESHOLD: f64 = 0.3;
const SCHISM_MINORITY_SHARE_THRESHOLD: f64 = 0.15;
const SCHISM_BASE_CHANCE: f64 = 0.01;
const SCHISM_ORTHODOXY_DAMPENING: f64 = 0.4;
const SCHISM_INSTABILITY_BONUS: f64 = 0.3;
const SCHISM_FERVOR_BOOST: f64 = 0.1;

// --- Prophecies ---
const PROPHECY_BASE_CHANCE: f64 = 0.003;
const PROPHECY_PIOUS_BOOST: f64 = 0.002;
const PROPHECY_COOLDOWN_YEARS: u64 = 20;

// --- Nature worship disaster fervor spike ---
const DISASTER_FERVOR_SPIKE: f64 = 0.05;

pub struct ReligionSystem;

impl SimSystem for ReligionSystem {
    fn name(&self) -> &str {
        "religion"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        religious_drift(ctx);
        spread_religion(ctx);
        check_schisms(ctx);
        check_prophecies(ctx);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::SettlementCaptured {
                    settlement_id,
                    new_faction_id,
                    ..
                } => {
                    let conqueror_religion = ctx
                        .world
                        .entities
                        .get(new_faction_id)
                        .and_then(|f| f.data.as_faction())
                        .and_then(|fd| fd.primary_religion);
                    if let Some(religion_id) = conqueror_religion {
                        add_religion_share(
                            ctx,
                            *settlement_id,
                            religion_id,
                            CONQUEST_RELIGION_SHARE,
                        );
                    }
                }
                SignalKind::RefugeesArrived {
                    settlement_id,
                    source_settlement_id,
                    count,
                } => {
                    let source_dominant = ctx
                        .world
                        .entities
                        .get(source_settlement_id)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.dominant_religion);
                    if let Some(religion_id) = source_dominant {
                        let target_pop = ctx
                            .world
                            .entities
                            .get(settlement_id)
                            .and_then(|e| e.data.as_settlement())
                            .map(|sd| sd.population)
                            .unwrap_or(1);
                        let fraction =
                            (*count as f64 / target_pop as f64).min(REFUGEE_RELIGION_FRACTION_MAX);
                        add_religion_share(ctx, *settlement_id, religion_id, fraction);
                    }
                }
                SignalKind::TradeRouteEstablished {
                    from_settlement,
                    to_settlement,
                    ..
                } => {
                    let from_religion = ctx
                        .world
                        .entities
                        .get(from_settlement)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.dominant_religion);
                    let to_religion = ctx
                        .world
                        .entities
                        .get(to_settlement)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.dominant_religion);
                    if let Some(rid) = from_religion {
                        add_religion_share(ctx, *to_settlement, rid, TRADE_ROUTE_RELIGION_SHARE);
                    }
                    if let Some(rid) = to_religion {
                        add_religion_share(ctx, *from_settlement, rid, TRADE_ROUTE_RELIGION_SHARE);
                    }
                }
                SignalKind::FactionSplit {
                    settlement_id,
                    new_faction_id,
                    ..
                } => {
                    if FACTION_SPLIT_INHERIT_RELIGION && let Some(new_fid) = new_faction_id {
                        let dominant = ctx
                            .world
                            .entities
                            .get(settlement_id)
                            .and_then(|e| e.data.as_settlement())
                            .and_then(|sd| sd.dominant_religion);
                        if let Some(rid) = dominant
                            && let Some(fd) = ctx
                                .world
                                .entities
                                .get_mut(new_fid)
                                .and_then(|e| e.data.as_faction_mut())
                        {
                            fd.primary_religion = Some(rid);
                        }
                    }
                }
                SignalKind::BuildingConstructed {
                    settlement_id,
                    building_type: BuildingType::Temple,
                    ..
                } => {
                    let faction_religion = helpers::settlement_faction(ctx.world, *settlement_id)
                        .and_then(|fid| {
                            ctx.world
                                .entities
                                .get(&fid)
                                .and_then(|e| e.data.as_faction())
                                .and_then(|fd| fd.primary_religion)
                        });
                    if let Some(rid) = faction_religion {
                        add_religion_share(
                            ctx,
                            *settlement_id,
                            rid,
                            TEMPLE_CONSTRUCTED_RELIGION_BONUS,
                        );
                    }
                }
                SignalKind::DisasterStruck {
                    settlement_id,
                    region_id: _,
                    ..
                } => {
                    // NatureWorship religions in the settlement get a fervor spike
                    let makeup: Vec<u64> = ctx
                        .world
                        .entities
                        .get(settlement_id)
                        .and_then(|e| e.data.as_settlement())
                        .map(|sd| sd.religion_makeup.keys().copied().collect())
                        .unwrap_or_default();

                    for rid in makeup {
                        let has_nature = ctx
                            .world
                            .entities
                            .get(&rid)
                            .and_then(|e| e.data.as_religion())
                            .is_some_and(|rd| rd.tenets.contains(&ReligiousTenet::NatureWorship));
                        if has_nature
                            && let Some(rd) = ctx
                                .world
                                .entities
                                .get_mut(&rid)
                                .and_then(|e| e.data.as_religion_mut())
                        {
                            rd.fervor = (rd.fervor + DISASTER_FERVOR_SPIKE).min(1.0);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tick phase 1: Religious drift
// ---------------------------------------------------------------------------

fn religious_drift(ctx: &mut TickContext) {
    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.is_alive())
        .map(|e| e.id)
        .collect();

    for sid in settlement_ids {
        // Get faction religion
        let faction_religion = helpers::settlement_faction(ctx.world, sid).and_then(|fid| {
            ctx.world
                .entities
                .get(&fid)
                .and_then(|e| e.data.as_faction())
                .and_then(|fd| fd.primary_religion)
        });

        // Check if faction has Spiritual culture value (amplifies drift)
        let is_spiritual = helpers::settlement_faction(ctx.world, sid)
            .and_then(|fid| {
                ctx.world
                    .entities
                    .get(&fid)
                    .and_then(|e| e.data.as_faction())
                    .and_then(|fd| fd.primary_culture)
            })
            .and_then(|cid| ctx.world.entities.get(&cid))
            .and_then(|e| e.data.as_culture())
            .is_some_and(|cd| cd.values.contains(&CulturalValue::Spiritual));

        // Get temple bonus
        let temple_bonus = ctx
            .world
            .entities
            .get(&sid)
            .map(|e| e.extra_f64_or(K::BUILDING_TEMPLE_RELIGION_BONUS, 0.0))
            .unwrap_or(0.0);

        let mut makeup = ctx
            .world
            .entities
            .get(&sid)
            .and_then(|e| e.data.as_settlement())
            .map(|sd| sd.religion_makeup.clone())
            .unwrap_or_default();

        if makeup.is_empty() {
            continue;
        }

        // Faction religion gains share
        if let Some(faction_rid) = faction_religion {
            let current = makeup.get(&faction_rid).copied().unwrap_or(0.0);
            if current < 1.0 {
                let mut gain = DRIFT_FACTION_RELIGION_GAIN + temple_bonus;
                if is_spiritual {
                    gain *= DRIFT_SPIRITUAL_MULTIPLIER;
                }
                *makeup.entry(faction_rid).or_insert(0.0) += gain;
            }
        }

        // Minorities lose share
        let dominant_rid = makeup
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(k, _)| *k);

        for (rid, share) in makeup.iter_mut() {
            if Some(*rid) != dominant_rid && *share > 0.0 {
                let mut rate = DRIFT_MINORITY_DECAY_RATE;
                if is_spiritual {
                    rate *= DRIFT_SPIRITUAL_MULTIPLIER;
                }
                *share *= 1.0 - rate;
            }
        }

        // Purge religions below threshold
        makeup.retain(|_, share| *share >= DRIFT_PURGE_THRESHOLD);

        // Normalize
        let total: f64 = makeup.values().sum();
        if total > 0.0 && (total - 1.0).abs() > 0.001 {
            for share in makeup.values_mut() {
                *share /= total;
            }
        }

        // Determine new dominant
        let new_dominant = makeup
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(k, _)| *k);

        // Get old dominant for shift detection
        let old_dominant = ctx
            .world
            .entities
            .get(&sid)
            .and_then(|e| e.data.as_settlement())
            .and_then(|sd| sd.dominant_religion);

        // Compute religious tension = 1 - max_fraction
        let max_fraction = makeup.values().cloned().fold(0.0f64, f64::max);
        let tension = 1.0 - max_fraction;

        // Apply to settlement
        if let Some(sd) = ctx
            .world
            .entities
            .get_mut(&sid)
            .and_then(|e| e.data.as_settlement_mut())
        {
            sd.religion_makeup = makeup;
            sd.dominant_religion = new_dominant;
            sd.religious_tension = tension;
        }

        // Emit ReligiousShift if dominant changed
        if let (Some(old), Some(new)) = (old_dominant, new_dominant)
            && old != new
        {
            let time = ctx.world.current_time;
            let ev = ctx.world.add_event(
                EventKind::CulturalShift,
                time,
                format!("Religious shift in settlement {sid}"),
            );
            ctx.world
                .add_event_participant(ev, sid, ParticipantRole::Location);
            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::ReligiousShift {
                    settlement_id: sid,
                    old_religion: old,
                    new_religion: new,
                },
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Tick phase 2: Spread religion along trade routes
// ---------------------------------------------------------------------------

fn spread_religion(ctx: &mut TickContext) {
    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.is_alive())
        .map(|e| e.id)
        .collect();

    // Collect trade route pairs and partner's dominant religion + fervor/proselytism
    let mut spreads: Vec<(u64, u64)> = Vec::new(); // (target_settlement, religion_to_spread)

    for &sid in &settlement_ids {
        let trade_partners: Vec<u64> = ctx
            .world
            .entities
            .get(&sid)
            .map(|e| {
                e.relationships
                    .iter()
                    .filter(|r| r.kind == RelationshipKind::TradeRoute && r.is_active())
                    .map(|r| r.target_entity_id)
                    .collect()
            })
            .unwrap_or_default();

        for partner_id in trade_partners {
            let partner_dominant = ctx
                .world
                .entities
                .get(&partner_id)
                .and_then(|e| e.data.as_settlement())
                .and_then(|sd| sd.dominant_religion);

            if let Some(partner_rid) = partner_dominant {
                // Get partner's religion fervor and proselytism
                let (fervor, proselytism) = ctx
                    .world
                    .entities
                    .get(&partner_rid)
                    .and_then(|e| e.data.as_religion())
                    .map(|rd| (rd.fervor, rd.proselytism))
                    .unwrap_or((0.0, 0.0));

                let chance = SPREAD_BASE_CHANCE * fervor * proselytism;
                if ctx.rng.random_bool(chance.clamp(0.0, 1.0)) {
                    spreads.push((sid, partner_rid));
                }
            }
        }
    }

    for (target_sid, religion_id) in spreads {
        add_religion_share_direct(ctx.world, target_sid, religion_id, SPREAD_SHARE_AMOUNT);
    }
}

// ---------------------------------------------------------------------------
// Tick phase 3: Check for schisms
// ---------------------------------------------------------------------------

fn check_schisms(ctx: &mut TickContext) {
    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.is_alive())
        .map(|e| e.id)
        .collect();

    for sid in settlement_ids {
        let (tension, makeup) = {
            let sd = match ctx
                .world
                .entities
                .get(&sid)
                .and_then(|e| e.data.as_settlement())
            {
                Some(sd) => sd,
                None => continue,
            };
            (sd.religious_tension, sd.religion_makeup.clone())
        };

        if tension <= SCHISM_TENSION_THRESHOLD {
            continue;
        }

        // Need 2+ religions each with >15% share
        let qualifying: Vec<u64> = makeup
            .iter()
            .filter(|(_, share)| **share >= SCHISM_MINORITY_SHARE_THRESHOLD)
            .map(|(k, _)| *k)
            .collect();

        if qualifying.len() < 2 {
            continue;
        }

        // Pick the dominant religion to schism from
        let dominant_rid = match makeup.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()) {
            Some((k, _)) => *k,
            None => continue,
        };

        let orthodoxy = ctx
            .world
            .entities
            .get(&dominant_rid)
            .and_then(|e| e.data.as_religion())
            .map(|rd| rd.orthodoxy)
            .unwrap_or(0.5);

        // Get faction stability bonus
        let stability_bonus = helpers::settlement_faction(ctx.world, sid)
            .and_then(|fid| {
                ctx.world
                    .entities
                    .get(&fid)
                    .and_then(|e| e.data.as_faction())
                    .map(|fd| {
                        if fd.stability < 0.3 {
                            SCHISM_INSTABILITY_BONUS
                        } else {
                            0.0
                        }
                    })
            })
            .unwrap_or(0.0);

        let chance =
            (SCHISM_BASE_CHANCE * tension * (1.0 - orthodoxy * SCHISM_ORTHODOXY_DAMPENING))
                + stability_bonus * SCHISM_BASE_CHANCE;

        if !ctx.rng.random_bool(chance.clamp(0.0, 1.0)) {
            continue;
        }

        // Create new religion (schism offspring)
        let parent_data = ctx
            .world
            .entities
            .get(&dominant_rid)
            .and_then(|e| e.data.as_religion())
            .cloned();

        let Some(parent) = parent_data else {
            continue;
        };

        // Mutate tenets: keep existing, maybe swap one
        let mut new_tenets = parent.tenets.clone();
        if !new_tenets.is_empty() && ctx.rng.random_bool(0.3) {
            let all_tenets = [
                ReligiousTenet::WarGod,
                ReligiousTenet::NatureWorship,
                ReligiousTenet::AncestorCult,
                ReligiousTenet::Prophecy,
                ReligiousTenet::Asceticism,
                ReligiousTenet::Commerce,
                ReligiousTenet::Knowledge,
                ReligiousTenet::Death,
            ];
            let idx = ctx.rng.random_range(0..new_tenets.len());
            let replacement = all_tenets[ctx.rng.random_range(0..all_tenets.len())];
            if !new_tenets.contains(&replacement) {
                new_tenets[idx] = replacement;
            }
        }

        let new_name = generate_religion_name(ctx.rng);
        let time = ctx.world.current_time;
        let ev = ctx.world.add_event(
            EventKind::Schism,
            time,
            format!("Religious schism: {new_name} splits from the faith in settlement {sid}"),
        );
        ctx.world
            .add_event_participant(ev, sid, ParticipantRole::Location);
        ctx.world
            .add_event_participant(ev, dominant_rid, ParticipantRole::Origin);

        let new_religion_id = ctx.world.add_entity(
            EntityKind::Religion,
            new_name.clone(),
            Some(time),
            EntityData::Religion(ReligionData {
                fervor: (parent.fervor + SCHISM_FERVOR_BOOST).min(1.0),
                proselytism: parent.proselytism,
                orthodoxy: parent.orthodoxy * 0.8,
                tenets: new_tenets.clone(),
            }),
            ev,
        );

        // Create a deity for the new religion
        let deity_name = generate_deity_name(ctx.rng);
        let domain = pick_schism_domain(ctx, &new_tenets);
        let deity_id = ctx.world.add_entity(
            EntityKind::Deity,
            deity_name,
            Some(time),
            EntityData::Deity(DeityData {
                domain,
                worship_strength: 0.5,
            }),
            ev,
        );
        ctx.world.add_relationship(
            deity_id,
            new_religion_id,
            RelationshipKind::MemberOf,
            time,
            ev,
        );

        // Transfer some of the parent's share to the new religion
        let transfer = makeup.get(&dominant_rid).copied().unwrap_or(0.0) * 0.3;
        if let Some(sd) = ctx
            .world
            .entities
            .get_mut(&sid)
            .and_then(|e| e.data.as_settlement_mut())
        {
            if let Some(parent_share) = sd.religion_makeup.get_mut(&dominant_rid) {
                *parent_share -= transfer;
            }
            sd.religion_makeup.insert(new_religion_id, transfer);
        }

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::ReligionSchism {
                parent_religion_id: dominant_rid,
                new_religion_id,
                settlement_id: sid,
            },
        });
    }
}

fn pick_schism_domain(ctx: &mut TickContext, tenets: &[ReligiousTenet]) -> DeityDomain {
    let domains = [
        DeityDomain::Sky,
        DeityDomain::Earth,
        DeityDomain::Sea,
        DeityDomain::War,
        DeityDomain::Death,
        DeityDomain::Harvest,
        DeityDomain::Craft,
        DeityDomain::Wisdom,
        DeityDomain::Storm,
        DeityDomain::Fire,
    ];
    // Simple weighted pick based on tenets
    if tenets.contains(&ReligiousTenet::WarGod) && ctx.rng.random_bool(0.4) {
        return DeityDomain::War;
    }
    if tenets.contains(&ReligiousTenet::NatureWorship) && ctx.rng.random_bool(0.4) {
        return DeityDomain::Earth;
    }
    if tenets.contains(&ReligiousTenet::Death) && ctx.rng.random_bool(0.4) {
        return DeityDomain::Death;
    }
    domains[ctx.rng.random_range(0..domains.len())]
}

// ---------------------------------------------------------------------------
// Tick phase 4: Check for prophecies
// ---------------------------------------------------------------------------

fn check_prophecies(ctx: &mut TickContext) {
    let settlement_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.is_alive())
        .map(|e| e.id)
        .collect();

    let current_year = ctx.world.current_time.year() as u64;

    for sid in settlement_ids {
        // Check if any religion in this settlement has Prophecy tenet
        let religions_with_prophecy: Vec<u64> = {
            let makeup = ctx
                .world
                .entities
                .get(&sid)
                .and_then(|e| e.data.as_settlement())
                .map(|sd| sd.religion_makeup.keys().copied().collect::<Vec<_>>())
                .unwrap_or_default();

            makeup
                .into_iter()
                .filter(|rid| {
                    ctx.world
                        .entities
                        .get(rid)
                        .and_then(|e| e.data.as_religion())
                        .is_some_and(|rd| rd.tenets.contains(&ReligiousTenet::Prophecy))
                })
                .collect()
        };

        if religions_with_prophecy.is_empty() {
            continue;
        }

        // Check cooldown
        let last_prophecy = ctx
            .world
            .entities
            .get(&sid)
            .map(|e| e.extra_u64_or(K::PROPHECY_COOLDOWN, 0))
            .unwrap_or(0);

        if current_year < last_prophecy + PROPHECY_COOLDOWN_YEARS {
            continue;
        }

        // Count pious NPCs in the settlement for chance boost
        let pious_count = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Person
                    && e.is_alive()
                    && e.has_active_rel(RelationshipKind::LocatedIn, sid)
                    && e.data
                        .as_person()
                        .is_some_and(|p| p.traits.contains(&crate::model::traits::Trait::Pious))
            })
            .count();

        let chance = PROPHECY_BASE_CHANCE + (pious_count as f64 * PROPHECY_PIOUS_BOOST);

        if !ctx.rng.random_bool(chance.clamp(0.0, 1.0)) {
            continue;
        }

        // Pick which Prophecy religion this relates to
        let religion_id =
            religions_with_prophecy[ctx.rng.random_range(0..religions_with_prophecy.len())];

        let religion_name = ctx
            .world
            .entities
            .get(&religion_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();

        // Find a possible prophet (pious person in settlement)
        let prophet_id: Option<u64> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Person
                    && e.is_alive()
                    && e.has_active_rel(RelationshipKind::LocatedIn, sid)
                    && e.data
                        .as_person()
                        .is_some_and(|p| p.traits.contains(&crate::model::traits::Trait::Pious))
            })
            .map(|e| e.id)
            .next();

        let time = ctx.world.current_time;

        // Create prophecy event
        let ev = ctx.world.add_event(
            EventKind::Ceremony,
            time,
            format!("A prophecy is declared in the name of {religion_name}"),
        );
        ctx.world
            .add_event_participant(ev, sid, ParticipantRole::Location);
        ctx.world
            .add_event_participant(ev, religion_id, ParticipantRole::Subject);
        if let Some(pid) = prophet_id {
            ctx.world
                .add_event_participant(ev, pid, ParticipantRole::Instigator);
        }

        // Create Knowledge entity (Religious category)
        let knowledge_id = ctx.world.add_entity(
            EntityKind::Knowledge,
            format!("Prophecy of {religion_name}"),
            Some(time),
            EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Religious,
                source_event_id: ev,
                origin_settlement_id: sid,
                origin_year: time.year(),
                significance: 0.4,
                ground_truth: serde_json::json!({
                    "type": "prophecy",
                    "religion": religion_name,
                    "year": time.year(),
                }),
            }),
            ev,
        );

        // Create OralTradition manifestation held by settlement
        let manifestation_id = ctx.world.add_entity(
            EntityKind::Manifestation,
            format!("Oral tradition: Prophecy of {religion_name}"),
            Some(time),
            EntityData::Manifestation(ManifestationData {
                knowledge_id,
                medium: Medium::OralTradition,
                content: serde_json::json!({
                    "type": "prophecy",
                    "religion": religion_name,
                    "year": time.year(),
                }),
                accuracy: 1.0,
                completeness: 1.0,
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: DerivationMethod::Witnessed,
                condition: 1.0,
                created_year: time.year(),
            }),
            ev,
        );
        ctx.world
            .add_relationship(manifestation_id, sid, RelationshipKind::HeldBy, time, ev);

        // Set cooldown
        ctx.world.set_extra(
            sid,
            K::PROPHECY_COOLDOWN,
            serde_json::json!(current_year),
            ev,
        );

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::ProphecyDeclared {
                knowledge_id,
                settlement_id: sid,
                prophet_id,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Add a religion share to a settlement, normalizing the makeup afterward.
/// Uses the TickContext for event recording.
fn add_religion_share(ctx: &mut TickContext, settlement_id: u64, religion_id: u64, amount: f64) {
    add_religion_share_direct(ctx.world, settlement_id, religion_id, amount);
}

/// Add a religion share directly to a settlement (no event recording needed).
fn add_religion_share_direct(
    world: &mut crate::model::World,
    settlement_id: u64,
    religion_id: u64,
    amount: f64,
) {
    if let Some(sd) = world
        .entities
        .get_mut(&settlement_id)
        .and_then(|e| e.data.as_settlement_mut())
    {
        *sd.religion_makeup.entry(religion_id).or_insert(0.0) += amount;

        // Normalize
        let total: f64 = sd.religion_makeup.values().sum();
        if total > 0.0 && (total - 1.0).abs() > 0.001 {
            for share in sd.religion_makeup.values_mut() {
                *share /= total;
            }
        }

        // Update dominant
        sd.dominant_religion = sd
            .religion_makeup
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(k, _)| *k);

        // Update tension
        let max_fraction = sd.religion_makeup.values().cloned().fold(0.0f64, f64::max);
        sd.religious_tension = 1.0 - max_fraction;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entity_data::Role;
    use crate::model::traits::Trait;
    use crate::scenario::Scenario;
    use crate::testutil;
    use std::collections::BTreeMap;

    fn religion_system() -> Vec<Box<dyn SimSystem>> {
        vec![Box::new(ReligionSystem)]
    }

    #[test]
    fn scenario_drift_changes_religion_makeup_over_time() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.add_faction("Kingdom");
        let settlement = s.settlement("Town", faction, region).population(300).id();

        // Create two religions
        let religion_a = s.add_religion_with("Faith A", |rd| {
            rd.fervor = 0.5;
            rd.proselytism = 0.3;
        });
        let religion_b = s.add_religion_with("Faith B", |rd| {
            rd.fervor = 0.5;
            rd.proselytism = 0.3;
        });

        // Set faction's primary religion to A
        s.modify_faction(faction, |fd| {
            fd.primary_religion = Some(religion_a);
        });

        // Settlement starts with mixed makeup
        s.modify_settlement(settlement, |sd| {
            sd.dominant_religion = Some(religion_a);
            sd.religion_makeup = BTreeMap::from([(religion_a, 0.6), (religion_b, 0.4)]);
        });

        let world = s.run(&mut religion_system(), 20, 42);

        let sd = world.settlement(settlement);
        // Faction religion A should have gained share over time
        let share_a = sd.religion_makeup.get(&religion_a).copied().unwrap_or(0.0);
        assert!(
            share_a > 0.6,
            "faction religion should gain share, got {share_a}"
        );
    }

    #[test]
    fn scenario_purges_minority_religion() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.add_faction("Kingdom");
        let settlement = s.settlement("Town", faction, region).population(300).id();

        let religion_a = s.add_religion("Faith A");
        let religion_b = s.add_religion("Faith B");

        s.modify_faction(faction, |fd| {
            fd.primary_religion = Some(religion_a);
        });

        // Religion B starts at a very small fraction
        s.modify_settlement(settlement, |sd| {
            sd.dominant_religion = Some(religion_a);
            sd.religion_makeup = BTreeMap::from([(religion_a, 0.95), (religion_b, 0.05)]);
        });

        let world = s.run(&mut religion_system(), 100, 42);

        let sd = world.settlement(settlement);
        // Religion B should be purged (below threshold)
        assert!(
            !sd.religion_makeup.contains_key(&religion_b),
            "tiny minority should be purged"
        );
    }

    #[test]
    fn scenario_trade_spreads_religion() {
        let mut s = Scenario::at_year(100);
        let region_a = s.add_region("Region A");
        let region_b = s.add_region("Region B");
        s.make_adjacent(region_a, region_b);

        let faction_a = s.add_faction("Kingdom A");
        let faction_b = s.add_faction("Kingdom B");

        let settlement_a = s
            .settlement("Town A", faction_a, region_a)
            .population(300)
            .id();
        let settlement_b = s
            .settlement("Town B", faction_b, region_b)
            .population(300)
            .id();

        let religion_a = s.add_religion_with("Faith A", |rd| {
            rd.fervor = 1.0;
            rd.proselytism = 1.0;
        });
        let religion_b = s.add_religion_with("Faith B", |rd| {
            rd.fervor = 1.0;
            rd.proselytism = 1.0;
        });

        s.modify_faction(faction_a, |fd| {
            fd.primary_religion = Some(religion_a);
        });
        s.modify_faction(faction_b, |fd| {
            fd.primary_religion = Some(religion_b);
        });

        s.modify_settlement(settlement_a, |sd| {
            sd.dominant_religion = Some(religion_a);
            sd.religion_makeup = BTreeMap::from([(religion_a, 1.0)]);
        });
        s.modify_settlement(settlement_b, |sd| {
            sd.dominant_religion = Some(religion_b);
            sd.religion_makeup = BTreeMap::from([(religion_b, 1.0)]);
        });

        // Establish a trade route
        s.make_trade_route(settlement_a, settlement_b);

        let world = s.run(&mut religion_system(), 300, 42);

        // Check both directions — at least one should have spread
        let share_a_in_b = world
            .settlement(settlement_b)
            .religion_makeup
            .get(&religion_a)
            .copied()
            .unwrap_or(0.0);
        let share_b_in_a = world
            .settlement(settlement_a)
            .religion_makeup
            .get(&religion_b)
            .copied()
            .unwrap_or(0.0);
        assert!(
            share_a_in_b > 0.0 || share_b_in_a > 0.0,
            "religion should spread via trade routes, got A-in-B={share_a_in_b}, B-in-A={share_b_in_a}"
        );
    }

    #[test]
    fn scenario_prophecy_creates_knowledge() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.add_faction("Kingdom");
        let settlement = s.settlement("Town", faction, region).population(300).id();

        let religion = s.add_religion_with("Prophetic Faith", |rd| {
            rd.tenets = vec![ReligiousTenet::Prophecy];
        });

        s.modify_faction(faction, |fd| {
            fd.primary_religion = Some(religion);
        });

        s.modify_settlement(settlement, |sd| {
            sd.dominant_religion = Some(religion);
            sd.religion_makeup = BTreeMap::from([(religion, 1.0)]);
        });

        // Add pious people to boost chance
        for i in 0..5 {
            s.add_person_in_with(&format!("Priest_{i}"), faction, settlement, |pd| {
                pd.role = Role::Priest;
                pd.traits = vec![Trait::Pious];
            });
        }

        // Run long enough to trigger prophecy (probabilistic)
        let world = s.run(&mut religion_system(), 200, 42);

        // Check that at least one Religious knowledge was created
        let religious_knowledge: Vec<_> = world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Knowledge
                    && e.data
                        .as_knowledge()
                        .is_some_and(|kd| kd.category == KnowledgeCategory::Religious)
            })
            .collect();

        assert!(
            !religious_knowledge.is_empty(),
            "prophecy should create Religious knowledge entities"
        );
    }

    #[test]
    fn scenario_schism_fires_under_conditions() {
        let mut s = Scenario::at_year(100);

        // Create many settlements with high tension for more independent chances
        let mut initial_religions = 0;
        for i in 0..20 {
            let region = s.add_region(&format!("Region {i}"));
            let faction = s.add_faction_with(&format!("Kingdom {i}"), |fd| {
                fd.stability = 0.2; // Low stability = instability bonus
            });
            let settlement = s
                .settlement(&format!("Town {i}"), faction, region)
                .population(500)
                .id();

            let religion_a = s.add_religion_with(&format!("Faith A{i}"), |rd| {
                rd.orthodoxy = 0.0; // Minimum orthodoxy = easiest schism
            });
            let religion_b = s.add_religion(&format!("Faith B{i}"));
            initial_religions += 2;

            // Don't set faction primary religion — drift won't bias either side
            s.modify_settlement(settlement, |sd| {
                sd.dominant_religion = Some(religion_a);
                sd.religion_makeup = BTreeMap::from([(religion_a, 0.55), (religion_b, 0.45)]);
                sd.religious_tension = 0.45;
            });
        }

        let world = s.run(&mut religion_system(), 500, 42);

        let religion_count = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Religion)
            .count();

        // With 20 settlements, 500 years, and maximally favorable conditions,
        // at least one schism should have occurred
        assert!(
            religion_count > initial_religions,
            "schism should create new religions, got {religion_count} (started with {initial_religions})"
        );

        let schism_events = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Schism)
            .count();
        assert!(schism_events > 0, "should have at least one Schism event");
    }

    // -----------------------------------------------------------------------
    // Signal handler tests (deliver_signals, zero ticks)
    // -----------------------------------------------------------------------

    #[test]
    fn scenario_conquest_adds_conqueror_religion() {
        let mut s = Scenario::at_year(100);
        let religion_a = s.add_religion("FaithA");
        let religion_b = s.add_religion("FaithB");
        let r = s.add_region("R");
        let old_f = s.add_faction("OldFaction");
        let new_f = s.add_faction("Conquerors");
        s.modify_faction(old_f, |fd| fd.primary_religion = Some(religion_a));
        s.modify_faction(new_f, |fd| fd.primary_religion = Some(religion_b));

        let mut makeup = BTreeMap::new();
        makeup.insert(religion_a, 1.0);
        let sett = s
            .settlement("Town", old_f, r)
            .population(300)
            .dominant_religion(Some(religion_a))
            .religion_makeup(makeup)
            .id();
        s.settlement("S2", new_f, r).population(200).id();
        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::SettlementCaptured {
                settlement_id: sett,
                old_faction_id: old_f,
                new_faction_id: new_f,
            },
        }];
        testutil::deliver_signals(&mut world, &mut ReligionSystem, &inbox, 42);

        let sd = world.settlement(sett);
        assert!(
            sd.religion_makeup.contains_key(&religion_b),
            "conquered settlement should gain conqueror's religion"
        );
    }

    #[test]
    fn scenario_refugees_bring_religion() {
        let mut s = Scenario::at_year(100);
        let religion_src = s.add_religion("SourceFaith");
        let religion_dst = s.add_religion("DestFaith");
        let r = s.add_region("R");
        let f = s.add_faction("F");

        let mut src_makeup = BTreeMap::new();
        src_makeup.insert(religion_src, 1.0);
        let source = s
            .settlement("Source", f, r)
            .population(500)
            .dominant_religion(Some(religion_src))
            .religion_makeup(src_makeup)
            .id();

        let mut dst_makeup = BTreeMap::new();
        dst_makeup.insert(religion_dst, 1.0);
        let dest = s
            .settlement("Dest", f, r)
            .population(500)
            .dominant_religion(Some(religion_dst))
            .religion_makeup(dst_makeup)
            .id();

        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::RefugeesArrived {
                settlement_id: dest,
                source_settlement_id: source,
                count: 50,
            },
        }];
        testutil::deliver_signals(&mut world, &mut ReligionSystem, &inbox, 42);

        let sd = world.settlement(dest);
        assert!(
            sd.religion_makeup.contains_key(&religion_src),
            "destination should gain source religion after refugees arrive"
        );
    }

    #[test]
    fn scenario_signal_trade_spreads_religion() {
        let mut s = Scenario::at_year(100);
        let religion_a = s.add_religion("FaithA");
        let religion_b = s.add_religion("FaithB");
        let r = s.add_region("R");
        let fa = s.add_faction("FA");
        let fb = s.add_faction("FB");

        let mut makeup_a = BTreeMap::new();
        makeup_a.insert(religion_a, 1.0);
        let sa = s
            .settlement("SA", fa, r)
            .population(300)
            .dominant_religion(Some(religion_a))
            .religion_makeup(makeup_a)
            .id();

        let mut makeup_b = BTreeMap::new();
        makeup_b.insert(religion_b, 1.0);
        let sb = s
            .settlement("SB", fb, r)
            .population(300)
            .dominant_religion(Some(religion_b))
            .religion_makeup(makeup_b)
            .id();

        let mut world = s.build();

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::TradeRouteEstablished {
                from_settlement: sa,
                to_settlement: sb,
                from_faction: fa,
                to_faction: fb,
            },
        }];
        testutil::deliver_signals(&mut world, &mut ReligionSystem, &inbox, 42);

        let sd_a = world.settlement(sa);
        let sd_b = world.settlement(sb);
        assert!(
            sd_a.religion_makeup.contains_key(&religion_b),
            "settlement A should gain religion B from trade"
        );
        assert!(
            sd_b.religion_makeup.contains_key(&religion_a),
            "settlement B should gain religion A from trade"
        );
    }

    #[test]
    fn scenario_faction_split_inherits_religion() {
        let mut s = Scenario::at_year(100);
        let religion = s.add_religion("SplitFaith");
        let r = s.add_region("R");
        let old_f = s.add_faction("OldFaction");
        let new_f = s.add_faction("NewFaction");

        let mut makeup = BTreeMap::new();
        makeup.insert(religion, 1.0);
        let sett = s
            .settlement("Town", old_f, r)
            .population(300)
            .dominant_religion(Some(religion))
            .religion_makeup(makeup)
            .id();

        let mut world = s.build();

        assert!(world.faction(new_f).primary_religion.is_none());

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::FactionSplit {
                old_faction_id: old_f,
                new_faction_id: Some(new_f),
                settlement_id: sett,
            },
        }];
        testutil::deliver_signals(&mut world, &mut ReligionSystem, &inbox, 42);

        assert_eq!(
            world.faction(new_f).primary_religion,
            Some(religion),
            "new faction should inherit settlement's dominant religion"
        );
    }

    #[test]
    fn scenario_temple_boosts_dominant_religion() {
        let mut s = Scenario::at_year(100);
        let religion = s.add_religion("TempleFaith");
        let r = s.add_region("R");
        let f = s.add_faction("Kingdom");
        s.modify_faction(f, |fd| fd.primary_religion = Some(religion));

        let mut makeup = BTreeMap::new();
        makeup.insert(religion, 0.7);
        let sett = s
            .settlement("Town", f, r)
            .population(300)
            .dominant_religion(Some(religion))
            .religion_makeup(makeup)
            .id();
        let building = s.add_building(crate::model::entity_data::BuildingType::Temple, sett);

        let mut world = s.build();

        let before = *world.settlement(sett).religion_makeup.get(&religion).unwrap_or(&0.0);

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::BuildingConstructed {
                building_id: building,
                settlement_id: sett,
                building_type: crate::model::entity_data::BuildingType::Temple,
            },
        }];
        testutil::deliver_signals(&mut world, &mut ReligionSystem, &inbox, 42);

        let after = *world.settlement(sett).religion_makeup.get(&religion).unwrap_or(&0.0);
        assert!(
            after > before,
            "temple construction should boost dominant religion share: {before} -> {after}"
        );
    }
}
