use std::ops::Deref;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;

// ---------------------------------------------------------------------------
// LocatedIn — settlement/army → region, building → settlement
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug)]
#[relationship(relationship_target = LocatedInSources)]
pub struct LocatedIn(pub Entity);

#[derive(Component, Default, Debug)]
#[relationship_target(relationship = LocatedIn)]
pub struct LocatedInSources(Vec<Entity>);

impl Deref for LocatedInSources {
    type Target = [Entity];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// MemberOf — person → faction, settlement → faction
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug)]
#[relationship(relationship_target = MemberOfSources)]
pub struct MemberOf(pub Entity);

#[derive(Component, Default, Debug)]
#[relationship_target(relationship = MemberOf)]
pub struct MemberOfSources(Vec<Entity>);

impl Deref for MemberOfSources {
    type Target = [Entity];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// LeaderOf — person → faction
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug)]
#[relationship(relationship_target = LeaderOfSources)]
pub struct LeaderOf(pub Entity);

#[derive(Component, Default, Debug)]
#[relationship_target(relationship = LeaderOf)]
pub struct LeaderOfSources(Vec<Entity>);

impl Deref for LeaderOfSources {
    type Target = [Entity];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// HeldBy — item → person
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug)]
#[relationship(relationship_target = HeldBySources)]
pub struct HeldBy(pub Entity);

#[derive(Component, Default, Debug)]
#[relationship_target(relationship = HeldBy)]
pub struct HeldBySources(Vec<Entity>);

impl Deref for HeldBySources {
    type Target = [Entity];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// HiredBy — mercenary faction → employer faction
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug)]
#[relationship(relationship_target = HiredBySources)]
pub struct HiredBy(pub Entity);

#[derive(Component, Default, Debug)]
#[relationship_target(relationship = HiredBy)]
pub struct HiredBySources(Vec<Entity>);

impl Deref for HiredBySources {
    type Target = [Entity];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// FlowsThrough — river → region
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug)]
#[relationship(relationship_target = FlowsThroughSources)]
pub struct FlowsThrough(pub Entity);

#[derive(Component, Default, Debug)]
#[relationship_target(relationship = FlowsThrough)]
pub struct FlowsThroughSources(Vec<Entity>);

impl Deref for FlowsThroughSources {
    type Target = [Entity];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Exploits — resource deposit → region
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug)]
#[relationship(relationship_target = ExploitsSources)]
pub struct Exploits(pub Entity);

#[derive(Component, Default, Debug)]
#[relationship_target(relationship = Exploits)]
pub struct ExploitsSources(Vec<Entity>);

impl Deref for ExploitsSources {
    type Target = [Entity];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
