use bevy_ecs::entity::Entity;
use bevy_ecs::message::Messages;
use bevy_ecs::world::World;

use crate::ecs::clock::SimClock;
use crate::ecs::events::SimReactiveEvent;
use crate::ecs::relationships::RelationshipGraph;
use crate::ecs::resources::event_log::EcsEvent;
use crate::ecs::resources::{EcsIdGenerator, EventLog, SimEntityMap, SimRng};
use crate::ecs::time::SimTime;
use crate::model::effect::{EventEffect, StateChange};
use crate::model::event::EventParticipant;

use super::apply_buildings;
use super::apply_crime;
use super::apply_culture;
use super::apply_demographics;
use super::apply_disease;
use super::apply_economy;
use super::apply_environment;
use super::apply_faction_stats;
use super::apply_items;
use super::apply_knowledge;
use super::apply_lifecycle;
use super::apply_migration;
use super::apply_military;
use super::apply_politics;
use super::apply_relationship;
use super::apply_religion;
use super::apply_reputation;
use super::apply_set_field;
use super::{SimCommand, SimCommandKind};

/// Context passed to all `apply_*` sub-functions, providing mutable access
/// to the resources they need without requiring direct World access.
pub(crate) struct ApplyCtx {
    pub event_log: EventLog,
    pub id_gen: EcsIdGenerator,
    pub entity_map: SimEntityMap,
    pub rel_graph: RelationshipGraph,
    pub clock_time: SimTime,
    pub reactive_events: Vec<SimReactiveEvent>,
}

impl ApplyCtx {
    /// Record an Event entry in the log for a non-bookkeeping command.
    /// Returns the event_id (0 for bookkeeping commands that skip recording).
    pub(crate) fn record_event(&mut self, cmd: &SimCommand) -> u64 {
        if cmd.is_bookkeeping() {
            return 0;
        }

        let event_id = self.id_gen.0.next_id();

        self.event_log.events.push(EcsEvent {
            id: event_id,
            kind: cmd.event_kind.clone(),
            timestamp: self.clock_time,
            description: cmd.description.clone(),
            caused_by: cmd.caused_by,
            data: cmd.event_data.clone(),
        });

        for (entity, role) in &cmd.participants {
            if let Some(sim_id) = self.entity_map.get_sim(*entity) {
                self.event_log.participants.push(EventParticipant {
                    event_id,
                    entity_id: sim_id,
                    role: role.clone(),
                });
            }
        }

        event_id
    }

    /// Record a state-change effect against an entity.
    pub(crate) fn record_effect(&mut self, event_id: u64, entity: Entity, change: StateChange) {
        let entity_id = self.entity_map.get_sim(entity).unwrap_or(0);
        self.event_log.effects.push(EventEffect {
            event_id,
            entity_id,
            effect: change,
        });
    }

    /// Queue a reactive event for emission after all commands are processed.
    pub(crate) fn emit(&mut self, event: SimReactiveEvent) {
        self.reactive_events.push(event);
    }
}

