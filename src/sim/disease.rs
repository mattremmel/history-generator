use rand::Rng;

use crate::model::entity::EntityKind;
use crate::model::entity_data::{ActiveDisease, DisasterType, DiseaseData};
use crate::model::event::{EventKind, ParticipantRole};
use crate::model::population::NUM_BRACKETS;
use crate::model::relationship::RelationshipKind;
use crate::model::timestamp::SimTimestamp;
use crate::worldgen::terrain::Terrain;

use super::context::TickContext;
use super::extra_keys as K;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};

// --- Constants ---

/// Base annual probability of a spontaneous outbreak in a settlement.
const BASE_OUTBREAK_CHANCE: f64 = 0.002;
/// Bonus outbreak chance when population exceeds 80% of carrying capacity.
const OVERCROWDING_BONUS: f64 = 0.003;
/// Bonus outbreak chance for swamp/jungle terrain.
const TERRAIN_BONUS: f64 = 0.002;
/// Bonus outbreak chance per active trade route.
const TRADE_ROUTE_BONUS: f64 = 0.0005;
/// Bonus outbreak chance when prosperity is low (post-war devastation).
const LOW_PROSPERITY_BONUS: f64 = 0.001;
/// Multiplier applied to outbreak chance for small settlements (pop < 100).
const SMALL_SETTLEMENT_FACTOR: f64 = 0.5;

/// Base transmission probability per infected settlement per connected target.
const BASE_TRANSMISSION: f64 = 0.3;
/// Extra transmission bonus when settlements are connected by a trade route.
const TRADE_TRANSMISSION_BONUS: f64 = 0.2;
/// Multiplier for adjacency-only spread (no trade route).
const ADJACENCY_ONLY_FACTOR: f64 = 0.5;

/// Infection rate climbs toward this fraction of virulence during ramp phase.
const RAMP_TARGET_FRACTION: f64 = 0.6;
/// Annual decay rate of infection_rate during decline phase.
const DECLINE_RATE: f64 = 0.30;
/// Infection rate below which the plague ends.
const END_THRESHOLD: f64 = 0.02;

/// Immunity granted when a plague ends in a settlement.
const RECOVERY_IMMUNITY: f64 = 0.7;
/// Annual decay of plague_immunity.
const IMMUNITY_DECAY: f64 = 0.05;

/// NPC plague death modifier (slightly lower than general pop — better fed, can isolate).
const NPC_DEATH_MODIFIER: f64 = 0.5;

// --- Disease profiles ---

/// Bracket severity profiles: [infant, child, young_adult, middle_age, elder, aged, ancient, centenarian]
const PROFILE_CLASSIC: [f64; NUM_BRACKETS] = [2.0, 0.5, 0.3, 0.5, 1.5, 2.5, 3.0, 4.0];
const PROFILE_YOUNG_KILLER: [f64; NUM_BRACKETS] = [1.0, 0.5, 2.5, 2.0, 1.0, 0.8, 0.5, 0.3];
const PROFILE_CHILD_KILLER: [f64; NUM_BRACKETS] = [3.0, 2.5, 0.3, 0.3, 0.5, 1.0, 1.5, 2.0];
const PROFILE_INDISCRIMINATE: [f64; NUM_BRACKETS] = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

const PROFILES: [[f64; NUM_BRACKETS]; 4] = [
    PROFILE_CLASSIC,
    PROFILE_YOUNG_KILLER,
    PROFILE_CHILD_KILLER,
    PROFILE_INDISCRIMINATE,
];

// --- Name generation ---

const ADJECTIVES: &[&str] = &[
    "Red", "Black", "Grey", "Sweating", "Rotting", "Creeping", "Silent", "Weeping", "White",
    "Burning", "Crimson", "Pale",
];

const NOUNS: &[&str] = &[
    "Plague",
    "Pox",
    "Fever",
    "Blight",
    "Wasting",
    "Flux",
    "Pestilence",
    "Death",
    "Rot",
    "Shakes",
    "Sickness",
];

fn generate_disease_name(rng: &mut dyn rand::RngCore) -> String {
    let adj = ADJECTIVES[rng.random_range(0..ADJECTIVES.len())];
    let noun = NOUNS[rng.random_range(0..NOUNS.len())];
    format!("The {adj} {noun}")
}

fn random_disease_data(rng: &mut dyn rand::RngCore) -> DiseaseData {
    let profile = PROFILES[rng.random_range(0..PROFILES.len())];
    DiseaseData {
        virulence: rng.random_range(0.3..0.8),
        lethality: rng.random_range(0.1..0.5),
        duration_years: rng.random_range(2..6),
        bracket_severity: profile,
    }
}

// --- Settlement info collection ---

