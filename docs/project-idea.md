Phase 1: Core Data Model & Skeleton
Get the foundation compiling and running before any simulation logic.
Define your core structs: Entity, Relationship, Event, EventParticipant. Keep them flat with integer IDs. Implement serde::Serialize on everything from day one. Build your World struct that holds the BTreeMap and HashMap collections. Write the JSONL flush function. Write a trivial loader script that reads JSONL into Postgres. Verify the round trip: create a handful of hardcoded entities in Rust, flush them, load them, query them in Postgres.
You want this pipeline proven before you write a single line of simulation logic.
Phase 2: Geography & Settlements
Generate a static world map — regions, terrain types, coordinates. Spawn initial settlements with populations, resources, and terrain associations. This is your spatial foundation. No simulation ticking yet, just world initialization. Flush, load, verify you can query “settlements near X” in Postgres.
Phase 3: The Tick Loop & Demographics
Implement the year-by-year loop. Start with just demographics: population growth and decline, birth and death of notable NPCs, aging. This forces you to solve the core architectural questions early — how entities are created mid-tick, how events reference participants, how you handle ID generation, how the flush checkpointing works. Run a thousand years. Look at the output. Make sure it feels right before adding complexity.
Phase 4: Relationships & Politics
Add factions, organizations, and governance. Leaders rule settlements. Relationships have sentiment and trust. Succession on death. This is where the relationship graph becomes real and you’ll stress-test your adjacency lookups. Keep politics simple at first — just who rules what and what happens when they die.
Phase 5: Conflicts
Wars, raids, battles. Driven by relationship tension and resource competition. Conflicts should produce cascading consequences: territory changes, refugees, deaths of notable NPCs, destruction of settlements. This is the first system that heavily exercises cross-system interaction. A war touches demographics, politics, relationships, and geography all at once.
Phase 6: Knowledge & Manifestations
Implement the ground-truth knowledge, manifestation, and derivation chain. This is the most structurally complex system. Get the lineage tracking and accuracy degradation working. Wire it into events so that battles produce knowledge, knowledge propagates along trade routes, and oral traditions degrade. This is where you’ll appreciate having native Rust structs — the derivation engine does a lot of conditional logic that would be painful in SQL.
Phase 7: Items & Burials
Add the item system with resonance accumulation and tier promotion. Add the burial system so deaths produce graves with goods, markers, and epitaphs. Wire burials into knowledge manifestations so epitaphs are queryable text that can be wrong. Wire items into events so provenance chains build naturally.
Phase 8: Culture, Religion & Language
Layer on cultural traits, drift, and blending. Religious systems with deities, worship, schisms, and prophecy. Language phonology and name generation. These systems are more about texture than mechanical consequence, so they can come later without blocking anything.
Phase 9: Monsters, Ruins & Environment
Monster ecology and territory. Settlement-to-ruin lifecycle. Natural disasters. These are the systems that produce adventure sites and threats. They depend on most prior systems being in place since a ruin’s contents come from items, knowledge, burials, and architectural history.
Phase 10: Atmosphere & Query Layer
Build the Postgres query layer for session prep: the settlement snapshot queries, quest hook generators, NPC finders, contradiction detectors. Build the everyday life generation — cuisine, superstitions, soundscapes, slang — derived from simulation state. This is where the investment pays off.
Strategy Principles
Get the pipeline working end-to-end in Phase 1. Resist the urge to build simulation logic before you can see output in Postgres. The feedback loop of generate → flush → load → query is how you’ll catch design mistakes early.
Add one system at a time and run a full thousand-year generation after each. Read the output. Look for nonsensical results. Tune probabilities. The simulation will surprise you — things will happen too often or never. You need to see the output to calibrate.
Keep your tick function as a simple dispatch. Each system gets its own tick method. The main loop just calls them in order. This lets you enable and disable systems during development and isolate bugs.
Design your structs for cache locality. Entities that are frequently accessed together (a settlement and its population, a person and their relationships) should be reachable without pointer chasing. Consider storing relationships inline as Vec<Relationship> on the entity rather than in a separate collection, then normalizing to the relational model only at flush time.
Flush every N years, not every year. Flushing every 50 simulated years is a reasonable starting point. Keep a generation counter and flush when it rolls over. This gives you natural checkpoints without constant IO.
Don’t optimize early. A thousand years of simulation with a few hundred thousand entities will run in seconds in Rust even with naive implementations. Profile only if it’s actually slow. The bottleneck will almost certainly be the Postgres load, not the simulation.
