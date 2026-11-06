"""Reconstruct opening bank cohorts from dated H.8 and Z.1 releases."""

import argparse
import csv
import hashlib
import io
import json
import shlex
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import date
from decimal import Decimal
from email.utils import parsedate_to_datetime
from pathlib import Path
from urllib.error import URLError
from urllib.parse import urlencode
from urllib.request import urlopen
from zipfile import ZipFile

from collect_labor_vintages import BASE

RESEARCH = Path(__file__).resolve().parents[1]
RAW = RESEARCH / "data/raw"
OUTPUT = RESEARCH / "data/processed/bank-opening"
ARCHIVE = RAW / "h8-alfred"
Z1 = RAW / "fed-z1-release-collection/2026-10-21"
# The 2015 and 2017 ZIPs have modification dates after their opening cutoffs.
RELEASES = {
    "2020-01-10": "20191212",
    "2021-01-08": "20201210",
    "2022-01-07": "20211209",
    "2023-01-06": "20221209",
}
FIELDS = {
    "deposits": "DPS", "cash_assets": "CAS", "consumer_loans": "CLS",
    "business_loans": "CIL", "assets": "TLA", "liabilities": "TLB",
}
COHORTS = {"large": "LC", "other_domestic": "SC"}
SCORING = "2024-02-02"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def request_details(origin, cohort, field):
    year = int(origin[:4]) - 1
    first = "2019-12-01" if origin == SCORING else f"{year}-07-01"
    code = f"{FIELDS[field]}{COHORTS[cohort]}BM027NBOG"
    url = BASE + "?" + urlencode({
        "id": code, "cosd": first, "coed": f"{year}-12-31", "vintage_date": origin,
    })
    return year, code, url


def parse_h8(raw, origin, year, code):
    reader = csv.reader(io.StringIO(raw.decode("utf-8-sig")), strict=True)
    if next(reader, None) != ["observation_date", f"{code}_{origin.replace('-', '')}"]:
        raise ValueError(f"unexpected H.8 vintage for {code} at {origin}")
    values = {}
    expected = 2019 * 12 + 11 if origin == SCORING else year * 12 + 6
    for period, value in reader:
        d = date.fromisoformat(period)
        number = Decimal(value)
        if d.year * 12 + d.month - 1 != expected or d.day != 1 or d.year > year or not number.is_finite() or number <= 0:
            raise ValueError(f"invalid H.8 data for {code} at {period}")
        values[period] = number
        expected += 1
    if expected not in (year * 12 + 11, year * 12 + 12) or (origin == SCORING and expected != year * 12 + 12):
        raise ValueError(f"H.8 coverage too short for {code} at {origin}")
    return values


def acquire(task):
    origin, cohort, field, archive = task
    year, code, url = request_details(origin, cohort, field)
    path = archive / origin / f"{code}.csv"
    if path.exists():
        raw = path.read_bytes()
        return cohort, field, url, code, raw, parse_h8(raw, origin, year, code)
    for attempt in range(3):
        try:
            with urlopen(url, timeout=25) as response:
                raw = response.read(16_385)
            break
        except (URLError, TimeoutError):
            if attempt == 2:
                raise
            time.sleep(1 + attempt)
    if len(raw) > 16_384:
        raise ValueError(f"oversized H.8 response for {code}")
    values = parse_h8(raw, origin, year, code)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    return cohort, field, url, code, raw, values


def z1_anchors(origin, release, root=Z1):
    collection = json.loads((root / "collection.json").read_text())
    edition = collection["releases"][release]
    cutoff = date.fromisoformat(origin)
    publication = date.fromisoformat(edition["publication_date_from_cover"])
    if publication >= cutoff or edition["reported_through"] != f"{cutoff.year - 1}:Q3":
        raise ValueError("Z.1 publication or period is inconsistent with the origin")
    combined = "z1_csv_files.zip" in edition["files"]
    name = "z1_csv_files.zip" if combined else "ltabs.zip"
    receipt = edition["files"][name]
    if not receipt.get("http_last_modified") or parsedate_to_datetime(receipt["http_last_modified"]).date() >= cutoff:
        raise ValueError(f"{name} postdates opening cutoff or has no modification date")
    payload = (root / release / name).read_bytes()
    if sha(payload) != receipt["sha256"]:
        raise ValueError("Z.1 cached bytes differ from release receipt")
    with ZipFile(io.BytesIO(payload)) as package:
        if combined:
            dictionary = package.read("data_dictionary/l111.txt").decode()
            rows = csv.DictReader(io.StringIO(package.read("csv/l111.csv").decode()))
        else:
            dictionary = ""
            records = iter(package.read("ltab111d.prn").decode().splitlines())
            header = shlex.split(next(records))
            rows = (dict(zip(["date", *header[1:]], shlex.split(line), strict=True)) for line in records)
    if combined:
        row = next(row for row in rows if row["date"] == edition["reported_through"])
    else:
        row = next(row for row in rows if row["date"] == f"{publication.year}03")
    keys = {
        "reserves": "FL763013005.Q",
        "checkable_deposits": "FL763127005.Q",
    }
    if "FL763130005.Q" in row:
        keys["time_savings"] = "FL763130005.Q"
    else:
        keys.update(time_savings_domestic="FL763131005.Q", time_savings_foreign="FL763135005.Q")
    values = {}
    for field, code in keys.items():
        if combined:
            labels = [line.lower() for line in dictionary.splitlines() if line.startswith(code + "\t")]
            phrase = "reserves" if field == "reserves" else "deposits"
            if len(labels) != 1 or phrase not in labels[0] or "millions of dollars; not seasonally adjusted" not in labels[0]:
                raise ValueError(f"Z.1 dictionary mismatch for {code}")
        values[field] = Decimal(row[code])
    return {
        "publication_date": publication.isoformat(),
        "observation_quarter": edition["reported_through"],
        "archive_sha256": receipt["sha256"],
        "url": receipt["source_url"],
        "classification": "dated publisher reconstruction, exact contemporaneous bytes not independently authenticated",
        "values_million_usd": {field: str(value) for field, value in values.items()},
    }, values["reserves"] / sum(value for field, value in values.items() if field != "reserves")


