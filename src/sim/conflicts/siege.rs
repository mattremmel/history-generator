use rand::Rng;

use crate::model::entity_data::ActiveSiege;
use crate::model::{
    EntityKind, EventKind, ParticipantRole, RelationshipKind, SiegeOutcome, SimTimestamp, World,
};
use crate::sim::context::TickContext;
use crate::sim::extra_keys as K;
use crate::sim::signal::{Signal, SignalKind};

use crate::sim::helpers::{entity_name, has_active_rel_of_kind};

use super::{get_army_region, get_terrain_defense_bonus};

// Siege constants
const SIEGE_PROSPERITY_DECAY: f64 = 0.03;
const SIEGE_STARVATION_THRESHOLD: f64 = 0.2;
const SIEGE_STARVATION_POP_LOSS: f64 = 0.01;
const SIEGE_ASSAULT_CHANCE: f64 = 0.10;
const SIEGE_ASSAULT_MIN_MONTHS: u32 = 2;
const SIEGE_ASSAULT_MORALE_MIN: f64 = 0.4;
const SIEGE_ASSAULT_POWER_RATIO: f64 = 1.5;
const SIEGE_ASSAULT_CASUALTY_MIN: f64 = 0.15;
const SIEGE_ASSAULT_CASUALTY_MAX: f64 = 0.30;
const SIEGE_ASSAULT_MORALE_PENALTY: f64 = 0.15;

pub(super) fn start_sieges(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    struct ConquestCandidate {
        army_id: u64,
        army_faction: u64,
        region_id: u64,
    }

    let candidates: Vec<ConquestCandidate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Army && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e.extra_u64(K::FACTION_ID)?;
            let region_id = e.active_rel(RelationshipKind::LocatedIn)?;
            Some(ConquestCandidate {
                army_id: e.id,
                army_faction: faction_id,
                region_id,
            })
        })
        .collect();

    for candidate in &candidates {
        // Check no opposing army in same region
        let has_opposition = candidates.iter().any(|other| {
            other.army_id != candidate.army_id
                && other.region_id == candidate.region_id
                && has_active_rel_of_kind(
                    ctx.world,
                    candidate.army_faction,
                    other.army_faction,
                    RelationshipKind::AtWar,
                )
        });
        if has_opposition {
            continue;
        }

        // Find enemy settlements in this region (no active siege)
        let enemy_settlements: Vec<(u64, u64, u8)> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Settlement
                    && e.end.is_none()
                    && e.has_active_rel(RelationshipKind::LocatedIn, candidate.region_id)
            })
            .filter_map(|e| {
                let sd = e.data.as_settlement()?;
                if sd.active_siege.is_some() {
                    return None;
                }
                let owner = e.active_rel(RelationshipKind::MemberOf)?;
                if owner != candidate.army_faction
                    && has_active_rel_of_kind(
                        ctx.world,
                        candidate.army_faction,
                        owner,
                        RelationshipKind::AtWar,
                    )
                {
                    Some((e.id, owner, sd.fortification_level))
                } else {
                    None
                }
            })
            .collect();

        for (settlement_id, loser_faction, fort_level) in enemy_settlements {
            let winner_faction = candidate.army_faction;

            if fort_level == 0 {
                // Instant conquest for unfortified settlements
                let _ = execute_conquest(
                    ctx,
                    settlement_id,
                    winner_faction,
                    loser_faction,
                    time,
                    current_year,
                );
            } else {
                // Begin siege
                let winner_name = entity_name(ctx.world, winner_faction);
                let settlement_name = entity_name(ctx.world, settlement_id);
                let loser_name = entity_name(ctx.world, loser_faction);

                let siege_ev = ctx.world.add_event(
                    EventKind::Siege,
                    time,
                    format!(
                        "{winner_name} began siege of {settlement_name} of {loser_name} in year {current_year}"
                    ),
                );
                ctx.world.add_event_participant(
                    siege_ev,
                    winner_faction,
                    ParticipantRole::Attacker,
                );
                ctx.world
                    .add_event_participant(siege_ev, settlement_id, ParticipantRole::Object);

                {
                    let entity = ctx.world.entities.get_mut(&settlement_id).unwrap();
                    let sd = entity.data.as_settlement_mut().unwrap();
                    sd.active_siege = Some(ActiveSiege {
                        attacker_army_id: candidate.army_id,
                        attacker_faction_id: winner_faction,
                        started: time,
                        months_elapsed: 0,
                        civilian_deaths: 0,
                    });
                }

                // Mark army as besieging
                ctx.world.set_extra(
                    candidate.army_id,
                    K::BESIEGING_SETTLEMENT_ID,
                    serde_json::json!(settlement_id),
                    siege_ev,
                );

                ctx.signals.push(Signal {
                    event_id: siege_ev,
                    kind: SignalKind::SiegeStarted {
                        settlement_id,
                        attacker_faction_id: winner_faction,
                        defender_faction_id: loser_faction,
                    },
                });
            }
        }
    }
}

