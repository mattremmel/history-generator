use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::cultural_value::{CulturalValue, NamingStyle};
use super::entity::EntityKind;
use super::traits::Trait;
use crate::sim::population::{NUM_BRACKETS, PopulationBreakdown};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonData {
    pub birth_year: u32,
    pub sex: String,
    pub role: String,
    pub traits: Vec<Trait>,
    #[serde(default)]
    pub last_action_year: u32,
    #[serde(default)]
    pub culture_id: Option<u64>,
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
    #[serde(default)]
    pub dominant_culture: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub culture_makeup: BTreeMap<u64, f64>,
    #[serde(default)]
    pub cultural_tension: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_disease: Option<ActiveDisease>,
    #[serde(default)]
    pub plague_immunity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveDisease {
    pub disease_id: u64,
    pub started_year: u32,
    pub infection_rate: f64,
    pub peak_reached: bool,
    /// Running total of deaths caused by this outbreak in this settlement.
    #[serde(default)]
    pub total_deaths: u32,
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
    #[serde(default)]
    pub primary_culture: Option<u64>,
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
pub struct CultureData {
    pub values: Vec<CulturalValue>,
    pub naming_style: NamingStyle,
    /// 0.0-1.0: higher means harder to assimilate.
    pub resistance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiseaseData {
    /// 0.0-1.0: how easily it spreads between settlements.
    pub virulence: f64,
    /// 0.0-1.0: base death rate among infected population.
    pub lethality: f64,
    /// How many years an outbreak typically lasts in a settlement.
    pub duration_years: u32,
    /// Per-bracket mortality multipliers (indexes match population brackets).
    pub bracket_severity: [f64; NUM_BRACKETS],
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
    Culture(CultureData),
    Disease(DiseaseData),
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
                culture_id: None,
            }),
            EntityKind::Settlement => EntityData::Settlement(SettlementData {
                population: 0,
                population_breakdown: PopulationBreakdown::empty(),
                x: 0.0,
                y: 0.0,
                resources: Vec::new(),
                prosperity: 0.5,
                treasury: 0.0,
                dominant_culture: None,
                culture_makeup: BTreeMap::new(),
                cultural_tension: 0.0,
                active_disease: None,
                plague_immunity: 0.0,
            }),
            EntityKind::Faction => EntityData::Faction(FactionData {
                government_type: "chieftain".to_string(),
                stability: 0.5,
                happiness: 0.5,
                legitimacy: 0.5,
                treasury: 0.0,
                alliance_strength: 0.0,
                primary_culture: None,
            }),
            EntityKind::Culture => EntityData::Culture(CultureData {
                values: Vec::new(),
                naming_style: NamingStyle::Nordic,
                resistance: 0.5,
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
            EntityKind::Disease => EntityData::Disease(DiseaseData {
                virulence: 0.5,
                lethality: 0.3,
                duration_years: 3,
                bracket_severity: [1.0; NUM_BRACKETS],
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

    pub fn as_culture(&self) -> Option<&CultureData> {
        match self {
            EntityData::Culture(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_culture_mut(&mut self) -> Option<&mut CultureData> {
        match self {
            EntityData::Culture(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_disease(&self) -> Option<&DiseaseData> {
        match self {
            EntityData::Disease(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_disease_mut(&mut self) -> Option<&mut DiseaseData> {
        match self {
            EntityData::Disease(d) => Some(d),
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
            culture_id: None,
        });
        let json = serde_json::to_string(&data).unwrap();
        let back: EntityData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }
}
