# OpenWorldGen systems, functions, and design decisions

Status date: 2026-07-24

MVP increment (2026-10-24): optional contract-credit scheduling and the bounded
`observe-firm` / `run-recorded-firm` CLI experiment are now available. The latter
replays consecutive decisions and returns a next observation for an adaptive
local policy driver; it is not a trained RL economy or an empirical forecast.

This is the living technical map of OpenWorldGen. It records what exists, what
is only a contract, what is gate-locked, and why each major design was selected
over plausible alternatives. It describes the implementation; it does not turn
a development model into a validated economic forecast.

For machine-readable availability, run:

```bash
owg capabilities --json
```

`docs/IMPLEMENTATION_STATUS.md` remains authoritative for validation status and
`docs/GATES.md` remains authoritative for promotion between stages.

## Decision criteria

Choices are evaluated in this order:

1. Accounting and scientific correctness.
2. Reproducibility and auditability.
3. Explicit failure instead of plausible-looking degradation.
4. Local execution and confidential-data safety.
5. Portability across ordinary computers.
6. Scale and performance.
7. Extensibility to learned policies, institutional data, and Minecraft.
8. Development convenience.

A faster or more fashionable option does not win if it weakens a higher-ranked
criterion.

## Product and system map

| System | Status | Current responsibility | Deliberate boundary |
| --- | --- | --- | --- |
| Rust simulation kernel | implemented, development | Authoritative state, time, markets, accounting, policies, checkpoints, metrics | No empirical validity claim |
| `owg` CLI | implemented | Validation, execution, comparison, calibration smoke, benchmark, reports, interactive plots, schemas, capabilities | Not a no-code policy dashboard |
| Python SDK | implemented | Safe subprocess control, scenario/data helpers, bundle inspection | Does not reimplement economic rules |
| Protobuf/gRPC API | contract only | Process-neutral service definition | No server process yet |
| Synthetic inflation laboratory | implemented, scientific gate open | Six policy branches around an imported-energy shock | One-replication smoke output is not evidence |
| US historical data pipeline | gate-locked | Versioned 2007–2023 research pilot; release-vintage schema and file verifier exist | No real US bundle or empirical calibration |
| India data/calibration pipeline | gate-locked | Later public-data pilot and simulation-based inference | Follows the US priority and scientific gates |
| RL and LLM policies | typed contract plus recorded replay | Future interchangeable strategic policies | No runtime model calls, authority enforcement, or trained weights |
| Minecraft Fabric adapter | gate-locked | Future visualization, controls, and role play | Never authoritative economic state |
| Multi-country, FX, climate, geopolitics | gate-locked | Controlled expansion after prior gates | Excluded from v0.1 |

## Runtime flow

One engine and one model pack serve every interface:

```text
Scenario + model pack + data bundle
                  |
                  v
        validate IDs, hashes, units
                  |
                  v
     deterministic typed scheduler
                  |
                  v
  economy handlers -> ledger transactions
                  |
                  v
 accounting checks + metric observations
                  |
                  v
 comparison + mechanisms + audit evidence
                  |
                  v
 checksum-protected ResultBundle v2 with offline graphs
```

Independent policy branches and Monte Carlo replications own isolated worlds.
Only those worlds are parallelized. Reference-mode reductions inside one world
remain ordered.

## Deterministic scheduler

`MultiRateScheduler` emits typed `ScheduledEvent` values. Ordering is
`month`, `within_month_order`, `priority`, `actor_id`, then `sequence`. The
completed schedule has its own rolling SHA-256 audit, separate from the
financial journal hash.

The default four-week month contains 23 events:

