"""Extract one point-in-time US macro snapshot from Philadelphia Fed RTDSM files.

The source workbooks are an archival matrix: rows are observations and columns
are vintages. Never give a historical policy the entire workbook.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
from datetime import date, datetime, timezone
from pathlib import Path

from openpyxl import load_workbook


ROOT = Path(__file__).resolve().parents[1]
RAW = ROOT / "data/raw/philadelphia-fed"
SPECS = {
    "ROUTPUT": ("ROUTPUTQvQd.xlsx", "ROUTPUT", "billions of chained real USD, SAAR"),
    "RUC": ("rucQvMd.xlsx", "ruc", "percent, seasonally adjusted"),
    "CPI": ("cpiQvMd.xlsx", "cpi", "index, seasonally adjusted; vintage-specific base"),
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def period_end(period: str) -> date:
    quarter = re.fullmatch(r"(\d{4}):Q([1-4])", period)
    if quarter:
        year, q = int(quarter[1]), int(quarter[2])
        month = q * 3
    else:
        monthly = re.fullmatch(r"(\d{4}):(0[1-9]|1[0-2])", period)
        if not monthly:
            raise ValueError(f"unexpected observation period: {period}")
        year, month = int(monthly[1]), int(monthly[2])
    next_year, next_month = (year + 1, 1) if month == 12 else (year, month + 1)
    return date.fromordinal(date(next_year, next_month, 1).toordinal() - 1)


def extract_one(source: str, origin: str, output: Path, as_of: date) -> dict:
    filename, sheet_name, units = SPECS[source]
    input_path = RAW / filename
    workbook = load_workbook(input_path, read_only=True, data_only=True)
    sheet = workbook[sheet_name]
    rows = sheet.values
    header = next(rows)
    column_name = source + origin[2:]
    if header.count(column_name) != 1:
        raise ValueError(f"expected one {column_name} column, found {header.count(column_name)}")
    column = header.index(column_name)
    result = []
    missing = 0
    future = 0
    for row in rows:
        period, value = row[0], row[column]
        if period_end(period) > as_of:
            future += 1
            if value is not None and value != "#N/A":
                raise ValueError(f"lookahead observation in {column_name}: {period}")
            continue
        if value is None or value == "#N/A":
            missing += 1
            continue
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise ValueError(f"non-numeric {source} value at {period}: {value!r}")
        result.append((period, value))
    if not result or len({period for period, _ in result}) != len(result):
        raise ValueError(f"empty or duplicate observations in {column_name}")
    ends = [period_end(period) for period, _ in result]
    if ends != sorted(ends):
        raise ValueError(f"unsorted observations in {column_name}")
    if source in ("CPI", "ROUTPUT") and any(value <= 0 for _, value in result):
        raise ValueError(f"invalid nonpositive {source} observation")
    if source == "RUC" and any(not 0 <= value <= 100 for _, value in result):
        raise ValueError("invalid unemployment percentage")
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow(("observation_period", "value"))
        writer.writerows(result)
    return {
        "id": source.lower() + "-" + origin.lower(),
        "publisher": "Federal Reserve Bank of Philadelphia",
        "source_url": f"https://www.philadelphiafed.org/surveys-and-data/real-time-data-research/{source.lower()}",
        "release_id": column_name,
        "published_at": as_of.isoformat() + "T23:59:59Z",
        "retrieved_at": "2026-10-17T15:10:45Z",
        "observation_start": ends[0].isoformat(),
        "observation_end": ends[-1].isoformat(),
        "relative_path": output.name,
        "sha256": sha256(output),
        "licence": "Public Philadelphia Fed research download; redistribution terms not yet verified",
        "geography": "US",
        "units": {"value": units},
        "classifications": {"series": source, "frequency": "quarterly" if source == "ROUTPUT" else "monthly"},
        "transformations": [
            f"Selected only {column_name} from {filename}; dropped #N/A and empty cells",
            f"Archived source workbook SHA-256: {sha256(input_path)}",
        ],
        "limitations": [
            "Published_at is a conservative same-day cutoff for the mid-quarter vintage, not the exact release timestamp",
            "Historical revisions and definition changes remain as they stood in this vintage",
        ],
    }, {"series": source, "rows": len(result), "missing_within_cutoff": missing, "future_empty_rows_excluded": future, "first": result[0], "last": result[-1]}


def extract_origin(origin: str) -> dict:
    match = re.fullmatch(r"(20\d{2})Q([1-4])", origin)
    if not match:
        raise ValueError("origin must be YYYYQ1..YYYYQ4, year 2000+")
    year, quarter = int(match[1]), int(match[2])
    as_of = date(year, (quarter - 1) * 3 + 2, 15)
    outdir = ROOT / "data/processed/philadelphia-fed" / origin
    sources, checks = [], []
    for source in SPECS:
        snapshot, check = extract_one(source, origin, outdir / f"{source.lower()}.csv", as_of)
        sources.append(snapshot)
        checks.append(check)
    manifest = {
        "schema_version": "owg.data-vintage.v1",
        "id": "us-" + origin.lower() + "-philly-macro",
        "as_of": as_of.isoformat() + "T23:59:59Z",
        "geography": "US",
        "currency": "USD",
        "sources": sources,
    }
    (outdir / "data-vintage.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (outdir / "quality.json").write_text(json.dumps(checks, indent=2) + "\n", encoding="utf-8")
    return {"origin": origin, "manifest": str(outdir / "data-vintage.json"), "checks": checks}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--origin", default="2006Q4", help="vintage YYYYQ1..YYYYQ4")
    parser.add_argument("--all", action="store_true", help="extract every 2007Q1–2023Q4 vintage")
    args = parser.parse_args()
    if args.all:
        for year in range(2007, 2024):
            for quarter in range(1, 5):
                result = extract_origin(f"{year}Q{quarter}")
                print(f"{result['origin']}: " + ", ".join(f"{check['series']}={check['rows']}" for check in result["checks"]), flush=True)
    else:
        print(json.dumps(extract_origin(args.origin), indent=2))


if __name__ == "__main__":
    main()
