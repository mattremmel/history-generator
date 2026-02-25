use serde::{Deserialize, Serialize};

use super::timestamp::SimTimestamp;

/// Institutional or personal memory of a wrong committed by another faction.
///
/// Stored as `BTreeMap<u64, Grievance>` on `FactionData` and `PersonData`,
/// keyed by the target faction ID (the wrongdoer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grievance {
    /// Current severity: 0.0 (forgotten) to 1.0 (maximum grudge).
    pub severity: f64,
    /// Human-readable tags describing what caused this grievance (capped at 5).
    pub sources: Vec<String>,
    /// Highest severity ever reached (useful for narrative flavour).
    pub peak: f64,
    /// When this grievance was last updated.
    pub updated: SimTimestamp,
}
