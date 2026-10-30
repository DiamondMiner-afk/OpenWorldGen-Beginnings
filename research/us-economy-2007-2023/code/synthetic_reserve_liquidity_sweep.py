#!/usr/bin/env python3
"""Bounded, preregistered synthetic reserve/liquidity checkpoint sweep.

This is an engineering diagnostic. It does not calibrate the model to US data.
Run from any directory; see the paired protocol note for the fixed profile.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import platform
import subprocess
import sys
import tempfile
import time
from pathlib import Path


REPO = Path(__file__).resolve().parents[3]
PROTOCOL = REPO / "research/us-economy-2007-2023/notes/SYNTHETIC_RESERVE_LIQUIDITY_PROTOCOL.md"
SEEDS = (20260724, 20260725, 20260726)
MONTHS = 12
BRANCH = "baseline"
REPLICATION = 0
CENTRAL_BANK_ID = "CENTRAL_BANK"


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def write_json(path: Path, value: object) -> None:
    with path.open("w", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write("\n")


def command(cli: Path, args: list[str], commands: list[dict]) -> subprocess.CompletedProcess[str]:
    invocation = [str(cli), *args]
    started = time.perf_counter()
    result = subprocess.run(invocation, cwd=REPO, capture_output=True, text=True, check=False)
    record = {
        "argv": invocation,
        "exit_code": result.returncode,
        "elapsed_seconds": round(time.perf_counter() - started, 3),
    }
    if result.returncode:
        record["stdout"] = result.stdout[-4000:]
        record["stderr"] = result.stderr[-4000:]
    commands.append(record)
    return result


def account_balances(checkpoint: dict) -> dict[tuple[str, str], int]:
    balances: dict[tuple[str, str], int] = {}
    for item in checkpoint["economy"]["ledger"]["balances"]:
        key = (item["account"]["owner_id"], item["account"]["kind"])
        if key in balances:
            raise ValueError(f"duplicate ledger account {key}")
        balances[key] = item["balance"]
    return balances


def inspect_checkpoint(checkpoint: dict, seed: int, month: int) -> tuple[list[dict], list[dict]]:
    if checkpoint["next_month"] != month or checkpoint["replication"] != REPLICATION:
        raise ValueError("checkpoint month or replication differs from the requested state")
    if checkpoint["branch_id"] != BRANCH or checkpoint["economy"]["root_seed"] != seed:
        raise ValueError("checkpoint branch or root seed differs from the requested state")
    if len(checkpoint["metrics"]) != month:
        raise ValueError("checkpoint metric history does not match its month")

    balances = account_balances(checkpoint)
    banks = checkpoint["economy"]["banks"]
    bank_ids = banks["ids"]
    minimums = banks["minimum_liquidity_ratio"]
    if len(bank_ids) != len(minimums):
        raise ValueError("bank IDs and liquidity floors have different lengths")
    cb_liability = balances.get((CENTRAL_BANK_ID, "reserve_liability"), 0)
    reserve_sum = sum(balances.get((bank_id, "reserve"), 0) for bank_id in bank_ids)
    reserve_residual = reserve_sum + cb_liability
    violations = []
    if reserve_residual:
        violations.append({"seed": seed, "month": month, "code": "reserve_claim_mismatch", "minor_units": reserve_residual})

    rows = []
    for bank_id, floor in zip(bank_ids, minimums, strict=True):
        reserve = balances.get((bank_id, "reserve"), 0)
        deposit_liability = balances.get((bank_id, "deposit_liability"), 0)
        deposits = -deposit_liability
        # The engine uses max(projected deposits, 1 major unit) when screening a loan.
        # This is a current-state screen, so it is not a loan-approval replay.
        liquidity_ratio = reserve / max(deposits, 100)
        if reserve < 0:
            violations.append({"seed": seed, "month": month, "bank": bank_id, "code": "negative_reserve", "minor_units": reserve})
        if deposit_liability > 0:
            violations.append({"seed": seed, "month": month, "bank": bank_id, "code": "deposit_liability_sign", "minor_units": deposit_liability})
        if not math.isfinite(liquidity_ratio) or liquidity_ratio < floor:
            violations.append({"seed": seed, "month": month, "bank": bank_id, "code": "below_liquidity_floor", "ratio": liquidity_ratio, "floor": floor})
        last_metric = checkpoint["metrics"][-1] if month else None
        rows.append({
            "seed": seed,
            "month": month,
            "bank": bank_id,
            "reserve_minor_units": reserve,
            "deposit_liability_minor_units": deposit_liability,
            "liquidity_ratio": format(liquidity_ratio, ".9f"),
            "liquidity_floor": format(floor, ".9f"),
            "central_bank_reserve_liability_minor_units": cb_liability,
            "reserve_claim_residual_minor_units": reserve_residual,
            "bank_resolutions_in_month": last_metric["bank_resolutions"] if last_metric else "",
            "real_gdp_in_month": format(last_metric["real_gdp"], ".9f") if last_metric else "",
            "journal_hash": checkpoint["journal_hash"],
            "scenario_hash": checkpoint["scenario_hash"],
        })
    return rows, violations


def run(cli: Path, output: Path) -> int:
    output.mkdir(parents=True, exist_ok=False)
    if not cli.is_file() or not PROTOCOL.is_file():
        raise FileNotFoundError("compiled CLI or preregistered protocol is missing")

    started = time.perf_counter()
    commands: list[dict] = []
    observations: list[dict] = []
    violations: list[dict] = []
    failed_commands: list[dict] = []
    negative_control: dict = {"status": "not_run"}
    scenario_hashes: dict[str, str] = {}
    error: str | None = None
    try:
        template_path = output / "scenario_template.json"
        result = command(cli, ["init-scenario", "--output", str(template_path)], commands)
        if result.returncode:
            raise RuntimeError("init-scenario failed")
        template = json.loads(template_path.read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory(prefix="owg-reserve-sweep-") as temp_name:
            temp = Path(temp_name)
            for seed in SEEDS:
                scenario = json.loads(json.dumps(template))
                scenario["scenario_id"] = f"synthetic-reserve-no-shock-seed-{seed}"
                scenario["title"] = f"Synthetic no-shock reserve sweep, root seed {seed}"
                scenario["root_seed"] = seed
                scenario["calendar"]["months"] = MONTHS
                scenario["shocks"] = []
                scenario["policy_branches"] = [branch for branch in scenario["policy_branches"] if branch["id"] == BRANCH]
                if len(scenario["policy_branches"]) != 1 or scenario["policy_branches"][0]["actions"]:
                    raise ValueError("template lacks a unique action-free baseline branch")
                scenario_path = output / f"scenario_seed_{seed}.json"
                write_json(scenario_path, scenario)
                scenario_hashes[str(seed)] = digest(scenario_path)
                checked = command(cli, ["validate", str(scenario_path), "--kind", "scenario", "--json"], commands)
                if checked.returncode:
                    failed_commands.append(commands[-1])
                    continue

                for month in range(MONTHS + 1):
                    checkpoint_path = temp / f"seed_{seed}_month_{month}.json"
                    created = command(cli, ["checkpoint", str(scenario_path), "--branch", BRANCH,
                                            "--replication", str(REPLICATION), "--month", str(month),
                                            "--output", str(checkpoint_path)], commands)
                    if created.returncode:
                        failed_commands.append(commands[-1])
                        continue
                    checked = command(cli, ["validate", str(checkpoint_path), "--kind", "checkpoint", "--json"], commands)
                    if checked.returncode:
                        failed_commands.append(commands[-1])
                        continue
                    checkpoint = json.loads(checkpoint_path.read_text(encoding="utf-8"))
                    rows, findings = inspect_checkpoint(checkpoint, seed, month)
                    observations.extend(rows)
                    violations.extend(findings)

                    if seed == SEEDS[0] and month == 0:
                        tampered = json.loads(json.dumps(checkpoint))
                        first_bank = tampered["economy"]["banks"]["ids"][0]
                        for item in tampered["economy"]["ledger"]["balances"]:
                            if item["account"] == {"owner_id": first_bank, "kind": "reserve"}:
                                item["balance"] += 1
                                break
                        else:
                            raise ValueError("negative-control reserve account missing")
                        tampered_path = temp / "tampered_reserve_plus_one.json"
                        write_json(tampered_path, tampered)
                        rejection = command(cli, ["validate", str(tampered_path), "--kind", "checkpoint", "--json"], commands)
                        rejected_for_reserve = rejection.returncode != 0 and "reserve" in (rejection.stdout + rejection.stderr).lower()
                        negative_control = {
                            "status": "rejected_for_reserve" if rejected_for_reserve else "unexpected_result",
                            "tamper": f"add one minor unit to {first_bank} reserve asset only",
                            "tampered_checkpoint_sha256": digest(tampered_path),
                            "exit_code": rejection.returncode,
                            "stderr": rejection.stderr[-1200:],
                        }
                        if not rejected_for_reserve:
                            violations.append({"seed": seed, "month": month, "code": "negative_control_not_rejected_for_reserve"})
                    checkpoint_path.unlink()
    except Exception as exc:  # Keep partial evidence rather than hiding a failed run.
        error = f"{type(exc).__name__}: {exc}"
    finally:
        if observations:
            with (output / "observations.csv").open("w", encoding="utf-8", newline="") as stream:
                writer = csv.DictWriter(stream, fieldnames=list(observations[0]))
                writer.writeheader()
                writer.writerows(observations)
        if failed_commands:
            write_json(output / "failed_commands.json", failed_commands)
        expected_rows = len(SEEDS) * (MONTHS + 1) * 2
        summary = {
            "claim_type": "simulation_engineering_diagnostic_not_empirical_validation",
            "profile": {"seeds": SEEDS, "months": MONTHS, "sampled_checkpoint_months": list(range(MONTHS + 1)),
                        "branch": BRANCH, "replication": REPLICATION, "shocks": [], "banks": 2},
            "predeclared_gates": {
                "reserve_claim_residual_minor_units": 0,
                "bank_reserve_minimum_minor_units": 0,
                "deposit_liability_maximum_minor_units": 0,
                "liquidity_ratio_at_least_bank_floor": True,
                "negative_control_rejected_for_reserve": True,
                "expected_bank_month_rows": expected_rows,
            },
            "status": "pass" if not (error or failed_commands or violations or len(observations) != expected_rows) else "fail",
            "observed_bank_month_rows": len(observations),
            "violation_count": len(violations),
            "violations": violations,
            "failed_command_count": len(failed_commands),
            "negative_control": negative_control,
            "error": error,
            "commands": commands,
            "runtime_seconds": round(time.perf_counter() - started, 3),
            "environment": {"python": sys.version.split()[0], "platform": platform.platform()},
            "sha256": {"script": digest(Path(__file__)), "protocol": digest(PROTOCOL), "cli": digest(cli),
                       "scenario_template": digest(output / "scenario_template.json") if (output / "scenario_template.json").exists() else None,
                       "scenarios": scenario_hashes,
                       "observations": digest(output / "observations.csv") if (output / "observations.csv").exists() else None},
        }
        write_json(output / "summary.json", summary)
    return 0 if summary["status"] == "pass" else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, default=REPO / "target/debug/owg")
    parser.add_argument("--output", type=Path, required=True, help="new directory for compact evidence")
    args = parser.parse_args()
    return run(args.cli.resolve(), args.output.resolve())


if __name__ == "__main__":
    sys.exit(main())
