"""Render paired US holdout forecasts using the economic application's native graph UI."""
import argparse
import hashlib
import json
import math
from pathlib import Path

from attach_observed_outcomes import checked_rows, table_section

REPO = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("experiment", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("use a new output path")
    csv_path = args.experiment / "predictions.csv"
    summary = json.loads((args.experiment / "summary.json").read_text())
    if summary["predictions_sha256"] != hashlib.sha256(csv_path.read_bytes()).hexdigest():
        raise ValueError("forecast CSV differs from scored artifact")
    rows, audit = checked_rows(csv_path)
    models = ["persistence", "expanding_ar1", "local_qwen"]
    labels = ["Observed next vintage", "Persistence prediction", "AR(1) prediction", "Qwen agent prediction"]
    dates = sorted({row["target_as_of"][:7] for row in rows})
    base = int(dates[0][:4]) * 12 + int(dates[0][5:7]) - 1
    months = {date: int(date[:4]) * 12 + int(date[5:7]) - 1 - base for date in dates}
    metrics = []
    names = {"ROUTPUT": ("real_gdp", "Real GDP growth (annualized q/q)"),
        "RUC": ("unemployment", "Unemployment rate"), "CPI": ("inflation_year_over_year", "CPI inflation (12-month)")}
    for series, (key, label) in names.items():
        subset = [row for row in rows if row["series"] == series]
        actuals = {}
        for row in subset:
            month = months[row["target_as_of"][:7]]
            actual = float(row["actual_next_vintage"])
            if month in actuals and not math.isclose(actuals[month], actual, abs_tol=1e-10):
                raise ValueError("inconsistent observed value across model rows")
            actuals[month] = actual
        traces = [{"branch_id": labels[0], "replication_count": 1,
            "points": [{"month": month, "mean": actual, "lower": None, "upper": None} for month, actual in sorted(actuals.items())]}]
        for model, model_label in zip(models, labels[1:]):
            points = [{"month": months[row["target_as_of"][:7]], "mean": float(row["prediction"]),
                "lower": float(row["interval_lower_80"]) if row["interval_lower_80"] else None,
                "upper": float(row["interval_upper_80"]) if row["interval_upper_80"] else None}
                for row in subset if row["model"] == model]
            if len(points) != len(actuals): raise ValueError("model/outcome comparison coverage differs")
            traces.append({"branch_id": model_label, "replication_count": 1, "points": points})
        metrics.append({"key": key, "label": label, "axis_label": "Percent", "format": "percent", "decimals": 2,
            "include_zero": True, "series": traces})
    payload = {"plot_version": "owg.plot.v1", "view_kind": "historical_forecast_comparison",
        "scenario_id": "Real US data — Qwen agent versus observed outcomes", "run_id": "us-agent-heldout-2020-2023",
        "validation_status": "Retrospective exploratory holdout", "confidence_level": 0.8,
        "evaluation_start_month": 0, "evaluation_end_month": max(months.values()), "branches": labels,
        "periods": [{"month": months[date], "label": date} for date in dates], "metrics": metrics,
        "policy_scores": [], "minimax_regret_branch": None,
        "warnings": ["Forecasting agent on real US data, NOT a calibrated US agent economy.",
            "Future labels withheld from prompts; historical outcomes may already exist in pretrained weights.",
            f"Invalid-call persistence fallbacks: {summary['fallback_origin_calls']} of 15 origin calls."]}
    template = (REPO / "crates/owg-cli/src/plot_template.html").read_text(encoding="utf-8")
    encoded = json.dumps(payload, allow_nan=False).replace("<", "\\u003c")
    document = template.replace("__OWG_PLOT_PAYLOAD__", encoded, 1)
    document = document.replace("</main>", table_section(rows, csv_path) + "</main>")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(document, encoding="utf-8")
    audit.update(output_path=str(args.output.resolve()), output_sha256=hashlib.sha256(args.output.read_bytes()).hexdigest(),
        scored_summary_sha256=hashlib.sha256((args.experiment / "summary.json").read_bytes()).hexdigest(),
        source_csv_sha256=hashlib.sha256(csv_path.read_bytes()).hexdigest(),
        boundary="Forecasting actor on real US vintages; no claim of full US economy initialization or contamination-free historical skill")
    (args.experiment / "plot_audit.json").write_text(json.dumps(audit, indent=2) + "\n")
    print(json.dumps({"status": "passed", "rows_checked": len(rows), "output": str(args.output)}))


if __name__ == "__main__":
    main()