struct SettlementDiseaseInfo {
    id: u64,
    population: u32,
    prosperity: f64,
    plague_immunity: f64,
    active_disease: Option<ActiveDisease>,
    region_id: Option<u64>,
    trade_route_targets: Vec<u64>,
    terrain: Terrain,
    carrying_capacity: u32,
}

fn collect_settlement_info(world: &crate::model::World) -> Vec<SettlementDiseaseInfo> {
    // Pre-compute region capacities
    let region_data: Vec<(u64, Terrain, u32)> = world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region)
        .filter_map(|e| {
            let region = e.data.as_region()?;
            let profile = crate::worldgen::terrain::TerrainProfile::new(
                region.terrain,
                region.terrain_tags.clone(),
            );
            let capacity = profile.effective_population_range().1 * 5;
            Some((e.id, region.terrain, capacity))
        })
        .collect();

    world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let s = e.data.as_settlement()?;

            let region_id = e
                .relationships
                .iter()
                .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
                .map(|r| r.target_entity_id);

            let trade_route_targets: Vec<u64> = e
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::TradeRoute && r.end.is_none())
                .map(|r| r.target_entity_id)
                .collect();

            let (terrain, carrying_capacity) = region_data
                .iter()
                .find(|(id, _, _)| Some(*id) == region_id)
                .map(|(_, t, c)| (*t, *c))
                .unwrap_or((Terrain::Plains, 500));

            Some(SettlementDiseaseInfo {
                id: e.id,
                population: s.population,
                prosperity: s.prosperity,
                plague_immunity: s.plague_immunity,
                active_disease: s.active_disease.clone(),
                region_id,
                trade_route_targets,
                terrain,
                carrying_capacity,
            })
        })
        .collect()
}

/// Find adjacent settlement IDs (settlements in regions adjacent to this settlement's region).
fn find_adjacent_settlements(
    world: &crate::model::World,
    settlement_region_id: Option<u64>,
    exclude_id: u64,
) -> Vec<u64> {
    let Some(region_id) = settlement_region_id else {
        return Vec::new();
    };

    // Find adjacent region IDs
    let adjacent_regions: Vec<u64> = world
        .entities
        .get(&region_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::AdjacentTo && r.end.is_none())
                .map(|r| r.target_entity_id)
                .collect()
        })
        .unwrap_or_default();

    // Find settlements in those regions
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Settlement
                && e.end.is_none()
                && e.id != exclude_id
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LocatedIn
                        && r.end.is_none()
                        && adjacent_regions.contains(&r.target_entity_id)
                })
        })
        .map(|e| e.id)
        .collect()
}

/// Determine which age bracket a person falls into given their birth year and current year.
fn age_bracket(birth_year: u32, current_year: u32) -> usize {
    use crate::model::population::BRACKET_WIDTHS;
    let age = current_year.saturating_sub(birth_year);
    let mut cumulative = 0u32;
    for (i, &width) in BRACKET_WIDTHS.iter().enumerate() {
        cumulative = cumulative.saturating_add(width);
        if age < cumulative {
            return i;
        }
    }
    NUM_BRACKETS - 1
}

// --- The System ---

pub struct DiseaseSystem;

impl SimSystem for DiseaseSystem {
    fn name(&self) -> &str {
        "disease"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        let settlements = collect_settlement_info(ctx.world);

        // Phase 1: Immunity decay (before outbreak checks)
        decay_immunity(ctx, &settlements, time);

        // Phase 2: Spontaneous outbreak checks
        check_outbreaks(ctx, &settlements, time);

        // Re-collect after possible mutations
        let settlements = collect_settlement_info(ctx.world);

        // Phase 3: Disease spread
        spread_disease(ctx, &settlements, time);

        // Re-collect after possible mutations
        let settlements = collect_settlement_info(ctx.world);

        // Phase 4: Disease progression + mortality
        progress_and_mortality(ctx, &settlements, time, current_year);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        // Listen for RefugeesArrived and SettlementCaptured to increase outbreak chance.
        // We store a transient marker in the settlement's extra data for this tick.
        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::RefugeesArrived { settlement_id, .. } => {
                    // Mark settlement as having received refugees (increases outbreak chance next tick)
                    if let Some(entity) = ctx.world.entities.get_mut(settlement_id) {
                        let current: f64 = entity
                            .extra
                            .get("refugee_disease_risk")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        entity.extra.insert(
                            "refugee_disease_risk".to_string(),
                            serde_json::json!(current + 0.0015),
                        );
                    }
                }
                SignalKind::SettlementCaptured { settlement_id, .. } => {
                    if let Some(entity) = ctx.world.entities.get_mut(settlement_id) {
                        entity.extra.insert(
                            "post_conquest_disease_risk".to_string(),
                            serde_json::json!(0.003),
                        );
                    }
                }
                SignalKind::SiegeStarted { settlement_id, .. } => {
                    if let Some(entity) = ctx.world.entities.get_mut(settlement_id) {
                        entity
                            .extra
                            .insert("siege_disease_bonus".to_string(), serde_json::json!(0.002));
                    }
                }
                SignalKind::SiegeEnded { settlement_id, .. } => {
                    if let Some(entity) = ctx.world.entities.get_mut(settlement_id) {
                        entity.extra.remove("siege_disease_bonus");
                    }
                }
                // Floods and earthquakes leave behind disease-prone conditions
                SignalKind::DisasterStruck {
                    settlement_id,
                    disaster_type,
                    ..
                } if *disaster_type == DisasterType::Flood
                    || *disaster_type == DisasterType::Earthquake =>
                {
                    if let Some(entity) = ctx.world.entities.get_mut(settlement_id) {
                        entity.extra.insert(
                            "post_disaster_disease_risk".to_string(),
                            serde_json::json!(0.002),
                        );
                    }
                }
                SignalKind::DisasterEnded { settlement_id, .. } => {
                    if let Some(entity) = ctx.world.entities.get_mut(settlement_id) {
                        entity.extra.remove("post_disaster_disease_risk");
                    }
                }
                _ => {}
            }
        }
    }
}

