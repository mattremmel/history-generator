//! Pure helper functions for reading and mutating grievances on factions and NPCs.
//!
//! No `SimSystem` — these are called from politics, conflicts, and agency.

use crate::model::World;
use crate::model::grievance::Grievance;
use crate::model::timestamp::SimTimestamp;
use crate::model::traits::Trait;

/// Maximum number of source tags stored per grievance entry.
const MAX_SOURCES: usize = 5;

/// Read the grievance severity that `holder` has against `target` (0.0 if none).
pub fn get_grievance(world: &World, holder: u64, target: u64) -> f64 {
    let Some(entity) = world.entities.get(&holder) else {
        return 0.0;
    };
    if let Some(fd) = entity.data.as_faction() {
        return fd
            .grievances
            .get(&target)
            .map(|g| g.severity)
            .unwrap_or(0.0);
    }
    if let Some(pd) = entity.data.as_person() {
        return pd
            .grievances
            .get(&target)
            .map(|g| g.severity)
            .unwrap_or(0.0);
    }
    0.0
}

/// Add or accumulate a grievance. Caps severity at 1.0 and sources at [`MAX_SOURCES`].
pub fn add_grievance(
    world: &mut World,
    holder: u64,
    target: u64,
    delta: f64,
    source: &str,
    time: SimTimestamp,
    event_id: u64,
) {
    // Try faction first, then person.
    let entity = world.entities.get_mut(&holder);
    let Some(entity) = entity else { return };

    let grievances = if let Some(fd) = entity.data.as_faction_mut() {
        &mut fd.grievances
    } else if let Some(pd) = entity.data.as_person_mut() {
        &mut pd.grievances
    } else {
        return;
    };

    let entry = grievances.entry(target).or_insert(Grievance {
        severity: 0.0,
        sources: Vec::new(),
        peak: 0.0,
        updated: time,
    });

    entry.severity = (entry.severity + delta).min(1.0);
    entry.updated = time;
    if entry.severity > entry.peak {
        entry.peak = entry.severity;
    }
    let tag = source.to_string();
    if !entry.sources.contains(&tag) {
        if entry.sources.len() >= MAX_SOURCES {
            entry.sources.remove(0);
        }
        entry.sources.push(tag);
    }
    let new_severity = entry.severity;

    // Record change for audit trail
    world.record_change(
        holder,
        event_id,
        "grievance",
        serde_json::json!(target),
        serde_json::json!(new_severity),
    );
}

/// Reduce a grievance by `delta`. Removes the entry entirely if severity drops below `threshold`.
pub fn reduce_grievance(world: &mut World, holder: u64, target: u64, delta: f64, threshold: f64) {
    let entity = world.entities.get_mut(&holder);
    let Some(entity) = entity else { return };

    let grievances = if let Some(fd) = entity.data.as_faction_mut() {
        &mut fd.grievances
    } else if let Some(pd) = entity.data.as_person_mut() {
        &mut pd.grievances
    } else {
        return;
    };

    if let Some(g) = grievances.get_mut(&target) {
        g.severity = (g.severity - delta).max(0.0);
        if g.severity < threshold {
            grievances.remove(&target);
        }
    }
}

/// Remove the entire grievance entry against a target (e.g. faction destroyed).
#[allow(dead_code)]
pub fn remove_grievance(world: &mut World, holder: u64, target: u64) {
    let entity = world.entities.get_mut(&holder);
    let Some(entity) = entity else { return };

    if let Some(fd) = entity.data.as_faction_mut() {
        fd.grievances.remove(&target);
    } else if let Some(pd) = entity.data.as_person_mut() {
        pd.grievances.remove(&target);
    }
}

