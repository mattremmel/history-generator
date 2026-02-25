use std::collections::BTreeMap;

use rand::Rng;

use super::context::TickContext;
use super::culture_names::generate_culture_entity_name;
use super::extra_keys as K;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::cultural_value::NamingStyle;
use crate::model::entity_data::CultureData;
use crate::model::{EntityData, EntityKind, EventKind, ParticipantRole, RelationshipKind};

// --- Signal: culture share adjustments ---
const CONQUEST_CULTURE_SHARE: f64 = 0.05;
const REFUGEE_CULTURE_FRACTION_MAX: f64 = 0.20;
const REFUGEE_CULTURE_FRACTION_DEFAULT: f64 = 0.05;
const TRADE_ROUTE_CULTURE_SHARE: f64 = 0.01;

// --- Cultural drift ---
const DRIFT_BASE_MINORITY_LOSS: f64 = 0.02;
const DRIFT_TRADE_BONUS_MULTIPLIER: f64 = 0.005;
const DRIFT_PROSPERITY_THRESHOLD: f64 = 0.6;
const DRIFT_PROSPERITY_BONUS: f64 = 0.005;
const DRIFT_PURGE_THRESHOLD: f64 = 0.03;
const DRIFT_NORMALIZE_TOLERANCE: f64 = 0.001;
const DOMINANT_CULTURE_MIN_FRACTION: f64 = 0.5;

// --- Cultural blending ---
const BLEND_QUALIFYING_SHARE: f64 = 0.30;
const BLEND_TIMER_THRESHOLD: u64 = 50;
const BLEND_CHANCE_PER_YEAR: f64 = 0.05;

// --- Cultural rebellion ---
const REBELLION_TENSION_THRESHOLD: f64 = 0.35;
const REBELLION_STABILITY_THRESHOLD: f64 = 0.5;
const REBELLION_BASE_CHANCE: f64 = 0.03;
const REBELLION_BASE_SUCCESS_CHANCE: f64 = 0.40;
const REBELLION_HIGH_TENSION_THRESHOLD: f64 = 0.6;
const REBELLION_HIGH_TENSION_BONUS: f64 = 0.20;
const REBELLION_LOW_STABILITY_THRESHOLD: f64 = 0.3;
const REBELLION_LOW_STABILITY_BONUS: f64 = 0.10;
const REBELLION_FAILED_STABILITY_PENALTY: f64 = 0.10;
const REBELLION_CRACKDOWN_CULTURE_SHARE: f64 = 0.10;

pub struct CultureSystem;

impl SimSystem for CultureSystem {
    fn name(&self) -> &str {
        "culture"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        cultural_drift(ctx);
        cultural_blending(ctx);
        rebellion_check(ctx);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::SettlementCaptured {
                    settlement_id,
                    new_faction_id,
                    ..
                } => {
                    // Add conquering faction's culture
                    let conqueror_culture = ctx
                        .world
                        .entities
                        .get(new_faction_id)
                        .and_then(|f| f.data.as_faction())
                        .and_then(|fd| fd.primary_culture);
                    if let Some(culture_id) = conqueror_culture {
                        add_culture_share(ctx, *settlement_id, culture_id, CONQUEST_CULTURE_SHARE);
                    }
                }
                SignalKind::RefugeesArrived {
                    settlement_id,
                    source_settlement_id,
                    count,
                    ..
                } => {
                    // Add source settlement's dominant culture proportional to refugee fraction
                    let source_culture = ctx
                        .world
                        .entities
                        .get(source_settlement_id)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.dominant_culture);
                    let dest_pop = ctx
                        .world
                        .entities
                        .get(settlement_id)
                        .and_then(|e| e.data.as_settlement())
                        .map(|sd| sd.population)
                        .unwrap_or(0);
                    if let Some(culture_id) = source_culture {
                        let fraction = if dest_pop > 0 {
                            (*count as f64 / dest_pop as f64).min(REFUGEE_CULTURE_FRACTION_MAX)
                        } else {
                            REFUGEE_CULTURE_FRACTION_DEFAULT
                        };
                        add_culture_share(ctx, *settlement_id, culture_id, fraction);
                    }
                }
                SignalKind::TradeRouteEstablished {
                    from_settlement,
                    to_settlement,
                    ..
                } => {
                    // Add partner's dominant culture in each settlement
                    let from_culture = ctx
                        .world
                        .entities
                        .get(from_settlement)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.dominant_culture);
                    let to_culture = ctx
                        .world
                        .entities
                        .get(to_settlement)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.dominant_culture);
                    if let Some(c) = to_culture {
                        add_culture_share(ctx, *from_settlement, c, TRADE_ROUTE_CULTURE_SHARE);
                    }
                    if let Some(c) = from_culture {
                        add_culture_share(ctx, *to_settlement, c, TRADE_ROUTE_CULTURE_SHARE);
                    }
                }
                SignalKind::FactionSplit {
                    new_faction_id: Some(new_faction_id),
                    settlement_id,
                    ..
                } => {
                    // New faction inherits splitting settlement's dominant culture
                    let culture = ctx
                        .world
                        .entities
                        .get(settlement_id)
                        .and_then(|e| e.data.as_settlement())
                        .and_then(|sd| sd.dominant_culture);
                    if let Some(culture_id) = culture
                        && let Some(faction) = ctx.world.entities.get_mut(new_faction_id)
                        && let Some(fd) = faction.data.as_faction_mut()
                    {
                        fd.primary_culture = Some(culture_id);
                    }
                }
                _ => {}
            }
        }
    }
}

