# Simulation Gaps Analysis

Review of current systems (Phases 1–5.5) against project goals for emergent, realistic historical behavior.

## Current System Status

| System | Status | Emergent Potential |
|--------|--------|-------------------|
| Core Data Model | Complete | Causal event chains, relationships, signals |
| WorldGen | Complete | Static geography with terrain, rivers, resources, adjacency |
| Demographics | Complete | Population brackets, NPC births with traits/roles, prosperity |
| Politics | Complete | Succession, coups, diplomacy, happiness/legitimacy/stability, faction splits |
| Conflicts | Complete | Wars, armies, supply/attrition, battles, retreats, conquest |
| Agency | Complete | Signal reactions, trait-modulated probability, defection, elections |
| Economy | Complete | Resource production, multi-hop trade routes, treasuries, alliance strength |

## Critical Gaps

### ~~1. Economy is completely absent~~ ✅ Resolved

Resource production from population and regional deposits, multi-hop BFS trade routes (up to 6 hops) with river bonuses, faction treasuries from taxation, prosperity derived from economic output. Trade creates happiness bonuses and alliances. Resource scarcity and wealth inequality drive war motivation. Alliance strength accumulates from multiple sources (trade, shared enemies, marriages) and modulates decay.

### ~~2. No family/genealogy tracking~~ ✅ Resolved

NPCs now have parent-child relationships at birth, marriages (intra-settlement and cross-faction), patrilineal surname inheritance creating visible dynasties, and bloodline-based hereditary succession (children → siblings → oldest).

### ~~3. Agency system is a skeleton~~ ✅ Resolved

NPCs now drive history through personality-weighted decisions with signal reactions, defection, and elections.

### ~~4. No migration or refugees~~ ✅ Resolved

Conquest flight, economic emigration, and NPC relocation. Refugees flee conquered/ruined settlements, creating demographic pressure and cultural mixing in neighboring settlements. RefugeesArrived signals propagate consequences across systems.

### 5. No disease/plague mechanics

Only background mortality rates exist. No epidemics or pandemics.

- Plagues reshape demographics, economics, and politics for centuries
- Disease could spread along trade routes (which now exist)
- Armies are historically devastated more by disease than battle
- **Impact**: Missing one of history's most powerful disruptors

## Significant Gaps

### 6. Diplomacy is coin-flip, not motivated

Alliances form at 0.8% × happiness. Rivalries at 0.6% × instability. No:

- ~~Reason *why* two factions ally beyond shared-enemy multiplier~~ ✅ trade routes and alliance strength system
- ~~Trade agreements~~ ✅ trade routes between factions, defensive pacts, non-aggression treaties
- ~~Marriage alliances (needs genealogy)~~ ✅ cross-faction marriages now create/strengthen alliances
- Betrayal of alliances for gain
- Tribute/vassalage relationships

### 7. War motivations are shallow

Wars only trigger from existing Enemy relationships + adjacency. No wars over:

- ~~Resources (no economy)~~ ✅ resource scarcity and wealth inequality now drive economic_war_motivation
- Succession claims (genealogy exists but no contested claims yet)
- Religious differences (no religion system)
- Revenge for past wrongs (no faction memory of grievances)
- Territorial ambition beyond "attack neighbor"

### 8. No siege mechanics or fortifications

Conquest is instant when an army reaches an undefended settlement. No:

- Walls, defenses, or garrison forces
- Siege duration or supply requirements
- Civilian casualties from siege
- Fortification building as a political/economic decision

### 9. Population breakdown disconnected from individuals

Settlements track aggregate brackets while also spawning named NPCs, but:

- Named NPCs aren't part of bracket counts (parallel systems)
- Only 2–4 NPCs born per settlement per year — tiny fraction of population
- Most people are statistical, not individuals
- Architecturally fine, but NPC-driven events only affect a thin slice

### ~~10. No cultural identity or drift~~ ✅ Resolved

Cultures are first-class entities with values (Martial, Mercantile, Scholarly, etc.), 6 distinct naming styles, and resistance ratings. Settlements track culture_makeup with drift toward ruling culture, blending of coexisting cultures, and cultural tension triggering rebellions. Demographics uses culture-aware naming. Politics penalizes happiness/stability for cultural tension.

## Smaller Gaps

| Gap | Why It Matters |
|-----|---------------|
| No buildings/infrastructure | Can't model walls, temples, markets — no settlement investment |
| No literacy/education | No basis for knowledge system (Phase 6) |
| No seasonal effects | Harvests, winter campaigns, famines don't exist |
| No naval capability | Coastal factions can't project power across water |
| No mercenaries | Small factions can't punch above their weight |
| No crime/banditry | No internal security challenges |
| No natural disasters | No famines, floods, earthquakes disrupting systems |
| No reputation/prestige | Factions and NPCs have no "fame" that influences others |
| ~~No war goals/peace terms~~ ✅ | Wars have structured goals, peace terms with reparations, and tribute systems |

## What's Working Well

- **Signal system** — leader dies → vacancy → succession → instability → coup → war cascades
- **Causal event chains** — `caused_by` on events enables narrative tracing
- **Monthly conflict resolution** — supply, morale, and movement create realistic campaign arcs
- **Terrain-dependent attrition** — mountain campaigns are grueling, swamps deadly, deserts impassable
- **Faction splits from misery** — low stability + low happiness → breakaway factions → new conflicts
- **Trait-weighted decisions** — Aggressive leaders start wars more often, Ambitious NPCs coup more

## Recommended Priority Order

Based on cascading impact per unit of effort:

1. ~~**Wire up Agency** — NPCs acting on desires makes history personal. An ambitious NPC assassinating a leader is more compelling than "a coup happened."~~ ✅ Done — signal reactions, trait-modulated actions, defection, elections
2. ~~**Add family/genealogy** — Parent-child + spouse relationships. Dynastic succession, inheritance, marriage alliances. Unlocks blood feuds and succession crises.~~ ✅ Done — parent-child rels, marriages, surname dynasties, bloodline succession, marriage alliances
3. ~~**Basic economy** — Resource production, trade along adjacency, wealth as a faction property. Makes war motivations real and prosperity meaningful.~~ ✅ Done — multi-hop trade routes, treasuries, economic prosperity, alliance strength, war motivation from resource scarcity
4. ~~**Migration/refugees** — Population movement from war, famine, conquest. Cultural mixing, demographic pressure, cascading consequences.~~ ✅ Done — conquest flight, economic emigration, NPC relocation, refugee signals
5. ~~**War goals and peace terms** — Purposeful wars and meaningful (breakable) peace treaties.~~ ✅ Done — structured war goals, peace terms with reparations, tribute system
6. ~~**Cultural identity** — Traits on factions that drift, blend, and create tension under foreign rule. Enables rebellions and cultural resistance.~~ ✅ Done — culture entities, 6 naming styles, cultural drift/blending/tension, rebellions, cross-system integration