| Order | Event | Cadence | Purpose |
| ---: | --- | --- | --- |
| 10 | `reset_month` | monthly | Clear flow accumulators and prior-period action fields |
| 20 | `update_policy` | monthly, rule adjusts quarterly | Apply the central-bank reaction rule and branch rate delta |
| 30 | `restart_inactive_firms` | monthly | Permit deterministic probabilistic firm entry after the waiting period |
| 40 | `match_labor` | monthly | Layoff and vacancy-search matching |
| 50 | targeted transfers, unemployment benefits | monthly | Apply discretionary and automatic fiscal transfers |
| 60 | `update_firm_prices` | monthly | Cost, markup, inventory, and shock-sensitive price setting |
| 70 | opening inventory, production/imports | monthly | Establish the resource-account opening stock and produce |
| 80 | payroll and household debt | monthly | Settle wages, taxes, interest, and principal |
| 100–103 | `goods_market` | four weekly events | Bounded-search household purchases |
| 200 | foreign demand | monthly | Export settlement against the rest-of-world boundary |
| 210 | government demand | monthly | Procurement and public-investment purchases |
| 220 | firm debt service | monthly | Interest and principal settlement |
| 230 | firm defaults and bank resolution | monthly | Traceable write-offs, exit, and recapitalization |
| 240 | learning | monthly | Update habits, expectations, and expected demand |
| 250 | reporting | monthly | Compute prices, accounts, distributions, and invariants |
| 260 | close month | monthly | Advance authoritative time only after reporting succeeds |

Production remains a monthly batch in v0.1 while household purchases are
weekly. This preserves the preregistered development mechanism. Weekly
production is deferred until it has its own inventory, payroll, and
scale-convergence tests.

## Economic systems

### Households

State uses structure-of-arrays storage for identifiers, weights, bank
relationships, employment, skills, consumption propensities, habit strength,
risk preference, information access, trust, inflation expectations,
consumption, income, and delinquency.

Behavior includes labor search, habit-sensitive consumption, limited supplier
search, liquidity constraints, essential-consumption credit, debt service,
delinquency, default, evidence/social expectation updates, and slow propensity
adaptation.

### Firms

State includes sector, bank, productivity, capacity, inventory, posted price,
wage offer, expected demand, sales, markup, energy intensity, activity,
default streak, and inactive duration.

Behavior includes inventory-sensitive pricing, production planning, imported
energy inputs, payroll, working-capital credit, bounded goods settlement,
exports, government procurement, debt service, default, exit, and entry.

### Banks

Banks maintain deposit liabilities, reserves, capital/equity, loan assets,
lending spreads, approval thresholds, expected losses, capital requirements,
and liquidity requirements. Credit creation, repayments, write-offs, and
resolution use explicit multi-posting journal entries.

### Government and central bank

Government implements income taxes, indirect taxes, unemployment benefits,
targeted transfers, procurement, public investment, treasury balances, debt,
and bank recapitalization. The central bank supplies the policy-rate rule,
reserve liabilities, government deposits, and the development government-debt
backstop.

### Rest of world

Version 0.1 has an explicit financial counterparty for imports and exports.
Foreign demand and imported-energy prices are exogenous. Endogenous exchange
rates, capital flows, foreign banks, and multiple currencies are intentionally
deferred.

### Markets

| Market | Implemented mechanism | Why this mechanism now | Deferred alternative |
| --- | --- | --- | --- |
| Sector goods | Posted prices plus bounded random search | Represents information/search limits and scales predictably | Auction or equilibrium clearing |
| Labor | Vacancy search with firm desired headcount | Allows unemployment and matching friction | Centralized Walrasian clearing |
| Deposits | Immediate double-entry transfer | Exact conservation and bank attribution | Aggregate money-pool updates |
| Credit | Bank threshold, collateral/headroom, capital and liquidity checks | Traceable rationing and transmission | Unconstrained loan demand |
| Government debt | Matched government liability and central-bank asset | Keeps financing explicit in the development model | Full bond auction and yield curve |

## Accounting systems

Money is integer minor units with checked arithmetic and deterministic
half-away-from-zero rate rounding. Real quantities and rates are floating point
with fixed reference-mode order.

Every financial transaction must:

