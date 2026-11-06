---
title: OpenWorldGen inventory adjustment and output measurement
type: research-increment
status: exploratory
tags: [openworldgen, economics, production, inventories]
---

# Inventory adjustment and output measurement

The prior sector opening improved the employment and spending mix but worsened the GDP forecast. In a seed-zero 2020 opening, nominal firm sales fell from 424,700 OWG in January to 296,600 in June while real GDP rose from 245,900 to 289,500 OWG. The engine fulfilled some of the early sales from its large opening stocks. It also counted unsold service capacity as durable inventory and valued imported energy using the producer price of domestic energy.

## Experimental comparisons

The accounting diagnostic records gross output at opening prices, imports at their own opening price, inventory and sales values, and service slots that expired without a sale. The alternate real-value-added series subtracts base-price imported inputs from base-price output. It is a model measure, not the BEA chain-weighted GDP measure.

The inventory candidate uses four [Census M3](https://www.census.gov/manufacturing/m3/definitions/index.html) series archived by vintage. Finished-goods stock and monthly shipments come from AMTMFI and AMTMVS for manufacturing and A11SFI and A11SVS for food. Subtract food from total manufacturing for the model's other manufacturing proxy. Each origin uses 60 observations available by the opening. Estimate a target in months of sales from the median ratio. Fit the next-month stock change against the lagged gap to that target by least squares through the origin, bound the coefficient to [0, 1], and retain the raw estimate. This is an aggregate response proxy and does not identify firm-level structural adjustment or remove valuation effects.

The candidate initializes food and manufacturing stocks from the latest observed ratios and replenishes toward the fitted targets. Unsold service slots expire before the monthly close and are excluded from recorded production. Energy stock rules remain as in the sector opening. Hold bank openings, labor input, seeds, origin, and horizon constant. Compare the published prior engine GDP measure, the same run with corrected valuation, the inventory candidate with corrected valuation, and last-observed growth persistence. Evaluate both industry stock-to-sales ratios against archived observations and carry-forward ratios. Quarterly changes in gross output and import inputs are decomposed at fixed prices with the prior quarter's value added as denominator.


## Results

| Mean absolute error | Inventory trial | Prior dynamics with fixed import prices | Prior published run | Opening persistence |
| --- | ---: | ---: | ---: | ---: |
| Annualized real GDP growth, percentage points (18 quarters) | 27.661 | 24.878 | 22.180 | 8.956 |
| Stock-to-sales, months (72 months across two groups) | 0.621 | 0.253 | 0.253 | 0.028 |

Average trial manufacturing stocks reached 1.559 months of sales against 0.538 observed. Average food stocks reached 0.615 months against 0.452 observed. In the 2020 opening, the prior model's fixed-price Q2 growth of 55.1% annualized decomposes into 12.87 quarterly percentage points from gross output and −1.27 from imports. The trial's Q2 rate of −38.1% decomposes into −13.89 points from gross output and +2.59 from imports. The trial thus reverses the direction of the early output swing and fails the average forecast-improvement test. The fitted adjustment acts on a mis-scaled opening sales plan, and expiry exposes the mismatch between planned and delivered service output. Separating stock dynamics and service expiry into their own ablations would further distinguish these effects.

A separate synthetic settling check ran 24 months at six openings and four seeds without scoring against future observations. Mean absolute annualized GDP changes in quarters 2–4 were 18.28 points in the first modeled year and 22.79 in the second for the prior dynamics. They were 24.80 and 24.74 in the trial. Average unemployment also rose from 14.59% to 19.75% in the prior dynamics and from 15.07% to 20.66% in the trial. A passive warm-up of this system does not resolve the drift. A conditioned start needs changes to demand, hiring, or both. See `settling-diagnostic.json`.

The default dashboard keeps the dated sector mix. Select **Dated mix + inventory trial** to run the candidate. **Production accounts** shows the fixed-price output bridge and the monthly Census M3 comparisons. Evidence lives under `artifacts/historical-economy-v5/`. The two trial baseline horizons reached December 2028 and December 2030 with identical overlapping frames. The COVID and AI scenario reached December 2028. Nominal production identities and ledger closure held at every checked step, including the month an import price shock was applied.

```sh
cargo build --release -p owg-cli
python3 research/us-economy-2007-2023/code/collect_inventory_adjustment.py
python3 research/us-economy-2007-2023/code/historical_lab.py --binary target/release/owg --finance --production --inventory --score
python3 research/us-economy-2007-2023/code/serve_historical_lab.py --binary target/release/owg --finance --production --inventory
```
