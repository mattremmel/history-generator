use std::collections::BTreeMap;

use bevy_ecs::component::Component;

use crate::ecs::time::SimTime;
use crate::model::{Claim, Grievance, Role, SecretDesire, Sex, Trait};

/// Core person identity and stats.
#[derive(Component, Debug, Clone)]
pub struct PersonCore {
    pub born: SimTime,
    pub sex: Sex,
    pub role: Role,
    pub traits: Vec<Trait>,
    pub last_action: SimTime,
    pub culture_id: Option<u64>,
    pub widowed_at: Option<SimTime>,
}

impl Default for PersonCore {
    fn default() -> Self {
        Self {
            born: SimTime::default(),
            sex: Sex::Male,
            role: Role::Common,
            traits: Vec::new(),
            last_action: SimTime::default(),
            culture_id: None,
            widowed_at: None,
        }
    }
}

/// Personal renown.
#[derive(Component, Debug, Clone, Default)]
pub struct PersonReputation {
    pub prestige: f64,
    pub prestige_tier: u8,
}

/// Social ties: grievances, secrets, claims, loyalty.
#[derive(Component, Debug, Clone, Default)]
pub struct PersonSocial {
    pub grievances: BTreeMap<u64, Grievance>,
    pub secrets: BTreeMap<u64, SecretDesire>,
    pub claims: BTreeMap<u64, Claim>,
    pub loyalty: BTreeMap<u64, f64>,
}

/// Education level.
#[derive(Component, Debug, Clone, Default)]
pub struct PersonEducation {
    pub education: f64,
}
