use rand::Rng;

use super::context::TickContext;
use super::helpers;
use super::signal::{Signal, SignalKind};
use super::system::{SimSystem, TickFrequency};
use crate::model::{
    EntityData, EntityKind, EventKind, ItemType, ParticipantRole, RelationshipKind, ResourceType,
    SiegeOutcome, SimTimestamp,
};

// ---------------------------------------------------------------------------
// Crafting parameters
// ---------------------------------------------------------------------------

/// Minimum settlement population required to craft items.
const CRAFT_MIN_POP: u32 = 100;
/// Base annual probability a settlement crafts an item.
const CRAFT_BASE_PROB: f64 = 0.03;
/// Additional crafting probability when a Workshop is present.
const CRAFT_WORKSHOP_BONUS: f64 = 0.02;
/// Treasury cost to craft an item.
const CRAFT_TREASURY_COST: f64 = 5.0;

// ---------------------------------------------------------------------------
// Resonance accumulation
// ---------------------------------------------------------------------------

/// Passive resonance gain per year for existing items.
const AGE_RESONANCE_PER_YEAR: f64 = 0.001;
/// Maximum resonance achievable from age alone.
const AGE_RESONANCE_CAP: f64 = 0.3;
/// Resonance gain from holder prestige (persons) per year.
const OWNER_PERSON_PRESTIGE_FACTOR: f64 = 0.02;
/// Resonance gain from holder prestige (settlements) per year.
const OWNER_SETTLEMENT_PRESTIGE_FACTOR: f64 = 0.01;

// ---------------------------------------------------------------------------
// Tier thresholds
// ---------------------------------------------------------------------------

const TIER_1_THRESHOLD: f64 = 0.2;
const TIER_2_THRESHOLD: f64 = 0.5;
const TIER_3_THRESHOLD: f64 = 0.8;

// ---------------------------------------------------------------------------
// Condition decay rates
// ---------------------------------------------------------------------------

/// Condition loss per year when held by a person.
const DECAY_HELD_BY_PERSON: f64 = 0.003;
/// Condition loss per year when held by a settlement.
const DECAY_HELD_BY_SETTLEMENT: f64 = 0.005;
/// Condition loss per year when unowned.
const DECAY_UNOWNED: f64 = 0.01;

// ---------------------------------------------------------------------------
// Signal response parameters
// ---------------------------------------------------------------------------

/// Resonance bonus for items that survived a siege.
const SIEGE_SURVIVAL_RESONANCE: f64 = 0.05;
/// Resonance bonus when an item is transferred on owner death.
const DEATH_TRANSFER_RESONANCE: f64 = 0.02;
/// Minimum resonance for an item to be considered "notable" (eligible for looting).
const NOTABLE_RESONANCE_THRESHOLD: f64 = 0.3;
/// Probability bandits steal a notable item during a raid.
const BANDIT_STEAL_PROB: f64 = 0.2;

// ---------------------------------------------------------------------------
// Material tables
// ---------------------------------------------------------------------------

const METAL_MATERIALS: &[&str] = &["iron", "bronze", "copper", "steel", "gold", "silver"];
const STONE_MATERIALS: &[&str] = &["granite", "obsidian", "marble", "basalt"];
const ORGANIC_MATERIALS: &[&str] = &["bone", "wood", "ivory", "horn"];
const PRECIOUS_MATERIALS: &[&str] = &["gold", "silver", "jade", "amber"];

pub struct ItemSystem;

impl SimSystem for ItemSystem {
    fn name(&self) -> &str {
        "items"
    }

    fn frequency(&self) -> TickFrequency {
        TickFrequency::Yearly
    }

    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let year_event = ctx.world.add_event(
            EventKind::Custom("item_tick".to_string()),
            time,
            format!("Item activity in year {}", time.year()),
        );

