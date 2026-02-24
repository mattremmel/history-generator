use rand::Rng;

use super::context::TickContext;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::entity_data::{ActiveDisaster, DisasterType};
use crate::model::{EntityData, EntityKind, EventKind, RelationshipKind, SimTimestamp};

// ---------------------------------------------------------------------------
// Season
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn from_month(month: u32) -> Self {
        match month {
            1..=3 => Season::Spring,
            4..=6 => Season::Summer,
            7..=9 => Season::Autumn,
            10..=12 => Season::Winter,
            _ => Season::Spring,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Season::Spring => "spring",
            Season::Summer => "summer",
            Season::Autumn => "autumn",
            Season::Winter => "winter",
        }
    }
}

// ---------------------------------------------------------------------------
// Climate zone (derived from y-coordinate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClimateZone {
    Tropical,
    Temperate,
    Boreal,
}

/// Map y-coordinate (0–1000) to climate zone.
/// Low y = tropical, mid = temperate, high y = boreal.
fn climate_zone_from_y(y: f64) -> ClimateZone {
    if y < 300.0 {
        ClimateZone::Tropical
    } else if y < 700.0 {
        ClimateZone::Temperate
    } else {
        ClimateZone::Boreal
    }
}

// ---------------------------------------------------------------------------
// Seasonal modifiers
// ---------------------------------------------------------------------------

struct SeasonalModifiers {
    food: f64,
    trade: f64,
    construction_blocked: bool,
    disease: f64,
    army: f64,
}

fn compute_modifiers(season: Season, climate: ClimateZone, terrain: &str) -> SeasonalModifiers {
    let (base_food, base_trade, base_disease, base_army) = match (season, climate) {
        // -- Tropical: mild seasons, muted variation --
        (Season::Spring, ClimateZone::Tropical) => (0.9, 1.0, 0.9, 1.0),
        (Season::Summer, ClimateZone::Tropical) => (1.0, 1.1, 1.3, 0.9),
        (Season::Autumn, ClimateZone::Tropical) => (1.1, 1.0, 1.0, 1.0),
        (Season::Winter, ClimateZone::Tropical) => (0.9, 1.0, 0.8, 1.0),

        // -- Temperate: clear seasonal cycle --
        (Season::Spring, ClimateZone::Temperate) => (0.8, 1.0, 0.8, 1.0),
        (Season::Summer, ClimateZone::Temperate) => (1.0, 1.1, 1.2, 0.9),
        (Season::Autumn, ClimateZone::Temperate) => (1.3, 1.0, 0.9, 1.0),
        (Season::Winter, ClimateZone::Temperate) => (0.4, 0.6, 0.7, 0.6),

        // -- Boreal: harsh winters --
        (Season::Spring, ClimateZone::Boreal) => (0.6, 0.8, 0.7, 0.8),
        (Season::Summer, ClimateZone::Boreal) => (1.0, 1.0, 1.0, 1.0),
        (Season::Autumn, ClimateZone::Boreal) => (1.2, 0.9, 0.8, 0.9),
        (Season::Winter, ClimateZone::Boreal) => (0.2, 0.3, 0.6, 0.4),
    };

    // Terrain adjustments
    let terrain_food_mult = match terrain {
        "desert" => 0.7,
        "tundra" => 0.6,
        "swamp" => 0.8,
        _ => 1.0,
    };
    let terrain_trade_mult = match terrain {
        "mountains" if season == Season::Winter => 0.5,
        "mountains" => 0.8,
        "swamp" if season == Season::Spring => 0.6, // spring flooding
        _ => 1.0,
    };
    let terrain_disease_mult = match terrain {
        "swamp" | "jungle" => 1.3,
        "tundra" | "desert" => 0.7,
        _ => 1.0,
    };

    let construction_blocked = match (season, climate) {
        (Season::Winter, ClimateZone::Boreal) => true,
        (Season::Winter, ClimateZone::Temperate)
            if terrain == "mountains" || terrain == "tundra" =>
        {
            true
        }
        _ => false,
    };

    SeasonalModifiers {
        food: base_food * terrain_food_mult,
        trade: base_trade * terrain_trade_mult,
        construction_blocked,
        disease: base_disease * terrain_disease_mult,
        army: base_army,
    }
}

