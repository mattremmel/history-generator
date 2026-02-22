use rand::Rng;
use rand::RngCore;

use super::context::TickContext;
use super::faction_names::generate_faction_name;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{EntityKind, EventKind, ParticipantRole, RelationshipKind, SimTimestamp, World};

pub struct PoliticsSystem;

impl SimSystem for PoliticsSystem {
    fn name(&self) -> &str {
        "politics"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        // --- 4a: Fill ruler vacancies ---
        fill_ruler_vacancies(ctx, time, current_year);

        // --- 4b: Stability drift ---
        update_stability(ctx, time);

        // --- 4c: Coups ---
        check_coups(ctx, time, current_year);

        // --- 4d: Inter-faction diplomacy ---
        update_diplomacy(ctx, time, current_year);

        // --- 4e: Faction splits ---
        check_faction_splits(ctx, time, current_year);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let current_year = time.year();

        for signal in ctx.inbox {
            if let SignalKind::RulerVacancy {
                faction_id,
                previous_ruler_id: _,
            } = &signal.kind
            {
                // Verify this is actually a faction (not a settlement from legacy signals)
                let is_faction = ctx
                    .world
                    .entities
                    .get(faction_id)
                    .is_some_and(|e| e.kind == EntityKind::Faction && e.end.is_none());
                if !is_faction {
                    continue;
                }

                // Skip if a ruler was already assigned this tick (e.g. by fill_ruler_vacancies)
                if has_ruler(ctx.world, *faction_id) {
                    continue;
                }

                let gov_type = get_government_type(ctx.world, *faction_id);
                let members = collect_faction_members(ctx.world, *faction_id);
                if let Some(ruler_id) = select_ruler(&members, &gov_type, ctx.world, ctx.rng) {
                    let ev = ctx.world.add_caused_event(
                        EventKind::Succession,
                        time,
                        format!("Succession in year {current_year}"),
                        signal.event_id,
                    );
                    ctx.world
                        .add_event_participant(ev, ruler_id, ParticipantRole::Subject);
                    ctx.world
                        .add_event_participant(ev, *faction_id, ParticipantRole::Object);
                    ctx.world.add_relationship(
                        ruler_id,
                        *faction_id,
                        RelationshipKind::RulerOf,
                        time,
                        ev,
                    );

                    // Succession causes a stability hit
                    apply_succession_stability_hit(ctx.world, *faction_id, ev);
                }
            }
        }
    }
}

// --- 4a: Fill ruler vacancies ---

fn fill_ruler_vacancies(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect faction info
    struct FactionInfo {
        id: u64,
        government_type: String,
    }

    let factions: Vec<FactionInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| FactionInfo {
            id: e.id,
            government_type: e
                .properties
                .get("government_type")
                .and_then(|v| v.as_str())
                .unwrap_or("chieftain")
                .to_string(),
        })
        .collect();

    // Find which factions have no ruler
    let leaderless: Vec<&FactionInfo> = factions
        .iter()
        .filter(|f| !has_ruler(ctx.world, f.id))
        .collect();

    for faction in leaderless {
        let members = collect_faction_members(ctx.world, faction.id);
        if let Some(ruler_id) = select_ruler(&members, &faction.government_type, ctx.world, ctx.rng)
        {
            let ev = ctx.world.add_event(
                EventKind::Succession,
                time,
                format!("New leader for faction in year {current_year}"),
            );
            ctx.world
                .add_event_participant(ev, ruler_id, ParticipantRole::Subject);
            ctx.world
                .add_event_participant(ev, faction.id, ParticipantRole::Object);
            ctx.world
                .add_relationship(ruler_id, faction.id, RelationshipKind::RulerOf, time, ev);

            // Succession causes a stability hit
            apply_succession_stability_hit(ctx.world, faction.id, ev);
        }
    }
}

// --- 4b: Stability drift ---

