# Implementation status

Status date: 2026-10-24
Release: 0.1.0-development

This file is the authoritative boundary between executable work and planned
scope. A checked-in interface is not evidence that its scientific gate passed.

## Runnable MVP increment — October 24

See [MVP runbook](MVP.md). The previously interrupted contract-credit layer now
compiles and passes nine focused tests plus a CLI integration test. A 120-month
synthetic run, three branches/two replications, bundle verification, exact
checkpoint replay and six live-Qwen executive choices completed. The executive
selected retain/no-op in all six months; a separate reference run executed
three cash transfers. Ordinary bundle behavior and the recorded AI experiment
remain separate.

A separate real-US forecasting actor test made 45 local-Qwen predictions from
15 rolling origins (2020Q1–2023Q3), withholding future target labels and dates
from prompts. All calls were accepted. Graphs compare forecasts to matched
next-vintage actuals and persistence/AR(1) baselines. This does not initialize
the economic kernel with measured US balances. Pretraining contamination is
uncontrolled, and claimed 80% intervals covered only 33–47% of outcomes.
It remains exploratory and not forecast-qualified. Source-origin admission,
balanced US initialization, trained RL and runtime MatrAIx calibration remain
open. Historical counts and limitations below describe earlier increments.

## Working vertical slice

- Rust 1.95 workspace with an independent `owg-core` kernel and `owg` CLI.
- Python 3.13 SDK that invokes the same CLI contract without shell expansion.
- Versioned scenario, model-pack, data-bundle, policy, manifest, result, and
  checkpoint schemas.
- Hash-checked built-in model/data resolution. The sector CSV supplies prices,
  input intensity, consumption weights, and tax rates used by the economy; the
  model pack supplies the implemented fiscal, credit, banking, and entry
  parameters.
- Structure-of-arrays households, firms, and banks; deterministic named random
  streams; bounded-search goods and labor markets; a typed multi-rate scheduler
  with strict ordering and a separate rolling schedule hash.
- Fixed-point money, double-entry transactions, rolling journal audit hashes,
  institutional balance sheets, defaults, bank resolution, government debt,
  production/expenditure/income diagnostics, institutional-sector stock
  closure, and national-account identity checks. Cross-bank and Treasury
  payments now include commercial-bank/central-bank reserve settlement, with
  an exact aggregate reserve-claim audit at opening, monthly close, load,
  and resume. Separate exact per-bank deposit and loan counterparty audits
  reject wrong-bank claims, unknown account owners, and unsupported signs. A
  shared checkpoint preflight checks ledger, metadata, and these account
  identities on both file load and direct in-memory resume.
- Six common-random-number policy branches for the imported-energy shock.
- Exact same-build reference replay, checkpoint/resume, bounded parallel
  branch/replication execution, Monte Carlo confidence intervals, Pareto
  filtering, and minimax-regret diagnostics.
- Parquet and CSV metrics, sector CPI decomposition, consumption deciles,
  mechanism summaries, branch accounting diagnostics, audit summaries,
  ResultBundle v2 reports, self-contained interactive HTML/SVG graphs,
  checkpoints, and explicit failure manifests.
- Machine-readable `capabilities` output and a maintained system/function/
  decision catalogue.
- A research ledger and mechanism register for the US 2007–2023 pilot, plus a
  standalone data-vintage contract that checks release dates, safe local paths,
  and archived file hashes. Three Philadelphia Fed real-time macro series were
  collected across 2006 Q4 and 2007–2023. This is not structural-economy
  calibration.
- A local research-state integrity auditor verifies registered artifact hashes,
  evidence links, duplicate claim IDs, and unsafe paths. It does not establish
  source authenticity or whether evidence supports a scientific claim.
- An opt-in `owg.data-vintage.v2` contract distinguishes published-release
  timing from independently claimed exact-byte capture. Its strict CLI path
  rejects publication-only and post-cutoff capture claims and checks local
  source/archive hashes. V1 and v2 source reads now reject symlink/path
  escapes. It still explicitly reports historical input **not admitted**:
  an independent archive-origin verifier is required; the local path check
  alone is not race-proof against concurrent filesystem replacement.
  V1 Philadelphia manifests remain compatible and are not auto-promoted.