        craft_items(ctx, time, year_event);
        age_resonance(ctx, year_event);
        owner_resonance(ctx, year_event);
        check_tier_promotions(ctx, time, year_event);
        decay_condition(ctx, time, year_event);
    }

    fn handle_signals(&mut self, ctx: &mut TickContext) {
        let time = ctx.world.current_time;
        let year_event = ctx.world.add_event(
            EventKind::Custom("item_signal".to_string()),
            time,
            format!("Item signal processing in year {}", time.year()),
        );

        for signal in ctx.inbox {
            match &signal.kind {
                SignalKind::EntityDied { entity_id } => {
                    handle_entity_died(ctx, time, year_event, *entity_id);
                }
                SignalKind::SettlementCaptured {
                    settlement_id,
                    new_faction_id,
                    ..
                } => {
                    handle_settlement_captured(
                        ctx,
                        time,
                        year_event,
                        *settlement_id,
                        *new_faction_id,
                    );
                }
                SignalKind::SiegeEnded {
                    settlement_id,
                    outcome,
                    ..
                } => {
                    if *outcome == SiegeOutcome::Conquered {
                        handle_siege_survived(ctx, year_event, *settlement_id);
                    }
                }
                SignalKind::BanditRaid {
                    bandit_faction_id,
                    settlement_id,
                    ..
                } => {
                    handle_bandit_raid(ctx, time, year_event, *bandit_faction_id, *settlement_id);
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tick phases
// ---------------------------------------------------------------------------

fn craft_items(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    // Collect settlement info for crafting decisions
    struct CraftCandidate {
        settlement_id: u64,
        faction_id: u64,
        resources: Vec<ResourceType>,
        has_workshop: bool,
        treasury: f64,
    }

    let candidates: Vec<CraftCandidate> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Settlement && e.is_alive())
        .filter_map(|e| {
            let sd = e.data.as_settlement()?;
            if sd.population < CRAFT_MIN_POP {
                return None;
            }
            let faction_id = e.active_rel(RelationshipKind::MemberOf)?;
            let faction = ctx.world.entities.get(&faction_id)?;
            let fd = faction.data.as_faction()?;
            if fd.treasury < CRAFT_TREASURY_COST {
                return None;
            }
            let has_workshop = ctx.world.entities.values().any(|b| {
                b.kind == EntityKind::Building
                    && b.is_alive()
                    && b.has_active_rel(RelationshipKind::LocatedIn, e.id)
                    && b.data
                        .as_building()
                        .is_some_and(|bd| bd.building_type == crate::model::BuildingType::Workshop)
            });
            Some(CraftCandidate {
                settlement_id: e.id,
                faction_id,
                resources: sd.resources.clone(),
                has_workshop,
                treasury: fd.treasury,
            })
        })
        .collect();

    for c in candidates {
        let prob = CRAFT_BASE_PROB
            + if c.has_workshop {
                CRAFT_WORKSHOP_BONUS
            } else {
                0.0
            };
        if !ctx.rng.random_bool(prob) {
            continue;
        }

        // Pick item type weighted by resources
        let item_type = pick_item_type(ctx.rng, &c.resources);
        let material = pick_material(ctx.rng, &c.resources);

        let name = format!("{} {}", capitalize(&material), item_type);
        let mut data = EntityData::default_for_kind(EntityKind::Item);
        let EntityData::Item(ref mut id) = data else {
            unreachable!()
        };
        id.item_type = item_type;
        id.material = material;
        id.condition = 1.0;
        id.created = time;

        let ev = ctx.world.add_caused_event(
            EventKind::Custom("item_crafted".to_string()),
            time,
            format!("{name} crafted"),
            year_event,
        );

        let item_id = ctx
            .world
            .add_entity(EntityKind::Item, name, Some(time), data, ev);
        ctx.world
            .add_relationship(item_id, c.settlement_id, RelationshipKind::HeldBy, time, ev);
        ctx.world
            .add_event_participant(ev, item_id, ParticipantRole::Subject);
        ctx.world
            .add_event_participant(ev, c.settlement_id, ParticipantRole::Location);

        // Deduct treasury
        let old_treasury = c.treasury;
        let new_treasury = (old_treasury - CRAFT_TREASURY_COST).max(0.0);
        ctx.world.faction_mut(c.faction_id).treasury = new_treasury;
        ctx.world.record_change(
            c.faction_id,
            ev,
            "treasury",
            serde_json::json!(old_treasury),
            serde_json::json!(new_treasury),
        );

        ctx.signals.push(Signal {
            event_id: ev,
            kind: SignalKind::ItemCrafted {
                item_id,
                settlement_id: c.settlement_id,
                crafter_id: None,
                item_type,
            },
        });
    }
}

fn age_resonance(ctx: &mut TickContext, year_event: u64) {
    let items: Vec<(u64, f64)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Item && e.is_alive())
        .filter_map(|e| {
            let id = e.data.as_item()?;
            Some((e.id, id.resonance))
        })
        .collect();

    for (item_id, resonance) in items {
        if resonance < AGE_RESONANCE_CAP {
            let new_resonance = (resonance + AGE_RESONANCE_PER_YEAR).min(AGE_RESONANCE_CAP);
            ctx.world.item_mut(item_id).resonance = new_resonance;
            ctx.world.record_change(
                item_id,
                year_event,
                "resonance",
                serde_json::json!(resonance),
                serde_json::json!(new_resonance),
            );
        }
    }
}

fn owner_resonance(ctx: &mut TickContext, year_event: u64) {
    struct ItemHolder {
        item_id: u64,
        holder_id: u64,
        resonance: f64,
    }

    let items: Vec<ItemHolder> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Item && e.is_alive())
        .filter_map(|e| {
            let id = e.data.as_item()?;
            let holder_id = e.active_rel(RelationshipKind::HeldBy)?;
            Some(ItemHolder {
                item_id: e.id,
                holder_id,
                resonance: id.resonance,
            })
        })
        .collect();

    for ih in items {
        let holder = match ctx.world.entities.get(&ih.holder_id) {
            Some(e) if e.is_alive() => e,
            _ => continue,
        };

        let prestige_bonus = match holder.kind {
            EntityKind::Person => holder
                .data
                .as_person()
                .map(|pd| pd.prestige * OWNER_PERSON_PRESTIGE_FACTOR)
                .unwrap_or(0.0),
            EntityKind::Settlement => holder
                .data
                .as_settlement()
                .map(|sd| sd.prestige * OWNER_SETTLEMENT_PRESTIGE_FACTOR)
                .unwrap_or(0.0),
            _ => 0.0,
        };

        if prestige_bonus > 0.0 {
            let new_resonance = (ih.resonance + prestige_bonus).min(1.0);
            ctx.world.item_mut(ih.item_id).resonance = new_resonance;
            ctx.world.record_change(
                ih.item_id,
                year_event,
                "resonance",
                serde_json::json!(ih.resonance),
                serde_json::json!(new_resonance),
            );
        }
    }
}

fn check_tier_promotions(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    let items: Vec<(u64, f64, u8)> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Item && e.is_alive())
        .filter_map(|e| {
            let id = e.data.as_item()?;
            let old_tier = e.data.as_item()?.resonance_tier;
            Some((e.id, id.resonance, old_tier))
        })
        .collect();

    for (item_id, resonance, old_tier) in items {
        let new_tier = resonance_tier(resonance);
        if new_tier != old_tier {
            ctx.world.item_mut(item_id).resonance_tier = new_tier;
            ctx.world.record_change(
                item_id,
                year_event,
                "resonance_tier",
                serde_json::json!(old_tier),
                serde_json::json!(new_tier),
            );

            let ev = ctx.world.add_caused_event(
                EventKind::Custom("item_tier_promoted".to_string()),
                time,
                format!(
                    "{} reached tier {}",
                    helpers::entity_name(ctx.world, item_id),
                    new_tier
                ),
                year_event,
            );
            ctx.world
                .add_event_participant(ev, item_id, ParticipantRole::Subject);

            ctx.signals.push(Signal {
                event_id: ev,
                kind: SignalKind::ItemTierPromoted {
                    item_id,
                    old_tier,
                    new_tier,
                },
            });
        }
    }
}