fn update_stability(ctx: &mut TickContext, time: SimTimestamp) {
    struct StabilityUpdate {
        faction_id: u64,
        new_stability: f64,
    }

    // Collect faction IDs and their current stability
    struct FactionStability {
        id: u64,
        old_stability: f64,
    }

    let factions: Vec<FactionStability> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| FactionStability {
            id: e.id,
            old_stability: e
                .properties
                .get("stability")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5),
        })
        .collect();

    let year_event = ctx.world.add_event(
        EventKind::Custom("politics_tick".to_string()),
        time,
        format!("Year {} politics tick", time.year()),
    );

    let mut updates: Vec<StabilityUpdate> = Vec::new();
    for faction in &factions {
        let has_leader = has_ruler(ctx.world, faction.id);
        let drift: f64 = ctx.rng.random_range(-0.05..0.05);
        let ruler_bonus = if has_leader { 0.0 } else { -0.05 };
        let new_stability = (faction.old_stability + drift + ruler_bonus).clamp(0.0, 1.0);
        updates.push(StabilityUpdate {
            faction_id: faction.id,
            new_stability,
        });
    }

    for update in updates {
        ctx.world.set_property(
            update.faction_id,
            "stability".to_string(),
            serde_json::json!(update.new_stability),
            year_event,
        );
    }
}

// --- 4c: Coups ---

