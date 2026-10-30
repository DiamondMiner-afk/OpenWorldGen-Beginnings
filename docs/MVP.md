---
title: OpenWorldGen MVP — run and evaluate
status: runnable-development
as_of: 2026-10-24
tags: [openworldgen, mvp, runbook]
---

# Runnable MVP

Two working paths now exist. They must not be confused:

1. **Real-US forecasting actor:** local Qwen predicts measured US macro series
   from past release-vintage histories, with subsequent outcomes withheld from
   its requests. Graphs overlay agent forecasts, observed outcomes, persistence,
   and expanding AR(1). This is an exploratory retrospective holdout, not a
   calibrated simulation of the US economy.
2. **Economic-engine MVP:** households, firms, banks, accounts, credit contracts,
   mutable scenario laws, policy branches, checkpoints, and a bounded strategic
   actor experiment run end to end. Its opening state remains synthetic.

## Run the real-US holdout

From the repository root, with Python 3, `llama-server` on PATH, the copied GGUF
in `local-models/`, and the cached Philadelphia Fed vintages present:

```bash
python3 research/us-economy-2007-2023/code/us_agent_holdout.py \
  --output research/us-economy-2007-2023/artifacts/my-us-holdout
python3 research/us-economy-2007-2023/code/render_us_holdout.py \
  research/us-economy-2007-2023/artifacts/my-us-holdout \
  --output runs/my-us-holdout.html
```

Choose new output paths; existing experiments are not overwritten. No paid API
or model download is needed. The script owns a loopback-only model server and
terminates it when finished. On this host the recorded 15-call test took about
117 seconds; this is a measurement, not a guaranteed runtime.

The fixed test uses 15 rolling origins from 2020Q1 through 2023Q3: 45 predictions
covering annualized quarterly real GDP growth, unemployment, and 12-month CPI
inflation. At each origin, only 32 past observations per indicator are sent.
Dates are masked; future target labels are not sent. Predictions are saved
before the target vintage is loaded for scoring. The frozen protocol records
the model, code, input hashes, prompt, and scoring rule. Invalid responses are
explicitly flagged persistence fallbacks, never silently called AI successes.

Observed outcomes are the next quarterly vintage's latest values, not today's
revised final values or necessarily their first public releases. CPI observation
horizons vary from two to four months. Graph dates are outcome-vintage dates;
the paired table retains actual target periods and horizons. Units and error
signs are explicit; unavailable intervals remain missing rather than zero.

**Important limitation:** withholding labels from requests does not remove
historical knowledge from pretrained weights. This is not contamination-free
forecast validation. The analyst has seen these historical episodes, and the
cached vintages have not passed independent archive-origin admission.

## Run the economic engine

Requires the local Rust toolchain and Python 3. The runner builds the CLI:

```bash
python3 scripts/run_mvp.py --output runs/my-mvp
```

For the bounded live-Qwen executive experiment as well:

```bash
python3 scripts/run_mvp.py --llm --output runs/my-mvp-with-qwen
```

Default: 120 fictional months, 96 households, 12 firms, two banks, three policy
branches and two replications. A smaller smoke run is:

```bash
python3 scripts/run_mvp.py --months 12 --replications 1 --output runs/my-smoke
```

The runner validates the scenario and result bundle, compares serialized
checkpoint/resume against an uninterrupted run, verifies a law-change lending
rejection, executes six adaptive firm decisions, and replays the final tape
exactly. Offline mode uses a clearly labelled deterministic reference policy;
`--llm` makes actual local model calls. The executive currently chooses only
no-op or a constrained direct-owner cash transfer—not pricing, employment, or
investment. This experiment is separate from the ordinary result bundle.

Open `economy/plots.html` for interactive metrics and SVG export. Synthetic
graphs explicitly state that no matching real-world outcomes exist. Do not
overlay arbitrary US numbers on those fictional paths.

## Verify an existing MVP without model calls

```bash
python3 scripts/verify_mvp.py runs/my-mvp \
  --output runs/my-mvp-verification.json
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
python3 -m unittest discover -s scripts -p 'test_run_mvp.py'
```

The verifier rechecks the bundle, checkpoints, creditor law rejection, and a
fresh strategic replay and records file hashes. These are engineering checks,
not evidence of economic realism.

## Saved October 24 results

- `runs/mvp-20261024-qwen/economy/us_agent_holdout.html`: real-US forecast/actual
  overlay with indicator selector, baseline lines, intervals, exact values,
  source link, and paired CSV download.
- `research/us-economy-2007-2023/artifacts/us-agent-holdout-20261024/`: frozen
  protocol, 15 raw requests/responses, 135 paired agent/baseline rows, scores,
  and graph audit. All 15 Qwen calls were accepted without fallback.
- `runs/mvp-20261024-qwen/`: successful 120-month engine demonstration and
  six live-Qwen executive choices. Those six choices all retained cash; a
  separate deterministic reference run executed three owner transfers.

The US agent's claimed 80% intervals covered only 6/15 GDP, 7/15 unemployment,
and 5/15 inflation outcomes. Its GDP line largely reverted toward normal
growth and missed the pandemic collapse/rebound. A lower average error than
these two baselines does not establish realistic crisis mechanisms or reliable
prediction. Do not tune this test after seeing its outcomes; use fresh holdouts
and stronger comparators for the next experiment.

## Remaining scope

No balanced measured US agent-economy opening state, trained RL policy,
representative runtime MatrAIx population, securities/derivatives market,
collateral bankruptcy model, full political enforcement, international economy,
Fabric edition, or forecast qualification is delivered by this MVP. The full
research objective remains active; this is a runnable starting point.
