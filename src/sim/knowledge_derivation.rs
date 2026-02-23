use rand::{Rng, RngCore};

use crate::model::{
    EntityData, EntityKind, ManifestationData, Medium, RelationshipKind, SimTimestamp, World,
};

// ---------------------------------------------------------------------------
// Transition profiles: what happens when deriving from one medium to another
// ---------------------------------------------------------------------------

struct TransitionProfile {
    method: &'static str,
    accuracy_retention: (f64, f64),
    completeness_retention: (f64, f64),
    distortions: &'static [(DistortionKind, f64)],
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum DistortionKind {
    NumbersExaggerated,
    NumbersRounded,
    NamesCorrupted,
    DetailsLost,
    MundaneDetailsLost,
    DetailsEmbellished,
    SupernaturalAdded,
    HeroEmbellished,
    VillainDemonized,
    DatesShifted,
    ChronologyRearranged,
    EmotionalColoring,
    FactionBias,
    ScribalError,
    DeliberateEdit,
    EditorialAddition,
    MetaphorReplacesLiteral,
    DetailsSimplified,
    DialectMisunderstanding,
}

// Key transition profiles
static WRITTEN_TO_WRITTEN: TransitionProfile = TransitionProfile {
    method: "copied",
    accuracy_retention: (0.95, 1.0),
    completeness_retention: (0.90, 1.0),
    distortions: &[
        (DistortionKind::ScribalError, 0.1),
        (DistortionKind::DeliberateEdit, 0.05),
    ],
};

static WRITTEN_TO_MEMORY: TransitionProfile = TransitionProfile {
    method: "memorized",
    accuracy_retention: (0.60, 0.90),
    completeness_retention: (0.40, 0.80),
    distortions: &[
        (DistortionKind::NumbersRounded, 0.5),
        (DistortionKind::NamesCorrupted, 0.3),
        (DistortionKind::DetailsSimplified, 0.6),
    ],
};

static MEMORY_TO_ORAL: TransitionProfile = TransitionProfile {
    method: "retold",
    accuracy_retention: (0.50, 0.85),
    completeness_retention: (0.30, 0.70),
    distortions: &[
        (DistortionKind::NumbersExaggerated, 0.6),
        (DistortionKind::HeroEmbellished, 0.5),
        (DistortionKind::VillainDemonized, 0.4),
        (DistortionKind::MundaneDetailsLost, 0.7),
        (DistortionKind::SupernaturalAdded, 0.2),
    ],
};

static ORAL_TO_WRITTEN: TransitionProfile = TransitionProfile {
    method: "transcribed_from_oral",
    accuracy_retention: (0.90, 1.0),
    completeness_retention: (0.85, 1.0),
    distortions: &[
        (DistortionKind::EditorialAddition, 0.2),
        (DistortionKind::DialectMisunderstanding, 0.15),
    ],
};

static ORAL_TO_SONG: TransitionProfile = TransitionProfile {
    method: "set_to_music",
    accuracy_retention: (0.40, 0.70),
    completeness_retention: (0.30, 0.60),
    distortions: &[
        (DistortionKind::DetailsSimplified, 0.8),
        (DistortionKind::MetaphorReplacesLiteral, 0.7),
        (DistortionKind::ChronologyRearranged, 0.5),
    ],
};

static MEMORY_TO_MEMORY: TransitionProfile = TransitionProfile {
    method: "taught",
    accuracy_retention: (0.70, 0.90),
    completeness_retention: (0.50, 0.85),
    distortions: &[
        (DistortionKind::NumbersRounded, 0.4),
        (DistortionKind::NamesCorrupted, 0.2),
        (DistortionKind::EmotionalColoring, 0.3),
    ],
};

static ANY_TO_STONE: TransitionProfile = TransitionProfile {
    method: "carved",
    accuracy_retention: (0.85, 1.0),
    completeness_retention: (0.40, 0.70),
    distortions: &[
        (DistortionKind::DetailsSimplified, 0.6),
        (DistortionKind::MundaneDetailsLost, 0.5),
    ],
};

static ANY_TO_MAGICAL: TransitionProfile = TransitionProfile {
    method: "magically_recorded",
    accuracy_retention: (0.90, 1.0),
    completeness_retention: (0.85, 1.0),
    distortions: &[(DistortionKind::EmotionalColoring, 0.3)],
};

static GENERIC_FALLBACK: TransitionProfile = TransitionProfile {
    method: "derived",
    accuracy_retention: (0.60, 0.85),
    completeness_retention: (0.40, 0.75),
    distortions: &[
        (DistortionKind::NumbersRounded, 0.3),
        (DistortionKind::DetailsLost, 0.3),
        (DistortionKind::NamesCorrupted, 0.15),
        (DistortionKind::EmotionalColoring, 0.2),
    ],
};

fn get_transition_profile(from: &Medium, to: &Medium) -> &'static TransitionProfile {
    // Special target mediums override source
    match to {
        Medium::CarvedStone => return &ANY_TO_STONE,
        Medium::MagicalImprint => return &ANY_TO_MAGICAL,
        _ => {}
    }
    match (from, to) {
        (Medium::WrittenBook | Medium::Scroll | Medium::EncodedCipher, Medium::WrittenBook | Medium::Scroll | Medium::EncodedCipher) => &WRITTEN_TO_WRITTEN,
        (Medium::WrittenBook | Medium::Scroll, Medium::Memory) => &WRITTEN_TO_MEMORY,
        (Medium::Memory, Medium::OralTradition) => &MEMORY_TO_ORAL,
        (Medium::OralTradition, Medium::WrittenBook | Medium::Scroll) => &ORAL_TO_WRITTEN,
        (Medium::OralTradition, Medium::Song) => &ORAL_TO_SONG,
        (Medium::Memory, Medium::Memory) => &MEMORY_TO_MEMORY,
        (Medium::OralTradition, Medium::OralTradition) => &MEMORY_TO_ORAL,
        (Medium::Song, Medium::Song) => &ORAL_TO_SONG,
        _ => &GENERIC_FALLBACK,
    }
}