fn decay_immunity(
    ctx: &mut TickContext,
    settlements: &[SettlementDiseaseInfo],
    time: SimTimestamp,
) {
    for info in settlements {
        if info.plague_immunity > 0.0 {
            let entity = ctx.world.entities.get_mut(&info.id).unwrap();
            if let Some(s) = entity.data.as_settlement_mut() {
                s.plague_immunity = (s.plague_immunity - IMMUNITY_DECAY).max(0.0);
            }
        }
    }
    let _ = time;
}

fn check_outbreaks(
    ctx: &mut TickContext,
    settlements: &[SettlementDiseaseInfo],
    time: SimTimestamp,
) {
    // Collect outbreak decisions first to avoid borrow conflicts
    struct OutbreakTarget {
        settlement_id: u64,
    }

    let mut targets = Vec::new();

    for info in settlements {
        // Skip settlements that already have an active disease
        if info.active_disease.is_some() {
            continue;
        }

        let mut chance = BASE_OUTBREAK_CHANCE;

        // Overcrowding
        if info.carrying_capacity > 0
            && info.population as f64 / info.carrying_capacity as f64 > 0.8
        {
            chance += OVERCROWDING_BONUS;
        }

        // Terrain
        if matches!(info.terrain, Terrain::Swamp | Terrain::Jungle) {
            chance += TERRAIN_BONUS;
        }

        // Trade routes increase exposure
        chance += info.trade_route_targets.len() as f64 * TRADE_ROUTE_BONUS;

        // Post-war devastation
        if info.prosperity < 0.3 {
            chance += LOW_PROSPERITY_BONUS;
        }

        // Refugee risk (set by handle_signals)
        if let Some(entity) = ctx.world.entities.get(&info.id) {
            if let Some(risk) = entity
                .extra
                .get("refugee_disease_risk")
                .and_then(|v| v.as_f64())
            {
                chance += risk;
            }
            if let Some(risk) = entity
                .extra
                .get("post_conquest_disease_risk")
                .and_then(|v| v.as_f64())
            {
                chance += risk;
            }
            // Post-disaster disease risk (floods, earthquakes)
            if let Some(risk) = entity
                .extra
                .get("post_disaster_disease_risk")
                .and_then(|v| v.as_f64())
            {
                chance += risk;
            }
        }

        // Seasonal disease modifier from environment system
        if let Some(entity) = ctx.world.entities.get(&info.id) {
            let season_mod = entity.extra_f64_or(K::SEASON_DISEASE_MODIFIER, 1.0);
            chance *= season_mod;
        }

        // Immunity reduces chance
        chance *= 1.0 - info.plague_immunity;

        // Small settlements less likely
        if info.population < 100 {
            chance *= SMALL_SETTLEMENT_FACTOR;
        }

        let roll: f64 = ctx.rng.random_range(0.0..1.0);
        if roll < chance {
            targets.push(OutbreakTarget {
                settlement_id: info.id,
            });
        }
    }

    // Apply outbreaks
    for target in targets {
        start_outbreak(ctx, target.settlement_id, time, None);
    }
}

