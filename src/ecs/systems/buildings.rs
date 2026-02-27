//! Buildings system — migrated from `src/sim/buildings.rs`.
//!
//! Four tick systems (yearly, Update phase):
//! 1. `compute_building_bonuses` — queries buildings per settlement, writes EcsBuildingBonuses
//! 2. `decay_buildings` — reduces condition, emits EndEntity for destroyed buildings
//! 3. `construct_buildings` — evaluates eligibility, emits ConstructBuilding commands
//! 4. `upgrade_buildings` — evaluates candidates, emits UpgradeBuilding commands
//!
//! One reaction system (Reactions phase):
//! 5. `handle_settlement_captured_buildings` — reads SettlementCaptured → DamageBuilding

use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::query::{With, Without};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    Building, BuildingState, EcsActiveSiege, EcsBuildingBonuses, EcsSeasonalModifiers,
    Faction, FactionCore, Settlement, SettlementCore, SettlementTrade, SimEntity,
    EcsActiveDisaster,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{LocatedIn, MemberOf};
use crate::ecs::resources::SimRng;
use crate::ecs::schedule::{SimPhase, SimTick};
use crate::model::entity_data::{BuildingType, ResourceType};
use crate::model::event::EventKind;
use crate::model::ParticipantRole;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BUILDING_SPECS: &[(BuildingType, u32, f64)] = &[
    (BuildingType::Granary, 100, 15.0),
    (BuildingType::Port, 150, 20.0),
    (BuildingType::Market, 200, 25.0),
    (BuildingType::Workshop, 300, 30.0),
    (BuildingType::Temple, 400, 40.0),
    (BuildingType::Library, 500, 50.0),
    (BuildingType::ScholarGuild, 800, 70.0),
    (BuildingType::Aqueduct, 800, 80.0),
];

const LEVEL_SCALING: f64 = 0.5;
const MINE_BONUS: f64 = 0.30;
const PORT_TRADE_BONUS: f64 = 0.20;
const PORT_FISHING_BONUS: f64 = 0.30;
const MARKET_BONUS: f64 = 0.25;
const TEMPLE_HAPPINESS_BONUS: f64 = 0.05;
const TEMPLE_KNOWLEDGE_BONUS: f64 = 0.10;
const TEMPLE_RELIGION_BONUS: f64 = 0.02;
const WORKSHOP_BONUS: f64 = 0.20;
const AQUEDUCT_CAPACITY_BONUS: f64 = 100.0;
const LIBRARY_HAPPINESS_BONUS: f64 = 0.02;
const LIBRARY_BONUS: f64 = 0.15;
const SCHOLAR_GUILD_ACADEMY_BONUS: f64 = 0.25;
const SCHOLAR_GUILD_HAPPINESS_BONUS: f64 = 0.03;

const NORMAL_DECAY_RATE: f64 = 0.01;
const SIEGE_DECAY_RATE: f64 = 0.05;
const ABANDONED_DECAY_RATE: f64 = 0.10;

const POP_PER_BUILDING_SLOT: u32 = 200;
const CONSTRUCTION_CHANCE_BASE: f64 = 0.3;
const CONSTRUCTION_CHANCE_PROSPERITY_FACTOR: f64 = 0.3;
const MIN_CONSTRUCTION_MONTHS: u32 = 4;
const AQUEDUCT_CAPACITY_RATIO_THRESHOLD: f64 = 0.8;

const MAX_BUILDING_LEVEL: u8 = 2;
const UPGRADE_MIN_PROSPERITY: f64 = 0.6;
const UPGRADE_POP_THRESHOLD_1: u32 = 200;
const UPGRADE_POP_THRESHOLD_2: u32 = 500;
const UPGRADE_COST_MULTIPLIER_1: f64 = 1.5;
const UPGRADE_COST_MULTIPLIER_2: f64 = 3.0;
const UPGRADE_DEFAULT_BASE_COST: f64 = 20.0;
const UPGRADE_PROBABILITY: f64 = 0.2;

const CONQUEST_MIN_DAMAGE: f64 = 0.2;
const CONQUEST_MAX_DAMAGE: f64 = 0.5;

fn effective_bonus(condition: f64, level: u8) -> f64 {
    condition * (1.0 + LEVEL_SCALING * level as f64)
}

