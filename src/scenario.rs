use crate::model::entity_data::*;
use crate::model::*;
use crate::model::population::PopulationBreakdown;
use crate::sim::{SimConfig, SimSystem, run};

/// IDs returned by [`Scenario::add_settlement_standalone`].
pub struct SettlementSetup {
    pub settlement: u64,
    pub faction: u64,
    pub region: u64,
}

/// IDs returned by [`Scenario::add_kingdom`] / [`Scenario::add_kingdom_with`].
pub struct KingdomIds {
    pub faction: u64,
    pub region: u64,
    pub settlement: u64,
    pub leader: u64,
}

/// IDs returned by [`Scenario::add_war_between`].
pub struct WarIds {
    pub attacker: KingdomIds,
    pub defender: KingdomIds,
    pub army: u64,
}

// -- Builder-style ref types --

/// Typed reference to a faction entity in a [`Scenario`], enabling chained field mutation.
///
/// Created by [`Scenario::faction`] (creation) or [`Scenario::faction_mut`] (mutation).
/// Call [`.id()`](FactionRef::id) to terminate the chain and extract the entity ID.
pub struct FactionRef<'a> {
    scenario: &'a mut Scenario,
    id: u64,
}

impl<'a> FactionRef<'a> {
    fn data_mut(&mut self) -> &mut FactionData {
        self.scenario.world.entities.get_mut(&self.id).unwrap()
            .data.as_faction_mut().unwrap()
    }

    pub fn government_type(mut self, v: &str) -> Self { self.data_mut().government_type = v.to_string(); self }
    pub fn stability(mut self, v: f64) -> Self { self.data_mut().stability = v; self }
    pub fn happiness(mut self, v: f64) -> Self { self.data_mut().happiness = v; self }
    pub fn legitimacy(mut self, v: f64) -> Self { self.data_mut().legitimacy = v; self }
    pub fn treasury(mut self, v: f64) -> Self { self.data_mut().treasury = v; self }
    pub fn alliance_strength(mut self, v: f64) -> Self { self.data_mut().alliance_strength = v; self }
    pub fn primary_culture(mut self, v: Option<u64>) -> Self { self.data_mut().primary_culture = v; self }
    pub fn prestige(mut self, v: f64) -> Self { self.data_mut().prestige = v; self }

    /// Escape hatch: apply an arbitrary closure to the faction data.
    pub fn with(mut self, f: impl FnOnce(&mut FactionData)) -> Self { f(self.data_mut()); self }

    /// Terminate the chain and return the entity ID.
    pub fn id(self) -> u64 { self.id }
}

/// Typed reference to a settlement entity in a [`Scenario`], enabling chained field mutation.
///
/// Created by [`Scenario::settlement`] (creation) or [`Scenario::settlement_mut`] (mutation).
/// Call [`.id()`](SettlementRef::id) to terminate the chain and extract the entity ID.
pub struct SettlementRef<'a> {
    scenario: &'a mut Scenario,
    id: u64,
}

impl<'a> SettlementRef<'a> {
    fn data_mut(&mut self) -> &mut SettlementData {
        self.scenario.world.entities.get_mut(&self.id).unwrap()
            .data.as_settlement_mut().unwrap()
    }

    pub fn population(mut self, v: u32) -> Self {
        let d = self.data_mut();
        d.population = v;
        d.population_breakdown = PopulationBreakdown::from_total(v);
        self
    }
    pub fn prosperity(mut self, v: f64) -> Self { self.data_mut().prosperity = v; self }
    pub fn treasury(mut self, v: f64) -> Self { self.data_mut().treasury = v; self }
    pub fn fortification_level(mut self, v: u8) -> Self { self.data_mut().fortification_level = v; self }
    pub fn resources(mut self, v: Vec<String>) -> Self { self.data_mut().resources = v; self }
    pub fn prestige(mut self, v: f64) -> Self { self.data_mut().prestige = v; self }
    pub fn plague_immunity(mut self, v: f64) -> Self { self.data_mut().plague_immunity = v; self }
    pub fn cultural_tension(mut self, v: f64) -> Self { self.data_mut().cultural_tension = v; self }
    pub fn dominant_culture(mut self, v: Option<u64>) -> Self { self.data_mut().dominant_culture = v; self }
    pub fn culture_makeup(mut self, v: std::collections::BTreeMap<u64, f64>) -> Self { self.data_mut().culture_makeup = v; self }

    /// Escape hatch: apply an arbitrary closure to the settlement data.
    pub fn with(mut self, f: impl FnOnce(&mut SettlementData)) -> Self { f(self.data_mut()); self }

    /// Terminate the chain and return the entity ID.
    pub fn id(self) -> u64 { self.id }
}

/// Typed reference to a person entity in a [`Scenario`], enabling chained field mutation.
///
/// Created by [`Scenario::person`] (creation) or [`Scenario::person_mut`] (mutation).
/// Call [`.id()`](PersonRef::id) to terminate the chain and extract the entity ID.
pub struct PersonRef<'a> {
    scenario: &'a mut Scenario,
    id: u64,
}

impl<'a> PersonRef<'a> {
    fn data_mut(&mut self) -> &mut PersonData {
        self.scenario.world.entities.get_mut(&self.id).unwrap()
            .data.as_person_mut().unwrap()
    }

    pub fn birth_year(mut self, v: u32) -> Self { self.data_mut().birth_year = v; self }
    pub fn sex(mut self, v: &str) -> Self { self.data_mut().sex = v.to_string(); self }
    pub fn role(mut self, v: &str) -> Self { self.data_mut().role = v.to_string(); self }
    pub fn traits(mut self, v: Vec<Trait>) -> Self { self.data_mut().traits = v; self }
    pub fn add_trait(mut self, t: Trait) -> Self { self.data_mut().traits.push(t); self }
    pub fn culture_id(mut self, v: Option<u64>) -> Self { self.data_mut().culture_id = v; self }
    pub fn prestige(mut self, v: f64) -> Self { self.data_mut().prestige = v; self }
    pub fn last_action_year(mut self, v: u32) -> Self { self.data_mut().last_action_year = v; self }

