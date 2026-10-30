#!/usr/bin/env python3
"""Run a synthetic OpenWorldGen MVP, verify replay, and optionally call local Qwen.

No pip dependencies. Output must be a NEW directory. This does not download data,
train RL, or admit historical US inputs. Model rationales are not causal evidence.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CODE = ROOT / "research/us-economy-2007-2023/code"


def save(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def load(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def fixture(base: dict, months: int, households: int = 96) -> dict:
    """Deliberately fictional economy; terms/weights are transparent assumptions."""
    base["scenario_id"] = "openworldgen-mvp-synthetic-v1"
    base["title"] = "Runnable development MVP — synthetic, not a US forecast"
    base["calendar"]["months"] = months
    base["population"].update(households=households, firms=max(4, households // 8), banks=2)
    shock = min(24, months // 3)
    duration = min(12, months - shock)
    base["shocks"] = [{"id": "energy-stress", "start_month": shock,
        "duration_months": duration, "decay_months": min(12, months - shock - duration),
        "type": "import_price", "sector": "energy", "multiplier": 1.30}]
    base["policy_branches"] = [
        {"id": "baseline", "label": "Baseline reaction rule", "actions": []},
        {"id": "monetary-tightening", "label": "Additional temporary tightening", "actions": [
            {"id": "tighten", "start_month": shock, "duration_months": duration,
             "type": "policy_rate_delta", "basis_points": 100}]},
        {"id": "targeted-transfers", "label": "Bottom-40-percent transfers", "actions": [
            {"id": "transfer", "start_month": shock, "duration_months": duration,
             "type": "targeted_transfer", "amount_per_household": 2500, "bottom_share": 0.4}]},
    ]
    freeze = min(12, months - 2)
    base["governance"] = {
        "schema_version": "owg.governance.v1",
        "groups": [{"id": name, "label": name, "weight": weight}
                   for name, weight in [("companies", 1), ("unions", 2), ("civic", 2)]],
        "gates": [{"id": domain + "-office", "domain": domain,
            "passage_threshold": 0.6, "amendment_surcharge": 0.1, "review_delay_months": 0}
            for domain in ["credit", "corporate-transfer"]],
        "laws": [{"id": name, "domain": domain, "enabled": True, "resistance": 0.0}
            for name, domain in [("credit-permission", "credit"), ("owner-transfer-permission", "corporate-transfer")]],
        "proposals": [{"id": "freeze-new-contract-credit", "submitted_month": freeze,
            "domain": "credit", "sponsor_id": "unions",
            "stances": [{"group_id": name, "value": value}
                        for name, value in [("companies", -1), ("unions", 1), ("civic", 1)]],
            "change": {"type": "amend_law", "law_id": "credit-permission", "enabled": False, "resistance": 0.0}}],
    }
    contracts = []
    for name, lender, borrower, origin, amount in [
        ("bank-working-capital", "BANK-001", "FIRM-00000", 0, 100_000),
        ("household-funded", "HH-000001", "HH-000000", 0, 10_000),
        ("after-law-change", "BANK-000", "FIRM-00001", freeze + 1, 25_000),
    ]:
        contracts.append({"id": name, "lender_id": lender, "borrower_id": borrower,
            "principal": amount, "origination_month": origin, "first_payment_month": origin + 1,
            "maturity_month": origin + 24, "annual_interest_basis_points": 800,
            "monthly_principal_basis_points": 500, "missed_payment_limit": 3, "payment_priority": 0})
    base["credit_market"] = {"schema_version": "owg.credit-market.v1", "service_order": "before_legacy_debt",
        "origination_law": {"governance_schema_version": "owg.governance.v1", "law_id": "credit-permission", "domain": "credit"},
        "contracts": contracts}
    return base


def actor_spec(observation: dict, month: int) -> dict:
    choices = [{"id": "retain", "action": {"type": "no_op"}}]
    amount = min(5000, max(0, observation["liquidity"]) // 20)
    if amount:
        choices.append({"id": "small-owner-transfer", "action": {
            "type": "direct_owner_cash_transfer", "recipient_owner_id": "HH-000000", "amount": amount}})
    return {"id": f"mvp-month-{month}", "steps": [{
        "as_of": f"2020-{month + 1:02d}-01T00:00:00Z",
        "observations": {"observations": [observation]},
        "actor_contexts": {"FIRM-00000": {
            "role": "Executive of the observed synthetic energy firm; balance operating liquidity and owner returns.",
            "constraints": ["This is a fictional development world, not US history.",
                "Amounts are fixed integer minor units, not a dividend/profit/tax calculation.",
                "Only choose an offered action. The engine checks ownership, funds, and the bound law."],
            "choices": choices,
        }},
    }]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--months", type=int, default=120)
    parser.add_argument("--households", type=int, default=96)
    parser.add_argument("--replications", type=int, default=2)
    parser.add_argument("--agent-months", type=int, default=6)
    parser.add_argument("--llm", action="store_true", help="launch this run's loopback-only llama-server and use the copied model")
    parser.add_argument("--model", type=Path, default=ROOT / "local-models/Qwen3.8-27B-UD-Q4_K_M.gguf")
    parser.add_argument("--port", type=int, default=18084)
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if not 6 <= args.months <= 1200 or not 12 <= args.households <= 10_000:
        parser.error("months must be 6..1200 and households 12..10000")
    if not 1 <= args.agent_months <= min(12, args.months) or not 1 <= args.replications <= 16:
        parser.error("agent-months must be 1..12 and <= months; replications must be 1..16")
    if args.llm and (not args.model.is_file() or not shutil.which("llama-server")):
        parser.error("--llm requires a local GGUF model and llama-server on PATH")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    owg = ROOT / "target/debug/owg"
    started = time.monotonic()
    commands = []
    def run(*arguments):
        command = [str(owg), *map(str, arguments)]
        commands.append(command)
        subprocess.run(command, cwd=ROOT, check=True)
    summary = {"status": "running", "classification": "development_simulation",
        "empirical_us_initialized": False, "forecast_qualified": False,
        "created_at_utc": datetime.now(timezone.utc).isoformat(), "commands": commands}
    save(out / "mvp_status.json", summary)
    server = None
    server_log = None
    try:
        if not args.skip_build:
            subprocess.run(["cargo", "build", "-p", "owg-cli"], cwd=ROOT, check=True)
        run("init-scenario", "--output", out / "scenario.json")
        scenario = fixture(load(out / "scenario.json"), args.months, args.households)
        save(out / "scenario.json", scenario)
        run("validate", out / "scenario.json", "--json")
        run("run", out / "scenario.json", "--replications", args.replications, "--output", out / "economy")
        run("validate", out / "economy", "--kind", "result-bundle", "--json")
        split = args.months // 2
        run("checkpoint", out / "scenario.json", "--month", split, "--output", out / "checkpoint.json")
        run("resume", out / "scenario.json", out / "checkpoint.json", "--output", out / "resumed.json")
        run("checkpoint", out / "scenario.json", "--month", args.months, "--output", out / "direct.json")
        resumed, direct = load(out / "resumed.json"), load(out / "direct.json")
        for field in ["economy", "metrics", "journal_hash"]:
            if resumed[field] != direct[field]:
                raise RuntimeError(f"checkpoint replay differs: {field}")
        late = direct["economy"]["contract_credit"]["positions"]["after-law-change"]
        if late["status"] != "rejected_law":
            raise RuntimeError("expected law-change rejection was not observed")
        sys.path.insert(0, str(CODE))
        import local_decision_adapter as adapter
        model_hash = None
        if args.llm:
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", args.port))
            server_log = (out / "llama-server.log").open("w", encoding="utf-8")
            server = subprocess.Popen(["llama-server", "--model", str(args.model.resolve()),
                "--host", "127.0.0.1", "--port", str(args.port), "--ctx-size", "4096",
                "--parallel", "1", "--gpu-layers", "99", "--reasoning", "off", "--alias", "owg-qwen-local"],
                stdout=server_log, stderr=subprocess.STDOUT)
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            deadline = time.monotonic() + 120
            while True:
                if server.poll() is not None:
                    raise RuntimeError("llama-server exited before readiness; inspect llama-server.log")
                try:
                    with opener.open(f"http://127.0.0.1:{args.port}/health", timeout=1) as response:
                        if response.status == 200: break
                except (OSError, urllib.error.URLError):
                    if time.monotonic() > deadline: raise TimeoutError("model startup exceeded 120 seconds")
                    time.sleep(0.25)
            model_hash = adapter.sha256_file(args.model)
        law = {"schema_version": "owg.firm-action-law-binding.v1", "governance_schema_version": "owg.governance.v1",
            "domain": "corporate-transfer", "law_id": "owner-transfer-permission"}
        save(out / "firm-law.json", law)
        run("observe-firm", out / "scenario.json", "--output", out / "observation-0.json")
        observation = load(out / "observation-0.json")
        tape = None
        accepted = fallback = 0
        for month in range(args.agent_months):
            spec = actor_spec(observation, month)
            save(out / f"actor-input-{month}.json", spec)
            if args.llm:
                if server.poll() is not None: raise RuntimeError("model server exited during run")
                single, audit = adapter.generate_tape(spec, model_sha256=model_hash,
                    model_path=str(args.model.resolve()), server=f"http://127.0.0.1:{args.port}", timeout_seconds=60, max_calls=1)
                save(out / f"model-audit-{month}.json", audit)
                accepted += audit["accepted_count"]
                fallback += audit["fallback_count"]
            else:
                choices = spec["steps"][0]["actor_contexts"]["FIRM-00000"]["choices"]
                action = choices[-1 if month % 2 == 0 else 0]["action"]
                single = {"schema_version": "owg.decision-tape.v1", "id": "mvp-reference",
                    "descriptor": {"id": "reference-menu", "version": "1", "sha256": adapter.sha256_file(Path(__file__)),
                        "kind": "rule", "deterministic": True, "requires_network": False, "maximum_latency_ms": 1},
                    "steps": [{"observations": {"observations": [observation]},
                        "actions": {"actions": [action], "warnings": [], "used_fallback": False},
                        "rationales": ["Synthetic deterministic reference menu, not AI."],
                        "expected_effects": ["Apply only the chosen cash action before the month."], "confidences": [1.0]}]}
            if tape is None:
                tape = {**single, "id": "mvp-adaptive-firm", "steps": []}
            if single["descriptor"] != tape["descriptor"]:
                raise RuntimeError("policy descriptor changed during adaptive run")
            tape["steps"].extend(single["steps"])
            save(out / "decisions.json", tape)
            run("run-recorded-firm", out / "scenario.json", out / "decisions.json", "--law-binding", out / "firm-law.json",
                "--output", out / f"strategic-{month + 1}.json")
            observation = load(out / f"strategic-{month + 1}.json")["next_firm_observation"]
        run("run-recorded-firm", out / "scenario.json", out / "decisions.json", "--law-binding", out / "firm-law.json", "--output", out / "strategic-replay.json")
        if load(out / "strategic-replay.json") != load(out / f"strategic-{args.agent_months}.json"):
            raise RuntimeError("strategic decision replay differs")
        if args.llm and accepted == 0:
            raise RuntimeError("no actual model proposal was accepted; inspect model audits")
        summary.update(status="passed", checkpoint_replay_equal=True, strategic_replay_equal=True,
            law_change_rejection_observed=True, months=args.months, households=args.households,
            replications=args.replications, strategic_months=args.agent_months,
            strategic_policy="local_llm" if args.llm else "deterministic_reference_not_ai",
            model_proposals_accepted=accepted, model_proposals_fallback=fallback, model_sha256=model_hash,
            runtime_seconds=time.monotonic() - started,
            limitations=["Synthetic population and opening accounts, not measured US calibration.",
                "No trained RL, runtime MatrAIx integration, or forecast qualification.",
                "Strategic actions are limited to one firm's no-op/direct-owner cash transfer.",
                "Adaptive replay rebuilds the prefix: O(M squared) monthly engine work, suitable only for this bounded demo.",
                "Ordinary result bundle uses baseline behavior; the AI experiment is a separate recorded run."])
        save(out / "mvp_status.json", summary)
        print(f"MVP PASSED: {out / 'economy/plots.html'}", flush=True)
        return 0
    except Exception as exc:
        summary.update(status="failed", error=f"{type(exc).__name__}: {exc}", runtime_seconds=time.monotonic() - started)
        save(out / "mvp_status.json", summary)
        raise
    finally:
        if server is not None:
            server.terminate()
            try: server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=10)
        if server_log is not None: server_log.close()


if __name__ == "__main__":
    raise SystemExit(main())
