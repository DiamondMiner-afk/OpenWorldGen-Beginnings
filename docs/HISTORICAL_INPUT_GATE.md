---
title: Historical exact-byte input gate
status: development
tags: [OpenWorldGen, economics, data-vintage, provenance]
---

# Historical exact-byte input gate

The existing `owg.data-vintage.v1` contract remains compatible with the
Philadelphia Fed research manifests. It checks that a declared release date
is no later than `as_of` and that the local file matches its manifest hash.
It **does not establish that those exact bytes existed at `as_of`**. An
extracted 2006Q4 column from a workbook retrieved in 2026 is useful for
point-in-time analysis, but is not an authenticated 2006 file.

`owg.data-vintage.v2` is the opt-in source-byte gate. Each source explicitly
declares one of two `byte_availability` bases:

- `publication_only`: the release-level date is supported, but exact source
  bytes have no independent cutoff-dated capture. This basis fails historical
  input admission, even if a current local file has the expected hash.
- `independent_capture`: a third-party archive is claimed to have captured
  the *same* bytes no later than `as_of`. It records provider, capture and
  record URLs, timestamp, digest, and a local copy of the archived bytes.
  Local validation requires both copies and matching digests; post-cutoff
  captures, reused first-party publisher names, unsafe paths, and conflicting
  digests fail.

The v1 and v2 CLI readers now reject manifest-relative paths containing a
symbolic-link component, including a same-hash link to a file outside the
manifest directory; the canonical target must remain inside it. Focused
hostile tests cover source, archive, and parent-directory links. This is
local-file containment, **not** third-party origin authentication. It is not
race-proof if another process replaces entries between checking and reading;
an eventual admitted-input loader needs stronger descriptor-based handling
for adversarial concurrent filesystems. See
`research/us-economy-2007-2023/notes/VINTAGE_LOCAL_PATH_HARDENING.md`.

Generate the contract with `owg schema --kind data-vintage-strict --output
schemas/data-vintage-v2.schema.json`. Run `owg validate --kind
data-vintage-strict <manifest.json> --json` to check **internal consistency of
the evidence claim and local files**. Its output says
`evidence_claim_validated: true`, `external_origin_verified: false`, and
`historical_input_admitted: false`. `valid: true` refers only to structural
contract validation. Do not treat this CLI result as historical input
admission.

The Rust admission API additionally requires a `CaptureOriginVerifier`.
That verifier must independently check the archive's *actual* origin,
timestamp, capture record, and returned content; copying a contemporary file
and writing an old date into JSON is insufficient. No generic origin verifier
is shipped yet, so the CLI deliberately cannot promote a self-declared
capture to admitted historical input. This is a fail-closed boundary, not a
claim that an archive URL or HTTP `Last-Modified` header cryptographically
proves historical availability.

A saved [bounded CDX probe](../research/us-economy-2007-2023/notes/ARCHIVE_ORIGIN_CDX_PROBE_2026-10-20.md)
found June 2007 as the first successful index capture for the tested BEA URL;
two exact Fed URL queries returned empty within their limited ranges. Those
index responses are reproducible negative leads, not an exhaustive absence
proof or an implemented `CaptureOriginVerifier`.

Consequences for the January 2007 US opening-state experiment:

1. The surviving 2004 SCF Bulletin PDF, with a 2017 server modification
   date, remains a publication-level source until matching pre-cutoff capture
   evidence is independently checked.
2. The December 2006 Z.1 release-labelled file with a March 2007 server
   modification date also remains publication-level at the January cutoff.
3. The BEA 2005 Make/Use workbook captured in June 2007 is too late to prove
   exact bytes at the January cutoff, despite a December 2006 release-level
   argument.
4. The Philadelphia Fed extracted vintage columns retain their v1 research
   use but are not silently promoted to v2 exact-byte input.

This gate concerns *exact bytes*, not whether a later reconstruction can
correctly recover earlier published values. Historical models may use the
latter only under a separately declared reconstruction/measurement policy,
never by relabeling them as authenticated actor-visible files.