/// Start a plague outbreak in a settlement. If `caused_by_event` is Some, links the
/// new event causally (for disease spread).
fn start_outbreak(
    ctx: &mut TickContext,
    settlement_id: u64,
    time: SimTimestamp,
    caused_by_event: Option<u64>,
) -> Option<u64> {
    // Create the disease entity
    let disease_data = random_disease_data(ctx.rng);
    let disease_name = generate_disease_name(ctx.rng);

    let creation_ev =
        ctx.world
            .add_event(EventKind::Disaster, time, format!("{disease_name} emerges"));

    let disease_id = ctx.world.add_entity(
        EntityKind::Disease,
        disease_name.clone(),
        Some(time),
        crate::model::entity_data::EntityData::Disease(disease_data.clone()),
        creation_ev,
    );

    // Create the outbreak event
    let settlement_name = ctx
        .world
        .entities
        .get(&settlement_id)
        .map(|e| e.name.clone())
        .unwrap_or_default();

    let ev = if let Some(cause) = caused_by_event {
        ctx.world.add_caused_event(
            EventKind::Disaster,
            time,
            format!("{disease_name} breaks out in {settlement_name}"),
            cause,
        )
    } else {
        ctx.world.add_event(
            EventKind::Disaster,
            time,
            format!("{disease_name} breaks out in {settlement_name}"),
        )
    };

    if let Some(event) = ctx.world.events.get_mut(&ev) {
        event.data = serde_json::json!({
            "type": "plague_outbreak",
            "disease_id": disease_id,
            "virulence": disease_data.virulence,
            "lethality": disease_data.lethality,
        });
    }

    ctx.world
        .add_event_participant(ev, settlement_id, ParticipantRole::Location);
    ctx.world
        .add_event_participant(ev, disease_id, ParticipantRole::Subject);

    // Set active disease on settlement
    let initial_rate = disease_data.virulence * 0.1; // starts low
    if let Some(entity) = ctx.world.entities.get_mut(&settlement_id)
        && let Some(s) = entity.data.as_settlement_mut()
    {
        s.active_disease = Some(ActiveDisease {
            disease_id,
            started_year: time.year(),
            infection_rate: initial_rate,
            peak_reached: false,
            total_deaths: 0,
        });
    }
    // Clean up transient risk markers
    if let Some(entity) = ctx.world.entities.get_mut(&settlement_id) {
        entity.extra.remove("refugee_disease_risk");
        entity.extra.remove("post_conquest_disease_risk");
        entity.extra.remove("post_disaster_disease_risk");
    }

    // Emit signal
    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::PlagueStarted {
            settlement_id,
            disease_id,
        },
    });

    Some(disease_id)
}

