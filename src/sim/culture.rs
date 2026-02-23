use std::collections::BTreeMap;

use rand::Rng;

use super::context::TickContext;
use super::culture_names::generate_culture_entity_name;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::cultural_value::NamingStyle;
use crate::model::entity_data::CultureData;
use crate::model::{EntityData, EntityKind, EventKind, ParticipantRole, RelationshipKind};

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
                    // Add conquering faction's culture at 0.05 share
                    let conqueror_culture = ctx
                        .world
                        .entities
                        .get(new_faction_id)
                        .and_then(|f| f.data.as_faction())
                        .and_then(|fd| fd.primary_culture);
                    if let Some(culture_id) = conqueror_culture {
                        add_culture_share(ctx, *settlement_id, culture_id, 0.05);
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
                            (*count as f64 / dest_pop as f64).min(0.20)
                        } else {
                            0.05
                        };
                        add_culture_share(ctx, *settlement_id, culture_id, fraction);
                    }
                }
                SignalKind::TradeRouteEstablished {
                    from_settlement,
                    to_settlement,
                    ..
                } => {
                    // Add 0.01 of partner's dominant culture in each settlement
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
                        add_culture_share(ctx, *from_settlement, c, 0.01);
                    }
                    if let Some(c) = from_culture {
                        add_culture_share(ctx, *to_settlement, c, 0.01);
                    }
                }
                SignalKind::FactionSplit {
                    new_faction_id,
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
            let faction_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
                .map(|r| r.target_entity_id);
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

                let mut loss = 0.02 * (1.0 - resistance);
                loss += trade_bonus * 0.005;
                if s.prosperity > 0.6 {
                    loss += 0.005;
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

        // Purge cultures below 0.03
        new_makeup.retain(|_, v| *v >= 0.03);

        // Re-normalize after purge
        let total: f64 = new_makeup.values().sum();
        if total > 0.0 && (total - 1.0).abs() > 0.001 {
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
            .filter(|(_, v)| **v > 0.5)
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
        .get("trade_routes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
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

        // Check: 2+ cultures each above 0.30
        let qualifying: Vec<(u64, f64)> = sd
            .culture_makeup
            .iter()
            .filter(|&(_, v)| *v >= 0.30)
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
            .and_then(|e| e.extra.get("blend_timer"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let new_timer = timer + 1;

        // Update timer
        if let Some(entity) = ctx.world.entities.get_mut(&candidate.settlement_id) {
            entity
                .extra
                .insert("blend_timer".to_string(), serde_json::json!(new_timer));
        }

        if new_timer < 50 {
            continue;
        }

        // 5% chance per year to blend
        if !ctx.rng.random_bool(0.05) {
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
            entity.extra.remove("blend_timer");
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

        if sd.cultural_tension <= 0.35 {
            continue;
        }

        let dominant = match sd.dominant_culture {
            Some(c) => c,
            None => continue,
        };

        let faction_id = match entity
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::MemberOf && r.end.is_none())
            .map(|r| r.target_entity_id)
        {
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

        if stability >= 0.5 {
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
        let rebellion_chance = 0.03 * c.tension * (1.0 - c.stability) * c.resistance;
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
        let mut success_chance: f64 = 0.40;
        if c.tension > 0.6 {
            success_chance += 0.20;
        }
        if c.stability < 0.3 {
            success_chance += 0.10;
        }

        if ctx.rng.random_bool(success_chance.clamp(0.0, 1.0)) {
            // Successful rebellion — emit FactionSplit signal
            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::FactionSplit {
                    old_faction_id: c.faction_id,
                    new_faction_id: 0, // politics system will handle actual creation
                    settlement_id: c.settlement_id,
                },
            });
        } else {
            // Failed rebellion — stability hit, crackdown
            if let Some(faction) = ctx.world.entities.get_mut(&c.faction_id)
                && let Some(fd) = faction.data.as_faction_mut()
            {
                fd.stability = (fd.stability - 0.10).max(0.0);
            }
            // Ruling culture gains +0.10 share (crackdown)
            let ruling_culture = ctx
                .world
                .entities
                .get(&c.faction_id)
                .and_then(|f| f.data.as_faction())
                .and_then(|fd| fd.primary_culture);
            if let Some(rc) = ruling_culture {
                add_culture_share(ctx, c.settlement_id, rc, 0.10);
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
    use crate::model::entity_data::{FactionData, SettlementData};
    use crate::model::{SimTimestamp, World};
    use crate::sim::population::PopulationBreakdown;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    fn setup_world_with_cultures() -> (World, u64, u64, u64, u64) {
        let mut world = World::new();
        world.current_time = ts(100);

        // Create two cultures
        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            ts(0),
            "setup".to_string(),
        );

        let culture_a = world.add_entity(
            EntityKind::Culture,
            "CultureA".to_string(),
            Some(ts(0)),
            EntityData::Culture(CultureData {
                values: vec![CulturalValue::Martial],
                naming_style: NamingStyle::Nordic,
                resistance: 0.7,
            }),
            ev,
        );

        let culture_b = world.add_entity(
            EntityKind::Culture,
            "CultureB".to_string(),
            Some(ts(0)),
            EntityData::Culture(CultureData {
                values: vec![CulturalValue::Mercantile],
                naming_style: NamingStyle::Desert,
                resistance: 0.3,
            }),
            ev,
        );

        // Create faction with culture_a as primary
        let faction = world.add_entity(
            EntityKind::Faction,
            "TestFaction".to_string(),
            Some(ts(0)),
            EntityData::Faction(FactionData {
                government_type: "chieftain".to_string(),
                stability: 0.5,
                happiness: 0.5,
                legitimacy: 0.5,
                treasury: 100.0,
                alliance_strength: 0.0,
                primary_culture: Some(culture_a),
                prestige: 0.0,
            }),
            ev,
        );

        // Create settlement with mixed cultures
        let mut makeup = BTreeMap::new();
        makeup.insert(culture_a, 0.6);
        makeup.insert(culture_b, 0.4);

        let settlement = world.add_entity(
            EntityKind::Settlement,
            "TestTown".to_string(),
            Some(ts(0)),
            EntityData::Settlement(SettlementData {
                population: 500,
                population_breakdown: PopulationBreakdown::from_total(500),
                x: 0.0,
                y: 0.0,
                resources: vec![],
                prosperity: 0.5,
                treasury: 0.0,
                dominant_culture: Some(culture_a),
                culture_makeup: makeup,
                cultural_tension: 0.4,
                active_disease: None,
                plague_immunity: 0.0,
                fortification_level: 0,
                active_siege: None,
                prestige: 0.0,
                active_disaster: None,
            }),
            ev,
        );
        world.add_relationship(settlement, faction, RelationshipKind::MemberOf, ts(0), ev);

        (world, settlement, faction, culture_a, culture_b)
    }

    #[test]
    fn drift_changes_makeup_over_time() {
        let (mut world, settlement, _, culture_a, culture_b) = setup_world_with_cultures();
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

    fn make_rebellion_world() -> World {
        let mut world = World::new();
        world.current_time = ts(100);

        let ev = world.add_event(
            EventKind::Custom("setup".to_string()),
            ts(0),
            "setup".to_string(),
        );

        let culture_ruler = world.add_entity(
            EntityKind::Culture,
            "RulerCulture".to_string(),
            Some(ts(0)),
            EntityData::Culture(CultureData {
                values: vec![CulturalValue::Martial],
                naming_style: NamingStyle::Imperial,
                resistance: 0.5,
            }),
            ev,
        );

        let culture_local = world.add_entity(
            EntityKind::Culture,
            "LocalCulture".to_string(),
            Some(ts(0)),
            EntityData::Culture(CultureData {
                values: vec![CulturalValue::Scholarly],
                naming_style: NamingStyle::Elvish,
                resistance: 0.9,
            }),
            ev,
        );

        let faction = world.add_entity(
            EntityKind::Faction,
            "OppressiveFaction".to_string(),
            Some(ts(0)),
            EntityData::Faction(FactionData {
                government_type: "chieftain".to_string(),
                stability: 0.2,
                happiness: 0.3,
                legitimacy: 0.3,
                treasury: 50.0,
                alliance_strength: 0.0,
                primary_culture: Some(culture_ruler),
                prestige: 0.0,
            }),
            ev,
        );

        // Settlement with high tension: dominant is local, faction primary is ruler
        let mut makeup = BTreeMap::new();
        makeup.insert(culture_local, 0.55);
        makeup.insert(culture_ruler, 0.45);

        let settlement = world.add_entity(
            EntityKind::Settlement,
            "OppressedTown".to_string(),
            Some(ts(0)),
            EntityData::Settlement(SettlementData {
                population: 300,
                population_breakdown: PopulationBreakdown::from_total(300),
                x: 0.0,
                y: 0.0,
                resources: vec![],
                prosperity: 0.4,
                treasury: 0.0,
                dominant_culture: Some(culture_local),
                culture_makeup: makeup,
                cultural_tension: 0.45,
                active_disease: None,
                plague_immunity: 0.0,
                fortification_level: 0,
                active_siege: None,
                prestige: 0.0,
                active_disaster: None,
            }),
            ev,
        );
        world.add_relationship(settlement, faction, RelationshipKind::MemberOf, ts(0), ev);
        world
    }

    #[test]
    fn rebellion_fires_under_conditions() {
        // Run rebellion check many times with fresh worlds to verify it can fire
        let mut rebellion_count = 0;
        for seed in 0..500 {
            let mut world = make_rebellion_world();
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
    fn signal_settlement_captured_adds_culture() {
        let (mut world, settlement, faction, culture_a, _culture_b) = setup_world_with_cultures();

        // Create a new culture for the conqueror
        let ev = world.add_event(
            EventKind::Custom("test".to_string()),
            ts(100),
            "test".to_string(),
        );
        let culture_c = world.add_entity(
            EntityKind::Culture,
            "ConquerorCulture".to_string(),
            Some(ts(0)),
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
            Some(ts(0)),
            EntityData::Faction(FactionData {
                government_type: "chieftain".to_string(),
                stability: 0.8,
                happiness: 0.7,
                legitimacy: 0.8,
                treasury: 200.0,
                alliance_strength: 0.0,
                primary_culture: Some(culture_c),
                prestige: 0.0,
            }),
            ev,
        );

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
}