- contain at least two postings;
- sum to exactly zero before commit;
- update no balance if validation fails;
- enter the rolling journal hash and phase/month audit.

Monthly diagnostics include:

- domestic production versus final uses and independently measured inventory
  change;
- production-side versus expenditure-side nominal GDP;
- income decomposition into gross value added, employee compensation, derived
  gross operating surplus, and taxes on products;
- household, firm, bank, government, central-bank, rest-of-world, ownership,
  and unclassified net financial positions;
- exact institutional-sector stock residual;
- maximum errors by branch in `accounting.json`.

Gross operating surplus is currently `gross value added - compensation`. It is
a transparent development decomposition, not an independently estimated income
series. Depreciation, subsidies, mixed income, and a fully independent
income-account source remain open work.

## Learning and policy hierarchy

Implemented reference behavior uses slow habits, adaptive expectations,
bounded search, peer/public signals, liquidity constraints, and firm demand
learning. No direct IQ-to-rationality coefficient exists.

`AgentPolicy` provides one boundary for rule, utility, RL, LLM, and human
policies:

```text
ObservationBatch -> AgentPolicy.act -> ActionBatch -> validation -> world event
```

Only deterministic rule behavior is active in reference runs. RL and LLM
entries in capabilities mean the typed contract exists, not that a trained or
runtime adapter is approved.

## Public contracts

| Contract | Current version | Purpose |
| --- | --- | --- |
| `ScenarioSpec` | `owg.scenario.v1` draft | Calendar, geography, population, systems, shocks, policies, metrics, precision, compute |
| `DataBundle` | `owg.data-bundle.v1` | Tables, hashes, units, classifications, provenance, confidentiality, transformations |
| `ModelPack` | `owg.model-pack.v1` | Archetypes, markets, schedules, rules, parameters, posterior reference, status |
| `PolicyContract` | schema v1 | Batched observations, typed actions, learning update, fallback declaration |
| `Checkpoint` | `owg.checkpoint.v2` | Exact state, institution graph, metrics, financial journal hash, scheduler audit, resume position; v1 is not resumable by this build |
| `RunManifest` | `owg.run-manifest.v1` | Input/build hashes, seed streams, platform, warnings, status, invariants |
| `ResultBundle` | `owg.result-bundle.v2` | Nine required artifacts plus an optional checksum-covered self-contained HTML graph dashboard |
| `EngineCapabilities` | `owg.capabilities.v1` | Machine-readable implemented and gated features |

ResultBundle v1 remains readable. Version 2 adds mandatory `accounting.json`;
new v2 writers also declare and checksum `plots.html`, while older v2 bundles
without it remain readable. The scenario and checkpoint v1 contracts are still
development drafts; additive fields remain possible until the first contract
freeze. After that freeze, incompatible changes require a new version and
migration path.

## CLI functions

| Command | Function |
| --- | --- |
| `owg capabilities [--json]` | Report contract versions, agents, markets, formats, implemented systems, and gated systems |
| `owg validate --kind ...` | Validate scenario, data, model, policy, manifest, checkpoint, or result-bundle contracts |
| `owg run` | Run selected branches and fixed replications |
| `owg compare` | Run every branch with common random numbers; optionally stop by declared precision |
| `owg checkpoint` | Execute to a month and save exact state |
| `owg resume` | Continue a compatible checkpoint |
| `owg calibrate` | Run the explicitly limited four-seed synthetic workflow smoke test |
| `owg benchmark` | Measure a baseline run and emit machine-readable performance evidence |
| `owg report` | Verify a bundle before producing a Markdown report |
| `owg plot` | Verify a bundle and produce a self-contained interactive HTML dashboard |
| `owg init-scenario` | Write the canonical inflation-lab scenario |
| `owg schema` | Generate the selected current JSON Schema |

`run` and `compare` write a failure manifest if execution fails. Validation,
checkpoint, resume, report, schema, and benchmark return explicit errors and do
not fabricate result bundles.