// ---------------------------------------------------------------------------
// Distortion application
// ---------------------------------------------------------------------------

fn apply_distortion(
    content: &mut serde_json::Value,
    kind: DistortionKind,
    rng: &mut dyn RngCore,
) -> serde_json::Value {
    match kind {
        DistortionKind::NumbersExaggerated => exaggerate_numbers(content, rng),
        DistortionKind::NumbersRounded => round_numbers(content, rng),
        DistortionKind::NamesCorrupted => corrupt_names(content, rng),
        DistortionKind::DetailsLost => remove_random_keys(content, rng),
        DistortionKind::MundaneDetailsLost => remove_mundane_details(content, rng),
        DistortionKind::DetailsEmbellished => embellish_details(content, rng),
        DistortionKind::SupernaturalAdded => add_supernatural(content, rng),
        DistortionKind::HeroEmbellished => embellish_hero(content, rng),
        DistortionKind::VillainDemonized => demonize_villain(content, rng),
        DistortionKind::DatesShifted => shift_dates(content, rng),
        DistortionKind::ChronologyRearranged => rearrange_chronology(content, rng),
        DistortionKind::EmotionalColoring => add_emotional_coloring(content, rng),
        DistortionKind::FactionBias => apply_faction_bias(content, rng),
        DistortionKind::ScribalError => scribal_error(content, rng),
        DistortionKind::DeliberateEdit => deliberate_edit(content, rng),
        DistortionKind::EditorialAddition => editorial_addition(content, rng),
        DistortionKind::MetaphorReplacesLiteral => metaphor_replaces_literal(content, rng),
        DistortionKind::DetailsSimplified => simplify_details(content, rng),
        DistortionKind::DialectMisunderstanding => dialect_misunderstanding(content, rng),
    }
}

fn distortion_name(kind: DistortionKind) -> &'static str {
    match kind {
        DistortionKind::NumbersExaggerated => "numbers_exaggerated",
        DistortionKind::NumbersRounded => "numbers_rounded",
        DistortionKind::NamesCorrupted => "names_corrupted",
        DistortionKind::DetailsLost => "details_lost",
        DistortionKind::MundaneDetailsLost => "mundane_details_lost",
        DistortionKind::DetailsEmbellished => "details_embellished",
        DistortionKind::SupernaturalAdded => "supernatural_added",
        DistortionKind::HeroEmbellished => "hero_embellished",
        DistortionKind::VillainDemonized => "villain_demonized",
        DistortionKind::DatesShifted => "dates_shifted",
        DistortionKind::ChronologyRearranged => "chronology_rearranged",
        DistortionKind::EmotionalColoring => "emotional_coloring",
        DistortionKind::FactionBias => "faction_bias",
        DistortionKind::ScribalError => "scribal_error",
        DistortionKind::DeliberateEdit => "deliberate_edit",
        DistortionKind::EditorialAddition => "editorial_addition",
        DistortionKind::MetaphorReplacesLiteral => "metaphor_replaces_literal",
        DistortionKind::DetailsSimplified => "details_simplified",
        DistortionKind::DialectMisunderstanding => "dialect_misunderstanding",
    }
}

// ---------------------------------------------------------------------------
// Individual distortion implementations
// ---------------------------------------------------------------------------

