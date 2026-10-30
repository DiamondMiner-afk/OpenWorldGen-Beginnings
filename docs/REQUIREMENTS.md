# Requirements register

Status values are `accepted`, `experimental`, `deferred`, and `rejected`.

| ID | Requirement | Status | Acceptance evidence |
| --- | --- | --- | --- |
| R-001 | Local CPU execution without a GPU or network | accepted | Minimal profile benchmark |
| R-002 | Exact financial double-entry conservation | accepted | Property and invariant tests |
| R-003 | Deterministic reference replay | accepted | Same-run and checkpoint tests |
| R-004 | Versioned scenario, data, policy, and result contracts | accepted | JSON Schema and compatibility tests |
| R-005 | One-region small-open-economy inflation laboratory | accepted | Synthetic v0.1 validation report |
| R-006 | Rule, utility, RL, and LLM policies share one typed boundary | accepted | Policy contract tests |
| R-007 | Point prediction of the economy | rejected | Scenario claim boundary |
| R-008 | India public-data calibration | deferred | Revisit after the first US historical pilot and synthetic gate |
| R-009 | Minecraft Java/Fabric adapter | deferred | Opens after synthetic validation gate |
| R-010 | Multi-country, equity markets, climate, geopolitics, and UN | deferred | Added only in dependency order |
| R-011 | IQ as a rationality score | rejected | Scientific charter |
| R-012 | Economic activity exists only in loaded Minecraft chunks | rejected | Conservation-preserving aggregation test |
| R-013 | Cloud compute is required | rejected | Portable local engine |
| R-014 | US 2007–2023 historical reconstruction | experimental | Archived data vintages, balanced starting accounts, preregistered rolling-origin tests |
| R-015 | Strategic local AI agents with recorded replay | experimental | Typed proposals, authority and accounting checks, replay, behavioral comparator |
| R-016 | Fabric projection of the authoritative engine | deferred | Detailed-versus-aggregate equivalence tests after scientific gate |

The vision document remains useful for ideas, but this register is authoritative
for implementation status.
