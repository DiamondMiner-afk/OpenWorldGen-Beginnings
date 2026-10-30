"""Rolling-origin, first-next-vintage benchmark on three US macro indicators.

The benchmark deliberately does not use an LLM: model weights trained after an
origin cannot be made historically blind by a prompt. This script evaluates
simple, reproducible comparators before agent/LLM forecasts are compared.

At the 15th-of-middle-month origin vintage, forecast the indicator measured at
the *next* quarterly vintage. The target is that next vintage's then-latest
value, not a 2026-revised final value or necessarily its first public release. Features are extracted only from
the current vintage. Training pairs and interval residuals must have target
release times no later than the current origin.

Run from anywhere: python3 research/us-economy-2007-2023/code/backtest_philadelphia.py
Only Python's standard library is required. Outputs are deterministic.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import random
import statistics
from dataclasses import dataclass
from datetime import date, datetime
from pathlib import Path


RESEARCH_ROOT = Path(__file__).resolve().parents[1]
VINTAGE_ROOT = RESEARCH_ROOT / "data/processed/philadelphia-fed"
ARTIFACT_ROOT = RESEARCH_ROOT / "artifacts"
SERIES = ("ROUTPUT", "RUC", "CPI")
MIN_AR_TRAIN = 12
MIN_INTERVAL_RESIDUALS = 12
INTERVAL_COVERAGE = 0.80
BOOTSTRAP_REPLICATIONS = 2000
BOOTSTRAP_BLOCK_QUARTERS = 4
BOOTSTRAP_SEED = 20261017


@dataclass(frozen=True)
class Vintage:
    origin: str
    as_of: datetime
    observations: dict[str, tuple[tuple[str, float], ...]]
    manifest_hash: str


@dataclass(frozen=True)
class Forecast:
    origin: str
    target_vintage: str
    series: str
    indicator: str
    feature_period: str
    feature: float
    model: str
    prediction: float
    training_pairs: int
    interval_lower: float | None
    interval_upper: float | None
    interval_residuals: int


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def quarter_index(origin: str) -> int:
    if len(origin) != 6 or origin[4] != "Q" or not origin[:4].isdigit() or origin[5] not in "1234":
        raise ValueError(f"invalid origin {origin!r}")
    return int(origin[:4]) * 4 + int(origin[5]) - 1


def period_index(period: str) -> int:
    if len(period) == 7 and period[4:6] == ":Q" and period[6] in "1234" and period[:4].isdigit():
        return (int(period[:4]) * 4 + int(period[6]) - 1) * 3 + 2
    if len(period) == 7 and period[4] == ":" and period[:4].isdigit() and period[5:].isdigit():
        month = int(period[5:])
        if 1 <= month <= 12:
            return int(period[:4]) * 12 + month - 1
    raise ValueError(f"invalid observation period {period!r}")


def period_end(period: str) -> date:
    index = period_index(period)
    year, month_zero = divmod(index, 12)
    if month_zero == 11:
        return date(year, 12, 31)
    return date.fromordinal(date(year, month_zero + 2, 1).toordinal() - 1)


def load_vintage(root: Path, origin: str) -> Vintage:
    """Read and validate just one point-in-time snapshot, never a raw matrix."""
    folder = root / origin
    manifest_path = folder / "data-vintage.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    as_of = datetime.fromisoformat(manifest["as_of"].replace("Z", "+00:00"))
    if manifest["schema_version"] != "owg.data-vintage.v1" or manifest["id"] != f"us-{origin.lower()}-philly-macro":
        raise ValueError(f"unexpected manifest contract for {origin}")
    if as_of.date() != date(int(origin[:4]), (int(origin[5]) - 1) * 3 + 2, 15):
        raise ValueError(f"unexpected release cutoff for {origin}")
    if manifest["geography"] != "US":
        raise ValueError(f"unexpected geography for {origin}")
    sources = manifest["sources"]
    if len(sources) != len(SERIES) or {s["classifications"]["series"] for s in sources} != set(SERIES):
        raise ValueError(f"missing/duplicate source for {origin}")
    observations = {}
    for source in sources:
        series = source["classifications"]["series"]
        if source["release_id"] != series + origin[2:]:
            raise ValueError(f"wrong release ID for {series} {origin}")
        expected_frequency = "quarterly" if series == "ROUTPUT" else "monthly"
        if source["classifications"]["frequency"] != expected_frequency or source["geography"] != "US":
            raise ValueError(f"wrong series classification for {series} {origin}")
        published = datetime.fromisoformat(source["published_at"].replace("Z", "+00:00"))
        if published > as_of:
            raise ValueError(f"future publication in {origin}")
        relative = Path(source["relative_path"])
        if relative.is_absolute() or len(relative.parts) != 1 or relative.name != series.lower() + ".csv":
            raise ValueError(f"unexpected source path in {origin}: {relative}")
        path = folder / relative
        if sha256(path) != source["sha256"]:
            raise ValueError(f"source hash mismatch: {path}")
        with path.open(newline="", encoding="utf-8") as stream:
            reader = csv.DictReader(stream)
            if reader.fieldnames != ["observation_period", "value"]:
                raise ValueError(f"unexpected CSV header in {path}")
            rows = tuple((r["observation_period"], float(r["value"])) for r in reader)
        if not rows or any(not math.isfinite(value) for _, value in rows):
            raise ValueError(f"empty/nonfinite data in {path}")
        if len({p for p, _ in rows}) != len(rows) or [period_index(p) for p, _ in rows] != sorted(period_index(p) for p, _ in rows):
            raise ValueError(f"duplicate/unsorted periods in {path}")
        if series in ("ROUTPUT", "CPI") and any(value <= 0 for _, value in rows):
            raise ValueError(f"nonpositive level in {path}")
        if series == "RUC" and any(not 0 <= value <= 100 for _, value in rows):
            raise ValueError(f"invalid unemployment rate in {path}")
        if (source["observation_start"] != period_end(rows[0][0]).isoformat()
                or source["observation_end"] != period_end(rows[-1][0]).isoformat()
                or period_end(rows[-1][0]) > as_of.date()):
            raise ValueError(f"observation cutoff mismatch in {path}")
        observations[series] = rows
    return Vintage(origin, as_of, observations, sha256(manifest_path))


def indicator(vintage: Vintage, series: str) -> tuple[str, float, str]:
    rows = vintage.observations[series]
    period = rows[-1][0]
    if series == "ROUTPUT":
        if len(rows) < 2 or period_index(period) - period_index(rows[-2][0]) != 3:
            raise ValueError(f"missing previous GDP quarter in {vintage.origin}")
        return period, 100 * ((rows[-1][1] / rows[-2][1]) ** 4 - 1), "real GDP growth, annualized q/q, percent"
    if series == "RUC":
        return period, rows[-1][1], "unemployment rate, percent"
    if series == "CPI":
        index = {period_index(p): v for p, v in rows}
        year_ago = index.get(period_index(period) - 12)
        if year_ago is None:
            raise ValueError(f"missing year-ago CPI in {vintage.origin}")
        return period, 100 * (rows[-1][1] / year_ago - 1), "CPI inflation, 12-month, percent"
    raise ValueError(f"unsupported series {series}")


def expanding_ar1(pairs: list[tuple[float, float]]) -> tuple[float, float]:
    """OLS y(next vintage) = intercept + slope*x(current vintage)."""
    if len(pairs) < MIN_AR_TRAIN:
        raise ValueError("insufficient past released pairs")
    mean_x = statistics.fmean(x for x, _ in pairs)
    mean_y = statistics.fmean(y for _, y in pairs)
    variance = sum((x - mean_x) ** 2 for x, _ in pairs)
    if variance < 1e-12:
        return mean_y, 0.0
    slope = sum((x - mean_x) * (y - mean_y) for x, y in pairs) / variance
    return mean_y - slope * mean_x, slope


def conformal_radius(previous_abs_errors: list[float]) -> float | None:
    """Conformal-style empirical radius; no formal coverage guarantee is claimed."""
    n = len(previous_abs_errors)
    if n < MIN_INTERVAL_RESIDUALS:
        return None
    order = math.ceil((n + 1) * INTERVAL_COVERAGE)
    if order > n:
        return None
    return sorted(previous_abs_errors)[order - 1]


def forecast_origin(vintage: Vintage, next_origin: str, history: dict[str, list[dict]]) -> list[Forecast]:
    """Forecast without access to the next vintage or any future outcomes."""
    output = []
    for series in SERIES:
        feature_period, feature, definition = indicator(vintage, series)
        past = history[series]
        if any(row["target_as_of"] > vintage.as_of for row in past):
            raise ValueError(f"training target not yet released at {vintage.origin}")
        pairs = [(row["feature"], row["actual"]) for row in past]
        candidates = [("persistence", feature, len(pairs))]
        if len(pairs) >= MIN_AR_TRAIN:
            intercept, slope = expanding_ar1(pairs)
            candidates.append(("expanding_ar1", intercept + slope * feature, len(pairs)))
        for model, prediction, n_pairs in candidates:
            errors = [abs(row["errors"][model]) for row in past if model in row["errors"]]
            radius = conformal_radius(errors)
            output.append(Forecast(
                vintage.origin, next_origin, series, definition, feature_period, feature,
                model, prediction, n_pairs,
                prediction - radius if radius is not None else None,
                prediction + radius if radius is not None else None,
                len(errors),
            ))
    return output


def evaluate_forecasts(forecasts: list[Forecast], target: Vintage) -> tuple[list[dict], dict[str, dict]]:
    if any(f.target_vintage != target.origin for f in forecasts):
        raise ValueError("forecast-target vintage mismatch")
    actuals = {series: indicator(target, series) for series in SERIES}
    records, histories = [], {}
    for forecast in forecasts:
        target_period, actual, _ = actuals[forecast.series]
        horizon_months = period_index(target_period) - period_index(forecast.feature_period)
        # CPI publication timing varies in these historical quarterly snapshots.
        # Never relabel a 2- or 4-month target as a 3-month forecast.
        allowed = (3,) if forecast.series != "CPI" else (2, 3, 4)
        if horizon_months not in allowed:
            raise ValueError(f"unexpected {horizon_months}-month horizon: {forecast.series} {forecast.origin}")
        error = forecast.prediction - actual
        records.append({
            "origin": forecast.origin,
            "origin_as_of": "",  # filled by run_backtest, keeping the forecast independent
            "target_vintage": target.origin,
            "target_as_of": target.as_of.isoformat(),
            "series": forecast.series,
            "indicator": forecast.indicator,
            "feature_period": forecast.feature_period,
            "target_period": target_period,
            "horizon_observation_months": horizon_months,
            "model": forecast.model,
            "feature": forecast.feature,
            "prediction": forecast.prediction,
            "actual_next_vintage": actual,
            "error": error,
            "absolute_error": abs(error),
            "training_pairs": forecast.training_pairs,
            "interval_lower_80": forecast.interval_lower,
            "interval_upper_80": forecast.interval_upper,
            "interval_prior_residuals": forecast.interval_residuals,
            "interval_hit_80": "" if forecast.interval_lower is None else int(forecast.interval_lower <= actual <= forecast.interval_upper),
        })
        history = histories.setdefault(forecast.series, {
            "feature": forecast.feature,
            "actual": actual,
            "target_as_of": target.as_of,
            "errors": {},
        })
        history["errors"][forecast.model] = error
    return records, histories


def origin_schedule() -> list[str]:
    return [f"{year}Q{quarter}" for year in range(2006, 2024) for quarter in range(1, 5)
            if (year, quarter) >= (2006, 4)]


def run_backtest(root: Path = VINTAGE_ROOT) -> tuple[list[dict], str]:
    schedule = origin_schedule()
    if len(schedule) != 69 or schedule[0] != "2006Q4" or schedule[-1] != "2023Q4":
        raise AssertionError("unexpected origin schedule")
    history: dict[str, list[dict]] = {series: [] for series in SERIES}
    records = []
    input_hashes = []
    for index, origin in enumerate(schedule[:-1]):
        current = load_vintage(root, origin)
        next_origin = schedule[index + 1]
        if quarter_index(next_origin) != quarter_index(origin) + 1:
            raise AssertionError("nonconsecutive origins")
        # Critical order: construct all forecasts before opening the target.
        pending = forecast_origin(current, next_origin, history)
        target = load_vintage(root, next_origin)
        if target.as_of <= current.as_of:
            raise ValueError("target was not released later")
        rows, additions = evaluate_forecasts(pending, target)
        for row in rows:
            row["origin_as_of"] = current.as_of.isoformat()
        records.extend(rows)
        for series, item in additions.items():
            history[series].append(item)
        input_hashes.append((origin, current.manifest_hash))
    input_hashes.append((schedule[-1], target.manifest_hash))
    fingerprint = hashlib.sha256(json.dumps(input_hashes, separators=(",", ":")).encode()).hexdigest()
    return records, fingerprint


def metrics(records: list[dict]) -> dict:
    if not records:
        return {"n": 0}
    errors = [row["error"] for row in records]
    intervals = [row for row in records if row["interval_hit_80"] != ""]
    return {
        "n": len(records),
        "mae": statistics.fmean(abs(error) for error in errors),
        "rmse": math.sqrt(statistics.fmean(error * error for error in errors)),
        "bias": statistics.fmean(errors),
        "interval_n": len(intervals),
        "interval_coverage": statistics.fmean(row["interval_hit_80"] for row in intervals) if intervals else None,
        "interval_mean_width": statistics.fmean(row["interval_upper_80"] - row["interval_lower_80"] for row in intervals) if intervals else None,
    }


def percentile(sorted_values: list[float], quantile: float) -> float:
    position = (len(sorted_values) - 1) * quantile
    lo, hi = math.floor(position), math.ceil(position)
    return sorted_values[lo] + (sorted_values[hi] - sorted_values[lo]) * (position - lo)


def block_bootstrap_mae_difference(paired: list[tuple[dict, dict]], seed: int) -> tuple[float, float]:
    """Approximate 95% CI for AR1 MAE minus persistence MAE; quarter blocks."""
    differences = [ar["absolute_error"] - persistence["absolute_error"] for persistence, ar in paired]
    n = len(differences)
    rng = random.Random(seed)
    sampled_means = []
    for _ in range(BOOTSTRAP_REPLICATIONS):
        sample = []
        while len(sample) < n:
            start = rng.randrange(n)
            sample.extend(differences[(start + offset) % n] for offset in range(BOOTSTRAP_BLOCK_QUARTERS))
        sampled_means.append(statistics.fmean(sample[:n]))
    sampled_means.sort()
    return percentile(sampled_means, 0.025), percentile(sampled_means, 0.975)


def summarize(records: list[dict], fingerprint: str) -> dict:
    output = {
        "schema_version": "owg.philadelphia-backtest.v1",
        "input_manifest_fingerprint_sha256": fingerprint,
        "target_definition": "next quarterly vintage's then-latest observation; GDP and unemployment advance 3 months, CPI can advance 2-4 months",
        "models": ["persistence", "expanding_ar1 (minimum 12 released training pairs)"],
        "metrics": {},
        "episodes": {},
        "bootstrap": {"replications": BOOTSTRAP_REPLICATIONS, "block_quarters": BOOTSTRAP_BLOCK_QUARTERS, "seed": BOOTSTRAP_SEED},
    }
    episodes = {
        "financial_crisis_2007_2009": (2007, 2009),
        "pandemic_2020": (2020, 2020),
        "inflation_2021_2023": (2021, 2023),
    }
    for series in SERIES:
        series_rows = [row for row in records if row["series"] == series]
        by_origin = {}
        for row in series_rows:
            by_origin.setdefault(row["origin"], {})[row["model"]] = row
        paired = [(items["persistence"], items["expanding_ar1"]) for items in by_origin.values() if "expanding_ar1" in items]
        persistence_common = [p for p, _ in paired]
        ar_common = [ar for _, ar in paired]
        output["metrics"][series] = {
            "persistence_all": metrics([r for r in series_rows if r["model"] == "persistence"]),
            "persistence_ar1_common_window": metrics(persistence_common),
            "expanding_ar1_common_window": metrics(ar_common),
            "ar1_minus_persistence_mae": metrics(ar_common)["mae"] - metrics(persistence_common)["mae"],
            "ar1_minus_persistence_mae_block_bootstrap_95": block_bootstrap_mae_difference(paired, BOOTSTRAP_SEED + SERIES.index(series)),
            "target_horizon_months_count": {
                str(months): sum(row["horizon_observation_months"] == months for row in series_rows if row["model"] == "persistence")
                for months in sorted({row["horizon_observation_months"] for row in series_rows})
            },
        }
        output["episodes"][series] = {}
        for name, (start, end) in episodes.items():
            selected = [r for r in series_rows if start <= int(r["target_vintage"][:4]) <= end]
            output["episodes"][series][name] = {
                model: metrics([r for r in selected if r["model"] == model])
                for model in ("persistence", "expanding_ar1")
            }
    return output


def write_artifacts(records: list[dict], summary: dict, output_root: Path) -> None:
    output_root.mkdir(parents=True, exist_ok=True)
    csv_path = output_root / "backtest_philadelphia_2007_2023.csv"
    json_path = output_root / "backtest_philadelphia_2007_2023.json"
    md_path = output_root / "backtest_philadelphia_2007_2023.md"
    with csv_path.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(records[0]))
        writer.writeheader()
        writer.writerows(records)
    json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    lines = [
        "---", "title: Philadelphia Fed rolling-origin benchmark, 2007-2023",
        "status: observed", "tags: [OpenWorldGen, economics, backtest, data-vintage]", "---", "",
        "# Philadelphia Fed rolling-origin benchmark", "",
        "This is an **observed benchmark**, not a validated economy simulator or a 10-year forecast.",
        "The [Philadelphia Fed real-time archive](https://www.philadelphiafed.org/surveys-and-data/real-time-data-research/real-time-data-set-full-time-series-history) gives historical values as known in each vintage. Each forecast is made from one source snapshot before the next snapshot is opened by the script.",
        "The evaluation labels are values recorded in the next quarterly vintage, not today\u2019s revised final values or necessarily the indicator\u2019s first public release. The vintage date uses the extraction manifest\u2019s conservative mid-quarter day cutoff; exact intraday release timing was not verified.", "",
        "## Target and methods", "",
        "- 68 consecutive vintage-to-vintage forecasts: origins 2006 Q4\u20132023 Q3, targets 2007 Q1\u20132023 Q4. GDP and unemployment advance three observation months. CPI release timing is irregular: its latest available observation sometimes advances two or four months, so CPI scores mix those explicitly reported horizons.",
        "- GDP: annualized quarter-over-quarter real growth (`100 * ((GDP_t / GDP_t-1)^4 - 1)`), percent. Unemployment: latest monthly rate, percent. CPI: latest monthly year-over-year inflation (`100 * (CPI_t / CPI_t-12 - 1)`), percent. Each ratio uses its **own** vintage, avoiding level comparisons across rebasing/revisions.",
        "- Persistence repeats the current-vintage indicator. Expanding AR(1) fits `next = intercept + slope * current` to **previously released** vintage pairs; it starts only after 12 such pairs. The common-window comparison excludes earlier persistence-only quarters.",
        "- Nominal 80% intervals use previous absolute errors and the conformal-style `ceil((n+1)*0.8)` order statistic, starting after 12 prior errors per model/series. They are calibration diagnostics, **not** formal 80% guarantees: serial dependence, changing model fits, crises, and regime shifts violate the simple assumptions. A 95% circular 4-quarter block bootstrap interval describes uncertainty in the *MAE difference*, not parameter or future-world uncertainty.",
        "- Primary score is MAE in the indicator\u2019s percentage-point units. RMSE, bias, interval coverage/width, and episode scores are diagnostics.", "",
        "## Results", "",
        "| Indicator | Persistence MAE, all 68 | Persistence MAE, AR common window | AR(1) MAE, common window | AR minus persistence MAE | 95% block-bootstrap interval |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for series in SERIES:
        report = summary["metrics"][series]
        p_all = report["persistence_all"]["mae"]
        p = report["persistence_ar1_common_window"]["mae"]
        ar = report["expanding_ar1_common_window"]["mae"]
        diff = report["ar1_minus_persistence_mae"]
        lo, hi = report["ar1_minus_persistence_mae_block_bootstrap_95"]
        lines.append(f"| {series} | {p_all:.3f} | {p:.3f} | {ar:.3f} | {diff:+.3f} | [{lo:+.3f}, {hi:+.3f}] |")
    lines += ["", "Negative AR minus persistence means AR improved MAE; positive means it worsened it. The comparison is descriptive because method design occurred after the historical period. No LLM forecast appears here; a 2026-trained LLM cannot be made blind to 2007\u201323 by timestamped prompting.", "", "Horizon counts (observation months from feature to label): " + "; ".join(f"{series}: {summary['metrics'][series]['target_horizon_months_count']}" for series in SERIES) + ". CPI horizon mixing is a comparability limitation even though both models face the same targets.", "", "## Interval diagnostics", "", "| Indicator | Model | Scored intervals | Empirical coverage | Mean width, percentage points |", "| --- | --- | ---: | ---: | ---: |"]
    for series in SERIES:
        report = summary["metrics"][series]
        for label, key in (("Persistence", "persistence_all"), ("AR(1)", "expanding_ar1_common_window")):
            value = report[key]
            coverage = "n/a" if value["interval_coverage"] is None else f"{value['interval_coverage']:.1%}"
            width = "n/a" if value["interval_mean_width"] is None else f"{value['interval_mean_width']:.3f}"
            lines.append(f"| {series} | {label} | {value['interval_n']} | {coverage} | {width} |")
    lines += ["", "## Episode check: persistence MAE", "", "| Indicator | 2007\u201309 | 2020 | 2021\u201323 |", "| --- | ---: | ---: | ---: |"]
    for series in SERIES:
        episodes = summary["episodes"][series]
        values = [episodes[name]["persistence"]["mae"] for name in ("financial_crisis_2007_2009", "pandemic_2020", "inflation_2021_2023")]
        lines.append(f"| {series} | {values[0]:.3f} | {values[1]:.3f} | {values[2]:.3f} |")
    lines += [
        "", "## Reproduce and audit", "",
        "```sh", "python3 research/us-economy-2007-2023/code/backtest_philadelphia.py",
        "python3 -m unittest discover -s research/us-economy-2007-2023/code -p 'test_backtest_philadelphia.py'", "```", "",
        f"Input manifest fingerprint SHA-256: `{summary['input_manifest_fingerprint_sha256']}`. Every source CSV hash is checked against its own vintage manifest before scoring.",
        "The row-level CSV records feature period, target period, observation-month horizon, as-of times, point forecast, next-vintage actual, errors, training count, and intervals. The JSON holds exact scores and bootstrap settings.", "",
        "## Boundaries", "",
        "This checks only three aggregate indicators, not households, firms, credit, trade, policy, or strategic agents. It does not test whether any simulator explains mechanisms or predicts counterfactuals. Historical source vintages were archived in 2026; their vintage columns are trusted as point-in-time releases but could still have archival corrections. Source metadata for CPI/GDP can shift base or definition; within-vintage ratio transforms mitigate but do not erase comparability risk. The interval coverage is empirical only, with no finite-sample guarantee here. A 10-year forecast needs a separate, prospective or genuinely unseen-origin evaluation and scenario uncertainty, not an extrapolation of this one-quarter benchmark.", "",
    ]
    md_path.write_text("\n".join(lines), encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vintages", type=Path, default=VINTAGE_ROOT)
    parser.add_argument("--output", type=Path, default=ARTIFACT_ROOT)
    args = parser.parse_args()
    records, fingerprint = run_backtest(args.vintages)
    summary = summarize(records, fingerprint)
    write_artifacts(records, summary, args.output)
    print(json.dumps({"rows": len(records), "fingerprint": fingerprint, "metrics": summary["metrics"]}, indent=2))


if __name__ == "__main__":
    main()
