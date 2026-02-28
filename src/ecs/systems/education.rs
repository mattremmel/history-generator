//! Education system — migrated from `src/sim/education.rs`.
//!
//! Three yearly systems that compute literacy/education as derived values:
//! 1. `update_settlement_literacy` — building bonuses, scholar density, culture → literacy_rate
//! 2. `update_person_education` — person role + settlement literacy → education
//! 3. `update_faction_literacy` — population-weighted average of settlement literacy

use bevy_app::{App, Plugin};
use bevy_ecs::entity::Entity;
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::Query;

use crate::ecs::components::{
    CultureState, EcsBuildingBonuses, Faction, FactionCore, Person, PersonCore, PersonEducation,
    Settlement, SettlementCore, SettlementCulture, SettlementEducation, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::relationships::{LocatedIn, MemberOf};
use crate::ecs::schedule::{DomainSet, SimTick};
use crate::model::entity_data::Role;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const LIBRARY_LITERACY_BONUS: f64 = 0.15;
const SCHOLAR_GUILD_LITERACY_BONUS: f64 = 0.25;
const TEMPLE_LITERACY_BONUS: f64 = 0.05;
const SCHOLAR_DENSITY_LITERACY_BONUS: f64 = 0.20;
const SCHOLARLY_CULTURE_BONUS: f64 = 0.10;
const SETTLEMENT_LITERACY_DRIFT: f64 = 0.10;
const PERSON_EDUCATION_DRIFT: f64 = 0.15;

fn role_education_factor(role: &Role) -> f64 {
    match role {
        Role::Common => 0.3,
        Role::Artisan => 0.5,
        Role::Merchant => 0.6,
        Role::Warrior => 0.3,
        Role::Elder => 0.4,
        Role::Priest => 0.5,
        Role::Scholar => 1.0,
        Role::Custom(_) => 0.3,
    }
}

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub struct EducationPlugin;

impl Plugin for EducationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SimTick,
            (
                update_settlement_literacy,
                update_person_education,
                update_faction_literacy,
            )
                .chain()
                .run_if(yearly)
                .in_set(DomainSet::Education),
        );
    }
}

// ---------------------------------------------------------------------------
// System 1: Settlement Literacy
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn update_settlement_literacy(
    mut settlements: Query<
        (
            Entity,
            &SimEntity,
            &SettlementCore,
            &EcsBuildingBonuses,
            &SettlementCulture,
            &mut SettlementEducation,
        ),
        With<Settlement>,
    >,
    persons: Query<(&SimEntity, &PersonCore, &LocatedIn), With<Person>>,
    cultures: Query<(&SimEntity, &CultureState)>,
) {
    // Collect updates to avoid borrow issues
    let updates: Vec<(Entity, f64)> = settlements
        .iter()
        .filter(|(_, sim, _, _, _, _)| sim.is_alive())
        .map(|(entity, _, core, bonuses, culture, edu)| {
            // Building-derived literacy targets
            let library_target =
                (bonuses.library / 0.15_f64.max(f64::EPSILON)) * LIBRARY_LITERACY_BONUS;
            let academy_target =
                (bonuses.academy / 0.25_f64.max(f64::EPSILON)) * SCHOLAR_GUILD_LITERACY_BONUS;
            let temple_count = bonuses.temple_knowledge / 0.10_f64.max(f64::EPSILON);
            let temple_target = temple_count * TEMPLE_LITERACY_BONUS;

            // Scholar density
            let mut scholars = 0u32;
            let mut total_npcs = 0u32;
            for (p_sim, p_core, p_loc) in persons.iter() {
                if p_sim.is_alive() && p_loc.0 == entity {
                    total_npcs += 1;
                    if p_core.role == Role::Scholar {
                        scholars += 1;
                    }
                }
            }
            let scholar_density = if total_npcs > 0 {
                scholars as f64 / total_npcs as f64
            } else {
                0.0
            };
            let scholar_target = scholar_density * SCHOLAR_DENSITY_LITERACY_BONUS;

            // Cultural scholarly bonus
            let scholarly_bonus = culture
                .dominant_culture
                .and_then(|cid| {
                    cultures
                        .iter()
                        .find(|(cs, _)| cs.id == cid)
                        .map(|(_, cd)| cd)
                })
                .filter(|cd| {
                    cd.values
                        .contains(&crate::model::cultural_value::CulturalValue::Scholarly)
                })
                .map(|_| SCHOLARLY_CULTURE_BONUS)
                .unwrap_or(0.0);

            let mut target =
                library_target + academy_target + temple_target + scholar_target + scholarly_bonus;
            target *= 0.5 + 0.5 * core.prosperity;
            let target = target.clamp(0.0, 1.0);

            let new_literacy = (edu.literacy_rate
                + (target - edu.literacy_rate) * SETTLEMENT_LITERACY_DRIFT)
                .clamp(0.0, 1.0);

            (entity, new_literacy)
        })
        .collect();

    for (entity, new_literacy) in updates {
        if let Ok((_, _, _, _, _, mut edu)) = settlements.get_mut(entity) {
            edu.literacy_rate = new_literacy;
        }
    }
}