// ---------------------------------------------------------------------------
// Settlement info gathered before mutation
// ---------------------------------------------------------------------------

struct SettlementInfo {
    id: u64,
    region_id: u64,
    terrain: String,
    terrain_tags: Vec<String>,
    region_y: f64,
    population: u32,
    has_active_disaster: bool,
}

fn gather_settlement_info(world: &crate::model::World) -> Vec<SettlementInfo> {
    let mut infos = Vec::new();

    for entity in world.entities.values() {
        if entity.kind != EntityKind::Settlement || entity.end.is_some() {
            continue;
        }
        let sd = match entity.data.as_settlement() {
            Some(sd) => sd,
            None => continue,
        };

        // Find region via LocatedIn relationship
        let region_id = entity
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::LocatedIn && r.end.is_none())
            .map(|r| r.target_entity_id);

        let (terrain, terrain_tags, region_y) = if let Some(rid) = region_id {
            if let Some(region) = world.entities.get(&rid) {
                if let Some(rd) = region.data.as_region() {
                    (rd.terrain.clone(), rd.terrain_tags.clone(), rd.y)
                } else {
                    ("plains".to_string(), vec![], 500.0)
                }
            } else {
                ("plains".to_string(), vec![], 500.0)
            }
        } else {
            ("plains".to_string(), vec![], 500.0)
        };

        infos.push(SettlementInfo {
            id: entity.id,
            region_id: region_id.unwrap_or(0),
            terrain,
            terrain_tags,
            region_y,
            population: sd.population,
            has_active_disaster: sd.active_disaster.is_some(),
        });
    }
    infos
}

// ---------------------------------------------------------------------------
// EnvironmentSystem
// ---------------------------------------------------------------------------

pub struct EnvironmentSystem;

impl SimSystem for EnvironmentSystem {
    fn name(&self) -> &str {
        "environment"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Monthly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let month = time.month();
        let season = Season::from_month(month);

        let tick_event = ctx.world.add_event(
            EventKind::Custom("environment_tick".to_string()),
            time,
            format!(
                "Environmental conditions, {} Y{}",
                season.as_str(),
                time.year()
            ),
        );

        let infos = gather_settlement_info(ctx.world);

        // Phase 1: Compute and store seasonal modifiers
        for info in &infos {
            let climate = climate_zone_from_y(info.region_y);
            let mods = compute_modifiers(season, climate, &info.terrain);

            ctx.world.set_extra(
                info.id,
                "season_food_modifier".to_string(),
                serde_json::json!(mods.food),
                tick_event,
            );
            ctx.world.set_extra(
                info.id,
                "season_trade_modifier".to_string(),
                serde_json::json!(mods.trade),
                tick_event,
            );
            ctx.world.set_extra(
                info.id,
                "season_construction_blocked".to_string(),
                serde_json::json!(mods.construction_blocked),
                tick_event,
            );
            ctx.world.set_extra(
                info.id,
                "season_disease_modifier".to_string(),
                serde_json::json!(mods.disease),
                tick_event,
            );
            ctx.world.set_extra(
                info.id,
                "season_army_modifier".to_string(),
                serde_json::json!(mods.army),
                tick_event,
            );
        }

        // Also compute construction_months at year start for yearly systems
        if month == 1 {
            for info in &infos {
                let climate = climate_zone_from_y(info.region_y);
                let construction_months: u32 = (1..=12)
                    .filter(|&m| {
                        let s = Season::from_month(m);
                        !compute_modifiers(s, climate, &info.terrain).construction_blocked
                    })
                    .count() as u32;
                ctx.world.set_extra(
                    info.id,
                    "season_construction_months".to_string(),
                    serde_json::json!(construction_months),
                    tick_event,
                );

                // Annual food modifier average for yearly systems
                let annual_food: f64 = (1..=12)
                    .map(|m| {
                        let s = Season::from_month(m);
                        compute_modifiers(s, climate, &info.terrain).food
                    })
                    .sum::<f64>()
                    / 12.0;
                ctx.world.set_extra(
                    info.id,
                    "season_food_modifier_annual".to_string(),
                    serde_json::json!(annual_food),
                    tick_event,
                );
            }
        }

        // Phase 2: Check for new natural disasters
        check_instant_disasters(ctx, &infos, season, time);
        check_persistent_disasters(ctx, &infos, season, time);

        // Phase 3: Progress active persistent disasters
        progress_active_disasters(ctx, time, tick_event);
    }