    /// Escape hatch: apply an arbitrary closure to the person data.
    pub fn with(mut self, f: impl FnOnce(&mut PersonData)) -> Self { f(self.data_mut()); self }

    /// Terminate the chain and return the entity ID.
    pub fn id(self) -> u64 { self.id }
}

/// Typed reference to an army entity in a [`Scenario`], enabling chained field mutation.
///
/// Created by [`Scenario::army`] (creation) or [`Scenario::army_mut`] (mutation).
/// Call [`.id()`](ArmyRef::id) to terminate the chain and extract the entity ID.
pub struct ArmyRef<'a> {
    scenario: &'a mut Scenario,
    id: u64,
}

impl<'a> ArmyRef<'a> {
    fn data_mut(&mut self) -> &mut ArmyData {
        self.scenario.world.entities.get_mut(&self.id).unwrap()
            .data.as_army_mut().unwrap()
    }

    pub fn morale(mut self, v: f64) -> Self { self.data_mut().morale = v; self }
    pub fn supply(mut self, v: f64) -> Self { self.data_mut().supply = v; self }
    pub fn strength(mut self, v: u32) -> Self { self.data_mut().strength = v; self }

    /// Escape hatch: apply an arbitrary closure to the army data.
    pub fn with(mut self, f: impl FnOnce(&mut ArmyData)) -> Self { f(self.data_mut()); self }

    /// Terminate the chain and return the entity ID.
    pub fn id(self) -> u64 { self.id }
}

/// Fluent builder for constructing World state.
///
/// Handles event creation automatically and uses `EntityData::default_for_kind()`
/// plus closure-based field mutation, so adding new struct fields never breaks callers.
///
/// Used by tests for deterministic scenario setup, and will serve as the foundation
/// for future template/premade-map starting points.
pub struct Scenario {
    world: World,
    setup_event: u64,
    start_year: u32,
}

impl Default for Scenario {
    fn default() -> Self {
        Self::new()
    }
}

impl Scenario {
    /// Create a new scenario starting at year 1.
    pub fn new() -> Self {
        Self::at_year(1)
    }

    /// Create a new scenario starting at the given year.
    pub fn at_year(year: u32) -> Self {
        let mut world = World::new();
        world.current_time = SimTimestamp::from_year(year);
        let setup_event = world.add_event(
            EventKind::Custom("test_setup".to_string()),
            SimTimestamp::from_year(year),
            "Scenario setup".to_string(),
        );
        Self {
            world,
            setup_event,
            start_year: year,
        }
    }

    // -- Entity creation --

    /// Add a region with default terrain ("plains").
    pub fn add_region(&mut self, name: &str) -> u64 {
        self.add_region_with(name, |_| {})
    }