fn check_coups(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    struct CoupTarget {
        faction_id: u64,
        current_ruler_id: u64,
        stability: f64,
    }

    let targets: Vec<CoupTarget> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .filter_map(|e| {
            let stability = e
                .properties
                .get("stability")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            if stability >= 0.4 {
                return None;
            }
            let ruler_id = find_faction_ruler(ctx.world, e.id)?;
            Some(CoupTarget {
                faction_id: e.id,
                current_ruler_id: ruler_id,
                stability,
            })
        })
        .collect();

    for target in targets {
        let coup_chance = 0.3 * (1.0 - target.stability);
        if ctx.rng.random_range(0.0..1.0) >= coup_chance {
            continue;
        }

        // Find a new ruler (warrior-weighted)
        let members = collect_faction_members(ctx.world, target.faction_id);
        let candidates: Vec<&MemberInfo> = members
            .iter()
            .filter(|m| m.id != target.current_ruler_id)
            .collect();
        if candidates.is_empty() {
            continue;
        }

        let new_ruler_id = select_weighted_member(&candidates, &["warrior", "elder"], ctx.rng);

        let ev = ctx.world.add_event(
            EventKind::Coup,
            time,
            format!("Coup in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, new_ruler_id, ParticipantRole::Instigator);
        ctx.world
            .add_event_participant(ev, target.current_ruler_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, target.faction_id, ParticipantRole::Object);

        // End old ruler's RulerOf
        ctx.world.end_relationship(
            target.current_ruler_id,
            target.faction_id,
            &RelationshipKind::RulerOf,
            time,
            ev,
        );

        // New ruler takes over
        ctx.world.add_relationship(
            new_ruler_id,
            target.faction_id,
            RelationshipKind::RulerOf,
            time,
            ev,
        );

        // Reset stability
        ctx.world.set_property(
            target.faction_id,
            "stability".to_string(),
            serde_json::json!(0.4),
            ev,
        );
    }
}

// --- 4d: Inter-faction diplomacy ---

fn update_diplomacy(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect living factions
    let faction_ids: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        .map(|e| e.id)
        .collect();

    // Check for dissolution of existing relationships
    struct EndAction {
        source_id: u64,
        target_id: u64,
        kind: RelationshipKind,
    }
    let mut ends: Vec<EndAction> = Vec::new();

    for &fid in &faction_ids {
        if let Some(entity) = ctx.world.entities.get(&fid) {
            for rel in &entity.relationships {
                if rel.end.is_some() {
                    continue;
                }
                match &rel.kind {
                    RelationshipKind::Ally => {
                        if ctx.rng.random_range(0.0..1.0) < 0.005 {
                            ends.push(EndAction {
                                source_id: fid,
                                target_id: rel.target_entity_id,
                                kind: RelationshipKind::Ally,
                            });
                        }
                    }
                    RelationshipKind::Enemy => {
                        if ctx.rng.random_range(0.0..1.0) < 0.01 {
                            ends.push(EndAction {
                                source_id: fid,
                                target_id: rel.target_entity_id,
                                kind: RelationshipKind::Enemy,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    for end in ends {
        let ev = ctx.world.add_event(
            EventKind::Dissolution,
            time,
            format!("Diplomatic relation ended in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, end.source_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, end.target_id, ParticipantRole::Object);
        ctx.world
            .end_relationship(end.source_id, end.target_id, &end.kind, time, ev);
    }

    // Check for new relationships between unrelated pairs
    struct NewRelAction {
        source_id: u64,
        target_id: u64,
        kind: RelationshipKind,
    }
    let mut new_rels: Vec<NewRelAction> = Vec::new();

    for i in 0..faction_ids.len() {
        for j in (i + 1)..faction_ids.len() {
            let a = faction_ids[i];
            let b = faction_ids[j];

            if has_active_diplomatic_rel(ctx.world, a, b) {
                continue;
            }

            let roll: f64 = ctx.rng.random_range(0.0..1.0);
            if roll < 0.02 {
                new_rels.push(NewRelAction {
                    source_id: a,
                    target_id: b,
                    kind: RelationshipKind::Ally,
                });
            } else if roll < 0.03 {
                new_rels.push(NewRelAction {
                    source_id: a,
                    target_id: b,
                    kind: RelationshipKind::Enemy,
                });
            }
        }
    }

    for rel in new_rels {
        let (kind_str, event_kind) = match &rel.kind {
            RelationshipKind::Ally => ("Alliance", EventKind::Treaty),
            RelationshipKind::Enemy => ("Rivalry", EventKind::Custom("rivalry".to_string())),
            _ => unreachable!(),
        };
        let ev = ctx.world.add_event(
            event_kind,
            time,
            format!("{kind_str} formed in year {current_year}"),
        );
        ctx.world
            .add_event_participant(ev, rel.source_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, rel.target_id, ParticipantRole::Object);
        ctx.world
            .add_relationship(rel.source_id, rel.target_id, rel.kind, time, ev);
    }
}

// --- 4e: Faction splits ---

fn check_faction_splits(ctx: &mut TickContext, time: SimTimestamp, current_year: u32) {
    // Collect settlements with their faction membership
    struct SettlementFaction {
        settlement_id: u64,
        faction_id: u64,
    }

    let settlement_factions: Vec<SettlementFaction> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
        .filter_map(|e| {
            let faction_id = e
                .relationships
                .iter()
                .find(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.end.is_none()
                        && ctx
                            .world
                            .entities
                            .get(&r.target_entity_id)
                            .is_some_and(|t| t.kind == EntityKind::Faction)
                })
                .map(|r| r.target_entity_id)?;
            Some(SettlementFaction {
                settlement_id: e.id,
                faction_id,
            })
        })
        .collect();

    // Count settlements per faction
    let mut faction_settlement_count: std::collections::BTreeMap<u64, u32> =
        std::collections::BTreeMap::new();
    for sf in &settlement_factions {
        *faction_settlement_count.entry(sf.faction_id).or_default() += 1;
    }

    // Only settlements in multi-settlement factions can split
    struct SplitPlan {
        settlement_id: u64,
        old_faction_id: u64,
    }

    let mut splits: Vec<SplitPlan> = Vec::new();
    for sf in &settlement_factions {
        let count = faction_settlement_count
            .get(&sf.faction_id)
            .copied()
            .unwrap_or(0);
        if count < 2 {
            continue;
        }
        if ctx.rng.random_range(0.0..1.0) < 0.003 {
            splits.push(SplitPlan {
                settlement_id: sf.settlement_id,
                old_faction_id: sf.faction_id,
            });
            // Decrease count so we don't split a faction down to 0
            if let Some(c) = faction_settlement_count.get_mut(&sf.faction_id) {
                *c = c.saturating_sub(1);
            }
        }
    }

    for split in splits {
        let name = generate_faction_name(ctx.rng);
        let ev = ctx.world.add_event(
            EventKind::FactionFormed,
            time,
            format!("{name} formed by secession in year {current_year}"),
        );

        let new_faction_id = ctx
            .world
            .add_entity(EntityKind::Faction, name, Some(time), ev);

        ctx.world.set_property(
            new_faction_id,
            "government_type".to_string(),
            serde_json::json!("chieftain"),
            ev,
        );
        ctx.world.set_property(
            new_faction_id,
            "stability".to_string(),
            serde_json::json!(0.5),
            ev,
        );

        // Move settlement to new faction
        ctx.world.end_relationship(
            split.settlement_id,
            split.old_faction_id,
            &RelationshipKind::MemberOf,
            time,
            ev,
        );
        ctx.world.add_relationship(
            split.settlement_id,
            new_faction_id,
            RelationshipKind::MemberOf,
            time,
            ev,
        );

        // Transfer NPCs in this settlement to new faction
        let npc_transfers: Vec<u64> = ctx
            .world
            .entities
            .values()
            .filter(|e| {
                e.kind == EntityKind::Person
                    && e.end.is_none()
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::LocatedIn
                            && r.target_entity_id == split.settlement_id
                            && r.end.is_none()
                    })
                    && e.relationships.iter().any(|r| {
                        r.kind == RelationshipKind::MemberOf
                            && r.target_entity_id == split.old_faction_id
                            && r.end.is_none()
                    })
            })
            .map(|e| e.id)
            .collect();

        for npc_id in npc_transfers {
            ctx.world.end_relationship(
                npc_id,
                split.old_faction_id,
                &RelationshipKind::MemberOf,
                time,
                ev,
            );
            ctx.world.add_relationship(
                npc_id,
                new_faction_id,
                RelationshipKind::MemberOf,
                time,
                ev,
            );
        }

        // High chance old and new factions become enemies
        if ctx.rng.random_bool(0.7) {
            ctx.world.add_relationship(
                split.old_faction_id,
                new_faction_id,
                RelationshipKind::Enemy,
                time,
                ev,
            );
        }

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::FactionSplit {
                old_faction_id: split.old_faction_id,
                new_faction_id,
                settlement_id: split.settlement_id,
            },
        });

        ctx.world
            .add_event_participant(ev, split.settlement_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, split.old_faction_id, ParticipantRole::Origin);
        ctx.world
            .add_event_participant(ev, new_faction_id, ParticipantRole::Destination);
    }
}

// --- Helpers ---

struct MemberInfo {
    id: u64,
    birth_year: u32,
    role: String,
}

fn collect_faction_members(world: &World, faction_id: u64) -> Vec<MemberInfo> {
    world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::MemberOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .map(|e| MemberInfo {
            id: e.id,
            birth_year: e
                .properties
                .get("birth_year")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            role: e
                .properties
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("common")
                .to_string(),
        })
        .collect()
}

fn select_ruler(
    members: &[MemberInfo],
    government_type: &str,
    _world: &World,
    rng: &mut dyn RngCore,
) -> Option<u64> {
    if members.is_empty() {
        return None;
    }

    match government_type {
        "hereditary" => {
            // Oldest member (lowest birth_year)
            members.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
        }
        "elective" => {
            // Weighted random: elder/scholar roles get 3x weight
            let preferred = ["elder", "scholar"];
            Some(select_weighted_member_from_slice(members, &preferred, rng))
        }
        _ => {
            // Chieftain: warrior preferred, else oldest
            let warriors: Vec<&MemberInfo> =
                members.iter().filter(|m| m.role == "warrior").collect();
            if !warriors.is_empty() {
                // Oldest warrior
                warriors.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
            } else {
                members.iter().min_by_key(|m| m.birth_year).map(|m| m.id)
            }
        }
    }
}

fn select_weighted_member(
    candidates: &[&MemberInfo],
    preferred_roles: &[&str],
    rng: &mut dyn RngCore,
) -> u64 {
    let weights: Vec<u32> = candidates
        .iter()
        .map(|m| {
            if preferred_roles.contains(&m.role.as_str()) {
                3
            } else {
                1
            }
        })
        .collect();
    let total: u32 = weights.iter().sum();
    let roll = rng.random_range(0..total);
    let mut cumulative = 0u32;
    for (i, &w) in weights.iter().enumerate() {
        cumulative += w;
        if roll < cumulative {
            return candidates[i].id;
        }
    }
    candidates.last().unwrap().id
}

fn select_weighted_member_from_slice(
    members: &[MemberInfo],
    preferred_roles: &[&str],
    rng: &mut dyn RngCore,
) -> u64 {
    let refs: Vec<&MemberInfo> = members.iter().collect();
    select_weighted_member(&refs, preferred_roles, rng)
}

fn has_ruler(world: &World, faction_id: u64) -> bool {
    world.entities.values().any(|e| {
        e.kind == EntityKind::Person
            && e.end.is_none()
            && e.relationships.iter().any(|r| {
                r.kind == RelationshipKind::RulerOf
                    && r.target_entity_id == faction_id
                    && r.end.is_none()
            })
    })
}

fn apply_succession_stability_hit(world: &mut World, faction_id: u64, event_id: u64) {
    if let Some(faction) = world.entities.get(&faction_id) {
        let old_stability = faction
            .properties
            .get("stability")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);
        let new_stability = (old_stability - 0.15).clamp(0.0, 1.0);
        world.set_property(
            faction_id,
            "stability".to_string(),
            serde_json::json!(new_stability),
            event_id,
        );
    }
}

fn find_faction_ruler(world: &World, faction_id: u64) -> Option<u64> {
    world
        .entities
        .values()
        .find(|e| {
            e.kind == EntityKind::Person
                && e.end.is_none()
                && e.relationships.iter().any(|r| {
                    r.kind == RelationshipKind::RulerOf
                        && r.target_entity_id == faction_id
                        && r.end.is_none()
                })
        })
        .map(|e| e.id)
}

fn get_government_type(world: &World, faction_id: u64) -> String {
    world
        .entities
        .get(&faction_id)
        .and_then(|e| e.properties.get("government_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("chieftain")
        .to_string()
}

fn has_active_diplomatic_rel(world: &World, a: u64, b: u64) -> bool {
    if let Some(entity) = world.entities.get(&a) {
        for rel in &entity.relationships {
            if rel.end.is_some() {
                continue;
            }
            if rel.target_entity_id == b
                && (rel.kind == RelationshipKind::Ally || rel.kind == RelationshipKind::Enemy)
            {
                return true;
            }
        }
    }
    if let Some(entity) = world.entities.get(&b) {
        for rel in &entity.relationships {
            if rel.end.is_some() {
                continue;
            }
            if rel.target_entity_id == a
                && (rel.kind == RelationshipKind::Ally || rel.kind == RelationshipKind::Enemy)
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::World;
    use crate::sim::demographics::DemographicsSystem;
    use crate::sim::runner::{SimConfig, run};
    use crate::worldgen::{self, config::WorldGenConfig};
    fn make_political_world(seed: u64, num_years: u32) -> World {
        let config = WorldGenConfig {
            seed,
            ..WorldGenConfig::default()
        };
        let mut world = worldgen::generate_world(&config);
        let mut systems: Vec<Box<dyn SimSystem>> =
            vec![Box::new(DemographicsSystem), Box::new(PoliticsSystem)];
        run(&mut world, &mut systems, SimConfig::new(1, num_years, seed));
        world
    }

    #[test]
    fn faction_gets_ruler_on_first_tick() {
        let world = make_political_world(42, 1);

        let factions: Vec<u64> = world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
            .map(|e| e.id)
            .collect();
        assert!(!factions.is_empty(), "should have factions");

        let mut ruled = 0;
        for &fid in &factions {
            if has_ruler(&world, fid) {
                ruled += 1;
            }
        }
        // After 1 year, factions with members should have rulers
        assert!(
            ruled > 0,
            "at least some factions should have rulers after year 1"
        );
    }

    #[test]
    fn stability_drifts_without_ruler() {
        // Create a world, run 1 year to establish factions, then check stability
        let world = make_political_world(42, 50);

        for faction in world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
        {
            assert!(
                faction.has_property("stability"),
                "faction {} should have stability",
                faction.name
            );
            let stability = faction.properties["stability"].as_f64().unwrap();
            assert!(
                (0.0..=1.0).contains(&stability),
                "stability should be in [0, 1], got {}",
                stability
            );
        }
    }

    #[test]
    fn succession_events_created() {
        let world = make_political_world(42, 100);

        let succession_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Succession)
            .count();
        assert!(
            succession_count > 0,
            "expected succession events after 100 years"
        );
    }

    #[test]
    fn diplomacy_forms_over_time() {
        let world = make_political_world(42, 200);

        let ally_count = world
            .collect_relationships()
            .filter(|r| r.kind == RelationshipKind::Ally)
            .count();
        let enemy_count = world
            .collect_relationships()
            .filter(|r| r.kind == RelationshipKind::Enemy)
            .count();
        assert!(
            ally_count + enemy_count > 0,
            "expected some diplomatic relationships after 200 years"
        );
    }

    #[test]
    fn coup_eventually_occurs() {
        // Run a longer simulation — coups need low stability
        let world = make_political_world(42, 500);

        let coup_count = world
            .events
            .values()
            .filter(|e| e.kind == EventKind::Coup)
            .count();
        // Coups are probabilistic, but over 500 years with many factions, at least one should happen
        // If this flakes, increase years or adjust. Using a known seed helps.
        assert!(coup_count > 0, "expected at least one coup in 500 years");
    }
}