    fn handle_signals(&mut self, _ctx: &mut TickContext) {
        // EnvironmentSystem doesn't react to signals from other systems.
    }
}

// ---------------------------------------------------------------------------
// Instant disasters
// ---------------------------------------------------------------------------

/// Terrain multiplier for a given disaster type.
fn instant_disaster_terrain_mult(disaster: &DisasterType, terrain: &str) -> f64 {
    match disaster {
        DisasterType::Earthquake => match terrain {
            "volcanic" => 5.0,
            "mountains" => 3.0,
            "hills" => 1.5,
            _ => 0.3,
        },
        DisasterType::VolcanicEruption => match terrain {
            "volcanic" => 1.0,
            _ => 0.0, // volcanic terrain only
        },
        DisasterType::Storm => match terrain {
            "coast" => 3.0,
            "plains" => 1.5,
            _ => 0.5,
        },
        DisasterType::Tsunami => match terrain {
            "coast" => 1.0,
            _ => 0.0, // coast only
        },
        _ => 1.0, // persistent disasters handled separately
    }
}

/// Terrain tag multiplier for a given disaster type.
fn instant_disaster_tag_mult(disaster: &DisasterType, tags: &[String]) -> f64 {
    let mut mult = 1.0;
    for tag in tags {
        mult *= match (disaster, tag.as_str()) {
            (DisasterType::Earthquake, "rugged") => 1.5,
            (DisasterType::Storm, "coastal") => 2.0,
            (DisasterType::Tsunami, "coastal") => 1.5,
            _ => 1.0,
        };
    }
    mult
}

fn season_mult_instant(disaster: &DisasterType, season: Season) -> f64 {
    match (disaster, season) {
        (DisasterType::Storm, Season::Summer | Season::Winter) => 2.0,
        _ => 1.0,
    }
}

struct InstantDisasterDef {
    disaster_type: DisasterType,
    base_monthly_prob: f64,
    pop_loss_range: (f64, f64),
    building_damage_range: (f64, f64),
    prosperity_hit: f64,
    sever_trade: bool,
}

const INSTANT_DISASTERS: &[InstantDisasterDef] = &[
    InstantDisasterDef {
        disaster_type: DisasterType::Earthquake,
        base_monthly_prob: 0.0005,
        pop_loss_range: (0.02, 0.08),
        building_damage_range: (0.2, 0.6),
        prosperity_hit: 0.15,
        sever_trade: true,
    },
    InstantDisasterDef {
        disaster_type: DisasterType::VolcanicEruption,
        base_monthly_prob: 0.0002,
        pop_loss_range: (0.05, 0.20),
        building_damage_range: (0.3, 0.8),
        prosperity_hit: 0.30,
        sever_trade: true,
    },
    InstantDisasterDef {
        disaster_type: DisasterType::Storm,
        base_monthly_prob: 0.001,
        pop_loss_range: (0.01, 0.03),
        building_damage_range: (0.1, 0.3),
        prosperity_hit: 0.05,
        sever_trade: false,
    },
    InstantDisasterDef {
        disaster_type: DisasterType::Tsunami,
        base_monthly_prob: 0.0002,
        pop_loss_range: (0.03, 0.10),
        building_damage_range: (0.3, 0.7),
        prosperity_hit: 0.20,
        sever_trade: true,
    },
];