fn decay_condition(ctx: &mut TickContext, time: SimTimestamp, year_event: u64) {
    struct DecayInfo {
        item_id: u64,
        condition: f64,
        holder_kind: Option<EntityKind>,
    }

    let items: Vec<DecayInfo> = ctx
        .world
        .entities
        .values()
        .filter(|e| e.kind == EntityKind::Item && e.is_alive())
        .filter_map(|e| {
            let id = e.data.as_item()?;
            let holder_kind = e
                .active_rel(RelationshipKind::HeldBy)
                .and_then(|hid| ctx.world.entities.get(&hid))
                .filter(|h| h.is_alive())
                .map(|h| h.kind);
            Some(DecayInfo {
                item_id: e.id,
                condition: id.condition,
                holder_kind,
            })
        })
        .collect();

    for di in items {
        let decay = match di.holder_kind {
            Some(EntityKind::Person) => DECAY_HELD_BY_PERSON,
            Some(EntityKind::Settlement) => DECAY_HELD_BY_SETTLEMENT,
            _ => DECAY_UNOWNED,
        };

        let new_condition = (di.condition - decay).max(0.0);
        ctx.world.item_mut(di.item_id).condition = new_condition;

        if new_condition <= 0.0 {
            let ev = ctx.world.add_caused_event(
                EventKind::Custom("item_destroyed".to_string()),
                time,
                format!(
                    "{} crumbled to dust",
                    helpers::entity_name(ctx.world, di.item_id)
                ),
                year_event,
            );
            ctx.world.end_entity(di.item_id, time, ev);
        }
    }
}

