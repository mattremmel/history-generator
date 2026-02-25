use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::cultural_value::{CulturalValue, NamingStyle};
use super::entity::EntityKind;
use super::grievance::Grievance;
use super::population::{NUM_BRACKETS, PopulationBreakdown};
use super::secret::SecretDesire;
use super::terrain::{Terrain, TerrainTag};
use super::timestamp::SimTimestamp;
use super::traits::Trait;

// ---------------------------------------------------------------------------
// Sub-structs for promoted extras
// ---------------------------------------------------------------------------

/// Seasonal modifiers applied by the EnvironmentSystem each month.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeasonalModifiers {
    #[serde(default = "default_one")]
    pub food: f64,
    #[serde(default = "default_one")]
    pub trade: f64,
    #[serde(default = "default_one")]
    pub disease: f64,
    #[serde(default = "default_one")]
    pub army: f64,
    #[serde(default)]
    pub construction_blocked: bool,
    #[serde(default = "default_twelve")]
    pub construction_months: u32,
    #[serde(default = "default_one")]
    pub food_annual: f64,
}

impl Default for SeasonalModifiers {
    fn default() -> Self {
        Self {
            food: 1.0,
            trade: 1.0,
            disease: 1.0,
            army: 1.0,
            construction_blocked: false,
            construction_months: 12,
            food_annual: 1.0,
        }
    }
}

/// Bonuses from buildings located in a settlement.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BuildingBonuses {
    #[serde(default)]
    pub mine: f64,
    #[serde(default)]
    pub workshop: f64,
    #[serde(default)]
    pub market: f64,
    #[serde(default)]
    pub port_trade: f64,
    #[serde(default)]
    pub port_range: f64,
    #[serde(default)]
    pub happiness: f64,
    #[serde(default)]
    pub capacity: f64,
    #[serde(default)]
    pub food_buffer: f64,
    #[serde(default)]
    pub library: f64,
    #[serde(default)]
    pub temple_knowledge: f64,
    #[serde(default)]
    pub temple_religion: f64,
}

/// A trade route connecting this settlement to another.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TradeRoute {
    pub target: u64,
    #[serde(default)]
    pub path: Vec<u64>,
    #[serde(default)]
    pub distance: u32,
    #[serde(default)]
    pub resource: String,
}

/// Disease risk factors for a settlement.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DiseaseRisk {
    #[serde(default)]
    pub refugee: f64,
    #[serde(default)]
    pub post_conquest: f64,
    #[serde(default)]
    pub post_disaster: f64,
    #[serde(default)]
    pub siege_bonus: f64,
}

/// A war goal targeting another faction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WarGoal {
    Territorial {
        target_settlements: Vec<u64>,
    },
    Economic {
        reparation_demand: f64,
    },
    Punitive,
    SuccessionClaim {
        claimant_id: u64,
    },
    Expansion {
        target_settlements: Vec<u64>,
        motivation: ExpansionMotivation,
    },
}

/// Motivation behind an expansion war.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpansionMotivation {
    /// Opportunistic attack against a weak neighbor.
    Opportunistic,
    /// Land grab for strategic resources the aggressor lacks.
    ResourceGrab {
        desired_resources: Vec<ResourceType>,
    },
    /// Buffer expansion between self and a powerful enemy.
    DefensiveBuffer { threat_faction_id: u64 },
}

/// A tribute obligation owed to another faction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TributeObligation {
    pub amount: f64,
    pub years_remaining: u32,
    #[serde(default)]
    pub treaty_event_id: u64,
}

/// A succession claim a person holds on a faction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Claim {
    pub strength: f64,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub year: u32,
}

fn default_one() -> f64 {
    1.0
}

fn default_twelve() -> u32 {
    12
}

fn default_diplomatic_trust() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Sex {
    Male,
    Female,
}

string_enum!(Sex {
    Male => "male",
    Female => "female",
});

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Role {
    Common,
    Warrior,
    Scholar,
    Merchant,
    Artisan,
    Elder,
    Priest,
    Custom(String),
}

