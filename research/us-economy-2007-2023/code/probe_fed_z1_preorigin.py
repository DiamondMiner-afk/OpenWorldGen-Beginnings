"""Bounded December 2006 Z.1 household-deposit probe with vintage caveats."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from zipfile import ZipFile

from bridge_fed_z1_household_deposits import (
    RAW as MARCH_RAW,
    SERIES,
    parse_prn,
    pdf_text,
    require_pdf_rounding,
)
from probe_fed_z1_adjacent_deposits import printed_anchor


ROOT = Path(__file__).resolve().parents[1]
RAW = ROOT / "data/raw/fed-z1-preorigin"
OUTPUT = ROOT / "artifacts/fed_z1_december_2006_household_deposits_probe.json"
ZIP_NAME = "2006-12-07_ltabs.zip"
PDF_NAME = "2006-12-07_z1.pdf"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inspect(raw: Path = RAW) -> tuple[dict, dict]:
    manifest = json.loads((raw / "source-provenance.json").read_text(encoding="utf-8"))
    if manifest.get("schema_version") != "owg.fed-z1-preorigin-source.v1":
        raise ValueError("unexpected December Z.1 source manifest")
    if manifest.get("release_date") != "2006-12-07" or manifest.get("reported_through") != "2006:Q3":
        raise ValueError("unexpected December Z.1 release identity")
    entries = manifest.get("files", [])
    if len(entries) != 2 or {item.get("filename") for item in entries} != {ZIP_NAME, PDF_NAME}:
        raise ValueError("December Z.1 source pair missing or duplicated")
    for item in entries:
        path = raw / item["filename"]
        if path.stat().st_size != item["bytes"] or sha256(path) != item["sha256"]:
            raise ValueError(f"source size/hash mismatch: {path.name}")
        if not item["source_url"].startswith("https://www.federalreserve.gov/releases/z1/20061207/"):
            raise ValueError(f"source URL mismatch: {path.name}")
        if item["http_last_modified_utc"] <= "2007-01-01T00:00:00Z":
            raise ValueError(f"unexpected pre-origin byte evidence declaration: {path.name}")
    if (raw / PDF_NAME).read_bytes()[:5] != b"%PDF-":
        raise ValueError("December Z.1 PDF signature mismatch")
    with ZipFile(raw / ZIP_NAME) as archive:
        if archive.testzip() is not None or "ltab100d.prn" not in archive.namelist():
            raise ValueError("December Z.1 ZIP member or CRC mismatch")
        table = parse_prn(archive.read("ltab100d.prn").decode("ascii"))
    if max(table) != "2006:Q3" or next(iter(table)) != "1952:Q1":
        raise ValueError("December Z.1 table quarter bounds mismatch")
    return manifest, table


def build_report(raw: Path = RAW, march_raw: Path = MARCH_RAW) -> dict:
    manifest, december = inspect(raw)
    with ZipFile(march_raw / "2007-03-08_ltabs.zip") as archive:
        march = parse_prn(archive.read("ltab100d.prn").decode("ascii"))
    if "2006:Q3" not in march:
        raise ValueError("March comparison quarter missing")
    text = pdf_text(raw / PDF_NAME)
    for marker in ("December 7, 2006", "Third Quarter 2006"):
        if marker not in text[:5000]:
            raise ValueError(f"December PDF cover marker missing: {marker}")
    series = []
    for code, spec in SERIES.items():
        value = december["2006:Q3"][code]
        later = march["2006:Q3"][code]
        if value is None or later is None or value < 0 or later < 0:
            raise ValueError(f"missing or negative selected level: {code}")
        printed = printed_anchor(text, era="legacy", code=code)
        require_pdf_rounding(value, printed)
        series.append({
            "code": code,
            "label": spec["label"],
            "december_file_2006_q3_millions": value,
            "march_file_same_2006_q3_millions": later,
            "march_minus_december_surviving_file_difference_millions": later - value,
            "december_pdf_printed_billions": str(printed),
        })
    december_combined = sum(item["december_file_2006_q3_millions"] for item in series)
    march_combined = sum(item["march_file_same_2006_q3_millions"] for item in series)
    return {
        "schema_version": "owg.fed-z1-preorigin-deposit-probe.v1",
        "source_manifest_sha256": sha256(raw / "source-provenance.json"),
        "december_zip_sha256": manifest["files"][0]["sha256"],
        "december_pdf_sha256": manifest["files"][1]["sha256"],
        "march_zip_sha256": sha256(march_raw / "2007-03-08_ltabs.zip"),
        "release_date": "2006-12-07",
        "observation_quarter": "2006:Q3",
        "unit": "millions of current US dollars, end-of-quarter stock, not seasonally adjusted",
        "series": series,
        "selected_two_series_combined_millions": {
            "december_surviving_file": december_combined,
            "march_surviving_file_same_quarter": march_combined,
            "march_minus_december": march_combined - december_combined,
        },
        "actor_input_status_for_2007_01_01": "blocked_exact_bytes_not_certified",
        "interpretation_warning": "Same-quarter surviving-file differences are not economic flows and do not establish when or why values changed. Release publication does not authenticate the surviving PDF/ZIP bytes at the January 2007 cutoff.",
    }


if __name__ == "__main__":
    OUTPUT.write_text(json.dumps(build_report(), indent=2) + "\n", encoding="utf-8")
