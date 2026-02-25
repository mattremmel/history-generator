//! Mercenary company lifecycle: formation, hiring, payment, desertion, disbanding.

use std::collections::BTreeSet;

use rand::Rng;

use crate::model::entity::EntityKind;
use crate::model::entity_data::{EntityData, GovernmentType, Role};
use crate::model::event::EventKind;
use crate::model::relationship::RelationshipKind;
use crate::model::timestamp::SimTimestamp;
use crate::model::traits::Trait;
use crate::sim::context::TickContext;
use crate::sim::helpers;
use crate::sim::loyalty;
use crate::sim::signal::{Signal, SignalKind};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Formation
const POST_WAR_FORMATION_CHANCE: f64 = 0.25;
const POST_WAR_MIN_STRENGTH: u32 = 25;
const SPONTANEOUS_FORMATION_CHANCE: f64 = 0.03;
const SPONTANEOUS_MIN_ADJACENT_FACTIONS: usize = 2;
const SPONTANEOUS_STRENGTH_MIN: u32 = 30;
const SPONTANEOUS_STRENGTH_MAX: u32 = 60;

// Hiring
const HIRE_TREASURY_MIN: f64 = 50.0;
const HIRE_SIGNING_BONUS_MONTHS: f64 = 3.0;
const HIRE_AFFORDABILITY_MONTHS: f64 = 6.0;
const HIRE_POWER_RATIO_MAX: f64 = 1.5;
const HIRE_BFS_MAX_HOPS: usize = 3;
const HIRE_INITIAL_LOYALTY: f64 = 0.7;

// Payment
const PAYMENT_LOYALTY_GAIN: f64 = 0.05;
const NONPAYMENT_LOYALTY_LOSS: f64 = 0.15;
const MERCENARY_WAGE_PER_STRENGTH: f64 = 1.0;

// Desertion
const DESERTION_LOYALTY_THRESHOLD: f64 = 0.3;
const DESERTION_CHANCE_FACTOR: f64 = 0.5;
const SIDE_SWITCH_CHANCE: f64 = 0.4;

// Disbanding
const DISBAND_MIN_STRENGTH: u32 = 10;
const IDLE_YEARS_BEFORE_DISBAND: u32 = 15;
const IDLE_DISBAND_CHANCE: f64 = 0.20;

// Name generation
const MERC_PREFIXES: &[&str] = &[
    "Iron", "Crimson", "Black", "Golden", "Silver", "Storm", "Shadow", "Blood",
    "Steel", "Bronze", "Red", "White", "Thunder", "Ember", "Frost",
];
const MERC_SUFFIXES: &[&str] = &[
    "Hawks", "Lances", "Wolves", "Shields", "Blades", "Company", "Guard",
    "Band", "Legion", "Swords", "Fangs", "Talons", "Riders", "Axes",
];

// ---------------------------------------------------------------------------
// Name generation
// ---------------------------------------------------------------------------

fn generate_merc_name(rng: &mut dyn rand::RngCore) -> String {
    let prefix = MERC_PREFIXES[rng.random_range(0..MERC_PREFIXES.len())];
    let suffix = MERC_SUFFIXES[rng.random_range(0..MERC_SUFFIXES.len())];
    format!("{prefix} {suffix}")
}

// ---------------------------------------------------------------------------
// Formation
// ---------------------------------------------------------------------------

/// Post-war formation: when a war ends, losing side's disbanded army may form a mercenary company.
pub(super) fn handle_post_war_formation(
    ctx: &mut TickContext,
    time: SimTimestamp,
    _loser_id: u64,
    loser_region: u64,
    disbanded_strength: u32,
    war_event_id: u64,
) {
    if disbanded_strength < POST_WAR_MIN_STRENGTH {
        return;
    }
    if ctx.rng.random::<f64>() >= POST_WAR_FORMATION_CHANCE {
        return;
    }

    create_mercenary_company(ctx, time, loser_region, disbanded_strength, war_event_id);
}

/// Spontaneous formation: border regions (adjacent to 2+ factions) may spawn companies.
pub(super) fn check_spontaneous_formation(ctx: &mut TickContext, time: SimTimestamp) {
    let current_year = time.year();

    // Collect all regions
    let regions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Region && e.end.is_none())
        .map(|e| e.id)
        .collect();

    for region_id in regions {
        // Count distinct factions with settlements in this region or adjacent regions
        let mut nearby_factions = BTreeSet::new();
        let mut check_regions = vec![region_id];
        check_regions.extend(helpers::adjacent_regions(ctx.world, region_id));

        for &rid in &check_regions {
            for e in ctx.world.entities.values() {
                if e.kind == EntityKind::Settlement
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::LocatedIn, rid)
                    && let Some(fid) = e.active_rel(RelationshipKind::MemberOf)
                    && !helpers::is_non_state_faction(ctx.world, fid)
                {
                    nearby_factions.insert(fid);
                }
            }
        }

        if nearby_factions.len() < SPONTANEOUS_MIN_ADJACENT_FACTIONS {
            continue;
        }

        // Already has a mercenary company?
        let has_merc = ctx.world.entities.values().any(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::LocatedIn, region_id)
                && e.data.as_army().is_some_and(|ad| ad.is_mercenary)
        });
        if has_merc {
            continue;
        }

        if ctx.rng.random::<f64>() >= SPONTANEOUS_FORMATION_CHANCE {
            continue;
        }

        let strength = ctx
            .rng
            .random_range(SPONTANEOUS_STRENGTH_MIN..=SPONTANEOUS_STRENGTH_MAX);

        let ev = ctx.world.add_event(
            EventKind::MercenaryFormed,
            time,
            format!("A mercenary company formed in year {current_year}"),
        );

        create_mercenary_company(ctx, time, region_id, strength, ev);
    }
}