fn capitalize_building_type(bt: &BuildingType) -> &str {
    match bt {
        BuildingType::Mine => "Mine",
        BuildingType::Port => "Port",
        BuildingType::Market => "Market",
        BuildingType::Granary => "Granary",
        BuildingType::Temple => "Temple",
        BuildingType::Workshop => "Workshop",
        BuildingType::Aqueduct => "Aqueduct",
        BuildingType::Library => "Library",
        BuildingType::ScholarGuild => "Scholar Guild",
    }
}

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub fn add_buildings_systems(app: &mut App) {
    app.add_systems(
        SimTick,
        (
            compute_building_bonuses,
            decay_buildings,
            construct_buildings,
            upgrade_buildings,
        )
            .chain()
            .run_if(yearly)
            .in_set(SimPhase::Update),
    );
    app.add_systems(
        SimTick,
        handle_settlement_captured_buildings
            .in_set(SimPhase::Reactions),
    );
}

// ---------------------------------------------------------------------------
// System 1: Compute building bonuses
// ---------------------------------------------------------------------------

fn compute_building_bonuses(
    mut settlements: Query<
        (Entity, &SimEntity, &SettlementCore, &SettlementTrade, &mut EcsBuildingBonuses),
        With<Settlement>,
    >,
    buildings: Query<(&SimEntity, &BuildingState, &LocatedIn), With<Building>>,
) {
    for (sett_entity, sett_sim, sett_core, sett_trade, mut bonuses) in settlements.iter_mut() {
        if !sett_sim.is_alive() {
            continue;
        }

        let has_fish = sett_trade.is_coastal
            && sett_core.resources.iter().any(|r| matches!(r, ResourceType::Fish));

        let mut mine_bonus = 0.0;
        let mut workshop_bonus = 0.0;
        let mut market_bonus = 0.0;
        let mut port_trade_bonus = 0.0;
        let mut port_range_bonus = 0.0;
        let mut happiness_bonus = 0.0;
        let mut capacity_bonus = 0.0;
        let mut food_buffer = 0.0;
        let mut library_bonus = 0.0;
        let mut temple_knowledge_bonus = 0.0;
        let mut temple_religion_bonus = 0.0;
        let mut academy_bonus = 0.0;
        let mut fishing_bonus = 0.0;

        for (b_sim, b_state, b_loc) in buildings.iter() {
            if !b_sim.is_alive() || b_loc.0 != sett_entity {
                continue;
            }
            let eff = effective_bonus(b_state.condition, b_state.level);
            match b_state.building_type {
                BuildingType::Mine => mine_bonus += MINE_BONUS * eff,
                BuildingType::Port => {
                    port_trade_bonus += PORT_TRADE_BONUS * eff;
                    port_range_bonus += 1.0;
                    if has_fish {
                        fishing_bonus += PORT_FISHING_BONUS * eff;
                    }
                }
                BuildingType::Market => market_bonus += MARKET_BONUS * eff,
                BuildingType::Granary => food_buffer += 1.0 * eff,
                BuildingType::Temple => {
                    happiness_bonus += TEMPLE_HAPPINESS_BONUS * eff;
                    temple_knowledge_bonus += TEMPLE_KNOWLEDGE_BONUS * eff;
                    temple_religion_bonus += TEMPLE_RELIGION_BONUS * eff;
                }
                BuildingType::Workshop => workshop_bonus += WORKSHOP_BONUS * eff,
                BuildingType::Aqueduct => capacity_bonus += AQUEDUCT_CAPACITY_BONUS * eff,
                BuildingType::Library => {
                    happiness_bonus += LIBRARY_HAPPINESS_BONUS * eff;
                    library_bonus += LIBRARY_BONUS * eff;
                }
                BuildingType::ScholarGuild => {
                    academy_bonus += SCHOLAR_GUILD_ACADEMY_BONUS * eff;
                    happiness_bonus += SCHOLAR_GUILD_HAPPINESS_BONUS * eff;
                }
            }
        }

        bonuses.mine = mine_bonus;
        bonuses.workshop = workshop_bonus;
        bonuses.market = market_bonus;
        bonuses.port_trade = port_trade_bonus;
        bonuses.port_range = port_range_bonus;
        bonuses.happiness = happiness_bonus;
        bonuses.capacity = capacity_bonus;
        bonuses.food_buffer = food_buffer;
        bonuses.library = library_bonus;
        bonuses.temple_knowledge = temple_knowledge_bonus;
        bonuses.temple_religion = temple_religion_bonus;
        bonuses.academy = academy_bonus;
        bonuses.fishing = fishing_bonus;
    }
}

