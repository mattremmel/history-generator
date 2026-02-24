use crate::model::entity_data::*;
use crate::model::*;
use crate::sim::population::PopulationBreakdown;

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

    /// Make two regions adjacent (bidirectional AdjacentTo).
    pub fn make_adjacent(&mut self, region_a: u64, region_b: u64) {
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        self.world
            .add_relationship(region_a, region_b, RelationshipKind::AdjacentTo, ts, ev);
        self.world
            .add_relationship(region_b, region_a, RelationshipKind::AdjacentTo, ts, ev);
    }

    /// Put two factions at war (bidirectional AtWar).
    pub fn make_at_war(&mut self, faction_a: u64, faction_b: u64) {
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        self.world
            .add_relationship(faction_a, faction_b, RelationshipKind::AtWar, ts, ev);
        self.world
            .add_relationship(faction_b, faction_a, RelationshipKind::AtWar, ts, ev);
    }

    /// Make two factions allies (bidirectional Ally).
    pub fn make_allies(&mut self, faction_a: u64, faction_b: u64) {
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        self.world
            .add_relationship(faction_a, faction_b, RelationshipKind::Ally, ts, ev);
        self.world
            .add_relationship(faction_b, faction_a, RelationshipKind::Ally, ts, ev);
    }

    /// Make a parent-child relationship (bidirectional Parent + Child).
    pub fn make_parent_child(&mut self, parent: u64, child: u64) {
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        self.world
            .add_relationship(parent, child, RelationshipKind::Parent, ts, ev);
        self.world
            .add_relationship(child, parent, RelationshipKind::Child, ts, ev);
    }

    /// Make two factions enemies (bidirectional Enemy).
    pub fn make_enemies(&mut self, faction_a: u64, faction_b: u64) {
        let ts = SimTimestamp::from_year(self.start_year);
        let ev = self.setup_event;
        self.world
            .add_relationship(faction_a, faction_b, RelationshipKind::Enemy, ts, ev);
        self.world
            .add_relationship(faction_b, faction_a, RelationshipKind::Enemy, ts, ev);
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

    // -- Output --

    /// Consume the scenario and return the constructed World.
    pub fn build(self) -> World {
        self.world
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