    /// Add a region, customizing its data via closure.
    pub fn add_region_with(&mut self, name: &str, modify: impl FnOnce(&mut RegionData)) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Region);
        if let EntityData::Region(ref mut rd) = data {
            rd.terrain = "plains".to_string();
            modify(rd);
        }
        self.world.add_entity(
            EntityKind::Region,
            name.to_string(),
            None,
            data,
            self.setup_event,
        )
    }

    /// Add a faction with sensible defaults (treasury=100, stability/happiness/legitimacy=0.5).
    pub fn add_faction(&mut self, name: &str) -> u64 {
        self.add_faction_with(name, |_| {})
    }

    /// Add a faction, customizing its data via closure.
    pub fn add_faction_with(&mut self, name: &str, modify: impl FnOnce(&mut FactionData)) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Faction);
        if let EntityData::Faction(ref mut fd) = data {
            fd.treasury = 100.0;
            modify(fd);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.add_entity(
            EntityKind::Faction,
            name.to_string(),
            Some(ts),
            data,
            self.setup_event,
        )
    }

    /// Add a settlement with default pop=200, auto-creating MemberOf→faction and LocatedIn→region.
    /// Also sets the `capacity` extra to 2×population.
    pub fn add_settlement(&mut self, name: &str, faction: u64, region: u64) -> u64 {
        self.add_settlement_with(name, faction, region, |_| {})
    }

    /// Add a settlement, customizing its data via closure.
    /// Auto-creates MemberOf→faction and LocatedIn→region relationships.
    /// Syncs PopulationBreakdown if population was changed in the closure.
    pub fn add_settlement_with(
        &mut self,
        name: &str,
        faction: u64,
        region: u64,
        modify: impl FnOnce(&mut SettlementData),
    ) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Settlement);
        if let EntityData::Settlement(ref mut sd) = data {
            sd.population = 200;
            sd.population_breakdown = PopulationBreakdown::from_total(200);
            sd.prosperity = 0.5;
            modify(sd);
            // Re-sync breakdown if population was changed
            if sd.population != sd.population_breakdown.total() {
                sd.population_breakdown = PopulationBreakdown::from_total(sd.population);
            }
        }
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        let id =
            self.world
                .add_entity(EntityKind::Settlement, name.to_string(), Some(ts), data, ev);
        self.world
            .add_relationship(id, faction, RelationshipKind::MemberOf, ts, ev);
        self.world
            .add_relationship(id, region, RelationshipKind::LocatedIn, ts, ev);

        // Set capacity extra (used by demographics/economy)
        let pop = self.world.entities[&id]
            .data
            .as_settlement()
            .unwrap()
            .population;
        self.world
            .set_extra(id, "capacity".to_string(), serde_json::json!(pop * 2), ev);
        id
    }

    /// Add a person with default birth_year and sex, auto-creating MemberOf→faction.
    pub fn add_person(&mut self, name: &str, faction: u64) -> u64 {
        self.add_person_with(name, faction, |_| {})
    }

    /// Add a person, customizing its data via closure. Auto-creates MemberOf→faction.
    pub fn add_person_with(
        &mut self,
        name: &str,
        faction: u64,
        modify: impl FnOnce(&mut PersonData),
    ) -> u64 {
        let id = self.add_person_standalone_with(name, modify);
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.add_relationship(
            id,
            faction,
            RelationshipKind::MemberOf,
            ts,
            self.setup_event,
        );
        id
    }

    /// Add a person with MemberOf→faction AND LocatedIn→settlement.
    pub fn add_person_in(&mut self, name: &str, faction: u64, settlement: u64) -> u64 {
        self.add_person_in_with(name, faction, settlement, |_| {})
    }

    /// Add a person with MemberOf→faction AND LocatedIn→settlement, customizing via closure.
    pub fn add_person_in_with(
        &mut self,
        name: &str,
        faction: u64,
        settlement: u64,
        modify: impl FnOnce(&mut PersonData),
    ) -> u64 {
        let id = self.add_person_with(name, faction, modify);
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.add_relationship(
            id,
            settlement,
            RelationshipKind::LocatedIn,
            ts,
            self.setup_event,
        );
        id
    }

    /// Add a person without any faction membership.
    pub fn add_person_standalone(&mut self, name: &str) -> u64 {
        self.add_person_standalone_with(name, |_| {})
    }

    /// Add a person without faction membership, customizing via closure.
    pub fn add_person_standalone_with(
        &mut self,
        name: &str,
        modify: impl FnOnce(&mut PersonData),
    ) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Person);
        if let EntityData::Person(ref mut pd) = data {
            pd.birth_year = self.start_year.saturating_sub(30);
            pd.sex = "male".to_string();
            modify(pd);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.add_entity(
            EntityKind::Person,
            name.to_string(),
            Some(ts),
            data,
            self.setup_event,
        )
    }

    /// Add an army with given strength, auto-creating MemberOf→faction, LocatedIn→region,
    /// and setting faction_id/home_region_id/starting_strength extras.
    pub fn add_army(&mut self, name: &str, faction: u64, region: u64, strength: u32) -> u64 {
        self.add_army_with(name, faction, region, strength, |_| {})
    }

    /// Add an army, customizing its data via closure.
    pub fn add_army_with(
        &mut self,
        name: &str,
        faction: u64,
        region: u64,
        strength: u32,
        modify: impl FnOnce(&mut ArmyData),
    ) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Army);
        if let EntityData::Army(ref mut ad) = data {
            ad.strength = strength;
            ad.morale = 1.0;
            ad.supply = 3.0;
            modify(ad);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        let id = self
            .world
            .add_entity(EntityKind::Army, name.to_string(), Some(ts), data, ev);
        self.world
            .add_relationship(id, faction, RelationshipKind::MemberOf, ts, ev);
        self.world
            .add_relationship(id, region, RelationshipKind::LocatedIn, ts, ev);
        self.world
            .set_extra(id, "faction_id".to_string(), serde_json::json!(faction), ev);
        self.world.set_extra(
            id,
            "home_region_id".to_string(),
            serde_json::json!(region),
            ev,
        );
        self.world.set_extra(
            id,
            "starting_strength".to_string(),
            serde_json::json!(strength),
            ev,
        );
        id
    }

    /// Add a building, auto-creating LocatedIn→settlement.
    pub fn add_building(&mut self, building_type: BuildingType, settlement: u64) -> u64 {
        self.add_building_with(building_type, settlement, |_| {})
    }

    /// Add a building, customizing its data via closure. Auto-creates LocatedIn→settlement.
    pub fn add_building_with(
        &mut self,
        building_type: BuildingType,
        settlement: u64,
        modify: impl FnOnce(&mut BuildingData),
    ) -> u64 {
        let bt_name = format!("{building_type}");
        let mut data = EntityData::default_for_kind(&EntityKind::Building);
        if let EntityData::Building(ref mut bd) = data {
            bd.building_type = building_type;
            bd.condition = 1.0;
            modify(bd);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        let id = self
            .world
            .add_entity(EntityKind::Building, bt_name, Some(ts), data, ev);
        self.world
            .add_relationship(id, settlement, RelationshipKind::LocatedIn, ts, ev);
        id
    }

    /// Add a culture with default data.
    pub fn add_culture(&mut self, name: &str) -> u64 {
        self.add_culture_with(name, |_| {})
    }

    /// Add a culture, customizing its data via closure.
    pub fn add_culture_with(&mut self, name: &str, modify: impl FnOnce(&mut CultureData)) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Culture);
        if let EntityData::Culture(ref mut cd) = data {
            modify(cd);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.add_entity(
            EntityKind::Culture,
            name.to_string(),
            Some(ts),
            data,
            self.setup_event,
        )
    }

    /// Add a disease entity with default data.
    pub fn add_disease(&mut self, name: &str) -> u64 {
        self.add_disease_with(name, |_| {})
    }

    /// Add a disease entity, customizing its data via closure.
    pub fn add_disease_with(&mut self, name: &str, modify: impl FnOnce(&mut DiseaseData)) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Disease);
        if let EntityData::Disease(ref mut dd) = data {
            modify(dd);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.add_entity(
            EntityKind::Disease,
            name.to_string(),
            Some(ts),
            data,
            self.setup_event,
        )
    }

    /// Add a knowledge entity with default data.
    pub fn add_knowledge(
        &mut self,
        name: &str,
        category: KnowledgeCategory,
        origin_settlement: u64,
    ) -> u64 {
        self.add_knowledge_with(name, category, origin_settlement, |_| {})
    }

    /// Add a knowledge entity, customizing its data via closure.
    pub fn add_knowledge_with(
        &mut self,
        name: &str,
        category: KnowledgeCategory,
        origin_settlement: u64,
        modify: impl FnOnce(&mut KnowledgeData),
    ) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Knowledge);
        if let EntityData::Knowledge(ref mut kd) = data {
            kd.category = category;
            kd.origin_settlement_id = origin_settlement;
            kd.origin_year = self.start_year;
            kd.source_event_id = self.setup_event;
            modify(kd);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.add_entity(
            EntityKind::Knowledge,
            name.to_string(),
            Some(ts),
            data,
            self.setup_event,
        )
    }

    /// Add a geographic feature with default data, auto-creating LocatedIn→region.
    pub fn add_geographic_feature(&mut self, name: &str, feature_type: &str, region: u64) -> u64 {
        self.add_geographic_feature_with(name, feature_type, region, |_| {})
    }

    /// Add a geographic feature, customizing its data via closure.
    /// Auto-creates LocatedIn→region.
    pub fn add_geographic_feature_with(
        &mut self,
        name: &str,
        feature_type: &str,
        region: u64,
        modify: impl FnOnce(&mut GeographicFeatureData),
    ) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::GeographicFeature);
        if let EntityData::GeographicFeature(ref mut gf) = data {
            gf.feature_type = feature_type.to_string();
            modify(gf);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        let id = self.world.add_entity(
            EntityKind::GeographicFeature,
            name.to_string(),
            Some(ts),
            data,
            ev,
        );
        self.world
            .add_relationship(id, region, RelationshipKind::LocatedIn, ts, ev);
        id
    }

    /// Add a river, auto-creating FlowsThrough for each region in the path.
    pub fn add_river(&mut self, name: &str, region_path: &[u64]) -> u64 {
        self.add_river_with(name, region_path, |_| {})
    }

    /// Add a river, customizing its data via closure.
    /// Auto-creates FlowsThrough for each region in the path.
    pub fn add_river_with(
        &mut self,
        name: &str,
        region_path: &[u64],
        modify: impl FnOnce(&mut RiverData),
    ) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::River);
        if let EntityData::River(ref mut rd) = data {
            rd.region_path = region_path.to_vec();
            rd.length = region_path.len();
            modify(rd);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        let id = self
            .world
            .add_entity(EntityKind::River, name.to_string(), Some(ts), data, ev);
        for &region in region_path {
            self.world
                .add_relationship(id, region, RelationshipKind::FlowsThrough, ts, ev);
        }
        id
    }

    /// Add a resource deposit, auto-creating LocatedIn→region.
    /// Defaults: quantity=100, quality=0.5, discovered=true.
    pub fn add_resource_deposit(&mut self, name: &str, resource_type: &str, region: u64) -> u64 {
        self.add_resource_deposit_with(name, resource_type, region, |_| {})
    }

    /// Add a resource deposit, customizing its data via closure.
    /// Auto-creates LocatedIn→region.
    pub fn add_resource_deposit_with(
        &mut self,
        name: &str,
        resource_type: &str,
        region: u64,
        modify: impl FnOnce(&mut ResourceDepositData),
    ) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::ResourceDeposit);
        if let EntityData::ResourceDeposit(ref mut rd) = data {
            rd.resource_type = resource_type.to_string();
            rd.quantity = 100;
            rd.quality = 0.5;
            rd.discovered = true;
            modify(rd);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        let id = self.world.add_entity(
            EntityKind::ResourceDeposit,
            name.to_string(),
            Some(ts),
            data,
            ev,
        );
        self.world
            .add_relationship(id, region, RelationshipKind::LocatedIn, ts, ev);
        id
    }

    /// Add a manifestation of knowledge, auto-creating HeldBy→holder.
    /// Defaults: accuracy=1.0, completeness=1.0, condition=1.0,
    /// derivation_method="witnessed", created_year=start_year.
    pub fn add_manifestation(
        &mut self,
        name: &str,
        knowledge: u64,
        medium: Medium,
        holder: u64,
    ) -> u64 {
        self.add_manifestation_with(name, knowledge, medium, holder, |_| {})
    }

    /// Add a manifestation of knowledge, customizing its data via closure.
    /// Auto-creates HeldBy→holder.
    pub fn add_manifestation_with(
        &mut self,
        name: &str,
        knowledge: u64,
        medium: Medium,
        holder: u64,
        modify: impl FnOnce(&mut ManifestationData),
    ) -> u64 {
        let mut data = EntityData::default_for_kind(&EntityKind::Manifestation);
        if let EntityData::Manifestation(ref mut md) = data {
            md.knowledge_id = knowledge;
            md.medium = medium;
            md.accuracy = 1.0;
            md.completeness = 1.0;
            md.condition = 1.0;
            md.derivation_method = "witnessed".to_string();
            md.created_year = self.start_year;
            modify(md);
        }
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        let id = self.world.add_entity(
            EntityKind::Manifestation,
            name.to_string(),
            Some(ts),
            data,
            ev,
        );
        self.world
            .add_relationship(id, holder, RelationshipKind::HeldBy, ts, ev);
        id
    }

    /// Create N males + N females placed in a settlement with faction membership.
    /// Returns the IDs of all created people.
    pub fn add_population(
        &mut self,
        faction: u64,
        settlement: u64,
        count_per_sex: usize,
    ) -> Vec<u64> {
        let mut ids = Vec::with_capacity(count_per_sex * 2);
        for i in 0..count_per_sex {
            ids.push(
                self.add_person_in_with(&format!("Male_{i}"), faction, settlement, |pd| {
                    pd.sex = "male".to_string()
                }),
            );
        }
        for i in 0..count_per_sex {
            ids.push(
                self.add_person_in_with(&format!("Female_{i}"), faction, settlement, |pd| {
                    pd.sex = "female".to_string()
                }),
            );
        }
        ids
    }

    // -- Standalone settlement shorthand --

    /// Create a region + faction + settlement in one call with default data.
    /// Names derived from the given name (e.g. "Town" → "Town Region", "Town Faction").
    pub fn add_settlement_standalone(&mut self, name: &str) -> SettlementSetup {
        self.add_settlement_standalone_with(name, |_| {}, |_| {})
    }

    /// Create a region + faction + settlement in one call with customization closures.
    pub fn add_settlement_standalone_with(
        &mut self,
        name: &str,
        modify_faction: impl FnOnce(&mut FactionData),
        modify_settlement: impl FnOnce(&mut SettlementData),
    ) -> SettlementSetup {
        let region = self.add_region(&format!("{name} Region"));
        let faction = self.add_faction_with(&format!("{name} Faction"), modify_faction);
        let settlement = self.add_settlement_with(name, faction, region, modify_settlement);
        SettlementSetup {
            settlement,
            faction,
            region,
        }
    }

    /// Like `add_settlement_standalone` but makes the new region adjacent to `neighbor_region`.
    pub fn add_rival_settlement(&mut self, name: &str, neighbor_region: u64) -> SettlementSetup {
        self.add_rival_settlement_with(name, neighbor_region, |_| {}, |_| {})
    }

    /// Like `add_settlement_standalone_with` but makes the new region adjacent to `neighbor_region`.
    pub fn add_rival_settlement_with(
        &mut self,
        name: &str,
        neighbor_region: u64,
        modify_faction: impl FnOnce(&mut FactionData),
        modify_settlement: impl FnOnce(&mut SettlementData),
    ) -> SettlementSetup {
        let setup = self.add_settlement_standalone_with(name, modify_faction, modify_settlement);
        self.make_adjacent(setup.region, neighbor_region);
        setup
    }

    // -- Relationship helpers --

    /// Make a person the leader of a faction (LeaderOf relationship).
    pub fn make_leader(&mut self, person: u64, faction: u64) {
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.add_relationship(
            person,
            faction,
            RelationshipKind::LeaderOf,
            ts,
            self.setup_event,
        );
    }

    /// Create a bidirectional relationship pair.
    fn make_bidirectional(
        &mut self,
        a: u64,
        b: u64,
        kind_ab: RelationshipKind,
        kind_ba: RelationshipKind,
    ) {
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        self.world.add_relationship(a, b, kind_ab, ts, ev);
        self.world.add_relationship(b, a, kind_ba, ts, ev);
    }

    /// Make two regions adjacent (bidirectional AdjacentTo).
    pub fn make_adjacent(&mut self, region_a: u64, region_b: u64) {
        self.make_bidirectional(region_a, region_b, RelationshipKind::AdjacentTo, RelationshipKind::AdjacentTo);
    }

    /// Put two factions at war (bidirectional AtWar).
    pub fn make_at_war(&mut self, faction_a: u64, faction_b: u64) {
        self.make_bidirectional(faction_a, faction_b, RelationshipKind::AtWar, RelationshipKind::AtWar);
    }

    /// Make two factions allies (bidirectional Ally).
    pub fn make_allies(&mut self, faction_a: u64, faction_b: u64) {
        self.make_bidirectional(faction_a, faction_b, RelationshipKind::Ally, RelationshipKind::Ally);
    }

    /// Make a parent-child relationship (bidirectional Parent + Child).
    pub fn make_parent_child(&mut self, parent: u64, child: u64) {
        self.make_bidirectional(parent, child, RelationshipKind::Parent, RelationshipKind::Child);
    }

    /// Make two people spouses (bidirectional Spouse).
    pub fn make_spouse(&mut self, person_a: u64, person_b: u64) {
        self.make_bidirectional(person_a, person_b, RelationshipKind::Spouse, RelationshipKind::Spouse);
    }

    /// Make two factions enemies (bidirectional Enemy).
    pub fn make_enemies(&mut self, faction_a: u64, faction_b: u64) {
        self.make_bidirectional(faction_a, faction_b, RelationshipKind::Enemy, RelationshipKind::Enemy);
    }

    /// End an entity (mark as dead/dissolved).
    pub fn end_entity(&mut self, entity: u64) {
        let ts = SimTimestamp::from_year(self.start_year);
        self.world.end_entity(entity, ts, self.setup_event);
    }

    /// Add a single directed relationship between two entities.
    pub fn add_relationship(&mut self, source: u64, target: u64, kind: RelationshipKind) {
        let ts = SimTimestamp::from_year(self.start_year);
        self.world
            .add_relationship(source, target, kind, ts, self.setup_event);
    }

    /// Create a trade route between two settlements (bidirectional TradeRoute).
    pub fn make_trade_route(&mut self, settlement_a: u64, settlement_b: u64) {
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        self.world.add_relationship(
            settlement_a,
            settlement_b,
            RelationshipKind::TradeRoute,
            ts,
            ev,
        );
        self.world.add_relationship(
            settlement_b,
            settlement_a,
            RelationshipKind::TradeRoute,
            ts,
            ev,
        );
    }

    /// Set an extra property on an entity.
    pub fn set_extra(&mut self, entity: u64, key: &str, value: serde_json::Value) {
        self.world
            .set_extra(entity, key.to_string(), value, self.setup_event);
    }

    // -- Entity mutation --

    /// Modify a settlement's data after creation.
    pub fn modify_settlement(&mut self, id: u64, modify: impl FnOnce(&mut SettlementData)) {
        let sd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_settlement_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a settlement"));
        modify(sd);
    }

    /// Modify a faction's data after creation.
    pub fn modify_faction(&mut self, id: u64, modify: impl FnOnce(&mut FactionData)) {
        let fd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_faction_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a faction"));
        modify(fd);
    }

    /// Modify a person's data after creation.
    pub fn modify_person(&mut self, id: u64, modify: impl FnOnce(&mut PersonData)) {
        let pd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_person_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a person"));
        modify(pd);
    }

    /// Modify an army's data after creation.
    pub fn modify_army(&mut self, id: u64, modify: impl FnOnce(&mut ArmyData)) {
        let ad = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_army_mut()
            .unwrap_or_else(|| panic!("entity {id} is not an army"));
        modify(ad);
    }

    /// Modify a building's data after creation.
    pub fn modify_building(&mut self, id: u64, modify: impl FnOnce(&mut BuildingData)) {
        let bd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_building_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a building"));
        modify(bd);
    }

    /// Modify a region's data after creation.
    pub fn modify_region(&mut self, id: u64, modify: impl FnOnce(&mut RegionData)) {
        let rd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_region_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a region"));
        modify(rd);
    }

    /// Modify a culture's data after creation.
    pub fn modify_culture(&mut self, id: u64, modify: impl FnOnce(&mut CultureData)) {
        let cd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_culture_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a culture"));
        modify(cd);
    }

    /// Modify a disease's data after creation.
    pub fn modify_disease(&mut self, id: u64, modify: impl FnOnce(&mut DiseaseData)) {
        let dd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_disease_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a disease"));
        modify(dd);
    }

    /// Modify a knowledge entity's data after creation.
    pub fn modify_knowledge(&mut self, id: u64, modify: impl FnOnce(&mut KnowledgeData)) {
        let kd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_knowledge_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a knowledge"));
        modify(kd);
    }

    /// Modify a geographic feature's data after creation.
    pub fn modify_geographic_feature(
        &mut self,
        id: u64,
        modify: impl FnOnce(&mut GeographicFeatureData),
    ) {
        let gf = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_geographic_feature_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a geographic feature"));
        modify(gf);
    }

    /// Modify a river's data after creation.
    pub fn modify_river(&mut self, id: u64, modify: impl FnOnce(&mut RiverData)) {
        let rd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_river_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a river"));
        modify(rd);
    }

    /// Modify a resource deposit's data after creation.
    pub fn modify_resource_deposit(
        &mut self,
        id: u64,
        modify: impl FnOnce(&mut ResourceDepositData),
    ) {
        let rd = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_resource_deposit_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a resource deposit"));
        modify(rd);
    }

    /// Modify a manifestation's data after creation.
    pub fn modify_manifestation(&mut self, id: u64, modify: impl FnOnce(&mut ManifestationData)) {
        let md = self
            .world
            .entities
            .get_mut(&id)
            .unwrap_or_else(|| panic!("entity {id} not found"))
            .data
            .as_manifestation_mut()
            .unwrap_or_else(|| panic!("entity {id} is not a manifestation"));
        modify(md);
    }

    // -- Complex state helpers --

    /// Start a siege on a settlement with default timing (started at scenario time).
    pub fn start_siege(&mut self, settlement: u64, army: u64, attacker_faction: u64) {
        self.start_siege_with(settlement, army, attacker_faction, |_| {});
    }

    /// Start a siege on a settlement, customizing the siege state via closure.
    pub fn start_siege_with(
        &mut self,
        settlement: u64,
        army: u64,
        attacker_faction: u64,
        modify: impl FnOnce(&mut ActiveSiege),
    ) {
        let mut siege = ActiveSiege {
            attacker_army_id: army,
            attacker_faction_id: attacker_faction,
            started_year: self.start_year,
            started_month: 1,
            months_elapsed: 0,
            civilian_deaths: 0,
        };
        modify(&mut siege);
        self.modify_settlement(settlement, |sd| {
            sd.active_siege = Some(siege);
        });
        self.set_extra(
            army,
            "besieging_settlement_id",
            serde_json::json!(settlement),
        );
    }

    /// Queue an action to be executed on the next tick.
    pub fn queue_action(&mut self, actor: u64, source: ActionSource, kind: ActionKind) {
        self.world.queue_action(Action {
            actor_id: actor,
            source,
            kind,
        });
    }

    /// Mark an entity as the player character.
    pub fn make_player(&mut self, entity: u64) {
        self.set_extra(entity, "is_player", serde_json::json!(true));
    }

    /// Add an active disaster to a settlement with default timing.
    pub fn add_active_disaster(
        &mut self,
        settlement: u64,
        disaster_type: DisasterType,
        severity: f64,
    ) {
        self.add_active_disaster_with(settlement, disaster_type, severity, |_| {});
    }

    /// Add an active disaster to a settlement, customizing via closure.
    pub fn add_active_disaster_with(
        &mut self,
        settlement: u64,
        disaster_type: DisasterType,
        severity: f64,
        modify: impl FnOnce(&mut ActiveDisaster),
    ) {
        let months = if disaster_type.is_persistent() { 6 } else { 0 };
        let mut disaster = ActiveDisaster {
            disaster_type,
            severity,
            started_year: self.start_year,
            started_month: 1,
            months_remaining: months,
            total_deaths: 0,
        };
        modify(&mut disaster);
        self.modify_settlement(settlement, |sd| {
            sd.active_disaster = Some(disaster);
        });
    }

    /// Set an active disease on a settlement with default infection parameters.
    pub fn add_active_disease_on(&mut self, settlement: u64, disease: u64) {
        self.add_active_disease_on_with(settlement, disease, |_| {});
    }

    /// Set an active disease on a settlement, customizing via closure.
    pub fn add_active_disease_on_with(
        &mut self,
        settlement: u64,
        disease: u64,
        modify: impl FnOnce(&mut ActiveDisease),
    ) {
        let mut active = ActiveDisease {
            disease_id: disease,
            started_year: self.start_year,
            infection_rate: 0.3,
            peak_reached: false,
            total_deaths: 0,
        };
        modify(&mut active);
        self.modify_settlement(settlement, |sd| {
            sd.active_disease = Some(active);
        });
    }

    /// Add a tribute obligation from one faction to another.
    pub fn add_tribute(&mut self, payer: u64, payee: u64, amount: f64, years: u32) {
        self.set_extra(
            payer,
            &format!("tribute_{payee}"),
            serde_json::json!({
                "amount": amount,
                "years_remaining": years,
                "treaty_event_id": self.setup_event,
            }),
        );
    }

    /// Set war exhaustion on a faction.
    pub fn set_war_exhaustion(&mut self, faction: u64, value: f64) {
        self.set_extra(faction, "war_exhaustion", serde_json::json!(value));
    }

    // -- Composite builders --

    /// Create a kingdom: region + faction + settlement + leader with LeaderOf.
    /// Names derived from the given name (e.g. "Empire" → "Empire Region", "Empire Capital", "Empire Leader").
    pub fn add_kingdom(&mut self, name: &str) -> KingdomIds {
        self.add_kingdom_with(name, |_| {}, |_| {}, |_| {})
    }

    /// Create a kingdom with closures to customize faction, settlement, and leader data.
    pub fn add_kingdom_with(
        &mut self,
        name: &str,
        modify_faction: impl FnOnce(&mut FactionData),
        modify_settlement: impl FnOnce(&mut SettlementData),
        modify_leader: impl FnOnce(&mut PersonData),
    ) -> KingdomIds {
        let region = self.add_region(&format!("{name} Region"));
        let faction = self.add_faction_with(name, modify_faction);
        let settlement = self.add_settlement_with(
            &format!("{name} Capital"),
            faction,
            region,
            modify_settlement,
        );
        let leader = self.add_person_with(&format!("{name} Leader"), faction, |pd| {
            pd.role = "warrior".to_string();
            modify_leader(pd);
        });
        self.make_leader(leader, faction);
        KingdomIds {
            faction,
            region,
            settlement,
            leader,
        }
    }

    /// Create a rival kingdom adjacent to an existing region.
    pub fn add_rival_kingdom(&mut self, name: &str, neighbor_region: u64) -> KingdomIds {
        self.add_rival_kingdom_with(name, neighbor_region, |_| {}, |_| {}, |_| {})
    }

    /// Create a rival kingdom adjacent to an existing region, with customization closures.
    pub fn add_rival_kingdom_with(
        &mut self,
        name: &str,
        neighbor_region: u64,
        modify_faction: impl FnOnce(&mut FactionData),
        modify_settlement: impl FnOnce(&mut SettlementData),
        modify_leader: impl FnOnce(&mut PersonData),
    ) -> KingdomIds {
        let k = self.add_kingdom_with(name, modify_faction, modify_settlement, modify_leader);
        self.make_adjacent(k.region, neighbor_region);
        k
    }

    /// Create two kingdoms at war with an army. The attacker's army is placed in the defender's region.
    pub fn add_war_between(
        &mut self,
        attacker_name: &str,
        defender_name: &str,
        army_strength: u32,
    ) -> WarIds {
        let attacker = self.add_kingdom(attacker_name);
        let defender = self.add_rival_kingdom(defender_name, attacker.region);
        self.make_at_war(attacker.faction, defender.faction);
        let army = self.add_army(
            &format!("{attacker_name} Army"),
            attacker.faction,
            defender.region,
            army_strength,
        );
        WarIds {
            attacker,
            defender,
            army,
        }
    }

    /// Add a player character in a faction.
    pub fn add_player_in(&mut self, name: &str, faction: u64) -> u64 {
        self.add_player_in_with(name, faction, |_| {})
    }

    /// Add a player character in a faction, customizing via closure.
    pub fn add_player_in_with(
        &mut self,
        name: &str,
        faction: u64,
        modify: impl FnOnce(&mut PersonData),
    ) -> u64 {
        let id = self.add_person_with(name, faction, modify);
        self.make_player(id);
        id
    }

    // -- Bulk operations --

    /// Create `count` people in a faction and settlement, with per-person customization.
    /// The closure receives the index (0..count) and a mutable reference to the person data.
    pub fn add_people(
        &mut self,
        faction: u64,
        settlement: u64,
        count: usize,
        modify: impl Fn(usize, &mut PersonData),
    ) -> Vec<u64> {
        let mut ids = Vec::with_capacity(count);
        for i in 0..count {
            ids.push(
                self.add_person_in_with(&format!("Person_{i}"), faction, settlement, |pd| {
                    modify(i, pd)
                }),
            );
        }
        ids
    }

    /// Modify all living settlements via closure.
    pub fn modify_all_settlements(&mut self, modify: impl Fn(&mut SettlementData)) {
        let ids: Vec<u64> = self
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Settlement && e.end.is_none())
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.modify_settlement(id, &modify);
        }
    }

    /// Modify all living factions via closure.
    pub fn modify_all_factions(&mut self, modify: impl Fn(&mut FactionData)) {
        let ids: Vec<u64> = self
            .world
            .entities
            .values()
            .filter(|e| e.kind == EntityKind::Faction && e.end.is_none())
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.modify_faction(id, &modify);
        }
    }

    /// Apply an active disease to multiple settlements at once.
    pub fn spread_disease(&mut self, disease: u64, settlements: &[u64]) {
        self.spread_disease_with(disease, settlements, |_| {});
    }

    /// Apply an active disease to multiple settlements, customizing each via closure.
    pub fn spread_disease_with(
        &mut self,
        disease: u64,
        settlements: &[u64],
        modify: impl Fn(&mut ActiveDisease),
    ) {
        for &s in settlements {
            let mut active = ActiveDisease {
                disease_id: disease,
                started_year: self.start_year,
                infection_rate: 0.3,
                peak_reached: false,
                total_deaths: 0,
            };
            modify(&mut active);
            self.modify_settlement(s, |sd| {
                sd.active_disease = Some(active);
            });
        }
    }

    /// Apply an active disaster to multiple settlements at once.
    pub fn spread_disaster(
        &mut self,
        settlements: &[u64],
        disaster_type: DisasterType,
        severity: f64,
    ) {
        for &s in settlements {
            self.add_active_disaster(s, disaster_type.clone(), severity);
        }
    }

    // -- Network topology helpers --

    /// Connect regions in a ring: A↔B↔C↔D↔A.
    pub fn connect_ring(&mut self, regions: &[u64]) {
        if regions.len() < 2 {
            return;
        }
        for window in regions.windows(2) {
            self.make_adjacent(window[0], window[1]);
        }
        if regions.len() > 2 {
            self.make_adjacent(*regions.last().unwrap(), regions[0]);
        }
    }

    /// Connect all regions to a central hub.
    pub fn connect_hub_and_spoke(&mut self, hub: u64, spokes: &[u64]) {
        for &spoke in spokes {
            self.make_adjacent(hub, spoke);
        }
    }

    /// Connect every region to every other region (complete graph).
    pub fn connect_all(&mut self, regions: &[u64]) {
        for i in 0..regions.len() {
            for j in (i + 1)..regions.len() {
                self.make_adjacent(regions[i], regions[j]);
            }
        }
    }

    /// Connect settlements in a trade ring: A↔B↔C↔D↔A.
    pub fn connect_trade_ring(&mut self, settlements: &[u64]) {
        if settlements.len() < 2 {
            return;
        }
        for window in settlements.windows(2) {
            self.make_trade_route(window[0], window[1]);
        }
        if settlements.len() > 2 {
            self.make_trade_route(*settlements.last().unwrap(), settlements[0]);
        }
    }

    /// Connect all settlements to a central trade hub.
    pub fn connect_trade_hub(&mut self, hub: u64, spokes: &[u64]) {
        for &spoke in spokes {
            self.make_trade_route(hub, spoke);
        }
    }

    // -- Pre-build query API --

    /// Look up an entity by ID without consuming the scenario.
    pub fn entity(&self, id: u64) -> Option<&Entity> {
        self.world.entities.get(&id)
    }

    /// Find an entity by name (first match). Returns the entity ID.
    pub fn find_by_name(&self, name: &str) -> Option<u64> {
        self.world
            .entities
            .values()
            .find(|e| e.name == name)
            .map(|e| e.id)
    }

    /// Count living entities of a given kind.
    pub fn count_living(&self, kind: &EntityKind) -> usize {
        self.world
            .entities
            .values()
            .filter(|e| e.kind == *kind && e.end.is_none())
            .count()
    }

    /// Get all living entity IDs of a given kind.
    pub fn living_ids(&self, kind: &EntityKind) -> Vec<u64> {
        self.world
            .entities
            .values()
            .filter(|e| e.kind == *kind && e.end.is_none())
            .map(|e| e.id)
            .collect()
    }

    // -- Builder-style creation --

    /// Create a faction and return a builder ref for chaining field mutations.
    /// Uses the same defaults as [`add_faction`](Scenario::add_faction) (treasury=100).
    pub fn faction(&mut self, name: &str) -> FactionRef<'_> {
        let id = self.add_faction(name);
        FactionRef { scenario: self, id }
    }

    /// Create a settlement and return a builder ref for chaining field mutations.
    /// Uses the same defaults as [`add_settlement`](Scenario::add_settlement) (pop=200, prosperity=0.5).
    pub fn settlement(&mut self, name: &str, faction: u64, region: u64) -> SettlementRef<'_> {
        let id = self.add_settlement(name, faction, region);
        SettlementRef { scenario: self, id }
    }

    /// Create a person with MemberOf→faction and return a builder ref.
    /// Uses the same defaults as [`add_person`](Scenario::add_person) (birth_year=start-30, sex="male").
    pub fn person(&mut self, name: &str, faction: u64) -> PersonRef<'_> {
        let id = self.add_person(name, faction);
        PersonRef { scenario: self, id }
    }

    /// Create a person with MemberOf→faction and LocatedIn→settlement, return a builder ref.
    pub fn person_in(&mut self, name: &str, faction: u64, settlement: u64) -> PersonRef<'_> {
        let id = self.add_person_in(name, faction, settlement);
        PersonRef { scenario: self, id }
    }

    /// Create a player character with MemberOf→faction and return a builder ref.
    /// Equivalent to [`add_player_in`](Scenario::add_player_in) but returns a builder.
    pub fn player_in(&mut self, name: &str, faction: u64) -> PersonRef<'_> {
        let id = self.add_player_in(name, faction);
        PersonRef { scenario: self, id }
    }

    /// Create an army and return a builder ref for chaining field mutations.
    /// Uses the same defaults as [`add_army`](Scenario::add_army) (morale=1.0, supply=3.0).
    pub fn army(&mut self, name: &str, faction: u64, region: u64, strength: u32) -> ArmyRef<'_> {
        let id = self.add_army(name, faction, region, strength);
        ArmyRef { scenario: self, id }
    }

    // -- Builder-style mutation --

    /// Return a builder ref for an existing faction entity.
    pub fn faction_mut(&mut self, id: u64) -> FactionRef<'_> {
        assert!(
            self.world.entities.get(&id)
                .and_then(|e| e.data.as_faction())
                .is_some(),
            "entity {id} is not a faction"
        );
        FactionRef { scenario: self, id }
    }

    /// Return a builder ref for an existing settlement entity.
    pub fn settlement_mut(&mut self, id: u64) -> SettlementRef<'_> {
        assert!(
            self.world.entities.get(&id)
                .and_then(|e| e.data.as_settlement())
                .is_some(),
            "entity {id} is not a settlement"
        );
        SettlementRef { scenario: self, id }
    }

    /// Return a builder ref for an existing person entity.
    pub fn person_mut(&mut self, id: u64) -> PersonRef<'_> {
        assert!(
            self.world.entities.get(&id)
                .and_then(|e| e.data.as_person())
                .is_some(),
            "entity {id} is not a person"
        );
        PersonRef { scenario: self, id }
    }

    /// Return a builder ref for an existing army entity.
    pub fn army_mut(&mut self, id: u64) -> ArmyRef<'_> {
        assert!(
            self.world.entities.get(&id)
                .and_then(|e| e.data.as_army())
                .is_some(),
            "entity {id} is not an army"
        );
        ArmyRef { scenario: self, id }
    }

    // -- Output --

    /// Consume the scenario and return the constructed World.
    pub fn build(self) -> World {
        self.world
    }

    /// Build the world and run the given systems. Uses the scenario's start year.
    pub fn run(self, systems: &mut [Box<dyn SimSystem>], num_years: u32, seed: u64) -> World {
        let start_year = self.start_year;
        let mut world = self.build();
        run(
            &mut world,
            systems,
            SimConfig::new(start_year, num_years, seed),
        );
        world
    }

    /// Borrow the world for inspection.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Borrow the world mutably for additional modifications.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }
}
