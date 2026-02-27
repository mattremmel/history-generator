pub mod adjacency;
pub mod graph;
pub mod structural;

pub use adjacency::RegionAdjacency;
pub use graph::{RelationshipGraph, RelationshipMeta, TradeRouteData};
pub use structural::{
    Exploits, ExploitsSources, FlowsThrough, FlowsThroughSources, HeldBy, HeldBySources, HiredBy,
    HiredBySources, LeaderOf, LeaderOfSources, LocatedIn, LocatedInSources, MemberOf,
    MemberOfSources,
};
