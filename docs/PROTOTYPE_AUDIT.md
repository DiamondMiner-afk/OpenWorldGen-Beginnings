# Deprecated prototype audit

The historical Python prototype is preserved unchanged under
`Old - Depreceated/Economics/`. It remains an independent nested Git
repository and is ignored by the new monorepo so its existing history,
generated weights, and working-tree changes cannot be absorbed accidentally.
It is not a production baseline.

Useful requirements recovered from it include:

- explicit households, firms, banks, government, and central bank;
- labor, goods, credit, interbank, and bond concepts;
- seeded execution, telemetry, and integrity checks;
- recognition that accounting and defaults must be modeled explicitly.

It is not reusable as an economic model because:

- `TinyBrain` networks are randomly initialized and never trained, calibrated,
  or assigned an economic objective;
- many behavioral coefficients and limits are arbitrary;
- the ledger is not a complete double-entry institutional accounting system;
- audits are calculated but do not establish national-account closure;
- markets and balance sheets mix inconsistent units and settlement semantics;
- no calibration, parameter recovery, holdout, sensitivity, or scale study is
  present.

The preserved 5,000-tick runs end with full unemployment, zero output, and only
one active firm in both inspected seeds. That is evidence of model collapse,
not a validated recession mechanism.