// ---------------------------------------------------------------------------
// System 2: Decay buildings
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn decay_buildings(
    clock: Res<SimClock>,
    mut buildings: Query<(Entity, &mut SimEntity, &mut BuildingState, &LocatedIn), (With<Building>, Without<Settlement>)>,
    settlements: Query<
        (&SimEntity, Option<&EcsActiveSiege>),
        (With<Settlement>, Without<Building>),
    >,
    mut commands: MessageWriter<SimCommand>,
) {
    let current_year = clock.time.year();

    struct DecayUpdate {
        entity: Entity,
        new_condition: f64,
        destroy: bool,
    }

    let updates: Vec<DecayUpdate> = buildings
        .iter()
        .filter(|(_, sim, _, _)| sim.is_alive())
        .map(|(entity, _, state, loc)| {
            let (settlement_ended, under_siege) = settlements
                .get(loc.0)
                .map(|(sim, siege)| (!sim.is_alive(), siege.is_some()))
                .unwrap_or((true, false));

            let decay_rate = if settlement_ended {
                ABANDONED_DECAY_RATE
            } else if under_siege {
                SIEGE_DECAY_RATE
            } else {
                NORMAL_DECAY_RATE
            };

            let new_condition = (state.condition - decay_rate).max(0.0);
            DecayUpdate {
                entity,
                new_condition,
                destroy: new_condition <= 0.0,
            }
        })
        .collect();

    for u in updates {
        if u.destroy {
            commands.write(
                SimCommand::new(
                    SimCommandKind::EndEntity { entity: u.entity },
                    EventKind::Destruction,
                    format!("Building crumbled to ruin in year {current_year}"),
                )
                .with_participant(u.entity, ParticipantRole::Subject),
            );
        } else if let Ok((_, _, mut state, _)) = buildings.get_mut(u.entity) {
            state.condition = u.new_condition;
        }
    }
}