- The 2023 Census BDS release is archived as a **latest-revised evaluation-only**
  2007–2023 firm-dynamics slice, with source hashes, suppression handling, and
  recorded national-versus-sector flow residuals. BDS was first released in
  2008, so even an older edition cannot be a January 2007 actor input. Four
  dated Fed Z.1 releases (eight ZIP/PDF files)
  passed format/hash probes. Two household/nonprofit deposit series are parsed
  and code/label/unit bridged across adjacent 2007 PRN and 2023 CSV archives,
  with same-quarter surviving-file differences recorded. Sector comparability
  and the surviving 2007 archives' exact as-of
  availability remain unresolved. They are not calibration inputs.
- A December 7, 2006 Z.1 PRN ZIP/PDF pair was archived as a pre-January-2007
  release-level candidate. Two 2006 Q3 household/nonprofit deposit codes
  parse and agree with PDF rounding, and same-quarter differences from the
  surviving March 2007 file are recorded. Both December-labelled current
  files have post-origin HTTP modification dates, so these exact bytes cannot
  enter a January 2007 actor state without stronger independent evidence.
- A 2005 BEA annual make/use workbook was archived from a June 2007 capture;
  it matches the current BEA-hosted bytes. BEA documented the 2005 I-O release
  in December 2006, but these exact bytes are not certified for a January 2007
  actor. A pinned direct-XLS parser checked all 8,840 Make/Use matrix cells and
  693 displayed-detail accounting comparisons. Published cross-table output
  totals match; the largest detail residual is $0.7 million. A versioned,
  reviewable 65-code four-sector **candidate** keeps signed adjustments
  separate and preserves raw-to-aggregated totals; its uncertain assignments
  and imported content prevent promotion to a supplier network. A matched,
  dated before-redefinitions import matrix is required to separate domestic
  commodity use; none has been verified for January 2007. The measured
  stock-flow opening state remains open.
- A bounded archive search rejected a 2007-captured 2005 BEA import workbook:
  it is **after redefinitions**, while the audited 2005 Make/Use workbook is
  **before redefinitions**. It cannot be subtracted cellwise or admitted at the
  January 2007 origin. A modern before-redefinitions workbook is retrospective
  evidence only; see the domestic/import bridge gate.
- The February 2006 Fed 2004 SCF Bulletin article is archived with a PDF hash,
  retrieval metadata, and a bounded parser for published 2004 family-wealth
  percentile cutoffs and within-group means. Two PDF extraction tests and
  internal median/weighted-mean cross-checks pass. The current PDF's exact
  January 2007 byte availability is unproved; these published summaries do
  not identify a full 2007 household distribution or matched financial
  accounts. Later public extracts are excluded from that historical origin.
- A BLS source scout identified three publication-level pre-January-2007
  labor/earnings/union candidates and three later-January leakage exclusions.
  Direct official PDF retrieval returned HTTP 403, so no BLS binary, source
  hash, or exact-byte actor input was ingested; this is source research only.
- A decision-tape contract and exact-observation replay adapter for recorded
  strategic policy proposals. A copied local Qwen model produced one isolated
  macro hypothesis probe and a separate two-slot fictional-strategy benchmark.
  A bounded loopback-only adapter can generate and verify schema-valid decision
  tapes with a mocked inference server; no live Qwen call has been made through
  this adapter. An isolated recorded-firm dispatcher now constructs a firm
  observation from engine state, exact-matches one tape step, and can atomically
  apply the existing direct-owner cash-transfer primitive. It is **not**
  scheduled by `SimulationEngine`, bound to run/checkpoint hashes, screened by
  mutable law, or a dividend/tax determination.
- An initial institution/ownership graph records legal form, tax classification,
  role, jurisdiction, and direct stakes. Scenario-supplied graphs are validated
  against agent IDs and included in the scenario hash; nested ultimate owners
  can be resolved, while circular ownership is rejected in v1. Checkpoints are
  v2 because the graph is required state; v1 checkpoints are not resumable.
- A guarded direct-owner cash-transfer primitive can debit an active firm's
  actual deposit and credit a direct household owner's deposit, with reserve
  settlement across banks. It is callable Rust functionality, not yet a
  scheduled strategic action or a legally/tax-qualified dividend. Malformed
  agent-column lengths or bank/employer indices now fail before settlement,
  stepping, checkpoint load, or resume; a settled owner payment prevents a
  month-zero rewrite through the initial-ownership setter.
