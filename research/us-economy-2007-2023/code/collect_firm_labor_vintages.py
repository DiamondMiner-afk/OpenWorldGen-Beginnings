"""Archive origin-dated BLS vacancy and payroll evidence for firm calibration."""

import argparse
import csv
import hashlib
import io
import json
import statistics
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timezone
from pathlib import Path
from urllib.parse import urlencode
from urllib.request import urlopen

from collect_labor_vintages import BASE, ROOT, VINTAGES

SOURCE = ROOT.parent / "jolts-alfred"
AUDIT = SOURCE.parents[2] / "artifacts/jolts-firm-labor-vintages.json"
START = "2001-01-01"
CODES = ("JTSJOL", "JTSHIL", "JTSLDL", "PAYEMS")


def source_url(series, vintage, end):
    return BASE + "?" + urlencode(
        {"id": series, "cosd": START, "coed": end, "vintage_date": vintage}
    )


def parse_series(payload, series, vintage, end):
    if len(payload) > 128 * 1024:
        raise ValueError(f"oversized data for {series}")
    reader = csv.reader(io.StringIO(payload.decode("utf-8-sig")), strict=True)
    header = ["observation_date", f"{series}_{vintage.replace('-', '')}"]
    if next(reader, None) != header:
        raise ValueError(f"incorrect vintage in {series} at {vintage}")
    values = {}
    previous_index = 2001 * 12 - 1
    for period, raw in reader:
        parsed = date.fromisoformat(period)
        index = parsed.year * 12 + parsed.month - 1
        value = int(raw)
        if parsed.day != 1 or index != previous_index + 1 or value <= 0 or period > end:
            raise ValueError(f"invalid or nonconsecutive {series} observation {period}")
        values[period] = value
        previous_index = index
    if not values or int(end[:4]) * 12 + int(end[5:7]) - 1 - previous_index > 3:
        raise ValueError(f"stale or empty {series} vintage at {vintage}")
    return values



def fetch(task):
    vintage, end, series = task
    url = source_url(series, vintage, end)
    with urlopen(url, timeout=30) as response:
        payload = response.read(128 * 1024 + 1)
    return vintage, series, url, payload, parse_series(payload, series, vintage, end)


def load_vintage(vintage):
    end = VINTAGES[vintage]
    folder = SOURCE / vintage
    manifest_bytes = (folder / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    if manifest["as_of"] != vintage or set(manifest["sources"]) != set(CODES):
        raise ValueError(f"incorrect JOLTS manifest for {vintage}")
    observations = {}
    for series, source in manifest["sources"].items():
        payload = (folder / f"{series}.csv").read_bytes()
        if (hashlib.sha256(payload).hexdigest() != source["sha256"]
                or source_url(series, vintage, end) != source["url"]):
            raise ValueError(f"incorrect JOLTS evidence for {series} at {vintage}")
        values = parse_series(payload, series, vintage, end)
        if next(reversed(values)) != source["last_period"]:
            raise ValueError(f"incorrect JOLTS final period for {series} at {vintage}")
        observations[series] = values
    return hashlib.sha256(manifest_bytes).hexdigest(), observations


def collect(output, audit):
    tasks = [(vintage, end, series) for vintage, end in VINTAGES.items() for series in CODES]
    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(fetch, tasks))
    report = {
        "schema_version": "owg.firm-labor-vintages.v1",
        "classification": "candidate empirical constraints, not fitted in the current engine",
        "stock_and_flow_note": "JTSJOL is an end-of-month opening stock. JTSHIL and JTSLDL are monthly payroll flows. They are not interchangeable with CPS household transitions or each other.",
        "retrieved_at": datetime.now(timezone.utc).isoformat(),
        "units": "thousands, seasonally adjusted, total nonfarm",
        "origins": [],
    }
    for vintage in VINTAGES:
        target = output / vintage
        target.mkdir(parents=True, exist_ok=True)
        sources, observations = {}, {}
        for archived, series, url, payload, values in results:
            if archived != vintage:
                continue
            (target / f"{series}.csv").write_bytes(payload)
            sources[series] = {
                "url": url, "sha256": hashlib.sha256(payload).hexdigest(),
                "last_period": next(reversed(values)),
            }
            observations[series] = values
        periods = sorted(set.intersection(*(set(values) for values in observations.values())))
        recent = periods[-60:]
        latest = recent[-1]
        rate = lambda series, period: observations[series][period] / observations["PAYEMS"][period]
        origin = {
            "as_of": vintage,
            "last_common_period": latest,
            "sources": sources,
            "openings_per_payroll_job": rate("JTSJOL", latest),
            "monthly_hires_per_payroll_job": rate("JTSHIL", latest),
            "monthly_layoffs_per_payroll_job": rate("JTSLDL", latest),
            "median_openings_per_payroll_job_recent_60": statistics.median(rate("JTSJOL", p) for p in recent),
        }
        report["origins"].append(origin)
        (target / "manifest.json").write_text(json.dumps(origin, indent=2) + "\n")
        print(f"{vintage} through {latest}: {origin['openings_per_payroll_job']:.2%} opening stock", flush=True)
    audit.parent.mkdir(parents=True, exist_ok=True)
    audit.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=SOURCE)
    parser.add_argument("--audit", type=Path, default=AUDIT)
    args = parser.parse_args()
    collect(args.output, args.audit)
