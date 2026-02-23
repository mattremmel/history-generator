use serde::{Deserialize, Serialize};

use super::entity::EntityKind;
use super::traits::Trait;
use crate::sim::population::PopulationBreakdown;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonData {
    pub birth_year: u32,
    pub sex: String,
    pub role: String,
    pub traits: Vec<Trait>,
    #[serde(default)]
    pub last_action_year: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettlementData {
    pub population: u32,
    pub population_breakdown: PopulationBreakdown,
    pub x: f64,
    pub y: f64,
    pub resources: Vec<String>,
    #[serde(default)]
    pub prosperity: f64,
    #[serde(default)]
    pub treasury: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactionData {
    pub government_type: String,
    #[serde(default)]
    pub stability: f64,
    #[serde(default)]
    pub happiness: f64,
    #[serde(default)]
    pub legitimacy: f64,
    #[serde(default)]
    pub treasury: f64,
    #[serde(default)]
    pub alliance_strength: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegionData {
    pub terrain: String,
    #[serde(default)]
    pub terrain_tags: Vec<String>,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArmyData {
    #[serde(default)]
    pub morale: f64,
    #[serde(default)]
    pub supply: f64,
    #[serde(default)]
    pub strength: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeographicFeatureData {
    pub feature_type: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceDepositData {
    pub resource_type: String,
    pub quantity: u32,
    pub quality: f64,
    pub discovered: bool,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuildingData {
    pub building_type: String,
    #[serde(default)]
    pub output_resource: Option<String>,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiverData {
    pub region_path: Vec<u64>,
    pub length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityData {
    Person(PersonData),
    Settlement(SettlementData),
    Faction(FactionData),
    Region(RegionData),
    Army(ArmyData),
    GeographicFeature(GeographicFeatureData),
    ResourceDeposit(ResourceDepositData),
    Building(BuildingData),
    River(RiverData),
    None,
}

impl EntityData {
    pub fn default_for_kind(kind: &EntityKind) -> Self {
        match kind {
            EntityKind::Person => EntityData::Person(PersonData {
                birth_year: 0,
                sex: String::new(),
                role: "common".to_string(),
                traits: Vec::new(),
                last_action_year: 0,
            }),
            EntityKind::Settlement => EntityData::Settlement(SettlementData {
                population: 0,
                population_breakdown: PopulationBreakdown::empty(),
                x: 0.0,
                y: 0.0,
                resources: Vec::new(),
                prosperity: 0.5,
                treasury: 0.0,
            }),
            EntityKind::Faction => EntityData::Faction(FactionData {
                government_type: "chieftain".to_string(),
                stability: 0.5,
                happiness: 0.5,
                legitimacy: 0.5,
                treasury: 0.0,
                alliance_strength: 0.0,
            }),
            EntityKind::Region => EntityData::Region(RegionData {
                terrain: String::new(),
                terrain_tags: Vec::new(),
                x: 0.0,
                y: 0.0,
                resources: Vec::new(),
            }),
            EntityKind::Army => EntityData::Army(ArmyData {
                morale: 1.0,
                supply: 1.0,
                strength: 0,
            }),
            EntityKind::GeographicFeature => EntityData::GeographicFeature(GeographicFeatureData {
                feature_type: String::new(),
                x: 0.0,
                y: 0.0,
            }),
            EntityKind::ResourceDeposit => EntityData::ResourceDeposit(ResourceDepositData {
                resource_type: String::new(),
                quantity: 0,
                quality: 0.0,
                discovered: false,
                x: 0.0,
                y: 0.0,
            }),
            EntityKind::Building => EntityData::Building(BuildingData {
                building_type: String::new(),
                output_resource: None,
                x: 0.0,
                y: 0.0,
            }),
            EntityKind::River => EntityData::River(RiverData {
                region_path: Vec::new(),
                length: 0,
            }),
            _ => EntityData::None,
        }
    }

    pub fn as_person(&self) -> Option<&PersonData> {
        match self {
            EntityData::Person(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_person_mut(&mut self) -> Option<&mut PersonData> {
        match self {
            EntityData::Person(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_settlement(&self) -> Option<&SettlementData> {
        match self {
            EntityData::Settlement(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_settlement_mut(&mut self) -> Option<&mut SettlementData> {
        match self {
            EntityData::Settlement(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_faction(&self) -> Option<&FactionData> {
        match self {
            EntityData::Faction(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_faction_mut(&mut self) -> Option<&mut FactionData> {
        match self {
            EntityData::Faction(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_region(&self) -> Option<&RegionData> {
        match self {
            EntityData::Region(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_region_mut(&mut self) -> Option<&mut RegionData> {
        match self {
            EntityData::Region(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_army(&self) -> Option<&ArmyData> {
        match self {
            EntityData::Army(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_army_mut(&mut self) -> Option<&mut ArmyData> {
        match self {
            EntityData::Army(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_geographic_feature(&self) -> Option<&GeographicFeatureData> {
        match self {
            EntityData::GeographicFeature(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_geographic_feature_mut(&mut self) -> Option<&mut GeographicFeatureData> {
        match self {
            EntityData::GeographicFeature(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_resource_deposit(&self) -> Option<&ResourceDepositData> {
        match self {
            EntityData::ResourceDeposit(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_resource_deposit_mut(&mut self) -> Option<&mut ResourceDepositData> {
        match self {
            EntityData::ResourceDeposit(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_building(&self) -> Option<&BuildingData> {
        match self {
            EntityData::Building(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_building_mut(&mut self) -> Option<&mut BuildingData> {
        match self {
            EntityData::Building(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_river(&self) -> Option<&RiverData> {
        match self {
            EntityData::River(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_river_mut(&mut self) -> Option<&mut RiverData> {
        match self {
            EntityData::River(d) => Some(d),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_for_kind_person() {
        let data = EntityData::default_for_kind(&EntityKind::Person);
        assert!(data.as_person().is_some());
    }

    #[test]
    fn default_for_kind_settlement() {
        let data = EntityData::default_for_kind(&EntityKind::Settlement);
        let s = data.as_settlement().unwrap();
        assert_eq!(s.population, 0);
        assert!((s.prosperity - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn default_for_kind_unknown_returns_none() {
        let data = EntityData::default_for_kind(&EntityKind::Deity);
        assert!(matches!(data, EntityData::None));
    }

    #[test]
    fn accessor_mut_works() {
        let mut data = EntityData::default_for_kind(&EntityKind::Faction);
        data.as_faction_mut().unwrap().stability = 0.9;
        assert!((data.as_faction().unwrap().stability - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn serde_round_trip() {
        let data = EntityData::Person(PersonData {
            birth_year: 100,
            sex: "male".to_string(),
            role: "warrior".to_string(),
            traits: vec![Trait::Ambitious, Trait::Aggressive],
            last_action_year: 105,
        });
        let json = serde_json::to_string(&data).unwrap();
        let back: EntityData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }
}