- An optional governance process screens scripted proposals through mutable law
  rules, group influence, and amendable institutional gates. Approved economic
  actions apply after a review delay; decisions and effective changes appear in
  the event audit and replay from checkpoints. An opt-in v2 adds a declared
  domain decision authority, separate from the sponsoring influence group;
  changing the authority itself passes the old gate and takes effect after
  review. Opt-in v3 binds a scenario-declared decision actor to a public-office
  appointment and exact jurisdiction, rejecting wrong or expired actors in
  deterministic replay. Legacy v1/v2 retain their serialized meanings.
  Independent adversarial review led to a scenario-horizon guard on resume,
  v3 decision/effective event provenance fields, and a bundle-verification
  cross-check between parsed event audit and the manifest's audit hash.
- Multi-platform CI definition, strict Rust formatting/linting/tests, Python
  tests, and dependency/advisory/licence/source policy.

## Contract only or partial

- Protobuf/gRPC control is specified in
  `proto/openworldgen/v1/simulation.proto`; no production server exists yet.
- Apache Arrow is used through Parquet output. Streaming Arrow IPC transport is
  not implemented.
- `calibrate` is a bounded four-seed workflow smoke test. It is not
  simulation-based inference, parameter recovery, or empirical calibration.
- The current rolling journal retains hashes and bounded audit aggregates. A
  durable streaming sink for every transaction is still required for
  institutional audit deployments.
- The reserve-claim audit is an aggregate identity, not a proof that each bank
  can settle every payment. Negative per-bank reserves currently lack an
  explicit central-bank overdraft/loan treatment; historical bank accounts
  and independent posting reconstruction remain open. The per-bank deposit
  and loan stock checks do not prove event-level journal causality or empirical
  bank balance-sheet correspondence. Loans currently have one
  borrower-assigned bank, not explicit loan contracts, sales, syndication, or
  nonbank creditors.
- Top-level real-state phases now flow through typed scheduled events.
  Individual household/firm mutations inside those handlers are deterministic
  batch transitions rather than separately persisted events. The stage-2
  event-sourcing gate therefore remains open.
- Income accounts expose gross value added, compensation, taxes on products,
  and gross operating surplus, but operating surplus is currently derived as
  value added less compensation. Depreciation, subsidies, mixed income, and an
  independently sourced income account remain open.
- Reference mode is exact on the same build and architecture. Cross-platform
  statistical-tolerance evidence has not yet been produced.
- Only the minimal 1,000-household profile has local performance evidence.
  The 100,000-household standard profile has not been qualified.
- Historical vintage metadata are not yet bound to simulation run manifests or
  result bundles. The new contract is a verifiable input artifact, not a
  completed no-lookahead empirical workflow.
- A rolling-origin benchmark scores persistence and expanding AR(1) against
  next-vintage Philadelphia Fed releases for three macro indicators across
  2007–2023. It is a baseline and diagnostic, not a validated economy forecast.
- The ownership graph alone still does not move cash. The separate direct-owner
  primitive does not compute profit, retained earnings, securities, control,
  pro-rata entitlement, tax treatment, legal authority, or evasion. Its
  acting-entity ID requires a future trusted dispatcher. Neither a graph-only
  edit nor a test transfer is measured wealth concentration.
- Governance influence weights, positions, legal resistance, review rules,
  and v3 appointments are scenario parameters, not US estimates. V3 verifies
  actor claims only against the supplied scenario graph; no external LLM or
  human identity, constitutional authority, or legal hierarchy is verified.
  Legacy policy branches
  remain exogenous interventions and bypass this process. Courts,
  enforcement, electoral representation, unlawful attempts, and lobbying
  expenditures are not yet modelled as separate actors or resource flows.
- A preregistered 12-month no-shock reserve/liquidity sweep passed 39 checkpoint
  and 78 bank-month engineering checks across three seeds, plus a one-minor-unit
  reserve-tamper control. It does not test historical bank solvency, crisis
  liquidity, or macroeconomic stability. Its runtime exceeded the preregistered
  four-minute estimate.
- The local MatrAIx archive passed structural QC, but is not a representative
  US sample and its sparse economic attributes are not calibrated to behavior.
- Recorded policy replay validates typed proposals and exact input identity.
  The bounded direct-owner dispatcher can apply one recorded action against a
  synthetic economy, but corporate-law authority, mutable-law permission,
  scheduled engine dispatch, checkpoint cursor/hash binding, and result-bundle
  integration remain open. The local decision adapter stores inference
  provenance in a sidecar; no runtime engine dispatch consumes it. The v1
  accepted action vocabulary expanded backward-compatibly, so a future strict
  immutable contract should use a new version.

