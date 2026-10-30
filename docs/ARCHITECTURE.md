# Architecture

## Boundaries

The Rust core owns time, state, accounting, markets, deterministic randomness,
checkpoints, and metrics. Research clients submit versioned scenarios and read
result bundles. Future Minecraft code is a projection and control adapter; it
never becomes the authoritative economic state.

## Deterministic execution

`MultiRateScheduler` emits typed events ordered by simulation month,
within-month phase, priority, actor identifier, and sequence. Completed events
feed a rolling schedule hash that is checkpointed and reported separately from
the financial journal. Random values come from streams derived from the root
seed, namespace, actor, and time index. Reference reductions are stable and
ordered.

## Accounting

Financial accounts use signed fixed-point minor units. Assets use positive
balances; liabilities and equity use negative balances. Every journal entry
must sum to zero before it can be committed. Real inventories are tracked in
typed quantities and reconciled with production, trade, consumption, loss, and
investment flows.

The in-memory ledger retains current balances plus a cryptographic rolling
journal hash and bounded phase/month counters. It does not retain millions of
expanded transaction objects. Engine-generated event labels include a
monotonic sequence, and immediate duplicate submission is rejected. Durable
full-event evidence will use an optional streaming audit sink rather than
unbounded RAM.

## Extension boundary

Agent policies receive immutable batched observations and return typed actions.
The model validates actions and converts accepted actions into events and
journal entries. An RL model, LLM, rule, or human interface therefore cannot
bypass physical, accounting, policy, or authorization constraints.