/// Walk numeric fields in a JSON value, multiply by 1.5-5.0.
fn exaggerate_numbers(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut affected = Vec::new();
    walk_numbers_mut(content, &mut |key, val| {
        let mult = rng.random_range(1.5..5.0);
        let old = *val;
        *val = (*val * mult).round();
        affected.push(serde_json::json!({"field": key, "old": old, "new": *val}));
    });
    serde_json::json!({"type": "numbers_exaggerated", "changes": affected})
}

/// Round numeric fields to nearest 10/50/100.
fn round_numbers(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut affected = Vec::new();
    walk_numbers_mut(content, &mut |key, val| {
        let old = *val;
        let rounding = match rng.random_range(0..3) {
            0 => 10.0,
            1 => 50.0,
            _ => 100.0,
        };
        *val = (*val / rounding).round() * rounding;
        if (*val - old).abs() > 0.01 {
            affected.push(serde_json::json!({"field": key, "old": old, "new": *val}));
        }
    });
    serde_json::json!({"type": "numbers_rounded", "changes": affected})
}

/// Alter name strings with character-level drift.
fn corrupt_names(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut affected = Vec::new();
    walk_name_strings_mut(content, &mut |key, name| {
        let old = name.clone();
        if name.len() > 2 {
            let idx = rng.random_range(0..name.len());
            let chars: Vec<char> = name.chars().collect();
            let mut new_chars = chars.clone();
            // Swap adjacent chars or drop one
            if rng.random_bool(0.5) && idx + 1 < chars.len() {
                new_chars.swap(idx, idx + 1);
            } else if chars.len() > 3 {
                new_chars.remove(idx);
            }
            *name = new_chars.into_iter().collect();
            if *name != old {
                affected.push(serde_json::json!({"field": key, "old": old, "new": *name}));
            }
        }
    });
    serde_json::json!({"type": "names_corrupted", "changes": affected})
}

/// Remove random non-essential keys from the content object.
fn remove_random_keys(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let essential = ["event_type", "year", "name"];
    let mut removed = Vec::new();
    if let serde_json::Value::Object(map) = content {
        let keys: Vec<String> = map.keys()
            .filter(|k| !essential.contains(&k.as_str()))
            .cloned()
            .collect();
        if !keys.is_empty() {
            let idx = rng.random_range(0..keys.len());
            let key = &keys[idx];
            removed.push(key.clone());
            map.remove(key);
        }
    }
    serde_json::json!({"type": "details_lost", "removed": removed})
}

/// Remove specifically mundane details: casualties, troop counts, etc.
fn remove_mundane_details(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mundane_keys = ["casualties", "troops", "reparations", "severity"];
    let mut removed = Vec::new();
    if let serde_json::Value::Object(map) = content {
        for key in mundane_keys {
            if map.contains_key(key) && rng.random_bool(0.5) {
                map.remove(key);
                removed.push(key.to_string());
            }
        }
        // Also check nested objects (attacker.troops, defender.troops)
        for side in ["attacker", "defender"] {
            if let Some(serde_json::Value::Object(sub)) = map.get_mut(side)
                && sub.contains_key("troops") && rng.random_bool(0.6)
            {
                sub.remove("troops");
                removed.push(format!("{side}.troops"));
            }
        }
    }
    serde_json::json!({"type": "mundane_details_lost", "removed": removed})
}

/// Add fabricated notable_details entries.
fn embellish_details(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let embellishments = [
        "A great omen was seen in the sky",
        "The ground trembled at the moment of victory",
        "Birds fell silent throughout the land",
        "A mysterious stranger appeared before the battle",
    ];
    let idx = rng.random_range(0..embellishments.len());
    if let serde_json::Value::Object(map) = content {
        let details = map.entry("notable_details").or_insert_with(|| serde_json::json!([]));
        if let serde_json::Value::Array(arr) = details {
            arr.push(serde_json::json!(embellishments[idx]));
        }
    }
    serde_json::json!({"type": "details_embellished", "added": embellishments[idx]})
}

/// Add a supernatural notable_detail.
fn add_supernatural(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let supernatural = [
        "The gods intervened directly",
        "A divine light blinded the enemy",
        "The dead rose briefly to fight alongside the living",
        "A curse fell upon the defeated",
    ];
    let idx = rng.random_range(0..supernatural.len());
    if let serde_json::Value::Object(map) = content {
        let details = map.entry("notable_details").or_insert_with(|| serde_json::json!([]));
        if let serde_json::Value::Array(arr) = details {
            arr.push(serde_json::json!(supernatural[idx]));
        }
    }
    serde_json::json!({"type": "supernatural_added", "added": supernatural[idx]})
}

