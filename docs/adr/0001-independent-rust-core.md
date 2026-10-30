# ADR 0001: Independent Rust core

Status: accepted

The authoritative engine is implemented independently in Rust and does not
derive from AGPL macro-ABM source code. Published models such as BeforeIT are
comparison targets and methodological references.

Rust provides portable native binaries, explicit numeric behavior, memory
safety, predictable resource use, and suitable integration boundaries for
Python and Java. Python remains the research orchestration surface. This
decision can be revisited only with measured evidence and a licence review.
