"""Fit an inventory-gap response from origin-dated Census M3 releases."""

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
RAW = RESEARCH / "data/raw/m3-alfred"
OUTPUT = RESEARCH / "data/processed/inventory-adjustment"
SCORING = "2024-02-02"
SERIES = ("AMTMFI", "AMTMVS", "A11SFI", "A11SVS")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def url(origin, code):
    start = "2016-01-01" if origin == SCORING else f"{int(origin[:4]) - 6}-01-01"
    return BASE + "?" + urlencode({"id": code, "cosd": start, "coed": VINTAGES[origin], "vintage_date": origin})


def parse(raw, origin, code):
    table = csv.reader(io.StringIO(raw.decode("utf-8-sig")), strict=True)
    if next(table, None) != ["observation_date", f"{code}_{origin.replace('-', '')}"]:
        raise ValueError(f"unexpected M3 vintage of {code} at {origin}")
    latest = VINTAGES[origin]
    first_year = 2016 if origin == SCORING else int(origin[:4]) - 6
    period_index = first_year * 12
    values = {}
    for period, value in table:
        day = date.fromisoformat(period)
        amount = int(value)
        if day.day != 1 or day.year * 12 + day.month - 1 != period_index or period > latest or amount <= 0:
            raise ValueError(f"bad M3 coverage or value at {code} {period}")
        period_index += 1
        values[period[:7]] = amount
    if not values or period_index < (int(latest[:4]) * 12 + 10):
        raise ValueError(f"M3 vintage too short at {origin} {code}")
    return values


def fit(points):
    recent = points[-60:]
    target = statistics.median(stock / shipments for stock, shipments in recent)
    gaps = [target * shipments - stock for stock, shipments in recent[:-1]]
    changes = [current[0] - prior[0] for prior, current in zip(recent, recent[1:])]
    denominator = sum(gap * gap for gap in gaps)
    raw = sum(gap * change for gap, change in zip(gaps, changes)) / denominator if denominator else 0.0
    return {
        "opening_months": recent[-1][0] / recent[-1][1],
        "target_months": target,
        "adjustment": min(1.0, max(0.0, raw)),
        "unconstrained_adjustment": raw,
    }


def transform(origin, archive):
    histories, sources = {}, {}
    for code in SERIES:
        path = archive / origin / f"{code}.csv"
        raw = path.read_bytes()
        histories[code] = parse(raw, origin, code)
        sources[code] = {"url": url(origin, code), "sha256": sha(raw)}
    periods = sorted(set.intersection(*(set(history) for history in histories.values())))
    food = []
    manufactured = []
    for period in periods:
        total_stock, total_shipments, food_stock, food_shipments = (histories[code][period] for code in SERIES)
        if not 0 < food_stock < total_stock or not 0 < food_shipments < total_shipments:
            raise ValueError(f"M3 food is not nested within manufacturing at {period}")
        food.append((food_stock, food_shipments))
        manufactured.append((total_stock - food_stock, total_shipments - food_shipments))
    result = {"as_of": origin, "latest_observation": periods[-1], "sources": sources}
    if origin == SCORING:
        result["ratios"] = {period: [a[0] / a[1], b[0] / b[1]]
                            for period, a, b in zip(periods, food, manufactured, strict=True)}
    else:
        if len(periods) < 60:
            raise ValueError("M3 fit requires 60 common months")
        result.update(fit_window=periods[-60:], food=fit(food), manufacturing=fit(manufactured))
    return result


def load(origin, archive=RAW, output=OUTPUT):
    path = output / f"{origin}.json"
    frozen = path.read_bytes()
    if transform(origin, archive) != json.loads(frozen):
        raise ValueError(f"frozen M3 inventory manifest mismatch at {origin}")
    return json.loads(frozen), sha(frozen)


def download(origin, code, archive):
    path = archive / origin / f"{code}.csv"
    if not path.exists():
        with urlopen(url(origin, code), timeout=30) as response:
            raw = response.read(32_769)
        if len(raw) > 32_768:
            raise ValueError("M3 source response exceeds the size limit")
        parse(raw, origin, code)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)


def collect(archive, output):
    output.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=8) as pool:
        for origin in VINTAGES:
            jobs = [pool.submit(download, origin, code, archive) for code in SERIES]
            for job in jobs:
                job.result()
            report = transform(origin, archive)
            (output / f"{origin}.json").write_text(json.dumps(report, indent=2) + "\n")
            print(origin, report["latest_observation"], report.get("food"), report.get("manufacturing"), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, default=RAW)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args()
    collect(args.archive, args.output)
