---
title: OpenWorldGen historical labor lab
type: research-increment
status: exploratory
tags: [openworldgen, labor, calibration, dashboard]
---

# Historical labor lab

This experiment asks whether historical worker movements plus simulated firm hiring can predict U.S. unemployment more accurately than the same historical information used by simpler models. The unit is a month. The labor-fit input and observed outcomes stay in separate files. The Rust engine receives only the scenario and fitted labor input.

## Run the dashboard

From the OpenWorldGen repository root:

```sh
cargo build --release -p owg-cli
python3 research/us-economy-2007-2023/code/serve_historical_lab.py \
  --binary target/release/owg \
  --data research/us-economy-2007-2023/artifacts/historical-labor-v1-release
```

Open [U.S. Labor Lab](http://127.0.0.1:8765/). Pick an opening year and seed. **Play** advances one scheduled month at a time. **Step** runs exactly one month. The slider revisits computed states. Compare to the opening rate, an AR(1), or a model that applies the fitted transition probabilities directly to population shares. Each session can run for up to 36 months with a single opening state.

## Data and fitting

- [BLS CPS labor-force flow definitions](https://www.bls.gov/cps/cps_flows.htm). BLS tracks movement between employed, unemployed, and outside the labor force. ALFRED archives each seasonally adjusted series with a vintage date. The collector uses one request per series because a multi-series graph request can mix the requested archive with current vintages.
- For each opening, the input is the January employment-release vintage and its history through December. The most recent 60 monthly transition counts are summed separately by origin and destination status. Dividing by each origin status total gives the three probabilities for that row. The December CPS participation and unemployment rates determine the three opening population counts, subject to rounding at 1,000 household agents.
- Six opening vintages cover 2016, 2018, and 2020–2023. The 2024-02-02 vintage supplies comparison outcomes through December 2023. The January release cutoffs provide an archived information set. Archive hashes bind the retrieved files and do not certify exact bytes in a contemporaneous third-party capture.
- The actors share fitted rates within each employment status. The run assigns the opening workers to firms and chooses which individuals occupy each status by seed. Employer links, incomes, wealth, firm balance sheets, credit, prices, and production remain synthetic. A household is the engine's proxy for one working-age person.
- A household draws its next proposed status from its current status row. Moves into employment require a match with one of the firms searched, up to the firm's simulated staffing plan. The reported matrix is the change from the prior close to the current close, so it incorporates firms that failed after matching. People outside the labor force do not receive the engine's unemployment benefit. They continue to participate in the rest of the economy under its development assumptions.

To regenerate source files and run the frozen exploratory comparison:

```sh
python3 research/us-economy-2007-2023/code/collect_labor_vintages.py
cargo build --release -p owg-cli
python3 research/us-economy-2007-2023/code/historical_lab.py \
  --binary target/release/owg \
  --output research/us-economy-2007-2023/artifacts/historical-labor-v1-release \
  --score
```

## Evaluation

The primary metric is mean absolute unemployment error across the first 12 months of each origin. Use eight fixed seeds and average the agent-engine rate before scoring. Report per-origin and 1-, 3-, and 12-month results alongside persistence, iterated AR(1) fit to the same 60 monthly stocks, and a deterministic transition-only control fit to the same 60 monthly flows. The latter isolates the contribution of engine interactions. Min/max across seeds measure run-to-run variation only.

The design was set before executing these runs. The historical episodes were already known, so these are exploratory holdouts with separated data and outcomes. Forecast qualification would require consistently better performance over multiple untouched origins, cross-checks of the simulated worker flows and other mechanisms, and a calibrated uncertainty evaluation. The project-wide requirements for balanced US initialization and provenance remain in [[US_OPENING_STATE_GATE]].

## Observed result

The release build ran 576 scored engine months across six origins and eight seeds. The mean across seeds was scored against 72 distinct observed months.

| Model | MAE (percentage points) |
| --- | ---: |
| Rust agent engine | 1.769 |
| Carry forward the opening unemployment rate | 1.122 |
| AR(1) | 1.227 |
| Fitted flows only | 1.750 |

The engine accepted 99.98% of its applications. It generated 304.4 open slots against 25.4 applications per step on average. Staffing demand rarely constrained the status draws. The flows-only comparator had slightly lower error and the persistence comparator performed substantially better. This candidate fails the forecast-improvement test.

The next fit needs an observable for actual firm hiring capacity. The BLS JOLTS evidence gathered for each existing origin gives an end-of-month opening stock of 3.77%–7.10% of nonfarm payroll jobs. The engine's open slots and the JOLTS survey stock have different timing and scope. Use the JOLTS openings stock alongside the separate monthly payroll hires and layoff flows to constrain firm postings and fill rates. Test the revised mechanism against the flows-only comparator and quantify sensitivity to sector scope and the synthetic firm opening state.

```sh
python3 research/us-economy-2007-2023/code/collect_firm_labor_vintages.py
```

Release evidence lives in `artifacts/historical-labor-v1-release/`. The dashboard displays scores only when their protocol and score file hashes match its prepared inputs. Software checks cover current Rust regressions, exact two-frame replay after poisoning the observed outcome file, zero-vacancy hiring behavior, and 36 live frames with closed ledgers. Desktop browser checks cover manual steps, controls, graph overlays, and the run ending.
