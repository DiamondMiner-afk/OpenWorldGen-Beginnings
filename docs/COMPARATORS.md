# External and canonical comparators

## Implemented

`crates/owg-core/tests/sfc_comparator.rs` reproduces the canonical commercial
bank money circuit: loan origination creates matched loan/deposit positions,
wages transfer deposits without creating money, and principal repayment
destroys matched assets and liabilities. A randomized multi-account transaction
suite checks the global identity after every posting.

These are accounting comparators, not evidence that the behavioral model is
empirically valid.

## Required before the synthetic gate closes

- reproduce a published small macro-ABM impulse response with a documented
  parameter bridge;
- replicate BeforeIT-style micro-behavior, accounting-identity, and
  statistical-equivalence tests separately;
- register tolerances before comparing outputs;
- record any mismatch as a model difference or failure, never silently retune
  the comparator after seeing results.