// ---------------------------------------------------------------------------
// System 2: Person Education
// ---------------------------------------------------------------------------

fn update_person_education(
    mut persons: Query<
        (
            Entity,
            &SimEntity,
            &PersonCore,
            &LocatedIn,
            &mut PersonEducation,
        ),
        With<Person>,
    >,
    settlements: Query<&SettlementEducation, With<Settlement>>,
) {
    let updates: Vec<(Entity, f64)> = persons
        .iter()
        .filter(|(_, sim, _, _, _)| sim.is_alive())
        .filter_map(|(entity, _, core, loc, edu)| {
            let settlement_literacy = settlements.get(loc.0).ok()?.literacy_rate;

            let target = if core.role == Role::Scholar {
                (0.70 + 0.20 * settlement_literacy).clamp(0.0, 1.0)
            } else {
                let factor = role_education_factor(&core.role);
                (settlement_literacy * factor).clamp(0.0, 1.0)
            };

            let new_edu =
                (edu.education + (target - edu.education) * PERSON_EDUCATION_DRIFT).clamp(0.0, 1.0);

            Some((entity, new_edu))
        })
        .collect();

    for (entity, new_edu) in updates {
        if let Ok((_, _, _, _, mut edu)) = persons.get_mut(entity) {
            edu.education = new_edu;
        }
    }
}

// ---------------------------------------------------------------------------
// System 3: Faction Literacy (population-weighted aggregate)
// ---------------------------------------------------------------------------