// ---------------------------------------------------------------------------
// Signal handlers
// ---------------------------------------------------------------------------

fn handle_entity_died(ctx: &mut TickContext, time: SimTimestamp, year_event: u64, entity_id: u64) {
    // Only handle persons
    let is_person = ctx
        .world
        .entities
        .get(&entity_id)
        .is_some_and(|e| e.kind == EntityKind::Person);
    if !is_person {
        return;
    }

    // Find items held by the deceased
    let held_items: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Item
                && e.is_alive()
                && e.has_active_rel(RelationshipKind::HeldBy, entity_id)
        })
        .map(|e| e.id)
        .collect();

    if held_items.is_empty() {
        return;
    }

    // Find faction of deceased, then leader, else settlement
    let faction_id = ctx
        .world
        .entities
        .get(&entity_id)
        .and_then(|e| e.active_rel(RelationshipKind::MemberOf));

    let new_holder = faction_id
        .and_then(|fid| helpers::faction_leader(ctx.world, fid))
        .filter(|lid| *lid != entity_id)
        .or_else(|| {
            // Fall back to any settlement of the faction
            faction_id.and_then(|fid| {
                helpers::faction_settlements(ctx.world, fid)
                    .into_iter()
                    .next()
            })
        });

    for item_id in held_items {
        // End old HeldBy
        ctx.world.end_relationship(
            item_id,
            entity_id,
            RelationshipKind::HeldBy,
            time,
            year_event,
        );

        // Add resonance for the death event
        let old_res = ctx.world.item(item_id).resonance;
        let new_res = (old_res + DEATH_TRANSFER_RESONANCE).min(1.0);
        ctx.world.item_mut(item_id).resonance = new_res;
        ctx.world.record_change(
            item_id,
            year_event,
            "resonance",
            serde_json::json!(old_res),
            serde_json::json!(new_res),
        );

        if let Some(holder_id) = new_holder {
            ctx.world.add_relationship(
                item_id,
                holder_id,
                RelationshipKind::HeldBy,
                time,
                year_event,
            );
            ctx.world.item_mut(item_id).last_transferred = Some(time);
            ctx.world.record_change(
                item_id,
                year_event,
                "last_transferred",
                serde_json::json!(null),
                serde_json::json!(time.year()),
            );

            ctx.signals.push(Signal {
                event_id: year_event,
                kind: SignalKind::ItemTransferred {
                    item_id,
                    old_holder_id: entity_id,
                    new_holder_id: holder_id,
                    cause: "owner_death".to_string(),
                },
            });
        }
        // If no new holder found, item becomes "lost" (no HeldBy)
    }
}

fn handle_settlement_captured(
    ctx: &mut TickContext,
    time: SimTimestamp,
    year_event: u64,
    settlement_id: u64,
    new_faction_id: u64,
) {
    // Notable items at settlement are looted by conquering faction
    let notable_items: Vec<u64> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Item
                && e.is_alive()
                && e.has_active_rel(RelationshipKind::HeldBy, settlement_id)
                && e.data
                    .as_item()
                    .is_some_and(|id| id.resonance > NOTABLE_RESONANCE_THRESHOLD)
        })
        .map(|e| e.id)
        .collect();

    // Find a settlement of the conquering faction to receive items
    let receiver = helpers::faction_settlements(ctx.world, new_faction_id)
        .into_iter()
        .find(|sid| *sid != settlement_id)
        .or(Some(settlement_id));

    let Some(receiver_id) = receiver else { return };

    for item_id in notable_items {
        ctx.world.end_relationship(
            item_id,
            settlement_id,
            RelationshipKind::HeldBy,
            time,
            year_event,
        );
        ctx.world.add_relationship(
            item_id,
            receiver_id,
            RelationshipKind::HeldBy,
            time,
            year_event,
        );
        ctx.world.item_mut(item_id).last_transferred = Some(time);
        ctx.world.record_change(
            item_id,
            year_event,
            "last_transferred",
            serde_json::json!(null),
            serde_json::json!(time.year()),
        );

        ctx.signals.push(Signal {
            event_id: year_event,
            kind: SignalKind::ItemTransferred {
                item_id,
                old_holder_id: settlement_id,
                new_holder_id: receiver_id,
                cause: "conquest".to_string(),
            },
        });
    }
}