fn spread_disease(
    ctx: &mut TickContext,
    settlements: &[SettlementDiseaseInfo],
    time: SimTimestamp,
) {
    // Collect spread targets first
    struct SpreadTarget {
        target_id: u64,
        disease_id: u64,
        source_id: u64,
    }

    let mut targets = Vec::new();

    for info in settlements {
        let Some(ref active) = info.active_disease else {
            continue;
        };

        // Get disease data
        let disease_data = ctx
            .world
            .entities
            .get(&active.disease_id)
            .and_then(|e| e.data.as_disease())
            .cloned();

        let Some(disease) = disease_data else {
            continue;
        };

        let base_spread = disease.virulence * active.infection_rate * BASE_TRANSMISSION;

        // Check trade route partners
        for &target_id in &info.trade_route_targets {
            // Skip if target already infected
            let target_info = settlements.iter().find(|s| s.id == target_id);
            if let Some(ti) = target_info {
                if ti.active_disease.is_some() {
                    continue;
                }
                let transmission =
                    (base_spread + TRADE_TRANSMISSION_BONUS) * (1.0 - ti.plague_immunity);
                let roll: f64 = ctx.rng.random_range(0.0..1.0);
                if roll < transmission {
                    targets.push(SpreadTarget {
                        target_id,
                        disease_id: active.disease_id,
                        source_id: info.id,
                    });
                }
            }
        }

        // Check adjacent settlements (slower spread)
        let adjacent = find_adjacent_settlements(ctx.world, info.region_id, info.id);
        for adj_id in adjacent {
            // Skip if already a trade route target (already checked above)
            if info.trade_route_targets.contains(&adj_id) {
                continue;
            }
            // Skip if already targeted for spread this tick
            if targets.iter().any(|t| t.target_id == adj_id) {
                continue;
            }

            let target_info = settlements.iter().find(|s| s.id == adj_id);
            if let Some(ti) = target_info {
                if ti.active_disease.is_some() {
                    continue;
                }
                let transmission = base_spread * ADJACENCY_ONLY_FACTOR * (1.0 - ti.plague_immunity);
                let roll: f64 = ctx.rng.random_range(0.0..1.0);
                if roll < transmission {
                    targets.push(SpreadTarget {
                        target_id: adj_id,
                        disease_id: active.disease_id,
                        source_id: info.id,
                    });
                }
            }
        }
    }

    // Apply spreads — infect target settlements with the same disease
    for spread in targets {
        let disease_data = ctx
            .world
            .entities
            .get(&spread.disease_id)
            .and_then(|e| e.data.as_disease())
            .cloned();
        let Some(disease) = disease_data else {
            continue;
        };

        let disease_name = ctx
            .world
            .entities
            .get(&spread.disease_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let source_name = ctx
            .world
            .entities
            .get(&spread.source_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let target_name = ctx
            .world
            .entities
            .get(&spread.target_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();

        let ev = ctx.world.add_event(
            EventKind::Disaster,
            time,
            format!("{disease_name} spreads from {source_name} to {target_name}"),
        );

        if let Some(event) = ctx.world.events.get_mut(&ev) {
            event.data = serde_json::json!({
                "type": "plague_spread",
                "disease_id": spread.disease_id,
                "from": spread.source_id,
            });
        }

        ctx.world
            .add_event_participant(ev, spread.target_id, ParticipantRole::Location);
        ctx.world
            .add_event_participant(ev, spread.source_id, ParticipantRole::Origin);
        ctx.world
            .add_event_participant(ev, spread.disease_id, ParticipantRole::Subject);

        // Set active disease on target
        let initial_rate = disease.virulence * 0.1;
        if let Some(entity) = ctx.world.entities.get_mut(&spread.target_id)
            && let Some(s) = entity.data.as_settlement_mut()
        {
            s.active_disease = Some(ActiveDisease {
                disease_id: spread.disease_id,
                started_year: time.year(),
                infection_rate: initial_rate,
                peak_reached: false,
                total_deaths: 0,
            });
        }

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::PlagueSpreading {
                settlement_id: spread.target_id,
                disease_id: spread.disease_id,
                from_settlement_id: spread.source_id,
            },
        });
    }
}

fn progress_and_mortality(
    ctx: &mut TickContext,
    settlements: &[SettlementDiseaseInfo],
    time: SimTimestamp,
    current_year: u32,
) {
    // Collect which settlements have active diseases
    struct InfectedInfo {
        settlement_id: u64,
        disease_id: u64,
        started_year: u32,
        infection_rate: f64,
        peak_reached: bool,
        total_deaths: u32,
    }

    let infected: Vec<InfectedInfo> = settlements
        .iter()
        .filter_map(|info| {
            let active = info.active_disease.as_ref()?;
            Some(InfectedInfo {
                settlement_id: info.id,
                disease_id: active.disease_id,
                started_year: active.started_year,
                infection_rate: active.infection_rate,
                peak_reached: active.peak_reached,
                total_deaths: active.total_deaths,
            })
        })
        .collect();

    for info in infected {
        let disease_data = ctx
            .world
            .entities
            .get(&info.disease_id)
            .and_then(|e| e.data.as_disease())
            .cloned();
        let Some(disease) = disease_data else {
            continue;
        };

        let years_active = current_year.saturating_sub(info.started_year);

        // Progress the infection rate
        let (new_rate, new_peak) = if !info.peak_reached {
            // Ramp phase
            let target = disease.virulence * RAMP_TARGET_FRACTION;
            let ramped = info.infection_rate + (target - info.infection_rate) * 0.6;
            let peak = ramped >= target * 0.95 || years_active >= 2;
            (ramped.min(target), peak)
        } else {
            // Decline phase
            let declined = info.infection_rate * (1.0 - DECLINE_RATE);
            (declined, true)
        };

        // Check if plague should end
        let should_end = new_rate < END_THRESHOLD || years_active >= disease.duration_years;

        if should_end {
            end_plague(
                ctx,
                info.settlement_id,
                info.disease_id,
                info.total_deaths,
                time,
            );
            continue;
        }

        // Apply mortality
        let mut mortality_rates = [0.0f64; NUM_BRACKETS];
        for (i, severity) in disease.bracket_severity.iter().enumerate() {
            mortality_rates[i] = new_rate * disease.lethality * severity;
        }

        let old_pop = ctx
            .world
            .entities
            .get(&info.settlement_id)
            .and_then(|e| e.data.as_settlement())
            .map(|s| s.population)
            .unwrap_or(0);

        let deaths = {
            let entity = ctx.world.entities.get_mut(&info.settlement_id).unwrap();
            let s = entity.data.as_settlement_mut().unwrap();
            let deaths = s
                .population_breakdown
                .apply_disease_mortality(&mortality_rates, ctx.rng);
            s.population = s.population_breakdown.total();
            deaths
        };

        let new_pop = ctx
            .world
            .entities
            .get(&info.settlement_id)
            .and_then(|e| e.data.as_settlement())
            .map(|s| s.population)
            .unwrap_or(0);

        // Update active disease state
        {
            let entity = ctx.world.entities.get_mut(&info.settlement_id).unwrap();
            if let Some(s) = entity.data.as_settlement_mut()
                && let Some(ref mut active) = s.active_disease
            {
                active.infection_rate = new_rate;
                active.peak_reached = new_peak;
                active.total_deaths += deaths;
            }
        }

        // Record population change if significant
        if deaths > 0 {
            let disease_name = ctx
                .world
                .entities
                .get(&info.disease_id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            let settlement_name = ctx
                .world
                .entities
                .get(&info.settlement_id)
                .map(|e| e.name.clone())
                .unwrap_or_default();

            let ev = ctx.world.add_event(
                EventKind::Disaster,
                time,
                format!(
                    "{disease_name} kills {deaths} in {settlement_name} (pop {old_pop} → {new_pop})"
                ),
            );
            if let Some(event) = ctx.world.events.get_mut(&ev) {
                event.data = serde_json::json!({
                    "type": "plague_mortality",
                    "disease_id": info.disease_id,
                    "deaths": deaths,
                });
            }
            ctx.world
                .add_event_participant(ev, info.settlement_id, ParticipantRole::Location);
            ctx.world.record_change(
                info.settlement_id,
                ev,
                "population",
                serde_json::json!(old_pop),
                serde_json::json!(new_pop),
            );

            if old_pop != new_pop {
                ctx.signals.push(Signal {
                    event_id: ev,
                    kind: SignalKind::PopulationChanged {
                        settlement_id: info.settlement_id,
                        old: old_pop,
                        new: new_pop,
                    },
                });
            }

            // NPC deaths
            kill_npcs_from_plague(
                ctx,
                info.settlement_id,
                &disease,
                new_rate,
                current_year,
                ev,
            );
        }
    }
}

fn end_plague(
    ctx: &mut TickContext,
    settlement_id: u64,
    disease_id: u64,
    total_deaths: u32,
    time: SimTimestamp,
) {
    let disease_name = ctx
        .world
        .entities
        .get(&disease_id)
        .map(|e| e.name.clone())
        .unwrap_or_default();
    let settlement_name = ctx
        .world
        .entities
        .get(&settlement_id)
        .map(|e| e.name.clone())
        .unwrap_or_default();

    let ev = ctx.world.add_event(
        EventKind::Disaster,
        time,
        format!("{disease_name} subsides in {settlement_name} after {total_deaths} deaths"),
    );
    if let Some(event) = ctx.world.events.get_mut(&ev) {
        event.data = serde_json::json!({
            "type": "plague_ended",
            "disease_id": disease_id,
            "total_deaths": total_deaths,
        });
    }
    ctx.world
        .add_event_participant(ev, settlement_id, ParticipantRole::Location);

    // Clear active disease and grant immunity
    if let Some(entity) = ctx.world.entities.get_mut(&settlement_id)
        && let Some(s) = entity.data.as_settlement_mut()
    {
        s.active_disease = None;
        s.plague_immunity = RECOVERY_IMMUNITY;
    }

    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::PlagueEnded {
            settlement_id,
            disease_id,
            deaths: total_deaths,
        },
    });
}