## Gate-locked

- Full structural US historical data ingestion, balanced opening accounts, and
  simulation-versus-baseline historical holdouts; India public-data ingestion,
  Social Accounting Matrix balancing, and simulation-based inference.
- RL training/evaluation, live model-backed strategic decisions, and
  simulation dispatch/evaluation of model decisions.
- Minecraft Fabric sidecar, aggregation/reconciliation, and multiplayer
  authority.
- Multi-region/country, endogenous FX, housing and financial markets, climate,
  procedural cities, geopolitics, and international institutions.

## Current claim

The software is suitable for engineering experiments on accounting,
determinism, interfaces, and qualitative mechanisms. Its economic outputs are
development simulations. They are not estimates, forecasts, policy advice, or
evidence that any branch is a correct decision.

## Local verification snapshot

On 2026-07-24, the local suite passed 48 Rust unit tests, two Rust integration/
property tests, nine Python SDK tests, strict Clippy, schema regeneration
comparison, and dependency/advisory/licence/source checks. A disk
checkpoint/resume matched an uninterrupted run exactly.

The current Apple M3 minimal-reference baseline completed one 120-month branch
in 10.73 engine seconds with 9,830,400 bytes maximum resident set size and a
maximum national-account residual of `7.01e-14`. This is a development
measurement from one machine, branch, and replication; it does not qualify the
standard profile or establish scientific validity.

On 2026-10-14, this host passed strict Clippy and the Rust suite (51 core unit,
one CLI unit, and two integration/property tests) after adding the vintage and
decision-tape contracts. These tests establish the new contract behavior only;
they do not advance the economic or empirical gates.

On 2026-10-18, this host passed 97 core unit tests, two CLI unit tests, three
Rust integration/property tests, strict workspace Clippy, schema regeneration,
and 56 Python research-code tests after the reserve-settlement, owner-cash,
per-bank deposit/checkpoint integrity, adjacent Z.1 probe, BEA matrix QC and
candidate aggregation, local decision-adapter, and v2/v3 authority increments.
The new checks cover graph and governance validity, checkpoint replay,
raw-source hashes, historical-vintage leakage guards, structured decision-tape
generation, and selected accounting identities. These establish software
behavior and source integrity, **not** empirical realism, causal explanation,
or forecast skill.

On 2026-10-20, this checkout passed 105 core unit tests, two CLI unit tests,
three Rust integration/property tests (one CLI and two core), strict workspace Clippy, CLI build,
schema regeneration, and 59 Python research-code tests. The new tests include
per-bank loan/debt stock counterparty checks, local-vintage symlink rejection,
transactional recorded-firm dispatch, and the research-state register auditor.
The Python suite needed the pinned `xlrd==2.0.2` workbook reader restored in
an isolated temporary target; its absence on the first run was an environment
import error, not a failed economic test. These remain engineering and evidence-
integrity results, not historical admission or forecast qualification.

On 2026-10-21, the interrupted opt-in recorded-firm run was completed and the
checkout passed 109 core unit tests, two CLI unit tests, three Rust integration/
property tests, 59 Python research-code tests, and strict workspace Clippy.
One synthetic test exercises one recorded firm decision at month zero, exact
scenario/tape/actor binding, a cash action, no-op replay, and tampered-result
rejection. It is not the ordinary scheduled engine path, a live LLM call,
checkpoint-resumable strategic replay, or empirical behavioral validation.
Saved December 2006 Z.1 and BLS archive captures are candidate pre-origin
bytes only; no independent origin verifier, complete source definition audit,
or balanced US opening state has admitted them as historical inputs.

The later 2026-10-21 data increment collected every official Z.1 release in
the selected 70-quarter range (reported 2006 Q3–2023 Q4): 296 files and
543,878,815 bytes. Local hashes, ZIP CRCs, calendar continuity, and 70 PDF
cover dates/quarters pass. Publication dates are taken from covers, including
the `20170305` path whose cover says March 9, 2017. A 64-table December 2006
extraction preserves 318,207 historical cells, checks 15,983 monetary values
against the archived PDF and 5,812 against the coded PDF, and reconciles
seven issuer/holder margins plus 23 declared source identities within the
reported resolution. The full research Python suite now passes 73 tests.
These source and measurement results still require vintage admission,
instrument-level engine accounts, bilateral allocation, and empirical
calibration; published discrepancies are retained rather than used as cash.
