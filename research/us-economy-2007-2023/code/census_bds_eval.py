"""Validate and stage the 2023 Census BDS release for retrospective evaluation.

This is a *latest-revised* annual dataset published in 2025. The 2007-2023
records must not be supplied as information available to simulated agents in
those historical years. No imputation is performed for disclosure/quality flags.

Run: python3 research/us-economy-2007-2023/code/census_bds_eval.py
Only Python's standard library is required.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
from collections import Counter, defaultdict
from decimal import Decimal, InvalidOperation
from pathlib import Path


RESEARCH_ROOT = Path(__file__).resolve().parents[1]
RAW = RESEARCH_ROOT / "data/raw/census-bds"
PROCESSED = RESEARCH_ROOT / "data/processed/census-bds/evaluation-only"
ARTIFACTS = RESEARCH_ROOT / "artifacts"
START_YEAR, END_YEAR = 2007, 2023
ALL_YEARS = set(range(1978, 2024))
SECTORS = (
    "11", "21", "22", "23", "31-33", "42", "44-45", "48-49", "51", "52", "53",
    "54", "55", "56", "61", "62", "71", "72", "81",
)
FLAGS = {"D", "S", "X", "N"}
COUNTS = (
    "firms", "estabs", "emp", "denom", "estabs_entry", "estabs_exit", "job_creation",
    "job_creation_births", "job_creation_continuers", "job_destruction",
    "job_destruction_deaths", "job_destruction_continuers", "net_job_creation",
    "firmdeath_firms", "firmdeath_estabs", "firmdeath_emp",
)
RATES = (
    "estabs_entry_rate", "estabs_exit_rate", "job_creation_rate_births",
    "job_creation_rate", "job_destruction_rate_deaths", "job_destruction_rate",
    "net_job_creation_rate", "reallocation_rate",
)
MEASURES = COUNTS + RATES
ECONOMY_HEADER = ("year",) + tuple(
    column for column in (
        "firms", "estabs", "emp", "denom", "estabs_entry", "estabs_entry_rate",
        "estabs_exit", "estabs_exit_rate", "job_creation", "job_creation_births",
        "job_creation_continuers", "job_creation_rate_births", "job_creation_rate",
        "job_destruction", "job_destruction_deaths", "job_destruction_continuers",
        "job_destruction_rate_deaths", "job_destruction_rate", "net_job_creation",
        "net_job_creation_rate", "reallocation_rate", "firmdeath_firms",
        "firmdeath_estabs", "firmdeath_emp",
    )
)
SECTOR_HEADER = ("year", "sector") + ECONOMY_HEADER[1:]
NONNEGATIVE = set(MEASURES) - {"net_job_creation", "net_job_creation_rate"}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_raw_sources(raw_root: Path) -> dict:
    provenance = json.loads((raw_root / "provenance.json").read_text(encoding="utf-8"))
    if provenance.get("schema_version") != "owg.raw-source.v1" or provenance.get("release_date") != "2025-09-25":
        raise ValueError("unexpected BDS provenance contract or release")
    if {entry["filename"] for entry in provenance["files"]} != {"bds2023.csv", "bds2023_sec.csv"}:
        raise ValueError("missing/duplicate BDS raw source")
    for entry in provenance["files"]:
        filename = entry["filename"]
        path = raw_root / filename
        if path.stat().st_size != entry["size_bytes"] or sha256(path) != entry["sha256"]:
            raise ValueError(f"raw BDS size/hash mismatch: {filename}")
        expected_url = "https://www2.census.gov/programs-surveys/bds/tables/time-series/2023/" + filename
        if entry["source_url"] != expected_url:
            raise ValueError(f"unexpected BDS source URL: {filename}")
    return provenance


def parse_measure(value: str, column: str, key: str) -> tuple[Decimal | None, str | None]:
    """Represent flags as missing-with-reason, never numeric zero."""
    if value in FLAGS:
        return None, value
    if value == "":
        return None, "blank"
    try:
        number = Decimal(value)
    except InvalidOperation as exc:
        raise ValueError(f"unrecognized {column} token {value!r} at {key}") from exc
    if not number.is_finite():
        raise ValueError(f"nonfinite {column} at {key}")
    if column in COUNTS and number != number.to_integral_value():
        raise ValueError(f"noninteger count {column} at {key}")
    if column in NONNEGATIVE and number < 0:
        raise ValueError(f"negative nonnegative measure {column} at {key}")
    return number, None


def parse_file(path: Path, sector_table: bool) -> tuple[list[dict[str, str]], dict]:
    expected = SECTOR_HEADER if sector_table else ECONOMY_HEADER
    rows: list[dict[str, str]] = []
    flags = Counter()
    flag_fields = Counter()
    flags_by_year = Counter()
    keys = set()
    years = set()
    per_year = Counter()
    with path.open(newline="", encoding="utf-8-sig") as stream:
        reader = csv.DictReader(stream)
        if tuple(reader.fieldnames or ()) != expected:
            raise ValueError(f"BDS schema drift in {path.name}: {reader.fieldnames!r}")
        for row in reader:
            if None in row or any(value is None for value in row.values()):
                raise ValueError(f"malformed row in {path.name}")
            if len(row["year"]) != 4 or not row["year"].isdigit():
                raise ValueError(f"invalid BDS year in {path.name}: {row['year']!r}")
            year = int(row["year"])
            if year not in ALL_YEARS:
                raise ValueError(f"out-of-release year {year} in {path.name}")
            sector = row.get("sector")
            if sector_table and sector not in SECTORS:
                raise ValueError(f"unknown BDS sector {sector!r} in {path.name}")
            key = f"{year}/{sector}" if sector_table else str(year)
            if key in keys:
                raise ValueError(f"duplicate BDS key {key}")
            keys.add(key)
            years.add(year)
            per_year[year] += 1
            for column in MEASURES:
                _, flag = parse_measure(row[column], column, key)
                if flag:
                    flags[flag] += 1
                    flag_fields[f"{column}:{flag}"] += 1
                    flags_by_year[f"{year}:{flag}"] += 1
            rows.append(row)
    if years != ALL_YEARS:
        raise ValueError(f"BDS annual coverage mismatch in {path.name}: missing {sorted(ALL_YEARS - years)}")
    expected_per_year = len(SECTORS) if sector_table else 1
    if any(per_year[year] != expected_per_year for year in ALL_YEARS):
        raise ValueError(f"BDS row grain/sector coverage mismatch in {path.name}")
    return rows, {
        "raw_rows": len(rows),
        "raw_year_range": [min(years), max(years)],
        "raw_rows_per_year": expected_per_year,
        "selected_rows": sum(START_YEAR <= int(row["year"]) <= END_YEAR for row in rows),
        "flags_all_years": dict(sorted(flags.items())),
        "flags_by_field_all_years": dict(sorted(flag_fields.items())),
        "flags_by_year_all_years": dict(sorted(flags_by_year.items())),
    }


def numeric(row: dict[str, str], field: str) -> Decimal | None:
    return parse_measure(row[field], field, row["year"])[0]


def check_identities(rows: list[dict[str, str]]) -> dict:
    """Describe exact count identities and rounded-rate residuals, skipping flags."""
    equations = {
        "net_jobs_equals_creation_minus_destruction": ("net_job_creation", "job_creation", "job_destruction", "difference", Decimal(0)),
        "creation_equals_births_plus_continuers": ("job_creation", "job_creation_births", "job_creation_continuers", "sum", Decimal(0)),
        "destruction_equals_deaths_plus_continuers": ("job_destruction", "job_destruction_deaths", "job_destruction_continuers", "sum", Decimal(0)),
        "net_rate_equals_creation_rate_minus_destruction_rate": ("net_job_creation_rate", "job_creation_rate", "job_destruction_rate", "difference", Decimal("0.002")),
    }
    result = {}
    for label, (result_field, left_field, right_field, operation, tolerance) in equations.items():
        tested, skipped, failures, max_abs = 0, 0, [], Decimal(0)
        for row in rows:
            values = [numeric(row, field) for field in (result_field, left_field, right_field)]
            if any(value is None for value in values):
                skipped += 1
                continue
            answer, left, right = values
            residual = answer - (left - right if operation == "difference" else left + right)
            tested += 1
            max_abs = max(max_abs, abs(residual))
            if abs(residual) > tolerance:
                failures.append({"year": int(row["year"]), "sector": row.get("sector"), "residual": str(residual)})
        result[label] = {
            "tested": tested,
            "skipped_flag_or_blank": skipped,
            "failures": len(failures),
            "maximum_absolute_residual": str(max_abs),
            "tolerance": str(tolerance),
            "failure_examples": failures[:5],
        }
    return result


def cross_table_checks(economy: list[dict[str, str]], sectors: list[dict[str, str]]) -> dict:
    """Diagnostic only: public-use noise/flags may impede exact additivity."""
    by_year = {int(row["year"]): row for row in economy}
    sector_by_year = defaultdict(list)
    for row in sectors:
        sector_by_year[int(row["year"])].append(row)
    fields = ("estabs", "emp", "job_creation", "job_destruction", "net_job_creation")
    report = {}
    for field in fields:
        comparable = []
        skipped = []
        for year in range(START_YEAR, END_YEAR + 1):
            national = numeric(by_year[year], field)
            parts = [numeric(row, field) for row in sector_by_year[year]]
            if national is None or any(part is None for part in parts):
                skipped.append(year)
                continue
            comparable.append({"year": year, "sector_sum_minus_economy": int(sum(parts) - national)})
        report[field] = {
            "comparable_years": len(comparable),
            "skipped_years_with_flag_or_blank": skipped,
            "nonzero_differences": sum(row["sector_sum_minus_economy"] != 0 for row in comparable),
            "maximum_absolute_difference": max((abs(row["sector_sum_minus_economy"]) for row in comparable), default=None),
            "differences_by_year": comparable,
        }
    return report


def write_filtered(path: Path, rows: list[dict[str, str]], sector_table: bool) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    header = SECTOR_HEADER if sector_table else ECONOMY_HEADER
    with path.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=header, lineterminator="\n")
        writer.writeheader()
        writer.writerows(row for row in rows if START_YEAR <= int(row["year"]) <= END_YEAR)


def build(raw_root: Path = RAW, processed_root: Path = PROCESSED, artifact_root: Path = ARTIFACTS) -> dict:
    provenance = verify_raw_sources(raw_root)
    economy, economy_qc = parse_file(raw_root / "bds2023.csv", sector_table=False)
    sectors, sector_qc = parse_file(raw_root / "bds2023_sec.csv", sector_table=True)
    selected_economy = [row for row in economy if START_YEAR <= int(row["year"]) <= END_YEAR]
    selected_sectors = [row for row in sectors if START_YEAR <= int(row["year"]) <= END_YEAR]
    economy_identity = check_identities(selected_economy)
    sector_identity = check_identities(selected_sectors)
    output_economy = processed_root / "bds2023_economy_2007_2023.csv"
    output_sectors = processed_root / "bds2023_sector_2007_2023.csv"
    write_filtered(output_economy, selected_economy, sector_table=False)
    write_filtered(output_sectors, selected_sectors, sector_table=True)
    audit = {
        "schema_version": "owg.census-bds-evaluation-audit.v1",
        "data_role": "latest-revised retrospective evaluation only; unavailable at historical simulation origins",
        "available_not_before": provenance["release_date"],
        "release_date": provenance["release_date"],
        "retrieved_at": provenance["retrieved_at"],
        "evaluation_years": [START_YEAR, END_YEAR],
        "raw_sources": provenance["files"],
        "economy": {**economy_qc, "identities_2007_2023": economy_identity},
        "sector": {
            **sector_qc,
            "sector_codes": list(SECTORS),
            "identities_2007_2023": sector_identity,
        },
        "cross_table_2007_2023": cross_table_checks(selected_economy, selected_sectors),
        "processed": {
            "economy": {"path": str(output_economy.relative_to(RESEARCH_ROOT)), "sha256": sha256(output_economy), "rows": len(selected_economy)},
            "sector": {"path": str(output_sectors.relative_to(RESEARCH_ROOT)), "sha256": sha256(output_sectors), "rows": len(selected_sectors)},
        },
    }
    (processed_root / "evaluation-only-manifest.json").write_text(json.dumps({
        "schema_version": "owg.evaluation-only-source.v1",
        "source_release_date": provenance["release_date"],
        "available_not_before": provenance["release_date"],
        "role": "retrospective_target_only",
        "prohibition": "Do not load into actor information sets or historical origin features before the source release date.",
        "files": audit["processed"],
    }, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    artifact_root.mkdir(parents=True, exist_ok=True)
    (artifact_root / "census_bds_2023_audit.json").write_text(json.dumps(audit, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return audit


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--raw", type=Path, default=RAW)
    parser.add_argument("--processed", type=Path, default=PROCESSED)
    parser.add_argument("--artifacts", type=Path, default=ARTIFACTS)
    args = parser.parse_args()
    audit = build(args.raw, args.processed, args.artifacts)
    print(json.dumps({
        "economy_rows": audit["processed"]["economy"]["rows"],
        "sector_rows": audit["processed"]["sector"]["rows"],
        "economy_flags": audit["economy"]["flags_all_years"],
        "sector_flags": audit["sector"]["flags_all_years"],
        "sector_identity_failures": {key: value["failures"] for key, value in audit["sector"]["identities_2007_2023"].items()},
    }, indent=2))


if __name__ == "__main__":
    main()
