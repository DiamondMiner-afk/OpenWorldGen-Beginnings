"""US Qwen release-vintage holdout. Labels withheld, pretrained-memory leakage uncontrolled.

This forecasts measured US series; it does NOT initialize the economic simulator
with a calibrated US state. Test protocol is frozen before any model response.
"""
import argparse
import csv
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import socket
import statistics
import subprocess
import time
import urllib.error
import urllib.request

from backtest_philadelphia import (Forecast, SERIES, VINTAGE_ROOT, evaluate_forecasts,
    indicator, load_vintage, origin_schedule, period_index)
from local_decision_adapter import canonical, request_completion, sha256_file, strict_json

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
TEST_ORIGINS = tuple(f"{year}Q{q}" for year in range(2020, 2024) for q in range(1, 5) if (year, q) <= (2023, 3))
PROMPT = '''Forecast ONLY from supplied past US macroeconomic observations. Dates
and future outcomes are hidden. Predict the latest value in the NEXT quarterly
data vintage, normally about three observation months ahead (CPI may differ due
to release timing). ROUTPUT = real GDP growth annualized q/q, percent. RUC =
unemployment rate, percent. CPI = 12-month inflation, percent. Histories are
chronological, ending with the latest available value. Do not infer dates or
import remembered events. Return ONLY JSON with exactly ROUTPUT, RUC, CPI; each
must have exactly prediction, lower_80, upper_80 as finite numbers satisfying
lower_80 <= prediction <= upper_80. Give your own 80% predictive interval, not
a validated coverage claim. No prose or code fences.'''