## Python SDK functions

`OpenWorldGen` never invokes a shell. It passes an argument vector to the same
Rust executable.

| Method | Function |
| --- | --- |
| `capabilities()` | Return the machine-readable capability map |
| `validate(path, kind=...)` | Validate any supported contract |
| `verify_bundle(path)` | Validate bundle structure, hashes, and cross-file consistency |
| `init_scenario(output)` | Create the canonical scenario |
| `write_schema(output, kind=...)` | Generate a current schema |
| `run(...)` | Run selected branches |
| `compare(...)` | Compare all policy branches |
| `checkpoint(...)` | Save a checkpoint |
| `resume(...)` | Resume a checkpoint |
| `calibrate(...)` | Invoke the bounded calibration smoke |
| `benchmark(...)` | Return benchmark JSON |
| `report(...)` | Generate a verified report |
| `plot(...)` | Generate a verified, offline interactive graph dashboard |

`ResultBundle` exposes `manifest`, `comparison`, `mechanisms`, `accounting`,
`package`, the declared `visualization` path, and CSV `metric_rows()`.

## Rust module and function catalogue

Only public APIs are compatibility candidates. Private handlers are listed
separately because they define current model mechanisms.

### `capabilities`

- `EngineCapabilities::current`: constructs the versioned runtime capability
  declaration.

### `contracts`

- `ScenarioSpec::validate`: returns structured validation issues without
  executing.
- `ScenarioSpec::validated`: converts invalid scenarios to an engine error.
- `ScenarioSpec::hash`: hashes canonical serialized scenario content.
- `ScenarioSpec::inflation_lab_v0_1`: constructs the canonical synthetic
  experiment.
- `ValidationIssue::error`: constructs a typed error issue.

### `data`

- `DataBundle::validate`: validates metadata, provenance, units, and paths.
- `DataBundle::hash`: hashes canonical bundle metadata.
- `DataBundle::validate_table_content`: verifies a declared table checksum.
- `DataBundle::sector_parameters_from_csv`: validates and loads the four-sector
  development table.

### `model`

- `ModelPack::validate`: validates model identity, definitions, parameters,
  schedules, and posterior reference.
- `ModelPack::hash`: hashes canonical model metadata.
- `ModelPack::validate_posterior_content`: verifies frozen learned/calibrated
  parameter content when present.

### `money`

- `Money::minor_units`: returns the authoritative integer representation.
- `checked_add`, `checked_sub`, `checked_mul`: overflow-safe arithmetic.
- `mul_rate`: deterministic rate multiplication and rounding.
- `as_major_f64`, `from_major_f64`: explicit boundary conversion.

### `rng`

- `SeedBook::u64`: derive a stable raw draw from root seed, namespace, actor,
  time, and draw index.
- `unit`: map a named draw to the unit interval.
- `range`: map a named draw to a declared floating-point interval.
- `normal`: derive a standard-normal draw without mutable RNG state.
- `index`: choose a deterministic bounded index.

### `ledger`

- `AccountKey::new`, `label`: construct and identify accounts.
- `Posting::new`, `JournalEntry::new`, `JournalEntry::residual`: construct and
  preflight transactions.
- `Ledger::balance`, `balances`, `owner_balance`: read balances.
- `Ledger::post`: atomically validate, hash, audit, and commit a journal entry.
- `Ledger::transfer`: convenience for a two-account transfer.
- `Ledger::audit`, `audit_global_balance`, `assert_balanced`, `journal_hash`:
  expose conservation evidence.

### `scheduler`

- `MultiRateScheduler::new`: validate scheduler configuration.
- `weeks_per_month`, `events_for_month`: inspect or generate the canonical
  schedule.
- `ScheduledEvent::identifier`: stable event identity.
- `SimulationEventKind::label`: stable audit label.
- `ScheduleAudit::record`, `schedule_hash`: enforce order and produce replay
  evidence.

