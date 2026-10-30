"""Fail-closed integrity audit for the OpenWorldGen research evidence register.

This checks local reproducibility, not external source authenticity or whether
an economic claim is supported by the registered evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
from typing import Any


RESEARCH_ROOT = Path(__file__).resolve().parents[1]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _safe_file(root: Path, name: str) -> Path | None:
    relative = PurePosixPath(name)
    if (
        not name
        or "\\" in name
        or relative.is_absolute()
        or any(part in {"", ".", ".."} for part in name.split("/"))
    ):
        return None
    candidate = root / relative
    prefix = root
    for part in relative.parts:
        prefix = prefix / part
        if prefix.is_symlink():
            return None
    try:
        resolved = candidate.resolve(strict=True)
    except (OSError, RuntimeError):
        return None
    if not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
        return None
    return resolved


def audit(root: Path) -> dict[str, Any]:
    root = root.resolve()
    state = json.loads((root / "research_state.json").read_text(encoding="utf-8"))
    hashes = state.get("reproducibility", {}).get("artifact_hashes")
    if not isinstance(hashes, dict) or not hashes:
        raise ValueError("research_state.json has no artifact_hashes mapping")

    failures: list[str] = []
    checked = 0
    for name, expected in sorted(hashes.items()):
        if not isinstance(name, str) or not isinstance(expected, str):
            failures.append(f"invalid artifact entry: {name!r}")
            continue
        path = _safe_file(root, name)
        if path is None:
            failures.append(f"missing or unsafe artifact path: {name}")
            continue
        if len(expected) != 64 or any(c not in "0123456789abcdef" for c in expected):
            failures.append(f"invalid SHA-256 in register: {name}")
            continue
        actual = sha256_file(path)
        checked += 1
        if actual != expected:
            failures.append(f"artifact hash mismatch: {name}")

    evidence = state.get("evidence", [])
    if not isinstance(evidence, list):
        failures.append("evidence is not a list")
        evidence = []
    claim_ids: set[str] = set()
    for item in evidence:
        if not isinstance(item, dict):
            failures.append("non-object evidence entry")
            continue
        claim_id = item.get("claim_id")
        path = item.get("artifact_path")
        if not isinstance(claim_id, str) or not claim_id or claim_id in claim_ids:
            failures.append(f"missing or duplicate claim_id: {claim_id!r}")
        else:
            claim_ids.add(claim_id)
        if not isinstance(path, str) or path not in hashes:
            failures.append(f"evidence has no hashed artifact: {claim_id!r}")

    return {
        "schema_version": "owg.research-state-audit.v1",
        "registered_artifacts": len(hashes),
        "hashed_artifacts": checked,
        "evidence_claims": len(evidence),
        "failures": failures,
        "passed": not failures,
        "boundary": "Local integrity only; not archive-origin verification or empirical validation",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=RESEARCH_ROOT)
    args = parser.parse_args()
    result = audit(args.root)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