pub(super) fn execute_conquest(
    ctx: &mut TickContext,
    settlement_id: u64,
    winner_faction: u64,
    loser_faction: u64,
    time: SimTimestamp,
    current_year: u32,
) -> u64 {
    let winner_name = entity_name(ctx.world, winner_faction);
    let loser_name = entity_name(ctx.world, loser_faction);
    let settlement_name = entity_name(ctx.world, settlement_id);

    let siege_ev = ctx.world.add_event(
        EventKind::Siege,
        time,
        format!("{winner_name} besieged {settlement_name} of {loser_name} in year {current_year}"),
    );
    ctx.world
        .add_event_participant(siege_ev, winner_faction, ParticipantRole::Attacker);
    ctx.world
        .add_event_participant(siege_ev, settlement_id, ParticipantRole::Object);

    let conquest_ev = ctx.world.add_caused_event(
        EventKind::Conquest,
        time,
        format!(
            "{winner_name} conquered {settlement_name} from {loser_name} in year {current_year}"
        ),
        siege_ev,
    );
    ctx.world
        .add_event_participant(conquest_ev, winner_faction, ParticipantRole::Attacker);
    ctx.world
        .add_event_participant(conquest_ev, loser_faction, ParticipantRole::Defender);
    ctx.world
        .add_event_participant(conquest_ev, settlement_id, ParticipantRole::Object);

    // Clear any active siege
    {
        let entity = ctx.world.entities.get_mut(&settlement_id).unwrap();
        let sd = entity.data.as_settlement_mut().unwrap();
        sd.active_siege = None;
    }
    ctx.world.record_change(
        settlement_id,
        conquest_ev,
        "active_siege",
        serde_json::json!("conquered"),
        serde_json::Value::Null,
    );

    // Transfer settlement
    ctx.world.end_relationship(
        settlement_id,
        loser_faction,
        RelationshipKind::MemberOf,
        time,
        conquest_ev,
    );
    ctx.world.add_relationship(
        settlement_id,
        winner_faction,
        RelationshipKind::MemberOf,
        time,
        conquest_ev,
    );

    // Transfer NPCs
    crate::sim::helpers::transfer_settlement_npcs(
        ctx.world,
        settlement_id,
        loser_faction,
        winner_faction,
        time,
        conquest_ev,
    );

    ctx.signals.push(Signal {
        event_id: conquest_ev,
        kind: SignalKind::SettlementCaptured {
            settlement_id,
            old_faction_id: loser_faction,
            new_faction_id: winner_faction,
        },
    });

    conquest_ev
}

