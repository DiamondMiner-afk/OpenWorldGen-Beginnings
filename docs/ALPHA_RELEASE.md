# Alpha source-release checklist

Release preparation: 2026-10-30. Intended label: **0.1.0 alpha**.
The package version remains `0.1.0`; no release tag has been created.

## Source directory

Use the **Current Repository** directory under OpenWorldGen as the source root,
not the enclosing Capstone, OpenWorldGen, REFERENCE, or Project History folders.
The root of the GitHub repository should contain `README.md`, `Cargo.toml`,
`LICENSE`, and `crates/` directly; do not add another Current Repository wrapper.

## Include

- `README.md`, `LICENSE`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
  `deny.toml`, `.gitignore`, `.gitattributes`, and `.python-version`.
- `.github/`, `crates/`, `docs/`, `schemas/`, `scenarios/`, `model-packs/`,
  `proto/`, and `experiments/`.
- `data/synthetic-balanced-v0.1/`: these small fixtures are embedded by Rust
  at build time; omitting them breaks a clean build.
- `python/`, including `python/LICENSE`, but excluding local environments,
  caches, build products, and generated package metadata.
- `scripts/` and `research/us-economy-2007-2023/code/`, excluding caches.
  The MVP runner imports the local decision adapter from that code directory,
  even in reference-policy mode. Keep that relative layout intact.
- `benchmarks/baselines/` for the recorded engineering reference.

## Exclude from the minimal alpha upload

- `.git/` when manually copying files into a new checkout. Do not delete the
  original repository's history; normal Git pushes handle history separately.
- `target/`, `outputs/`, `runs/`, local `.venv/` directories, Python caches,
  `dist/`, and `build/`.
- `local-models/`, GGUF/safetensors model files, and external model installs.
- Research `data/raw/`, `data/processed/`, and `artifacts/`: these include
  downloaded datasets, persona archives, model responses, local paths, and
  experiment outputs. Review provenance and redistribution terms before any
  separate data/evidence release.
- `Old - Depreceated/`, the enclosing Project History, and external references.
  The local `Reference/` specification folder is not needed to build this alpha;
  publish it only after separately reviewing its content and rights.
- `.env` files, credentials, logs, and machine-specific files.

The research notes and ledger are local research history, not a self-contained
public reproducibility bundle if their evidence files are omitted. They are
optional for the minimal alpha; do not claim their data-dependent audits work
from a source-only clone. The public README and `docs/` describe the boundaries.

**Important:** `.gitignore` is not a packaging filter for Finder or GitHub's
browser uploader, and does not remove already tracked files. Use the inclusion
list above for manual copying. Before any commit, inspect both `git status
--short` and `git diff --cached --stat`; review the actual staged content.
Do not use `git archive HEAD` to package this dirty checkout: it omits the new
and modified alpha implementation that is not committed yet.

## Licence

The user selected GPL 3.0; this alpha uses **GPL-3.0-only**, not
GPL-3.0-or-later. Both Rust crates inherit that identifier from the workspace.
The Python package declares the same identifier and carries a copy of the
licence for its distribution. Preserve third-party licence notices.
The original synthetic fixture's legacy `internal-research` provenance field
is retained for hash compatibility, not as a restriction on its GPL licence.

## Checks before publishing

1. Copy the selected source into a clean checkout, preserving the paths above.
2. Run the README's Rust format/lint/test commands, Python SDK tests, and MVP
   smoke command. Validate the generated result bundle and open its graph.
3. Confirm that the build and reference-policy demo do not need files from
   your model directory, downloaded data, or original absolute workspace path.
4. Review dependencies and their notices; run the configured dependency
   advisory/licence checks. GPL metadata alone is not a dependency audit.
5. Inspect the staged filenames, secrets, source-data rights, and large files.
   A clean local test is not a secrets scan or GitHub CI pass.
6. Publish as a development alpha. Do not claim calibrated US simulation,
   trained RL, representative MatrAIx agents, a working Fabric edition, or
   forecast qualification. Link any later research-data release separately.

This checklist does not publish, push, tag, or create a GitHub release.
