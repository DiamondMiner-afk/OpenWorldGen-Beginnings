# Household budgets and firm opening plans

Freeze this specification before running the new profile against the six existing retrospective origins. The trial tests whether household spending anchored to dated income and benefits, paired with firm opening plans tied to that spending, improves the inventory model's subsequent dynamics.

- At each origin, use the median PCE to disposable personal income ratio and median Social Security to wage-and-salary disbursements ratio over the last 12 available common months of the archived BEA vintage. Fit the slope with an intercept over 59 monthly changes in PCE and disposable personal income deflated by the same PCE price index. Clip the slope to [0, 1]. This aggregate proxy does not identify individual marginal spending behavior.
- Anchor a monthly treasury transfer per household outside the labor force to the Social Security ratio applied to opening payroll. Index the unit transfer to lagged simulated CPI. Eligibility and allocation are stylized. Start household income references at expected disposable wages or transfers. Target a fraction of reference income plus a bounded response to current income, scale by confidence, and update references with a fixed 15% monthly weight. Preserve normal settlement, cash constraints and permitted borrowing.
- Rebase each sector's opening firm sales plans, capacity, productivity and inventories proportionally to planned household purchases net of tax and modeled public and foreign demand. Preserve the prior opening staff-to-plan and inventory-to-plan ratios. Subsequent production, sales, prices and hires use the regular agent and market rules.
- Compare the joint intervention to the paired inventory model with the same six origins, eight seeds and dated banks wherever available. Compare the ensemble's annualized quarter-over-quarter fixed-price GDP growth MAE across the 18 common scored quarters and unemployment MAE across 72 months. Include the same dated persistence baselines. Compare the simulated cash purchases to modeled disposable income ratio with BEA PCE/DPI for diagnosis. National accounts also include flows outside the simulated household budget, so their levels are not directly comparable.
- Require zero ledger residual, nominal GDP identity residual at most 1e-8, service expiry and consistent employment counts. Evidence from these previously examined origins is retrospective development evidence. COVID replay and AI scenarios are separate from baseline scoring.

## Saved results

The release build completed, all 125 workspace Rust tests passed, and strict Clippy passed for core and CLI. The six retrospective origins completed across eight seeds each. Accounting and labor stock checks passed for the scored runs.

| Mean absolute error | Household demand trial | Inventory control | Persistence |
| --- | ---: | ---: | ---: |
| Annualized quarterly GDP growth, 18 quarters | 28.52 pp | 27.66 pp | 8.96 pp |
| Unemployment, 72 months | 5.51 pp | 6.23 pp | 1.12 pp |
| Spending share, 72 months | 14.60 pp | 9.88 pp | — |

The trial improved unemployment against its paired inventory control, while GDP and spending share worsened. Persistence retained the lead on GDP and unemployment. Keep the dated sector profile as the dashboard default. The saved candidate remains a retrospective experiment.

## Resume

- Raw sources and scored artifacts follow the local-only ignore policy. See the [live lab runbook](US_ECONOMY_LIVE_LAB.md) for source preparation.
- Inputs are in `data/raw/household-bea-alfred/` and `data/processed/household-behavior/`. Frozen run protocol, per-origin inputs, hashes, and diagnostics are in `artifacts/historical-economy-v6/`.
- To rerun from the checkout, use `cargo build --release -p owg-cli`, then `python3 research/us-economy-2007-2023/code/historical_lab.py --binary target/release/owg --finance --production --inventory --household-demand --score`.
- The live server defaults to the v5 profile. It defaults to 2028 and offers the baseline, COVID replay, AI adoption and combined scenarios. The household trial has CLI and backtest integration. Its API/dashboard controls, focused regression test and long-horizon candidate checks are pending.
- For further model work, use the saved demand diagnostics to separate planned purchases, actual cash purchases, income, gross output and imported inputs. The short-run income slope clipped to zero at four origins. Separate the household budgeting, transfers and firm opening adjustments before deciding which part to carry forward. Freeze the next specification before opening an independent evaluation window.