pub(super) fn progress_sieges(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect settlements with active sieges
    struct SiegeInfo {
        settlement_id: u64,
        defender_faction_id: u64,
        attacker_army_id: u64,
        attacker_faction_id: u64,
        months_elapsed: u32,
        fort_level: u8,
        prosperity: f64,
        population: u32,
        civilian_deaths: u32,
    }

    let sieges: Vec<SiegeInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            let siege = sd.active_siege.as_ref()?;
            let defender_faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            Some(SiegeInfo {
                settlement_id: e.id,
                defender_faction_id,
                attacker_army_id: siege.attacker_army_id,
                attacker_faction_id: siege.attacker_faction_id,
                months_elapsed: siege.months_elapsed,
                fort_level: sd.fortification_level,
                prosperity: sd.prosperity,
                population: sd.population,
                civilian_deaths: siege.civilian_deaths,
            })
        })
        .collect();

    for info in sieges {
        // Validation: check if attacker army is still alive and in the same region
        let army_alive = ctx
            .world
            .entities
            .get(&info.attacker_army_id)
            .is_some_and(|e| e.end.is_none());

        let still_at_war = has_active_rel_of_kind(
            ctx.world,
            info.attacker_faction_id,
            info.defender_faction_id,
            RelationshipKind::AtWar,
        );

        let army_in_same_region = if army_alive {
            let army_region = get_army_region(ctx.world, info.attacker_army_id);
            let settlement_region = ctx
                .world
                .entities
                .get(&info.settlement_id)
                .and_then(|e| e.active_rel(RelationshipKind::LocatedIn));
            army_region.is_some() && army_region == settlement_region
        } else {
            false
        };

        if !army_alive || !still_at_war || !army_in_same_region {
            let outcome = if !army_alive {
                SiegeOutcome::Lifted
            } else {
                SiegeOutcome::Abandoned
            };
            clear_siege(
                ctx,
                SiegeClearParams {
                    settlement_id: info.settlement_id,
                    army_id: info.attacker_army_id,
                    attacker_faction_id: info.attacker_faction_id,
                    defender_faction_id: info.defender_faction_id,
                    outcome,
                },
                time,
                current_year,
            );
            continue;
        }

        // Increment months
        let months = info.months_elapsed + 1;
        let mut civilian_deaths = info.civilian_deaths;

        // Starvation: prosperity decays
        let mut prosperity = info.prosperity;
        prosperity = (prosperity - SIEGE_PROSPERITY_DECAY).max(0.0);

        let mut pop = info.population;
        // Below starvation threshold, population losses
        if prosperity < SIEGE_STARVATION_THRESHOLD && pop > 0 {
            let losses = (pop as f64 * SIEGE_STARVATION_POP_LOSS).ceil() as u32;
            pop = pop.saturating_sub(losses);
            civilian_deaths += losses;
        }

        // Update settlement state
        {
            let entity = ctx.world.entities.get_mut(&info.settlement_id).unwrap();
            let sd = entity.data.as_settlement_mut().unwrap();
            sd.prosperity = prosperity;
            sd.population = pop;
            sd.population_breakdown.scale_to(pop);
            if let Some(siege) = sd.active_siege.as_mut() {
                siege.months_elapsed = months;
                siege.civilian_deaths = civilian_deaths;
            }
        }

        // Surrender check (starts at 3 months)
        if months >= 3 {
            let base_chance = match months {
                3..=5 => 0.02,
                6..=11 => 0.05,
                _ => 0.10,
            };
            // Lower prosperity increases surrender chance
            let prosperity_mod = 1.0 + (1.0 - prosperity);
            // Higher fortification reduces surrender chance
            let fort_mod = 1.0 / (1.0 + info.fort_level as f64 * 0.3);
            let surrender_chance = base_chance * prosperity_mod * fort_mod;

            if ctx.rng.random_range(0.0..1.0) < surrender_chance {
                let conquest_ev = execute_conquest(
                    ctx,
                    info.settlement_id,
                    info.attacker_faction_id,
                    info.defender_faction_id,
                    time,
                    current_year,
                );
                // Clear besieging marker on army
                clear_besieging_extra(ctx.world, info.attacker_army_id, conquest_ev);
                ctx.signals.push(Signal {
                    event_id: conquest_ev,
                    kind: SignalKind::SiegeEnded {
                        settlement_id: info.settlement_id,
                        attacker_faction_id: info.attacker_faction_id,
                        defender_faction_id: info.defender_faction_id,
                        outcome: SiegeOutcome::Conquered,
                    },
                });
                continue;
            }
        }

        // Assault attempt (after minimum months, with morale check)
        if months >= SIEGE_ASSAULT_MIN_MONTHS
            && ctx.rng.random_range(0.0..1.0) < SIEGE_ASSAULT_CHANCE
        {
            let army_strength = super::army_strength(ctx.world, info.attacker_army_id);
            let army_morale = super::army_morale(ctx.world, info.attacker_army_id);

            if army_morale >= SIEGE_ASSAULT_MORALE_MIN {
                let settlement_region = ctx
                    .world
                    .entities
                    .get(&info.settlement_id)
                    .and_then(|e| e.active_rel(RelationshipKind::LocatedIn));
                let terrain_bonus = settlement_region
                    .and_then(|r| get_terrain_defense_bonus(ctx.world, r))
                    .unwrap_or(1.0);

                let attacker_power = army_strength as f64 * army_morale;
                let defender_power = pop as f64 * 0.05 * info.fort_level as f64 * terrain_bonus;

                if attacker_power >= defender_power * SIEGE_ASSAULT_POWER_RATIO {
                    // Assault succeeds
                    let conquest_ev = execute_conquest(
                        ctx,
                        info.settlement_id,
                        info.attacker_faction_id,
                        info.defender_faction_id,
                        time,
                        current_year,
                    );
                    clear_besieging_extra(ctx.world, info.attacker_army_id, conquest_ev);
                    ctx.signals.push(Signal {
                        event_id: conquest_ev,
                        kind: SignalKind::SiegeEnded {
                            settlement_id: info.settlement_id,
                            attacker_faction_id: info.attacker_faction_id,
                            defender_faction_id: info.defender_faction_id,
                            outcome: SiegeOutcome::Conquered,
                        },
                    });
                } else {
                    // Assault fails — attacker takes casualties and morale hit
                    let casualty_rate = ctx
                        .rng
                        .random_range(SIEGE_ASSAULT_CASUALTY_MIN..SIEGE_ASSAULT_CASUALTY_MAX);
                    let casualties = (army_strength as f64 * casualty_rate).round() as u32;
                    let new_strength = army_strength.saturating_sub(casualties);
                    let new_morale = (army_morale - SIEGE_ASSAULT_MORALE_PENALTY).clamp(0.0, 1.0);

                    let army_name = entity_name(ctx.world, info.attacker_army_id);
                    let settlement_name = entity_name(ctx.world, info.settlement_id);
                    let ev = ctx.world.add_event(
                        EventKind::Custom("siege_assault_failed".to_string()),
                        time,
                        format!(
                            "{army_name} failed to storm {settlement_name}, losing {casualties} troops in year {current_year}"
                        ),
                    );
                    ctx.world.add_event_participant(
                        ev,
                        info.attacker_army_id,
                        ParticipantRole::Subject,
                    );
                    ctx.world.add_event_participant(
                        ev,
                        info.settlement_id,
                        ParticipantRole::Object,
                    );

                    {
                        let entity = ctx.world.entities.get_mut(&info.attacker_army_id).unwrap();
                        let ad = entity.data.as_army_mut().unwrap();
                        ad.strength = new_strength;
                        ad.morale = new_morale;
                    }
                    ctx.world.record_change(
                        info.attacker_army_id,
                        ev,
                        "strength",
                        serde_json::json!(army_strength),
                        serde_json::json!(new_strength),
                    );

                    if new_strength == 0 {
                        ctx.world.end_entity(info.attacker_army_id, time, ev);
                        clear_siege(
                            ctx,
                            SiegeClearParams {
                                settlement_id: info.settlement_id,
                                army_id: info.attacker_army_id,
                                attacker_faction_id: info.attacker_faction_id,
                                defender_faction_id: info.defender_faction_id,
                                outcome: SiegeOutcome::Lifted,
                            },
                            time,
                            current_year,
                        );
                    }
                }
            }
        }
    }
}