def save(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def histories(vintage):
    result = {}
    for series in SERIES:
        rows = vintage.observations[series]
        if series == "ROUTPUT":
            values = [100 * ((b[1] / a[1]) ** 4 - 1) for a, b in zip(rows, rows[1:])][-32:]
        elif series == "RUC":
            values = [value for _, value in rows[-32:]]
        else:
            indexed = {period_index(period): value for period, value in rows}
            values = [100 * (value / indexed[period_index(period) - 12] - 1)
                      for period, value in rows if period_index(period) - 12 in indexed][-32:]
        if len(values) != 32 or not all(math.isfinite(v) for v in values):
            raise ValueError("incomplete origin histories")
        result[series] = {"history": values, "latest": indicator(vintage, series)[1]}
    return result


def proposal(raw):
    answer = strict_json(raw["choices"][0]["message"]["content"])
    if set(answer) != set(SERIES): raise ValueError("missing indicator")
    for series, values in answer.items():
        if set(values) != {"prediction", "lower_80", "upper_80"}: raise ValueError("wrong forecast fields")
        if any(type(v) not in (int, float) or not math.isfinite(v) for v in values.values()): raise ValueError("nonfinite forecast")
        if not values["lower_80"] <= values["prediction"] <= values["upper_80"]: raise ValueError("unordered interval")
        if series == "RUC" and not 0 <= values["lower_80"] <= values["upper_80"] <= 100: raise ValueError("unemployment outside bounds")
    return answer


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", type=Path, default=REPO / "local-models/Qwen3.8-27B-UD-Q4_K_M.gguf")
    parser.add_argument("--port", type=int, default=18086)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    out = args.output.resolve()
    started = time.monotonic()
    baseline_path = ROOT / "artifacts/backtest_philadelphia_2007_2023.csv"
    with baseline_path.open(newline="", encoding="utf-8") as stream:
        baseline = [row for row in csv.DictReader(stream) if row["origin"] in TEST_ORIGINS]
    if len(baseline) != 90: raise ValueError("baseline holdout coverage changed")
    inputs = {origin: histories(load_vintage(VINTAGE_ROOT, origin)) for origin in TEST_ORIGINS}
    protocol = {"schema_version": "owg.us-agent-holdout.v1", "frozen_at_utc": datetime.now(timezone.utc).isoformat(),
        "origins": TEST_ORIGINS, "forecast_count": 45, "model_calls": 15, "prompt": PROMPT,
        "primary_metric": "MAE percentage points by indicator on all 15 origins, versus matched persistence and AR(1)",
        "input_hashes": {key: hashlib.sha256(canonical(value)).hexdigest() for key, value in inputs.items()},
        "model_sha256": sha256_file(args.model), "baseline_sha256": sha256_file(baseline_path),
        "code_sha256": sha256_file(Path(__file__)), "temperature": 0, "max_tokens": 384,
        "retrospective": True, "pretraining_contamination_controlled": False, "empirical_us_economy_initialized": False,
        "limits": ["Labels withheld from prompts and dates masked; pretrained weights may still contain test history.",
            "Analyst has seen historical outcomes: not a prospective blind trial; no prompt tuning after this freeze.",
            "Release-vintage reconstruction is not independent archive-origin admission.",
            "Invalid calls are flagged persistence fallbacks, not successful Qwen forecasts."]}
    save(out / "frozen_protocol.json", protocol)
    with socket.socket() as probe: probe.bind(("127.0.0.1", args.port))
    process = None
    rows, accepted, fallback = list(baseline), 0, 0
    try:
        with (out / "llama-server.log").open("w", encoding="utf-8") as log:
            process = subprocess.Popen(["llama-server", "--model", str(args.model.resolve()), "--host", "127.0.0.1",
                "--port", str(args.port), "--ctx-size", "4096", "--parallel", "1", "--gpu-layers", "99",
                "--reasoning", "off", "--alias", "owg-qwen-local"], stdout=log, stderr=subprocess.STDOUT)
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            deadline = time.monotonic() + 120
            while True:
                if process.poll() is not None: raise RuntimeError("model server exited")
                try:
                    with opener.open(f"http://127.0.0.1:{args.port}/health", timeout=1) as response:
                        if response.status == 200: break
                except (OSError, urllib.error.URLError):
                    if time.monotonic() > deadline: raise TimeoutError("model startup timeout")
                    time.sleep(0.25)
            schedule = origin_schedule()
            for index, origin in enumerate(TEST_ORIGINS):
                request = {"model": "owg-qwen-local", "temperature": 0, "seed": 20261024 + index, "max_tokens": 384,
                    "response_format": {"type": "json_object"}, "stream": False,
                    "messages": [{"role": "system", "content": PROMPT}, {"role": "user", "content": canonical(inputs[origin]).decode()}]}
                call = {"origin": origin, "request": request, "input_sha256": protocol["input_hashes"][origin]}
                try:
                    raw = request_completion(request, f"http://127.0.0.1:{args.port}", 60)
                    call["raw_response"] = raw
                    forecasts = proposal(raw)
                    call["status"] = "accepted"
                    accepted += 1
                except (ValueError, KeyError, TypeError, IndexError, OSError, TimeoutError) as error:
                    forecasts = {s: {"prediction": inputs[origin][s]["latest"], "lower_80": None, "upper_80": None} for s in SERIES}
                    call.update(status="persistence_fallback", error=f"{type(error).__name__}: {error}")
                    fallback += 1
                call["forecasts"] = forecasts
                save(out / f"proposal-{origin}.json", call)  # persist BEFORE opening target vintage
                source = load_vintage(VINTAGE_ROOT, origin)
                next_origin = schedule[schedule.index(origin) + 1]
                terms = []
                for series in SERIES:
                    period, feature, definition = indicator(source, series)
                    values = forecasts[series]
                    terms.append(Forecast(origin, next_origin, series, definition, period, feature,
                        "local_qwen", values["prediction"], 0, values["lower_80"], values["upper_80"], 0))
                records, _ = evaluate_forecasts(terms, load_vintage(VINTAGE_ROOT, next_origin))
                for row in records: row["origin_as_of"] = source.as_of.isoformat()
                rows.extend(records)
                print(f"{origin}: {call['status']} ({index + 1}/15)", flush=True)
        rows.sort(key=lambda row: (row["origin"], row["series"], row["model"]))
        with (out / "predictions.csv").open("w", newline="", encoding="utf-8") as stream:
            writer = csv.DictWriter(stream, fieldnames=list(baseline[0]))
            writer.writeheader()
            writer.writerows(rows)
        scores = {}
        for series in SERIES:
            scores[series] = {}
            for model in ["local_qwen", "persistence", "expanding_ar1"]:
                subset = [r for r in rows if r["series"] == series and r["model"] == model]
                errors = [float(r["error"]) for r in subset]
                intervals = [r for r in subset if r["interval_hit_80"] != ""]
                scores[series][model] = {"n": len(errors), "mae_pp": statistics.fmean(abs(e) for e in errors),
                    "rmse_pp": math.sqrt(statistics.fmean(e * e for e in errors)), "interval_n": len(intervals),
                    "coverage_80": statistics.fmean(int(r["interval_hit_80"]) for r in intervals) if intervals else None}
        save(out / "summary.json", {"status": "passed", "accepted_origin_calls": accepted, "fallback_origin_calls": fallback,
            "runtime_seconds": time.monotonic() - started, "scores": scores, "forecast_qualified": False,
            "protocol_sha256": sha256_file(out / "frozen_protocol.json"), "predictions_sha256": sha256_file(out / "predictions.csv"),
            "classification": "retrospective_exploratory_forecasting_actor_not_us_economy_validation"})
        print(json.dumps({"status": "passed", "accepted_calls": accepted, "fallback_calls": fallback, "output": str(out)}), flush=True)
    finally:
        if process is not None:
            process.terminate()
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)


if __name__ == "__main__":
    main()
