//! Items system — migrated from `src/sim/items.rs`.
//!
//! Five chained yearly systems (Update phase):
//! 1. `craft_items` — settlements with pop >= 100 may craft items
//! 2. `age_resonance` — passive resonance gain from age
//! 3. `owner_resonance` — prestige-based resonance from holder
//! 4. `check_tier_promotions` — detect tier threshold crossings
//! 5. `decay_condition` — condition decay, destroy at 0
//!
//! One reaction system (Reactions phase):
//! 6. `handle_item_events` — EntityDied, SettlementCaptured, SiegeEnded, BanditRaid

use bevy_app::{App, Plugin};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::query::With;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res, ResMut};
use rand::Rng;

use crate::ecs::clock::SimClock;
use crate::ecs::commands::{SimCommand, SimCommandKind};
use crate::ecs::components::{
    EcsBuildingBonuses, Faction, FactionCore, ItemMarker, ItemState, Person, PersonReputation,
    Settlement, SettlementCore, SimEntity,
};
use crate::ecs::conditions::yearly;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::{HeldBy, LeaderOf, MemberOf};
use crate::ecs::resources::ItemsRng;
use crate::ecs::schedule::{DomainSet, SimPhase, SimTick};
use crate::model::event::{EventKind, ParticipantRole};
use crate::model::{ItemType, ResourceType};

// ---------------------------------------------------------------------------
// Crafting parameters
// ---------------------------------------------------------------------------
const CRAFT_MIN_POP: u32 = 100;
const CRAFT_BASE_PROB: f64 = 0.03;
const CRAFT_WORKSHOP_BONUS: f64 = 0.02;
const CRAFT_TREASURY_COST: f64 = 5.0;

// ---------------------------------------------------------------------------
// Resonance accumulation
// ---------------------------------------------------------------------------
const AGE_RESONANCE_PER_YEAR: f64 = 0.001;
const AGE_RESONANCE_CAP: f64 = 0.3;
const OWNER_PERSON_PRESTIGE_FACTOR: f64 = 0.02;
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
const DECAY_HELD_BY_PERSON: f64 = 0.003;
const DECAY_HELD_BY_SETTLEMENT: f64 = 0.005;
const DECAY_UNOWNED: f64 = 0.01;

// ---------------------------------------------------------------------------
// Signal response parameters
// ---------------------------------------------------------------------------
const SIEGE_SURVIVAL_RESONANCE: f64 = 0.05;
const DEATH_TRANSFER_RESONANCE: f64 = 0.02;
const NOTABLE_RESONANCE_THRESHOLD: f64 = 0.3;
const BANDIT_STEAL_PROB: f64 = 0.2;