pub(super) struct SiegeClearParams {
    pub settlement_id: u64,
    pub army_id: u64,
    pub attacker_faction_id: u64,
    pub defender_faction_id: u64,
    pub outcome: SiegeOutcome,
}

pub(super) fn clear_siege(
    ctx: &mut TickContext,
    params: SiegeClearParams,
    time: SimTimestamp,
    current_year: u32,
) {
    let SiegeClearParams {
        settlement_id,
        army_id,
        attacker_faction_id,
        defender_faction_id,
        outcome,
    } = params;
    let settlement_name = entity_name(ctx.world, settlement_id);
    let ev = ctx.world.add_event(
        EventKind::Custom("siege_ended".to_string()),
        time,
        format!("Siege of {settlement_name} ended ({outcome}) in year {current_year}"),
    );
    ctx.world
        .add_event_participant(ev, settlement_id, ParticipantRole::Subject);

    {
        let entity = ctx.world.entities.get_mut(&settlement_id).unwrap();
        let sd = entity.data.as_settlement_mut().unwrap();
        sd.active_siege = None;
    }
    ctx.world.record_change(
        settlement_id,
        ev,
        "active_siege",
        serde_json::json!(outcome),
        serde_json::Value::Null,
    );

    clear_besieging_extra(ctx.world, army_id, ev);

    ctx.signals.push(Signal {
        event_id: ev,
        kind: SignalKind::SiegeEnded {
            settlement_id,
            attacker_faction_id,
            defender_faction_id,
            outcome,
        },
    });
}

pub(super) fn clear_besieging_extra(world: &mut World, army_id: u64, event_id: u64) {
    world.remove_extra(army_id, K::BESIEGING_SETTLEMENT_ID, event_id);
}