fn create_mercenary_company(
    ctx: &mut TickContext,
    time: SimTimestamp,
    region_id: u64,
    strength: u32,
    cause_event_id: u64,
) {
    let current_year = time.year();
    let company_name = generate_merc_name(ctx.rng);

    let ev = cause_event_id;

    // Create faction using default_for_kind + mutation
    let mut faction_data = EntityData::default_for_kind(EntityKind::Faction);
    let EntityData::Faction(ref mut fd) = faction_data else {
        unreachable!()
    };
    fd.government_type = GovernmentType::MercenaryCompany;
    fd.mercenary_wage = MERCENARY_WAGE_PER_STRENGTH;
    fd.treasury = 0.0;
    fd.stability = 0.5;
    fd.happiness = 0.5;
    fd.legitimacy = 0.0;

    let faction_id = ctx.world.add_entity(
        EntityKind::Faction,
        company_name.clone(),
        Some(time),
        faction_data,
        ev,
    );

    // Create army
    let mut army_data = EntityData::default_for_kind(EntityKind::Army);
    let EntityData::Army(ref mut ad) = army_data else {
        unreachable!()
    };
    ad.strength = strength;
    ad.morale = 0.8;
    ad.supply = 3.0;
    ad.faction_id = faction_id;
    ad.home_region_id = region_id;
    ad.starting_strength = strength;
    ad.is_mercenary = true;

    let army_id = ctx.world.add_entity(
        EntityKind::Army,
        format!("{company_name} Company"),
        Some(time),
        army_data,
        ev,
    );
    ctx.world
        .add_relationship(army_id, faction_id, RelationshipKind::MemberOf, time, ev);
    ctx.world
        .add_relationship(army_id, region_id, RelationshipKind::LocatedIn, time, ev);

    // Create leader
    let leader_name = crate::sim::names::generate_unique_person_name(ctx.world, ctx.rng);
    let mut leader_data = EntityData::default_for_kind(EntityKind::Person);
    let EntityData::Person(ref mut pd) = leader_data else {
        unreachable!()
    };
    pd.born = SimTimestamp::from_year(current_year.saturating_sub(ctx.rng.random_range(20..40)));
    pd.role = Role::Warrior;
    pd.traits = vec![Trait::Ambitious];

    let leader_id = ctx.world.add_entity(
        EntityKind::Person,
        leader_name,
        Some(time),
        leader_data,
        ev,
    );
    ctx.world
        .add_relationship(leader_id, faction_id, RelationshipKind::MemberOf, time, ev);
    ctx.world.add_relationship(
        leader_id,
        faction_id,
        RelationshipKind::LeaderOf,
        time,
        ev,
    );

    ctx.world.add_event_participant(ev, faction_id, crate::model::ParticipantRole::Subject);
    ctx.world.add_event_participant(ev, army_id, crate::model::ParticipantRole::Object);
    ctx.world.add_event_participant(ev, leader_id, crate::model::ParticipantRole::Instigator);
}

// ---------------------------------------------------------------------------
// Hiring
// ---------------------------------------------------------------------------

