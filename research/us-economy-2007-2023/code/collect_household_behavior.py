"""Archive BEA income, spending and Social Security vintages for household behavior."""

import argparse
import csv
import hashlib
import io
import json
import statistics
from concurrent.futures import ThreadPoolExecutor
from datetime import date
from pathlib import Path
from urllib.parse import urlencode
from urllib.request import urlopen

from collect_labor_vintages import BASE, VINTAGES

RESEARCH = Path(__file__).resolve().parents[1]
ARCHIVE = RESEARCH / "data/raw/household-bea-alfred"
OUTPUT = RESEARCH / "data/processed/household-behavior"
SCORING = "2024-02-02"
CODES = ("PCE", "DSPI", "PCEPI", "W823RC1", "A576RC1")


def sha(payload):
    return hashlib.sha256(payload).hexdigest()


def source(origin, code):
    start = f"{2015 if origin == SCORING else int(origin[:4]) - 6}-01-01"
    end = VINTAGES[origin]
    return start, end, BASE + "?" + urlencode({"id": code, "cosd": start, "coed": end, "vintage_date": origin})


def parse(payload, origin, code):
    start, end, _ = source(origin, code)
    reader = csv.reader(io.StringIO(payload.decode("utf-8-sig")), strict=True)
    if next(reader, None) != ["observation_date", f"{code}_{origin.replace('-', '')}"]:
        raise ValueError(f"incorrect BEA vintage: {code} {origin}")
    values = {}
    expected = int(start[:4]) * 12
    for period, amount in reader:
        day = date.fromisoformat(period)
        value = float(amount)
        if not (day.day == 1 and day.year * 12 + day.month - 1 == expected
                and start <= period <= end and 0 < value < float("inf")):
            raise ValueError(f"invalid BEA observation: {code} {period}")
        values[period[:7]] = value
        expected += 1
    if not values or expected < int(end[:4]) * 12 + 11:
        raise ValueError(f"incomplete BEA coverage: {code} {origin}")
    return values


def fetch(origin, code, archive):
    path = archive / origin / f"{code}.csv"
    if path.exists():
        return
    with urlopen(source(origin, code)[2], timeout=30) as response:
        payload = response.read(32769)
    if len(payload) > 32768:
        raise ValueError("BEA response exceeds size limit")
    parse(payload, origin, code)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)


def fit(rows):
    trailing = rows[-12:]
    propensity = statistics.median(pce / dpi for pce, dpi, *_ in trailing)
    ss_share = statistics.median(ss / wages for _, _, _, ss, wages in trailing)
    real = [(pce / index, dpi / index) for pce, dpi, index, *_ in rows[-60:]]
    changes = [(current[0] - prior[0], current[1] - prior[1]) for prior, current in zip(real, real[1:])]
    # Very large income shocks in a small window do not identify individual MPC.
    # Preserve the raw fit, use bounded values for this scenario rule.
    mean_c, mean_y = (statistics.mean(column) for column in zip(*changes))
    variance = sum((income - mean_y) ** 2 for _, income in changes)
    raw = sum((income - mean_y) * (spend - mean_c) for spend, income in changes) / variance if variance else 0.0
    return {
        "average_spending_share": propensity,
        "income_response": min(1.0, max(0.0, raw)),
        "raw_income_response": raw,
        "social_security_per_wage": ss_share,
    }


def transform(origin, archive):
    data, sources = {}, {}
    for code in CODES:
        payload = (archive / origin / f"{code}.csv").read_bytes()
        data[code] = parse(payload, origin, code)
        sources[code] = {"url": source(origin, code)[2], "sha256": sha(payload)}
    periods = sorted(set.intersection(*(set(values) for values in data.values())))
    rows = [tuple(data[code][period] for code in CODES) for period in periods]
    if len(rows) < 60:
        raise ValueError("household behavior fit requires 60 common months")
    result = {"as_of": origin, "sources": sources, "latest_month": periods[-1]}
    if origin == SCORING:
        result["spending_share"] = {period: row[0] / row[1] for period, row in zip(periods, rows, strict=True)}
    else:
        result.update(fit(rows), fit_months=periods[-60:])
    return result


def load(origin, archive=ARCHIVE, output=OUTPUT):
    payload = (output / f"{origin}.json").read_bytes()
    frozen = json.loads(payload)
    if frozen != transform(origin, archive):
        raise ValueError(f"BEA household source hash mismatch at {origin}")
    return frozen, sha(payload)


def collect(archive, output):
    output.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=8) as pool:
        for origin in VINTAGES:
            tasks = [pool.submit(fetch, origin, code, archive) for code in CODES]
            for task in tasks:
                task.result()
            result = transform(origin, archive)
            (output / f"{origin}.json").write_text(json.dumps(result, indent=2) + "\n")
            print(origin, result["latest_month"], result.get("average_spending_share"), result.get("income_response"),
                  result.get("social_security_per_wage"), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, default=ARCHIVE)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args()
    collect(args.archive, args.output)