### `economy`

- `Households::len/is_empty`, `Firms::len/is_empty`, `Banks::len/is_empty`:
  data-oriented collection inspection.
- `EconomicParameters::from_inputs`: construct checked runtime parameters from
  model/data contracts.
- `Economy::new`, `new_with_parameters`: create a deterministic balanced world.
- `Economy::step_month`: execute one complete typed schedule and return one
  metric observation.

Private mechanism handlers called by the scheduler are:

- `update_policy`, `restart_inactive_firms`, `update_labor_market`;
- `apply_targeted_transfers`, `apply_unemployment_benefits`;
- `update_firm_prices`, `produce_and_import`, `pay_wages`;
- `household_consumption`, `foreign_demand`, `government_demand`;
- `ensure_firm_liquidity`, `ensure_household_liquidity`;
- `service_household_debt`, `service_firm_debt`;
- `resolve_defaults`, `resolve_banks`, `update_learning`;
- `collect_metrics`, `institutional_balances`;
- deposit, treasury, lending, inventory, and event-ID helpers.

### `metrics` and `comparison`

- `MetricRow::finite`: rejects non-finite or internally inconsistent rows.
- `IncomeAccounts::finite`: validates income components.
- `InstitutionalBalances::calculated_residual`: exact sector-stock closure.
- `gini`, `weighted_deciles`: weighted distribution statistics.
- `ComparisonReport::from_metrics`: replication aggregation, confidence
  intervals, Pareto filtering, and normalized minimax regret.

### `engine`

- `RunOptions::for_scenario`: construct the declared default run plan.
- `SimulationEngine::new`: resolve built-in model/data with hash checks.
- `with_inputs`: inject validated process-neutral inputs.
- `scenario`, `model_pack`, `data_bundle`: inspect resolved contracts.
- `run`: execute fixed branches and replications.
- `run_to_precision`: add replication batches until precision or maximum.
- `checkpoint`, `resume`: exact restartable execution.

### `checkpoint`, `manifest`, `result`, and `policy`

- `Checkpoint::new`, `save`, `load`: create and validate restart state.
- `RunManifest::new`, `validate`: create and verify run provenance.
- `ResultBundleContract::validate`: validate safe paths, version, and checksums.
- `PolicyContract::validate`: validate batch shapes, observations, actions, and
  learning rewards.
- `AgentPolicy::descriptor`, `act`, `learn`: adapter interface.
- `NoOpPolicy::new`: deterministic fallback/reference adapter.

### CLI scenario and artifact I/O

- `scenario_io::load_scenario`, `save_scenario`: parse or serialize YAML/JSON
  scenarios through the same validation contract.
- `output::write_failure_manifest`: persist an explicit failed-run artifact.
- `write_result_bundle`: write the required v2 artifacts, interactive graph
  dashboard, and their checksums.
- `verify_result_bundle`: validate paths, checksums, row counts, hashes,
  accounting diagnostics, and audit consistency.
- `write_report`: verify first, then regenerate the Markdown report.

### `plot`

- `write_plot_dashboard_from_result`: aggregate replications into monthly means
  and confidence intervals and write the bundled dashboard.
- `write_plot_dashboard_from_bundle`: verify an existing bundle, load its CSV
  metrics, and generate a dashboard at a user-selected path.
- The dashboard provides selectable metrics and branches, evaluation-window
  shading, confidence bands, keyboard-selectable periods, hover values, a
  Pareto-aware inflation/output trade-off plot, and SVG export.

The remaining CLI functions are command dispatch, schema generation, benchmark
and calibration orchestration, validation presentation, and private
CSV/Parquet/JSON serialization helpers. They are implementation details behind
the public commands listed above.

## Why these choices instead of the alternatives