/// Yearly: factions at war consider hiring available mercenaries.
pub(super) fn check_hiring(ctx: &mut TickContext, time: SimTimestamp) {
    // Collect factions currently at war
    let at_war_factions: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && !helpers::is_non_state_faction(ctx.world, e.id)
                && e.relationships
                    .iter()
                    .any(|r| r.kind == RelationshipKind::AtWar && r.is_active())
        })
        .map(|e| e.id)
        .collect();

    // Collect available (unhired) mercenary companies
    let available_mercs: Vec<(u64, u64, u64)> = ctx  // (faction_id, army_id, region_id)
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.data
                    .as_faction()
                    .is_some_and(|fd| fd.government_type == GovernmentType::MercenaryCompany)
                && !e.relationships
                    .iter()
                    .any(|r| r.kind == RelationshipKind::HiredBy && r.is_active())
        })
        .filter_map(|e| {
            // Find this merc's army and its region
            let army = ctx.world.entities.values().find(|a| {
                a.kind == EntityKind::Army
                    && a.end.is_none()
                    && a.has_active_rel(RelationshipKind::MemberOf, e.id)
            })?;
            let region = army.active_rel(RelationshipKind::LocatedIn)?;
            Some((e.id, army.id, region))
        })
        .collect();

    for &faction_id in &at_war_factions {
        // Already has hired mercs?
        let already_hired = ctx
            .world
            .entities
            .values()
            .any(|e| {
                e.kind == EntityKind::Faction
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::HiredBy, faction_id)
            });
        if already_hired {
            continue;
        }

        let treasury = ctx
            .world
            .entities
            .get(&faction_id)
            .and_then(|e| e.data.as_faction())
            .map(|fd| fd.treasury)
            .unwrap_or(0.0);
        if treasury < HIRE_TREASURY_MIN {
            continue;
        }

        // Check military power ratio
        let own_strength = faction_military_strength(ctx.world, faction_id);
        let enemy_strength = faction_enemy_strength(ctx.world, faction_id);
        if enemy_strength > 0 && (own_strength as f64 / enemy_strength as f64) >= HIRE_POWER_RATIO_MAX {
            continue; // Already dominant, no need for mercs
        }

        // Get faction's region for distance check
        let Some(faction_region) = helpers::faction_capital_largest(ctx.world, faction_id)
            .map(|(_, rid)| rid)
            .or_else(|| helpers::faction_capital_oldest(ctx.world, faction_id)
                .and_then(|sid| ctx.world.entities.get(&sid)
                    .and_then(|e| e.active_rel(RelationshipKind::LocatedIn))))
        else {
            continue;
        };

        // Find nearest available merc within BFS distance
        for &(merc_fid, merc_army_id, merc_region) in &available_mercs {
            // Check merc is still available (not hired by someone earlier in this loop)
            let still_available = ctx
                .world
                .entities
                .get(&merc_fid)
                .is_some_and(|e| {
                    !e.relationships
                        .iter()
                        .any(|r| r.kind == RelationshipKind::HiredBy && r.is_active())
                });
            if !still_available {
                continue;
            }

            // Check distance (BFS hops)
            if !within_bfs_distance(ctx.world, faction_region, merc_region, HIRE_BFS_MAX_HOPS) {
                continue;
            }

            // Check affordability
            let merc_strength = ctx
                .world
                .entities
                .get(&merc_army_id)
                .and_then(|e| e.data.as_army())
                .map(|ad| ad.strength)
                .unwrap_or(0);
            let wage = ctx
                .world
                .entities
                .get(&merc_fid)
                .and_then(|e| e.data.as_faction())
                .map(|fd| fd.mercenary_wage)
                .unwrap_or(MERCENARY_WAGE_PER_STRENGTH);
            let monthly_cost = merc_strength as f64 * wage;
            let signing_bonus = monthly_cost * HIRE_SIGNING_BONUS_MONTHS;
            let total_needed = signing_bonus + monthly_cost * HIRE_AFFORDABILITY_MONTHS;

            if treasury < total_needed {
                continue;
            }

            // Hire!
            let ev = ctx.world.add_event(
                EventKind::MercenaryHired,
                time,
                format!(
                    "{} hired by {}",
                    helpers::entity_name(ctx.world, merc_fid),
                    helpers::entity_name(ctx.world, faction_id)
                ),
            );

            ctx.world.add_relationship(
                merc_fid,
                faction_id,
                RelationshipKind::HiredBy,
                time,
                ev,
            );

            // Deduct signing bonus
            if let Some(entity) = ctx.world.entities.get_mut(&faction_id)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                fd.treasury -= signing_bonus;
            }

            // Set initial loyalty
            loyalty::set_loyalty(ctx.world, merc_fid, faction_id, HIRE_INITIAL_LOYALTY);

            // Reset unpaid months
            if let Some(entity) = ctx.world.entities.get_mut(&merc_fid)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                fd.unpaid_months = 0;
            }

            ctx.world.add_event_participant(ev, merc_fid, crate::model::ParticipantRole::Subject);
            ctx.world.add_event_participant(ev, faction_id, crate::model::ParticipantRole::Object);
            ctx.world.add_event_participant(ev, merc_army_id, crate::model::ParticipantRole::Witness);

            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::MercenaryHired {
                    mercenary_faction_id: merc_fid,
                    hiring_faction_id: faction_id,
                    army_id: merc_army_id,
                },
            });

            break; // Only hire one merc per faction per year
        }
    }
}

// ---------------------------------------------------------------------------
// Payment & Loyalty (monthly)
// ---------------------------------------------------------------------------

