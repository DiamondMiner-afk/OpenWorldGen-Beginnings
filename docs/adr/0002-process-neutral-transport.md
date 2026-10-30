# ADR 0002: Process-neutral transport

Status: accepted for contract; service implementation pending the synthetic
scientific gate.

The engine API is defined in `proto/openworldgen/v1/simulation.proto`. Scenario,
policy, manifest, result, and checkpoint payloads remain governed by the JSON
Schemas in `schemas/`; bulk metrics use Arrow IPC or Parquet.

Model/data metadata travels as typed JSON contracts. Content-addressed
artifacts may be supplied inline or by URI, and every artifact is checked
against its declared SHA-256 before execution. A deployment may prohibit URI
schemes or inline data according to its confidentiality policy.

Version 0.1 ships the local CLI and Python subprocess client first. The same
protobuf service will be implemented by the local sidecar used by Minecraft and
remote runners. Keeping the transport contract separate prevents the game,
Python, or a future cluster backend from becoming the economic source of truth.

An unavailable transport is a capability limitation, not permission to mutate
engine state through an unversioned interface.
