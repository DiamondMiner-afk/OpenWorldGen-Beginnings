# Model governance

Every distributable run bundle contains:

- engine, schema, model, data, scenario, and policy versions and hashes;
- the root seed and named random-stream derivation version;
- determinism and fidelity classes;
- validation status and warnings;
- the host platform and execution mode;
- metric data, audit events, and checkpoint lineage.

No confidential row-level data may be sent to external AI services. Learned
policies are frozen and hashed for evaluation. Plugins receive typed
observations and return typed proposals; they cannot write directly to the
ledger or world state.

## Validation statuses

- `development`: mechanics are incomplete or changing;
- `synthetic_validated`: correctness, recovery, stability, scale, and
  performance gates pass on synthetic data;
- `empirically_calibrated`: a named real-data vintage has been calibrated;
- `scenario_qualified`: holdout and uncertainty gates support the declared
  scenario use;
- `forecast_qualified`: preregistered forecasting thresholds pass;
- `invalid`: a correctness or integrity condition failed.

Status is attached to a specific model and data hash, never to the project name
in general.