fn check_instant_disasters(
    ctx: &mut TickContext,
    infos: &[SettlementInfo],
    season: Season,
    time: SimTimestamp,
) {
    // Collect candidates: (settlement_index, disaster_def_index, effective_prob)
    let mut candidates: Vec<(usize, usize, f64)> = Vec::new();
    for (si, info) in infos.iter().enumerate() {
        if info.has_active_disaster || info.population < 10 {
            continue;
        }
        for (di, def) in INSTANT_DISASTERS.iter().enumerate() {
            let terrain_m = instant_disaster_terrain_mult(&def.disaster_type, &info.terrain);
            if terrain_m == 0.0 {
                continue;
            }
            let tag_m = instant_disaster_tag_mult(&def.disaster_type, &info.terrain_tags);
            let season_m = season_mult_instant(&def.disaster_type, season);
            let prob = def.base_monthly_prob * terrain_m * tag_m * season_m;
            candidates.push((si, di, prob));
        }
    }

    // Roll for each candidate
    let rolls: Vec<(usize, usize, f64)> = candidates
        .iter()
        .map(|&(si, di, prob)| {
            let roll: f64 = ctx.rng.random();
            (si, di, if roll < prob { prob } else { -1.0 })
        })
        .filter(|&(_, _, p)| p >= 0.0)
        .collect();

    for (si, di, _) in rolls {
        let info = &infos[si];
        let def = &INSTANT_DISASTERS[di];
        apply_instant_disaster(ctx, info, def, time);
    }
}

fn apply_instant_disaster(
    ctx: &mut TickContext,
    info: &SettlementInfo,
    def: &InstantDisasterDef,
    time: SimTimestamp,
) {
    let severity: f64 = ctx.rng.random();

    // Create disaster event
    let disaster_event = ctx.world.add_event(
        EventKind::Custom(format!("disaster_{}", def.disaster_type.as_str())),
        time,
        format!(
            "{} strikes settlement (severity {:.0}%)",
            def.disaster_type.as_str(),
            severity * 100.0
        ),
    );

    // Link to tick event
    ctx.world
        .event_participants
        .push(crate::model::EventParticipant {
            event_id: disaster_event,
            entity_id: info.id,
            role: crate::model::ParticipantRole::Object,
        });

    // Population loss
    let loss_frac = def.pop_loss_range.0 + severity * (def.pop_loss_range.1 - def.pop_loss_range.0);
    let mut old_pop = 0u32;
    let mut new_pop = 0u32;
    if let Some(entity) = ctx.world.entities.get_mut(&info.id)
        && let Some(sd) = entity.data.as_settlement_mut()
    {
        let deaths = (sd.population as f64 * loss_frac) as u32;
        old_pop = sd.population;
        sd.population = sd.population.saturating_sub(deaths);
        new_pop = sd.population;
        sd.population_breakdown.scale_to(sd.population);

        // Prosperity hit
        sd.prosperity = (sd.prosperity - def.prosperity_hit * severity).max(0.0);
    }
    if old_pop != new_pop {
        ctx.world.record_change(
            info.id,
            disaster_event,
            "population",
            serde_json::json!(old_pop),
            serde_json::json!(new_pop),
        );
    }

    // Building damage
    let damage = def.building_damage_range.0
        + severity * (def.building_damage_range.1 - def.building_damage_range.0);
    damage_settlement_buildings(
        ctx,
        info.id,
        damage,
        time,
        disaster_event,
        &def.disaster_type,
    );

    // Sever trade routes
    if def.sever_trade {
        sever_settlement_trade_routes(ctx, info.id, time, disaster_event);
    }

    // Create geographic feature for severe volcanic eruptions/earthquakes
    if severity > 0.7
        && matches!(
            def.disaster_type,
            DisasterType::VolcanicEruption | DisasterType::Earthquake
        )
    {
        let feature_type = match def.disaster_type {
            DisasterType::VolcanicEruption => "lava_field",
            DisasterType::Earthquake => "fault_line",
            _ => "crater",
        };
        let feature_id = ctx.world.add_entity(
            EntityKind::GeographicFeature,
            format!("{feature_type} near settlement"),
            Some(time),
            EntityData::GeographicFeature(crate::model::entity_data::GeographicFeatureData {
                feature_type: feature_type.to_string(),
                x: 0.0,
                y: 0.0,
            }),
            disaster_event,
        );
        if info.region_id != 0 {
            ctx.world.add_relationship(
                feature_id,
                info.region_id,
                RelationshipKind::LocatedIn,
                time,
                disaster_event,
            );
        }
    }

    // Emit signal
    ctx.signals.push(Signal {
        event_id: disaster_event,
        kind: SignalKind::DisasterStruck {
            settlement_id: info.id,
            region_id: info.region_id,
            disaster_type: def.disaster_type.clone(),
            severity,
        },
    });
}