/// Exclusive system that drains all pending `SimCommand` messages, applies
/// state changes, records audit trail, and emits `SimReactiveEvent` messages.
///
/// Runs in `SimPhase::PostUpdate`.
pub fn apply_sim_commands(world: &mut World) {
    // Drain all pending commands
    let commands: Vec<SimCommand> = {
        let Some(mut messages) = world.get_resource_mut::<Messages<SimCommand>>() else {
            return;
        };
        messages.drain().collect()
    };

    if commands.is_empty() {
        return;
    }

    // Extract resources into ApplyCtx
    let clock_time = world.resource::<SimClock>().time;
    let event_log = world.remove_resource::<EventLog>().unwrap();
    let id_gen = world.remove_resource::<EcsIdGenerator>().unwrap();
    let entity_map = world.remove_resource::<SimEntityMap>().unwrap();
    let rel_graph = world.remove_resource::<RelationshipGraph>().unwrap();

    let mut ctx = ApplyCtx {
        event_log,
        id_gen,
        entity_map,
        rel_graph,
        clock_time,
        reactive_events: Vec::new(),
    };

    // Process each command
    for cmd in &commands {
        let event_id = ctx.record_event(cmd);

        match &cmd.kind {
            // Entity Lifecycle
            SimCommandKind::EndEntity { entity } => {
                apply_lifecycle::apply_end_entity(&mut ctx, world, event_id, *entity);
            }
            SimCommandKind::RenameEntity { entity, new_name } => {
                apply_lifecycle::apply_rename_entity(&mut ctx, world, event_id, *entity, new_name);
            }

            // Relationships
            SimCommandKind::AddRelationship {
                source,
                target,
                kind,
            } => {
                apply_relationship::apply_add_relationship(
                    &mut ctx, world, event_id, *source, *target, kind,
                );
            }
            SimCommandKind::EndRelationship {
                source,
                target,
                kind,
            } => {
                apply_relationship::apply_end_relationship(
                    &mut ctx, world, event_id, *source, *target, kind,
                );
            }

            // Demographics
            SimCommandKind::GrowPopulation {
                settlement,
                new_total,
            } => {
                apply_demographics::apply_grow_population(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *new_total,
                );
            }
            SimCommandKind::PersonDied { person } => {
                apply_demographics::apply_person_died(&mut ctx, world, event_id, *person);
            }
            SimCommandKind::PersonBorn {
                name,
                faction,
                settlement,
                sex,
                role,
                traits,
                culture_id,
                father,
                mother,
            } => {
                apply_demographics::apply_person_born(
                    &mut ctx,
                    world,
                    event_id,
                    name,
                    *faction,
                    *settlement,
                    *sex,
                    role,
                    traits,
                    *culture_id,
                    *father,
                    *mother,
                );
            }
            SimCommandKind::Marriage { person_a, person_b } => {
                apply_demographics::apply_marriage(&mut ctx, world, event_id, *person_a, *person_b);
            }

            // Military
            SimCommandKind::DeclareWar { attacker, defender } => {
                apply_military::apply_declare_war(&mut ctx, world, event_id, *attacker, *defender);
            }
            SimCommandKind::CaptureSettlement {
                settlement,
                new_faction,
            } => {
                apply_military::apply_capture_settlement(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *new_faction,
                );
            }
            SimCommandKind::MusterArmy { faction, region } => {
                apply_military::apply_muster_army(&mut ctx, world, event_id, *faction, *region);
            }
            SimCommandKind::MarchArmy {
                army,
                target_region,
            } => {
                apply_military::apply_march_army(&mut ctx, world, event_id, *army, *target_region);
            }
            SimCommandKind::ResolveBattle {
                attacker_army,
                defender_army,
                attacker_casualties,
                defender_casualties,
                attacker_won,
            } => {
                apply_military::apply_resolve_battle(
                    &mut ctx,
                    world,
                    event_id,
                    *attacker_army,
                    *defender_army,
                    *attacker_casualties,
                    *defender_casualties,
                    *attacker_won,
                );
            }
            SimCommandKind::BeginSiege { army, settlement } => {
                apply_military::apply_begin_siege(&mut ctx, world, event_id, *army, *settlement);
            }
            SimCommandKind::ResolveAssault {
                army,
                settlement,
                succeeded,
                attacker_casualties,
                defender_casualties,
            } => {
                apply_military::apply_resolve_assault(
                    &mut ctx,
                    world,
                    event_id,
                    *army,
                    *settlement,
                    *succeeded,
                    *attacker_casualties,
                    *defender_casualties,
                );
            }
            SimCommandKind::SignTreaty {
                faction_a,
                faction_b,
                winner,
                loser,
                decisive,
            } => {
                apply_military::apply_sign_treaty(
                    &mut ctx, world, event_id, *faction_a, *faction_b, *winner, *loser, *decisive,
                );
            }
            SimCommandKind::DisbandArmy { army } => {
                apply_military::apply_disband_army(&mut ctx, world, event_id, *army);
            }
            SimCommandKind::CreateMercenaryCompany {
                region,
                strength,
                name,
            } => {
                let mut rng = world.remove_resource::<SimRng>().unwrap();
                apply_military::apply_create_mercenary_company(
                    &mut ctx,
                    world,
                    event_id,
                    *region,
                    *strength,
                    name.clone(),
                    &mut rng.0,
                );
                world.insert_resource(rng);
            }
            SimCommandKind::HireMercenary {
                employer,
                mercenary,
                wage,
            } => {
                apply_military::apply_hire_mercenary(
                    &mut ctx, world, event_id, *employer, *mercenary, *wage,
                );
            }
            SimCommandKind::EndMercenaryContract { mercenary } => {
                apply_military::apply_end_mercenary_contract(&mut ctx, world, event_id, *mercenary);
            }

            // Environment
            SimCommandKind::TriggerDisaster {
                settlement,
                disaster_type,
                severity,
                pop_loss_frac,
                building_damage,
                prosperity_hit,
                sever_trade,
                create_feature,
            } => {
                apply_environment::apply_trigger_disaster(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *disaster_type,
                    *severity,
                    *pop_loss_frac,
                    *building_damage,
                    *prosperity_hit,
                    *sever_trade,
                    create_feature,
                );
            }
            SimCommandKind::StartPersistentDisaster {
                settlement,
                disaster_type,
                severity,
                months,
            } => {
                apply_environment::apply_start_persistent_disaster(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *disaster_type,
                    *severity,
                    *months,
                );
            }
            SimCommandKind::EndDisaster { settlement } => {
                apply_environment::apply_end_disaster(&mut ctx, world, event_id, *settlement);
            }
            SimCommandKind::CreateGeographicFeature {
                name,
                region,
                feature_type,
                x,
                y,
            } => {
                apply_environment::apply_create_geographic_feature(
                    &mut ctx,
                    world,
                    event_id,
                    name,
                    *region,
                    feature_type,
                    *x,
                    *y,
                );
            }

            // Buildings
            SimCommandKind::ConstructBuilding {
                settlement,
                faction,
                building_type,
                cost,
                x,
                y,
            } => {
                apply_buildings::apply_construct_building(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *faction,
                    *building_type,
                    *cost,
                    *x,
                    *y,
                );
            }
            SimCommandKind::DamageBuilding {
                building, damage, ..
            } => {
                apply_buildings::apply_damage_building(
                    &mut ctx, world, event_id, *building, *damage,
                );
            }
            SimCommandKind::UpgradeBuilding {
                building,
                new_level,
                cost,
                faction,
            } => {
                apply_buildings::apply_upgrade_building(
                    &mut ctx, world, event_id, *building, *new_level, *cost, *faction,
                );
            }

            // Generic
            SimCommandKind::SetField {
                entity,
                field,
                old_value,
                new_value,
            } => {
                apply_set_field::apply_set_field(
                    &mut ctx, event_id, *entity, field, old_value, new_value,
                );
            }

            // Economy
            SimCommandKind::EstablishTradeRoute {
                settlement_a,
                settlement_b,
            } => {
                apply_economy::apply_establish_trade_route(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement_a,
                    *settlement_b,
                );
            }
            SimCommandKind::SeverTradeRoute {
                settlement_a,
                settlement_b,
            } => {
                apply_economy::apply_sever_trade_route(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement_a,
                    *settlement_b,
                );
            }

            // Disease
            SimCommandKind::StartPlague {
                settlement,
                disease_name,
                virulence,
                lethality,
                duration_years,
                bracket_severity,
            } => {
                apply_disease::apply_start_plague(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    disease_name,
                    *virulence,
                    *lethality,
                    *duration_years,
                    bracket_severity,
                );
            }
            SimCommandKind::EndPlague { settlement } => {
                apply_disease::apply_end_plague(&mut ctx, world, event_id, *settlement);
            }
            SimCommandKind::SpreadPlague {
                from_settlement: _,
                to_settlement,
                disease_name,
                virulence,
                lethality,
                duration_years,
                bracket_severity,
            } => {
                apply_disease::apply_spread_plague(
                    &mut ctx,
                    world,
                    event_id,
                    *to_settlement,
                    disease_name,
                    *virulence,
                    *lethality,
                    *duration_years,
                    bracket_severity,
                );
            }

            // Reputation
            SimCommandKind::AdjustPrestige { entity, delta } => {
                apply_reputation::apply_adjust_prestige(&mut ctx, world, event_id, *entity, *delta);
            }
            SimCommandKind::UpdatePrestigeTier { entity, new_tier } => {
                apply_reputation::apply_update_prestige_tier(
                    &mut ctx, world, event_id, *entity, *new_tier,
                );
            }

            // Crime
            SimCommandKind::FormBanditGang { region } => {
                let mut rng = world.remove_resource::<SimRng>().unwrap();
                apply_crime::apply_form_bandit_gang(&mut ctx, world, event_id, *region, &mut rng.0);
                world.insert_resource(rng);
            }
            SimCommandKind::BanditRaid { settlement } => {
                apply_crime::apply_bandit_raid(&mut ctx, world, event_id, *settlement);
            }
            SimCommandKind::RaidTradeRoute {
                bandit_faction,
                settlement_a,
                settlement_b,
                sever,
            } => {
                apply_crime::apply_raid_trade_route(
                    &mut ctx,
                    world,
                    event_id,
                    *bandit_faction,
                    *settlement_a,
                    *settlement_b,
                    *sever,
                );
            }
            SimCommandKind::DisbandBanditGang { faction } => {
                apply_crime::apply_disband_bandit_gang(&mut ctx, world, event_id, *faction);
            }

            // Culture
            SimCommandKind::CulturalShift {
                settlement,
                new_culture,
            } => {
                apply_culture::apply_cultural_shift(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *new_culture,
                );
            }
            SimCommandKind::BlendCultures {
                settlement,
                parent_culture_a,
                parent_culture_b,
                new_name,
                values,
                naming_style,
                resistance,
            } => {
                apply_culture::apply_blend_cultures(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *parent_culture_a,
                    *parent_culture_b,
                    new_name,
                    values,
                    naming_style.clone(),
                    *resistance,
                );
            }
            SimCommandKind::CulturalRebellion {
                settlement,
                rebel_culture,
                succeeded,
                new_faction_name,
            } => {
                apply_culture::apply_cultural_rebellion(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *rebel_culture,
                    *succeeded,
                    new_faction_name,
                );
            }

            // Religion
            SimCommandKind::FoundReligion { founder, name } => {
                apply_religion::apply_found_religion(&mut ctx, world, event_id, *founder, name);
            }
            SimCommandKind::ReligiousSchism {
                parent_religion,
                settlement,
                new_name,
                tenets,
            } => {
                apply_religion::apply_religious_schism(
                    &mut ctx,
                    world,
                    event_id,
                    *parent_religion,
                    *settlement,
                    new_name,
                    tenets,
                );
            }
            SimCommandKind::ConvertFaction { faction, religion } => {
                apply_religion::apply_convert_faction(
                    &mut ctx, world, event_id, *faction, *religion,
                );
            }
            SimCommandKind::SpreadReligion {
                settlement,
                religion,
                share,
            } => {
                apply_religion::apply_spread_religion(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *religion,
                    *share,
                );
            }
            SimCommandKind::DeclareProphecy {
                settlement,
                religion,
                prophet,
            } => {
                apply_religion::apply_declare_prophecy(
                    &mut ctx,
                    world,
                    event_id,
                    *settlement,
                    *religion,
                    *prophet,
                );
            }

            // Items
            SimCommandKind::CraftItem {
                crafter,
                settlement,
                name,
                item_type,
                material,
            } => {
                apply_items::apply_craft_item(
                    &mut ctx,
                    world,
                    event_id,
                    *crafter,
                    *settlement,
                    name,
                    *item_type,
                    material,
                );
            }
            SimCommandKind::TransferItem { item, new_holder } => {
                apply_items::apply_transfer_item(&mut ctx, world, event_id, *item, *new_holder);
            }

            // Knowledge
            SimCommandKind::CreateKnowledge {
                name,
                settlement,
                category,
                significance,
                ground_truth,
                is_secret,
                secret_sensitivity,
                secret_motivation,
            } => {
                apply_knowledge::apply_create_knowledge(
                    &mut ctx,
                    world,
                    event_id,
                    name,
                    *settlement,
                    *category,
                    *significance,
                    ground_truth,
                    *is_secret,
                    *secret_sensitivity,
                    *secret_motivation,
                );
            }
            SimCommandKind::CreateManifestation {
                knowledge,
                settlement,
                medium,
                content,
                accuracy,
                completeness,
                distortions,
                derived_from_id,
                derivation_method,
            } => {
                apply_knowledge::apply_create_manifestation(
                    &mut ctx,
                    world,
                    event_id,
                    *knowledge,
                    *settlement,
                    *medium,
                    content,
                    *accuracy,
                    *completeness,
                    distortions,
                    *derived_from_id,
                    derivation_method,
                );
            }
            SimCommandKind::DestroyManifestation { manifestation } => {
                apply_knowledge::apply_destroy_manifestation(
                    &mut ctx,
                    world,
                    event_id,
                    *manifestation,
                );
            }
            SimCommandKind::RevealSecret { knowledge } => {
                apply_knowledge::apply_reveal_secret(&mut ctx, world, event_id, *knowledge);
            }

            // Politics
            SimCommandKind::SucceedLeader {
                faction,
                new_leader,
            } => {
                apply_politics::apply_succeed_leader(
                    &mut ctx,
                    world,
                    event_id,
                    *faction,
                    *new_leader,
                );
            }
            SimCommandKind::AttemptCoup {
                faction,
                instigator,
                succeeded,
                execute_instigator,
            } => {
                apply_politics::apply_attempt_coup(
                    &mut ctx,
                    world,
                    event_id,
                    *faction,
                    *instigator,
                    *succeeded,
                    *execute_instigator,
                );
            }
            SimCommandKind::FormAlliance {
                faction_a,
                faction_b,
            } => {
                apply_politics::apply_form_alliance(
                    &mut ctx, world, event_id, *faction_a, *faction_b,
                );
            }
            SimCommandKind::BetrayAlliance { betrayer, betrayed } => {
                apply_politics::apply_betray_alliance(
                    &mut ctx, world, event_id, *betrayer, *betrayed,
                );
            }
            SimCommandKind::SplitFaction {
                parent_faction,
                new_faction_name,
                settlement,
            } => {
                apply_politics::apply_split_faction(
                    &mut ctx,
                    world,
                    event_id,
                    *parent_faction,
                    new_faction_name.clone(),
                    *settlement,
                );
            }

            // Migration
            SimCommandKind::MigratePopulation {
                from_settlement,
                to_settlement,
                count,
            } => {
                apply_migration::apply_migrate_population(
                    &mut ctx,
                    world,
                    event_id,
                    *from_settlement,
                    *to_settlement,
                    *count,
                );
            }
            SimCommandKind::RelocatePerson {
                person,
                to_settlement,
            } => {
                apply_migration::apply_relocate_person(
                    &mut ctx,
                    world,
                    event_id,
                    *person,
                    *to_settlement,
                );
            }
            SimCommandKind::AbandonSettlement { settlement } => {
                apply_migration::apply_abandon_settlement(&mut ctx, world, event_id, *settlement);
            }

            // Faction Stats
            SimCommandKind::AdjustFactionStats {
                faction,
                stability_delta,
                happiness_delta,
                legitimacy_delta,
                trust_delta,
                prestige_delta,
            } => {
                apply_faction_stats::apply_adjust_faction_stats(
                    &mut ctx,
                    world,
                    event_id,
                    *faction,
                    *stability_delta,
                    *happiness_delta,
                    *legitimacy_delta,
                    *trust_delta,
                    *prestige_delta,
                );
            }
            SimCommandKind::SetWarGoal {
                faction,
                target_faction,
                goal,
            } => {
                apply_faction_stats::apply_set_war_goal(
                    &mut ctx,
                    world,
                    event_id,
                    *faction,
                    *target_faction,
                    goal,
                );
            }

            // Explicitly listed unimplemented variants (no wildcard — compiler
            // will flag new variants that need handling)
            SimCommandKind::CollectTaxes { .. }
            | SimCommandKind::PayArmyMaintenance { .. }
            | SimCommandKind::UpdateProduction { .. }
            | SimCommandKind::UpdateInfection { .. } => {
                tracing::warn!("Unimplemented SimCommandKind: {:?}", cmd.kind);
            }
        }
    }

    // Write reactive events
    let reactive_events = std::mem::take(&mut ctx.reactive_events);
    if let Some(mut messages) = world.get_resource_mut::<Messages<SimReactiveEvent>>() {
        messages.write_batch(reactive_events);
    }

    // Put resources back
    world.insert_resource(ctx.event_log);
    world.insert_resource(ctx.id_gen);
    world.insert_resource(ctx.entity_map);
    world.insert_resource(ctx.rel_graph);
}

