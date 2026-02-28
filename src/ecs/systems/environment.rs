//! Environment system — migrated from `src/sim/environment.rs`.
//!
//! Four systems:
//! 1. `compute_seasonal_modifiers` (monthly) — terrain/climate → seasonal modifiers
//! 2. `compute_annual_modifiers` (yearly) — construction_months, food_annual
//! 3. `check_disasters` (monthly) — roll for instant & persistent disasters → commands
//! 4. `progress_active_disasters` (monthly) — monthly erosion, end expired disasters

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    Building, BuildingState, EcsActiveDisaster, EcsSeasonalModifiers, Region, RegionState,
    Settlement, SettlementCore, SimEntity,
};
use crate::ecs::conditions::{monthly, yearly};
use crate::ecs::relationships::LocatedIn;
use crate::ecs::resources::SimRng;
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::model::ParticipantRole;
use crate::model::entity_data::DisasterType;
use crate::model::event::EventKind;
use crate::worldgen::terrain::{Terrain, TerrainTag};

// ---------------------------------------------------------------------------
// Season / ClimateZone
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
            _ => unreachable!("month {month} out of range 1-12"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClimateZone {
    Tropical,
    Temperate,
    Boreal,
}

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
// Seasonal modifiers computation
// ---------------------------------------------------------------------------

struct SeasonalMods {
    food: f64,
    trade: f64,
    construction_blocked: bool,
    disease: f64,
    army: f64,
}

fn compute_modifiers(season: Season, climate: ClimateZone, terrain: Terrain) -> SeasonalMods {
    let (base_food, base_trade, base_disease, base_army) = match (season, climate) {
        (Season::Spring, ClimateZone::Tropical) => (0.9, 1.0, 0.9, 1.0),
        (Season::Summer, ClimateZone::Tropical) => (1.0, 1.1, 1.3, 0.9),
        (Season::Autumn, ClimateZone::Tropical) => (1.1, 1.0, 1.0, 1.0),
        (Season::Winter, ClimateZone::Tropical) => (0.9, 1.0, 0.8, 1.0),

        (Season::Spring, ClimateZone::Temperate) => (0.8, 1.0, 0.8, 1.0),
        (Season::Summer, ClimateZone::Temperate) => (1.0, 1.1, 1.2, 0.9),
        (Season::Autumn, ClimateZone::Temperate) => (1.3, 1.0, 0.9, 1.0),
        (Season::Winter, ClimateZone::Temperate) => (0.4, 0.6, 0.7, 0.6),

        (Season::Spring, ClimateZone::Boreal) => (0.6, 0.8, 0.7, 0.8),
        (Season::Summer, ClimateZone::Boreal) => (1.0, 1.0, 1.0, 1.0),
        (Season::Autumn, ClimateZone::Boreal) => (1.2, 0.9, 0.8, 0.9),
        (Season::Winter, ClimateZone::Boreal) => (0.2, 0.3, 0.6, 0.4),
    };

    let terrain_food_mult = match terrain {
        Terrain::Desert => 0.7,
        Terrain::Tundra => 0.6,
        Terrain::Swamp => 0.8,
        _ => 1.0,
    };
    let terrain_trade_mult = match terrain {
        Terrain::Mountains if season == Season::Winter => 0.5,
        Terrain::Mountains => 0.8,
        Terrain::Swamp if season == Season::Spring => 0.6,
        _ => 1.0,
    };
    let terrain_disease_mult = match terrain {
        Terrain::Swamp | Terrain::Jungle => 1.3,
        Terrain::Tundra | Terrain::Desert => 0.7,
        _ => 1.0,
    };

    let construction_blocked = match (season, climate) {
        (Season::Winter, ClimateZone::Boreal) => true,
        (Season::Winter, ClimateZone::Temperate)
            if terrain == Terrain::Mountains || terrain == Terrain::Tundra =>
        {
            true
        }
        _ => false,
    };

    SeasonalMods {
        food: base_food * terrain_food_mult,
        trade: base_trade * terrain_trade_mult,
        construction_blocked,
        disease: base_disease * terrain_disease_mult,
        army: base_army,
    }
}

// ---------------------------------------------------------------------------
// Instant disaster definitions
// ---------------------------------------------------------------------------

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

fn instant_disaster_terrain_mult(disaster: &DisasterType, terrain: Terrain) -> f64 {
    match disaster {
        DisasterType::Earthquake => match terrain {
            Terrain::Volcanic => 5.0,
            Terrain::Mountains => 3.0,
            Terrain::Hills => 1.5,
            _ => 0.3,
        },
        DisasterType::VolcanicEruption => match terrain {
            Terrain::Volcanic => 1.0,
            _ => 0.0,
        },
        DisasterType::Storm => match terrain {
            Terrain::Coast => 3.0,
            Terrain::Plains => 1.5,
            _ => 0.5,
        },
        DisasterType::Tsunami => match terrain {
            Terrain::Coast => 1.0,
            _ => 0.0,
        },
        _ => 1.0,
    }
}

fn instant_disaster_tag_mult(disaster: &DisasterType, tags: &[TerrainTag]) -> f64 {
    let mut mult = 1.0;
    for tag in tags {
        mult *= match (disaster, tag) {
            (DisasterType::Earthquake, TerrainTag::Rugged) => 1.5,
            (DisasterType::Storm, TerrainTag::Coastal) => 2.0,
            (DisasterType::Tsunami, TerrainTag::Coastal) => 1.5,
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

// ---------------------------------------------------------------------------
// Persistent disaster definitions
// ---------------------------------------------------------------------------

struct PersistentDisasterDef {
    disaster_type: DisasterType,
    base_monthly_prob: f64,
    terrain_gates: &'static [(Terrain, f64)],
    tag_gates: &'static [(TerrainTag, f64)],
    season_gates: &'static [(Season, f64)],
    duration_range: (u32, u32),
}

const PERSISTENT_DISASTERS: &[PersistentDisasterDef] = &[
    PersistentDisasterDef {
        disaster_type: DisasterType::Drought,
        base_monthly_prob: 0.0008,
        terrain_gates: &[(Terrain::Desert, 3.0), (Terrain::Plains, 1.5)],
        tag_gates: &[(TerrainTag::Arid, 3.0), (TerrainTag::Fertile, 0.5)],
        season_gates: &[(Season::Summer, 4.0)],
        duration_range: (3, 12),
    },
    PersistentDisasterDef {
        disaster_type: DisasterType::Flood,
        base_monthly_prob: 0.001,
        terrain_gates: &[(Terrain::Swamp, 2.0), (Terrain::Coast, 2.0)],
        tag_gates: &[(TerrainTag::Riverine, 3.0), (TerrainTag::Coastal, 2.0)],
        season_gates: &[(Season::Spring, 3.0), (Season::Summer, 1.5)],
        duration_range: (1, 4),
    },
    PersistentDisasterDef {
        disaster_type: DisasterType::Wildfire,
        base_monthly_prob: 0.0006,
        terrain_gates: &[
            (Terrain::Forest, 3.0),
            (Terrain::Jungle, 2.0),
            (Terrain::Plains, 1.5),
        ],
        tag_gates: &[(TerrainTag::Forested, 2.0)],
        season_gates: &[(Season::Summer, 3.0), (Season::Autumn, 2.0)],
        duration_range: (1, 3),
    },
];

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub fn add_environment_systems(app: &mut App) {
    app.add_systems(
        SimTick,
        (
            compute_seasonal_modifiers,
            check_disasters,
            progress_active_disasters,
        )
            .chain()
            .run_if(monthly)
            .in_set(SimPhase::Update),
    );
    app.add_systems(
        SimTick,
        compute_annual_modifiers
            .run_if(yearly)
            .in_set(SimPhase::Update),
    );
}

// ---------------------------------------------------------------------------
// System 1: Seasonal modifiers (monthly)
// ---------------------------------------------------------------------------

fn compute_seasonal_modifiers(
    clock: Res<SimClock>,
    mut settlements: Query<(&SimEntity, &LocatedIn, &mut EcsSeasonalModifiers), With<Settlement>>,
    regions: Query<&RegionState, With<Region>>,
) {
    let month = clock.time.month();
    let season = Season::from_month(month);

    for (sim, loc, mut seasonal) in settlements.iter_mut() {
        if !sim.is_alive() {
            continue;
        }
        let (terrain, y) = regions
            .get(loc.0)
            .map(|rs| (rs.terrain, rs.y))
            .unwrap_or((Terrain::Plains, 500.0));

        let climate = climate_zone_from_y(y);
        let mods = compute_modifiers(season, climate, terrain);

        seasonal.food = mods.food;
        seasonal.trade = mods.trade;
        seasonal.construction_blocked = mods.construction_blocked;
        seasonal.disease = mods.disease;
        seasonal.army = mods.army;
    }
}

// ---------------------------------------------------------------------------
// System 2: Annual modifiers (yearly)
// ---------------------------------------------------------------------------

fn compute_annual_modifiers(
    mut settlements: Query<(&SimEntity, &LocatedIn, &mut EcsSeasonalModifiers), With<Settlement>>,
    regions: Query<&RegionState, With<Region>>,
) {
    for (sim, loc, mut seasonal) in settlements.iter_mut() {
        if !sim.is_alive() {
            continue;
        }
        let (terrain, y) = regions
            .get(loc.0)
            .map(|rs| (rs.terrain, rs.y))
            .unwrap_or((Terrain::Plains, 500.0));

        let climate = climate_zone_from_y(y);

        let construction_months: u32 = (1..=12)
            .filter(|&m| {
                let s = Season::from_month(m);
                !compute_modifiers(s, climate, terrain).construction_blocked
            })
            .count() as u32;

        let annual_food: f64 = (1..=12)
            .map(|m| {
                let s = Season::from_month(m);
                compute_modifiers(s, climate, terrain).food
            })
            .sum::<f64>()
            / 12.0;

        seasonal.construction_months = construction_months;
        seasonal.food_annual = annual_food;
    }
}

// ---------------------------------------------------------------------------
// System 3: Check for disasters (monthly)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn check_disasters(
    clock: Res<SimClock>,
    mut rng: ResMut<SimRng>,
    settlements: Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            &LocatedIn,
            Option<&EcsActiveDisaster>,
        ),
        With<Settlement>,
    >,
    regions: Query<&RegionState, With<Region>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let month = clock.time.month();
    let season = Season::from_month(month);

    // Collect settlement info
    struct SettlementInfo {
        entity: Entity,
        terrain: Terrain,
        terrain_tags: Vec<TerrainTag>,
        #[allow(dead_code)]
        region_y: f64,
        population: u32,
        has_active_disaster: bool,
    }

    let infos: Vec<SettlementInfo> = settlements
        .iter()
        .filter(|(_, sim, _, _, _)| sim.is_alive())
        .map(|(entity, _, core, loc, active_disaster)| {
            let (terrain, terrain_tags, region_y) = regions
                .get(loc.0)
                .map(|rs| (rs.terrain, rs.terrain_tags.clone(), rs.y))
                .unwrap_or((Terrain::Plains, vec![], 500.0));
            SettlementInfo {
                entity,
                terrain,
                terrain_tags,
                region_y,
                population: core.population,
                has_active_disaster: active_disaster.is_some(),
            }
        })
        .collect();

    let rng = &mut rng.0;

    // Check instant disasters
    for info in &infos {
        if info.has_active_disaster || info.population < 10 {
            continue;
        }
        for def in INSTANT_DISASTERS {
            let terrain_m = instant_disaster_terrain_mult(&def.disaster_type, info.terrain);
            if terrain_m == 0.0 {
                continue;
            }
            let tag_m = instant_disaster_tag_mult(&def.disaster_type, &info.terrain_tags);
            let season_m = season_mult_instant(&def.disaster_type, season);
            let prob = def.base_monthly_prob * terrain_m * tag_m * season_m;

            let roll: f64 = rng.random();
            if roll < prob {
                let severity: f64 = rng.random();
                let pop_loss_frac =
                    def.pop_loss_range.0 + severity * (def.pop_loss_range.1 - def.pop_loss_range.0);
                let building_damage = def.building_damage_range.0
                    + severity * (def.building_damage_range.1 - def.building_damage_range.0);

                // Create feature for severe volcanic/earthquake
                let create_feature = if severity > 0.7
                    && matches!(
                        def.disaster_type,
                        DisasterType::VolcanicEruption | DisasterType::Earthquake
                    ) {
                    let feature_type = match def.disaster_type {
                        DisasterType::VolcanicEruption => crate::model::FeatureType::LavaField,
                        DisasterType::Earthquake => crate::model::FeatureType::FaultLine,
                        _ => crate::model::FeatureType::Crater,
                    };
                    Some((
                        format!("{} near settlement", feature_type.as_str()),
                        feature_type,
                    ))
                } else {
                    None
                };

                commands.write(
                    SimCommand::new(
                        SimCommandKind::TriggerDisaster {
                            settlement: info.entity,
                            disaster_type: def.disaster_type,
                            severity,
                            pop_loss_frac,
                            building_damage,
                            prosperity_hit: def.prosperity_hit,
                            sever_trade: def.sever_trade,
                            create_feature,
                        },
                        EventKind::Disaster,
                        format!(
                            "{} strikes settlement (severity {:.0}%)",
                            def.disaster_type.as_str(),
                            severity * 100.0
                        ),
                    )
                    .with_participant(info.entity, ParticipantRole::Object)
                    .with_data(serde_json::json!({
                        "disaster_type": def.disaster_type.as_str(),
                        "phase": "instant"
                    })),
                );
                break; // One disaster per settlement per tick
            }
        }
    }

    // Check persistent disasters
    for info in &infos {
        if info.has_active_disaster || info.population < 10 {
            continue;
        }
        for def in PERSISTENT_DISASTERS {
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

            let roll: f64 = rng.random();
            if roll < prob {
                let duration = rng.random_range(def.duration_range.0..=def.duration_range.1);
                let severity: f64 = rng.random_range(0.3..1.0);

                commands.write(
                    SimCommand::new(
                        SimCommandKind::StartPersistentDisaster {
                            settlement: info.entity,
                            disaster_type: def.disaster_type,
                            severity,
                            months: duration,
                        },
                        EventKind::Disaster,
                        format!(
                            "{} begins in settlement (severity {:.0}%, est. {} months)",
                            def.disaster_type.as_str(),
                            severity * 100.0,
                            duration
                        ),
                    )
                    .with_participant(info.entity, ParticipantRole::Object)
                    .with_data(serde_json::json!({
                        "disaster_type": def.disaster_type.as_str(),
                        "phase": "start"
                    })),
                );
                break; // One disaster per settlement per tick
            }
        }
    }
}

// ---------------------------------------------------------------------------
// System 4: Progress active disasters (monthly)
// ---------------------------------------------------------------------------

fn progress_active_disasters(
    mut settlements: Query<
        (
            Entity,
            &SimEntity,
            &mut SettlementCore,
            &mut EcsSeasonalModifiers,
            &mut EcsActiveDisaster,
        ),
        With<Settlement>,
    >,
    buildings: Query<(Entity, &BuildingState, &LocatedIn), With<Building>>,
    mut rng: ResMut<SimRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    let updates: Vec<(Entity, u32, f64, f64, DisasterType, bool)> = settlements
        .iter()
        .filter(|(_, sim, _, _, _)| sim.is_alive())
        .map(|(entity, _, core, _, disaster)| {
            let (pop_loss_frac, building_damage) = match disaster.disaster_type {
                DisasterType::Drought => (0.005 + disaster.severity * 0.015, 0.0),
                DisasterType::Flood => (0.01 + disaster.severity * 0.02, 0.1),
                DisasterType::Wildfire => {
                    (0.02 + disaster.severity * 0.03, 0.2 * disaster.severity)
                }
                _ => (0.0, 0.0),
            };
            let deaths = (core.population as f64 * pop_loss_frac) as u32;
            let prosperity_hit = match disaster.disaster_type {
                DisasterType::Drought => 0.02 * disaster.severity,
                DisasterType::Flood => 0.03 * disaster.severity,
                DisasterType::Wildfire => 0.03 * disaster.severity,
                _ => 0.0,
            };
            let ended = disaster.months_remaining <= 1;
            (
                entity,
                deaths,
                prosperity_hit,
                building_damage,
                disaster.disaster_type,
                ended,
            )
        })
        .collect();

    for (entity, deaths, prosperity_hit, building_damage, disaster_type, ended) in &updates {
        if let Ok((_, _, mut core, mut seasonal, mut disaster)) = settlements.get_mut(*entity) {
            core.population = core.population.saturating_sub(*deaths);
            let pop = core.population;
            core.population_breakdown.scale_to(pop);
            core.prosperity = (core.prosperity - prosperity_hit).max(0.0);

            disaster.months_remaining = disaster.months_remaining.saturating_sub(1);
            disaster.total_deaths += deaths;

            // Override food modifier for drought
            if *disaster_type == DisasterType::Drought {
                seasonal.food = 0.2;
            }
        }

        // Apply building damage: each building at this settlement has a chance to be damaged
        if *building_damage > 0.0 {
            for (bld_entity, _, bld_loc) in buildings.iter() {
                if bld_loc.0 == *entity && rng.0.random_bool(*building_damage) {
                    commands.write(
                        SimCommand::new(
                            SimCommandKind::DamageBuilding {
                                building: bld_entity,
                                damage: 1.0,
                                cause: format!("{} damage", disaster_type.as_str()),
                            },
                            EventKind::Disaster,
                            format!("Building damaged by {}", disaster_type.as_str()),
                        )
                        .with_participant(bld_entity, ParticipantRole::Object)
                        .with_participant(*entity, ParticipantRole::Location),
                    );
                }
            }
        }

        if *ended {
            commands.write(
                SimCommand::new(
                    SimCommandKind::EndDisaster {
                        settlement: *entity,
                    },
                    EventKind::Disaster,
                    format!("{} ends", disaster_type.as_str()),
                )
                .with_participant(*entity, ParticipantRole::Object)
                .with_data(serde_json::json!({
                    "disaster_type": disaster_type.as_str(),
                    "phase": "end"
                })),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app_seeded;
    use crate::ecs::components::*;
    use crate::ecs::resources::SimEntityMap;
    use crate::ecs::test_helpers::tick_months;
    use crate::ecs::time::SimTime;

    fn setup_app() -> App {
        let mut app = build_sim_app_seeded(100, 42);
        add_environment_systems(&mut app);
        app
    }

    fn spawn_region(app: &mut App, sim_id: u64, terrain: Terrain, y: f64) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Region".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState {
                    terrain,
                    y,
                    ..RegionState::default()
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_settlement(app: &mut App, sim_id: u64, region: Entity, population: u32) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Town".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Settlement,
                SettlementCore {
                    population,
                    prosperity: 0.5,
                    ..SettlementCore::default()
                },
                SettlementCulture::default(),
                SettlementDisease::default(),
                SettlementTrade::default(),
                SettlementMilitary::default(),
                SettlementCrime::default(),
                SettlementEducation::default(),
                EcsSeasonalModifiers::default(),
                EcsBuildingBonuses::default(),
            ))
            .id();
        app.world_mut().entity_mut(entity).insert(LocatedIn(region));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

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
        let temperate_winter =
            compute_modifiers(Season::Winter, ClimateZone::Temperate, Terrain::Plains);
        let temperate_autumn =
            compute_modifiers(Season::Autumn, ClimateZone::Temperate, Terrain::Plains);
        assert!(
            temperate_winter.food < temperate_autumn.food,
            "winter food {} should be < autumn food {}",
            temperate_winter.food,
            temperate_autumn.food
        );
    }

    #[test]
    fn boreal_winter_harshest() {
        let boreal_winter = compute_modifiers(Season::Winter, ClimateZone::Boreal, Terrain::Plains);
        let temperate_winter =
            compute_modifiers(Season::Winter, ClimateZone::Temperate, Terrain::Plains);
        let tropical_winter =
            compute_modifiers(Season::Winter, ClimateZone::Tropical, Terrain::Plains);
        assert!(boreal_winter.food < temperate_winter.food);
        assert!(temperate_winter.food < tropical_winter.food);
        assert!(boreal_winter.construction_blocked);
    }

    #[test]
    fn volcanic_terrain_allows_eruption() {
        let m = instant_disaster_terrain_mult(&DisasterType::VolcanicEruption, Terrain::Volcanic);
        assert!(m > 0.0);
        let m2 = instant_disaster_terrain_mult(&DisasterType::VolcanicEruption, Terrain::Plains);
        assert_eq!(m2, 0.0);
    }

    #[test]
    fn tsunami_coast_only() {
        let m = instant_disaster_terrain_mult(&DisasterType::Tsunami, Terrain::Coast);
        assert!(m > 0.0);
        let m2 = instant_disaster_terrain_mult(&DisasterType::Tsunami, Terrain::Mountains);
        assert_eq!(m2, 0.0);
    }

    #[test]
    fn seasonal_modifiers_applied() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1001, Terrain::Plains, 500.0);
        let settlement = spawn_settlement(&mut app, 1002, region, 500);

        // Tick 1 month
        tick_months(&mut app, 1);

        let seasonal = app.world().get::<EcsSeasonalModifiers>(settlement).unwrap();
        // At year 100 month 1 = Spring + Temperate (y=500) + Plains
        assert!(
            (seasonal.food - 0.8).abs() < 0.01,
            "spring temperate plains food should be ~0.8, got {}",
            seasonal.food
        );
    }

    #[test]
    fn annual_modifiers_computed_at_year_start() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1001, Terrain::Plains, 500.0);
        let settlement = spawn_settlement(&mut app, 1002, region, 500);

        // Tick 1 year — yearly system fires once, computing construction_months
        crate::ecs::test_helpers::tick_years(&mut app, 1);

        let seasonal = app.world().get::<EcsSeasonalModifiers>(settlement).unwrap();
        // Temperate Plains: winter is blocked in mountains/tundra only, so all 12 months available
        assert_eq!(
            seasonal.construction_months, 12,
            "temperate plains should have 12 construction months"
        );
        assert!(
            seasonal.food_annual > 0.0,
            "annual food modifier should be positive"
        );
    }

    #[test]
    fn persistent_disaster_progress_and_end() {
        let mut app = setup_app();
        let region = spawn_region(&mut app, 1001, Terrain::Plains, 500.0);
        let settlement = spawn_settlement(&mut app, 1002, region, 500);

        // Manually add an active disaster
        app.world_mut()
            .entity_mut(settlement)
            .insert(EcsActiveDisaster {
                disaster_type: DisasterType::Drought,
                severity: 0.7,
                started: SimTime::from_year(100),
                months_remaining: 2,
                total_deaths: 0,
            });

        let initial_prosperity = app
            .world()
            .get::<SettlementCore>(settlement)
            .unwrap()
            .prosperity;

        // Tick 3 months — disaster should progress and end
        tick_months(&mut app, 3);

        // Prosperity should have decreased from drought erosion
        let new_prosperity = app
            .world()
            .get::<SettlementCore>(settlement)
            .unwrap()
            .prosperity;
        assert!(
            new_prosperity < initial_prosperity,
            "prosperity should decrease during drought: before={initial_prosperity}, after={new_prosperity}"
        );

        // Disaster should be cleared (EndDisaster command was emitted)
        assert!(
            app.world().get::<EcsActiveDisaster>(settlement).is_none(),
            "active disaster should be cleared after expiration"
        );
    }
}
