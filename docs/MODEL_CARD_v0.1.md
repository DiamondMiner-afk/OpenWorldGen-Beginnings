# Model card: small open economy v0.1

## Identification

- Model pack: `owg-small-open-economy@0.1.0`
- Validation status: `development`
- Geography/data: synthetic `SYNTHETICA`, currency `OWG`, base year 2020
- Intended engine: OpenWorldGen 0.1.x

The exact model-pack, data-bundle, scenario, policies, engine, and output hashes
are written into each run manifest.

## Intended use

The model supports software verification and conditional scenario experiments
about transmission of a temporary imported-energy-price shock. It can compare
policy branches under identical random streams and expose mechanisms and
trade-offs.

It must not be used for real-world forecasts, numerical policy
recommendations, credit decisions, allocation of benefits, or claims about
individual people or firms.

## Agents, markets, and timing

The minimal profile contains 1,000 weighted households, 100 heterogeneous firms
across energy, food, manufacturing, and services, two banks, one government,
one central bank, and an explicit rest-of-world boundary.

Household goods markets execute weekly. Production, payroll, taxes, debt
service, credit, defaults, transfers, and firm planning execute monthly. Policy
rules update quarterly while discretionary branch deltas are applied monthly.
The typed scheduler records every completed phase in a separate schedule hash.
Goods use posted prices and bounded search; labor uses vacancy search;
deposits, loans, taxes, transfers, bonds, defaults, and bank resolution settle
through double-entry accounts.

Households differ in skills, liquidity, habits, consumption propensity, risk
preference, information access, trust, expectations, employment, and debt.
Firms differ in sector, productivity, capacity, inventories, price, wage,
markup, energy intensity, leverage, expectations, and viability. Behavioral
updates are deterministic conditional on named random streams and prior state.

## Data and calibration

All current values are synthetic development assumptions. The data bundle
contains four sector rows and complete provenance/checksum metadata. Model
parameters are point values, not an estimated posterior. No confidential or
personal data is present.

No empirical calibration, parameter-identification analysis, synthetic
parameter-recovery study, holdout evaluation, or forecasting comparison has
passed.

## Verification achieved

- Every accepted journal entry has exactly zero fixed-point residual.
- Randomized transfer sequences preserve financial closure.
- National-account production and expenditure residuals are checked at every
  month and invalidate a run above the declared tolerance.
- The development income decomposition and institutional-sector stock
  residuals are reported and checked every month.
- Typed scheduler order is hashed, checkpointed, and compared during replay.
- Repeated reference runs have identical metric hashes.
- Checkpoint/resume matches uninterrupted execution.
- Forced household default and bank-resolution paths preserve closure.
- Invalid contracts and failed operations produce errors or failure manifests
  rather than partial plausible output.

These are software and accounting checks, not external economic validation.

## Known limitations and failure regions

- Four sectors and one aggregate geography omit most production-network and
  spatial structure.
- Rest-of-world prices, demand, capital conditions, and exchange rates are
  exogenous.
- Rules are stylized and several behavioral coefficients remain hard-coded
  development assumptions.
- Goods matching is bounded search, not welfare-maximizing market clearing.
- Potential output is a development trend estimator, not an econometrically
  identified latent series.
- Uniform household weights in the default bundle do not reproduce a real
  population.
- Policy scores are sensitive to objective definitions, evaluation windows,
  parameter uncertainty, and model structure.
- Rare collapses and tail risks require substantially more replications than a
  smoke run.
- Scale convergence and cross-platform statistical equivalence remain open.

Any invariant failure, non-finite state, unmatched input hash, failed
calibration, or insufficient Monte Carlo precision must limit or invalidate the
run.

## Responsible interpretation

Outputs answer: “what happened in this synthetic model under these declared
assumptions?” The Pareto frontier and minimax-regret branch are diagnostics.
A single recommendation requires decision-owner objectives and constraints,
plus a model version that has passed the relevant scientific gate.
