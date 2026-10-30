# Dependency and licence policy

OpenWorldGen's original source is licensed under GNU GPL version 3 only
(`GPL-3.0-only`); see the repository's `LICENSE`. The Rust crates are currently
marked `publish = false`: this prevents registry publication, not source
distribution or use under the GPL. Every Rust dependency must come
from the crates.io registry, declare an allowed SPDX licence, and pass the
RustSec advisory scan in CI. Git dependencies, unknown registries, wildcard
version requirements, yanked crates, vulnerabilities, and unsoundness
advisories are denied.

Multiple transitive versions are reported as warnings because Arrow, Parquet,
platform support, and procedural-macro stacks do not always converge on one
release. A warning is reviewed rather than silently suppressed.

Unmaintained-crate findings fail for direct workspace dependencies. They remain
visible but do not fail for transitive dependencies when no compatible upstream
replacement exists. As of 2026-07-24, `parquet 59.1.0` transitively uses
`paste 1.0.15` (`RUSTSEC-2024-0436`). OpenWorldGen does not call `paste`
directly. This exception must be reconsidered whenever Parquet is updated; it
does not waive vulnerability or unsoundness findings.

The policy is implemented in `deny.toml` and run by the `supply-chain` CI job.

Third-party dependencies retain their own licences and notices. Local model
weights, downloaded datasets, persona archives, and external reference material
are not relicensed by OpenWorldGen's GPL notice and are excluded from the
alpha source upload. Check their individual terms before redistribution.
The bundled synthetic fixture's historical `internal-research` provenance tag
is retained for run-hash compatibility; it is not an additional restriction on
the GPL grant for OpenWorldGen's own fixture. See its README.
