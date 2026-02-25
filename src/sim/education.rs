use super::context::TickContext;
use super::system::{SimSystem, TickFrequency};
use crate::model::entity::EntityKind;
use crate::model::entity_data::Role;
use crate::model::relationship::RelationshipKind;
use crate::sim::helpers;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Library bonus to literacy target per effective library level.
const LIBRARY_LITERACY_BONUS: f64 = 0.15;
/// Scholar guild bonus to literacy target per effective academy level.
const SCHOLAR_GUILD_LITERACY_BONUS: f64 = 0.25;
/// Temple bonus to literacy target (per approximate temple count, derived from temple_knowledge / 0.10).
const TEMPLE_LITERACY_BONUS: f64 = 0.05;
/// Multiplier on scholar density for literacy target.
const SCHOLAR_DENSITY_LITERACY_BONUS: f64 = 0.20;
/// Bonus to literacy target if settlement's dominant culture has the Scholarly value.
const SCHOLARLY_CULTURE_BONUS: f64 = 0.10;
/// Rate at which settlement literacy converges to target per year (10%).
const SETTLEMENT_LITERACY_DRIFT: f64 = 0.10;
/// Rate at which person education converges to target per year (15%).
const PERSON_EDUCATION_DRIFT: f64 = 0.15;

/// Role-based factor for education target (multiplied by settlement literacy).
fn role_education_factor(role: &Role) -> f64 {
    match role {
        Role::Common => 0.3,
        Role::Artisan => 0.5,
        Role::Merchant => 0.6,
        Role::Warrior => 0.3,
        Role::Elder => 0.4,
        Role::Priest => 0.5,
        Role::Scholar => 1.0, // Scholars handled separately
        Role::Custom(_) => 0.3,
    }
}

pub struct EducationSystem;

impl SimSystem for EducationSystem {
    fn name(&self) -> &str {
        "education"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        update_settlement_literacy(ctx);
        update_person_education(ctx);
        update_faction_literacy(ctx);
    }
}

// ---------------------------------------------------------------------------
// Phase 1: Settlement Literacy
// ---------------------------------------------------------------------------

fn update_settlement_literacy(ctx: &mut TickContext) {
    struct LiteracyUpdate {
        id: u64,
        target: f64,
        old_literacy: f64,
    }

    let updates: Vec<LiteracyUpdate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            let sid = e.id;

            // Building bonuses — derive effective level counts from bonus values
            // library bonus = 0.15 * eff per library -> level = bonus / 0.15
            // academy bonus = 0.25 * eff per guild  -> level = bonus / 0.25
            // temple_knowledge = 0.10 * eff per temple -> count ~ bonus / 0.10
            let library_target =
                (sd.building_bonuses.library / 0.15_f64.max(f64::EPSILON)) * LIBRARY_LITERACY_BONUS;
            let academy_target = (sd.building_bonuses.academy / 0.25_f64.max(f64::EPSILON))
                * SCHOLAR_GUILD_LITERACY_BONUS;
            let temple_count = sd.building_bonuses.temple_knowledge / 0.10_f64.max(f64::EPSILON);
            let temple_target = temple_count * TEMPLE_LITERACY_BONUS;

            // Scholar density
            let scholars = ctx
                .world
                .entities
                .values()
                .filter(|p| {
                    p.kind == EntityKind::Person
                        && p.end.is_none()
                        && p.has_active_rel(RelationshipKind::LocatedIn, sid)
                        && p.data
                            .as_person()
                            .is_some_and(|pd| pd.role == Role::Scholar)
                })
                .count();
            let total_npcs = ctx
                .world
                .entities
                .values()
                .filter(|p| {
                    p.kind == EntityKind::Person
                        && p.end.is_none()
                        && p.has_active_rel(RelationshipKind::LocatedIn, sid)
                })
                .count();
            let scholar_density = if total_npcs > 0 {
                scholars as f64 / total_npcs as f64
            } else {
                0.0
            };
            let scholar_target = scholar_density * SCHOLAR_DENSITY_LITERACY_BONUS;

            // Cultural scholarly bonus
            let scholarly_bonus = sd
                .dominant_culture
                .and_then(|cid| ctx.world.entities.get(&cid))
                .and_then(|ce| ce.data.as_culture())
                .filter(|cd| {
                    cd.values
                        .contains(&crate::model::cultural_value::CulturalValue::Scholarly)
                })
                .map(|_| SCHOLARLY_CULTURE_BONUS)
                .unwrap_or(0.0);

            let mut target =
                library_target + academy_target + temple_target + scholar_target + scholarly_bonus;

            // Scale by prosperity
            target *= 0.5 + 0.5 * sd.prosperity;

            let target = target.clamp(0.0, 1.0);

            Some(LiteracyUpdate {
                id: sid,
                target,
                old_literacy: sd.literacy_rate,
            })
        })
        .collect();

    for u in updates {
        let new_literacy = u.old_literacy + (u.target - u.old_literacy) * SETTLEMENT_LITERACY_DRIFT;
        let new_literacy = new_literacy.clamp(0.0, 1.0);
        ctx.world.settlement_mut(u.id).literacy_rate = new_literacy;
    }
}

