---
title: OpenWorldGen dated production and spending opening
type: research-increment
status: exploratory
tags: [openworldgen, economics, production, households, calibration]
---

# Dated production and spending opening

The experiment tests whether a sector mix constrained by private payrolls and a household basket constrained by actual expenditure improve the model's sector behavior and real output growth. Compare matched seeds, origins, and bank openings. Keep the historical shock presets off. Score realized household purchases and employment by sector and quarterly real GDP growth. Compare each to the unchanged engine and to carrying the corresponding opening shares or growth rate forward.

## Coverage and transformations

- Use the four latest available quarterly nominal PCE observations in each origin vintage. Food covers food and beverages purchased for off-premises consumption. Energy covers energy goods and services. Manufacturing is a spending proxy for goods after subtracting food and energy goods. Services are the remainder after removing energy services. The nominal components reconcile up to publication rounding. These groups differ from production industries. [BEA table 2.3.5](https://fred.stlouisfed.org/release/tables?eid=5125&rid=53)
- From the same dated archive, use latest monthly private nonfarm payrolls. Food covers food manufacturing. Energy uses oil and gas extraction and utilities as proxies. Manufacturing excludes food manufacturing. The residual includes services, construction and other private nonfarm employment. CPS employed agents are placed using these payroll shares, so the placement assumes the private payroll mix applies to the modeled workforce. [BLS Current Employment Statistics](https://www.bls.gov/ces/)
- The model allocates firms by payroll share with a one-firm floor per sector. Firms are representatives with no inferred legal or ownership identity. Opening worker counts use the payroll proportions at the modeled scale. Nominal planned demand is redistributed by PCE shares while preserving the total development-model scale. Initial inventory and capacity retain the same multiples of planned demand. Productivity is inferred as planned units per opening worker. That quantity is a modeling assumption and has no empirical productivity unit.
- Households apply the fitted basket in their weekly purchases, with energy and food floors rescaled in proportion to the new shares. The same weights inform the model price index. The official CPI has a different basket. The production profile keeps the existing government, foreign, and financial behavior. Reentering firms retain their sector-scaled capacity and productivity.
- The scoring vintage is February 2, 2024. Score the first 12 months of each origin using 8 matched seeds. Bank mode is held constant within each origin. Sector metrics report mean absolute percentage-point error across the four categories. PCE observations are quarterly and evaluated against the sum of the three simulated months. Evaluate annualized real GDP growth after two complete simulated quarters and compare with the last observed growth rate available at each opening. Payroll and GDP reports are comparisons, with output observed in different economic units.


## Results

| Mean absolute error (percentage points) | Periods | Dated sector opening | Synthetic sectors | Carry forward opening |
| --- | ---: | ---: | ---: | ---: |
| Private payroll share | 72 months | 0.145 | 31.775 | 0.076 |
| Household spending share | 24 quarters | 1.771 | 17.792 | 0.836 |
| Annualized real GDP growth | 18 quarters | 22.180 | 19.567 | 8.956 |

The reconstructed sector book materially improves the simulated mix. Persisting the observed mix still wins both composition comparisons. The profile also worsens aggregate real growth error, including outside the pandemic origin. The evidence supports the starting sector reconstruction but fails the forecast-improvement test. The engine's physical capacity, wage setting, aggregate demand, inventories and adjustment dynamics still lack a matched structural fit. A next test should decompose the quarterly growth errors into capacity, sales and inventory changes, then test lagged adjustment or a historically conditioned warm-up using only pre-origin information.

A live run with both dated sector and bank inputs reached December 2030. The 2028 and 2030 baseline runs were identical over the common months. A COVID plus AI path reached 2028. All four tested paths reconciled household sector spending to the full household total, and each monthly ledger residual was zero.

```sh
cargo build --release -p owg-cli
python3 research/us-economy-2007-2023/code/collect_production_opening.py
python3 research/us-economy-2007-2023/code/historical_lab.py --binary target/release/owg --finance --production --score
python3 research/us-economy-2007-2023/code/serve_historical_lab.py --binary target/release/owg --finance --production
```

Use **Sector opening** to switch the input before a new run. **Sector composition** shows simulated, observed, and opening shares. The household spending view compares completed quarters. Evidence is saved under `artifacts/historical-economy-v4/`.