// ---------------------------------------------------------------------------
// System 3: Construct buildings
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn construct_buildings(
    clock: Res<SimClock>,
    mut rng: ResMut<SimRng>,
    settlements: Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            &SettlementTrade,
            &EcsSeasonalModifiers,
            Option<&EcsActiveSiege>,
            Option<&EcsActiveDisaster>,
            Option<&MemberOf>,
        ),
        With<Settlement>,
    >,
    buildings: Query<(&SimEntity, &BuildingState, &LocatedIn), With<Building>>,
    factions: Query<&FactionCore, With<Faction>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let current_year = clock.time.year();
    let rng = &mut rng.0;

    struct BuildPlan {
        settlement: Entity,
        settlement_name: String,
        faction: Entity,
        building_type: BuildingType,
        cost: f64,
        x: f64,
        y: f64,
    }

    let mut plans: Vec<BuildPlan> = Vec::new();

    for (sett_entity, sett_sim, sett_core, sett_trade, seasonal, siege, disaster, member_of) in
        settlements.iter()
    {
        if !sett_sim.is_alive() || siege.is_some() || disaster.is_some() {
            continue;
        }
        if seasonal.construction_months < MIN_CONSTRUCTION_MONTHS {
            continue;
        }
        let faction_entity = match member_of {
            Some(m) => m.0,
            None => continue,
        };

        // Building count and capacity
        let building_count = buildings
            .iter()
            .filter(|(b_sim, _, b_loc)| b_sim.is_alive() && b_loc.0 == sett_entity)
            .count();
        let max_buildings = (sett_core.population / POP_PER_BUILDING_SLOT).max(1) as usize;
        if building_count >= max_buildings {
            continue;
        }

        // Probability check
        let season_scale = seasonal.construction_months as f64 / 12.0;
        let build_chance = (CONSTRUCTION_CHANCE_BASE
            + CONSTRUCTION_CHANCE_PROSPERITY_FACTOR * sett_core.prosperity)
            * season_scale;
        if rng.random_range(0.0..1.0) >= build_chance {
            continue;
        }

        let has_trade_routes = !sett_trade.trade_routes.is_empty();
        let has_non_food_resource = sett_core
            .resources
            .iter()
            .any(|r| !crate::sim::helpers::is_food_resource(r));

        // Priority order
        for &(bt, min_pop, cost) in BUILDING_SPECS {
            if sett_core.population < min_pop {
                continue;
            }
            // Skip if already has this building type
            let already_has = buildings
                .iter()
                .any(|(b_sim, b_state, b_loc)| {
                    b_sim.is_alive() && b_loc.0 == sett_entity && b_state.building_type == bt
                });
            if already_has {
                continue;
            }

            // Prerequisites
            match bt {
                BuildingType::Port => {
                    if !sett_trade.is_coastal {
                        continue;
                    }
                }
                BuildingType::Market => {
                    if !has_trade_routes {
                        continue;
                    }
                }
                BuildingType::Workshop => {
                    if !has_non_food_resource {
                        continue;
                    }
                }
                BuildingType::Library => {
                    let has_temple = buildings.iter().any(|(b_sim, b_state, b_loc)| {
                        b_sim.is_alive()
                            && b_loc.0 == sett_entity
                            && b_state.building_type == BuildingType::Temple
                    });
                    if !has_temple {
                        continue;
                    }
                }
                BuildingType::ScholarGuild => {
                    let has_library = buildings.iter().any(|(b_sim, b_state, b_loc)| {
                        b_sim.is_alive()
                            && b_loc.0 == sett_entity
                            && b_state.building_type == BuildingType::Library
                    });
                    if !has_library {
                        continue;
                    }
                }
                BuildingType::Aqueduct => {
                    let capacity = if sett_core.capacity == 0 {
                        500u64
                    } else {
                        sett_core.capacity as u64
                    };
                    if (sett_core.population as f64)
                        <= capacity as f64 * AQUEDUCT_CAPACITY_RATIO_THRESHOLD
                    {
                        continue;
                    }
                }
                _ => {}
            }

            // Check faction treasury
            let treasury = factions
                .get(faction_entity)
                .map(|fc| fc.treasury)
                .unwrap_or(0.0);
            if treasury < cost {
                continue;
            }

            plans.push(BuildPlan {
                settlement: sett_entity,
                settlement_name: sett_sim.name.clone(),
                faction: faction_entity,
                building_type: bt,
                cost,
                x: sett_core.x,
                y: sett_core.y,
            });
            break; // One building per settlement per year
        }
    }

    for plan in plans {
        commands.write(
            SimCommand::new(
                SimCommandKind::ConstructBuilding {
                    settlement: plan.settlement,
                    faction: plan.faction,
                    building_type: plan.building_type,
                    cost: plan.cost,
                    x: plan.x,
                    y: plan.y,
                },
                EventKind::Construction,
                format!(
                    "{} built in {} in year {current_year}",
                    capitalize_building_type(&plan.building_type),
                    plan.settlement_name
                ),
            )
            .with_participant(plan.settlement, ParticipantRole::Subject),
        );
    }
}

// ---------------------------------------------------------------------------
// System 4: Upgrade buildings
// ---------------------------------------------------------------------------

fn upgrade_buildings(
    clock: Res<SimClock>,
    mut rng: ResMut<SimRng>,
    settlements: Query<
        (Entity, &SimEntity, &SettlementCore, Option<&MemberOf>),
        With<Settlement>,
    >,
    buildings: Query<(Entity, &SimEntity, &BuildingState, &LocatedIn), With<Building>>,
    factions: Query<&FactionCore, With<Faction>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let current_year = clock.time.year();
    let rng = &mut rng.0;

    for (sett_entity, sett_sim, sett_core, member_of) in settlements.iter() {
        if !sett_sim.is_alive() || sett_core.prosperity <= UPGRADE_MIN_PROSPERITY {
            continue;
        }
        let faction_entity = match member_of {
            Some(m) => m.0,
            None => continue,
        };

        // Find upgradable buildings
        let upgradable: Vec<(Entity, BuildingType, u8)> = buildings
            .iter()
            .filter(|(_, b_sim, b_state, b_loc)| {
                b_sim.is_alive()
                    && b_loc.0 == sett_entity
                    && b_state.level < MAX_BUILDING_LEVEL
            })
            .map(|(e, _, b_state, _)| (e, b_state.building_type, b_state.level))
            .collect();

        if upgradable.is_empty() {
            continue;
        }

        let idx = rng.random_range(0..upgradable.len());
        let (building_entity, building_type, level) = upgradable[idx];

        let (pop_threshold, cost_multiplier) = match level {
            0 => (UPGRADE_POP_THRESHOLD_1, UPGRADE_COST_MULTIPLIER_1),
            1 => (UPGRADE_POP_THRESHOLD_2, UPGRADE_COST_MULTIPLIER_2),
            _ => continue,
        };

        if sett_core.population < pop_threshold {
            continue;
        }

        let base_cost = BUILDING_SPECS
            .iter()
            .find(|(bt, _, _)| *bt == building_type)
            .map(|(_, _, cost)| *cost)
            .unwrap_or(UPGRADE_DEFAULT_BASE_COST);
        let upgrade_cost = base_cost * cost_multiplier;

        let treasury = factions
            .get(faction_entity)
            .map(|fc| fc.treasury)
            .unwrap_or(0.0);
        if treasury < upgrade_cost {
            continue;
        }

        if rng.random_range(0.0..1.0) >= UPGRADE_PROBABILITY {
            continue;
        }

        let new_level = level + 1;
        let level_name = match new_level {
            1 => "improved",
            2 => "grand",
            _ => "upgraded",
        };

        commands.write(
            SimCommand::new(
                SimCommandKind::UpgradeBuilding {
                    building: building_entity,
                    new_level,
                    cost: upgrade_cost,
                    faction: faction_entity,
                },
                EventKind::Upgrade,
                format!(
                    "Building upgraded to {level_name} in year {current_year}"
                ),
            )
            .with_participant(building_entity, ParticipantRole::Subject),
        );
    }
}

