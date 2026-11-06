---
title: OpenWorldGen live U.S. economy lab
type: research-increment
status: exploratory
tags: [openworldgen, economics, calibration, dashboard]
---

# Live U.S. economy lab

The dashboard requires local historical inputs. The source branch includes the
collectors and extraction code. For a fresh checkout, save the Philadelphia Fed
RTDSM workbooks `ROUTPUTQvQd.xlsx`, `rucQvMd.xlsx`, and `cpiQvMd.xlsx` under
`research/us-economy-2007-2023/data/raw/philadelphia-fed/` and install
`openpyxl` in the Python environment. Run the preparation commands below
before starting the server. [Philadelphia Fed RTDSM](https://www.philadelphiafed.org/surveys-and-data/real-time-data-research/real-time-data-set-for-macroeconomists)

From the repository root:

```sh
cargo build --release -p owg-cli
python3 research/us-economy-2007-2023/code/serve_historical_lab.py --binary target/release/owg --finance --production --inventory
```

Open [U.S. Economy Lab](http://127.0.0.1:8765/). Pick the opening year, end year, scenario, and seed, then click **New run**. The **Bank opening** selector switches between dated bank ratios and the synthetic control when the chosen origin has evidence. See [[BANK_OPENING_RECONSTRUCTION]]. The **Sector opening** selector uses dated payroll and consumption shares or the synthetic sector control. See [[PRODUCTION_OPENING_RECONSTRUCTION]]. The optional inventory trial and production-account graphs are documented in [[INVENTORY_AND_OUTPUT_DYNAMICS]]. The default opens in 2020 and continues through 2028. The end year is selectable through 2100. **Play** and **Step +1 month** both compute engine frames. Use **Labor & validation** to inspect worker transitions and forecast errors.

The overview shows quarterly real GDP growth, consumer-price inflation, credit, reserves, fiscal balances, imports, sector sales, employment, and bank balance sheets. Simulated levels use OWG model units. Observed GDP growth and inflation come from the 2023Q4 Philadelphia Fed vintage and end with that vintage's series coverage. Observed unemployment comes from the February 2024 CPS vintage and ends in December 2023. Observation values enter the display only. Runs continue with their existing agent state beyond the archive.

## Scenario inputs

- **Endogenous baseline** uses the opening information set and agent decisions without scenario shocks.
- **COVID replay** starts in April 2020. It applies a three-month reduction in household confidence, service capacity, and manufacturing capacity, then decays over nine months. The historical timing is informed by the [April 2020 BLS release](https://www.bls.gov/news.release/archives/empsit_05082020.htm). The amplitudes and recovery path are experiment assumptions. Use a start no later than 2020.
- **AI adoption scenario** increases services productivity by 10% and manufacturing productivity by 5% from January 2026. The change affects production and labor requirements. These are hypothetical productivity assumptions. [Census business AI usage evidence](https://www.census.gov/library/stories/2026/05/ai-use-businesses.html) can inform future adoption paths by industry and firm size. Adoption shares and productivity effects are separate quantities.
- **COVID replay + AI scenario** combines both event packages. The event descriptions and dates appear with the run. Scores in the validation view come from the separate baseline backtest.

## Firm recruiting and evaluation

The Rust engine carries forward unfilled firm vacancies, withdraws slots when plans change, and posts new slots based on each firm's planned staffing. Workers contact a bounded sample of firms. The engine reports opening, withdrawn, added, filled, and closing slots. The baseline worker-only policy uses the same CPS fit, seed and engine with this new recruiting mechanism disabled.

The fit uses the most recently available JOLTS openings per payroll job for initial vacancy stock and the last 12 common months of payroll hires and layoffs to scale withdrawal and stress sensitivity. The new-posting flow uses CPS accessions per employed person plus an allowance for withdrawals. The contact adjustment uses an approximate chance of locating a vacancy, based on stock and accession rates. Four independent employer draws and the withdrawal coefficient are modeling assumptions. JOLTS hires are payroll events, whereas CPS accessions count transitions into employment. They are used separately. [BLS JOLTS definitions](https://www.bls.gov/jlt/jltdef.htm) · [BLS CPS transition definitions](https://www.bls.gov/cps/cps_flows.htm)

| Model | Unemployment MAE (percentage points) |
| --- | ---: |
| Agent engine with recruiting | 2.826 |
| Worker-only engine | 1.769 |
| Opening-rate persistence | 1.122 |
| AR(1) | 1.227 |
| Fitted flows only | 1.750 |

Across six origins and eight matched seeds, recruiting reduced the application fill rate from 99.98% to 59.71%. The mean simulated closing vacancy stock was 3.20% of simulated employed people, against an observed 5.59% per payroll job. Those denominators differ. The new vacancy mechanism made firm decisions matter but did not improve unemployment prediction. This is exploratory evidence on known historical episodes. Wider tests require a better vacancy formation mechanism, cohort and firm distributions, measured bank opening stocks, and production calibration. See [[HISTORICAL_LABOR_LAB]] and [[US_OPENING_STATE_GATE]].

Prepare the historical inputs and compute the archived comparison:

```sh
python3 research/us-economy-2007-2023/code/extract_philadelphia_vintage.py --origin 2023Q4
python3 research/us-economy-2007-2023/code/collect_z1_release_history.py
python3 research/us-economy-2007-2023/code/collect_labor_vintages.py
python3 research/us-economy-2007-2023/code/collect_firm_labor_vintages.py
python3 research/us-economy-2007-2023/code/collect_bank_opening.py
python3 research/us-economy-2007-2023/code/collect_production_opening.py
python3 research/us-economy-2007-2023/code/collect_inventory_adjustment.py
python3 research/us-economy-2007-2023/code/historical_lab.py --binary target/release/owg --finance --production --inventory --score
```

The [household demand trial](HOUSEHOLD_DEMAND_AND_OPENING_PLANS.md) has a
separate CLI backtest. Use `collect_household_behavior.py` to prepare its
additional inputs. It is an offline experiment.

Verification exercised 108 months through December 2028 for baseline and combined events and 132 baseline months through December 2030. The combined case matched the baseline up to March 2020 and diverged in April. The overlapping baseline frames were identical between the two end years. The backtest and live checks verified vacancy stock reconciliation and zero ledger residuals. The interactive overview and labor tab were exercised in desktop and narrow Chromium views.