pub(super) fn process_payment_and_loyalty(ctx: &mut TickContext, _time: SimTimestamp) {
    // Collect hired merc factions: (merc_faction, employer_faction)
    let hired_mercs: Vec<(u64, u64)> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.data
                    .as_faction()
                    .is_some_and(|fd| fd.government_type == GovernmentType::MercenaryCompany)
        })
        .filter_map(|e| {
            let employer = e.active_rel(RelationshipKind::HiredBy)?;
            Some((e.id, employer))
        })
        .collect();

    for (merc_fid, employer_fid) in hired_mercs {
        // Calculate monthly wage
        let (merc_strength, wage_rate) = {
            let army_strength: u32 = ctx
                .world
                .entities
                .values()
                .filter(|e| {
                    e.kind == EntityKind::Army
                        && e.end.is_none()
                        && e.has_active_rel(RelationshipKind::MemberOf, merc_fid)
                })
                .filter_map(|e| e.data.as_army().map(|ad| ad.strength))
                .sum();

            let wage = ctx
                .world
                .entities
                .get(&merc_fid)
                .and_then(|e| e.data.as_faction())
                .map(|fd| fd.mercenary_wage)
                .unwrap_or(MERCENARY_WAGE_PER_STRENGTH);

            (army_strength, wage)
        };

        let monthly_cost = merc_strength as f64 * wage_rate;

        let employer_treasury = ctx
            .world
            .entities
            .get(&employer_fid)
            .and_then(|e| e.data.as_faction())
            .map(|fd| fd.treasury)
            .unwrap_or(0.0);

        if employer_treasury >= monthly_cost {
            // Pay
            if let Some(entity) = ctx.world.entities.get_mut(&employer_fid)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                fd.treasury -= monthly_cost;
            }
            loyalty::adjust_loyalty(ctx.world, merc_fid, employer_fid, PAYMENT_LOYALTY_GAIN);

            // Reset unpaid months
            if let Some(entity) = ctx.world.entities.get_mut(&merc_fid)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                fd.unpaid_months = 0;
            }
        } else {
            // Cannot pay
            loyalty::adjust_loyalty(ctx.world, merc_fid, employer_fid, -NONPAYMENT_LOYALTY_LOSS);

            if let Some(entity) = ctx.world.entities.get_mut(&merc_fid)
                && let Some(fd) = entity.data.as_faction_mut()
            {
                fd.unpaid_months += 1;
            }
        }

        // Floor army morale at loyalty * 0.8
        let current_loyalty = loyalty::get_loyalty(ctx.world, merc_fid, employer_fid);
        let morale_floor = current_loyalty * 0.8;
        for e in ctx.world.entities.values_mut() {
            if e.kind == EntityKind::Army
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, merc_fid)
                && let Some(ad) = e.data.as_army_mut()
                && ad.morale < morale_floor
            {
                ad.morale = morale_floor;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Desertion (monthly)
// ---------------------------------------------------------------------------

pub(super) fn check_desertion(ctx: &mut TickContext, time: SimTimestamp) {
    // Collect hired merc factions with low loyalty
    let potential_deserters: Vec<(u64, u64, u64)> = ctx  // (merc_faction, employer, army_id)
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.data
                    .as_faction()
                    .is_some_and(|fd| fd.government_type == GovernmentType::MercenaryCompany)
        })
        .filter_map(|e| {
            let employer = e.active_rel(RelationshipKind::HiredBy)?;
            let army = ctx.world.entities.values().find(|a| {
                a.kind == EntityKind::Army
                    && a.end.is_none()
                    && a.has_active_rel(RelationshipKind::MemberOf, e.id)
            })?;
            Some((e.id, employer, army.id))
        })
        .collect();

    for (merc_fid, employer_fid, army_id) in potential_deserters {
        let current_loyalty = loyalty::get_loyalty(ctx.world, merc_fid, employer_fid);
        if current_loyalty >= DESERTION_LOYALTY_THRESHOLD {
            continue;
        }

        let desertion_chance = (DESERTION_LOYALTY_THRESHOLD - current_loyalty) * DESERTION_CHANCE_FACTOR;
        if ctx.rng.random::<f64>() >= desertion_chance {
            continue;
        }

        // Desertion happens!
        let switch_side = ctx.rng.random::<f64>() < SIDE_SWITCH_CHANCE;

        if switch_side {
            // Find enemy faction that can afford signing bonus
            let new_employer = find_enemy_employer(ctx, employer_fid, merc_fid, army_id);
            if let Some(new_emp) = new_employer {
                // Switch sides
                let ev = ctx.world.add_event(
                    EventKind::MercenarySwitched,
                    time,
                    format!(
                        "{} switched from {} to {}",
                        helpers::entity_name(ctx.world, merc_fid),
                        helpers::entity_name(ctx.world, employer_fid),
                        helpers::entity_name(ctx.world, new_emp),
                    ),
                );

                // End old HiredBy
                ctx.world.end_relationship(
                    merc_fid,
                    employer_fid,
                    RelationshipKind::HiredBy,
                    time,
                    ev,
                );

                // Create new HiredBy
                ctx.world.add_relationship(
                    merc_fid,
                    new_emp,
                    RelationshipKind::HiredBy,
                    time,
                    ev,
                );

                // Set loyalty to new employer
                loyalty::set_loyalty(ctx.world, merc_fid, new_emp, HIRE_INITIAL_LOYALTY);
                loyalty::remove_loyalty(ctx.world, merc_fid, employer_fid);

                // Grievance on betrayed faction
                crate::sim::grievance::add_grievance(
                    ctx.world,
                    employer_fid,
                    merc_fid,
                    0.30,
                    "mercenary_betrayal",
                    time,
                    ev,
                );

                ctx.world.add_event_participant(ev, merc_fid, crate::model::ParticipantRole::Subject);
                ctx.world.add_event_participant(ev, employer_fid, crate::model::ParticipantRole::Origin);
                ctx.world.add_event_participant(ev, new_emp, crate::model::ParticipantRole::Destination);

                ctx.signals.push(Signal {
                    event_id: ev,
                    kind: SignalKind::MercenaryDeserted {
                        mercenary_faction_id: merc_fid,
                        former_employer_id: employer_fid,
                        army_id,
                        switched_to: Some(new_emp),
                    },
                });

                continue;
            }
        }

        // Go independent
        let ev = ctx.world.add_event(
            EventKind::MercenaryDeserted,
            time,
            format!(
                "{} deserted {}",
                helpers::entity_name(ctx.world, merc_fid),
                helpers::entity_name(ctx.world, employer_fid),
            ),
        );

        ctx.world.end_relationship(
            merc_fid,
            employer_fid,
            RelationshipKind::HiredBy,
            time,
            ev,
        );
        loyalty::remove_loyalty(ctx.world, merc_fid, employer_fid);

        ctx.world.add_event_participant(ev, merc_fid, crate::model::ParticipantRole::Subject);
        ctx.world.add_event_participant(ev, employer_fid, crate::model::ParticipantRole::Object);

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::MercenaryDeserted {
                mercenary_faction_id: merc_fid,
                former_employer_id: employer_fid,
                army_id,
                switched_to: None,
            },
        });
    }
}