fn kill_npcs_from_plague(
    ctx: &mut TickContext,
    settlement_id: u64,
    disease: &DiseaseData,
    infection_rate: f64,
    current_year: u32,
    outbreak_event: u64,
) {
    // Find living NPCs in this settlement
    let npcs: Vec<(u64, u32)> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::LocatedIn
                        && r.target_entity_id == settlement_id
                        && r.end.is_none()
                })
        })
        .filter_map(|e| {
            let p = e.data.as_person()?;
            Some((e.id, p.birth_year))
        })
        .collect();

    let time = ctx.world.current_time;

    let mut deaths = Vec::new();
    for (npc_id, birth_year) in &npcs {
        let bracket = age_bracket(*birth_year, current_year);
        let death_chance = infection_rate
            * disease.lethality
            * disease.bracket_severity[bracket]
            * NPC_DEATH_MODIFIER;
        let roll: f64 = ctx.rng.random_range(0.0..1.0);
        if roll < death_chance {
            deaths.push(*npc_id);
        }
    }

    for npc_id in deaths {
        let npc_name = ctx
            .world
            .entities
            .get(&npc_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let disease_name = ctx
            .world
            .entities
            .get(&settlement_id)
            .and_then(|e| e.data.as_settlement())
            .and_then(|s| s.active_disease.as_ref())
            .and_then(|ad| ctx.world.entities.get(&ad.disease_id))
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "plague".to_string());

        let ev = ctx.world.add_caused_event(
            EventKind::Death,
            time,
            format!("{npc_name} died of {disease_name} in year {current_year}"),
            outbreak_event,
        );
        ctx.world
            .add_event_participant(ev, npc_id, ParticipantRole::Subject);

        // End the person's relationships
        let rels: Vec<(u64, RelationshipKind)> = ctx
            .world
            .entities
            .get(&npc_id)
            .map(|e| {
                e.relationships
                    .iter()
                    .filter(|r| {
                        r.end.is_none()
                            && matches!(
                                r.kind,
                                RelationshipKind::LocatedIn
                                    | RelationshipKind::MemberOf
                                    | RelationshipKind::Spouse
                            )
                    })
                    .map(|r| (r.target_entity_id, r.kind.clone()))
                    .collect()
            })
            .unwrap_or_default();

        for (target_id, kind) in rels {
            ctx.world
                .end_relationship(npc_id, target_id, kind, time, ev);
        }

        // End the person entity
        ctx.world.end_entity(npc_id, time, ev);

        // Emit death signal
        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::EntityDied { entity_id: npc_id },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::World;
    use crate::scenario::Scenario;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn ts(year: u32) -> SimTimestamp {
        SimTimestamp::from_year(year)
    }

    /// Minimal disease test world: one region, one faction, one settlement with given population.
    fn disease_scenario(pop: u32) -> (World, u64) {
        let mut s = Scenario::new();
        let setup = s.add_settlement_standalone("TestTown");
        s.settlement_mut(setup.settlement).population(pop);
        (s.build(), setup.settlement)
    }

    #[test]
    fn disease_name_generation() {
        let mut rng = SmallRng::seed_from_u64(42);
        let name = generate_disease_name(&mut rng);
        assert!(name.starts_with("The "));
        assert!(name.len() > 5);
    }

    #[test]
    fn random_disease_has_valid_ranges() {
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..100 {
            let d = random_disease_data(&mut rng);
            assert!((0.3..=0.8).contains(&d.virulence));
            assert!((0.1..=0.5).contains(&d.lethality));
            assert!((2..=5).contains(&d.duration_years));
            assert_eq!(d.bracket_severity.len(), NUM_BRACKETS);
        }
    }

    #[test]
    fn scenario_start_outbreak_creates_disease_entity() {
        let (mut world, settlement) = disease_scenario(500);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let time = ts(10);
        world.current_time = time;

        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        let disease_id = start_outbreak(&mut ctx, settlement, time, None);
        assert!(disease_id.is_some());

        // Check disease entity exists
        let disease = ctx.world.entities.get(&disease_id.unwrap()).unwrap();
        assert_eq!(disease.kind, EntityKind::Disease);
        assert!(disease.data.as_disease().is_some());

        // Check settlement has active disease
        let s = ctx
            .world
            .entities
            .get(&settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap();
        assert!(s.active_disease.is_some());
        assert_eq!(
            s.active_disease.as_ref().unwrap().disease_id,
            disease_id.unwrap()
        );

        // Check signal emitted
        assert!(signals.iter().any(|s| matches!(
            &s.kind,
            SignalKind::PlagueStarted {
                settlement_id,
                ..
            } if *settlement_id == settlement
        )));
    }

    #[test]
    fn scenario_immunity_prevents_outbreak() {
        let (mut world, settlement) = disease_scenario(500);
        // Set high immunity
        {
            let entity = world.entities.get_mut(&settlement).unwrap();
            entity.data.as_settlement_mut().unwrap().plague_immunity = 0.99;
        }

        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        world.current_time = ts(10);

        // Run many outbreak checks — immunity should prevent nearly all
        let mut outbreaks = 0;
        for _ in 0..1000 {
            let settlements = collect_settlement_info(&world);
            let mut ctx = TickContext {
                world: &mut world,
                rng: &mut rng,
                signals: &mut signals,
                inbox: &[],
            };
            check_outbreaks(&mut ctx, &settlements, ts(10));
            if world
                .entities
                .get(&settlement)
                .unwrap()
                .data
                .as_settlement()
                .unwrap()
                .active_disease
                .is_some()
            {
                outbreaks += 1;
                // Reset for next iteration
                world
                    .entities
                    .get_mut(&settlement)
                    .unwrap()
                    .data
                    .as_settlement_mut()
                    .unwrap()
                    .active_disease = None;
            }
        }
        // With 0.99 immunity, chance is ~0.002 * 0.01 = 0.00002 per check
        // In 1000 checks, expect ~0.02 outbreaks — should be very rare
        assert!(
            outbreaks < 5,
            "Expected very few outbreaks with high immunity, got {outbreaks}"
        );
    }

    #[test]
    fn scenario_disease_mortality_reduces_population() {
        let (mut world, settlement) = disease_scenario(1000);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let time = ts(10);
        world.current_time = time;

        // Manually infect
        {
            let mut ctx = TickContext {
                world: &mut world,
                rng: &mut rng,
                signals: &mut signals,
                inbox: &[],
            };
            start_outbreak(&mut ctx, settlement, time, None);
        }

        // Set high infection rate manually for testing
        {
            let s = world
                .entities
                .get_mut(&settlement)
                .unwrap()
                .data
                .as_settlement_mut()
                .unwrap();
            s.active_disease.as_mut().unwrap().infection_rate = 0.5;
            s.active_disease.as_mut().unwrap().peak_reached = true;
        }

        let pop_before = world
            .entities
            .get(&settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .population;

        let settlements = collect_settlement_info(&world);
        signals.clear();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };
        progress_and_mortality(&mut ctx, &settlements, time, 10);

        let pop_after = ctx
            .world
            .entities
            .get(&settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .population;

        assert!(
            pop_after < pop_before,
            "Population should decrease from plague: {pop_before} → {pop_after}"
        );
    }

    #[test]
    fn scenario_disease_lifecycle_ramp_peak_decline_end() {
        let (mut world, settlement) = disease_scenario(2000);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        world.current_time = ts(10);

        // Start outbreak
        {
            let mut ctx = TickContext {
                world: &mut world,
                rng: &mut rng,
                signals: &mut signals,
                inbox: &[],
            };
            start_outbreak(&mut ctx, settlement, ts(10), None);
        }

        // Track infection rate over years
        let mut rates = Vec::new();
        let initial_rate = world
            .entities
            .get(&settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .active_disease
            .as_ref()
            .unwrap()
            .infection_rate;
        rates.push(initial_rate);

        for year in 11..25 {
            world.current_time = ts(year);
            signals.clear();
            let settlements = collect_settlement_info(&world);
            let mut ctx = TickContext {
                world: &mut world,
                rng: &mut rng,
                signals: &mut signals,
                inbox: &[],
            };
            progress_and_mortality(&mut ctx, &settlements, ts(year), year);

            let active = ctx
                .world
                .entities
                .get(&settlement)
                .unwrap()
                .data
                .as_settlement()
                .unwrap()
                .active_disease
                .clone();
            if let Some(ad) = active {
                rates.push(ad.infection_rate);
            } else {
                // Disease ended
                break;
            }
        }

        // Should have ramped up then declined
        assert!(rates.len() >= 3, "Disease should last at least a few years");
        // Rate should have increased from initial
        assert!(
            rates.iter().copied().fold(0.0f64, f64::max) > initial_rate,
            "Infection rate should increase during ramp"
        );

        // Disease should have ended (active_disease cleared)
        let final_state = world
            .entities
            .get(&settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .active_disease
            .clone();
        // It's fine if it hasn't ended yet in 15 years for some disease profiles,
        // but the rate should be declining by now
        if final_state.is_some() {
            let last_rate = *rates.last().unwrap();
            let peak_rate = rates.iter().copied().fold(0.0f64, f64::max);
            assert!(
                last_rate < peak_rate,
                "Rate should be declining: peak={peak_rate:.4} last={last_rate:.4}"
            );
        }
    }

    #[test]
    fn scenario_immunity_decays_over_time() {
        let (mut world, settlement) = disease_scenario(500);
        {
            let s = world
                .entities
                .get_mut(&settlement)
                .unwrap()
                .data
                .as_settlement_mut()
                .unwrap();
            s.plague_immunity = RECOVERY_IMMUNITY;
        }

        let settlements = collect_settlement_info(&world);
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        decay_immunity(&mut ctx, &settlements, ts(10));

        let immunity = ctx
            .world
            .entities
            .get(&settlement)
            .unwrap()
            .data
            .as_settlement()
            .unwrap()
            .plague_immunity;

        assert!(
            (immunity - (RECOVERY_IMMUNITY - IMMUNITY_DECAY)).abs() < 0.001,
            "Immunity should decay by {IMMUNITY_DECAY}: expected {} got {immunity}",
            RECOVERY_IMMUNITY - IMMUNITY_DECAY
        );
    }

    #[test]
    fn age_bracket_mapping() {
        // infant: 0-5 (bracket 0)
        assert_eq!(age_bracket(100, 100), 0); // age 0
        assert_eq!(age_bracket(100, 105), 0); // age 5
        // child: 6-15 (bracket 1)
        assert_eq!(age_bracket(100, 106), 1); // age 6
        assert_eq!(age_bracket(100, 115), 1); // age 15
        // young_adult: 16-40 (bracket 2)
        assert_eq!(age_bracket(100, 116), 2); // age 16
        assert_eq!(age_bracket(100, 140), 2); // age 40
        // middle_age: 41-60 (bracket 3)
        assert_eq!(age_bracket(100, 141), 3); // age 41
        // elder: 61-75 (bracket 4)
        assert_eq!(age_bracket(100, 161), 4); // age 61
    }
}