// --- Phase A: Cultural Drift ---

fn cultural_drift(ctx: &mut TickContext) {
    let time = ctx.world.current_time;

    struct SettlementInfo {
        id: u64,
        faction_id: Option<u64>,
        makeup: BTreeMap<u64, f64>,
        prosperity: f64,
    }

    let settlements: Vec<SettlementInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            if sd.culture_makeup.is_empty() {
                return None;
            }
            let faction_id = e.active_rel(RelationshipKind::MemberOf);
            Some(SettlementInfo {
                id: e.id,
                faction_id,
                makeup: sd.culture_makeup.clone(),
                prosperity: sd.prosperity,
            })
        })
        .collect();

    struct DriftUpdate {
        settlement_id: u64,
        new_makeup: BTreeMap<u64, f64>,
        new_dominant: Option<u64>,
        old_dominant: Option<u64>,
    }

    let mut updates: Vec<DriftUpdate> = Vec::new();

    for s in &settlements {
        let ruling_culture = s.faction_id.and_then(|fid| {
            ctx.world
                .entities
                .get(&fid)
                .and_then(|f| f.data.as_faction())
                .and_then(|fd| fd.primary_culture)
        });

        let mut new_makeup = s.makeup.clone();

        if let Some(ruling_id) = ruling_culture {
            // Count trade routes to settlements of ruling culture
            let trade_bonus = count_ruling_culture_trade_routes(ctx, s.id, ruling_id);

            // Drift: ruling culture gains, minorities lose
            let minority_ids: Vec<u64> = new_makeup
                .keys()
                .filter(|&&c| c != ruling_id)
                .copied()
                .collect();

            let mut total_gained = 0.0;
            for mid in &minority_ids {
                let resistance = ctx
                    .world
                    .entities
                    .get(mid)
                    .and_then(|e| e.data.as_culture())
                    .map(|cd| cd.resistance)
                    .unwrap_or(0.5);

                let mut loss = DRIFT_BASE_MINORITY_LOSS * (1.0 - resistance);
                loss += trade_bonus * DRIFT_TRADE_BONUS_MULTIPLIER;
                if s.prosperity > DRIFT_PROSPERITY_THRESHOLD {
                    loss += DRIFT_PROSPERITY_BONUS;
                }

                let current = *new_makeup.get(mid).unwrap_or(&0.0);
                let actual_loss = loss.min(current);
                if actual_loss > 0.0 {
                    *new_makeup.get_mut(mid).unwrap() -= actual_loss;
                    total_gained += actual_loss;
                }
            }

            // Ruling culture gains what minorities lost
            *new_makeup.entry(ruling_id).or_insert(0.0) += total_gained;
        }

        // Normalize
        let total: f64 = new_makeup.values().sum();
        if total > 0.0 {
            for v in new_makeup.values_mut() {
                *v /= total;
            }
        }

        // Purge cultures below threshold
        new_makeup.retain(|_, v| *v >= DRIFT_PURGE_THRESHOLD);

        // Re-normalize after purge
        let total: f64 = new_makeup.values().sum();
        if total > 0.0 && (total - 1.0).abs() > DRIFT_NORMALIZE_TOLERANCE {
            for v in new_makeup.values_mut() {
                *v /= total;
            }
        }

        // Find dominant culture
        let old_dominant = ctx
            .world
            .entities
            .get(&s.id)
            .and_then(|e| e.data.as_settlement())
            .and_then(|sd| sd.dominant_culture);

        let new_dominant = new_makeup
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .filter(|(_, v)| **v > DOMINANT_CULTURE_MIN_FRACTION)
            .map(|(&k, _)| k);

        updates.push(DriftUpdate {
            settlement_id: s.id,
            new_makeup,
            new_dominant,
            old_dominant,
        });
    }

    // Apply updates
    for update in updates {
        // Check if dominant culture changed
        if update.new_dominant != update.old_dominant
            && let (Some(new_c), Some(old_c)) = (update.new_dominant, update.old_dominant)
        {
            let settlement_name = ctx
                .world
                .entities
                .get(&update.settlement_id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let new_culture_name = ctx
                .world
                .entities
                .get(&new_c)
                .map(|e| e.name.clone())
                .unwrap_or_default();

            let ev = ctx.world.add_event(
                EventKind::CulturalShift,
                time,
                format!("Cultural shift in {settlement_name}: {new_culture_name} became dominant"),
            );
            ctx.world
                .add_event_participant(ev, update.settlement_id, ParticipantRole::Location);

            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::CulturalShift {
                    settlement_id: update.settlement_id,
                    old_culture: old_c,
                    new_culture: new_c,
                },
            });
        }

        // Compute cultural tension = 1.0 - dominant_fraction
        let dominant_fraction = update
            .new_dominant
            .and_then(|c| update.new_makeup.get(&c))
            .copied()
            .unwrap_or(0.0);
        let tension = 1.0 - dominant_fraction;

        if let Some(settlement) = ctx.world.entities.get_mut(&update.settlement_id)
            && let Some(sd) = settlement.data.as_settlement_mut()
        {
            sd.culture_makeup = update.new_makeup;
            sd.dominant_culture = update.new_dominant;
            sd.cultural_tension = tension;
        }
    }
}