fn find_enemy_employer(
    ctx: &TickContext,
    current_employer: u64,
    merc_fid: u64,
    army_id: u64,
) -> Option<u64> {
    // Find factions at war with the current employer
    let enemies: Vec<u64> = ctx
        .world
        .entities
        .get(&current_employer)?
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::AtWar && r.is_active())
        .map(|r| r.target_entity_id)
        .collect();

    let merc_strength = ctx
        .world
        .entities
        .get(&army_id)
        .and_then(|e| e.data.as_army())
        .map(|ad| ad.strength)
        .unwrap_or(0);

    let wage = ctx
        .world
        .entities
        .get(&merc_fid)
        .and_then(|e| e.data.as_faction())
        .map(|fd| fd.mercenary_wage)
        .unwrap_or(MERCENARY_WAGE_PER_STRENGTH);

    let signing_bonus = merc_strength as f64 * wage * HIRE_SIGNING_BONUS_MONTHS;

    for enemy_fid in enemies {
        let treasury = ctx
            .world
            .entities
            .get(&enemy_fid)
            .and_then(|e| e.data.as_faction())
            .map(|fd| fd.treasury)
            .unwrap_or(0.0);

        if treasury >= signing_bonus {
            return Some(enemy_fid);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Contract termination (yearly, after war endings)
// ---------------------------------------------------------------------------

/// End mercenary contracts when the associated war ends.
pub(super) fn terminate_contracts_for_war_end(
    ctx: &mut TickContext,
    time: SimTimestamp,
    faction_a: u64,
    faction_b: u64,
) {
    // Find mercs hired by either side
    let mercs_to_release: Vec<(u64, u64)> = ctx  // (merc_faction, employer)
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.data
                    .as_faction()
                    .is_some_and(|fd| fd.government_type == GovernmentType::MercenaryCompany)
        })
        .filter_map(|e| {
            let employer = e.active_rel(RelationshipKind::HiredBy)?;
            if employer == faction_a || employer == faction_b {
                Some((e.id, employer))
            } else {
                None
            }
        })
        .collect();

    for (merc_fid, employer_fid) in mercs_to_release {
        let ev = ctx.world.add_event(
            EventKind::MercenaryDisbanded,
            time,
            format!(
                "{} contract with {} ended",
                helpers::entity_name(ctx.world, merc_fid),
                helpers::entity_name(ctx.world, employer_fid),
            ),
        );

        ctx.world.end_relationship(
            merc_fid,
            employer_fid,
            RelationshipKind::HiredBy,
            time,
            ev,
        );
        loyalty::remove_loyalty(ctx.world, merc_fid, employer_fid);

        // Reset unpaid months
        if let Some(entity) = ctx.world.entities.get_mut(&merc_fid)
            && let Some(fd) = entity.data.as_faction_mut()
        {
            fd.unpaid_months = 0;
        }

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::MercenaryContractEnded {
                mercenary_faction_id: merc_fid,
                employer_faction_id: employer_fid,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Disbanding (yearly)
// ---------------------------------------------------------------------------

pub(super) fn check_disbanding(ctx: &mut TickContext, time: SimTimestamp) {
    let merc_factions: Vec<(u64, bool)> = ctx  // (faction_id, is_hired)
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Faction
                && e.end.is_none()
                && e.data
                    .as_faction()
                    .is_some_and(|fd| fd.government_type == GovernmentType::MercenaryCompany)
        })
        .map(|e| {
            let is_hired = e.relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::HiredBy && r.is_active());
            (e.id, is_hired)
        })
        .collect();

    for (merc_fid, is_hired) in merc_factions {
        let should_disband = check_disband_conditions(ctx, merc_fid, is_hired, time);
        if !should_disband {
            continue;
        }

        disband_mercenary(ctx, merc_fid, time);
    }
}

