"""Archive BEA expenditure baskets and BLS private payroll composition by vintage."""

import argparse
import csv
import hashlib
import io
import json
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import date
from decimal import Decimal
from pathlib import Path
from urllib.error import URLError
from urllib.parse import urlencode
from urllib.request import urlopen

from collect_labor_vintages import BASE, VINTAGES

RESEARCH = Path(__file__).resolve().parents[1]
ROOT = RESEARCH / "data/raw/production-alfred"
OUTPUT = RESEARCH / "data/processed/production-opening"
SCORING = "2024-02-02"
SECTORS = ("energy", "food", "manufacturing", "services")
PCE = ("PCEC", "DGDSRC1Q027SBEA", "PCESV", "DFXARC1Q027SBEA",
       "DGOERC1Q027SBEA", "DNRGRC1Q027SBEA")
CES = ("USPRIV", "MANEMP", "CES3231100001", "CES1021100001", "CES4422000001")
GDP = "GDPC1"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def spec(origin, code):
    end = VINTAGES[origin][:7] + "-31"
    if VINTAGES[origin][5:7] != "12":
        raise ValueError("unexpected origin layout")
    start = f"{2015 if origin == SCORING else int(origin[:4]) - 2}-01-01"
    return start, end, (3 if code in (*PCE, GDP) else 1)


def source_url(origin, code):
    start, end, _ = spec(origin, code)
    return BASE + "?" + urlencode({"id": code, "cosd": start, "coed": end, "vintage_date": origin})


def parse(raw, origin, code):
    start, end, stride = spec(origin, code)
    reader = csv.reader(io.StringIO(raw.decode("utf-8-sig")), strict=True)
    if next(reader, None) != ["observation_date", f"{code}_{origin.replace('-', '')}"]:
        raise ValueError(f"unexpected edition of {code} at {origin}")
    result = {}
    expected = int(start[:4]) * 12
    for period, raw in reader:
        day = date.fromisoformat(period)
        value = Decimal(raw)
        if not (start <= period <= end and day.day == 1 and value.is_finite() and value > 0
                and day.year * 12 + day.month - 1 == expected):
            raise ValueError(f"invalid observation for {code} at {period}")
        result[period[:7]] = value
        expected += stride
    end_expected = int(end[:4]) * 12 + 12
    if expected != end_expected and (stride != 3 or origin == SCORING or expected != end_expected - 3):
        raise ValueError(f"incomplete vintage for {code} at {origin}")
    return result


def acquire(origin, code, root):
    file = root / origin / f"{code}.csv"
    if file.exists():
        raw = file.read_bytes()
    else:
        for attempt in range(3):
            try:
                with urlopen(source_url(origin, code), timeout=25) as response:
                    raw = response.read(24_577)
                break
            except (URLError, TimeoutError):
                if attempt == 2:
                    raise
                time.sleep(attempt + 1)
        if len(raw) > 24_576:
            raise ValueError("oversized vintage download")
        parse(raw, origin, code)
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_bytes(raw)
    return code, raw, parse(raw, origin, code)


def shares(parts, total, label, tolerance=Decimal("0.01")):
    if min(parts) <= 0 or total <= 0 or abs(sum(parts) - total) > tolerance:
        raise ValueError(f"{label} does not reconcile")
    return [float(value / sum(parts)) for value in parts]


def pce_shares(values, period):
    total, goods, services, food, gasoline, energy = (values[code][period] for code in PCE)
    if energy < gasoline:
        raise ValueError("BEA energy components overlap incorrectly")
    rounding = sum(Decimal(10) ** value.as_tuple().exponent for value in (goods, services, total)) / 2
    return shares((energy, food, goods - food - gasoline, services - (energy - gasoline)),
                  total, "BEA", min(rounding, Decimal("0.15")))


def payroll_shares(values, period):
    private, manufacturing, food, oil, utilities = (values[code][period] for code in CES)
    energy = oil + utilities
    return shares((energy, food, manufacturing - food, private - manufacturing - energy), private, "CES")


def transform(origin, rows):
    values = {code: series for code, _, series in rows}
    sources = {code: {"url": source_url(origin, code), "sha256": digest(raw),
                      "last_observation": next(reversed(series))}
               for code, raw, series in rows}
    pce_periods = sorted(set.intersection(*(set(values[code]) for code in PCE)))
    ces_periods = sorted(set.intersection(*(set(values[code]) for code in CES)))
    result = {"as_of": origin, "sources": sources, "sector_order": SECTORS,
              "max_pce_additivity_residual_billions": str(max(
                  abs(values[PCE[1]][period] + values[PCE[2]][period] - values[PCE[0]][period])
                  for period in pce_periods
              ))}
    if origin == SCORING:
        result.update(
            pce={period: pce_shares(values, period) for period in pce_periods},
            payroll={period: payroll_shares(values, period) for period in ces_periods},
            real_gdp={period: float(value) for period, value in values[GDP].items()},
        )
    else:
        pce_window = pce_periods[-4:]
        basket = [pce_shares(values, period) for period in pce_window]
        result.update(
            pce_quarters=pce_window,
            opening_gdp_growth=float((list(values[GDP].values())[-1] / list(values[GDP].values())[-2]) ** 4 - 1),
            payroll_month=ces_periods[-1],
            consumption_shares=[sum(column) / len(basket) for column in zip(*basket)],
            payroll_shares=payroll_shares(values, ces_periods[-1]),
        )
    return result


def load_vintage(origin, root=ROOT, output=OUTPUT):
    path = output / f"{origin}.json"
    if not path.exists():
        raise ValueError("build the production archive with collect_production_opening.py")
    frozen = path.read_bytes()
    result = json.loads(frozen)
    codes = (*PCE, *CES, GDP)
    rows = []
    for code in codes:
        raw = (root / origin / f"{code}.csv").read_bytes()
        rows.append((code, raw, parse(raw, origin, code)))
    if json.dumps(transform(origin, rows), sort_keys=True) != json.dumps(result, sort_keys=True):
        raise ValueError(f"production manifest mismatch at {origin}")
    return result, digest(frozen)


def collect(root, output):
    output.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=8) as pool:
        for origin in VINTAGES:
            codes = (*PCE, *CES, GDP)
            jobs = [pool.submit(acquire, origin, code, root) for code in codes]
            result = transform(origin, [job.result() for job in jobs])
            (output / f"{origin}.json").write_text(json.dumps(result, indent=2) + "\n")
            print(origin, result.get("payroll_month", "scoring"), result.get("consumption_shares", ""), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args()
    collect(args.archive, args.output)