fn update_faction_literacy(
    mut factions: Query<(Entity, &SimEntity, &mut FactionCore), With<Faction>>,
    settlements: Query<
        (&SimEntity, &SettlementCore, &SettlementEducation, &MemberOf),
        With<Settlement>,
    >,
) {
    let faction_updates: Vec<(Entity, f64)> = factions
        .iter()
        .filter(|(_, sim, _)| sim.is_alive())
        .map(|(faction_entity, _, _)| {
            let mut total_pop: u64 = 0;
            let mut weighted_literacy: f64 = 0.0;

            for (s_sim, s_core, s_edu, s_member) in settlements.iter() {
                if s_sim.is_alive() && s_member.0 == faction_entity {
                    let pop = s_core.population as u64;
                    total_pop += pop;
                    weighted_literacy += pop as f64 * s_edu.literacy_rate;
                }
            }

            let faction_literacy = if total_pop > 0 {
                weighted_literacy / total_pop as f64
            } else {
                0.0
            };

            (faction_entity, faction_literacy)
        })
        .collect();

    for (entity, literacy) in faction_updates {
        if let Ok((_, _, mut fc)) = factions.get_mut(entity) {
            fc.literacy_rate = literacy;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::app::build_sim_app;
    use crate::ecs::components::*;
    use crate::ecs::relationships::{LocatedIn, MemberOf};
    use crate::ecs::resources::SimEntityMap;
    use crate::ecs::test_helpers::tick_years;
    use crate::ecs::time::SimTime;

    fn spawn_test_faction(app: &mut App, sim_id: u64, name: &str) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Faction,
                FactionCore::default(),
                FactionDiplomacy::default(),
                FactionMilitary::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_test_settlement(
        app: &mut App,
        sim_id: u64,
        name: &str,
        population: u32,
        prosperity: f64,
    ) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
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
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_test_person(app: &mut App, sim_id: u64, name: &str, role: Role) -> Entity {
        let entity = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Person,
                PersonCore {
                    role,
                    ..PersonCore::default()
                },
                PersonReputation::default(),
                PersonSocial::default(),
                PersonEducation::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(sim_id, entity);
        entity
    }

    fn spawn_test_building(
        app: &mut App,
        sim_id: u64,
        building_type: crate::model::BuildingType,
        settlement: Entity,
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
                    condition: 1.0,
                    level: 0,
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

    fn setup_app() -> App {
        let mut app = build_sim_app(100);
        app.add_plugins(EducationPlugin);
        // Also add building bonus computation so bonuses feed literacy
        app.add_plugins(crate::ecs::systems::buildings::BuildingsPlugin);
        app
    }

    #[test]
    fn literacy_grows_with_library() {
        let mut app = setup_app();

        let faction = spawn_test_faction(&mut app, 1, "Kingdom");
        let region = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "Plains".to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(2, region);

        let settlement = spawn_test_settlement(&mut app, 3, "Town", 500, 0.8);
        app.world_mut()
            .entity_mut(settlement)
            .insert((LocatedIn(region), MemberOf(faction)));

        spawn_test_building(&mut app, 10, crate::model::BuildingType::Temple, settlement);
        spawn_test_building(
            &mut app,
            11,
            crate::model::BuildingType::Library,
            settlement,
        );

        tick_years(&mut app, 10);

        let literacy = app
            .world()
            .get::<SettlementEducation>(settlement)
            .unwrap()
            .literacy_rate;
        assert!(
            literacy > 0.05,
            "literacy should grow with library: got {literacy}"
        );
    }

    #[test]
    fn literacy_grows_faster_with_scholar_guild() {
        // Settlement 1: library only
        let mut app1 = setup_app();
        let f1 = spawn_test_faction(&mut app1, 1, "K1");
        let r1 = app1
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "R1".into(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app1.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(2, r1);
        let s1 = spawn_test_settlement(&mut app1, 3, "Town1", 1000, 0.8);
        app1.world_mut()
            .entity_mut(s1)
            .insert((LocatedIn(r1), MemberOf(f1)));
        spawn_test_building(&mut app1, 10, crate::model::BuildingType::Temple, s1);
        spawn_test_building(&mut app1, 11, crate::model::BuildingType::Library, s1);

        // Settlement 2: library + scholar guild
        let mut app2 = setup_app();
        let f2 = spawn_test_faction(&mut app2, 1, "K2");
        let r2 = app2
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "R2".into(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app2.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(2, r2);
        let s2 = spawn_test_settlement(&mut app2, 3, "Town2", 1000, 0.8);
        app2.world_mut()
            .entity_mut(s2)
            .insert((LocatedIn(r2), MemberOf(f2)));
        spawn_test_building(&mut app2, 10, crate::model::BuildingType::Temple, s2);
        spawn_test_building(&mut app2, 11, crate::model::BuildingType::Library, s2);
        spawn_test_building(&mut app2, 12, crate::model::BuildingType::ScholarGuild, s2);

        tick_years(&mut app1, 20);
        tick_years(&mut app2, 20);

        let lit1 = app1
            .world()
            .get::<SettlementEducation>(s1)
            .unwrap()
            .literacy_rate;
        let lit2 = app2
            .world()
            .get::<SettlementEducation>(s2)
            .unwrap()
            .literacy_rate;
        assert!(
            lit2 > lit1,
            "scholar guild should boost literacy: lib_only={lit1}, with_guild={lit2}"
        );
    }

    #[test]
    fn convergence_is_gradual() {
        let mut app = setup_app();
        let f = spawn_test_faction(&mut app, 1, "K");
        let r = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "R".into(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app.world_mut().resource_mut::<SimEntityMap>().insert(2, r);
        let s = spawn_test_settlement(&mut app, 3, "Town", 500, 0.8);
        app.world_mut()
            .entity_mut(s)
            .insert((LocatedIn(r), MemberOf(f)));
        spawn_test_building(&mut app, 10, crate::model::BuildingType::Temple, s);
        spawn_test_building(&mut app, 11, crate::model::BuildingType::Library, s);

        tick_years(&mut app, 1);

        let literacy = app
            .world()
            .get::<SettlementEducation>(s)
            .unwrap()
            .literacy_rate;
        assert!(
            literacy < 0.5,
            "convergence should be gradual: got {literacy} after 1 year"
        );
    }

    #[test]
    fn literacy_declines_without_infrastructure() {
        let mut app = setup_app();
        let f = spawn_test_faction(&mut app, 1, "K");
        let r = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "R".into(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app.world_mut().resource_mut::<SimEntityMap>().insert(2, r);
        let s = spawn_test_settlement(&mut app, 3, "Town", 500, 0.8);
        app.world_mut()
            .entity_mut(s)
            .insert((LocatedIn(r), MemberOf(f)));

        // Start with high literacy but no buildings
        app.world_mut()
            .get_mut::<SettlementEducation>(s)
            .unwrap()
            .literacy_rate = 0.8;

        tick_years(&mut app, 20);

        let literacy = app
            .world()
            .get::<SettlementEducation>(s)
            .unwrap()
            .literacy_rate;
        assert!(
            literacy < 0.5,
            "literacy should decline without infrastructure: got {literacy}"
        );
    }

    #[test]
    fn scholars_have_higher_education_than_common() {
        let mut app = setup_app();
        let f = spawn_test_faction(&mut app, 1, "K");
        let r = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "R".into(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app.world_mut().resource_mut::<SimEntityMap>().insert(2, r);
        let s = spawn_test_settlement(&mut app, 3, "Town", 500, 0.8);
        app.world_mut()
            .entity_mut(s)
            .insert((LocatedIn(r), MemberOf(f)));
        spawn_test_building(&mut app, 10, crate::model::BuildingType::Temple, s);
        spawn_test_building(&mut app, 11, crate::model::BuildingType::Library, s);
        app.world_mut()
            .get_mut::<SettlementEducation>(s)
            .unwrap()
            .literacy_rate = 0.5;

        let scholar = spawn_test_person(&mut app, 20, "Scholar1", Role::Scholar);
        app.world_mut()
            .entity_mut(scholar)
            .insert((LocatedIn(s), MemberOf(f)));
        let commoner = spawn_test_person(&mut app, 21, "Common1", Role::Common);
        app.world_mut()
            .entity_mut(commoner)
            .insert((LocatedIn(s), MemberOf(f)));

        tick_years(&mut app, 20);

        let scholar_edu = app
            .world()
            .get::<PersonEducation>(scholar)
            .unwrap()
            .education;
        let common_edu = app
            .world()
            .get::<PersonEducation>(commoner)
            .unwrap()
            .education;
        assert!(
            scholar_edu > common_edu,
            "scholars should have higher education: scholar={scholar_edu}, common={common_edu}"
        );
    }

    #[test]
    fn faction_literacy_is_population_weighted() {
        let mut app = build_sim_app(100);
        app.add_plugins(EducationPlugin);

        let faction = spawn_test_faction(&mut app, 1, "Kingdom");
        let r = app
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "R".into(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app.world_mut().resource_mut::<SimEntityMap>().insert(2, r);

        // Large settlement with low literacy
        let big = spawn_test_settlement(&mut app, 3, "BigTown", 1000, 0.5);
        app.world_mut()
            .entity_mut(big)
            .insert((LocatedIn(r), MemberOf(faction)));
        app.world_mut()
            .get_mut::<SettlementEducation>(big)
            .unwrap()
            .literacy_rate = 0.1;

        // Small settlement with high literacy
        let small = spawn_test_settlement(&mut app, 4, "SmallTown", 100, 0.5);
        app.world_mut()
            .entity_mut(small)
            .insert((LocatedIn(r), MemberOf(faction)));
        app.world_mut()
            .get_mut::<SettlementEducation>(small)
            .unwrap()
            .literacy_rate = 0.9;

        tick_years(&mut app, 1);

        let faction_lit = app
            .world()
            .get::<FactionCore>(faction)
            .unwrap()
            .literacy_rate;
        // Weighted: closer to 0.1 (big town) than 0.9 (small town)
        assert!(
            faction_lit < 0.5,
            "faction literacy should be weighted toward larger settlement: got {faction_lit}"
        );
    }

    #[test]
    fn prosperity_modulates_literacy_target() {
        // High prosperity
        let mut app1 = setup_app();
        let f1 = spawn_test_faction(&mut app1, 1, "K1");
        let r1 = app1
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "R1".into(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app1.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(2, r1);
        let s1 = spawn_test_settlement(&mut app1, 3, "Rich", 500, 1.0);
        app1.world_mut()
            .entity_mut(s1)
            .insert((LocatedIn(r1), MemberOf(f1)));
        spawn_test_building(&mut app1, 10, crate::model::BuildingType::Temple, s1);
        spawn_test_building(&mut app1, 11, crate::model::BuildingType::Library, s1);

        // Low prosperity
        let mut app2 = setup_app();
        let f2 = spawn_test_faction(&mut app2, 1, "K2");
        let r2 = app2
            .world_mut()
            .spawn((
                SimEntity {
                    id: 2,
                    name: "R2".into(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Region,
                RegionState::default(),
            ))
            .id();
        app2.world_mut()
            .resource_mut::<SimEntityMap>()
            .insert(2, r2);
        let s2 = spawn_test_settlement(&mut app2, 3, "Poor", 500, 0.1);
        app2.world_mut()
            .entity_mut(s2)
            .insert((LocatedIn(r2), MemberOf(f2)));
        spawn_test_building(&mut app2, 10, crate::model::BuildingType::Temple, s2);
        spawn_test_building(&mut app2, 11, crate::model::BuildingType::Library, s2);

        tick_years(&mut app1, 20);
        tick_years(&mut app2, 20);

        let lit_rich = app1
            .world()
            .get::<SettlementEducation>(s1)
            .unwrap()
            .literacy_rate;
        let lit_poor = app2
            .world()
            .get::<SettlementEducation>(s2)
            .unwrap()
            .literacy_rate;
        assert!(
            lit_rich > lit_poor,
            "high prosperity should lead to higher literacy: rich={lit_rich}, poor={lit_poor}"
        );
    }
}
