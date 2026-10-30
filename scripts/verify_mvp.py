#!/usr/bin/env python3
"""Independently recheck a completed MVP directory without calling a model.

Emits bounded software evidence and hashes; does not certify economic realism.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    run = args.run.resolve()
    status = load(run / "mvp_status.json")
    if status["status"] != "passed" or status["forecast_qualified"] or status["empirical_us_initialized"]:
        raise ValueError("run is unfinished or claims qualification outside MVP scope")
    checks = []
    def call(*arguments):
        command = [str(ROOT / "target/debug/owg"), *map(str, arguments)]
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=True)
        checks.append({"command": command, "stdout": result.stdout.strip(), "return_code": result.returncode})
    call("validate", run / "economy", "--kind", "result-bundle", "--json")
    call("validate", run / "resumed.json", "--kind", "checkpoint", "--json")
    call("validate", run / "direct.json", "--kind", "checkpoint", "--json")
    direct, resumed = load(run / "direct.json"), load(run / "resumed.json")
    for field in ["economy", "metrics", "journal_hash"]:
        if direct[field] != resumed[field]:
            raise ValueError(f"resume mismatch: {field}")
    final = load(run / f"strategic-{status['strategic_months']}.json")
    if final != load(run / "strategic-replay.json"):
        raise ValueError("saved strategic replay differs")
    with tempfile.TemporaryDirectory(prefix="owg-mvp-verify-") as scratch:
        replay = Path(scratch) / "replay.json"
        call("run-recorded-firm", run / "scenario.json", run / "decisions.json", "--law-binding", run / "firm-law.json", "--output", replay)
        if load(replay) != final:
            raise ValueError("fresh strategic replay differs")
    positions = direct["economy"]["contract_credit"]["positions"]
    if positions["after-law-change"]["status"] != "rejected_law":
        raise ValueError("law-change rejection missing")
    tape = load(run / "decisions.json")
    non_noop = sum(step["actions"]["actions"][0]["type"] != "no_op" for step in tape["steps"])
    accepted = fallback = 0
    if status["strategic_policy"] == "local_llm":
        for month, step in enumerate(tape["steps"]):
            audit = load(run / f"model-audit-{month}.json")
            entry = audit["steps"][0]["entries"][0]
            if entry["selected_action"] != step["actions"]["actions"][0]:
                raise ValueError("model audit and applied tape disagree")
            accepted += audit["accepted_count"]
            fallback += audit["fallback_count"]
        if accepted != status["model_proposals_accepted"] or fallback != status["model_proposals_fallback"]:
            raise ValueError("model proposal counts disagree")
    files = {}
    for path in sorted(run.rglob("*")):
        if path.is_symlink():
            raise ValueError("symlink in MVP run directory")
        if path.is_file():
            files[str(path.relative_to(run))] = digest(path)
    result = {"schema_version": "owg.mvp-verification.v1", "verified_at_utc": datetime.now(timezone.utc).isoformat(),
        "run_directory": str(run), "status": "passed", "run_status": status, "checks": checks,
        "checkpoint_equal": True, "strategic_replay_equal": True,
        "strategic_non_noop_actions": non_noop, "model_proposals_accepted": accepted,
        "model_proposals_fallback": fallback, "file_hashes": files,
        "source_hashes": {name: digest(ROOT / name) for name in ["scripts/run_mvp.py", "scripts/verify_mvp.py", "crates/owg-core/src/credit.rs", "crates/owg-core/src/credit_settlement.rs"]},
        "boundary": "Software runtime/replay evidence only. Synthetic world, no empirical US initialization, trained RL, or forecast qualification."}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2, allow_nan=False)
        stream.write("\n")
    print(json.dumps({"status": "passed", "files_hashed": len(files), "non_noop_actions": non_noop, "output": str(args.output)}))


if __name__ == "__main__":
    main()