/// Trait-based decay rate multiplier for NPC grievances.
///
/// Grudge-holders (Ruthless, Aggressive) forget slowly; forgiving types (Content, Honorable) faster.
/// Multiplicative stacking, default 1.0.
pub fn trait_decay_multiplier(traits: &[Trait]) -> f64 {
    let mut mult = 1.0;
    for t in traits {
        let m = match t {
            Trait::Ruthless => 0.5,
            Trait::Aggressive => 0.7,
            Trait::Content => 1.5,
            Trait::Honorable => 1.3,
            _ => 1.0,
        };
        mult *= m;
    }
    mult
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::timestamp::SimTimestamp;
    use crate::scenario::Scenario;

    #[test]
    fn get_grievance_returns_zero_when_none() {
        let s = Scenario::at_year(100);
        let world = s.build();
        // No entities, returns 0
        assert!((get_grievance(&world, 999, 888)).abs() < f64::EPSILON);
    }

    #[test]
    fn add_and_get_grievance_faction() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();
        let ev = world.add_event(
            crate::model::EventKind::Custom("test".into()),
            crate::model::SimTimestamp::from_year(100),
            "test".into(),
        );

        let ts = SimTimestamp::from_year(100);
        add_grievance(&mut world, a.faction, b.faction, 0.40, "conquest", ts, ev);
        let sev = get_grievance(&world, a.faction, b.faction);
        assert!((sev - 0.40).abs() < f64::EPSILON);
    }

    #[test]
    fn add_grievance_accumulates_and_caps() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();
        let ev = world.add_event(
            crate::model::EventKind::Custom("test".into()),
            crate::model::SimTimestamp::from_year(100),
            "test".into(),
        );

        let ts = SimTimestamp::from_year(100);
        add_grievance(&mut world, a.faction, b.faction, 0.60, "conquest", ts, ev);
        add_grievance(
            &mut world,
            a.faction,
            b.faction,
            0.60,
            "betrayal",
            SimTimestamp::from_year(101),
            ev,
        );
        let sev = get_grievance(&world, a.faction, b.faction);
        assert!((sev - 1.0).abs() < f64::EPSILON, "should cap at 1.0");

        // Sources should have both tags
        let fd = world.faction(a.faction);
        let g = fd.grievances.get(&b.faction).unwrap();
        assert!(g.sources.contains(&"conquest".to_string()));
        assert!(g.sources.contains(&"betrayal".to_string()));
        assert!((g.peak - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reduce_grievance_removes_below_threshold() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();
        let ev = world.add_event(
            crate::model::EventKind::Custom("test".into()),
            crate::model::SimTimestamp::from_year(100),
            "test".into(),
        );

        let ts = SimTimestamp::from_year(100);
        add_grievance(&mut world, a.faction, b.faction, 0.20, "raid", ts, ev);
        reduce_grievance(&mut world, a.faction, b.faction, 0.10, 0.05);
        let sev = get_grievance(&world, a.faction, b.faction);
        assert!((sev - 0.10).abs() < 1e-10);

        // Reduce below threshold → removed
        reduce_grievance(&mut world, a.faction, b.faction, 0.08, 0.05);
        let sev2 = get_grievance(&world, a.faction, b.faction);
        assert!(sev2 < 0.05, "should be below threshold, got {sev2}");
        assert!(
            world
                .faction(a.faction)
                .grievances
                .get(&b.faction)
                .is_none()
        );
    }

    #[test]
    fn add_grievance_person() {
        let mut s = Scenario::at_year(100);
        let k = s.add_kingdom("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();
        let ev = world.add_event(
            crate::model::EventKind::Custom("test".into()),
            crate::model::SimTimestamp::from_year(100),
            "test".into(),
        );

        let ts = SimTimestamp::from_year(100);
        add_grievance(
            &mut world,
            k.leader,
            b.faction,
            0.45,
            "family_killed",
            ts,
            ev,
        );
        let sev = get_grievance(&world, k.leader, b.faction);
        assert!((sev - 0.45).abs() < f64::EPSILON);
    }

    #[test]
    fn trait_decay_multiplier_stacks() {
        // Ruthless + Aggressive = 0.5 * 0.7 = 0.35
        let m = trait_decay_multiplier(&[Trait::Ruthless, Trait::Aggressive]);
        assert!((m - 0.35).abs() < 1e-10);

        // Content + Honorable = 1.5 * 1.3 = 1.95
        let m2 = trait_decay_multiplier(&[Trait::Content, Trait::Honorable]);
        assert!((m2 - 1.95).abs() < 1e-10);

        // No relevant traits = 1.0
        let m3 = trait_decay_multiplier(&[Trait::Charismatic]);
        assert!((m3 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn sources_cap_at_max() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();
        let ev = world.add_event(
            crate::model::EventKind::Custom("test".into()),
            crate::model::SimTimestamp::from_year(100),
            "test".into(),
        );

        let ts = SimTimestamp::from_year(100);
        for i in 0..7 {
            add_grievance(
                &mut world,
                a.faction,
                b.faction,
                0.05,
                &format!("source_{i}"),
                ts,
                ev,
            );
        }
        let g = world.faction(a.faction).grievances.get(&b.faction).unwrap();
        assert_eq!(g.sources.len(), 5);
        // Oldest sources should have been dropped
        assert!(!g.sources.contains(&"source_0".to_string()));
        assert!(!g.sources.contains(&"source_1".to_string()));
        assert!(g.sources.contains(&"source_6".to_string()));
    }

    #[test]
    fn remove_grievance_clears_entry() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();
        let ev = world.add_event(
            crate::model::EventKind::Custom("test".into()),
            crate::model::SimTimestamp::from_year(100),
            "test".into(),
        );

        let ts = SimTimestamp::from_year(100);
        add_grievance(&mut world, a.faction, b.faction, 0.80, "conquest", ts, ev);
        remove_grievance(&mut world, a.faction, b.faction);
        assert!(get_grievance(&world, a.faction, b.faction).abs() < f64::EPSILON);
    }
}