// ---------------------------------------------------------------------------
// Phase 2: Person Education
// ---------------------------------------------------------------------------

fn update_person_education(ctx: &mut TickContext) {
    struct EduUpdate {
        id: u64,
        target: f64,
        old_education: f64,
    }

    let updates: Vec<EduUpdate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Person && e.end.is_none())
        .filter_map(|e| {
            let pd = e.data.as_person()?;
            let settlement_id = e.active_rel(RelationshipKind::LocatedIn)?;
            let settlement_literacy = ctx
                .world
                .entities
                .get(&settlement_id)
                .and_then(|se| se.data.as_settlement())
                .map(|sd| sd.literacy_rate)
                .unwrap_or(0.0);

            let target = if pd.role == Role::Scholar {
                (0.70 + 0.20 * settlement_literacy).clamp(0.0, 1.0)
            } else {
                let factor = role_education_factor(&pd.role);
                (settlement_literacy * factor).clamp(0.0, 1.0)
            };

            Some(EduUpdate {
                id: e.id,
                target,
                old_education: pd.education,
            })
        })
        .collect();

    for u in updates {
        let new_edu = u.old_education + (u.target - u.old_education) * PERSON_EDUCATION_DRIFT;
        let new_edu = new_edu.clamp(0.0, 1.0);
        ctx.world.person_mut(u.id).education = new_edu;
    }
}

// ---------------------------------------------------------------------------
// Phase 3: Faction Literacy (derived aggregate)
// ---------------------------------------------------------------------------

