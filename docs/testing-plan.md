# Testing Plan: Coverage Gaps & New Tests

## Methodology
- All tests use the Scenario builder for minimal setup
- Run minimum ticks needed to observe behavior (usually 1-3 years)
- Tests marked `#[ignore]` if the behavior is desired but not yet implemented

---

## 1. Cross-System Signal Response Tests

These verify that when System A emits a signal, System B responds correctly.
Each test delivers a specific signal and checks the receiving system's state change.

### 1a. Economy signal handlers (currently untested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_war_severs_faction_trade_routes` | WarStarted | All trade routes between warring factions severed | Gap |
| `scenario_plague_severs_trade_routes` | PlagueStarted | Trade routes to/from plagued settlement severed | Gap |
| `scenario_siege_severs_trade_routes` | SiegeStarted | Trade routes to/from besieged settlement severed | Gap |
| `scenario_disaster_severs_trade_routes` | DisasterStruck | Trade routes to/from disaster settlement severed | Gap |
| `scenario_bandit_raid_reduces_prosperity` | BanditRaid | Target settlement prosperity decreases | Gap |

### 1b. Politics signal handlers (partially tested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_war_started_reduces_happiness` | WarStarted | Both factions lose happiness | Gap |
| `scenario_plague_reduces_happiness` | PlagueStarted | Settlement's faction happiness drops | Gap |
| `scenario_siege_reduces_happiness_and_stability` | SiegeStarted | Defender faction: happiness -0.10, stability -0.05 | Gap |
| `scenario_siege_ended_conquered_stability_hit` | SiegeEnded(conquered) | Defender faction major stability hit | Gap |
| `scenario_siege_ended_lifted_morale_boost` | SiegeEnded(lifted) | Defender faction stability boost | Gap |
| `scenario_disaster_reduces_happiness` | DisasterStruck | Settlement's faction happiness drops | Gap |
| `scenario_disaster_ended_happiness_recovery` | DisasterEnded | Settlement's faction happiness partially recovers | Gap |
| `scenario_bandit_gang_reduces_stability` | BanditGangFormed | Region owner faction loses stability | Gap |
| `scenario_bandit_raid_reduces_happiness` | BanditRaid | Raided settlement's faction loses happiness | Gap |
| `scenario_trade_route_raided_reduces_stability` | TradeRouteRaided | Route endpoints' factions lose stability | Gap |
| `scenario_alliance_betrayed_victim_rally` | AllianceBetrayed | Victim faction: +0.05 happiness, +0.05 stability | Gap |
| `scenario_refugees_reduce_happiness` | RefugeesArrived | Receiving faction happiness slight decrease | Gap |
| `scenario_cultural_rebellion_reduces_stability` | CulturalRebellion | Faction stability hit | Gap |

### 1c. Crime signal handlers (untested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_conquest_spikes_crime` | SettlementCaptured | Crime rate increases in captured settlement | Gap |
| `scenario_decisive_war_end_spikes_loser_crime` | WarEnded(decisive) | Loser faction settlements get crime spike | Gap |
| `scenario_plague_end_spikes_crime` | PlagueEnded (high deaths) | Crime rate increases in affected settlement | Gap |
| `scenario_disaster_spikes_crime` | DisasterStruck | Crime increases proportional to severity | Gap |

### 1d. Disease signal handlers (untested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_refugees_increase_disease_risk` | RefugeesArrived | Disease outbreak probability increased | Gap |
| `scenario_siege_increases_disease_risk` | SiegeStarted | Siege disease bonus extra set | Gap |
| `scenario_siege_end_clears_disease_bonus` | SiegeEnded | Siege disease bonus removed | Gap |
| `scenario_flood_increases_disease_risk` | DisasterStruck(Flood) | Post-disaster disease risk extra set | Gap |
| `scenario_disaster_end_clears_disease_risk` | DisasterEnded | Post-disaster disease risk cleared | Gap |

### 1e. Culture signal handlers (partially tested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_refugees_bring_culture` | RefugeesArrived | Source culture added to destination makeup | Gap |
| `scenario_trade_spreads_culture` | TradeRouteEstablished | Small culture share transferred between endpoints | Gap |
| `scenario_faction_split_inherits_culture` | FactionSplit | New faction gets settlement's dominant culture | Gap |