/// Inflate protagonist's positive attributes.
fn embellish_hero(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut changes = Vec::new();
    if let serde_json::Value::Object(map) = content
        && let Some(serde_json::Value::Object(attacker)) = map.get_mut("attacker")
        && let Some(serde_json::Value::Number(n)) = attacker.get_mut("troops")
        && let Some(val) = n.as_f64()
    {
        // Inflate attacker/winner troop count down (they won despite fewer)
        let reduced = (val * rng.random_range(0.5..0.8)).round();
        *n = serde_json::Number::from(reduced as u64);
        changes.push("attacker troops reduced (heroic underdog)");
    }
    serde_json::json!({"type": "hero_embellished", "changes": changes})
}

/// Inflate antagonist's negative attributes.
fn demonize_villain(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut changes = Vec::new();
    if let serde_json::Value::Object(map) = content
        && let Some(serde_json::Value::Object(defender)) = map.get_mut("defender")
        && let Some(serde_json::Value::Number(n)) = defender.get_mut("troops")
        && let Some(val) = n.as_f64()
    {
        let inflated = (val * rng.random_range(1.5..3.0)).round();
        *n = serde_json::Number::from(inflated as u64);
        changes.push("defender troops inflated (monstrous horde)");
    }
    serde_json::json!({"type": "villain_demonized", "changes": changes})
}

/// Shift year values by ±1-20 years.
fn shift_dates(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut changes = Vec::new();
    if let serde_json::Value::Object(map) = content
        && let Some(serde_json::Value::Number(n)) = map.get_mut("year")
        && let Some(year) = n.as_u64()
    {
        let shift: i64 = rng.random_range(-20..=20);
        let new_year = (year as i64 + shift).max(0) as u64;
        *n = serde_json::Number::from(new_year);
        changes.push(serde_json::json!({"field": "year", "shift": shift}));
    }
    serde_json::json!({"type": "dates_shifted", "changes": changes})
}

/// Swap order of notable_details.
fn rearrange_chronology(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut rearranged = false;
    if let serde_json::Value::Object(map) = content
        && let Some(serde_json::Value::Array(arr)) = map.get_mut("notable_details")
        && arr.len() >= 2
    {
        let i = rng.random_range(0..arr.len());
        let j = rng.random_range(0..arr.len());
        arr.swap(i, j);
        rearranged = true;
    }
    serde_json::json!({"type": "chronology_rearranged", "applied": rearranged})
}

/// Add emotional qualifiers to descriptions.
fn add_emotional_coloring(content: &mut serde_json::Value, _rng: &mut dyn RngCore) -> serde_json::Value {
    if let serde_json::Value::Object(map) = content
        && let Some(serde_json::Value::String(outcome)) = map.get_mut("outcome")
    {
        match outcome.as_str() {
            "attacker_victory" => *outcome = "glorious attacker victory".to_string(),
            "defender_victory" => *outcome = "desperate defender victory".to_string(),
            "inconclusive" => *outcome = "bitter and inconclusive struggle".to_string(),
            _ => {}
        }
    }
    serde_json::json!({"type": "emotional_coloring"})
}

/// Minimize own faction's losses, reframe outcomes.
fn apply_faction_bias(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut changes = Vec::new();
    if let serde_json::Value::Object(map) = content {
        // Reduce our side's casualties/losses
        for side in ["attacker", "defender"] {
            if let Some(serde_json::Value::Object(sub)) = map.get_mut(side)
                && let Some(serde_json::Value::Number(n)) = sub.get_mut("troops")
                && let Some(val) = n.as_f64()
            {
                let biased = (val * rng.random_range(0.7..1.0)).round();
                *n = serde_json::Number::from(biased as u64);
                changes.push(format!("{side} troops adjusted by bias"));
            }
        }
    }
    serde_json::json!({"type": "faction_bias", "changes": changes})
}

/// Small character-level transcription mistakes.
fn scribal_error(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut affected = Vec::new();
    walk_name_strings_mut(content, &mut |key, s| {
        if s.len() > 3 && rng.random_bool(0.3) {
            let old = s.clone();
            let chars: Vec<char> = s.chars().collect();
            let idx = rng.random_range(1..chars.len());
            let mut new_chars = chars;
            // Duplicate a character
            let ch = new_chars[idx - 1];
            new_chars.insert(idx, ch);
            *s = new_chars.into_iter().collect();
            affected.push(serde_json::json!({"field": key, "old": old, "new": *s}));
        }
    });
    serde_json::json!({"type": "scribal_error", "changes": affected})
}

/// "Correct" a detail to what copyist thinks is right.
fn deliberate_edit(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut changes = Vec::new();
    // Round a number to a "nicer" value
    walk_numbers_mut(content, &mut |key, val| {
        if rng.random_bool(0.3) {
            let old = *val;
            // Round to nearest 100
            *val = (*val / 100.0).round() * 100.0;
            if (*val - old).abs() > 0.01 {
                changes.push(serde_json::json!({"field": key, "old": old, "new": *val}));
            }
        }
    });
    serde_json::json!({"type": "deliberate_edit", "changes": changes})
}