// ---------------------------------------------------------------------------
// Persistent disasters
// ---------------------------------------------------------------------------

struct PersistentDisasterDef {
    disaster_type: DisasterType,
    base_monthly_prob: f64,
    terrain_gates: &'static [(&'static str, f64)],
    tag_gates: &'static [(&'static str, f64)],
    season_gates: &'static [(Season, f64)],
    duration_range: (u32, u32),
}

const PERSISTENT_DISASTERS: &[PersistentDisasterDef] = &[
    PersistentDisasterDef {
        disaster_type: DisasterType::Drought,
        base_monthly_prob: 0.0008,
        terrain_gates: &[("desert", 3.0), ("plains", 1.5)],
        tag_gates: &[("arid", 3.0), ("fertile", 0.5)],
        season_gates: &[(Season::Summer, 4.0)],
        duration_range: (3, 12),
    },
    PersistentDisasterDef {
        disaster_type: DisasterType::Flood,
        base_monthly_prob: 0.001,
        terrain_gates: &[("swamp", 2.0), ("coast", 2.0)],
        tag_gates: &[("riverine", 3.0), ("coastal", 2.0)],
        season_gates: &[(Season::Spring, 3.0), (Season::Summer, 1.5)],
        duration_range: (1, 4),
    },
    PersistentDisasterDef {
        disaster_type: DisasterType::Wildfire,
        base_monthly_prob: 0.0006,
        terrain_gates: &[("forest", 3.0), ("jungle", 2.0), ("plains", 1.5)],
        tag_gates: &[("forested", 2.0)],
        season_gates: &[(Season::Summer, 3.0), (Season::Autumn, 2.0)],
        duration_range: (1, 3),
    },
];