### 1f. Religion signal handlers (partially tested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_conquest_adds_conqueror_religion` | SettlementCaptured | Conqueror's religion gets share in captured settlement | Gap |
| `scenario_refugees_bring_religion` | RefugeesArrived | Source religion added to destination makeup | Gap |
| `scenario_faction_split_inherits_religion` | FactionSplit | New faction gets settlement's dominant religion | Gap |
| `scenario_temple_boosts_dominant_religion` | BuildingConstructed(Temple) | Dominant religion share increases | Gap |
| `scenario_disaster_boosts_fervor` | DisasterStruck | Settlement's faction fervor increases (if nature-worship tenet) | Gap |

### 1g. Reputation signal handlers (mostly untested beyond war)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_conquest_prestige_changes` | SettlementCaptured | Captor faction +prestige, loser -prestige | Gap |
| `scenario_siege_prestige_changes` | SiegeEnded | Winner/loser prestige adjusted by outcome | Gap |
| `scenario_building_constructed_prestige` | BuildingConstructed | Settlement prestige slight increase | Gap |
| `scenario_building_upgraded_prestige` | BuildingUpgraded | Settlement prestige increase | Gap |
| `scenario_trade_route_prestige` | TradeRouteEstablished | Both endpoint settlements gain prestige | Gap |
| `scenario_plague_end_prestige` | PlagueEnded | Settlement prestige decrease | Gap |
| `scenario_faction_split_prestige` | FactionSplit | Old faction prestige decrease | Gap |
| `scenario_cultural_rebellion_prestige` | CulturalRebellion | Faction prestige decrease | Gap |
| `scenario_treasury_depleted_prestige` | TreasuryDepleted | Faction prestige decrease | Gap |
| `scenario_leader_death_prestige` | EntityDied (leader) | Faction prestige hit | Gap |
| `scenario_disaster_prestige` | DisasterStruck | Settlement prestige decrease by severity | Gap |
| `scenario_bandit_gang_prestige` | BanditGangFormed | Region owner prestige decrease | Gap |
| `scenario_bandit_raid_prestige` | BanditRaid | Victim settlement prestige decrease | Gap |
| `scenario_item_tier_prestige` | ItemTierPromoted (tier>=2) | Holder prestige increase | Gap |
| `scenario_knowledge_created_prestige` | KnowledgeCreated | Settlement prestige increase | Gap |
| `scenario_religion_schism_prestige` | ReligionSchism | Settlement prestige change | Gap |
| `scenario_prophecy_prestige` | ProphecyDeclared | Settlement prestige change | Gap |
| `scenario_religion_founded_prestige` | ReligionFounded | Settlement prestige increase | Gap |
| `scenario_betrayal_prestige` | AllianceBetrayed | Betrayer -prestige, victim +prestige | Gap |
| `scenario_succession_crisis_prestige` | SuccessionCrisis | Faction prestige decrease | Gap |

### 1h. Knowledge signal handlers (mostly untested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_conquest_creates_knowledge` | SettlementCaptured | Historical knowledge created | Gap |
| `scenario_siege_creates_knowledge` | SiegeEnded | Military knowledge created | Gap |
| `scenario_leader_death_creates_knowledge` | EntityDied (leader) | Dynasty knowledge created | Gap |
| `scenario_faction_split_creates_knowledge` | FactionSplit | Political knowledge created | Gap |
| `scenario_disaster_creates_knowledge` | DisasterStruck (severe) | Natural knowledge created | Gap |
| `scenario_plague_end_creates_knowledge` | PlagueEnded | Medical knowledge created | Gap |
| `scenario_cultural_rebellion_creates_knowledge` | CulturalRebellion | Cultural knowledge created | Gap |
| `scenario_building_creates_knowledge` | BuildingConstructed | Technical knowledge created | Gap |
| `scenario_item_tier_creates_knowledge` | ItemTierPromoted (tier>=2) | Cultural knowledge created | Gap |
| `scenario_item_crafted_creates_knowledge` | ItemCrafted (notable crafter) | Cultural knowledge created | Gap |
| `scenario_religion_schism_creates_knowledge` | ReligionSchism | Religious knowledge created | Gap |
| `scenario_religion_founded_creates_knowledge` | ReligionFounded | Religious knowledge created | Gap |
| `scenario_betrayal_creates_knowledge` | AllianceBetrayed | Dynasty knowledge created | Gap |
| `scenario_succession_crisis_creates_knowledge` | SuccessionCrisis | Political knowledge created | Gap |