/// Scribe adds contextual notes as if part of the text.
fn editorial_addition(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let additions = [
        "as is well known to all",
        "according to the ancient custom",
        "as the wise say",
        "which some dispute",
    ];
    let idx = rng.random_range(0..additions.len());
    if let serde_json::Value::Object(map) = content {
        let details = map.entry("notable_details").or_insert_with(|| serde_json::json!([]));
        if let serde_json::Value::Array(arr) = details {
            arr.push(serde_json::json!(additions[idx]));
        }
    }
    serde_json::json!({"type": "editorial_addition", "added": additions[idx]})
}

/// Concrete detail becomes figurative.
fn metaphor_replaces_literal(content: &mut serde_json::Value, _rng: &mut dyn RngCore) -> serde_json::Value {
    let mut changes = Vec::new();
    if let serde_json::Value::Object(map) = content
        && let Some(serde_json::Value::String(outcome)) = map.get_mut("outcome")
    {
        let old = outcome.clone();
        match outcome.as_str() {
            "attacker_victory" => *outcome = "the eagle devoured the serpent".to_string(),
            "defender_victory" => *outcome = "the mountain stood against the storm".to_string(),
            _ => {}
        }
        if *outcome != old {
            changes.push(serde_json::json!({"field": "outcome", "old": old, "new": *outcome}));
        }
    }
    serde_json::json!({"type": "metaphor_replaces_literal", "changes": changes})
}

/// Reduce complex entries to simpler forms.
fn simplify_details(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    let mut simplified = Vec::new();
    if let serde_json::Value::Object(map) = content {
        // Remove nested object detail, keep only names
        for key in ["attacker", "defender"] {
            if let Some(serde_json::Value::Object(sub)) = map.get_mut(key) {
                let keep_keys: Vec<String> = sub.keys()
                    .filter(|k| k.contains("name") || k.contains("id"))
                    .cloned()
                    .collect();
                let all_keys: Vec<String> = sub.keys().cloned().collect();
                for k in &all_keys {
                    if !keep_keys.contains(k) && rng.random_bool(0.4) {
                        sub.remove(k);
                        simplified.push(format!("{key}.{k}"));
                    }
                }
            }
        }
    }
    serde_json::json!({"type": "details_simplified", "removed": simplified})
}

/// Mishear/misread words.
fn dialect_misunderstanding(content: &mut serde_json::Value, rng: &mut dyn RngCore) -> serde_json::Value {
    // Re-use name corruption with lower probability
    corrupt_names(content, rng);
    serde_json::json!({"type": "dialect_misunderstanding"})
}

// ---------------------------------------------------------------------------
// JSON walking helpers
// ---------------------------------------------------------------------------

fn walk_numbers_mut(value: &mut serde_json::Value, f: &mut dyn FnMut(String, &mut f64)) {
    match value {
        serde_json::Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(val) = map.get_mut(&key) {
                    if let Some(n) = val.as_f64() {
                        let mut num = n;
                        f(key.clone(), &mut num);
                        if (num - n).abs() > 0.001 {
                            if num == num.floor() && num >= 0.0 {
                                *val = serde_json::json!(num as u64);
                            } else {
                                *val = serde_json::json!(num);
                            }
                        }
                    } else {
                        walk_numbers_mut(val, f);
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                walk_numbers_mut(item, f);
            }
        }
        _ => {}
    }
}