fn count_ruling_culture_trade_routes(
    ctx: &TickContext,
    settlement_id: u64,
    ruling_culture: u64,
) -> f64 {
    let settlement = match ctx.world.entities.get(&settlement_id) {
        Some(e) => e,
        None => return 0.0,
    };

    let trade_partner_ids: Vec<u64> = settlement
        .extra
        .get(K::TRADE_ROUTES)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("target").and_then(|t| t.as_u64()))
                .collect()
        })
        .unwrap_or_default();

    let mut count = 0.0;
    for &partner_id in &trade_partner_ids {
        let partner_culture = ctx
            .world
            .entities
            .get(&partner_id)
            .and_then(|e| e.data.as_settlement())
            .and_then(|sd| sd.dominant_culture);
        if partner_culture == Some(ruling_culture) {
            count += 1.0;
        }
    }
    count
}

// --- Phase B: Cultural Blending ---

fn cultural_blending(ctx: &mut TickContext) {
    let time = ctx.world.current_time;

    struct BlendCandidate {
        settlement_id: u64,
        parent_cultures: Vec<(u64, f64)>,
    }

    let mut candidates: Vec<BlendCandidate> = Vec::new();

    for entity in ctx.world.entities.values() {
        if entity.kind != EntityKind::Settlement || entity.end.is_some() {
            continue;
        }
        let sd = match entity.data.as_settlement() {
            Some(sd) => sd,
            None => continue,
        };

        // Check: 2+ cultures each above blend qualifying share
        let qualifying: Vec<(u64, f64)> = sd
            .culture_makeup
            .iter()
            .filter(|&(_, v)| *v >= BLEND_QUALIFYING_SHARE)
            .map(|(&k, &v)| (k, v))
            .collect();

        if qualifying.len() < 2 {
            // Reset blend timer if conditions not met
            continue;
        }

        candidates.push(BlendCandidate {
            settlement_id: entity.id,
            parent_cultures: qualifying,
        });
    }

    for candidate in candidates {
        // Track blend timer via extra
        let timer = ctx
            .world
            .entities
            .get(&candidate.settlement_id)
            .map(|e| e.extra_u64_or(K::BLEND_TIMER, 0))
            .unwrap_or(0);

        let new_timer = timer + 1;

        // Update timer
        if let Some(entity) = ctx.world.entities.get_mut(&candidate.settlement_id) {
            entity
                .extra
                .insert(K::BLEND_TIMER.to_string(), serde_json::json!(new_timer));
        }

        if new_timer < BLEND_TIMER_THRESHOLD {
            continue;
        }

        if !ctx.rng.random_bool(BLEND_CHANCE_PER_YEAR) {
            continue;
        }

        // Pick first two qualifying parents
        let parent_a = candidate.parent_cultures[0].0;
        let parent_b = candidate.parent_cultures[1].0;

        // Inherit one value from each parent, and one parent's naming style
        let value_a = ctx
            .world
            .entities
            .get(&parent_a)
            .and_then(|e| e.data.as_culture())
            .and_then(|cd| cd.values.first().cloned());
        let value_b = ctx
            .world
            .entities
            .get(&parent_b)
            .and_then(|e| e.data.as_culture())
            .and_then(|cd| cd.values.last().cloned());
        let naming_style = ctx
            .world
            .entities
            .get(&parent_a)
            .and_then(|e| e.data.as_culture())
            .map(|cd| cd.naming_style.clone())
            .unwrap_or(NamingStyle::Nordic);
        let resistance = ctx
            .world
            .entities
            .get(&parent_a)
            .and_then(|e| e.data.as_culture())
            .map(|cd| cd.resistance)
            .unwrap_or(0.5);

        let mut values = Vec::new();
        if let Some(v) = value_a {
            values.push(v);
        }
        if let Some(v) = value_b
            && !values.contains(&v)
        {
            values.push(v);
        }

        let name = generate_culture_entity_name(ctx.rng);
        let settlement_name = ctx
            .world
            .entities
            .get(&candidate.settlement_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();

        let ev = ctx.world.add_event(
            EventKind::Custom("culture_blended".to_string()),
            time,
            format!("A blended culture {name} emerged in {settlement_name}"),
        );

        let blended_id = ctx.world.add_entity(
            EntityKind::Culture,
            name,
            Some(time),
            EntityData::Culture(CultureData {
                values,
                naming_style,
                resistance,
            }),
            ev,
        );

        // Replace both parents' fractions in this settlement
        if let Some(entity) = ctx.world.entities.get_mut(&candidate.settlement_id) {
            if let Some(sd) = entity.data.as_settlement_mut() {
                let share_a = sd.culture_makeup.remove(&parent_a).unwrap_or(0.0);
                let share_b = sd.culture_makeup.remove(&parent_b).unwrap_or(0.0);
                sd.culture_makeup.insert(blended_id, share_a + share_b);
                if sd.dominant_culture == Some(parent_a) || sd.dominant_culture == Some(parent_b) {
                    sd.dominant_culture = Some(blended_id);
                }
            }
            entity.extra.remove(K::BLEND_TIMER);
        }
    }
}

// --- Phase C: Rebellion Check ---

fn rebellion_check(ctx: &mut TickContext) {
    let time = ctx.world.current_time;
    let current_year = time.year();

    struct RebellionCandidate {
        settlement_id: u64,
        faction_id: u64,
        dominant_culture: u64,
        tension: f64,
        stability: f64,
        resistance: f64,
    }

    let mut candidates: Vec<RebellionCandidate> = Vec::new();

    for entity in ctx.world.entities.values() {
        if entity.kind != EntityKind::Settlement || entity.end.is_some() {
            continue;
        }
        let sd = match entity.data.as_settlement() {
            Some(sd) => sd,
            None => continue,
        };

        if sd.cultural_tension <= REBELLION_TENSION_THRESHOLD {
            continue;
        }

        let dominant = match sd.dominant_culture {
            Some(c) => c,
            None => continue,
        };

        let faction_id = match entity.active_rel(RelationshipKind::MemberOf) {
            Some(f) => f,
            None => continue,
        };

        let (faction_primary, stability) = match ctx.world.entities.get(&faction_id) {
            Some(f) => {
                let fd = f.data.as_faction();
                (
                    fd.and_then(|fd| fd.primary_culture),
                    fd.map(|fd| fd.stability).unwrap_or(0.5),
                )
            }
            None => continue,
        };

        // Check: dominant culture != faction's primary culture
        if faction_primary == Some(dominant) {
            continue;
        }

        if stability >= REBELLION_STABILITY_THRESHOLD {
            continue;
        }

        let resistance = ctx
            .world
            .entities
            .get(&dominant)
            .and_then(|e| e.data.as_culture())
            .map(|cd| cd.resistance)
            .unwrap_or(0.5);

        candidates.push(RebellionCandidate {
            settlement_id: entity.id,
            faction_id,
            dominant_culture: dominant,
            tension: sd.cultural_tension,
            stability,
            resistance,
        });
    }

    for c in candidates {
        let rebellion_chance =
            REBELLION_BASE_CHANCE * c.tension * (1.0 - c.stability) * c.resistance;
        if !ctx.rng.random_bool(rebellion_chance.clamp(0.0, 1.0)) {
            continue;
        }

        let settlement_name = ctx
            .world
            .entities
            .get(&c.settlement_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let culture_name = ctx
            .world
            .entities
            .get(&c.dominant_culture)
            .map(|e| e.name.clone())
            .unwrap_or_default();

        let ev = ctx.world.add_event(
            EventKind::Rebellion,
            time,
            format!(
                "Cultural rebellion by {culture_name} in {settlement_name} in year {current_year}"
            ),
        );
        ctx.world
            .add_event_participant(ev, c.settlement_id, ParticipantRole::Location);

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::CulturalRebellion {
                settlement_id: c.settlement_id,
                faction_id: c.faction_id,
                culture_id: c.dominant_culture,
            },
        });

        // Success check
        let mut success_chance: f64 = REBELLION_BASE_SUCCESS_CHANCE;
        if c.tension > REBELLION_HIGH_TENSION_THRESHOLD {
            success_chance += REBELLION_HIGH_TENSION_BONUS;
        }
        if c.stability < REBELLION_LOW_STABILITY_THRESHOLD {
            success_chance += REBELLION_LOW_STABILITY_BONUS;
        }

        if ctx.rng.random_bool(success_chance.clamp(0.0, 1.0)) {
            // Successful rebellion — emit FactionSplit signal
            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::FactionSplit {
                    old_faction_id: c.faction_id,
                    new_faction_id: None,
                    settlement_id: c.settlement_id,
                },
            });
        } else {
            // Failed rebellion — stability hit, crackdown
            if let Some(faction) = ctx.world.entities.get_mut(&c.faction_id)
                && let Some(fd) = faction.data.as_faction_mut()
            {
                fd.stability = (fd.stability - REBELLION_FAILED_STABILITY_PENALTY).max(0.0);
            }
            // Ruling culture gains share (crackdown)
            let ruling_culture = ctx
                .world
                .entities
                .get(&c.faction_id)
                .and_then(|f| f.data.as_faction())
                .and_then(|fd| fd.primary_culture);
            if let Some(rc) = ruling_culture {
                add_culture_share(ctx, c.settlement_id, rc, REBELLION_CRACKDOWN_CULTURE_SHARE);
            }
        }
    }
}

