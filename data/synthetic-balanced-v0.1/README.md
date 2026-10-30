# Synthetic balanced data bundle

This directory records the transparent inputs for the built-in v0.1 generator.
The current engine still embeds these development parameters so a release
binary has no runtime file dependency. The duplication is checked manually for
now and must be replaced by model-pack loading before the synthetic gate closes.

Generated populations and opening balance sheets are determined by the
scenario root seed and the named-stream algorithm recorded in each run
manifest.

## Licence

These original OpenWorldGen synthetic fixtures are distributed under
`GPL-3.0-only`, as provided in the repository's `LICENSE`. The legacy
`internal-research` string in the bundle and scenario provenance is retained
to preserve existing content hashes; it describes the fixture's original
development role, not an additional use or redistribution restriction.
The numbers remain uncalibrated synthetic assumptions, not measured US data.