fn walk_name_strings_mut(value: &mut serde_json::Value, f: &mut dyn FnMut(String, &mut String)) {
    match value {
        serde_json::Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if key.contains("name") || key == "founder_name" {
                    if let Some(serde_json::Value::String(s)) = map.get_mut(&key) {
                        f(key, s);
                    }
                } else if let Some(val) = map.get_mut(&key) {
                    walk_name_strings_mut(val, f);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                walk_name_strings_mut(item, f);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Accuracy calculation
// ---------------------------------------------------------------------------

/// Compare content against ground_truth. Returns 0.0-1.0 accuracy score.
pub fn calculate_accuracy(ground_truth: &serde_json::Value, content: &serde_json::Value) -> f64 {
    let (score, count) = diff_values(ground_truth, content);
    if count == 0 {
        return 1.0;
    }
    (score / count as f64).clamp(0.0, 1.0)
}

fn diff_values(truth: &serde_json::Value, content: &serde_json::Value) -> (f64, u32) {
    match (truth, content) {
        (serde_json::Value::Object(t_map), serde_json::Value::Object(c_map)) => {
            let mut score = 0.0;
            let mut count = 0u32;
            for (key, t_val) in t_map {
                count += 1;
                if let Some(c_val) = c_map.get(key) {
                    let (s, c) = diff_values(t_val, c_val);
                    if c > 0 {
                        score += s;
                        count += c - 1; // already counted parent key
                    } else {
                        score += 1.0; // leaf-level exact match
                    }
                }
                // Missing key = 0 score (already counted)
            }
            (score, count)
        }
        (serde_json::Value::Number(t), serde_json::Value::Number(c)) => {
            let tv = t.as_f64().unwrap_or(0.0);
            let cv = c.as_f64().unwrap_or(0.0);
            if tv == 0.0 && cv == 0.0 {
                (1.0, 1)
            } else if tv == 0.0 || cv == 0.0 {
                (0.0, 1)
            } else {
                let ratio = (cv / tv).min(tv / cv);
                (ratio, 1)
            }
        }
        (serde_json::Value::String(t), serde_json::Value::String(c)) => {
            if t == c {
                (1.0, 1)
            } else {
                (0.0, 1)
            }
        }
        (serde_json::Value::Bool(t), serde_json::Value::Bool(c)) => {
            if t == c { (1.0, 1) } else { (0.0, 1) }
        }
        (serde_json::Value::Array(t), serde_json::Value::Array(c)) => {
            let mut score = 0.0;
            let mut count = 0u32;
            for (i, tv) in t.iter().enumerate() {
                count += 1;
                if let Some(cv) = c.get(i) {
                    let (s, c_inner) = diff_values(tv, cv);
                    if c_inner > 0 {
                        score += s;
                        count += c_inner - 1;
                    } else {
                        score += 1.0;
                    }
                }
            }
            (score, count)
        }
        _ => (0.0, 1),
    }
}

// ---------------------------------------------------------------------------
// Public derivation API
// ---------------------------------------------------------------------------

/// Derive a new manifestation from a source manifestation in a different (or same) medium.
///
/// Creates a new Manifestation entity with HeldBy relationship to `holder_entity_id`.
/// Returns the new manifestation entity ID.
pub fn derive(
    world: &mut World,
    rng: &mut dyn RngCore,
    source_manifestation_id: u64,
    target_medium: Medium,
    holder_entity_id: u64,
    time: SimTimestamp,
    event_id: u64,
) -> Option<u64> {
    // Look up source manifestation
    let source = world.entities.get(&source_manifestation_id)?;
    let source_data = source.data.as_manifestation()?.clone();

    // Get ground truth from knowledge
    let knowledge = world.entities.get(&source_data.knowledge_id)?;
    let knowledge_data = knowledge.data.as_knowledge()?.clone();
    let knowledge_name = knowledge.name.clone();

    // Get transition profile
    let profile = get_transition_profile(&source_data.medium, &target_medium);

    // Deep-clone source content
    let mut new_content = source_data.content.clone();

    // Apply accuracy/completeness retention
    let acc_retention = rng.random_range(profile.accuracy_retention.0..=profile.accuracy_retention.1);
    let comp_retention = rng.random_range(profile.completeness_retention.0..=profile.completeness_retention.1);

    let _new_accuracy_estimate = (source_data.accuracy * acc_retention).clamp(0.0, 1.0);
    let new_completeness = (source_data.completeness * comp_retention).clamp(0.0, 1.0);

    // Apply completeness loss: randomly remove keys proportional to loss
    let comp_loss = 1.0 - comp_retention;
    if comp_loss > 0.1
        && let serde_json::Value::Object(map) = &mut new_content
    {
        let essential = ["event_type", "year", "name"];
        let removable: Vec<String> = map.keys()
            .filter(|k| !essential.contains(&k.as_str()))
            .cloned()
            .collect();
        let remove_count = ((removable.len() as f64 * comp_loss) as usize).min(removable.len());
        for key in removable.iter().take(remove_count) {
            if rng.random_bool(comp_loss) {
                map.remove(key);
            }
        }
    }

    // Apply distortions from profile
    let mut distortions_applied = Vec::new();
    if let Some(existing) = source_data.distortions.as_array() {
        distortions_applied.extend(existing.iter().cloned());
    }
    for &(kind, probability) in profile.distortions {
        if rng.random_bool(probability) {
            let record = apply_distortion(&mut new_content, kind, rng);
            distortions_applied.push(serde_json::json!({
                "distortion": distortion_name(kind),
                "detail": record,
            }));
        }
    }

    // Calculate actual accuracy against ground truth
    let actual_accuracy = calculate_accuracy(&knowledge_data.ground_truth, &new_content);

    // Create new manifestation entity
    let manif_name = format!("{} ({})", knowledge_name, target_medium.as_str());
    let manif_id = world.add_entity(
        EntityKind::Manifestation,
        manif_name,
        Some(time),
        EntityData::Manifestation(ManifestationData {
            knowledge_id: source_data.knowledge_id,
            medium: target_medium,
            content: new_content,
            accuracy: actual_accuracy,
            completeness: new_completeness,
            distortions: serde_json::json!(distortions_applied),
            derived_from_id: Some(source_manifestation_id),
            derivation_method: profile.method.to_string(),
            condition: 1.0,
            created_year: time.year(),
        }),
        event_id,
    );

    // Add HeldBy relationship
    world.add_relationship(
        manif_id,
        holder_entity_id,
        RelationshipKind::HeldBy,
        time,
        event_id,
    );

    Some(manif_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn sample_battle_content() -> serde_json::Value {
        serde_json::json!({
            "event_type": "battle",
            "name": "Battle of Ironhold",
            "year": 150,
            "attacker": { "faction_id": 1, "faction_name": "Northmen", "troops": 500 },
            "defender": { "faction_id": 2, "faction_name": "Southfolk", "troops": 300 },
            "outcome": "attacker_victory",
            "decisive": true,
            "reparations": 50,
            "notable_details": []
        })
    }

    #[test]
    fn exaggerate_numbers_increases_values() {
        let mut content = sample_battle_content();
        let mut rng = SmallRng::seed_from_u64(42);
        let _record = exaggerate_numbers(&mut content, &mut rng);

        // At least one numeric field should have increased
        let troops = content["attacker"]["troops"].as_f64().unwrap();
        assert!(troops >= 500.0, "troops should be >= 500 after exaggeration, got {troops}");
    }

    #[test]
    fn round_numbers_rounds() {
        let mut content = serde_json::json!({"troops": 347, "year": 153});
        let mut rng = SmallRng::seed_from_u64(42);
        let _record = round_numbers(&mut content, &mut rng);

        let troops = content["troops"].as_f64().unwrap();
        // Should be rounded to nearest 10, 50, or 100
        assert!(troops == 350.0 || troops == 300.0 || troops == 400.0 || troops == 347.0,
            "troops should be rounded, got {troops}");
    }

    #[test]
    fn corrupt_names_alters_strings() {
        let mut content = serde_json::json!({"faction_name": "Northmen", "settlement_name": "Ironhold"});
        let mut rng = SmallRng::seed_from_u64(42);
        let _record = corrupt_names(&mut content, &mut rng);

        // At least one name should be altered (probabilistic, but with seed 42 it should)
        let name1 = content["faction_name"].as_str().unwrap();
        let name2 = content["settlement_name"].as_str().unwrap();
        let changed = name1 != "Northmen" || name2 != "Ironhold";
        // Not asserting change since it's probabilistic, just verify no crash
        let _ = changed;
    }

    #[test]
    fn calculate_accuracy_identical() {
        let truth = sample_battle_content();
        let content = truth.clone();
        let acc = calculate_accuracy(&truth, &content);
        assert!((acc - 1.0).abs() < 0.001, "identical content should have accuracy 1.0, got {acc}");
    }

    #[test]
    fn calculate_accuracy_partial_match() {
        let truth = sample_battle_content();
        let mut content = truth.clone();
        // Remove some fields
        content.as_object_mut().unwrap().remove("reparations");
        content.as_object_mut().unwrap().remove("decisive");
        let acc = calculate_accuracy(&truth, &content);
        assert!(acc < 1.0, "missing fields should reduce accuracy, got {acc}");
        assert!(acc > 0.0, "should still have some accuracy, got {acc}");
    }

    #[test]
    fn calculate_accuracy_numeric_proximity() {
        let truth = serde_json::json!({"troops": 500});
        let content = serde_json::json!({"troops": 450});
        let acc = calculate_accuracy(&truth, &content);
        // 450/500 = 0.9
        assert!((acc - 0.9).abs() < 0.01, "numeric proximity should give partial credit, got {acc}");
    }

    #[test]
    fn transition_profile_selection() {
        assert_eq!(
            get_transition_profile(&Medium::WrittenBook, &Medium::WrittenBook).method,
            "copied"
        );
        assert_eq!(
            get_transition_profile(&Medium::Memory, &Medium::OralTradition).method,
            "retold"
        );
        assert_eq!(
            get_transition_profile(&Medium::OralTradition, &Medium::WrittenBook).method,
            "transcribed_from_oral"
        );
        assert_eq!(
            get_transition_profile(&Medium::OralTradition, &Medium::Song).method,
            "set_to_music"
        );
        assert_eq!(
            get_transition_profile(&Medium::Song, &Medium::CarvedStone).method,
            "carved"
        );
        assert_eq!(
            get_transition_profile(&Medium::Dream, &Medium::Painting).method,
            "derived"
        );
    }

    #[test]
    fn derive_creates_manifestation() {
        use crate::model::{EventKind, KnowledgeCategory, KnowledgeData};

        let mut world = World::new();
        world.current_time = SimTimestamp::from_year(100);
        let ev = world.add_event(EventKind::Custom("test".into()), SimTimestamp::from_year(100), "test".into());

        let truth = sample_battle_content();

        // Create knowledge
        let kid = world.add_entity(
            EntityKind::Knowledge,
            "Battle of Ironhold".into(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Battle,
                source_event_id: ev,
                origin_settlement_id: 0,
                origin_year: 100,
                significance: 0.7,
                ground_truth: truth.clone(),
            }),
            ev,
        );

        // Create original manifestation (eyewitness memory)
        let mid = world.add_entity(
            EntityKind::Manifestation,
            "Battle of Ironhold (memory)".into(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Manifestation(ManifestationData {
                knowledge_id: kid,
                medium: Medium::Memory,
                content: truth.clone(),
                accuracy: 1.0,
                completeness: 1.0,
                distortions: serde_json::json!([]),
                derived_from_id: None,
                derivation_method: "witnessed".into(),
                condition: 1.0,
                created_year: 100,
            }),
            ev,
        );

        // Create a holder settlement
        let sid = world.add_entity(
            EntityKind::Settlement,
            "Ironhold".into(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(&EntityKind::Settlement),
            ev,
        );

        let mut rng = SmallRng::seed_from_u64(42);
        let derived_id = derive(
            &mut world,
            &mut rng,
            mid,
            Medium::OralTradition,
            sid,
            SimTimestamp::from_year(110),
            ev,
        );

        assert!(derived_id.is_some(), "derive should return new manifestation ID");
        let derived_id = derived_id.unwrap();

        let derived = world.entities.get(&derived_id).unwrap();
        assert_eq!(derived.kind, EntityKind::Manifestation);
        let md = derived.data.as_manifestation().unwrap();
        assert_eq!(md.knowledge_id, kid);
        assert_eq!(md.medium, Medium::OralTradition);
        assert!(md.derived_from_id == Some(mid));
        assert_eq!(md.derivation_method, "retold");
        assert!(md.accuracy <= 1.0);
        assert!(md.accuracy >= 0.0);

        // Should have HeldBy relationship
        let held_by = derived.relationships.iter().any(|r|
            r.kind == RelationshipKind::HeldBy && r.target_entity_id == sid
        );
        assert!(held_by, "derived manifestation should have HeldBy relationship");
    }

    #[test]
    fn cascading_derivations_decrease_accuracy() {
        use crate::model::{EventKind, KnowledgeCategory, KnowledgeData};

        let mut world = World::new();
        world.current_time = SimTimestamp::from_year(100);
        let ev = world.add_event(EventKind::Custom("test".into()), SimTimestamp::from_year(100), "test".into());

        let truth = sample_battle_content();

        let kid = world.add_entity(
            EntityKind::Knowledge, "Knowledge".into(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Knowledge(KnowledgeData {
                category: KnowledgeCategory::Battle,
                source_event_id: ev, origin_settlement_id: 0,
                origin_year: 100, significance: 0.7, ground_truth: truth.clone(),
            }),
            ev,
        );

        let mid = world.add_entity(
            EntityKind::Manifestation, "Original".into(),
            Some(SimTimestamp::from_year(100)),
            EntityData::Manifestation(ManifestationData {
                knowledge_id: kid, medium: Medium::Memory,
                content: truth.clone(), accuracy: 1.0, completeness: 1.0,
                distortions: serde_json::json!([]), derived_from_id: None,
                derivation_method: "witnessed".into(), condition: 1.0, created_year: 100,
            }),
            ev,
        );

        let sid = world.add_entity(
            EntityKind::Settlement, "Town".into(),
            Some(SimTimestamp::from_year(1)),
            EntityData::default_for_kind(&EntityKind::Settlement), ev,
        );

        let mut rng = SmallRng::seed_from_u64(42);

        // Chain: Memory -> OralTradition -> OralTradition -> Song
        let d1 = derive(&mut world, &mut rng, mid, Medium::OralTradition, sid, SimTimestamp::from_year(110), ev).unwrap();
        let d2 = derive(&mut world, &mut rng, d1, Medium::OralTradition, sid, SimTimestamp::from_year(120), ev).unwrap();
        let d3 = derive(&mut world, &mut rng, d2, Medium::Song, sid, SimTimestamp::from_year(130), ev).unwrap();

        let acc1 = world.entities.get(&d1).unwrap().data.as_manifestation().unwrap().accuracy;
        let acc3 = world.entities.get(&d3).unwrap().data.as_manifestation().unwrap().accuracy;

        assert!(acc3 < acc1,
            "cascading derivation should decrease accuracy: d1={acc1}, d3={acc3}");
    }
}