fn check_disband_conditions(
    ctx: &mut TickContext,
    merc_fid: u64,
    is_hired: bool,
    time: SimTimestamp,
) -> bool {
    // Check army strength
    let total_strength: u32 = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, merc_fid)
        })
        .filter_map(|e| e.data.as_army().map(|ad| ad.strength))
        .sum();

    if total_strength < DISBAND_MIN_STRENGTH {
        return true;
    }

    // Check for leader
    let has_leader = helpers::faction_leader(ctx.world, merc_fid).is_some();
    if !has_leader {
        // Check if any warrior can take over
        let has_warrior = ctx.world.entities.values().any(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, merc_fid)
                && e.data
                    .as_person()
                    .is_some_and(|pd| pd.role == Role::Warrior)
        });
        if !has_warrior {
            return true;
        }
    }

    // Check idle time (not hired for too long)
    if !is_hired {
        let faction_start = ctx
            .world
            .entities
            .get(&merc_fid)
            .and_then(|e| e.origin)
            .unwrap_or_default();
        let years_alive = time.years_since(faction_start);
        if years_alive >= IDLE_YEARS_BEFORE_DISBAND {
            // Check last hired date — use faction start as proxy
            if ctx.rng.random::<f64>() < IDLE_DISBAND_CHANCE {
                return true;
            }
        }
    }

    false
}

fn disband_mercenary(ctx: &mut TickContext, merc_fid: u64, time: SimTimestamp) {
    let ev = ctx.world.add_event(
        EventKind::MercenaryDisbanded,
        time,
        format!(
            "{} disbanded",
            helpers::entity_name(ctx.world, merc_fid),
        ),
    );

    ctx.world.add_event_participant(ev, merc_fid, crate::model::ParticipantRole::Subject);

    // End all armies
    let army_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, merc_fid)
        })
        .map(|e| e.id)
        .collect();
    for aid in army_ids {
        ctx.world.end_entity(aid, time, ev);
    }

    // End all members
    let member_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, merc_fid)
        })
        .map(|e| e.id)
        .collect();
    for mid in member_ids {
        helpers::end_all_person_relationships(ctx.world, mid, time, ev);
    }

    // End the faction
    ctx.world.end_entity(merc_fid, time, ev);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn faction_military_strength(world: &crate::model::World, faction_id: u64) -> u32 {
    let effective_fid = helpers::employer_or_self(world, faction_id);
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Army
                && e.end.is_none()
                && e.has_active_rel(RelationshipKind::MemberOf, faction_id)
        })
        .chain(
            // Also count mercenary armies hired by this faction
            world
                .entities
                .values()
                .filter(|e| {
                    e.kind == EntityKind::Army
                        && e.end.is_none()
                        && e.data.as_army().is_some_and(|ad| ad.is_mercenary)
                        && e.active_rel(RelationshipKind::MemberOf)
                            .and_then(|mfid| helpers::mercenary_employer(world, mfid))
                            .is_some_and(|emp| emp == effective_fid)
                }),
        )
        .filter_map(|e| e.data.as_army().map(|ad| ad.strength))
        .sum()
}

fn faction_enemy_strength(world: &crate::model::World, faction_id: u64) -> u32 {
    let enemies: Vec<u64> = world
        .entities
        .get(&faction_id)
        .map(|e| {
            e.relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::AtWar && r.is_active())
                .map(|r| r.target_entity_id)
                .collect()
        })
        .unwrap_or_default();

    enemies
        .iter()
        .map(|&eid| faction_military_strength(world, eid))
        .sum()
}