def reconstruct(origin, release, rows):
    sources, series = {}, {}
    for cohort, field, url, code, raw, values in rows:
        sources[code] = {"url": url, "sha256": sha(raw)}
        series[cohort, field] = values
    period = max(set.intersection(*(set(values) for values in series.values())))
    data = {cohort: {field: series[cohort, field][period] for field in FIELDS} for cohort in COHORTS}
    anchor, reserve_per_deposit = z1_anchors(origin, release)
    deposits = sum(values["deposits"] for values in data.values())
    total_cash = sum(values["cash_assets"] for values in data.values())
    reserves = reserve_per_deposit * deposits
    if not 0 < reserves <= total_cash:
        raise ValueError(f"Z.1 reserves exceed H.8 cash bridge at {origin}")
    banks = []
    for cohort, values in data.items():
        dep = values["deposits"]
        reserve = reserves * values["cash_assets"] / total_cash
        if not (values["assets"] > values["liabilities"] >= dep
                and values["assets"] >= reserve + values["consumer_loans"] + values["business_loans"]):
            raise ValueError(f"incompatible gross balance sheet at {origin} {cohort}")
        banks.append({
            "cohort": cohort,
            "deposit_share": float(dep / deposits),
            "reserve_per_deposit": float(reserve / dep),
            "household_loans_per_deposit": float(values["consumer_loans"] / dep),
            "firm_loans_per_deposit": float(values["business_loans"] / dep),
            "assets_per_deposit": float(values["assets"] / dep),
            "liabilities_per_deposit": float(values["liabilities"] / dep),
        })
    return {
        "as_of": origin,
        "h8_observation_month": period[:7],
        "h8_sources": sources,
        "h8_values_billion_usd": {cohort: {field: str(value) for field, value in values.items()} for cohort, values in data.items()},
        "z1": anchor,
        "reserve_bridge": "Z.1 reserve-to-deposit ratio applied to H.8 deposits, allocated by H.8 cash shares",
        "banks": banks,
    }


def load_bank_opening(origin, archive=ARCHIVE, output=OUTPUT):
    if origin not in RELEASES:
        return None
    path = output / f"{origin}.json"
    if not path.exists():
        raise ValueError("build banking sources with collect_bank_opening.py first")
    expected = json.loads(path.read_text())
    rows = []
    for cohort in COHORTS:
        for field in FIELDS:
            year, code, url = request_details(origin, cohort, field)
            raw = (archive / origin / f"{code}.csv").read_bytes()
            rows.append((cohort, field, url, code, raw, parse_h8(raw, origin, year, code)))
    if reconstruct(origin, RELEASES[origin], rows) != expected:
        raise ValueError(f"bank opening receipt does not match raw data at {origin}")
    return expected, sha(path.read_bytes())


def scoring_rows(rows):
    sources, series = {}, {}
    for cohort, field, url, code, raw, values in rows:
        sources[code] = {"url": url, "sha256": sha(raw)}
        series[cohort, field] = values
    periods = sorted(set.intersection(*(set(values) for values in series.values())))
    return {
        "as_of": SCORING,
        "sources": sources,
        "loan_to_deposit": {
            period[:7]: float(sum(series[cohort, field][period] for cohort in COHORTS
                                 for field in ("consumer_loans", "business_loans"))
                              / sum(series[cohort, "deposits"][period] for cohort in COHORTS))
            for period in periods
        },
    }


def load_bank_scoring(archive=ARCHIVE, output=OUTPUT):
    path = output / "scoring.json"
    expected = json.loads(path.read_text())
    rows = []
    for cohort in COHORTS:
        for field in ("deposits", "consumer_loans", "business_loans"):
            year, code, url = request_details(SCORING, cohort, field)
            raw = (archive / SCORING / f"{code}.csv").read_bytes()
            rows.append((cohort, field, url, code, raw, parse_h8(raw, SCORING, year, code)))
    if scoring_rows(rows) != expected:
        raise ValueError("bank scoring receipt does not match raw data")
    return expected["loan_to_deposit"], sha(path.read_bytes())


def main(archive, output):
    output.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=8) as pool:
        for origin, release in RELEASES.items():
            tasks = [(origin, cohort, field, archive) for cohort in COHORTS for field in FIELDS]
            report = reconstruct(origin, release, pool.map(acquire, tasks))
            (output / f"{origin}.json").write_text(json.dumps(report, indent=2) + "\n")
            print(f"{origin}: H.8 {report['h8_observation_month']}, Z.1 {report['z1']['observation_quarter']}", flush=True)
        scoring_tasks = [(SCORING, cohort, field, archive) for cohort in COHORTS
                         for field in ("deposits", "consumer_loans", "business_loans")]
        scored = scoring_rows(pool.map(acquire, scoring_tasks))
        (output / "scoring.json").write_text(json.dumps(scored, indent=2) + "\n")
        print(f"{SCORING}: {len(scored['loan_to_deposit'])} comparison months", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, default=ARCHIVE)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args()
    main(args.archive, args.output)