fn check_persistent_disasters(
    ctx: &mut TickContext,
    infos: &[SettlementInfo],
    season: Season,
    time: SimTimestamp,
) {
    let mut candidates: Vec<(usize, usize, f64)> = Vec::new();
    for (si, info) in infos.iter().enumerate() {
        if info.has_active_disaster || info.population < 10 {
            continue;
        }
        for (di, def) in PERSISTENT_DISASTERS.iter().enumerate() {
            let terrain_m = def
                .terrain_gates
                .iter()
                .find(|(t, _)| *t == info.terrain)
                .map(|(_, m)| *m)
                .unwrap_or(0.3);
            let tag_m: f64 = def
                .tag_gates
                .iter()
                .map(|(tag, mult)| {
                    if info.terrain_tags.iter().any(|t| t == tag) {
                        *mult
                    } else {
                        1.0
                    }
                })
                .product();
            let season_m = def
                .season_gates
                .iter()
                .find(|(s, _)| *s == season)
                .map(|(_, m)| *m)
                .unwrap_or(1.0);
            let prob = def.base_monthly_prob * terrain_m * tag_m * season_m;
            candidates.push((si, di, prob));
        }
    }

    let rolls: Vec<(usize, usize)> = candidates
        .iter()
        .filter_map(|&(si, di, prob)| {
            let roll: f64 = ctx.rng.random();
            if roll < prob { Some((si, di)) } else { None }
        })
        .collect();

    for (si, di) in rolls {
        let info = &infos[si];
        let def = &PERSISTENT_DISASTERS[di];
        let duration = ctx
            .rng
            .random_range(def.duration_range.0..=def.duration_range.1);
        let severity: f64 = ctx.rng.random_range(0.3..1.0);

        let disaster_event = ctx.world.add_event(
            EventKind::Custom(format!("disaster_{}_start", def.disaster_type.as_str())),
            time,
            format!(
                "{} begins in settlement (severity {:.0}%, est. {} months)",
                def.disaster_type.as_str(),
                severity * 100.0,
                duration
            ),
        );

        ctx.world
            .event_participants
            .push(crate::model::EventParticipant {
                event_id: disaster_event,
                entity_id: info.id,
                role: crate::model::ParticipantRole::Object,
            });

        if let Some(entity) = ctx.world.entities.get_mut(&info.id)
            && let Some(sd) = entity.data.as_settlement_mut()
        {
            sd.active_disaster = Some(ActiveDisaster {
                disaster_type: def.disaster_type.clone(),
                severity,
                started_year: time.year(),
                started_month: time.month(),
                months_remaining: duration,
                total_deaths: 0,
            });
        }

        ctx.signals.push(Signal {
            event_id: disaster_event,
            kind: SignalKind::DisasterStarted {
                settlement_id: info.id,
                disaster_type: def.disaster_type.clone(),
                severity,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Progress active persistent disasters
// ---------------------------------------------------------------------------

fn progress_active_disasters(ctx: &mut TickContext, time: SimTimestamp, tick_event: u64) {
    // Collect settlements with active disasters
    let active: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter(|e| {
            e.data
                .as_settlement()
                .is_some_and(|sd| sd.active_disaster.is_some())
        })
        .map(|e| e.id)
        .collect();

    for sid in active {
        // Extract disaster info
        let (disaster_type, severity, months_remaining, population) = {
            let entity = match ctx.world.entities.get(&sid) {
                Some(e) => e,
                None => continue,
            };
            let sd = match entity.data.as_settlement() {
                Some(sd) => sd,
                None => continue,
            };
            let ad = match &sd.active_disaster {
                Some(ad) => ad,
                None => continue,
            };
            (
                ad.disaster_type.clone(),
                ad.severity,
                ad.months_remaining,
                sd.population,
            )
        };

        if months_remaining == 0 {
            // Should not happen for persistent, but clear it
            end_disaster(ctx, sid, time);
            continue;
        }

        // Apply monthly effects
        let (pop_loss_frac, building_damage) = match disaster_type {
            DisasterType::Drought => (0.005 + severity * 0.015, 0.0),
            DisasterType::Flood => (0.01 + severity * 0.02, 0.1),
            DisasterType::Wildfire => (0.02 + severity * 0.03, 0.2 * severity),
            _ => (0.0, 0.0),
        };

        let deaths = (population as f64 * pop_loss_frac) as u32;

        // Apply damage
        if let Some(entity) = ctx.world.entities.get_mut(&sid)
            && let Some(sd) = entity.data.as_settlement_mut()
        {
            sd.population = sd.population.saturating_sub(deaths);
            sd.population_breakdown.scale_to(sd.population);

            // Prosperity erosion
            let prosperity_hit = match disaster_type {
                DisasterType::Drought => 0.02 * severity,
                DisasterType::Flood => 0.03 * severity,
                DisasterType::Wildfire => 0.03 * severity,
                _ => 0.0,
            };
            sd.prosperity = (sd.prosperity - prosperity_hit).max(0.0);

            // Update disaster state
            if let Some(ad) = &mut sd.active_disaster {
                ad.months_remaining = ad.months_remaining.saturating_sub(1);
                ad.total_deaths += deaths;
            }
        }

        // Override food modifier for drought
        if disaster_type == DisasterType::Drought {
            ctx.world.set_extra(
                sid,
                "season_food_modifier".to_string(),
                serde_json::json!(0.2),
                tick_event,
            );
        }

        // Building damage for flood/wildfire
        if building_damage > 0.0 {
            damage_settlement_buildings(
                ctx,
                sid,
                building_damage,
                time,
                tick_event,
                &disaster_type,
            );
        }

        // Check if disaster ended
        let ended = {
            ctx.world
                .entities
                .get(&sid)
                .and_then(|e| e.data.as_settlement())
                .and_then(|sd| sd.active_disaster.as_ref())
                .is_some_and(|ad| ad.months_remaining == 0)
        };

        if ended {
            end_disaster(ctx, sid, time);
        }
    }
}

fn end_disaster(ctx: &mut TickContext, settlement_id: u64, time: SimTimestamp) {
    let (disaster_type, total_deaths, started_year, started_month) = {
        let entity = match ctx.world.entities.get(&settlement_id) {
            Some(e) => e,
            None => return,
        };
        let sd = match entity.data.as_settlement() {
            Some(sd) => sd,
            None => return,
        };
        let ad = match &sd.active_disaster {
            Some(ad) => ad,
            None => return,
        };
        (
            ad.disaster_type.clone(),
            ad.total_deaths,
            ad.started_year,
            ad.started_month,
        )
    };

    let months_duration =
        (time.year() * 12 + time.month()).saturating_sub(started_year * 12 + started_month);

    let end_event = ctx.world.add_event(
        EventKind::Custom(format!("disaster_{}_end", disaster_type.as_str())),
        time,
        format!(
            "{} ends after {} months ({} deaths)",
            disaster_type.as_str(),
            months_duration,
            total_deaths
        ),
    );

    ctx.world
        .event_participants
        .push(crate::model::EventParticipant {
            event_id: end_event,
            entity_id: settlement_id,
            role: crate::model::ParticipantRole::Object,
        });

    // Clear disaster
    if let Some(entity) = ctx.world.entities.get_mut(&settlement_id)
        && let Some(sd) = entity.data.as_settlement_mut()
    {
        sd.active_disaster = None;
    }

    ctx.signals.push(Signal {
        event_id: end_event,
        kind: SignalKind::DisasterEnded {
            settlement_id,
            disaster_type: disaster_type.clone(),
            total_deaths,
            months_duration,
        },
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Damage all buildings in a settlement by reducing condition.
fn damage_settlement_buildings(
    ctx: &mut TickContext,
    settlement_id: u64,
    damage: f64,
    time: SimTimestamp,
    event_id: u64,
    disaster_type: &DisasterType,
) {
    // Find buildings located in this settlement
    let building_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Building && e.end.is_none())
        .filter(|e| {
            e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::LocatedIn
                    && r.target_entity_id == settlement_id
                    && r.end.is_none()
            })
        })
        .map(|e| e.id)
        .collect();

    // Filter building types based on disaster
    let affects_building = |bt: &str| -> bool {
        match disaster_type {
            DisasterType::Storm => bt == "port" || bt == "market",
            DisasterType::Flood => bt == "granary" || bt == "workshop" || bt == "mine",
            DisasterType::Wildfire => bt == "workshop" || bt == "granary" || bt == "market",
            _ => true, // earthquake, volcanic, tsunami affect all
        }
    };

    for bid in building_ids {
        let should_damage = ctx
            .world
            .entities
            .get(&bid)
            .and_then(|e| e.data.as_building())
            .is_some_and(|bd| affects_building(bd.building_type.as_str()));

        if !should_damage {
            continue;
        }

        let destroyed = {
            if let Some(entity) = ctx.world.entities.get_mut(&bid) {
                if let Some(bd) = entity.data.as_building_mut() {
                    bd.condition = (bd.condition - damage).max(0.0);
                    bd.condition <= 0.0
                } else {
                    false
                }
            } else {
                false
            }
        };

        if destroyed {
            let building_type = ctx
                .world
                .entities
                .get(&bid)
                .and_then(|e| e.data.as_building())
                .map(|bd| bd.building_type.clone());
            let Some(building_type) = building_type else {
                continue;
            };

            ctx.world.end_entity(bid, time, event_id);

            ctx.signals.push(Signal {
                event_id,
                kind: SignalKind::BuildingDestroyed {
                    building_id: bid,
                    settlement_id,
                    building_type,
                    cause: disaster_type.as_str().to_string(),
                },
            });
        }
    }
}

/// Sever trade routes involving this settlement.
fn sever_settlement_trade_routes(
    ctx: &mut TickContext,
    settlement_id: u64,
    time: SimTimestamp,
    event_id: u64,
) {
    let routes: Vec<(u64, u64)> = ctx
        .world
        .entities
        .get(&settlement_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::TradeRoute && r.end.is_none())
                .map(|r| (r.source_entity_id, r.target_entity_id))
                .collect()
        })
        .unwrap_or_default();

    for (source, target) in routes {
        ctx.world.end_relationship(
            source,
            target,
            RelationshipKind::TradeRoute,
            time,
            event_id,
        );

        ctx.signals.push(Signal {
            event_id,
            kind: SignalKind::TradeRouteSevered {
                from_settlement: source,
                to_settlement: target,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_from_month_correct() {
        assert_eq!(Season::from_month(1), Season::Spring);
        assert_eq!(Season::from_month(3), Season::Spring);
        assert_eq!(Season::from_month(4), Season::Summer);
        assert_eq!(Season::from_month(6), Season::Summer);
        assert_eq!(Season::from_month(7), Season::Autumn);
        assert_eq!(Season::from_month(9), Season::Autumn);
        assert_eq!(Season::from_month(10), Season::Winter);
        assert_eq!(Season::from_month(12), Season::Winter);
    }

    #[test]
    fn climate_zone_boundaries() {
        assert_eq!(climate_zone_from_y(0.0), ClimateZone::Tropical);
        assert_eq!(climate_zone_from_y(299.0), ClimateZone::Tropical);
        assert_eq!(climate_zone_from_y(300.0), ClimateZone::Temperate);
        assert_eq!(climate_zone_from_y(699.0), ClimateZone::Temperate);
        assert_eq!(climate_zone_from_y(700.0), ClimateZone::Boreal);
        assert_eq!(climate_zone_from_y(1000.0), ClimateZone::Boreal);
    }

    #[test]
    fn winter_food_lower_than_autumn() {
        let temperate_winter = compute_modifiers(Season::Winter, ClimateZone::Temperate, "plains");
        let temperate_autumn = compute_modifiers(Season::Autumn, ClimateZone::Temperate, "plains");
        assert!(
            temperate_winter.food < temperate_autumn.food,
            "winter food {} should be < autumn food {}",
            temperate_winter.food,
            temperate_autumn.food
        );
    }

    #[test]
    fn boreal_winter_harshest() {
        let boreal_winter = compute_modifiers(Season::Winter, ClimateZone::Boreal, "plains");
        let temperate_winter = compute_modifiers(Season::Winter, ClimateZone::Temperate, "plains");
        let tropical_winter = compute_modifiers(Season::Winter, ClimateZone::Tropical, "plains");
        assert!(boreal_winter.food < temperate_winter.food);
        assert!(temperate_winter.food < tropical_winter.food);
        assert!(boreal_winter.construction_blocked);
    }

    #[test]
    fn volcanic_terrain_allows_eruption() {
        let m = instant_disaster_terrain_mult(&DisasterType::VolcanicEruption, "volcanic");
        assert!(m > 0.0);
        let m2 = instant_disaster_terrain_mult(&DisasterType::VolcanicEruption, "plains");
        assert_eq!(m2, 0.0);
    }

    #[test]
    fn tsunami_coast_only() {
        let m = instant_disaster_terrain_mult(&DisasterType::Tsunami, "coast");
        assert!(m > 0.0);
        let m2 = instant_disaster_terrain_mult(&DisasterType::Tsunami, "mountains");
        assert_eq!(m2, 0.0);
    }

    #[test]
    fn disaster_type_persistent_classification() {
        assert!(!DisasterType::Earthquake.is_persistent());
        assert!(!DisasterType::VolcanicEruption.is_persistent());
        assert!(!DisasterType::Storm.is_persistent());
        assert!(!DisasterType::Tsunami.is_persistent());
        assert!(DisasterType::Drought.is_persistent());
        assert!(DisasterType::Flood.is_persistent());
        assert!(DisasterType::Wildfire.is_persistent());
    }
}