string_enum_open!(Role, "Role", {
    Common => "common",
    Warrior => "warrior",
    Scholar => "scholar",
    Merchant => "merchant",
    Artisan => "artisan",
    Elder => "elder",
    Priest => "priest",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum BuildingType {
    Mine,
    Port,
    Market,
    Granary,
    Temple,
    Workshop,
    Aqueduct,
    Library,
}

string_enum!(BuildingType {
    Mine => "mine",
    Port => "port",
    Market => "market",
    Granary => "granary",
    Temple => "temple",
    Workshop => "workshop",
    Aqueduct => "aqueduct",
    Library => "library",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonData {
    pub born: SimTimestamp,
    pub sex: Sex,
    pub role: Role,
    pub traits: Vec<Trait>,
    #[serde(default)]
    pub last_action: SimTimestamp,
    #[serde(default)]
    pub culture_id: Option<u64>,
    /// Personal renown: 0.0 (nobody) to 1.0 (legendary). Decays toward baseline.
    #[serde(default)]
    pub prestige: f64,
    /// Personal vendettas against factions, keyed by faction ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub grievances: BTreeMap<u64, Grievance>,
    /// Knowledge this person wants to keep secret, keyed by knowledge entity ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<u64, SecretDesire>,
    /// Succession claims on factions, keyed by faction ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub claims: BTreeMap<u64, Claim>,
    /// When this person was widowed (spouse died).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widowed_at: Option<SimTimestamp>,
    /// Cached prestige tier (0=Obscure, 1=Notable, 2=Renowned, 3=Illustrious, 4=Legendary).
    #[serde(default)]
    pub prestige_tier: u8,
    /// Generic loyalty toward other entities (target entity ID → loyalty score 0.0-1.0).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub loyalty: BTreeMap<u64, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettlementData {
    pub population: u32,
    pub population_breakdown: PopulationBreakdown,
    pub x: f64,
    pub y: f64,
    pub resources: Vec<ResourceType>,
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
    #[serde(default)]
    pub fortification_level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_siege: Option<ActiveSiege>,
    /// Settlement renown: 0.0 (forgotten hamlet) to 1.0 (legendary city). Decays toward baseline.
    #[serde(default)]
    pub prestige: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_disaster: Option<ActiveDisaster>,
    /// Crime rate: 0.0 (peaceful) to 1.0 (lawless). Computed by CrimeSystem.
    #[serde(default)]
    pub crime_rate: f64,
    /// Guard/patrol strength: 0.0 (unpatrolled) to 1.0 (heavily guarded). Funded by faction treasury.
    #[serde(default)]
    pub guard_strength: f64,
    /// Bandit pressure from nearby bandit factions. Proportional to bandit army strength.
    #[serde(default)]
    pub bandit_threat: f64,
    /// The dominant religion in this settlement (highest share).
    #[serde(default)]
    pub dominant_religion: Option<u64>,
    /// Share of each religion: religion_id → fraction (0.0-1.0, sums to ~1.0).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub religion_makeup: BTreeMap<u64, f64>,
    /// Religious tension: 0.0 (homogeneous) to 1.0 (deeply divided).
    #[serde(default)]
    pub religious_tension: f64,
    /// Carrying capacity of this settlement.
    #[serde(default)]
    pub capacity: u32,
    /// Happiness bonus from active trade routes.
    #[serde(default)]
    pub trade_happiness_bonus: f64,
    /// Culture blending countdown timer (years remaining).
    #[serde(default)]
    pub blend_timer: u32,
    /// Year of the last prophecy declared here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prophecy_year: Option<u32>,
    /// Cached trade income (set by economy trade system each year).
    #[serde(default)]
    pub trade_income: f64,
    /// Active trade routes from this settlement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trade_routes: Vec<TradeRoute>,
    /// Resource production amounts by type.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub production: BTreeMap<ResourceType, f64>,
    /// Resource surplus/deficit by type.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub surplus: BTreeMap<ResourceType, f64>,
    /// Seasonal modifiers (set by EnvironmentSystem each month).
    #[serde(default)]
    pub seasonal: SeasonalModifiers,
    /// Building bonuses (set by BuildingSystem each tick).
    #[serde(default)]
    pub building_bonuses: BuildingBonuses,
    /// Disease risk factors from various sources.
    #[serde(default)]
    pub disease_risk: DiseaseRisk,
    /// Cached prestige tier (0=Obscure, 1=Notable, 2=Renowned, 3=Illustrious, 4=Legendary).
    #[serde(default)]
    pub prestige_tier: u8,
}

impl SettlementData {
    /// Synchronize `self.population` with the total from `self.population_breakdown`.
    pub fn sync_population(&mut self) {
        self.population = self.population_breakdown.total();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveSiege {
    pub attacker_army_id: u64,
    pub attacker_faction_id: u64,
    pub started: SimTimestamp,
    pub months_elapsed: u32,
    pub civilian_deaths: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum DisasterType {
    Earthquake,
    Flood,
    Drought,
    VolcanicEruption,
    Wildfire,
    Storm,
    Tsunami,
}

string_enum!(DisasterType {
    Earthquake => "earthquake",
    Flood => "flood",
    Drought => "drought",
    VolcanicEruption => "volcanic_eruption",
    Wildfire => "wildfire",
    Storm => "storm",
    Tsunami => "tsunami",
});

impl DisasterType {
    /// Returns true if this disaster type persists across multiple months.
    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            DisasterType::Drought | DisasterType::Flood | DisasterType::Wildfire
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum SiegeOutcome {
    Conquered,
    Lifted,
    Abandoned,
}

string_enum!(SiegeOutcome {
    Conquered => "conquered",
    Lifted => "lifted",
    Abandoned => "abandoned",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveDisaster {
    pub disaster_type: DisasterType,
    pub severity: f64,
    pub started: SimTimestamp,
    pub months_remaining: u32,
    #[serde(default)]
    pub total_deaths: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveDisease {
    pub disease_id: u64,
    pub started: SimTimestamp,
    pub infection_rate: f64,
    pub peak_reached: bool,
    /// Running total of deaths caused by this outbreak in this settlement.
    #[serde(default)]
    pub total_deaths: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum GovernmentType {
    Hereditary,
    Elective,
    Chieftain,
    BanditClan,
    Theocracy,
    MercenaryCompany,
}

string_enum!(GovernmentType {
    Hereditary => "hereditary",
    Elective => "elective",
    Chieftain => "chieftain",
    BanditClan => "bandit_clan",
    Theocracy => "theocracy",
    MercenaryCompany => "mercenary_company",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactionData {
    pub government_type: GovernmentType,
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
    /// Faction prestige: 0.0 (obscure) to 1.0 (hegemonic). Decays toward baseline.
    #[serde(default)]
    pub prestige: f64,
    /// The faction's official/primary religion.
    #[serde(default)]
    pub primary_religion: Option<u64>,
    /// Institutional grudges against other factions, keyed by target faction ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub grievances: BTreeMap<u64, Grievance>,
    /// Knowledge this faction wants to keep secret, keyed by knowledge entity ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<u64, SecretDesire>,
    /// When the current war started (None if not at war).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub war_started: Option<SimTimestamp>,
    /// Economic motivation for the current war.
    #[serde(default)]
    pub economic_motivation: f64,
    /// Diplomatic trust level (default 1.0). Low values block alliances.
    #[serde(default = "default_diplomatic_trust")]
    pub diplomatic_trust: f64,
    /// Number of times this faction has betrayed allies.
    #[serde(default)]
    pub betrayal_count: u32,
    /// When this faction last committed a betrayal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_betrayal: Option<SimTimestamp>,
    /// Entity ID of the faction that last betrayed this faction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_betrayed_by: Option<u64>,
    /// When the current succession crisis started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub succession_crisis_at: Option<SimTimestamp>,
    /// Tribute obligations owed to other factions, keyed by payee faction ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tributes: BTreeMap<u64, TributeObligation>,
    /// Cached prestige tier (0=Obscure, 1=Notable, 2=Renowned, 3=Illustrious, 4=Legendary).
    #[serde(default)]
    pub prestige_tier: u8,
    /// Cached trade partner route counts (partner faction ID → route count).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trade_partner_routes: BTreeMap<u64, u32>,
    /// Marriage alliance years (partner faction ID → year formed).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marriage_alliances: BTreeMap<u64, u32>,
    /// Active war goals against other factions, keyed by target faction ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub war_goals: BTreeMap<u64, WarGoal>,
    /// Generic loyalty toward other entities (target entity ID → loyalty score 0.0-1.0).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub loyalty: BTreeMap<u64, f64>,
    /// Gold per strength per month (only meaningful for MercenaryCompany factions).
    #[serde(default)]
    pub mercenary_wage: f64,
    /// Consecutive months the employer failed to pay mercenary wages.
    #[serde(default)]
    pub unpaid_months: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegionData {
    pub terrain: Terrain,
    #[serde(default)]
    pub terrain_tags: Vec<TerrainTag>,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub resources: Vec<ResourceType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArmyData {
    #[serde(default)]
    pub morale: f64,
    #[serde(default)]
    pub supply: f64,
    #[serde(default)]
    pub strength: u32,
    /// The faction this army belongs to.
    #[serde(default)]
    pub faction_id: u64,
    /// The region this army was mustered from.
    #[serde(default)]
    pub home_region_id: u64,
    /// The settlement this army is currently besieging, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub besieging_settlement_id: Option<u64>,
    /// How many months this army has been campaigning.
    #[serde(default)]
    pub months_campaigning: u32,
    /// The initial strength when mustered.
    #[serde(default)]
    pub starting_strength: u32,
    /// Whether this army belongs to a mercenary company.
    #[serde(default)]
    pub is_mercenary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum FeatureType {
    Cave,
    MountainPass,
    Clearing,
    Grove,
    Sinkhole,
    HotSpring,
    LavaTube,
    Harbor,
    LavaField,
    FaultLine,
    Crater,
    Custom(String),
}

string_enum_open!(FeatureType, "feature type", {
    Cave => "cave",
    MountainPass => "mountain_pass",
    Clearing => "clearing",
    Grove => "grove",
    Sinkhole => "sinkhole",
    HotSpring => "hot_spring",
    LavaTube => "lava_tube",
    Harbor => "harbor",
    LavaField => "lava_field",
    FaultLine => "fault_line",
    Crater => "crater",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeographicFeatureData {
    pub feature_type: FeatureType,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ResourceType {
    Grain,
    Timber,
    Game,
    Horses,
    Cattle,
    Sheep,
    Herbs,
    Peat,
    Furs,
    Freshwater,
    Iron,
    Stone,
    Copper,
    Gold,
    Gems,
    Obsidian,
    Sulfur,
    Clay,
    Glass,
    Ivory,
    Ore,
    Salt,
    Pearls,
    Spices,
    Dyes,
    Fish,
    Whales,
    Custom(String),
}

string_enum_open!(ResourceType, "resource type", {
    Grain => "grain",
    Timber => "timber",
    Game => "game",
    Horses => "horses",
    Cattle => "cattle",
    Sheep => "sheep",
    Herbs => "herbs",
    Peat => "peat",
    Furs => "furs",
    Freshwater => "freshwater",
    Iron => "iron",
    Stone => "stone",
    Copper => "copper",
    Gold => "gold",
    Gems => "gems",
    Obsidian => "obsidian",
    Sulfur => "sulfur",
    Clay => "clay",
    Glass => "glass",
    Ivory => "ivory",
    Ore => "ore",
    Salt => "salt",
    Pearls => "pearls",
    Spices => "spices",
    Dyes => "dyes",
    Fish => "fish",
    Whales => "whales",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceDepositData {
    pub resource_type: ResourceType,
    pub quantity: u32,
    pub quality: f64,
    pub discovered: bool,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuildingData {
    pub building_type: BuildingType,
    #[serde(default)]
    pub output_resource: Option<ResourceType>,
    pub x: f64,
    pub y: f64,
    /// Structural condition: 0.0 (ruined) to 1.0 (pristine).
    #[serde(default = "default_condition")]
    pub condition: f64,
    /// Upgrade level: 0 (basic), 1 (improved), 2 (grand).
    #[serde(default)]
    pub level: u8,
    /// When the building was constructed.
    #[serde(default)]
    pub constructed: SimTimestamp,
}

fn default_condition() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiverData {
    pub region_path: Vec<u64>,
    pub length: u32,
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

// ---------------------------------------------------------------------------
// Religion data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ReligiousTenet {
    WarGod,
    NatureWorship,
    AncestorCult,
    Prophecy,
    Asceticism,
    Commerce,
    Knowledge,
    Death,
}

string_enum!(ReligiousTenet {
    WarGod => "war_god",
    NatureWorship => "nature_worship",
    AncestorCult => "ancestor_cult",
    Prophecy => "prophecy",
    Asceticism => "asceticism",
    Commerce => "commerce",
    Knowledge => "knowledge",
    Death => "death",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum DeityDomain {
    Sky,
    Earth,
    Sea,
    War,
    Death,
    Harvest,
    Craft,
    Wisdom,
    Storm,
    Fire,
}

string_enum!(DeityDomain {
    Sky => "sky",
    Earth => "earth",
    Sea => "sea",
    War => "war",
    Death => "death",
    Harvest => "harvest",
    Craft => "craft",
    Wisdom => "wisdom",
    Storm => "storm",
    Fire => "fire",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReligionData {
    /// 0.0-1.0: zealousness of the faith.
    pub fervor: f64,
    /// 0.0-1.0: missionary aggressiveness.
    pub proselytism: f64,
    /// 0.0-1.0: doctrinal rigidity (harder schism but more explosive).
    pub orthodoxy: f64,
    /// Core tenets of this religion.
    #[serde(default)]
    pub tenets: Vec<ReligiousTenet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeityData {
    pub domain: DeityDomain,
    /// 0.0-1.0: how strongly this deity is worshipped.
    #[serde(default)]
    pub worship_strength: f64,
}

// ---------------------------------------------------------------------------
// Item data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum ItemType {
    Weapon,
    Tool,
    Jewelry,
    Idol,
    Amulet,
    Seal,
    Crown,
    Tablet,
    Pottery,
    Chest,
}

string_enum!(ItemType {
    Weapon => "weapon",
    Tool => "tool",
    Jewelry => "jewelry",
    Idol => "idol",
    Amulet => "amulet",
    Seal => "seal",
    Crown => "crown",
    Tablet => "tablet",
    Pottery => "pottery",
    Chest => "chest",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemData {
    pub item_type: ItemType,
    pub material: String,
    /// Accumulated narrative significance: 0.0 (mundane) to 1.0 (legendary).
    #[serde(default)]
    pub resonance: f64,
    /// Physical condition: 0.0 (destroyed) to 1.0 (pristine).
    #[serde(default = "default_condition")]
    pub condition: f64,
    /// When the item was created.
    #[serde(default)]
    pub created: SimTimestamp,
    /// Resonance tier (0-3), derived from resonance value.
    #[serde(default)]
    pub resonance_tier: u8,
    /// When this item was last transferred to a new holder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transferred: Option<SimTimestamp>,
}

// ---------------------------------------------------------------------------
// Knowledge & Manifestation data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum KnowledgeCategory {
    Battle,
    Conquest,
    Dynasty,
    Disaster,
    Founding,
    Cultural,
    Diplomatic,
    Construction,
    Religious,
}

string_enum!(KnowledgeCategory {
    Battle => "battle",
    Conquest => "conquest",
    Dynasty => "dynasty",
    Disaster => "disaster",
    Founding => "founding",
    Cultural => "cultural",
    Diplomatic => "diplomatic",
    Construction => "construction",
    Religious => "religious",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeData {
    pub category: KnowledgeCategory,
    pub source_event_id: u64,
    pub origin_settlement_id: u64,
    pub origin_time: SimTimestamp,
    /// 0.0-1.0: gates propagation range and derivation likelihood.
    pub significance: f64,
    /// The actual facts — DM's version.
    pub ground_truth: serde_json::Value,
    /// When this knowledge was revealed (secret became public).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revealed_at: Option<SimTimestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Medium {
    Memory,
    OralTradition,
    WrittenBook,
    Scroll,
    CarvedStone,
    Song,
    Painting,
    Tapestry,
    Tattoo,
    Dream,
    MagicalImprint,
    EncodedCipher,
}

string_enum!(Medium {
    Memory => "memory",
    OralTradition => "oral_tradition",
    WrittenBook => "written_book",
    Scroll => "scroll",
    CarvedStone => "carved_stone",
    Song => "song",
    Painting => "painting",
    Tapestry => "tapestry",
    Tattoo => "tattoo",
    Dream => "dream",
    MagicalImprint => "magical_imprint",
    EncodedCipher => "encoded_cipher",
});

impl Medium {
    /// Annual decay rate for this medium's physical condition.
    pub fn decay_rate(&self) -> f64 {
        match self {
            Medium::Memory => 0.05,
            Medium::OralTradition => 0.02,
            Medium::Song => 0.01,
            Medium::WrittenBook => 0.005,
            Medium::Scroll => 0.008,
            Medium::CarvedStone => 0.001,
            Medium::Painting => 0.004,
            Medium::Tapestry => 0.003,
            Medium::Tattoo => 0.03,
            Medium::Dream => 0.10,
            Medium::MagicalImprint => 0.0,
            Medium::EncodedCipher => 0.005,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum DerivationMethod {
    #[default]
    Witnessed,
    Copied,
    Memorized,
    Retold,
    TranscribedFromOral,
    SetToMusic,
    Taught,
    Carved,
    MagicallyRecorded,
    Derived,
    Dreamed,
    Custom(String),
}

string_enum_open!(DerivationMethod, "derivation method", {
    Witnessed => "witnessed",
    Copied => "copied",
    Memorized => "memorized",
    Retold => "retold",
    TranscribedFromOral => "transcribed_from_oral",
    SetToMusic => "set_to_music",
    Taught => "taught",
    Carved => "carved",
    MagicallyRecorded => "magically_recorded",
    Derived => "derived",
    Dreamed => "dreamed",
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestationData {
    pub knowledge_id: u64,
    pub medium: Medium,
    /// What THIS version says (may diverge from ground_truth).
    pub content: serde_json::Value,
    /// 0.0-1.0 accuracy vs ground truth.
    pub accuracy: f64,
    /// 0.0-1.0 how much of the original is present.
    pub completeness: f64,
    /// Applied distortions (each element describes one drift/mutation).
    #[serde(default)]
    pub distortions: Vec<serde_json::Value>,
    /// Parent manifestation entity ID (None = original/eyewitness).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from_id: Option<u64>,
    /// How this manifestation was created.
    #[serde(default)]
    pub derivation_method: DerivationMethod,
    /// Physical condition: 1.0 pristine → 0.0 destroyed.
    pub condition: f64,
    pub created: SimTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
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
    Knowledge(KnowledgeData),
    Manifestation(ManifestationData),
    Item(ItemData),
    Religion(ReligionData),
    Deity(DeityData),
    None,
}

macro_rules! entity_data_accessors {
    ($( $variant:ident, $data_ty:ident, $as_ref:ident, $as_mut:ident; )*) => {
        $(
            pub fn $as_ref(&self) -> Option<&$data_ty> {
                match self {
                    EntityData::$variant(d) => Some(d),
                    _ => None,
                }
            }

            pub fn $as_mut(&mut self) -> Option<&mut $data_ty> {
                match self {
                    EntityData::$variant(d) => Some(d),
                    _ => None,
                }
            }
        )*
    };
}

impl EntityData {
    pub fn default_for_kind(kind: EntityKind) -> Self {
        match kind {
            EntityKind::Person => EntityData::Person(PersonData {
                born: SimTimestamp::default(),
                sex: Sex::Male,
                role: Role::Common,
                traits: Vec::new(),
                last_action: SimTimestamp::default(),
                culture_id: None,
                prestige: 0.0,
                grievances: BTreeMap::new(),
                secrets: BTreeMap::new(),
                claims: BTreeMap::new(),
                widowed_at: None,
                prestige_tier: 0,
                loyalty: BTreeMap::new(),
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
                fortification_level: 0,
                active_siege: None,
                prestige: 0.0,
                active_disaster: None,
                crime_rate: 0.0,
                guard_strength: 0.0,
                bandit_threat: 0.0,
                dominant_religion: None,
                religion_makeup: BTreeMap::new(),
                religious_tension: 0.0,
                capacity: 0,
                trade_happiness_bonus: 0.0,
                blend_timer: 0,
                last_prophecy_year: None,
                trade_routes: Vec::new(),
                production: BTreeMap::new(),
                surplus: BTreeMap::new(),
                seasonal: SeasonalModifiers::default(),
                building_bonuses: BuildingBonuses::default(),
                disease_risk: DiseaseRisk::default(),
                prestige_tier: 0,
                trade_income: 0.0,
            }),
            EntityKind::Faction => EntityData::Faction(FactionData {
                government_type: GovernmentType::Chieftain,
                stability: 0.5,
                happiness: 0.5,
                legitimacy: 0.5,
                treasury: 0.0,
                alliance_strength: 0.0,
                primary_culture: None,
                prestige: 0.0,
                primary_religion: None,
                grievances: BTreeMap::new(),
                secrets: BTreeMap::new(),
                war_started: None,
                economic_motivation: 0.0,
                diplomatic_trust: 1.0,
                betrayal_count: 0,
                last_betrayal: None,
                last_betrayed_by: None,
                succession_crisis_at: None,
                tributes: BTreeMap::new(),
                prestige_tier: 0,
                trade_partner_routes: BTreeMap::new(),
                marriage_alliances: BTreeMap::new(),
                war_goals: BTreeMap::new(),
                loyalty: BTreeMap::new(),
                mercenary_wage: 0.0,
                unpaid_months: 0,
            }),
            EntityKind::Culture => EntityData::Culture(CultureData {
                values: Vec::new(),
                naming_style: NamingStyle::Nordic,
                resistance: 0.5,
            }),
            EntityKind::Region => EntityData::Region(RegionData {
                terrain: Terrain::Plains,
                terrain_tags: Vec::new(),
                x: 0.0,
                y: 0.0,
                resources: Vec::new(),
            }),
            EntityKind::Army => EntityData::Army(ArmyData {
                morale: 1.0,
                supply: 1.0,
                strength: 0,
                faction_id: 0,
                home_region_id: 0,
                besieging_settlement_id: None,
                months_campaigning: 0,
                starting_strength: 0,
                is_mercenary: false,
            }),
            EntityKind::GeographicFeature => EntityData::GeographicFeature(GeographicFeatureData {
                feature_type: FeatureType::Crater,
                x: 0.0,
                y: 0.0,
            }),
            EntityKind::ResourceDeposit => EntityData::ResourceDeposit(ResourceDepositData {
                resource_type: ResourceType::Iron,
                quantity: 0,
                quality: 0.0,
                discovered: false,
                x: 0.0,
                y: 0.0,
            }),
            EntityKind::Building => EntityData::Building(BuildingData {
                building_type: BuildingType::Mine,
                output_resource: None,
                x: 0.0,
                y: 0.0,
                condition: 1.0,
                level: 0,
                constructed: SimTimestamp::default(),
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
            EntityKind::Knowledge => EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Battle,
                source_event_id: 0,
                origin_settlement_id: 0,
                origin_time: SimTimestamp::default(),
                significance: 0.0,
                ground_truth: serde_json::Value::Null,
                revealed_at: None,
            }),
            EntityKind::Manifestation => EntityData::Manifestation(ManifestationData {
                knowledge_id: 0,
                medium: Medium::Memory,
                content: serde_json::Value::Null,
                accuracy: 1.0,
                completeness: 1.0,
                distortions: Vec::new(),
                derived_from_id: None,
                derivation_method: DerivationMethod::default(),
                condition: 1.0,
                created: SimTimestamp::default(),
            }),
            EntityKind::Item => EntityData::Item(ItemData {
                item_type: ItemType::Tool,
                material: String::new(),
                resonance: 0.0,
                condition: 1.0,
                created: SimTimestamp::default(),
                resonance_tier: 0,
                last_transferred: None,
            }),
            EntityKind::Religion => EntityData::Religion(ReligionData {
                fervor: 0.5,
                proselytism: 0.3,
                orthodoxy: 0.5,
                tenets: Vec::new(),
            }),
            EntityKind::Deity => EntityData::Deity(DeityData {
                domain: DeityDomain::Sky,
                worship_strength: 0.5,
            }),
            EntityKind::Creature => EntityData::None,
        }
    }

    entity_data_accessors! {
        Person, PersonData, as_person, as_person_mut;
        Settlement, SettlementData, as_settlement, as_settlement_mut;
        Faction, FactionData, as_faction, as_faction_mut;
        Region, RegionData, as_region, as_region_mut;
        Army, ArmyData, as_army, as_army_mut;
        GeographicFeature, GeographicFeatureData, as_geographic_feature, as_geographic_feature_mut;
        ResourceDeposit, ResourceDepositData, as_resource_deposit, as_resource_deposit_mut;
        Building, BuildingData, as_building, as_building_mut;
        River, RiverData, as_river, as_river_mut;
        Culture, CultureData, as_culture, as_culture_mut;
        Disease, DiseaseData, as_disease, as_disease_mut;
        Knowledge, KnowledgeData, as_knowledge, as_knowledge_mut;
        Manifestation, ManifestationData, as_manifestation, as_manifestation_mut;
        Item, ItemData, as_item, as_item_mut;
        Religion, ReligionData, as_religion, as_religion_mut;
        Deity, DeityData, as_deity, as_deity_mut;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_for_kind_person() {
        let data = EntityData::default_for_kind(EntityKind::Person);
        assert!(data.as_person().is_some());
    }

    #[test]
    fn default_for_kind_settlement() {
        let data = EntityData::default_for_kind(EntityKind::Settlement);
        let s = data.as_settlement().unwrap();
        assert_eq!(s.population, 0);
        assert!((s.prosperity - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn default_for_kind_unknown_returns_none() {
        let data = EntityData::default_for_kind(EntityKind::Creature);
        assert!(matches!(data, EntityData::None));
    }

    #[test]
    fn accessor_mut_works() {
        let mut data = EntityData::default_for_kind(EntityKind::Faction);
        data.as_faction_mut().unwrap().stability = 0.9;
        assert!((data.as_faction().unwrap().stability - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn serde_round_trip() {
        let data = EntityData::Person(PersonData {
            born: SimTimestamp::from_year(100),
            sex: Sex::Male,
            role: Role::Warrior,
            traits: vec![Trait::Ambitious, Trait::Aggressive],
            last_action: SimTimestamp::from_year(105),
            culture_id: None,
            prestige: 0.0,
            grievances: BTreeMap::new(),
            secrets: BTreeMap::new(),
            claims: BTreeMap::new(),
            widowed_at: None,
            prestige_tier: 0,
            loyalty: BTreeMap::new(),
        });
        let json = serde_json::to_string(&data).unwrap();
        let back: EntityData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }
}