fn handle_siege_survived(ctx: &mut TickContext, year_event: u64, settlement_id: u64) {
    // Items at the settlement gain resonance from surviving the siege
    let siege_items: Vec<(u64, f64)> = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Item
                && e.is_alive()
                && e.has_active_rel(RelationshipKind::HeldBy, settlement_id)
        })
        .filter_map(|e| Some((e.id, e.data.as_item()?.resonance)))
        .collect();

    for (item_id, old_res) in siege_items {
        let new_res = (old_res + SIEGE_SURVIVAL_RESONANCE).min(1.0);
        ctx.world.item_mut(item_id).resonance = new_res;
        ctx.world.record_change(
            item_id,
            year_event,
            "resonance",
            serde_json::json!(old_res),
            serde_json::json!(new_res),
        );
    }
}

fn handle_bandit_raid(
    ctx: &mut TickContext,
    time: SimTimestamp,
    year_event: u64,
    bandit_faction_id: u64,
    settlement_id: u64,
) {
    // 20% chance bandits steal a notable item
    if !ctx.rng.random_bool(BANDIT_STEAL_PROB) {
        return;
    }

    let stolen_item = ctx
        .world
        .entities
        .values()
        .filter(|e| {
            e.kind == EntityKind::Item
                && e.is_alive()
                && e.has_active_rel(RelationshipKind::HeldBy, settlement_id)
                && e.data
                    .as_item()
                    .is_some_and(|id| id.resonance > NOTABLE_RESONANCE_THRESHOLD)
        })
        .map(|e| e.id)
        .next();

    let Some(item_id) = stolen_item else { return };

    // Find bandit settlement (hideout)
    let bandit_hideout = helpers::faction_settlements(ctx.world, bandit_faction_id)
        .into_iter()
        .next();

    let Some(hideout_id) = bandit_hideout else {
        return;
    };

    ctx.world.end_relationship(
        item_id,
        settlement_id,
        RelationshipKind::HeldBy,
        time,
        year_event,
    );
    ctx.world.add_relationship(
        item_id,
        hideout_id,
        RelationshipKind::HeldBy,
        time,
        year_event,
    );
    ctx.world.item_mut(item_id).last_transferred = Some(time);
    ctx.world.record_change(
        item_id,
        year_event,
        "last_transferred",
        serde_json::json!(null),
        serde_json::json!(time.year()),
    );

    ctx.signals.push(Signal {
        event_id: year_event,
        kind: SignalKind::ItemTransferred {
            item_id,
            old_holder_id: settlement_id,
            new_holder_id: hideout_id,
            cause: "bandit_raid".to_string(),
        },
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resonance_tier(resonance: f64) -> u8 {
    if resonance >= TIER_3_THRESHOLD {
        3
    } else if resonance >= TIER_2_THRESHOLD {
        2
    } else if resonance >= TIER_1_THRESHOLD {
        1
    } else {
        0
    }
}

fn pick_item_type(rng: &mut dyn rand::RngCore, resources: &[ResourceType]) -> ItemType {
    let has_metal = resources.iter().any(|r| {
        matches!(
            r,
            ResourceType::Iron | ResourceType::Copper | ResourceType::Ore
        )
    });
    let has_precious = resources.iter().any(|r| {
        matches!(
            r,
            ResourceType::Gold | ResourceType::Gems | ResourceType::Pearls
        )
    });
    let has_stone = resources
        .iter()
        .any(|r| matches!(r, ResourceType::Stone | ResourceType::Obsidian));
    let has_clay = resources.iter().any(|r| matches!(r, ResourceType::Clay));

    // Build weighted candidates
    let mut candidates: Vec<(ItemType, u32)> = Vec::new();
    if has_metal {
        candidates.push((ItemType::Weapon, 3));
        candidates.push((ItemType::Tool, 2));
    }
    if has_precious {
        candidates.push((ItemType::Jewelry, 3));
        candidates.push((ItemType::Crown, 1));
        candidates.push((ItemType::Amulet, 2));
    }
    if has_stone {
        candidates.push((ItemType::Tablet, 2));
        candidates.push((ItemType::Idol, 2));
    }
    if has_clay {
        candidates.push((ItemType::Pottery, 3));
    }
    // Always available
    candidates.push((ItemType::Seal, 1));
    candidates.push((ItemType::Chest, 1));
    candidates.push((ItemType::Tool, 1));

    let total: u32 = candidates.iter().map(|(_, w)| w).sum();
    let mut roll = rng.random_range(0..total);
    for (item_type, weight) in &candidates {
        if roll < *weight {
            return *item_type;
        }
        roll -= weight;
    }
    ItemType::Tool
}

fn pick_material(rng: &mut dyn rand::RngCore, resources: &[ResourceType]) -> String {
    let has_metal = resources.iter().any(|r| {
        matches!(
            r,
            ResourceType::Iron | ResourceType::Copper | ResourceType::Ore
        )
    });
    let has_precious = resources.iter().any(|r| {
        matches!(
            r,
            ResourceType::Gold | ResourceType::Gems | ResourceType::Pearls
        )
    });
    let has_stone = resources
        .iter()
        .any(|r| matches!(r, ResourceType::Stone | ResourceType::Obsidian));

    let pool: &[&str] = if has_precious && rng.random_bool(0.3) {
        PRECIOUS_MATERIALS
    } else if has_metal && rng.random_bool(0.5) {
        METAL_MATERIALS
    } else if has_stone && rng.random_bool(0.4) {
        STONE_MATERIALS
    } else {
        ORGANIC_MATERIALS
    };

    pool[rng.random_range(0..pool.len())].to_string()
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    use crate::testutil;

    #[test]
    fn scenario_item_crafting_requires_population() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone_with(
            "SmallVillage",
            |fd| fd.treasury = 200.0,
            |sd| {
                sd.population = 50; // Below CRAFT_MIN_POP
                sd.population_breakdown = crate::model::PopulationBreakdown::from_total(50);
            },
        );

        let world = s.run(&mut [Box::new(ItemSystem)], 10, 42);

        let items = world.count_living(&EntityKind::Item);
        assert_eq!(items, 0, "tiny settlements should not craft items");
    }

    #[test]
    fn scenario_item_age_resonance_accumulates() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let item = s.add_item(ItemType::Weapon, "iron", setup.settlement);

        let world = s.run(&mut [Box::new(ItemSystem)], 50, 42);

        let res = world.item(item).resonance;
        assert!(res > 0.0, "item should gain resonance over time, got {res}");
        assert!(
            res <= AGE_RESONANCE_CAP + 0.01, // small tolerance for floating point
            "age-only resonance should be capped at {AGE_RESONANCE_CAP}, got {res}"
        );
    }

    #[test]
    fn scenario_item_owner_prestige_boosts_resonance() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let leader = s.person("Hero", setup.faction).prestige(0.8).id();

        let item = s.add_item(ItemType::Crown, "gold", leader);

        let world = s.run(
            &mut [Box::new(crate::sim::ReputationSystem), Box::new(ItemSystem)],
            20,
            42,
        );

        let res = world.item(item).resonance;
        // Should be higher than age alone (20 * 0.001 = 0.02)
        assert!(
            res > 0.02,
            "item held by prestigious person should gain extra resonance, got {res}"
        );
    }

    #[test]
    fn scenario_item_condition_decays() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let item = s.add_item(ItemType::Pottery, "clay", setup.settlement);

        let world = s.run(&mut [Box::new(ItemSystem)], 30, 42);

        let cond = world.item(item).condition;
        assert!(
            cond < 1.0,
            "item condition should decay over time, got {cond}"
        );
        assert!(
            cond > 0.0,
            "item should not be destroyed in 30 years, got {cond}"
        );
    }

    #[test]
    fn scenario_item_tier_promotion_emits_signal() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        // Set resonance above tier 1 threshold (0.2) — the item has no stored tier
        // extra yet, so check_tier_promotions will see old_tier=0, new_tier=1 and emit.
        let item = s.add_item_with(ItemType::Weapon, "iron", setup.settlement, |id| {
            id.resonance = 0.25;
        });

        let mut system = ItemSystem;
        let signals = testutil::tick_system(&mut s.build(), &mut system, 100, 42);

        let has_promotion = testutil::has_signal(&signals, |sk| {
            matches!(sk, SignalKind::ItemTierPromoted { new_tier: 1, .. })
        });
        assert!(has_promotion, "should emit ItemTierPromoted signal");
    }

    #[test]
    fn scenario_item_death_transfers_item() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let leader = s.person("King", setup.faction).id();
        s.make_leader(leader, setup.faction);
        let warrior = s.person("Warrior", setup.faction).id();
        let item = s.add_item(ItemType::Weapon, "iron", warrior);

        // Simulate death signal
        let mut world = s.build();
        let death_ev = world.add_event(
            EventKind::Death,
            SimTimestamp::from_year(100),
            "Warrior died".to_string(),
        );
        world.end_entity(warrior, SimTimestamp::from_year(100), death_ev);
        let signals = vec![Signal {
            event_id: death_ev,
            kind: SignalKind::EntityDied { entity_id: warrior },
        }];

        let mut system = ItemSystem;
        let out = testutil::deliver_signals(&mut world, &mut system, &signals, 42);

        // Item should now be held by the leader
        assert!(
            testutil::has_relationship(&world, item, &RelationshipKind::HeldBy, leader),
            "item should transfer to faction leader on owner death"
        );
        assert!(
            testutil::has_signal(&out, |sk| matches!(
                sk,
                SignalKind::ItemTransferred { cause, .. } if cause == "owner_death"
            )),
            "should emit ItemTransferred signal"
        );
    }

    #[test]
    fn scenario_item_destroyed_when_condition_zero() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let item = s.add_item_with(ItemType::Pottery, "clay", setup.settlement, |id| {
            id.condition = 0.004; // Will reach 0 after one tick at 0.005/year
        });

        let world = s.run(&mut [Box::new(ItemSystem)], 1, 42);

        assert!(
            !testutil::is_alive(&world, item),
            "item with condition 0 should be ended"
        );
    }

    #[test]
    fn resonance_tier_thresholds() {
        assert_eq!(resonance_tier(0.0), 0);
        assert_eq!(resonance_tier(0.1), 0);
        assert_eq!(resonance_tier(0.2), 1);
        assert_eq!(resonance_tier(0.5), 2);
        assert_eq!(resonance_tier(0.8), 3);
        assert_eq!(resonance_tier(1.0), 3);
    }

    #[test]
    fn scenario_entity_died_records_resonance_change() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("TestTown");
        let person = s.person("Hero", setup.faction).id();
        let item = s.add_item_with(ItemType::Weapon, "steel", person, |id| {
            id.resonance = 0.1;
        });
        s.make_leader(person, setup.faction);

        let mut world = s.build();
        let ev = world.add_event(
            EventKind::Death,
            SimTimestamp::from_year(100),
            "Hero died".to_string(),
        );
        world.current_time = SimTimestamp::from_year(100);

        let inbox = vec![Signal {
            event_id: ev,
            kind: SignalKind::EntityDied { entity_id: person },
        }];
        testutil::deliver_signals(&mut world, &mut ItemSystem, &inbox, 42);

        testutil::assert_property_changed(&world, item, "resonance");
    }

    #[test]
    fn scenario_craft_records_treasury_change() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone_with(
            "CraftTown",
            |fd| fd.treasury = 200.0,
            |sd| {
                sd.population = 300;
                sd.population_breakdown = crate::model::PopulationBreakdown::from_total(300);
            },
        );

        // Run long enough to get at least one craft (probabilistic, so use many years)
        let world = s.run(&mut [Box::new(ItemSystem)], 50, 42);

        // If any item was crafted, treasury change should be recorded
        let items = testutil::living_entities(&world, &EntityKind::Item);
        if !items.is_empty() {
            testutil::assert_property_changed(&world, setup.faction, "treasury");
        }
    }

    #[test]
    fn scenario_entity_died_transfers_items_to_leader() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_kingdom("Kingdom");
        let holder = s.add_person("ItemHolder", setup.faction);
        let item = s.add_item(ItemType::Weapon, "iron", holder);

        let mut world = s.build();

        // Create death event and end the holder
        let death_ev = world.add_event(
            EventKind::Death,
            SimTimestamp::from_year(100),
            "ItemHolder died".to_string(),
        );
        world.end_entity(holder, SimTimestamp::from_year(100), death_ev);

        let inbox = vec![Signal {
            event_id: death_ev,
            kind: SignalKind::EntityDied { entity_id: holder },
        }];
        testutil::deliver_signals(&mut world, &mut ItemSystem, &inbox, 42);

        // Item should no longer be held by the dead person
        assert!(
            !world.entities[&item].has_active_rel(RelationshipKind::HeldBy, holder),
            "item should no longer be held by deceased"
        );
        // Item should now be held by the kingdom leader
        assert!(
            world.entities[&item].has_active_rel(RelationshipKind::HeldBy, setup.leader),
            "item should transfer to faction leader on owner death"
        );
        // Resonance should have increased by DEATH_TRANSFER_RESONANCE (0.02)
        let resonance = world.item(item).resonance;
        assert!(
            (resonance - DEATH_TRANSFER_RESONANCE).abs() < f64::EPSILON,
            "item resonance should be {DEATH_TRANSFER_RESONANCE}, got {resonance}"
        );
    }

    #[test]
    fn scenario_settlement_captured_loots_items() {
        let mut s = Scenario::at_year(100);
        let kingdom_a = s.add_kingdom("Kingdom A");
        let kingdom_b = s.add_rival_kingdom("Kingdom B", kingdom_a.region);
        // Create item with resonance above NOTABLE_RESONANCE_THRESHOLD (0.3)
        let item = s.add_item_with(ItemType::Crown, "gold", kingdom_a.settlement, |id| {
            id.resonance = 0.5;
        });

        let mut world = s.build();
        world.current_time = SimTimestamp::from_year(100);

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::SettlementCaptured {
                settlement_id: kingdom_a.settlement,
                old_faction_id: kingdom_a.faction,
                new_faction_id: kingdom_b.faction,
            },
        }];
        testutil::deliver_signals(&mut world, &mut ItemSystem, &inbox, 42);

        // Item should be looted to a settlement of the conquering faction
        assert!(
            world.entities[&item].has_active_rel(RelationshipKind::HeldBy, kingdom_b.settlement),
            "notable item should be looted to conquering faction's settlement"
        );
        // Should no longer be held by the original settlement
        assert!(
            !world.entities[&item].has_active_rel(RelationshipKind::HeldBy, kingdom_a.settlement),
            "item should no longer be held by captured settlement"
        );
    }

    #[test]
    fn scenario_siege_conquered_adds_resonance() {
        let mut s = Scenario::at_year(100);
        let setup = s.add_settlement_standalone("Town");
        let item = s.add_item(ItemType::Idol, "stone", setup.settlement);

        let mut world = s.build();
        world.current_time = SimTimestamp::from_year(100);

        let inbox = vec![Signal {
            event_id: 0,
            kind: SignalKind::SiegeEnded {
                settlement_id: setup.settlement,
                attacker_faction_id: 999,
                defender_faction_id: setup.faction,
                outcome: SiegeOutcome::Conquered,
            },
        }];
        testutil::deliver_signals(&mut world, &mut ItemSystem, &inbox, 42);

        let resonance = world.item(item).resonance;
        assert!(
            (resonance - SIEGE_SURVIVAL_RESONANCE).abs() < f64::EPSILON,
            "item resonance should be {SIEGE_SURVIVAL_RESONANCE} after siege, got {resonance}"
        );
    }

    #[test]
    fn scenario_bandit_raid_steals_items() {
        let mut stolen = false;
        for seed in 0..100u64 {
            let mut s = Scenario::at_year(100);
            let setup = s.add_settlement_standalone("Town");
            let item = s.add_item_with(ItemType::Amulet, "gold", setup.settlement, |id| {
                id.resonance = 0.5;
            });
            let bandit_region = s.add_region("BanditLands");
            let bandit_faction = s.faction("Bandits").id();
            let hideout = s
                .settlement("Hideout", bandit_faction, bandit_region)
                .population(30)
                .id();

            let mut world = s.build();
            world.current_time = SimTimestamp::from_year(100);

            let inbox = vec![Signal {
                event_id: 0,
                kind: SignalKind::BanditRaid {
                    bandit_faction_id: bandit_faction,
                    settlement_id: setup.settlement,
                    population_lost: 0,
                    treasury_stolen: 0.0,
                },
            }];
            testutil::deliver_signals(&mut world, &mut ItemSystem, &inbox, seed);

            if world.entities[&item].has_active_rel(RelationshipKind::HeldBy, hideout) {
                stolen = true;
                break;
            }
        }
        assert!(
            stolen,
            "bandit raid should steal notable items within 100 attempts"
        );
    }
}
