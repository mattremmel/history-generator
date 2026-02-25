use serde::{Deserialize, Serialize};

use super::timestamp::SimTimestamp;

/// A desire to keep a specific piece of knowledge secret.
///
/// Stored as `BTreeMap<u64, SecretDesire>` on `FactionData` and `PersonData`,
/// keyed by the knowledge entity ID that should be suppressed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretDesire {
    /// Why this knowledge is being suppressed.
    pub motivation: SecretMotivation,
    /// How sensitive: 0.0 (mild) to 1.0 (existential). Affects suppression strength
    /// and consequences when revealed.
    pub sensitivity: f64,
    /// Manifestation accuracy below this threshold = no longer worth suppressing.
    /// Defaults to 0.3 — a heavily distorted version has lost the dangerous content.
    #[serde(default = "default_accuracy_threshold")]
    pub accuracy_threshold: f64,
    /// When this secret desire was created.
    pub created: SimTimestamp,
}

fn default_accuracy_threshold() -> f64 {
    0.3
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum SecretMotivation {
    /// Military weakness, planned attack, hidden resources.
    Strategic,
    /// Crimes, betrayals, scandals.
    Shameful,
    /// Forbidden rituals, destructive knowledge.
    Dangerous,
    /// Religious mysteries.
    Sacred,
}

string_enum!(SecretMotivation {
    Strategic => "strategic",
    Shameful => "shameful",
    Dangerous => "dangerous",
    Sacred => "sacred",
});
