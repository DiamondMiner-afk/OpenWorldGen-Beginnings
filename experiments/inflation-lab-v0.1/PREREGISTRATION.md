# Inflation laboratory v0.1 protocol

Protocol status: locked draft for the first formal synthetic validation run.
Engineering smoke runs made before a repository tag are excluded from
scientific evidence. The formal run must record the tag/commit, scenario hash,
model hash, data hash, engine build hash, platform, and every seed.

## Question

Under the declared synthetic small-open-economy model, how do six policy
branches trade off inflation, output, employment, distribution, defaults, and
fiscal cost after a temporary imported-energy-price shock?

The study does not test a claim that one branch is universally best or that the
simulated trajectory predicts a real economy.

## Fixed design

- Population: 1,000 households, 100 firms, two banks, government, central bank,
  and rest of world.
- Horizon: months 0 through 119.
- Pre-shock period: months 0 through 23.
- Shock: imported energy price multiplier 1.30 in months 24 through 35,
  followed by linear decay in months 36 through 47.
- Primary evaluation window: months 24 through 59.
- Branches: baseline, temporary 100-basis-point tightening, bottom-40-percent
  transfer, temporary energy indirect-tax reduction, energy-efficiency public
  investment, and the declared combined policy.
- Common random numbers: replication `r` uses the same named actor/module
  streams in every branch.
- Replications: begin with 8 and add batches of 8 up to 64. Stop only when every
  branch/objective 90% interval has relative half-width at most 5%; a
  near-zero mean instead requires near-zero absolute half-width. If the target
  is not reached, report the limitation without changing the rule.

The machine-readable definitions in
`scenarios/v0.1/inflation_lab.yaml` are authoritative for amounts and effective
dates. Monetary values there are integer `OWG` minor units.

## Primary objective losses

All objectives are minimized over the primary evaluation window:

1. Inflation: mean absolute year-over-year CPI inflation.
2. Output: mean negative output gap, with positive gaps assigned zero loss.
3. Employment: mean unemployment share.
4. Distribution: mean of `0.5 * Gini + 0.5 * poverty_share`.
5. Defaults: mean monthly firm defaults plus household defaults plus ten times
   bank resolutions.
6. Fiscal cost: mean positive fiscal deficit, with surpluses assigned zero.

The primary presentation is the six-dimensional Pareto frontier. Normalized
worst-objective regret is a robustness diagnostic. It is not a policy
recommendation.

## Secondary outcomes and mechanisms

Report CPI by sector, real and nominal GDP, wages, consumption deciles,
inventories, household/firm/bank failures, credit, fiscal balance, public debt,
imports, production/expenditure/income identity errors, institutional-sector
financial positions, peak-inflation month, sector price changes at the peak,
peak credit, and cumulative deficits/defaults. Secondary outcomes are
descriptive and do not replace the primary objectives.

## Required validity analyses

Before closing the synthetic gate:

- no-shock stability distribution and explicit collapse taxonomy;
- accounting and transaction trace for every failed run;
- global sensitivity and alternative behavioral specifications;
- ablation of expectations, habits, peer learning, credit, fiscal, and monetary
  channels;
- synthetic parameter-recovery coverage;
- comparison with stock-flow-consistent and published macro-ABM examples;
- minimal/standard scale convergence and policy-ranking agreement;
- repeatability on macOS, Linux, and Windows.

## Exclusions and deviations

Runs with invalid input hashes, invariant failures, non-finite values, or
incomplete manifests are excluded and counted as failures. No outcome may be
silently winsorized, dropped, or rerun with a favorable seed. Any protocol
change creates a new version and a written deviation record; it must not
overwrite this protocol.