// --- Helpers ---

fn add_culture_share(ctx: &mut TickContext, settlement_id: u64, culture_id: u64, share: f64) {
    if let Some(entity) = ctx.world.entities.get_mut(&settlement_id)
        && let Some(sd) = entity.data.as_settlement_mut()
    {
        *sd.culture_makeup.entry(culture_id).or_insert(0.0) += share;
        // Normalize
        let total: f64 = sd.culture_makeup.values().sum();
        if total > 0.0 {
            for v in sd.culture_makeup.values_mut() {
                *v /= total;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::cultural_value::CulturalValue;
    use crate::model::{SimTimestamp, World};
    use crate::scenario::Scenario;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    /// Two cultures, one faction (primary=culture_a), one settlement with mixed makeup.
    fn culture_scenario() -> (World, u64, u64, u64, u64) {
        let mut s = Scenario::at_year(100);

        let culture_a = s.add_culture_with("CultureA", |cd| {
            cd.values = vec![CulturalValue::Martial];
            cd.naming_style = NamingStyle::Nordic;
            cd.resistance = 0.7;
        });
        let culture_b = s.add_culture_with("CultureB", |cd| {
            cd.values = vec![CulturalValue::Mercantile];
            cd.naming_style = NamingStyle::Desert;
            cd.resistance = 0.3;
        });

        let mut makeup = BTreeMap::new();
        makeup.insert(culture_a, 0.6);
        makeup.insert(culture_b, 0.4);

        let setup = s.add_settlement_standalone("TestTown");
        let _ = s
            .faction_mut(setup.faction)
            .primary_culture(Some(culture_a));
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .dominant_culture(Some(culture_a))
            .culture_makeup(makeup)
            .cultural_tension(0.4);
        let settlement = setup.settlement;
        let faction = setup.faction;

        (s.build(), settlement, faction, culture_a, culture_b)
    }

    #[test]
    fn scenario_drift_changes_makeup_over_time() {
        let (mut world, settlement, _, culture_a, culture_b) = culture_scenario();
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();

        // Run drift multiple times
        for year in 100..120 {
            world.current_time = ts(year);
            let mut ctx = TickContext {
                world: &mut world,
                rng: &mut rng,
                signals: &mut signals,
                inbox: &[],
            };
            cultural_drift(&mut ctx);
        }

        let sd = world
            .entities
            .get(&settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap();

        let a_share = sd.culture_makeup.get(&culture_a).copied().unwrap_or(0.0);
        let b_share = sd.culture_makeup.get(&culture_b).copied().unwrap_or(0.0);

        // Ruling culture (A) should have gained
        assert!(a_share > 0.6, "culture A should grow, got {a_share}");
        // Minority culture (B) should have shrunk
        assert!(b_share < 0.4, "culture B should shrink, got {b_share}");
    }

    fn rebellion_scenario() -> World {
        let mut s = Scenario::at_year(100);

        let culture_ruler = s.add_culture_with("RulerCulture", |cd| {
            cd.values = vec![CulturalValue::Martial];
            cd.naming_style = NamingStyle::Imperial;
            cd.resistance = 0.5;
        });
        let culture_local = s.add_culture_with("LocalCulture", |cd| {
            cd.values = vec![CulturalValue::Scholarly];
            cd.naming_style = NamingStyle::Elvish;
            cd.resistance = 0.9;
        });

        let mut makeup = BTreeMap::new();
        makeup.insert(culture_local, 0.55);
        makeup.insert(culture_ruler, 0.45);

        let setup = s.add_settlement_standalone("OppressedTown");
        let _ = s
            .faction_mut(setup.faction)
            .stability(0.2)
            .happiness(0.3)
            .legitimacy(0.3)
            .treasury(50.0)
            .primary_culture(Some(culture_ruler));
        let _ = s
            .settlement_mut(setup.settlement)
            .population(300)
            .prosperity(0.4)
            .dominant_culture(Some(culture_local))
            .culture_makeup(makeup)
            .cultural_tension(0.45);

        s.build()
    }

    #[test]
    fn scenario_rebellion_fires_under_conditions() {
        // Run rebellion check many times with fresh worlds to verify it can fire
        let mut rebellion_count = 0;
        for seed in 0..500 {
            let mut world = rebellion_scenario();
            let mut rng = SmallRng::seed_from_u64(seed);
            let mut signals = Vec::new();
            let mut ctx = TickContext {
                world: &mut world,
                rng: &mut rng,
                signals: &mut signals,
                inbox: &[],
            };
            rebellion_check(&mut ctx);
            if signals
                .iter()
                .any(|s| matches!(s.kind, SignalKind::CulturalRebellion { .. }))
            {
                rebellion_count += 1;
            }
        }

        assert!(
            rebellion_count > 0,
            "rebellion should fire at least once in 500 attempts"
        );
        assert!(
            rebellion_count < 500,
            "rebellion should not fire every time"
        );
    }

    #[test]
    fn scenario_signal_settlement_captured_adds_culture() {
        let (mut world, settlement, faction, _culture_a, _culture_b) = culture_scenario();

        // Create a conqueror culture and faction via Scenario API on the existing world
        let ev = world.events.keys().next().copied().unwrap();
        let culture_c = world.add_entity(
            EntityKind::Culture,
            "ConquerorCulture".to_string(),
            Some(ts(100)),
            EntityData::Culture(CultureData {
                values: vec![CulturalValue::Martial],
                naming_style: NamingStyle::Steppe,
                resistance: 0.5,
            }),
            ev,
        );
        let new_faction = world.add_entity(
            EntityKind::Faction,
            "Conquerors".to_string(),
            Some(ts(100)),
            EntityData::default_for_kind(EntityKind::Faction),
            ev,
        );
        if let Some(fd) = world
            .entities
            .get_mut(&new_faction)
            .and_then(|e| e.data.as_faction_mut())
        {
            fd.primary_culture = Some(culture_c);
            fd.treasury = 200.0;
        }

        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::SettlementCaptured {
                settlement_id: settlement,
                old_faction_id: faction,
                new_faction_id: new_faction,
            },
        }];

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &inbox,
        };

        let mut system = CultureSystem;
        system.handle_signals(&mut ctx);

        // Check that culture_c was added to settlement
        let sd = world
            .entities
            .get(&settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap();
        assert!(
            sd.culture_makeup.contains_key(&culture_c),
            "conqueror culture should be added to settlement"
        );
    }

    // -----------------------------------------------------------------------
    // Signal handler tests (deliver_signals, zero ticks)
    // -----------------------------------------------------------------------

    use crate::testutil;

    #[test]
    fn scenario_refugees_bring_culture() {
        let mut s = Scenario::at_year(100);
        let culture_src = s.add_culture("SourceCulture");
        let culture_dst = s.add_culture("DestCulture");
        let r = s.add_region("R");
        let f = s.add_faction("F");
        let _ = s.faction_mut(f).primary_culture(Some(culture_dst));

        let mut src_makeup = BTreeMap::new();
        src_makeup.insert(culture_src, 1.0);
        let source = s
            .settlement("Source", f, r)
            .population(500)
            .dominant_culture(Some(culture_src))
            .culture_makeup(src_makeup)
            .id();

        let mut dst_makeup = BTreeMap::new();
        dst_makeup.insert(culture_dst, 1.0);
        let dest = s
            .settlement("Dest", f, r)
            .population(500)
            .dominant_culture(Some(culture_dst))
            .culture_makeup(dst_makeup)
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
        testutil::deliver_signals(&mut world, &mut CultureSystem, &inbox, 42);

        let sd = world.settlement(dest);
        assert!(
            sd.culture_makeup.contains_key(&culture_src),
            "destination should gain source culture after refugees arrive"
        );
    }

    #[test]
    fn scenario_trade_spreads_culture() {
        let mut s = Scenario::at_year(100);
        let culture_a = s.add_culture("CultureA");
        let culture_b = s.add_culture("CultureB");
        let r = s.add_region("R");
        let fa = s.add_faction("FA");
        let fb = s.add_faction("FB");
        let _ = s.faction_mut(fa).primary_culture(Some(culture_a));
        let _ = s.faction_mut(fb).primary_culture(Some(culture_b));

        let mut makeup_a = BTreeMap::new();
        makeup_a.insert(culture_a, 1.0);
        let sa = s
            .settlement("SA", fa, r)
            .population(300)
            .dominant_culture(Some(culture_a))
            .culture_makeup(makeup_a)
            .id();

        let mut makeup_b = BTreeMap::new();
        makeup_b.insert(culture_b, 1.0);
        let sb = s
            .settlement("SB", fb, r)
            .population(300)
            .dominant_culture(Some(culture_b))
            .culture_makeup(makeup_b)
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
        testutil::deliver_signals(&mut world, &mut CultureSystem, &inbox, 42);

        let sd_a = world.settlement(sa);
        let sd_b = world.settlement(sb);
        assert!(
            sd_a.culture_makeup.contains_key(&culture_b),
            "settlement A should gain culture B from trade"
        );
        assert!(
            sd_b.culture_makeup.contains_key(&culture_a),
            "settlement B should gain culture A from trade"
        );
    }

    #[test]
    fn scenario_faction_split_inherits_culture() {
        let mut s = Scenario::at_year(100);
        let culture = s.add_culture("SplitCulture");
        let r = s.add_region("R");
        let old_f = s.add_faction("OldFaction");
        let new_f = s.add_faction("NewFaction");

        let mut makeup = BTreeMap::new();
        makeup.insert(culture, 1.0);
        let sett = s
            .settlement("Town", old_f, r)
            .population(300)
            .dominant_culture(Some(culture))
            .culture_makeup(makeup)
            .id();

        let mut world = s.build();

        // Verify new faction has no primary culture initially
        assert!(world.faction(new_f).primary_culture.is_none());

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::FactionSplit {
                old_faction_id: old_f,
                new_faction_id: Some(new_f),
                settlement_id: sett,
            },
        }];
        testutil::deliver_signals(&mut world, &mut CultureSystem, &inbox, 42);

        assert_eq!(
            world.faction(new_f).primary_culture,
            Some(culture),
            "new faction should inherit settlement's dominant culture"
        );
    }
}
