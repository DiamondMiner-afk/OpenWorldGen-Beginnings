"""Attach paired historical baseline predictions/outcomes to an MVP result view.

Does NOT match fictional agent-world output to US history. Each historical
baseline row is checked against its origin and target vintage before display.
Only standard-library dependencies; existing engine HTML remains authoritative
for synthetic paths. No new statistical model is fitted by this renderer.
"""
from __future__ import annotations
import argparse
import base64
import csv
from datetime import datetime
from functools import lru_cache
import hashlib
import html
import json
import math
from pathlib import Path

from backtest_philadelphia import VINTAGE_ROOT, indicator, load_vintage, period_index

ROOT = Path(__file__).resolve().parents[1]


def checked_rows(path: Path) -> tuple[list[dict], dict]:
    with path.open(newline="", encoding="utf-8") as stream:
        rows = list(csv.DictReader(stream))
    if not rows:
        raise ValueError("no historical prediction/outcome rows")
    seen = set()
    @lru_cache(maxsize=128)
    def vintage(origin):
        return load_vintage(VINTAGE_ROOT, origin)
    for row in rows:
        key = (row["origin"], row["series"], row["model"])
        if key in seen:
            raise ValueError("duplicate prediction grain")
        seen.add(key)
        if row["model"] not in {"persistence", "expanding_ar1", "local_qwen"}:
            raise ValueError("unknown baseline model")
        source, target = vintage(row["origin"]), vintage(row["target_vintage"])
        if source.as_of >= target.as_of or datetime.fromisoformat(row["origin_as_of"]) != source.as_of or datetime.fromisoformat(row["target_as_of"]) != target.as_of:
            raise ValueError("forecast origin/target release chronology mismatch")
        feature_period, feature, definition = indicator(source, row["series"])
        target_period, actual, _ = indicator(target, row["series"])
        prediction = float(row["prediction"])
        if (feature_period != row["feature_period"] or target_period != row["target_period"]
                or definition != row["indicator"] or not math.isfinite(prediction)
                or not math.isclose(float(row["feature"]), feature, abs_tol=1e-10)
                or not math.isclose(float(row["actual_next_vintage"]), actual, abs_tol=1e-10)
                or not math.isclose(float(row["error"]), prediction - actual, abs_tol=1e-10)
                or not math.isclose(float(row["absolute_error"]), abs(prediction - actual), abs_tol=1e-10)
                or int(row["horizon_observation_months"]) != period_index(target_period) - period_index(feature_period)):
            raise ValueError("prediction/outcome definitions or values do not match vintages")
        if row["interval_lower_80"] or row["interval_upper_80"]:
            lower, upper = float(row["interval_lower_80"]), float(row["interval_upper_80"])
            if not math.isfinite(lower) or not math.isfinite(upper) or lower > upper:
                raise ValueError("invalid interval bounds")
            expected_hit = str(int(lower <= actual <= upper))
            if row["interval_hit_80"] != expected_hit:
                raise ValueError("interval coverage flag mismatch")
    manifests = {origin: vintage(origin).manifest_hash
                 for origin in sorted({r["origin"] for r in rows} | {r["target_vintage"] for r in rows})}
    return rows, {"rows_checked": len(rows), "unique_predictions": len(seen),
        "horizon_observation_months": sorted({int(r["horizon_observation_months"]) for r in rows}),
        "target_as_of_start": min(r["target_as_of"] for r in rows), "target_as_of_end": max(r["target_as_of"] for r in rows),
        "vintage_manifest_hashes": manifests}


