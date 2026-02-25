//! Pure helper functions for reading and mutating loyalty on factions and persons.
//!
//! No `SimSystem` — these are called from the mercenary system (and later from
//! vassal, NPC, and inter-faction loyalty systems).
//!
//! Mirrors the `grievance.rs` pattern: works on both FactionData and PersonData
//! via dual-dispatch.

use crate::model::World;

/// Default loyalty score when no entry exists.
const DEFAULT_LOYALTY: f64 = 0.5;

/// Read the loyalty that `holder` has toward `target` (default 0.5 if unset).
pub fn get_loyalty(world: &World, holder: u64, target: u64) -> f64 {
    let Some(entity) = world.entities.get(&holder) else {
        return DEFAULT_LOYALTY;
    };
    if let Some(fd) = entity.data.as_faction() {
        return fd
            .loyalty
            .get(&target)
            .copied()
            .unwrap_or(DEFAULT_LOYALTY);
    }
    if let Some(pd) = entity.data.as_person() {
        return pd
            .loyalty
            .get(&target)
            .copied()
            .unwrap_or(DEFAULT_LOYALTY);
    }
    DEFAULT_LOYALTY
}

/// Set the loyalty that `holder` has toward `target`, clamped to 0.0-1.0.
pub fn set_loyalty(world: &mut World, holder: u64, target: u64, value: f64) {
    let clamped = value.clamp(0.0, 1.0);
    let Some(entity) = world.entities.get_mut(&holder) else {
        return;
    };
    if let Some(fd) = entity.data.as_faction_mut() {
        fd.loyalty.insert(target, clamped);
    } else if let Some(pd) = entity.data.as_person_mut() {
        pd.loyalty.insert(target, clamped);
    }
}

/// Adjust loyalty by `delta`, clamped to 0.0-1.0.
pub fn adjust_loyalty(world: &mut World, holder: u64, target: u64, delta: f64) {
    let current = get_loyalty(world, holder, target);
    set_loyalty(world, holder, target, current + delta);
}

/// Check if loyalty is below a threshold.
#[allow(dead_code)]
pub fn loyalty_below(world: &World, holder: u64, target: u64, threshold: f64) -> bool {
    get_loyalty(world, holder, target) < threshold
}

/// Remove the loyalty entry for `target` from `holder`.
pub fn remove_loyalty(world: &mut World, holder: u64, target: u64) {
    let Some(entity) = world.entities.get_mut(&holder) else {
        return;
    };
    if let Some(fd) = entity.data.as_faction_mut() {
        fd.loyalty.remove(&target);
    } else if let Some(pd) = entity.data.as_person_mut() {
        pd.loyalty.remove(&target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;

    #[test]
    fn default_loyalty_is_half() {
        let s = Scenario::at_year(100);
        let world = s.build();
        assert!((get_loyalty(&world, 999, 888) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn set_and_get_loyalty_faction() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();

        set_loyalty(&mut world, a.faction, b.faction, 0.8);
        let loy = get_loyalty(&world, a.faction, b.faction);
        assert!((loy - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn set_loyalty_clamps() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();

        set_loyalty(&mut world, a.faction, b.faction, 1.5);
        assert!((get_loyalty(&world, a.faction, b.faction) - 1.0).abs() < f64::EPSILON);

        set_loyalty(&mut world, a.faction, b.faction, -0.3);
        assert!(get_loyalty(&world, a.faction, b.faction).abs() < f64::EPSILON);
    }

    #[test]
    fn adjust_loyalty_accumulates() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();

        set_loyalty(&mut world, a.faction, b.faction, 0.5);
        adjust_loyalty(&mut world, a.faction, b.faction, 0.2);
        assert!((get_loyalty(&world, a.faction, b.faction) - 0.7).abs() < f64::EPSILON);

        adjust_loyalty(&mut world, a.faction, b.faction, -0.4);
        assert!((get_loyalty(&world, a.faction, b.faction) - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn loyalty_below_threshold() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();

        set_loyalty(&mut world, a.faction, b.faction, 0.2);
        assert!(loyalty_below(&world, a.faction, b.faction, 0.3));
        assert!(!loyalty_below(&world, a.faction, b.faction, 0.1));
    }

    #[test]
    fn loyalty_on_person() {
        let mut s = Scenario::at_year(100);
        let k = s.add_kingdom("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();

        set_loyalty(&mut world, k.leader, b.faction, 0.9);
        assert!((get_loyalty(&world, k.leader, b.faction) - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn remove_loyalty_clears_entry() {
        let mut s = Scenario::at_year(100);
        let a = s.add_settlement_standalone("A");
        let b = s.add_settlement_standalone("B");
        let mut world = s.build();

        set_loyalty(&mut world, a.faction, b.faction, 0.8);
        remove_loyalty(&mut world, a.faction, b.faction);
        // Should return default (0.5)
        assert!((get_loyalty(&world, a.faction, b.faction) - 0.5).abs() < f64::EPSILON);
    }
}
