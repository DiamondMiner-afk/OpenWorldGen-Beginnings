---
title: US opening-state measurement gate
type: preregistered-research-design
status: not-constructed
as_of: 2026-10-21
tags: [OpenWorldGen, US-economy, opening-balance-sheet, no-lookahead, validation]
---

# US opening-state measurement gate

> [!important] Target, not result
> The engine's current opening money and firms are synthetic. This note defines
> the next evidence gate; it does **not** claim a balanced or calibrated US
> economy has been built.

## One question, two information sets

For a forecast origin $t_0$, distinguish:

1. **Actor information:** publications available by $t_0$, including their
   release dates and revisions known then. Actor prompts, rules, and learned
   policies may read only this set.
2. **Retrospective truth:** later-revised series used *only* to score simulated
   outcomes. They cannot silently seed the initial state or train a policy for
   a claimed historical no-lookahead test.

The first candidate origin is the start of 2007, but a complete 2006 Q4
Financial Accounts value was not necessarily published then. Thus the origin
must use earlier released stocks plus a documented nowcast or move later; it
must not relabel a March/June 2007 archive as January 2007 knowledge. An
initial-state estimate may have uncertainty. That uncertainty belongs in the
run inputs and propagated results, not in an unexplained residual account.

The industry vintage has the same trap. BEA's [December 2006 annual-industry
account](https://apps.bea.gov/scb/pdf/2006/12december/1206_indyaccts.pdf)
states that its December 11 update incorporated 2005 annual input-output
accounts; BEA's [September 2007 release](https://www.bea.gov/news/2007/benchmark-input-output-accounts-us-economy-2002)
still said the 2006 annual update would arrive in December 2007. The later
[2007 benchmark account](https://www.bea.gov/news/2013/2007-benchmark-input-output-account)
was released in 2013. Therefore a January 2007 actor cannot be initialized
with the 2006 annual I-O update or 2007 benchmark table. These publication
claims identify candidate releases. A 2005 make/use workbook has since been
recovered with a June 2007 byte capture, but its exact bytes are not certified
for January 1; see [[BEA_IO_2005_SOURCE_PROBE]]. A July 2007 origin is a
candidate for the first exact-byte-constrained pilot, subject to validating
every other source against the same cutoff. Early-2007 months would then be a
separate retrospective reconstruction, not an out-of-sample forecast.

## Minimum account mapping

| Simulation stock/flow | Empirical target to find | Counterparty and test | Current gate |
| --- | --- | --- | --- |
| Household deposits and cash | Dated Federal Reserve Z.1 household/nonprofit levels, split by instrument; distributional source separately | Match bank deposit liabilities only after including firms, government, nonprofits, and rest of world at compatible scope | December 2006 extraction checks all sector/instrument monetary rows against a pre-origin PDF candidate and provides deposit/currency issuer-holder margins; source-origin admission and individual bank links remain open; see [[Z1_FINANCIAL_ACCOUNTS_2006_EXTRACTION]] |
| Household wealth distribution | Dated SCF family weights, implicates, asset/debt composition, and uncertainty | Quantiles and group moments constrain agent allocation; balance-sheet totals must be separately reconciled to Z.1 coverage | February 2006 article provides 2004 published cutoffs and group moments only; exact current PDF bytes and 2007 joint distribution not certified; see [[SCF_2004_BULLETIN_WEALTH_PROBE]] |
| Firm cash and credit | Dated financial-account or firm-disclosure stocks by sector and size | Firm deposit assets must have a bank liability; firm debt must have a lender asset | Z.1 sector aggregates and seven instrument margins extracted; representative firm-size distributions, bilateral creditors, and valuation mapping remain open |
| Bank reserves and deposits | Dated bank/central-bank balance-sheet stocks with institutional scope | Sum bank reserve assets + central-bank reserve liability = 0; deposits and bank capital reconcile | L.108/L.109 and savings/credit-union source stocks extracted; mapping net positions, reserve scope, securities, and individual banks to engine accounts remains open |
| Household and firm debt | Dated sector/instrument debt stocks, with borrower/lender match | Borrower liability = lender asset at declared coverage | Bank-loan and mortgage issuer/holder margins reconcile as measured constraints; no individual debtor-creditor network or nonbank loan settlement in the engine |
| Government Treasury, debt, tax flows | Dated federal/state/local stocks and receipts/outlays | Government cash and debt link to central bank, banks, households, and rest of world | Federal/state/local stocks and Treasury holder margins extracted; raw flow packages collected for the full period; fiscal flow calibration and Treasury instrument/account mapping remain open |
| Production, intermediate inputs, final demand | BEA supply/use vintage with a reproducible industry-to-engine map | Commodity supply and uses reconcile after explicit imports, exports, inventories, margins, and valuation conventions | 2005 Make/Use cells audited; provisional four-sector candidate preserves signed adjustments but import allocation, classification review, and actor-vintage certification remain open |
| Wages, employment, prices | Dated BLS/BEA releases, with survey and payroll definitions kept distinct | Flow periods, base years, seasonality, and population denominators explicit | Pre-origin BLS CPI/employment/PPI HTML candidates saved; linked tables, labor/union series definitions, origin admission, and micro calibration remain open; see [[PREORIGIN_BLS_Z1_ARCHIVE_LEADS_2026-10-21]] |
| Ownership and securities | Measured security issuance, holdings, and beneficial-ownership evidence where available | An issuer liability/claim must be matched by holder assets; graph stakes alone have no market value | Legal graph only; no securities accounts |

The four engine sectors—energy, food, manufacturing, services—cannot be joined
to official industry rows by name. A versioned many-to-one classification
matrix must specify treatment of trade margins, imports, government output,
housing, finance, inventory, and residual industries. Conservation is checked
both before and after aggregation. A mismatch is reported, not hidden in
`OpeningResidual` or passed to an LLM to explain away.

The December 2006 source L.5 bridge now makes equity, unallocated claims, and
float adjustments explicit; L.231 alone reports a −3,827,706 million-dollar
unidentified-claims discrepancy. The extraction retains this measurement
structure. It is not a new cash account or an economic transaction. The full
release collection in [[Z1_RELEASE_COLLECTION_2007_2023]] contains 70 separate
editions through reported 2023 Q4, with publication dates checked from PDF
covers rather than assumed from URL path names.

## Construction procedure

1. Freeze an origin, release cutoff, source files, hashes, exact series codes,
   units, sector definitions, and transformation code. Record whether each
   value is observed, nowcast, imputed, or withheld for evaluation.
2. Build a measured sector-by-instrument stock matrix and a separate annual or
   quarterly transaction matrix. Do not subtract stocks from different
   vintages to manufacture flows.
3. Map stocks onto an institution/ownership graph with explicit depositor-bank,
   debtor-creditor, security-holder/issuer, and government/central-bank links.
   Allocate heterogeneous agents from measured distributions while preserving
   measured aggregate totals exactly.
4. Solve only *declared* small reconciliation discrepancies, with a documented
   objective, source uncertainties, and constrained counterparties. Preserve
   pre-reconciliation values and a residual report. Reject a source mismatch
   larger than its preregistered tolerance.
5. Run a no-shock closed-account test and independent review. Then freeze
   parameters and test a historical interval against persistence/AR(1) and
   accounting-only baselines using the same actor information set.

## Acceptance and rejection

Pass requires exact minor-unit ledger closure, instrument-level counterparty
closure at the chosen coverage, transparent real-resource discrepancies,
measured distributional targets with uncertainty, and a full source-vintage
manifest bound to the run. A post-origin publication in actor input, an
unexplained plug, incompatible sector units, or a graph ownership stake used
as cash fails the gate. Passing this gate would license a **measured opening
state**, not a realistic predictor; that still needs held-out behavioral and
causal tests. Baka, the difficult part is proving what the agents could have
known, not persuading them to tell a story about it.

## Immediate source tasks

- Verify the December 16, 2006 Z.1 PDF capture through an independent origin
  verifier for a January 2007 origin—or choose a later cutoff. Its matching
  displayed values anchor a bounded extraction, but earlier ZIP history and
  the coded PDF have separate provenance limits. The March 2007 correction and adjacent
  release comparisons in [[FED_Z1_DECEMBER_2006_PREORIGIN_PROBE]] and
  [[FED_Z1_ADJACENT_HOUSEHOLD_DEPOSITS_PROBE]] are revision evidence, not
  an authenticated early-2007 actor information set.
- Review and stress [[BEA_IO_2005_FOUR_SECTOR_CANDIDATE_V1]] against the raw
  matrices in [[BEA_IO_2005_MATRIX_QC]], then resolve import allocation and
  ambiguous housing/finance/government classifications. Do not apply those
  exact bytes at January 2007 until availability is resolved.
- Locate dated bank and fiscal releases available at the chosen origin. Treat
  [[SCF_2004_BULLETIN_WEALTH_PROBE]] as bounded 2004 household-distribution
  constraints, then find a vintage-valid late-2006 bridge and audit survey
  weights, implicates, sampling error, and Z.1 scope. Keep retrospective
  evaluation editions separate.
- Build a machine-checkable matrix of account identities and tolerances before
  fitting agent behavior or adding strategic AI to historical tests.
