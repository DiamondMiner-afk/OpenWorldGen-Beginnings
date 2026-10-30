#!/usr/bin/env python3
"""Benchmark local GGUF strategic-decision JSON calls, then replay saved outputs.

All prompts are fictional and self-contained. This measures inference throughput and
record/replay integrity, not economic forecast accuracy.
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import threading
import time
from typing import Any
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
MODEL = ROOT.parents[1] / "local-models/Qwen3.8-27B-UD-Q4_K_M.gguf"
OUTPUT = ROOT / "artifacts/local_strategy_benchmark.json"
ALLOWED = {
    "firm-01": {"hold_price", "raise_price_5pct", "invest_capacity"},
    "union-01": {"accept_offer", "negotiate", "strike"},
}
ACTORS = {
    "firm-01": {
        "role": "executive of a fictional appliance maker",
        "known_state": {
            "period": "scenario-year-1-month-4",
            "cash_millions": 12,
            "debt_millions": 23,
            "monthly_revenue_millions": 8,
            "monthly_payroll_millions": 2.6,
            "input_cost_change_pct": 7,
            "inventory_months": 1.4,
            "workers": 240,
            "demand_signal": "flat but noisy",
            "credit_line_remaining_millions": 4,
        },
        "allowed_actions": ["hold_price", "raise_price_5pct", "invest_capacity"],
        "constraint": "No coordination with rivals; available cash and credit must cover investment.",
    },
    "union-01": {
        "role": "leader of a fictional factory-workers union",
        "known_state": {
            "period": "scenario-year-1-month-4",
            "strike_fund_millions": 1.2,
            "members": 180,
            "current_monthly_wage": 4200,
            "employer_offer_raise_pct": 3,
            "local_price_change_pct": 6,
            "member_support_for_strike_pct": 58,
            "contract_expires_in_days": 28,
        },
        "allowed_actions": ["accept_offer", "negotiate", "strike"],
        "constraint": "A strike requires a member vote and cannot begin before the contract expires.",
    },
}


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def digest_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def prompt_for(actor_id: str) -> str:
    return (
        "You are one bounded decision-maker in a fictional economy. Use only the "
        "time-stamped information supplied here. Do not assume hidden facts, later "
        "outcomes, or authority you lack. Return exactly one compact JSON object with "
        "keys actor_id, action, rationale, expected_effect, confidence. Choose one "
        "allowed action. Rationale and expected_effect must each be one short sentence; "
        "confidence is a number from 0 to 1. This is a decision proposal, not a "
        "forecast or evidence of causation. No markdown.\n"
        + json.dumps({"actor_id": actor_id, **ACTORS[actor_id]}, separators=(",", ":"))
    )


def request_for(actor_id: str) -> dict[str, Any]:
    return {
        "model": "owg-qwen-local",
        "messages": [{"role": "user", "content": prompt_for(actor_id)}],
        "temperature": 0,
        "seed": 17 if actor_id == "firm-01" else 18,
        "max_tokens": 192,
        "response_format": {"type": "json_object"},
        "stream": False,
    }


def validate_decision(actor_id: str, content: str) -> dict[str, Any]:
    answer = json.loads(content)
    if not isinstance(answer, dict):
        raise ValueError("decision is not a JSON object")
    if answer.get("actor_id") != actor_id or answer.get("action") not in ALLOWED[actor_id]:
        raise ValueError("actor/action mismatch or unauthorized action")
    if not isinstance(answer.get("rationale"), str) or not answer["rationale"].strip():
        raise ValueError("missing rationale")
    if not isinstance(answer.get("expected_effect"), str) or not answer["expected_effect"].strip():
        raise ValueError("missing expected effect")
    confidence = answer.get("confidence")
    if isinstance(confidence, bool) or not isinstance(confidence, (float, int)) or not 0 <= confidence <= 1:
        raise ValueError("confidence must be numeric in [0,1]")
    return answer


def request_model(base_url: str, actor_id: str) -> dict[str, Any]:
    request_data = request_for(actor_id)
    payload = json.dumps(request_data, separators=(",", ":")).encode()
    request = urllib.request.Request(
        base_url + "/v1/chat/completions", data=payload,
        headers={"Content-Type": "application/json"}, method="POST",
    )
    start = time.monotonic()
    with urllib.request.urlopen(request, timeout=180) as response:
        raw = json.load(response)
    elapsed = time.monotonic() - start
    content = raw["choices"][0]["message"]["content"]
    decision = validate_decision(actor_id, content)
    return {
        "actor_id": actor_id,
        "request": request_data,
        "request_sha256": digest_bytes(payload),
        "response_content": content,
        "response_sha256": digest_bytes(content.encode()),
        "decision": decision,
        "usage": raw.get("usage"),
        "timings": raw.get("timings"),
        "latency_seconds": elapsed,
        "prompt_chars": len(prompt_for(actor_id)),
    }


def system_free_percent() -> int | None:
    completed = subprocess.run(["memory_pressure", "-Q"], capture_output=True, text=True, check=False)
    match = re.search(r"System-wide memory free percentage: (\d+)%", completed.stdout)
    return int(match.group(1)) if match else None


def observed_rss_kib(pid: int) -> int | None:
    result = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True, check=False)
    try:
        return int(result.stdout.strip())
    except ValueError:
        return None


def monitor_rss(pid: int, stop: threading.Event, samples: list[int]) -> None:
    while not stop.is_set():
        observed = observed_rss_kib(pid)
        if observed is not None:
            samples.append(observed)
        stop.wait(0.25)


def wait_for_server(base_url: str, process: subprocess.Popen[Any], timeout: float = 180) -> float:
    start = time.monotonic()
    while time.monotonic() - start < timeout:
        if process.poll() is not None:
            raise RuntimeError(f"llama-server exited with code {process.returncode}")
        try:
            with urllib.request.urlopen(base_url + "/health", timeout=2) as response:
                status = json.load(response).get("status")
            if status == "ok":
                return time.monotonic() - start
        except (urllib.error.URLError, TimeoutError, ValueError):
            pass
        time.sleep(0.5)
    raise TimeoutError("llama-server did not become ready")


def run_pair(base_url: str, concurrent: bool) -> dict[str, Any]:
    start = time.monotonic()
    if concurrent:
        barrier = threading.Barrier(3)

        def call(actor_id: str) -> dict[str, Any]:
            barrier.wait()
            return request_model(base_url, actor_id)

        with ThreadPoolExecutor(max_workers=2) as pool:
            futures = [pool.submit(call, actor_id) for actor_id in ACTORS]
            barrier.wait()
            results = [future.result() for future in futures]
    else:
        results = [request_model(base_url, actor_id) for actor_id in ACTORS]
    elapsed = time.monotonic() - start
    completed_tokens = sum((result["usage"] or {}).get("completion_tokens", 0) for result in results)
    return {
        "mode": "concurrent_2" if concurrent else "sequential_1",
        "wall_seconds": elapsed,
        "completed_tokens": completed_tokens,
        "effective_completion_tokens_per_wall_second": completed_tokens / elapsed,
        "calls": results,
    }


def replay(path: Path) -> dict[str, Any]:
    artifact = json.loads(path.read_text(encoding="utf-8"))
    checked = 0
    for group in artifact["groups"]:
        for call in group["calls"]:
            actor_id = call["actor_id"]
            current_request = request_for(actor_id)
            if call["request"] != current_request:
                raise ValueError(f"request drift for {actor_id}")
            payload = json.dumps(current_request, separators=(",", ":")).encode()
            if digest_bytes(payload) != call["request_sha256"]:
                raise ValueError(f"request hash mismatch for {actor_id}")
            if digest_bytes(call["response_content"].encode()) != call["response_sha256"]:
                raise ValueError(f"response hash mismatch for {actor_id}")
            if validate_decision(actor_id, call["response_content"]) != call["decision"]:
                raise ValueError(f"parsed-decision mismatch for {actor_id}")
            checked += 1
    return {"status": "recorded_decisions_replayed", "checked_calls": checked,
            "model_call_count": 0, "artifact": str(path.resolve())}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, default=MODEL)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--port", type=int, default=18082)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--replay", type=Path, help="validate and replay saved decisions without a model call")
    args = parser.parse_args()
    if args.replay:
        print(json.dumps(replay(args.replay), indent=2))
        return 0
    if args.repeats < 1 or args.repeats > 5:
        parser.error("--repeats must be 1..5")
    if not args.model.is_file():
        parser.error(f"model not found: {args.model}")
    memory_bytes = int(subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True).strip())
    if memory_bytes < 64 * 1024**3:
        parser.error("benchmark needs at least 64 GiB physical memory for this 27B model")
    free_before = system_free_percent()
    if free_before is not None and free_before < 30:
        parser.error(f"memory pressure too high: {free_before}% system-wide free")
    with socket.socket() as probe:
        try:
            probe.bind(("127.0.0.1", args.port))
        except OSError as exc:
            parser.error(f"port {args.port} unavailable: {exc}")

    base_url = f"http://127.0.0.1:{args.port}"
    command = [
        "llama-server", "--model", str(args.model.resolve()),
        "--host", "127.0.0.1", "--port", str(args.port),
        "--ctx-size", "4096", "--parallel", "2", "--gpu-layers", "99",
        "--reasoning", "off", "--alias", "owg-qwen-local",
    ]
    stop = threading.Event()
    rss_samples: list[int] = []
    with tempfile.TemporaryFile(mode="w+t") as server_log:
        process = subprocess.Popen(command, stdout=server_log, stderr=subprocess.STDOUT)
        monitor = threading.Thread(target=monitor_rss, args=(process.pid, stop, rss_samples), daemon=True)
        monitor.start()
        try:
            load_seconds = wait_for_server(base_url, process)
            warmup = request_model(base_url, "firm-01")
            groups = []
            for repeat in range(args.repeats):
                groups.append({"repeat": repeat + 1, **run_pair(base_url, concurrent=False)})
                groups.append({"repeat": repeat + 1, **run_pair(base_url, concurrent=True)})
            artifact = {
                "status": "local_inference_benchmark_not_forecast_validation",
                "created_at_utc": datetime.now(timezone.utc).isoformat(),
                "hardware": {
                    "chip": subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip(),
                    "physical_memory_bytes": memory_bytes,
                    "system_free_percent_before": free_before,
                    "system_free_percent_after": system_free_percent(),
                    "peak_observed_process_rss_kib": max(rss_samples) if rss_samples else None,
                    "rss_sample_count": len(rss_samples),
                    "rss_caveat": "ps RSS samples may not capture all Metal/unified-memory allocations; sampled every 0.25 s",
                },
                "server": {
                    "version": subprocess.check_output(["llama-server", "--version"], text=True).strip(),
                    "command": command,
                    "load_seconds": load_seconds,
                    "context_total_tokens": 4096,
                    "parallel_slots": 2,
                },
                "model": {"path": str(args.model.resolve()), "bytes": args.model.stat().st_size,
                          "sha256": digest_file(args.model)},
                "warmup": {"latency_seconds": warmup["latency_seconds"], "usage": warmup["usage"],
                            "timings": warmup["timings"]},
                "groups": groups,
                "scope_warning": "Synthetic two-actor systems benchmark only; no historical or future forecasting quality established.",
            }
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
            replay_summary = replay(args.output)
            print(json.dumps({
                "artifact": str(args.output.resolve()),
                "server_load_seconds": load_seconds,
                "peak_observed_process_rss_kib": artifact["hardware"]["peak_observed_process_rss_kib"],
                "groups": [{key: group[key] for key in ("repeat", "mode", "wall_seconds", "completed_tokens", "effective_completion_tokens_per_wall_second")}
                           for group in groups],
                "replay": replay_summary,
            }, indent=2))
        except Exception:
            server_log.seek(0)
            print("llama-server log tail:\n" + server_log.read()[-3000:])
            raise
        finally:
            stop.set()
            monitor.join(timeout=2)
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