### 1i. Items signal handlers (partially tested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_conquest_loots_items` | SettlementCaptured | Notable items transferred to captor | Gap |
| `scenario_siege_adds_item_resonance` | SiegeEnded(conquered) | Items at settlement gain siege resonance | Gap |
| `scenario_bandit_raid_steals_items` | BanditRaid | 20% chance notable items stolen | Gap |

### 1j. Buildings signal handlers (partially tested)

| Test | Signal | Expected behavior | Status |
|------|--------|-------------------|--------|
| `scenario_conquest_damages_buildings` | SettlementCaptured | Building conditions reduced | Gap |

---

## 2. Core System Behavior Tests

### 2a. Economy — Treasury & Tribute

| Test | Expected behavior | Status |
|------|-------------------|--------|
| `scenario_tribute_flows_to_receiver` | Treasury decreases for payer, increases for receiver per year | Gap (integration exists but no unit test) |
| `scenario_tribute_ends_after_specified_years` | Tribute relationship ends when years run out | Gap |
| `scenario_bankrupt_faction_cannot_pay_tribute` | Tribute payment capped at available treasury | Gap |
| `scenario_prosperity_clamped_to_bounds` | Prosperity stays within [0.05, 0.95] even with extreme inputs | Gap |
| `scenario_overcrowding_reduces_prosperity` | Pop > capacity reduces prosperity target | Gap |
| `scenario_crime_reduces_prosperity` | High crime rate reduces prosperity | Gap |

### 2b. Demographics — Capacity & Growth

| Test | Expected behavior | Status |
|------|-------------------|--------|
| `scenario_aqueduct_increases_capacity` | Aqueduct building bonus increases settlement capacity | Gap |
| `scenario_granary_provides_food_buffer` | Granary building bonus during low food seasons | Gap |
| `scenario_seasonal_food_affects_growth` | Winter months have lower growth than summer | Partially covered |
| `scenario_overcrowded_settlement_grows_slower` | Pop near capacity reduces birth rate | Gap |

### 2c. Conflicts — War Goals & Peace Terms

| Test | Expected behavior | Status |
|------|-------------------|--------|
| `scenario_grievance_increases_war_chance` | Factions with grievances more likely to declare war | Gap |
| `scenario_high_grievance_causes_punitive_war` | Grievance > 0.5 leads to Punitive war goal | Gap |
| `scenario_high_grievance_increases_reparations` | Grievance > 0.4 leads to 1.5x reparations | Gap |
| `scenario_prestige_affects_peace_terms` | Winner prestige > 0.5 leads to harsher terms | Gap |
| `scenario_prestige_affects_battle_morale` | Higher faction prestige gives morale bonus in battle | Gap |
| `scenario_religious_fervor_increases_war_motivation` | Avg fervor adds to war declaration chance | Gap |

### 2d. Politics — Happiness & Stability Drivers

| Test | Expected behavior | Status |
|------|-------------------|--------|
| `scenario_leaderless_faction_loses_happiness` | No leader = happiness penalty | Gap |
| `scenario_religious_tension_reduces_happiness` | High religious_tension reduces happiness | Gap |
| `scenario_temple_bonus_increases_happiness` | Temple building bonus adds happiness | Gap |
| `scenario_stability_drifts_toward_legitimacy` | Stability converges toward legitimacy target | Partial |
| `scenario_leader_prestige_boosts_legitimacy` | Leader prestige raises legitimacy target | Gap |
| `scenario_prestige_reduces_coup_chance` | Leader with high prestige resists coups | Gap |
| `scenario_prestige_reduces_faction_split_chance` | High faction prestige reduces split probability | Gap |

### 2e. Grievance System

| Test | Expected behavior | Status |
|------|-------------------|--------|
| `scenario_conquest_creates_grievance` | SettlementCaptured creates 0.40 grievance | Gap |
| `scenario_war_defeat_creates_grievance` | Decisive war loss creates 0.35 grievance | Gap |
| `scenario_betrayal_creates_grievance` | AllianceBetrayed creates 0.50 grievance | Gap |
| `scenario_grievance_decays_over_time` | Grievances decay at 0.03/year for factions | Indirectly tested |
| `scenario_war_victory_satisfies_grievance` | Winning war reduces grievance against loser | Gap |
| `scenario_grievance_blocks_alliance` | Mutual grievance > threshold blocks alliance formation | Gap |
| `scenario_seek_revenge_desire_from_grievance` | Leader with grievance >= 0.3 generates SeekRevenge | Gap |
| `scenario_ruthless_trait_slows_grievance_decay` | Ruthless person decays grievance at 0.5x rate | Gap |