| Chosen design | Alternative not selected | Reason |
| --- | --- | --- |
| Independent Rust kernel | Python-only engine | Predictable memory, native portability, checked integer arithmetic, and parallel Monte Carlo without making notebooks authoritative |
| Rust rather than C++ | C++ kernel | Rust provides comparable systems performance with stronger memory safety and easier portable dependency management |
| Python SDK over rewriting models in Python | Separate Python model | One source of economic truth prevents language drift |
| Process-neutral Protobuf/gRPC boundary | Minecraft or notebooks linked directly into Rust internals | Crash isolation, server authority, language neutrality, and future remote execution |
| Arrow/Parquet plus CSV | JSON-only metrics | Typed columns and compact analytical I/O, with CSV retained for universal inspection |
| Self-contained HTML/SVG graphs | Mandatory plotting server or Python notebook | Opens locally without a network, GPU, package installation, or resident service and preserves the verified source bundle |
| Fixed-point integer money | Binary floating-point money | Exact transaction closure and deterministic rounding |
| Floating quantities/rates | Fixed-point for every physical value | Scientific calculations need range and convenience; deterministic order plus tolerance is more practical |
| Structure of arrays | One heap object per agent | Better memory locality and scale on weak computers |
| Named counter-based random streams | One global sequential RNG | Adding a thread or unrelated draw cannot silently change every later outcome |
| Typed scheduler | Ad hoc calls and callbacks | Auditable order, checkpoint continuity, cadence tests, and future event extensions |
| Double-entry global ledger | Direct balance mutation | Conservation, counterparties, defaults, and traceable mechanisms |
| Rolling in-memory journal hash | Retain millions of full entries in RAM | Bounded memory; a durable streaming sink can later preserve full records |
| Agent-based core | DSGE-only system | Heterogeneity, networks, defaults, bounded rationality, and distributional outcomes |
| Scenario comparison plus statistical baselines | ABM as a point-forecast oracle | Separates causal assumptions from forecast qualification |
| Common random numbers | Independent random seeds per policy | Reduces comparison noise and attributes differences to policy branches |
| Pareto frontier and minimax regret | Automatic scalar “best policy” | Makes trade-offs visible and requires users to declare objectives |
| Layered adaptive rules in reference mode | Online RL everywhere | Easier reproduction, interpretation, calibration, and failure analysis |
| Typed LLM proposals only | LLM directly mutates the world or ledger | Enforces permissions, accounting, timeouts, and deterministic fallback |
| Behavioral constraints rather than IQ | IQ as a rationality multiplier | Avoids an invalid one-dimensional proxy and supports observable mechanisms |
| Local-first, CPU-required baseline | Mandatory cloud/GPU | Institutional confidentiality and broad hardware access |
| Minecraft sidecar | Economy embedded in game ticks | Keeps game performance and economic authority separate |
| Synthetic gate before India calibration | Immediate fitting to public data | Prevents empirical calibration from masking accounting or mechanism defects |
| Dependency-ordered expansion | Add stocks, wars, UN, climate immediately | Each new feedback loop otherwise multiplies unidentified parameters and failure modes |

## Current rejected or deferred options

- No AGPL-derived engine code until a separate licensing decision.
- No online-learning reference runs.
- No row-level confidential data sent to external AI services.
- No “best move” without declared objective weights and risk constraints.
- No forecast label until preregistered baselines and interval calibration pass.
- No Minecraft implementation before the synthetic scientific gate closes.
- No full transaction retention in RAM; the future implementation is a bounded
  streaming audit sink.
- No claim that income accounting is independently estimated while operating
  surplus remains derived.

## Maintenance rule

Any code change that adds, removes, or materially changes a public command,
contract, market, agent type, scheduler event, accounting identity, policy
kind, or gate must update:

1. `EngineCapabilities::current`;
2. this catalogue;
3. the relevant schema and tests;
4. `docs/IMPLEMENTATION_STATUS.md`;
5. the model card or preregistration if interpretation changes.

This rule keeps the human document, machine-readable capability declaration,
and executable behavior from becoming three different stories.
