---
title: OpenWorldGen dated bank opening reconstruction
type: research-increment
status: exploratory
tags: [openworldgen, economics, banking, calibration]
---

# Dated bank opening reconstruction

The economy lab can initialize two aggregate commercial bank agents from dated financial observations. The method projects measured balance-sheet ratios into the model's monetary scale. It preserves the total deposits of the synthetic control, so the experiment changes the composition and distribution of the opening book. The individual assignments of borrowers and depositors remain synthetic.

## Evidence and mapping

1. Use the latest common monthly observation in each origin's H.8 vintage for large and other domestically chartered commercial banks. Keep deposit, asset, liability, cash, consumer-loan, and commercial-and-industrial-loan series separate. The groups are sector aggregates with changing membership. [Federal Reserve H.8 coverage](https://www.federalreserve.gov/releases/H8/about.htm)
2. Compute the reserve-to-deposit ratio from the preceding September's L.111 Z.1 release, using chartered depositories' reserves at the Fed divided by checkable and time and savings deposits. Apply the ratio to the H.8 deposit total. Distribute reserves using H.8 cash asset shares and require that total imputed reserves fit within total cash. This bridges different survey dates and scopes and leaves the two bank cohorts' actual reserve allocations unknown. [Example Z.1 edition](https://www.federalreserve.gov/releases/z1/20191212/z1.pdf)
3. Split model deposits by H.8 cohort shares. Allocate the deposits and matched household and business loans within each cohort using the existing synthetic account weights. Use cumulative integer allocation so posted amounts match the cohort targets to the cent. Match reserves to the central bank and every loan to a borrower in the assigned bank. Treasury opening cash is inherited from the development model.
4. Record gross other assets and liabilities separately as the portion of reported totals left outside the covered instruments. Their opposing claims are carried in separate opening accounts in the accounting sector. They are not spendable assets or assigned exposures. Equity is reported assets less liabilities, a book net-assets proxy rather than a measure of regulatory capital. Each cohort satisfies assets = liabilities + equity exactly.

The four usable origins are 2020, 2021, 2022, and 2023. The local 2015 and 2017 Z.1 downloads report modification dates after the corresponding January cutoff, so those earlier labor origins use synthetic banks. ALFRED H.8 headers and raw source hashes are checked. Z.1 files match the publisher's dated-release receipts, but their exact contemporaneous bytes lack independent authentication. This is a publication-level reconstruction.

## Paired comparison

Eight fixed seeds at each of four openings were run for 12 months with dated bank balances and with synthetic bank balances. Both modes used the same labor policies and no event presets. Observed loan-to-deposit ratios are H.8 consumer plus commercial-and-industrial loans divided by deposits for the two domestic cohorts, from the February 2024 scoring edition. The model ratio includes loans held by the simulated commercial banks. Monthly averages in H.8 and month-end model stocks have slightly different timing.

| Metric (MAE, percentage points) | Dated banks | Synthetic banks | Opening value carried forward |
| --- | ---: | ---: | ---: |
| Loan-to-deposit ratio | 1.670 | 25.012 | 1.568 |
| Unemployment | 2.499 | 2.490 | 1.596 |

Dated bank initialization corrects a large level error in the development opening. The subsequent loan-to-deposit path still underperforms the opening-value baseline. The inherited origination, repayment, default, deposit-flow, and reserve behavior require calibration. Production, fiscal and household-wealth calibration remain separate open work.

The saved release run is `artifacts/historical-economy-v3/`. Both opening modes ran through December 2028 with exact ledger closure. The reconstructed opening also ran through 2030, with an identical overlapping path. The dashboard can switch between openings and between the loan-to-deposit comparison and model balance levels.

```sh
cargo build --release -p owg-cli
python3 research/us-economy-2007-2023/code/collect_bank_opening.py
python3 research/us-economy-2007-2023/code/historical_lab.py --binary target/release/owg --finance --score
python3 research/us-economy-2007-2023/code/serve_historical_lab.py --binary target/release/owg --finance
```