---

## 3. Emergent Multi-System Behavior Tests

These test that multiple systems interact to produce expected cascading effects.

| Test | Systems involved | Expected chain | Status |
|------|-----------------|----------------|--------|
| `scenario_war_causes_economic_collapse` | Conflicts + Economy + Politics | War → trade severed → prosperity drops → happiness drops | Gap |
| `scenario_siege_disease_outbreak` | Conflicts + Disease | Siege → disease bonus → higher outbreak chance → plague | Gap (may need `#[ignore]` if probabilistic) |
| `scenario_conquest_refugee_culture_shift` | Conflicts + Migration + Culture | Conquest → refugees flee → destination culture changes | Gap |
| `scenario_plague_crime_wave` | Disease + Crime | Plague ends → crime spikes → bandits form | Gap (multi-year, may need `#[ignore]`) |
| `scenario_building_boosts_economy` | Buildings + Economy | Workshop/Mine → production bonuses → higher prosperity | Gap |
| `scenario_temple_boosts_religion` | Buildings + Religion | Temple construction → dominant religion share grows | Gap |
| `scenario_betrayal_cascade` | Actions + Politics + Reputation + Grievance | Betray → grievance + trust loss + prestige loss → revenge war | Gap |
| `scenario_faction_split_inherits_state` | Politics + Culture + Religion | Faction split → new faction gets culture, religion, leader | Gap |
| `scenario_bandit_disrupts_trade` | Crime + Economy | Bandits raid trade routes → income lost → prosperity drops | Gap |
| `scenario_disaster_chain` | Environment + Buildings + Disease + Politics | Disaster → building damage + disease risk + happiness hit | Gap |

---

## 4. Desired-But-Unimplemented Behaviors (mark as `#[ignore]`)

These test behaviors we WANT but that the current code doesn't support.

| Test | What it would verify | Why it's missing |
|------|---------------------|-----------------|
| `scenario_succession_crisis_triggers_claim_war` | SuccessionCrisis signal → conflict system declares war between claimants | Conflicts has no handle_signals; SuccessionCrisis consumed only by reputation/knowledge/agency |
| `scenario_religious_war_between_factions` | Factions with different dominant religions more likely to war | Religion doesn't feed into war declarations directly (only fervor × 0.05 cap 0.10) |
| `scenario_plague_triggers_religious_fervor` | PlagueStarted → religion fervor increases | Religion doesn't handle PlagueStarted signal |
| `scenario_disaster_triggers_religious_fervor` | DisasterStarted → religion fervor increases for all tenet types | Religion only handles DisasterStruck, and only for nature-worship |
| `scenario_trade_spreads_disease` | Diseases spread faster along trade routes | Disease spread checks adjacency but trade route bonus may not be strong enough / testable |
| `scenario_overcrowding_triggers_emigration` | Pop > capacity → people emigrate to less crowded settlements | Migration checks prosperity not capacity directly |

---

## 5. Implementation Priority

### Phase 1 — High-value signal handler tests (Section 1)
Start with economy (1a), politics (1b), and crime (1c) signal handlers. These are the most impactful cross-system interactions and completely untested.

### Phase 2 — Core behavior tests (Section 2)
Economy treasury/tribute, grievance integration, and prestige-affecting-gameplay tests.

### Phase 3 — Emergent behavior tests (Section 3)
Multi-system chain tests that verify the simulation produces expected narratives.

### Phase 4 — Ignored tests for desired features (Section 4)
Document what we want but haven't built yet.

---

## Test File Organization

All new tests go in existing system test modules (inside `#[cfg(test)]` blocks):
- Economy signal tests → `src/sim/economy/mod.rs`
- Politics signal tests → `src/sim/politics/mod.rs`
- Crime signal tests → `src/sim/crime.rs`
- Disease signal tests → `src/sim/disease.rs`
- Culture signal tests → `src/sim/culture.rs`
- Religion signal tests → `src/sim/religion.rs`
- Reputation signal tests → `src/sim/reputation.rs`
- Knowledge signal tests → `src/sim/knowledge.rs`
- Items signal tests → `src/sim/items.rs`
- Buildings signal tests → `src/sim/buildings.rs`
- Emergent tests → `tests/emergent.rs` (new integration test file)
- Ignored/desired tests → `tests/desired_behaviors.rs` (new integration test file)