fn within_bfs_distance(
    world: &crate::model::World,
    start: u64,
    goal: u64,
    max_hops: usize,
) -> bool {
    if start == goal {
        return true;
    }
    let mut visited = BTreeSet::new();
    visited.insert(start);
    let mut frontier = vec![start];

    for _hop in 0..max_hops {
        let mut next_frontier = Vec::new();
        for &region in &frontier {
            for adj in helpers::adjacent_regions(world, region) {
                if adj == goal {
                    return true;
                }
                if visited.insert(adj) {
                    next_frontier.push(adj);
                }
            }
        }
        frontier = next_frontier;
        if frontier.is_empty() {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use crate::testutil;

    #[test]
    fn mercenary_company_creation_via_scenario() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Borderlands");
        let merc = s.add_mercenary_company("Iron Hawks", region, 50);

        let world = s.build();

        // Faction exists and is MercenaryCompany
        let fd = world.faction(merc.faction);
        assert_eq!(fd.government_type, GovernmentType::MercenaryCompany);
        assert!((fd.mercenary_wage - 1.0).abs() < f64::EPSILON);

        // Army exists and is_mercenary
        let ad = world.army(merc.army);
        assert!(ad.is_mercenary);
        assert_eq!(ad.strength, 50);

        // Leader exists and is Warrior with LeaderOf
        let pd = world.person(merc.leader);
        assert_eq!(pd.role, Role::Warrior);
        assert!(
            world
                .entities
                .get(&merc.leader)
                .unwrap()
                .has_active_rel(RelationshipKind::LeaderOf, merc.faction)
        );
    }

    #[test]
    fn hire_mercenary_creates_relationship() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Borderlands");
        let faction = s.faction("Kingdom").treasury(500.0).id();
        s.settlement("Town", faction, region).population(200).id();

        let merc = s.add_mercenary_company("Iron Hawks", region, 50);
        s.hire_mercenary(merc.faction, faction);

        let world = s.build();

        // HiredBy relationship exists
        assert!(
            world
                .entities
                .get(&merc.faction)
                .unwrap()
                .has_active_rel(RelationshipKind::HiredBy, faction)
        );

        // employer_or_self resolves correctly
        assert_eq!(helpers::employer_or_self(&world, merc.faction), faction);
    }

    #[test]
    fn mercenary_employer_returns_none_when_not_hired() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Borderlands");
        let merc = s.add_mercenary_company("Iron Hawks", region, 50);
        let world = s.build();

        assert!(helpers::mercenary_employer(&world, merc.faction).is_none());
        assert_eq!(helpers::employer_or_self(&world, merc.faction), merc.faction);
    }

    #[test]
    fn loyalty_system_integration() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Borderlands");
        let faction = s.add_faction("Kingdom");
        let merc = s.add_mercenary_company("Iron Hawks", region, 50);
        let mut world = s.build();

        // Set loyalty and verify
        loyalty::set_loyalty(&mut world, merc.faction, faction, 0.7);
        assert!((loyalty::get_loyalty(&world, merc.faction, faction) - 0.7).abs() < f64::EPSILON);

        // Adjust loyalty (payment)
        loyalty::adjust_loyalty(&mut world, merc.faction, faction, PAYMENT_LOYALTY_GAIN);
        assert!(
            (loyalty::get_loyalty(&world, merc.faction, faction) - 0.75).abs() < f64::EPSILON
        );

        // Adjust loyalty (non-payment)
        loyalty::adjust_loyalty(&mut world, merc.faction, faction, -NONPAYMENT_LOYALTY_LOSS);
        assert!(
            (loyalty::get_loyalty(&world, merc.faction, faction) - 0.6).abs() < f64::EPSILON
        );
    }

    #[test]
    fn mercenary_scenario_setup() {
        let setup = testutil::mercenary_scenario();

        // Merc faction is a MercenaryCompany
        let fd = setup.world.faction(setup.merc_faction);
        assert_eq!(fd.government_type, GovernmentType::MercenaryCompany);

        // Merc is hired by attacker
        assert!(
            setup
                .world
                .entities
                .get(&setup.merc_faction)
                .unwrap()
                .has_active_rel(RelationshipKind::HiredBy, setup.attacker_faction)
        );

        // Attacker and defender are at war
        assert!(helpers::has_active_rel_of_kind(
            &setup.world,
            setup.attacker_faction,
            setup.defender_faction,
            RelationshipKind::AtWar,
        ));

        // employer_or_self resolves merc → attacker
        assert_eq!(
            helpers::employer_or_self(&setup.world, setup.merc_faction),
            setup.attacker_faction
        );
    }

    #[test]
    fn is_non_state_faction_covers_both() {
        let mut s = Scenario::at_year(100);
        let region = s.add_region("Region");

        let kingdom = s.add_faction("Kingdom");
        let bandits = s.faction("Bandits").government_type(GovernmentType::BanditClan).id();
        let merc = s.add_mercenary_company("Mercs", region, 30);

        let world = s.build();

        assert!(!helpers::is_non_state_faction(&world, kingdom));
        assert!(helpers::is_non_state_faction(&world, bandits));
        assert!(helpers::is_non_state_faction(&world, merc.faction));
    }

    #[test]
    fn bfs_distance_check() {
        let mut s = Scenario::at_year(100);
        let r1 = s.add_region("R1");
        let r2 = s.add_region("R2");
        let r3 = s.add_region("R3");
        let r4 = s.add_region("R4");
        s.make_adjacent(r1, r2);
        s.make_adjacent(r2, r3);
        s.make_adjacent(r3, r4);
        let world = s.build();

        assert!(within_bfs_distance(&world, r1, r1, 0)); // same
        assert!(within_bfs_distance(&world, r1, r2, 1)); // 1 hop
        assert!(!within_bfs_distance(&world, r1, r3, 1)); // 2 hops, max=1
        assert!(within_bfs_distance(&world, r1, r3, 2)); // 2 hops
        assert!(within_bfs_distance(&world, r1, r4, 3)); // 3 hops
        assert!(!within_bfs_distance(&world, r1, r4, 2)); // 3 hops, max=2
    }

    #[test]
    fn payment_increases_loyalty() {
        let setup = testutil::mercenary_scenario();
        let mut world = setup.world;

        let initial_loyalty = loyalty::get_loyalty(&world, setup.merc_faction, setup.attacker_faction);

        // Simulate payment
        loyalty::adjust_loyalty(&mut world, setup.merc_faction, setup.attacker_faction, PAYMENT_LOYALTY_GAIN);
        let new_loyalty = loyalty::get_loyalty(&world, setup.merc_faction, setup.attacker_faction);

        assert!(
            (new_loyalty - initial_loyalty - PAYMENT_LOYALTY_GAIN).abs() < f64::EPSILON,
            "loyalty should increase by {PAYMENT_LOYALTY_GAIN}"
        );
    }

    #[test]
    fn nonpayment_decreases_loyalty() {
        let setup = testutil::mercenary_scenario();
        let mut world = setup.world;

        let initial_loyalty = loyalty::get_loyalty(&world, setup.merc_faction, setup.attacker_faction);

        loyalty::adjust_loyalty(&mut world, setup.merc_faction, setup.attacker_faction, -NONPAYMENT_LOYALTY_LOSS);
        let new_loyalty = loyalty::get_loyalty(&world, setup.merc_faction, setup.attacker_faction);

        assert!(
            (new_loyalty - (initial_loyalty - NONPAYMENT_LOYALTY_LOSS)).abs() < f64::EPSILON,
            "loyalty should decrease by {NONPAYMENT_LOYALTY_LOSS}"
        );
    }

    #[test]
    fn mercenary_army_fights_employers_enemy() {
        // Mercenary army should be hostile to the employer's enemies
        let setup = testutil::mercenary_scenario();

        assert!(
            super::super::are_effectively_hostile(
                &setup.world,
                setup.merc_faction,
                setup.defender_faction
            ),
            "merc hired by attacker should be hostile to defender"
        );
        assert!(
            !super::super::are_effectively_hostile(
                &setup.world,
                setup.merc_faction,
                setup.attacker_faction
            ),
            "merc hired by attacker should not be hostile to attacker"
        );
    }

    #[test]
    fn mercenary_moves_toward_enemy() {
        // Mercenary army should move toward employer's enemy
        let setup = testutil::mercenary_scenario();
        let mut world = setup.world;

        let enemies = super::super::effective_war_enemies(&world, setup.merc_faction);
        assert!(
            !enemies.is_empty(),
            "merc should have effective enemies through employer"
        );
        assert!(
            enemies.contains(&setup.defender_faction),
            "defender should be among merc's effective enemies"
        );

        // Run a conflict tick — merc army should move toward defender
        let initial_region = world
            .entities
            .get(&setup.merc_army)
            .and_then(|e| e.active_rel(RelationshipKind::LocatedIn))
            .unwrap();

        let mut system = crate::ConflictSystem;
        testutil::tick_system(&mut world, &mut system, 11, 42);

        let new_region = world
            .entities
            .get(&setup.merc_army)
            .and_then(|e| e.active_rel(RelationshipKind::LocatedIn));

        // Army should have moved (or be in same region if already adjacent to enemy)
        assert!(
            new_region.is_some(),
            "merc army should still have a location"
        );
        // Either moved closer to enemy or stayed (if enemy not reachable or already there)
        let moved = new_region != Some(initial_region);
        let at_enemy = new_region == Some(setup.defender_region);
        assert!(
            moved || at_enemy || initial_region == setup.defender_region,
            "merc army should move toward employer's enemy or already be there"
        );
    }

    #[test]
    fn territory_status_resolves_through_employer() {
        let setup = testutil::mercenary_scenario();

        // Attacker's region should be friendly for the merc army
        let status = super::super::get_territory_status(
            &setup.world,
            setup.attacker_region,
            setup.merc_faction,
        );
        assert_eq!(
            status,
            super::super::TerritoryStatus::Friendly,
            "employer's territory should be friendly to hired merc"
        );

        // Defender's region should be enemy for the merc army
        let status = super::super::get_territory_status(
            &setup.world,
            setup.defender_region,
            setup.merc_faction,
        );
        assert_eq!(
            status,
            super::super::TerritoryStatus::Enemy,
            "employer's enemy territory should be enemy to hired merc"
        );
    }

    #[test]
    fn determinism_with_mercenaries() {
        let systems_a = testutil::combat_systems();
        let systems_b = testutil::combat_systems();
        let world1 = testutil::generate_and_run(42, 30, systems_a);
        let world2 = testutil::generate_and_run(42, 30, systems_b);
        testutil::assert_deterministic(&world1, &world2);
    }

    #[test]
    fn integration_wealthy_faction_hires_mercs_in_war() {
        use crate::model::population::PopulationBreakdown;

        // A small wealthy faction at war should hire available mercenaries
        let mut s = Scenario::at_year(100);

        // Small wealthy kingdom — needs enough pop to muster an army (MIN_ARMY_STRENGTH=20)
        // and enough treasury to afford signing bonus + 6 months wages
        let small = s.add_kingdom_with(
            "Aurum",
            |fd| {
                fd.treasury = 800.0;
                fd.stability = 0.8;
            },
            |sd| {
                sd.population = 800;
                sd.population_breakdown = PopulationBreakdown::from_total(800);
            },
            |_| {},
        );

        // Large poor kingdom adjacent to small
        let large = s.add_rival_kingdom_with(
            "Ferrum",
            small.region,
            |fd| {
                fd.treasury = 30.0;
                fd.stability = 0.7;
            },
            |sd| {
                sd.population = 2000;
                sd.population_breakdown = PopulationBreakdown::from_total(2000);
            },
            |_| {},
        );

        // Create merc company in a region adjacent to both kingdoms
        let merc_region = s.add_region("Borderlands");
        s.make_adjacent(merc_region, small.region);
        s.make_adjacent(merc_region, large.region);
        s.add_mercenary_company("Silver Blades", merc_region, 40);

        // Start a war
        s.make_at_war(small.faction, large.faction);
        s.make_enemies(small.faction, large.faction);

        let mut systems: Vec<Box<dyn crate::SimSystem>> = vec![
            Box::new(crate::DemographicsSystem),
            Box::new(crate::EconomySystem),
            Box::new(crate::ConflictSystem),
        ];
        let world = s.run(&mut systems, 20, 7);

        // Check: at least one MercenaryHired event should have occurred
        let hired_events = testutil::count_events(&world, &EventKind::MercenaryHired);
        assert!(
            hired_events > 0,
            "expected at least one MercenaryHired event across 20 years of war, got {hired_events}"
        );
    }
}