fn update_faction_literacy(ctx: &mut TickContext) {
    struct FactionLiteracy {
        id: u64,
        literacy: f64,
    }

    let faction_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    let mut updates: Vec<FactionLiteracy> = Vec::new();

    for fid in &faction_ids {
        let settlements = helpers::faction_settlements(ctx.world, *fid);
        let mut total_pop: u64 = 0;
        let mut weighted_literacy: f64 = 0.0;

        for sid in &settlements {
            if let Some(sd) = ctx
                .world
                .entities
                .get(sid)
                .and_then(|e| e.data.as_settlement())
            {
                let pop = sd.population as u64;
                total_pop += pop;
                weighted_literacy += pop as f64 * sd.literacy_rate;
            }
        }

        let faction_literacy = if total_pop > 0 {
            weighted_literacy / total_pop as f64
        } else {
            0.0
        };

        updates.push(FactionLiteracy {
            id: *fid,
            literacy: faction_literacy,
        });
    }

    for u in updates {
        ctx.world.faction_mut(u.id).literacy_rate = u.literacy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entity_data::BuildingType;
    use crate::scenario::Scenario;
    use crate::testutil;

    #[test]
    fn literacy_grows_with_library() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.8);
        s.add_building(BuildingType::Temple, setup.settlement);
        s.add_building(BuildingType::Library, setup.settlement);
        let mut world = s.build();

        // Run buildings first to compute bonuses, then education
        for year in 100..110 {
            testutil::tick_system(&mut world, &mut crate::sim::BuildingSystem, year, 42);
            testutil::tick_system(&mut world, &mut EducationSystem, year, 42);
        }

        let literacy = world.settlement(setup.settlement).literacy_rate;
        assert!(
            literacy > 0.05,
            "literacy should grow with library: got {literacy}"
        );
    }

    #[test]
    fn literacy_grows_faster_with_scholar_guild() {
        // Settlement with library only
        let mut s1 = Scenario::at_year(100);
        let setup1 = s1.add_settlement_standalone("Town1");
        let _ = s1
            .settlement_mut(setup1.settlement)
            .population(1000)
            .prosperity(0.8);
        s1.add_building(BuildingType::Temple, setup1.settlement);
        s1.add_building(BuildingType::Library, setup1.settlement);
        let mut world1 = s1.build();

        // Settlement with library + scholar guild
        let mut s2 = Scenario::at_year(100);
        let setup2 = s2.add_settlement_standalone("Town2");
        let _ = s2
            .settlement_mut(setup2.settlement)
            .population(1000)
            .prosperity(0.8);
        s2.add_building(BuildingType::Temple, setup2.settlement);
        s2.add_building(BuildingType::Library, setup2.settlement);
        s2.add_building(BuildingType::ScholarGuild, setup2.settlement);
        let mut world2 = s2.build();

        for year in 100..120 {
            testutil::tick_system(&mut world1, &mut crate::sim::BuildingSystem, year, 42);
            testutil::tick_system(&mut world1, &mut EducationSystem, year, 42);
            testutil::tick_system(&mut world2, &mut crate::sim::BuildingSystem, year, 42);
            testutil::tick_system(&mut world2, &mut EducationSystem, year, 42);
        }

        let lit1 = world1.settlement(setup1.settlement).literacy_rate;
        let lit2 = world2.settlement(setup2.settlement).literacy_rate;
        assert!(
            lit2 > lit1,
            "scholar guild should boost literacy: lib_only={lit1}, with_guild={lit2}"
        );
    }

    #[test]
    fn convergence_is_gradual() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.8);
        s.add_building(BuildingType::Temple, setup.settlement);
        s.add_building(BuildingType::Library, setup.settlement);
        let mut world = s.build();

        // Run buildings to compute bonuses
        testutil::tick_system(&mut world, &mut crate::sim::BuildingSystem, 100, 42);
        testutil::tick_system(&mut world, &mut EducationSystem, 100, 42);

        let literacy_y1 = world.settlement(setup.settlement).literacy_rate;
        // Should not jump to target immediately — should be < 0.5 after 1 year
        assert!(
            literacy_y1 < 0.5,
            "convergence should be gradual: got {literacy_y1} after 1 year"
        );
    }

    #[test]
    fn literacy_declines_without_infrastructure() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.8);
        // Start with high literacy but no buildings
        s.modify_settlement(setup.settlement, |sd| sd.literacy_rate = 0.8);
        let mut world = s.build();

        for year in 100..120 {
            testutil::tick_system(&mut world, &mut crate::sim::BuildingSystem, year, 42);
            testutil::tick_system(&mut world, &mut EducationSystem, year, 42);
        }

        let literacy = world.settlement(setup.settlement).literacy_rate;
        assert!(
            literacy < 0.5,
            "literacy should decline without infrastructure: got {literacy}"
        );
    }

    #[test]
    fn scholars_have_higher_education_than_common() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let _ = s
            .settlement_mut(setup.settlement)
            .population(500)
            .prosperity(0.8);
        s.add_building(BuildingType::Temple, setup.settlement);
        s.add_building(BuildingType::Library, setup.settlement);
        s.modify_settlement(setup.settlement, |sd| sd.literacy_rate = 0.5);

        let scholar = s
            .person_in("Scholar1", setup.faction, setup.settlement)
            .role(crate::model::entity_data::Role::Scholar)
            .id();
        let commoner = s
            .person_in("Common1", setup.faction, setup.settlement)
            .role(crate::model::entity_data::Role::Common)
            .id();

        let mut world = s.build();

        for year in 100..120 {
            testutil::tick_system(&mut world, &mut crate::sim::BuildingSystem, year, 42);
            testutil::tick_system(&mut world, &mut EducationSystem, year, 42);
        }

        let scholar_edu = world.person(scholar).education;
        let common_edu = world.person(commoner).education;
        assert!(
            scholar_edu > common_edu,
            "scholars should have higher education: scholar={scholar_edu}, common={common_edu}"
        );
    }

    #[test]
    fn faction_literacy_is_population_weighted() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Plains");
        let faction = s.add_faction("Kingdom");

        // Large settlement with low literacy
        let big = s.add_settlement("BigTown", faction, region);
        let _ = s.settlement_mut(big).population(1000).prosperity(0.5);
        s.modify_settlement(big, |sd| sd.literacy_rate = 0.1);

        // Small settlement with high literacy
        let small = s.add_settlement("SmallTown", faction, region);
        let _ = s.settlement_mut(small).population(100).prosperity(0.5);
        s.modify_settlement(small, |sd| sd.literacy_rate = 0.9);

        let mut world = s.build();

        testutil::tick_system(&mut world, &mut EducationSystem, 100, 42);

        let faction_lit = world.faction(faction).literacy_rate;
        // Weighted: (1000*0.1 + 100*0.9) / 1100 = (100+90)/1100 ≈ 0.1727
        // But education system converges literacy first, so values will shift slightly
        // The key assertion: faction literacy should be closer to 0.1 (big town) than 0.9 (small town)
        assert!(
            faction_lit < 0.5,
            "faction literacy should be weighted toward larger settlement: got {faction_lit}"
        );
    }

    #[test]
    fn prosperity_modulates_literacy_target() {
        // High prosperity
        let mut s1 = Scenario::at_year(100);
        let setup1 = s1.add_settlement_standalone("Rich");
        let _ = s1
            .settlement_mut(setup1.settlement)
            .population(500)
            .prosperity(1.0);
        s1.add_building(BuildingType::Temple, setup1.settlement);
        s1.add_building(BuildingType::Library, setup1.settlement);
        let mut world1 = s1.build();

        // Low prosperity
        let mut s2 = Scenario::at_year(100);
        let setup2 = s2.add_settlement_standalone("Poor");
        let _ = s2
            .settlement_mut(setup2.settlement)
            .population(500)
            .prosperity(0.1);
        s2.add_building(BuildingType::Temple, setup2.settlement);
        s2.add_building(BuildingType::Library, setup2.settlement);
        let mut world2 = s2.build();

        for year in 100..120 {
            testutil::tick_system(&mut world1, &mut crate::sim::BuildingSystem, year, 42);
            testutil::tick_system(&mut world1, &mut EducationSystem, year, 42);
            testutil::tick_system(&mut world2, &mut crate::sim::BuildingSystem, year, 42);
            testutil::tick_system(&mut world2, &mut EducationSystem, year, 42);
        }

        let lit_rich = world1.settlement(setup1.settlement).literacy_rate;
        let lit_poor = world2.settlement(setup2.settlement).literacy_rate;
        assert!(
            lit_rich > lit_poor,
            "high prosperity should lead to higher literacy: rich={lit_rich}, poor={lit_poor}"
        );
    }
}
