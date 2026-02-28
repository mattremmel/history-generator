//! Shared helpers used across multiple ECS systems.

use std::collections::BTreeMap;

use crate::ecs::components::FactionCore;
use crate::model::GovernmentType;

/// Normalize a culture/religion makeup map so values sum to 1.0.
pub(crate) fn normalize_makeup(makeup: &mut BTreeMap<u64, f64>) {
    let total: f64 = makeup.values().sum();
    if total > 0.0 {
        for share in makeup.values_mut() {
            *share /= total;
        }
    }
}

/// Remove entries below a minimum share threshold.
pub(crate) fn purge_below_threshold(makeup: &mut BTreeMap<u64, f64>, threshold: f64) {
    makeup.retain(|_, share| *share >= threshold);
}

/// Returns true if the faction is a bandit clan or mercenary company (non-state).
pub(crate) fn is_non_state_faction(core: &FactionCore) -> bool {
    matches!(
        core.government_type,
        GovernmentType::BanditClan | GovernmentType::MercenaryCompany
    )
}
