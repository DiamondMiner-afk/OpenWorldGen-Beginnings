# Accounting design

OpenWorldGen uses one global double-entry journal with entity-specific accounts.
A deposit-funded purchase moves a deposit asset from buyer to seller. A bank
loan creates four mirror postings: borrower deposit asset, bank deposit
liability, bank loan asset, and borrower debt liability. Taxes and transfers are
ordinary treasury/deposit transfers.

The engine separately checks that each commercial bank's deposit liability is
the exact negative of deposits held by the households and firms assigned to
that bank (plus the development rest-of-world depositor assigned to bank 0).
This catches wrong-bank transfers that global double-entry and the aggregate
reserve identity can miss. Unknown deposit owners and bank liabilities,
negative customer deposits, and positive commercial-bank deposit liabilities
fail the check. It runs at opening, before and after each simulated month, and
for both file-loaded and directly resumed checkpoints. This is an instrument-counterparty check for the present
synthetic account topology, **not** an empirically reconciled US banking system;
joint accounts, bank migration, nonbank deposits, and deposit insurance are not
represented.

The engine also checks each bank's nonnegative loan assets against the exact
minor-unit sum of nonpositive debts owed by its assigned household and firm
borrowers. It rejects unknown borrower or lender accounts and wrong-sign
claims. This catches a loan asset moved to the wrong bank even if the global
journal, aggregate loan total, deposits, and reserves still balance. The check
runs at opening, monthly close, and checkpoint preflight. The present
`bank_index` is a single-lender shortcut; loan sales, syndication, nonbank
creditors, and securitization require a contractual creditor graph before
this relation can represent them. See
`research/us-economy-2007-2023/notes/BANK_LOAN_COUNTERPARTY_AUDIT.md`.

Opening balances must globally sum to zero. The current synthetic initializer
posts explicit opening equity closure for commercial banks, government, and
the central bank; it does **not** close every household or firm as a complete
entity balance sheet. The ownership sector carries a counterpart to those
opening equity postings. Per-entity, instrument-level empirical balance-sheet
closure remains a research gate, and an unexplained residual is not an
acceptable shortcut for that gate.

Financial conservation alone is insufficient. The current development kernel
also checks:

- domestic production = household final consumption at producer prices +
  government purchases + exports + inventory change + recorded inventory
  losses;
- nominal GDP measured from production and expenditure agrees within the
  declared floating-point tolerance;
- nominal GDP measured from production and the development income
  decomposition agrees within the declared tolerance;
- household, firm, bank, government, central-bank, rest-of-world, ownership,
  and unclassified financial positions sum to exactly zero in minor units.

The income decomposition reports gross value added, employee compensation,
taxes on products, and gross operating surplus. Gross operating surplus is
currently derived as value added less compensation. Depreciation, subsidies,
mixed income, and independently sourced income estimates remain required
before the stage-2 accounting gate can close.

The goods-resource residual is measured independently. Opening inventories are
valued after the month's price update; production adds physical units; settled
goods purchases convert integer minor-unit payments back to quantities without
overselling; closing inventory is revalued at the unchanged transaction price;
and inventory destroyed during a firm resolution is recorded explicitly as a
loss. The engine compares measured closing-minus-opening inventory with
production and final uses. It does not define inventory change as the balancing
item needed to force the identity to pass.

CPI uses tax-inclusive purchaser prices relative to tax-inclusive base-period
prices. Producer-price indices remain separate and are used to deflate imported
inputs for the development real-GDP measure, so an indirect-tax change affects
CPI directly without mechanically changing that deflator.
