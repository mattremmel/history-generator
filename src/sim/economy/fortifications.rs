use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp};
use crate::sim::context::TickContext;

const FORT_PALISADE_POP: u32 = 150;
const FORT_PALISADE_COST: f64 = 20.0;
const FORT_STONE_POP: u32 = 500;
const FORT_STONE_COST: f64 = 100.0;
const FORT_FORTRESS_POP: u32 = 1500;
const FORT_FORTRESS_COST: f64 = 300.0;

pub(super) fn update_fortifications(
    ctx: &mut TickContext,
    time: SimTimestamp,
    current_year: u32,
    year_event: u64,
) {
    struct FortCandidate {
        settlement_id: u64,
        faction_id: u64,
        population: u32,
        current_level: u8,
    }

    let candidates: Vec<FortCandidate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            // Cannot build while under siege
            if sd.active_siege.is_some() {
                return None;
            }
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            Some(FortCandidate {
                settlement_id: e.id,
                faction_id,
                population: sd.population,
                current_level: sd.fortification_level,
            })
        })
        .collect();

    for c in candidates {
        let (needed_pop, cost, new_level) = match c.current_level {
            0 => (FORT_PALISADE_POP, FORT_PALISADE_COST, 1u8),
            1 => (FORT_STONE_POP, FORT_STONE_COST, 2u8),
            2 => (FORT_FORTRESS_POP, FORT_FORTRESS_COST, 3u8),
            _ => continue,
        };

        if c.population < needed_pop {
            continue;
        }

        // Check faction treasury
        let treasury = ctx
            .world
            .entities
            .get(&c.faction_id)
            .and_then(|e| e.data.as_faction())
            .map(|f| f.treasury)
            .unwrap_or(0.0);
        if treasury < cost {
            continue;
        }

        // Deduct from faction treasury
        {
            let entity = ctx.world.entities.get_mut(&c.faction_id).unwrap();
            let fd = entity.data.as_faction_mut().unwrap();
            fd.treasury -= cost;
        }

        // Upgrade fortification
        {
            let entity = ctx.world.entities.get_mut(&c.settlement_id).unwrap();
            let sd = entity.data.as_settlement_mut().unwrap();
            sd.fortification_level = new_level;
        }

        let settlement_name = ctx
            .world
            .entities
            .get(&c.settlement_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let fort_name = match new_level {
            1 => "a palisade",
            2 => "stone walls",
            3 => "a fortress",
            _ => "fortifications",
        };

        let ev = ctx.world.add_caused_event(
            EventKind::Custom("construction".to_string()),
            time,
            format!("{settlement_name} built {fort_name} in year {current_year}"),
            year_event,
        );
        ctx.world
            .add_event_participant(ev, c.settlement_id, ParticipantRole::Subject);
        ctx.world.record_change(
            c.settlement_id,
            ev,
            "fortification_level",
            serde_json::json!(c.current_level),
            serde_json::json!(new_level),
        );
        ctx.world.record_change(
            c.faction_id,
            ev,
            "treasury",
            serde_json::json!(treasury),
            serde_json::json!(treasury - cost),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entity_data::ActiveSiege;
    use crate::scenario::Scenario;
    use crate::testutil::{assert_approx, get_faction, get_settlement};
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn scenario_fortification_with_sufficient_pop_and_treasury() {
        let mut s = Scenario::at_year(10);
        let setup = s.add_settlement_standalone("BigTown");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement).population(600);
        let settlement = setup.settlement;
        let faction = setup.faction;
        let mut world = s.build();

        let ev = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        update_fortifications(&mut ctx, SimTimestamp::from_year(10), 10, ev);

        assert_eq!(get_settlement(ctx.world, settlement).fortification_level, 1);
        assert_approx(
            get_faction(ctx.world, faction).treasury,
            480.0,
            0.01,
            "treasury after palisade",
        );
    }

    #[test]
    fn scenario_no_fortification_under_siege() {
        let mut s = Scenario::at_year(10);
        let setup = s.add_settlement_standalone("SiegedTown");
        s.faction_mut(setup.faction).treasury(500.0);
        s.settlement_mut(setup.settlement)
            .population(600)
            .with(|sd| {
                sd.active_siege = Some(ActiveSiege {
                    attacker_army_id: 999,
                    attacker_faction_id: 888,
                    started_year: 10,
                    started_month: 1,
                    months_elapsed: 2,
                    civilian_deaths: 0,
                });
            });
        let settlement = setup.settlement;
        let faction = setup.faction;
        let mut world = s.build();

        let ev = world.add_event(
            EventKind::Custom("test".to_string()),
            world.current_time,
            "test".to_string(),
        );
        let mut rng = SmallRng::seed_from_u64(42);
        let mut signals = Vec::new();
        let mut ctx = TickContext {
            world: &mut world,
            rng: &mut rng,
            signals: &mut signals,
            inbox: &[],
        };

        update_fortifications(&mut ctx, SimTimestamp::from_year(10), 10, ev);

        assert_eq!(get_settlement(ctx.world, settlement).fortification_level, 0);
        assert_approx(
            get_faction(ctx.world, faction).treasury,
            500.0,
            0.01,
            "treasury unchanged",
        );
    }
}
