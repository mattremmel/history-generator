//! Centralized string constants for entity `extra` map keys.
//!
//! Using these constants instead of raw string literals prevents typos and makes
//! it easy to find all producers/consumers of a given key via "Find Usages".

// --- Army extras ---
pub const FACTION_ID: &str = "faction_id";
pub const HOME_REGION_ID: &str = "home_region_id";
pub const BESIEGING_SETTLEMENT_ID: &str = "besieging_settlement_id";
pub const MONTHS_CAMPAIGNING: &str = "months_campaigning";
pub const STARTING_STRENGTH: &str = "starting_strength";

// --- Faction extras ---
pub const WAR_START_YEAR: &str = "war_start_year";
pub const WAR_EXHAUSTION: &str = "war_exhaustion";
pub const ECONOMIC_WAR_MOTIVATION: &str = "economic_war_motivation";

// --- Season/environment (settlement extras, set by EnvironmentSystem) ---
pub const SEASON_FOOD_MODIFIER: &str = "season_food_modifier";
pub const SEASON_TRADE_MODIFIER: &str = "season_trade_modifier";
pub const SEASON_DISEASE_MODIFIER: &str = "season_disease_modifier";
pub const SEASON_ARMY_MODIFIER: &str = "season_army_modifier";
pub const SEASON_CONSTRUCTION_MONTHS: &str = "season_construction_months";
pub const SEASON_FOOD_MODIFIER_ANNUAL: &str = "season_food_modifier_annual";

// --- Building bonuses (settlement extras, set by BuildingSystem) ---
pub const BUILDING_MINE_BONUS: &str = "building_mine_bonus";
pub const BUILDING_WORKSHOP_BONUS: &str = "building_workshop_bonus";
pub const BUILDING_MARKET_BONUS: &str = "building_market_bonus";
pub const BUILDING_PORT_TRADE_BONUS: &str = "building_port_trade_bonus";
pub const BUILDING_PORT_RANGE_BONUS: &str = "building_port_range_bonus";
pub const BUILDING_HAPPINESS_BONUS: &str = "building_happiness_bonus";
pub const BUILDING_CAPACITY_BONUS: &str = "building_capacity_bonus";
pub const BUILDING_FOOD_BUFFER: &str = "building_food_buffer";
pub const BUILDING_LIBRARY_BONUS: &str = "building_library_bonus";
pub const BUILDING_TEMPLE_KNOWLEDGE_BONUS: &str = "building_temple_knowledge_bonus";

// --- Economy (settlement extras) ---
pub const PRODUCTION: &str = "production";
pub const SURPLUS: &str = "surplus";
pub const TRADE_ROUTES: &str = "trade_routes";
pub const TRADE_HAPPINESS_BONUS: &str = "trade_happiness_bonus";

// --- Demographics (settlement extras) ---
pub const CAPACITY: &str = "capacity";

// --- Demographics (person extras) ---
pub const WIDOWED_YEAR: &str = "widowed_year";

// --- Reputation ---
pub const PRESTIGE_TIER: &str = "prestige_tier";

// --- Culture ---
pub const BLEND_TIMER: &str = "blend_timer";

// --- Player/special ---
pub const IS_PLAYER: &str = "is_player";