def table_section(rows: list[dict], source: Path) -> str:
    groups = sorted({(row["series"], row["model"]) for row in rows})
    sections = []
    for series, model in groups:
        subset = [row for row in rows if (row["series"], row["model"]) == (series, model)]
        cells = []
        for row in subset:
            interval = (f'{float(row["interval_lower_80"]):.3f} to {float(row["interval_upper_80"]):.3f}'
                        if row["interval_lower_80"] else "Unavailable (insufficient prior errors)")
            values = [row["origin"], row["origin_as_of"][:10], row["target_as_of"][:10], row["target_period"],
                row["horizon_observation_months"], f'{float(row["prediction"]):.3f}',
                f'{float(row["actual_next_vintage"]):.3f}', f'{float(row["error"]):+.3f}', interval]
            cells.append("<tr>" + "".join(f"<td>{html.escape(value)}</td>" for value in values) + "</tr>")
        headings = ["Origin vintage", "Prediction cutoff", "Outcome vintage date", "Outcome period", "Horizon (months)",
            "Predicted (%)", "Observed next vintage (%)", "Error (percentage points)", "80% empirical prediction interval"]
        sections.append(f'<details open><summary>{html.escape(subset[0]["indicator"])} — {html.escape(model)} ({len(subset)} predictions)</summary>'
            '<div style="overflow-x:auto"><table><thead><tr>' + "".join(f"<th>{h}</th>" for h in headings)
            + "</tr></thead><tbody>" + "".join(cells) + "</tbody></table></div></details>")
    encoded = base64.b64encode(source.read_bytes()).decode("ascii")
    section = '''<section class="panel" id="observed-outcomes" aria-label="Historical predicted versus observed outcomes">
<h2>US historical baselines: predicted versus observed</h2>
<p>These are persistence and expanding AR(1) benchmarks, <strong>not predictions from the agent economy or Qwen</strong>.</p>
<p>Observed means the value measured in the next quarterly data vintage, not today's revised final value or necessarily the first public release.
Actual observation horizons vary from 2 to 4 months. Error = prediction minus observed, in percentage points.
Intervals use only earlier released residuals; 80% coverage is a target, not a guarantee.</p>
<p><a href="https://www.philadelphiafed.org/surveys-and-data/real-time-data-research/real-time-data-set-for-macroeconomists">Source: Philadelphia Fed real-time data</a> ·
''' + f'<a download="predicted_vs_observed.csv" href="data:text/csv;base64,{encoded}">Download all paired predictions and outcomes</a></p>' + "".join(sections) + "</section>"
    if any(row["model"] == "local_qwen" for row in rows):
        section = section.replace("US historical baselines: predicted versus observed", "US forecast agent and baselines: paired outcomes")
        section = section.replace("These are persistence and expanding AR(1) benchmarks, <strong>not predictions from the agent economy or Qwen</strong>.",
            "Qwen forecasting-agent predictions and persistence/AR(1) baselines on real US data. This is <strong>not a calibrated US agent economy</strong>.")
        section = section.replace("Intervals use only earlier released residuals; 80% coverage is a target, not a guarantee.",
            "Baseline intervals use earlier released residuals; Qwen intervals are model-stated. Neither guarantees 80% coverage. Pretraining contamination is uncontrolled.")
        section = section.replace("80% empirical prediction interval", "80% prediction interval")
    return section


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--predictions", type=Path, default=ROOT / "artifacts/backtest_philadelphia_2007_2023.csv")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--audit", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.audit.exists():
        parser.error("use new output/audit paths; existing evidence is not overwritten")
    rows, audit = checked_rows(args.predictions)
    template = (args.bundle / "plots.html").read_text(encoding="utf-8")
    if template.count("</main>") != 1:
        raise ValueError("unsupported result-view structure")
    notice = '<p class="warning"><strong>Agent simulation: observed outcomes unavailable.</strong> This fictional economy has no matching real-world actual series. No forecast accuracy is measured for its curves. Historical baseline comparisons follow below.</p>'
    rendered = template.replace("</main>", table_section(rows, args.predictions) + "</main>")
    rendered = rendered.replace('<section class="controls"', notice + '<section class="controls"', 1)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(rendered, encoding="utf-8")
    audit.update(schema_version="owg.observed-outcome-view.v1", status="passed",
        predictions_sha256=hashlib.sha256(args.predictions.read_bytes()).hexdigest(),
        original_plot_sha256=hashlib.sha256((args.bundle / "plots.html").read_bytes()).hexdigest(),
        output_sha256=hashlib.sha256(args.output.read_bytes()).hexdigest(),
        source="Philadelphia Fed RTDSM cached vintages; origin authenticity not independently certified",
        boundary="Paired historical baselines only; no measured US agent simulation or forecast qualification")
    args.audit.parent.mkdir(parents=True, exist_ok=True)
    args.audit.write_text(json.dumps(audit, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": "passed", "rows_checked": len(rows), "output": str(args.output)}))


if __name__ == "__main__":
    main()