// ---------------------------------------------------------------------------
// Material tables
// ---------------------------------------------------------------------------
const METAL_MATERIALS: &[&str] = &["iron", "bronze", "copper", "steel", "gold", "silver"];
const STONE_MATERIALS: &[&str] = &["granite", "obsidian", "marble", "basalt"];
const ORGANIC_MATERIALS: &[&str] = &["bone", "wood", "ivory", "horn"];
const PRECIOUS_MATERIALS: &[&str] = &["gold", "silver", "jade", "amber"];

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SimTick,
            (
                craft_items,
                age_resonance,
                owner_resonance,
                check_tier_promotions,
                decay_condition,
            )
                .chain()
                .run_if(yearly)
                .in_set(DomainSet::Items),
        );
        app.add_systems(SimTick, handle_item_events.in_set(SimPhase::Reactions));
    }
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

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn pick_item_type(rng: &mut impl Rng, resources: &[ResourceType]) -> ItemType {
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

fn pick_material(rng: &mut impl Rng, resources: &[ResourceType]) -> String {
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

// ---------------------------------------------------------------------------
// System 1: Craft items (yearly)
// ---------------------------------------------------------------------------

fn craft_items(
    settlements: Query<
        (
            Entity,
            &SettlementCore,
            &EcsBuildingBonuses,
            &SimEntity,
            &MemberOf,
        ),
        With<Settlement>,
    >,
    mut factions: Query<&mut FactionCore, With<Faction>>,
    mut rng: ResMut<ItemsRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    // Collect crafting candidates to avoid borrow conflicts
    struct CraftCandidate {
        settlement: Entity,
        faction: Entity,
        resources: Vec<ResourceType>,
        has_workshop: bool,
    }

    let candidates: Vec<CraftCandidate> = settlements
        .iter()
        .filter(|(_, core, _, sim, _)| sim.is_alive() && core.population >= CRAFT_MIN_POP)
        .filter_map(|(entity, core, bonuses, _, member_of)| {
            let faction = member_of.0;
            let Ok(faction_core) = factions.get(faction) else {
                return None;
            };
            if faction_core.treasury < CRAFT_TREASURY_COST {
                return None;
            }
            Some(CraftCandidate {
                settlement: entity,
                faction,
                resources: core.resources.clone(),
                has_workshop: bonuses.workshop > 0.0,
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
        if !rng.0.random_bool(prob) {
            continue;
        }

        let item_type = pick_item_type(&mut rng.0, &c.resources);
        let material = pick_material(&mut rng.0, &c.resources);
        let name = format!("{} {}", capitalize(&material), item_type);

        // Deduct treasury
        if let Ok(mut faction_core) = factions.get_mut(c.faction) {
            faction_core.treasury = (faction_core.treasury - CRAFT_TREASURY_COST).max(0.0);
        }

        commands.write(
            SimCommand::new(
                SimCommandKind::CraftItem {
                    crafter: c.settlement,
                    settlement: c.settlement,
                    name: name.clone(),
                    item_type,
                    material,
                },
                EventKind::Crafted,
                format!("{name} crafted"),
            )
            .with_participant(c.settlement, ParticipantRole::Location),
        );
    }
}

// ---------------------------------------------------------------------------
// System 2: Age resonance (yearly)
// ---------------------------------------------------------------------------

fn age_resonance(mut items: Query<&mut ItemState, With<ItemMarker>>) {
    for mut state in &mut items {
        if state.resonance < AGE_RESONANCE_CAP {
            state.resonance = (state.resonance + AGE_RESONANCE_PER_YEAR).min(AGE_RESONANCE_CAP);
        }
    }
}

// ---------------------------------------------------------------------------
// System 3: Owner resonance (yearly)
// ---------------------------------------------------------------------------

fn owner_resonance(
    mut items: Query<(&mut ItemState, &HeldBy, &SimEntity), With<ItemMarker>>,
    persons: Query<&PersonReputation, With<Person>>,
    settlement_cores: Query<&SettlementCore, With<Settlement>>,
) {
    for (mut state, held_by, sim) in &mut items {
        if !sim.is_alive() {
            continue;
        }

        let holder = held_by.0;
        let prestige_bonus = if let Ok(rep) = persons.get(holder) {
            rep.prestige * OWNER_PERSON_PRESTIGE_FACTOR
        } else if let Ok(core) = settlement_cores.get(holder) {
            core.prestige * OWNER_SETTLEMENT_PRESTIGE_FACTOR
        } else {
            0.0
        };

        if prestige_bonus > 0.0 {
            state.resonance = (state.resonance + prestige_bonus).min(1.0);
        }
    }
}

// ---------------------------------------------------------------------------
// System 4: Check tier promotions (yearly)
// ---------------------------------------------------------------------------

fn check_tier_promotions(
    mut items: Query<(Entity, &mut ItemState, &SimEntity), With<ItemMarker>>,
    mut commands: MessageWriter<SimCommand>,
) {
    for (entity, mut state, sim) in &mut items {
        if !sim.is_alive() {
            continue;
        }
        let new_tier = resonance_tier(state.resonance);
        if new_tier != state.resonance_tier {
            let old_tier = state.resonance_tier;
            state.resonance_tier = new_tier;

            commands.write(
                SimCommand::new(
                    SimCommandKind::SetField {
                        entity,
                        field: "resonance_tier".to_string(),
                        old_value: serde_json::json!(old_tier),
                        new_value: serde_json::json!(new_tier),
                    },
                    EventKind::Upgrade,
                    format!("{} reached tier {new_tier}", sim.name),
                )
                .with_participant(entity, ParticipantRole::Subject),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// System 5: Decay condition (yearly)
// ---------------------------------------------------------------------------

fn decay_condition(
    mut items: Query<(Entity, &mut ItemState, &SimEntity, Option<&HeldBy>), With<ItemMarker>>,
    persons: Query<&SimEntity, With<Person>>,
    settlements: Query<&SimEntity, With<Settlement>>,
    mut commands: MessageWriter<SimCommand>,
) {
    for (entity, mut state, sim, held_by) in &mut items {
        if !sim.is_alive() {
            continue;
        }

        let decay = if let Some(holder) = held_by {
            if persons.get(holder.0).is_ok_and(|s| s.is_alive()) {
                DECAY_HELD_BY_PERSON
            } else if settlements.get(holder.0).is_ok_and(|s| s.is_alive()) {
                DECAY_HELD_BY_SETTLEMENT
            } else {
                DECAY_UNOWNED
            }
        } else {
            DECAY_UNOWNED
        };

        state.condition = (state.condition - decay).max(0.0);

        if state.condition <= 0.0 {
            commands.write(
                SimCommand::new(
                    SimCommandKind::EndEntity { entity },
                    EventKind::Destruction,
                    format!("{} crumbled to dust", sim.name),
                )
                .with_participant(entity, ParticipantRole::Subject),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Reaction system: handle cross-system events
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_item_events(
    mut events: MessageReader<SimReactiveEvent>,
    mut items: Query<(Entity, &mut ItemState, &SimEntity, Option<&HeldBy>), With<ItemMarker>>,
    persons: Query<(Entity, &SimEntity, Option<&MemberOf>), With<Person>>,
    faction_leaders: Query<(Entity, &LeaderOf)>,
    settlements: Query<(Entity, &SimEntity, Option<&MemberOf>), With<Settlement>>,
    clock: Res<SimClock>,
    mut rng: ResMut<ItemsRng>,
    mut commands: MessageWriter<SimCommand>,
) {
    for event in events.read() {
        match event {
            SimReactiveEvent::EntityDied { entity, .. } => {
                // Only handle person deaths
                let Ok((_, person_sim, person_member)) = persons.get(*entity) else {
                    continue;
                };
                if person_sim.end.is_none() {
                    continue;
                }

                // Find items held by the deceased
                let held_items: Vec<(Entity, f64)> = items
                    .iter()
                    .filter(|(_, _, sim, held_by)| {
                        sim.is_alive() && held_by.is_some_and(|h| h.0 == *entity)
                    })
                    .map(|(e, state, _, _)| (e, state.resonance))
                    .collect();

                if held_items.is_empty() {
                    continue;
                }

                // Find new holder: faction leader or any faction settlement
                let faction = person_member.map(|m| m.0);
                let new_holder = faction
                    .and_then(|f| {
                        faction_leaders
                            .iter()
                            .find(|(_, lo)| lo.0 == f)
                            .map(|(e, _)| e)
                    })
                    .filter(|leader| *leader != *entity)
                    .or_else(|| {
                        faction.and_then(|f| {
                            settlements
                                .iter()
                                .find(|(_, sim, member)| {
                                    sim.is_alive() && member.is_some_and(|m| m.0 == f)
                                })
                                .map(|(e, _, _)| e)
                        })
                    });

                for (item_entity, old_res) in held_items {
                    if let Ok((_, mut state, _, _)) = items.get_mut(item_entity) {
                        state.resonance = (old_res + DEATH_TRANSFER_RESONANCE).min(1.0);
                        state.last_transferred = Some(clock.time);
                    }

                    if let Some(holder) = new_holder {
                        commands.write(SimCommand::bookkeeping(SimCommandKind::TransferItem {
                            item: item_entity,
                            new_holder: holder,
                        }));
                    }
                }
            }

            SimReactiveEvent::SettlementCaptured {
                settlement,
                new_faction,
                ..
            } => {
                // Notable items at settlement are looted
                let notable_items: Vec<Entity> = items
                    .iter()
                    .filter(|(_, state, sim, held_by)| {
                        sim.is_alive()
                            && held_by.is_some_and(|h| h.0 == *settlement)
                            && state.resonance > NOTABLE_RESONANCE_THRESHOLD
                    })
                    .map(|(e, _, _, _)| e)
                    .collect();

                if notable_items.is_empty() {
                    continue;
                }

                // Find a settlement of the conquering faction
                let receiver = settlements
                    .iter()
                    .find(|(e, sim, member)| {
                        sim.is_alive()
                            && *e != *settlement
                            && member.is_some_and(|m| m.0 == *new_faction)
                    })
                    .map(|(e, _, _)| e)
                    .unwrap_or(*settlement);

                for item_entity in notable_items {
                    if let Ok((_, mut state, _, _)) = items.get_mut(item_entity) {
                        state.last_transferred = Some(clock.time);
                    }
                    commands.write(SimCommand::bookkeeping(SimCommandKind::TransferItem {
                        item: item_entity,
                        new_holder: receiver,
                    }));
                }
            }

            SimReactiveEvent::SiegeEnded { settlement, .. } => {
                // Items at the settlement gain siege survival resonance
                let siege_items: Vec<(Entity, f64)> = items
                    .iter()
                    .filter(|(_, _, sim, held_by)| {
                        sim.is_alive() && held_by.is_some_and(|h| h.0 == *settlement)
                    })
                    .map(|(e, state, _, _)| (e, state.resonance))
                    .collect();

                for (item_entity, old_res) in siege_items {
                    if let Ok((_, mut state, _, _)) = items.get_mut(item_entity) {
                        state.resonance = (old_res + SIEGE_SURVIVAL_RESONANCE).min(1.0);
                    }
                }
            }

            SimReactiveEvent::BanditRaid { settlement, .. } => {
                // 20% chance bandits steal a notable item
                if !rng.0.random_bool(BANDIT_STEAL_PROB) {
                    continue;
                }

                let stolen = items
                    .iter()
                    .find(|(_, state, sim, held_by)| {
                        sim.is_alive()
                            && held_by.is_some_and(|h| h.0 == *settlement)
                            && state.resonance > NOTABLE_RESONANCE_THRESHOLD
                    })
                    .map(|(e, _, _, _)| e);

                if let Some(item_entity) = stolen {
                    if let Ok((_, mut state, _, _)) = items.get_mut(item_entity) {
                        state.last_transferred = Some(clock.time);
                    }
                    // Bandits steal the item — remove HeldBy (item is now unowned)
                    commands.write(SimCommand::bookkeeping(SimCommandKind::EndRelationship {
                        source: item_entity,
                        target: *settlement,
                        kind: crate::model::relationship::RelationshipKind::HeldBy,
                    }));
                }
            }

            _ => {}
        }
    }
}
