use std::collections::BTreeMap;

use bevy_ecs::component::Component;

use crate::ecs::time::SimTime;
use crate::model::{GovernmentType, Grievance, SecretDesire, TributeObligation, WarGoal};

/// Core faction identity and stats.
#[derive(Component, Debug, Clone)]
pub struct FactionCore {
    pub government_type: GovernmentType,
    pub stability: f64,
    pub happiness: f64,
    pub legitimacy: f64,
    pub treasury: f64,
    pub primary_culture: Option<u64>,
    pub primary_religion: Option<u64>,
    pub prestige: f64,
    pub prestige_tier: u8,
    pub literacy_rate: f64,
    pub succession_crisis_at: Option<SimTime>,
}

impl Default for FactionCore {
    fn default() -> Self {
        Self {
            government_type: GovernmentType::Chieftain,
            stability: 0.0,
            happiness: 0.0,
            legitimacy: 0.0,
            treasury: 0.0,
            primary_culture: None,
            primary_religion: None,
            prestige: 0.0,
            prestige_tier: 0,
            literacy_rate: 0.0,
            succession_crisis_at: None,
        }
    }
}

/// Diplomatic state: grievances, trust, alliances, secrets.
#[derive(Component, Debug, Clone, Default)]
pub struct FactionDiplomacy {
    pub grievances: BTreeMap<u64, Grievance>,
    pub war_goals: BTreeMap<u64, WarGoal>,
    pub tributes: BTreeMap<u64, TributeObligation>,
    pub alliance_strength: f64,
    pub marriage_alliances: BTreeMap<u64, u32>,
    pub loyalty: BTreeMap<u64, f64>,
    pub trade_partner_routes: BTreeMap<u64, u32>,
    pub secrets: BTreeMap<u64, SecretDesire>,
    pub diplomatic_trust: f64,
    pub betrayal_count: u32,
    pub last_betrayal: Option<SimTime>,
    pub last_betrayed_by: Option<u64>,
}

/// Military posture and mercenary state.
#[derive(Component, Debug, Clone, Default)]
pub struct FactionMilitary {
    pub war_started: Option<SimTime>,
    pub mercenary_wage: f64,
    pub unpaid_months: u32,
    pub economic_motivation: f64,
}