// ---------------------------------------------------------------------------
// Reaction system: SettlementCaptured → DamageBuilding
// ---------------------------------------------------------------------------

fn handle_settlement_captured_buildings(
    mut rng: ResMut<SimRng>,
    mut events: MessageReader<SimReactiveEvent>,
    buildings: Query<(Entity, &SimEntity, &LocatedIn), With<Building>>,
    mut commands: MessageWriter<SimCommand>,
) {
    let rng = &mut rng.0;
    for event in events.read() {
        if let SimReactiveEvent::SettlementCaptured {
            event_id,
            settlement,
            ..
        } = event
        {
            let building_entities: Vec<Entity> = buildings
                .iter()
                .filter(|(_, sim, loc)| sim.is_alive() && loc.0 == *settlement)
                .map(|(e, _, _)| e)
                .collect();

            for building in building_entities {
                let damage = rng.random_range(CONQUEST_MIN_DAMAGE..CONQUEST_MAX_DAMAGE);
                commands.write(
                    SimCommand::new(
                        SimCommandKind::DamageBuilding {
                            building,
                            damage,
                            cause: "conquest".to_string(),
                        },
                        EventKind::Destruction,
                        "Building damaged during conquest".to_string(),
                    )
                    .caused_by(*event_id)
                    .with_participant(building, ParticipantRole::Object),
                );
            }
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
    use crate::ecs::test_helpers::tick_years;
    use crate::ecs::time::SimTime;

    fn setup_app() -> App {
        let mut app = build_sim_app_seeded(100, 42);
        add_buildings_systems(&mut app);
        app
    }

    fn spawn_faction(app: &mut App, sim_id: u64, treasury: f64) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: "Kingdom".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Faction,
                FactionCore {
                    treasury,
                    ..FactionCore::default()
                },
                FactionDiplomacy::default(),
                FactionMilitary::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_settlement(
        app: &mut App,
        sim_id: u64,
        faction: Entity,
        population: u32,
        prosperity: f64,
    ) -> Entity {
        let region = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id + 100,
                    name: "Region".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id + 100, region);

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
                    prosperity,
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
        app.world_mut()
            .entity_mut(entity)
            .insert((LocatedIn(region), MemberOf(faction)));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_building(
        app: &mut App,
        sim_id: u64,
        building_type: BuildingType,
        settlement: Entity,
        condition: f64,
        level: u8,
    ) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: format!("{:?}", building_type),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Building,
                BuildingState {
                    building_type,
                    condition,
                    level,
                    ..BuildingState::default()
                },
            ))
            .id();
        app.world_mut()
            .entity_mut(entity)
            .insert(LocatedIn(settlement));
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    #[test]
    fn mine_bonus_computed() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1001, 500.0);
        let sett = spawn_settlement(&mut app, 1002, faction, 500, 0.7);
        // Add resources so settlement has iron
        app.world_mut().get_mut::<SettlementCore>(sett).unwrap().resources =
            vec![ResourceType::Iron, ResourceType::Grain];
        spawn_building(&mut app, 1010, BuildingType::Mine, sett, 1.0, 0);

        tick_years(&mut app, 1);

        let bonuses = app.world().get::<EcsBuildingBonuses>(sett).unwrap();
        assert!(
            (bonuses.mine - 0.30).abs() < 0.01,
            "mine bonus should be ~0.30, got {}",
            bonuses.mine
        );
    }

    #[test]
    fn bonus_scales_with_level() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1001, 500.0);
        let sett = spawn_settlement(&mut app, 1002, faction, 500, 0.7);
        spawn_building(&mut app, 1010, BuildingType::Temple, sett, 1.0, 2);

        tick_years(&mut app, 1);

        let bonuses = app.world().get::<EcsBuildingBonuses>(sett).unwrap();
        // 0.05 * 1.0 * (1 + 0.5 * 2) = 0.05 * 2.0 = 0.10
        assert!(
            (bonuses.happiness - 0.10).abs() < 0.01,
            "level 2 temple happiness should be ~0.10, got {}",
            bonuses.happiness
        );
    }

    #[test]
    fn decay_reduces_condition() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1001, 500.0);
        let sett = spawn_settlement(&mut app, 1002, faction, 500, 0.7);
        let bld = spawn_building(&mut app, 1010, BuildingType::Market, sett, 0.5, 0);

        tick_years(&mut app, 1);

        let state = app.world().get::<BuildingState>(bld).unwrap();
        assert!(
            (state.condition - 0.49).abs() < 0.02,
            "condition should decrease by ~0.01: got {}",
            state.condition
        );
    }

    #[test]
    fn decay_destroys_at_zero() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1001, 500.0);
        let sett = spawn_settlement(&mut app, 1002, faction, 500, 0.7);
        let bld = spawn_building(&mut app, 1010, BuildingType::Granary, sett, 0.005, 0);

        tick_years(&mut app, 1);

        let sim = app.world().get::<SimEntity>(bld).unwrap();
        assert!(sim.end.is_some(), "building should be destroyed");
    }

    #[test]
    fn construction_creates_building() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1001, 500.0);
        let sett = spawn_settlement(&mut app, 1002, faction, 500, 0.7);
        app.world_mut().get_mut::<SettlementCore>(sett).unwrap().resources =
            vec![ResourceType::Iron, ResourceType::Grain];

        let buildings_before: usize = app
            .world_mut()
            .query_filtered::<&SimEntity, With<Building>>()
            .iter(app.world())
            .filter(|s| s.is_alive())
            .count();

        // Run for several years to give construction a chance
        tick_years(&mut app, 20);

        let buildings_after: usize = app
            .world_mut()
            .query_filtered::<&SimEntity, With<Building>>()
            .iter(app.world())
            .filter(|s| s.is_alive())
            .count();

        assert!(
            buildings_after > buildings_before,
            "should have constructed at least one building (before={buildings_before}, after={buildings_after})"
        );
    }

    #[test]
    fn no_construction_under_siege() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1001, 500.0);
        let sett = spawn_settlement(&mut app, 1002, faction, 500, 0.9);
        app.world_mut().get_mut::<SettlementCore>(sett).unwrap().resources =
            vec![ResourceType::Iron, ResourceType::Grain];

        // Add siege
        app.world_mut().entity_mut(sett).insert(EcsActiveSiege {
            attacker_army_id: 999,
            attacker_faction_id: 888,
            started: SimTime::from_year(99),
            months_elapsed: 3,
            civilian_deaths: 0,
        });

        tick_years(&mut app, 10);

        let building_count: usize = app
            .world_mut()
            .query_filtered::<&SimEntity, With<Building>>()
            .iter(app.world())
            .filter(|s| s.is_alive())
            .count();

        assert_eq!(building_count, 0, "no buildings should be constructed under siege");
    }

    #[test]
    fn capacity_limit_respected() {
        let mut app = setup_app();
        let faction = spawn_faction(&mut app, 1001, 500.0);
        let sett = spawn_settlement(&mut app, 1002, faction, 100, 0.9);
        app.world_mut().get_mut::<SettlementCore>(sett).unwrap().resources =
            vec![ResourceType::Iron, ResourceType::Grain];
        // max buildings = max(1, 100/200) = 1; fill it
        spawn_building(&mut app, 1010, BuildingType::Granary, sett, 1.0, 0);

        tick_years(&mut app, 10);

        let building_count: usize = app
            .world_mut()
            .query_filtered::<&SimEntity, With<Building>>()
            .iter(app.world())
            .filter(|s| s.is_alive())
            .count();

        assert!(
            building_count <= 1,
            "should not exceed building capacity limit, got {building_count}"
        );
    }
}
