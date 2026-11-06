"""Collect and verify dated BLS CPS archives from ALFRED, one series per request."""

import argparse
import csv
import hashlib
import io
import json
import math
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timezone
from pathlib import Path
from urllib.error import URLError
from urllib.parse import urlencode
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1] / "data/raw/cps-labor-alfred"
FLOWS = (
    ("LNS17000000", "LNS17400000", "LNS17800000"),
    ("LNS17100000", "LNS17500000", "LNS17900000"),
    ("LNS17200000", "LNS17600000", "LNS18000000"),
)
SERIES = tuple(code for row in FLOWS for code in row) + ("UNRATE", "CIVPART")
# Employment Situation January releases. The last one is reserved for scoring.
VINTAGES = {
    "2016-01-08": "2015-12-01",
    "2018-01-05": "2017-12-01",
    "2020-01-10": "2019-12-01",
    "2021-01-08": "2020-12-01",
    "2022-01-07": "2021-12-01",
    "2023-01-06": "2022-12-01",
    "2024-02-02": "2023-12-01",
}
START = "2000-01-01"
BASE = "https://alfred.stlouisfed.org/graph/alfredgraph.csv"


def source_url(series, vintage, end):
    return BASE + "?" + urlencode(
        {"id": series, "cosd": START, "coed": end, "vintage_date": vintage}
    )


def parse_series(payload, series, vintage, end):
    reader = csv.reader(io.StringIO(payload.decode("utf-8-sig")), strict=True)
    expected = ["observation_date", f"{series}_{vintage.replace('-', '')}"]
    if next(reader, None) != expected:
        raise ValueError(f"{series} at {vintage}: unexpected vintage header")
    data = {}
    for period, value in reader:
        parsed = date.fromisoformat(period)
        if parsed.day != 1 or not START <= period <= end or period in data:
            raise ValueError(f"{series} at {vintage}: invalid period {period}")
        number = float(value)
        if not math.isfinite(number) or number < 0 or (series in ("UNRATE", "CIVPART") and number > 100):
            raise ValueError(f"{series} at {vintage}: invalid value at {period}")
        data[period] = number
    start_month = int(START[:4]) * 12
    end_month = int(end[:4]) * 12 + int(end[5:7]) - 1
    expected_months = [f"{m // 12:04d}-{m % 12 + 1:02d}-01" for m in range(start_month, end_month + 1)]
    if list(data) != expected_months:
        raise ValueError(f"{series} at {vintage}: incomplete or unsorted coverage")
    return data


def fetch(task):
    vintage, end, series = task
    url = source_url(series, vintage, end)
    for attempt in range(3):
        try:
            with urlopen(url, timeout=25) as response:
                payload = response.read(128 * 1024 + 1)
            break
        except (URLError, TimeoutError):
            if attempt == 2:
                raise
            time.sleep(1 + attempt)
    if len(payload) > 128 * 1024:
        raise ValueError(f"oversized response for {series}")
    parse_series(payload, series, vintage, end)
    return vintage, series, payload


def collect(root):
    tasks = [(vintage, end, series) for vintage, end in VINTAGES.items() for series in SERIES]
    with ThreadPoolExecutor(max_workers=10) as pool:
        results = list(pool.map(fetch, tasks))
    for vintage, end in VINTAGES.items():
        target = root / vintage
        target.mkdir(parents=True, exist_ok=True)
        manifest = {
            "archive": "ALFRED / U.S. Bureau of Labor Statistics",
            "as_of": vintage,
            "last_observation": end,
            "retrieved_at": datetime.now(timezone.utc).isoformat(),
            "sources": {},
        }
        for archived, series, payload in results:
            if archived != vintage:
                continue
            path = target / f"{series}.csv"
            path.write_bytes(payload)
            manifest["sources"][series] = {
                "url": source_url(series, vintage, end),
                "sha256": hashlib.sha256(payload).hexdigest(),
            }
        (target / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"{vintage}  {len(manifest['sources'])} series through {end}", flush=True)


def load_vintage(root, vintage):
    target = root / vintage
    manifest_bytes = (target / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    end = VINTAGES[vintage]
    if (
        manifest["as_of"] != vintage
        or manifest["last_observation"] != end
        or set(manifest["sources"]) != set(SERIES)
    ):
        raise ValueError(f"invalid vintage manifest {vintage}")
    observations = {}
    for series, source in manifest["sources"].items():
        payload = (target / f"{series}.csv").read_bytes()
        if hashlib.sha256(payload).hexdigest() != source["sha256"] or source["url"] != source_url(series, vintage, end):
            raise ValueError(f"source evidence mismatch for {series} at {vintage}")
        observations[series] = parse_series(payload, series, vintage, end)
    return hashlib.sha256(manifest_bytes).hexdigest(), observations


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT)
    collect(parser.parse_args().output)