#[cfg(test)]
mod tests {
    use crate::ecs::app::build_sim_app;
    use crate::ecs::commands::{SimCommand, SimCommandKind};
    use crate::ecs::components::common::SimEntity;
    use crate::ecs::components::{
        Faction, FactionCore, Person, PersonCore, PersonEducation, PersonReputation, PersonSocial,
        Settlement, SettlementCore,
    };
    use crate::ecs::events::SimReactiveEvent;
    use crate::ecs::relationships::{LeaderOf, MemberOf, RelationshipGraph};
    use crate::ecs::resources::{EventLog, SimEntityMap};
    use crate::ecs::schedule::SimTick;
    use crate::ecs::time::SimTime;
    use crate::model::effect::StateChange;
    use crate::model::event::{EventKind, ParticipantRole};
    use crate::model::relationship::RelationshipKind;

    use bevy_ecs::message::Messages;
    use bevy_ecs::world::World;

    use super::*;

    /// Helper: spawn a minimal person entity with sim_id registration.
    fn spawn_test_person(world: &mut World, sim_id: u64, name: &str) -> Entity {
        let entity = world
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Person,
                PersonCore::default(),
                PersonReputation::default(),
                PersonSocial::default(),
                PersonEducation::default(),
            ))
            .id();
        world.resource_mut::<SimEntityMap>().insert(sim_id, entity);
        entity
    }

    /// Helper: spawn a minimal faction entity.
    fn spawn_test_faction(world: &mut World, sim_id: u64, name: &str) -> Entity {
        let entity = world
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Faction,
                FactionCore::default(),
            ))
            .id();
        world.resource_mut::<SimEntityMap>().insert(sim_id, entity);
        entity
    }

    /// Helper: spawn a minimal settlement entity.
    fn spawn_test_settlement(world: &mut World, sim_id: u64, name: &str) -> Entity {
        let entity = world
            .spawn((
                SimEntity {
                    id: sim_id,
                    name: name.to_string(),
                    origin: Some(SimTime::from_year(100)),
                    end: None,
                },
                Settlement,
                SettlementCore::default(),
            ))
            .id();
        world.resource_mut::<SimEntityMap>().insert(sim_id, entity);
        entity
    }

    fn write_command(world: &mut World, cmd: SimCommand) {
        world.resource_mut::<Messages<SimCommand>>().write(cmd);
    }

    fn tick(app: &mut bevy_app::App) {
        app.world_mut().run_schedule(SimTick);
    }

    #[test]
    fn end_entity_marks_ended() {
        let mut app = build_sim_app(100);
        let person = spawn_test_person(app.world_mut(), 1, "Aldric");

        let cmd = SimCommand::new(
            SimCommandKind::EndEntity { entity: person },
            EventKind::Death,
            "Aldric died",
        )
        .with_participant(person, ParticipantRole::Subject);

        write_command(app.world_mut(), cmd);
        tick(&mut app);

        // Verify entity is ended
        let sim_entity = app.world().get::<SimEntity>(person).unwrap();
        assert!(sim_entity.end.is_some());

        // Verify EventLog has the event
        let log = app.world().resource::<EventLog>();
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].kind, EventKind::Death);
        assert_eq!(log.events[0].description, "Aldric died");

        // Verify EntityEnded effect
        let has_ended_effect = log
            .effects
            .iter()
            .any(|e| e.entity_id == 1 && matches!(e.effect, StateChange::EntityEnded));
        assert!(has_ended_effect, "expected EntityEnded effect");

        // Verify reactive event emitted
        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(!reactive.is_empty());
    }

    #[test]
    fn duplicate_end_entity_is_noop() {
        let mut app = build_sim_app(100);
        let person = spawn_test_person(app.world_mut(), 1, "Aldric");

        // First end
        let cmd = SimCommand::new(
            SimCommandKind::EndEntity { entity: person },
            EventKind::Death,
            "Aldric died",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        assert_eq!(app.world().resource::<EventLog>().events.len(), 1);

        // Second end (idempotent — no new effect)
        let cmd2 = SimCommand::new(
            SimCommandKind::EndEntity { entity: person },
            EventKind::Death,
            "Aldric died again",
        );
        write_command(app.world_mut(), cmd2);
        tick(&mut app);

        // 2 events recorded (both commands), but only 1 EntityEnded effect
        let log = app.world().resource::<EventLog>();
        let ended_effects: Vec<_> = log
            .effects
            .iter()
            .filter(|e| matches!(e.effect, StateChange::EntityEnded))
            .collect();
        assert_eq!(ended_effects.len(), 1);
    }

    #[test]
    fn rename_entity() {
        let mut app = build_sim_app(100);
        let settlement = spawn_test_settlement(app.world_mut(), 1, "Ironhold");

        let cmd = SimCommand::new(
            SimCommandKind::RenameEntity {
                entity: settlement,
                new_name: "Ironhaven".to_string(),
            },
            EventKind::Renamed,
            "Ironhold renamed to Ironhaven",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let sim_entity = app.world().get::<SimEntity>(settlement).unwrap();
        assert_eq!(sim_entity.name, "Ironhaven");

        let log = app.world().resource::<EventLog>();
        let has_name_effect = log.effects.iter().any(|e| {
            matches!(
                &e.effect,
                StateChange::NameChanged { old, new } if old == "Ironhold" && new == "Ironhaven"
            )
        });
        assert!(has_name_effect, "expected NameChanged effect");
    }

    #[test]
    fn declare_war_creates_relationship() {
        let mut app = build_sim_app(100);
        let faction_a = spawn_test_faction(app.world_mut(), 1, "Kingdom A");
        let faction_b = spawn_test_faction(app.world_mut(), 2, "Kingdom B");

        let cmd = SimCommand::new(
            SimCommandKind::DeclareWar {
                attacker: faction_a,
                defender: faction_b,
            },
            EventKind::WarDeclared,
            "Kingdom A declares war on Kingdom B",
        )
        .with_participant(faction_a, ParticipantRole::Attacker)
        .with_participant(faction_b, ParticipantRole::Defender);

        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let rel_graph = app.world().resource::<RelationshipGraph>();
        assert!(rel_graph.are_at_war(faction_a, faction_b));

        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(!reactive.is_empty());
    }

    #[test]
    fn add_structural_relationship() {
        let mut app = build_sim_app(100);
        let person = spawn_test_person(app.world_mut(), 1, "Aldric");
        let faction = spawn_test_faction(app.world_mut(), 2, "Kingdom");

        let cmd = SimCommand::new(
            SimCommandKind::AddRelationship {
                source: person,
                target: faction,
                kind: RelationshipKind::MemberOf,
            },
            EventKind::Joined,
            "Aldric joins the Kingdom",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let member_of = app.world().get::<MemberOf>(person);
        assert!(member_of.is_some());
        assert_eq!(member_of.unwrap().0, faction);
    }

    #[test]
    fn set_field_bookkeeping() {
        let mut app = build_sim_app(100);
        let person = spawn_test_person(app.world_mut(), 1, "Aldric");

        let cmd = SimCommand::bookkeeping(SimCommandKind::SetField {
            entity: person,
            field: "prestige".to_string(),
            old_value: serde_json::json!(10.0),
            new_value: serde_json::json!(15.0),
        });
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        // No event in the log
        let log = app.world().resource::<EventLog>();
        assert!(log.events.is_empty());

        // But there is a PropertyChanged effect
        let has_prop_effect = log.effects.iter().any(|e| {
            matches!(&e.effect, StateChange::PropertyChanged { field, .. } if field == "prestige")
        });
        assert!(has_prop_effect, "expected PropertyChanged effect");
    }

    #[test]
    fn person_died_ends_relationships() {
        let mut app = build_sim_app(100);
        let faction = spawn_test_faction(app.world_mut(), 1, "Kingdom");
        let person = spawn_test_person(app.world_mut(), 2, "Aldric");

        // Set up person as leader and member of faction
        app.world_mut()
            .entity_mut(person)
            .insert((MemberOf(faction), LeaderOf(faction)));

        let cmd = SimCommand::new(
            SimCommandKind::PersonDied { person },
            EventKind::Death,
            "Aldric died",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        // Verify ended
        let sim_entity = app.world().get::<SimEntity>(person).unwrap();
        assert!(sim_entity.end.is_some());

        // Verify MemberOf removed
        assert!(app.world().get::<MemberOf>(person).is_none());

        // Verify LeaderOf removed
        assert!(app.world().get::<LeaderOf>(person).is_none());

        // Verify LeaderVacancy reactive event
        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(!reactive.is_empty());
    }

    #[test]
    fn capture_settlement_changes_faction() {
        let mut app = build_sim_app(100);
        let old_faction = spawn_test_faction(app.world_mut(), 1, "Kingdom A");
        let new_faction = spawn_test_faction(app.world_mut(), 2, "Kingdom B");
        let settlement = spawn_test_settlement(app.world_mut(), 3, "Ironhold");

        app.world_mut()
            .entity_mut(settlement)
            .insert(MemberOf(old_faction));

        let cmd = SimCommand::new(
            SimCommandKind::CaptureSettlement {
                settlement,
                new_faction,
            },
            EventKind::Conquest,
            "Ironhold captured by Kingdom B",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let member_of = app.world().get::<MemberOf>(settlement).unwrap();
        assert_eq!(member_of.0, new_faction);

        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(!reactive.is_empty());
    }

    #[test]
    fn causal_chain_preserved() {
        let mut app = build_sim_app(100);
        let faction_a = spawn_test_faction(app.world_mut(), 1, "Kingdom A");
        let faction_b = spawn_test_faction(app.world_mut(), 2, "Kingdom B");

        // First: declare war
        let cmd = SimCommand::new(
            SimCommandKind::DeclareWar {
                attacker: faction_a,
                defender: faction_b,
            },
            EventKind::WarDeclared,
            "War declared",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        let war_event_id = app.world().resource::<EventLog>().events[0].id;

        // Second: bookkeeping caused by the war
        let cmd2 = SimCommand::bookkeeping(SimCommandKind::SetField {
            entity: faction_a,
            field: "at_war".to_string(),
            old_value: serde_json::json!(false),
            new_value: serde_json::json!(true),
        })
        .caused_by(war_event_id);
        write_command(app.world_mut(), cmd2);
        tick(&mut app);

        let log = app.world().resource::<EventLog>();
        // Only the war event (bookkeeping skips event creation)
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].id, war_event_id);
    }

    #[test]
    fn messages_cleared_between_ticks() {
        let mut app = build_sim_app(100);
        let faction_a = spawn_test_faction(app.world_mut(), 1, "Kingdom A");
        let faction_b = spawn_test_faction(app.world_mut(), 2, "Kingdom B");

        // Tick 1: emit a command
        let cmd = SimCommand::new(
            SimCommandKind::DeclareWar {
                attacker: faction_a,
                defender: faction_b,
            },
            EventKind::WarDeclared,
            "War declared",
        );
        write_command(app.world_mut(), cmd);
        tick(&mut app);

        assert!(
            !app.world()
                .resource::<Messages<SimReactiveEvent>>()
                .is_empty()
        );

        // Tick 2: no commands — message_update_system rotates buffers
        tick(&mut app);

        // Tick 3: old messages fully cleared from double-buffer
        tick(&mut app);

        let reactive = app.world().resource::<Messages<SimReactiveEvent>>();
        assert!(
            reactive.is_empty(),
            "stale reactive events should be cleared"
        );
    }
}
