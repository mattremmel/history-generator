use std::collections::BTreeMap;

use bevy_ecs::component::Component;

use crate::model::{DiseaseRisk, PopulationBreakdown, ResourceType, TradeRoute};

/// Core settlement data: identity, geography, economics.
#[derive(Component, Debug, Clone)]
pub struct SettlementCore {
    pub x: f64,
    pub y: f64,
    pub population: u32,
    pub population_breakdown: PopulationBreakdown,
    pub prosperity: f64,
    pub treasury: f64,
    pub capacity: u32,
    pub blend_timer: u32,
    pub last_prophecy_year: Option<u32>,
    pub resources: Vec<ResourceType>,
    pub prestige: f64,
    pub prestige_tier: u8,
}

impl Default for SettlementCore {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            population: 0,
            population_breakdown: PopulationBreakdown::empty(),
            prosperity: 0.5,
            treasury: 0.0,
            capacity: 0,
            blend_timer: 0,
            last_prophecy_year: None,
            resources: Vec::new(),
            prestige: 0.0,
            prestige_tier: 0,
        }
    }
}

/// Cultural composition and tension.
#[derive(Component, Debug, Clone, Default)]
pub struct SettlementCulture {
    pub culture_makeup: BTreeMap<u64, f64>,
    pub dominant_culture: Option<u64>,
    pub cultural_tension: f64,
    pub religion_makeup: BTreeMap<u64, f64>,
    pub dominant_religion: Option<u64>,
    pub religious_tension: f64,
}

/// Disease state and risk factors.
#[derive(Component, Debug, Clone, Default)]
pub struct SettlementDisease {
    pub disease_risk: DiseaseRisk,
    pub plague_immunity: f64,
}

/// Trade routes, income, and production.
#[derive(Component, Debug, Clone, Default)]
pub struct SettlementTrade {
    pub trade_routes: Vec<TradeRoute>,
    pub trade_income: f64,
    pub production: BTreeMap<ResourceType, f64>,
    pub surplus: BTreeMap<ResourceType, f64>,
    pub trade_happiness_bonus: f64,
    pub is_coastal: bool,
}

/// Military infrastructure.
#[derive(Component, Debug, Clone, Default)]
pub struct SettlementMilitary {
    pub fortification_level: u8,
    pub guard_strength: f64,
}

/// Crime and banditry.
#[derive(Component, Debug, Clone, Default)]
pub struct SettlementCrime {
    pub crime_rate: f64,
    pub bandit_threat: f64,
}

/// Education metrics.
#[derive(Component, Debug, Clone, Default)]
pub struct SettlementEducation {
    pub literacy_rate: f64,
}

/// Seasonal modifiers (ECS-side, independent of model's SeasonalModifiers).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct EcsSeasonalModifiers {
    pub food: f64,
    pub trade: f64,
    pub disease: f64,
    pub army: f64,
    pub construction_blocked: bool,
    pub construction_months: u32,
    pub food_annual: f64,
}

impl Default for EcsSeasonalModifiers {
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

/// Building bonuses (ECS-side, independent of model's BuildingBonuses).
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct EcsBuildingBonuses {
    pub mine: f64,
    pub workshop: f64,
    pub market: f64,
    pub port_trade: f64,
    pub port_range: f64,
    pub happiness: f64,
    pub capacity: f64,
    pub food_buffer: f64,
    pub library: f64,
    pub temple_knowledge: f64,
    pub temple_religion: f64,
    pub academy: f64,
    pub fishing: f64,
}
