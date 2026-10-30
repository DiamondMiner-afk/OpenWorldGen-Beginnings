"""Check adjacent dated Z.1 releases for two named deposit level series.

This probe measures *same-quarter differences between surviving files labelled
as dated releases*, not economic flows. A historical row in a later release is
never an input available to an earlier actor. The surviving June 2007 bytes have
post-release HTTP modification dates, so they cannot establish a June 2007
information set.
"""

from __future__ import annotations

import hashlib
import json
import re
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
from zipfile import ZipFile

from bridge_fed_z1_household_deposits import (
    RAW,
    SERIES,
    parse_csv,
    parse_prn,
    pdf_text,
    require_pdf_rounding,
    validate_dictionary,
)
from probe_fed_z1_archives import inspect as inspect_original_archives


ROOT = Path(__file__).resolve().parents[1]
ADJACENT_RAW = ROOT / "data/raw/fed-z1-adjacent"
ARTIFACT = ROOT / "artifacts/fed_z1_adjacent_household_deposits_probe.json"
EXPECTED_FILES = {
    "2007-06-07_ltabs.zip",
    "2007-06-07_z1.pdf",
    "2023-06-08_z1_csv_files.zip",
    "2023-06-08_z1.pdf",
}
RELEASES = (
    ("2007-03-08", RAW, "2007-03-08_ltabs.zip", "ltab100d.prn", "2007-03-08_z1.pdf", "2006:Q4", "legacy"),
    ("2007-06-07", ADJACENT_RAW, "2007-06-07_ltabs.zip", "ltab100d.prn", "2007-06-07_z1.pdf", "2007:Q1", "legacy"),
    ("2023-03-09", RAW, "2023-03-09_z1_csv_files.zip", "csv/l101.csv", "2023-03-09_z1.pdf", "2022:Q4", "modern"),
    ("2023-06-08", ADJACENT_RAW, "2023-06-08_z1_csv_files.zip", "csv/l101.csv", "2023-06-08_z1.pdf", "2023:Q1", "modern"),
)
ENDPOINT_COVERS = {
    "2007-06-07": ("June 7, 2007", "First Quarter 2007"),
    "2023-06-08": ("June 8, 2023", "First Quarter 2023"),
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def utc(value: str) -> datetime:
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.utcoffset() != timezone.utc.utcoffset(parsed):
        raise ValueError("timestamp must carry UTC offset")
    return parsed


def source_passes_not_before_screen(entry: dict, request_time_utc: str) -> bool:
    """Reject earlier times; passing does *not* authenticate historical access."""
    requested = utc(request_time_utc)
    release_midnight = utc(f"{entry['release_date']}T00:00:00Z")
    cutoff = utc(entry["conservative_available_not_before_utc"])
    return requested >= max(release_midnight, cutoff)


def release_passes_not_before_screen(files: list[dict], release_date: str, request_time_utc: str) -> bool:
    """Check PDF and ZIP lower bounds, never grant actor-input eligibility."""
    selected = [item for item in files if item["release_date"] == release_date]
    if len(selected) != 2 or {Path(item["filename"]).suffix for item in selected} != {".zip", ".pdf"}:
        raise ValueError(f"release lacks one source pair: {release_date}")
    return all(source_passes_not_before_screen(item, request_time_utc) for item in selected)


def inspect_adjacent(raw: Path = ADJACENT_RAW) -> dict:
    manifest = json.loads((raw / "source-provenance.json").read_text(encoding="utf-8"))
    if manifest.get("schema_version") != "owg.fed-z1-adjacent-source.v1":
        raise ValueError("unexpected adjacent Z.1 manifest version")
    files = manifest.get("files", [])
    if {item.get("filename") for item in files} != EXPECTED_FILES or len(files) != 4:
        raise ValueError("missing or duplicate adjacent Z.1 source")
    inspected = []
    for item in files:
        name = item["filename"]
        path = raw / name
        release_date = item["release_date"]
        if not name.startswith(release_date + "_"):
            raise ValueError(f"release/filename mismatch: {name}")
        if not item["source_url"].startswith(
            f"https://www.federalreserve.gov/releases/z1/{release_date.replace('-', '')}/"
        ):
            raise ValueError(f"source URL does not match dated Fed release: {name}")
        if path.stat().st_size != item["bytes"] or sha256(path) != item["sha256"]:
            raise ValueError(f"source size/hash mismatch: {name}")
        if utc(item["conservative_available_not_before_utc"]) <= utc(item["http_last_modified_utc"]):
            raise ValueError(f"actor cutoff precedes HTTP modification: {name}")
        inspected_item = dict(item)
        if path.suffix == ".zip":
            with ZipFile(path) as archive:
                names = archive.namelist()
                if len(names) != len(set(names)) or not names:
                    raise ValueError(f"duplicate/empty ZIP members: {name}")
                if any(member.startswith("/") or ".." in Path(member).parts for member in names):
                    raise ValueError(f"unsafe ZIP member: {name}")
                if archive.testzip() is not None:
                    raise ValueError(f"ZIP CRC mismatch: {name}")
                inspected_item["member_count"] = len(names)
        else:
            with path.open("rb") as stream:
                if stream.read(5) != b"%PDF-":
                    raise ValueError(f"invalid PDF signature: {name}")
        inspected.append(inspected_item)
    return {"manifest_sha256": sha256(raw / "source-provenance.json"), "files": inspected}


def printed_anchor(text: str, *, era: str, code: str, quarters_before_latest: int = 0) -> Decimal:
    """Read a printed latest/prior-quarter PDF level in billions."""
    spec = SERIES[code]
    if era == "legacy":
        heading = "L.100 Households and Nonprofit Organizations (1)"
        next_heading = "L.101 Nonfinancial Business (1)"
        line_no = spec["old_pdf_line"]
        pattern = rf"^\s*{line_no}\s+{re.escape(spec['label'])}\s+"
    elif era == "modern":
        heading = "L.101 Households and Nonprofit Organizations (1)"
        next_heading = "L.102 Nonfinancial Business (1)"
        line_no = spec["new_pdf_line"]
        pattern = rf"^\s*{line_no}\s+{re.escape(code[:-2])}\s+{re.escape(spec['label'])}\s+"
    else:
        raise ValueError(f"unsupported PDF table era: {era}")
    start = text.find(heading)
    end = text.find(next_heading, start + len(heading))
    if start < 0 or end < 0:
        raise ValueError(f"PDF table heading absent: {era}")
    section = text[start:end]
    if "Billions of dollars; amounts outstanding end of period, not seasonally adjusted" not in section:
        raise ValueError("PDF level/stock unit definition absent")
    lines = [line for line in section.splitlines() if re.search(pattern, line)]
    if len(lines) != 1:
        raise ValueError(f"PDF series line not unique: {era}/{code}")
    numbers = re.findall(r"(?<!\S)-?\d+\.\d+(?!\S)", lines[0])
    if quarters_before_latest < 0 or quarters_before_latest >= len(numbers):
        raise ValueError(f"PDF series values absent: {era}/{code}")
    return Decimal(numbers[-(quarters_before_latest + 1)])


def read_release_table(raw: Path, zip_name: str, member: str, era: str) -> dict:
    with ZipFile(raw / zip_name) as archive:
        if era == "legacy":
            return parse_prn(archive.read(member).decode("ascii"))
        if era == "modern":
            table = parse_csv(archive.read(member).decode("utf-8-sig"))
            validate_dictionary(archive.read("data_dictionary/l101.txt").decode("utf-8-sig"))
            return table
    raise ValueError(f"unsupported table era: {era}")


def build_report(original_raw: Path = RAW, adjacent_raw: Path = ADJACENT_RAW) -> dict:
    original_probe = inspect_original_archives(original_raw)
    adjacent_probe = inspect_adjacent(adjacent_raw)
    original = {item["filename"]: item for item in original_probe["files"]}
    adjacent = {item["filename"]: item for item in adjacent_probe["files"]}
    tables = {}
    pdf_anchors = {}
    prior_pdf_anchors = {}
    release_evidence = {}
    for release, default_raw, zip_name, member, pdf_name, latest, era in RELEASES:
        raw = original_raw if default_raw == RAW else adjacent_raw
        table = read_release_table(raw, zip_name, member, era)
        if max(table) != latest or next(iter(table)) != ("1952:Q1" if era == "legacy" else "1945:Q4"):
            raise ValueError(f"unexpected latest/first quarter in {release}")
        text = pdf_text(raw / pdf_name)
        if release in ENDPOINT_COVERS and not all(marker in text[:5000] for marker in ENDPOINT_COVERS[release]):
            raise ValueError(f"PDF cover/release mismatch: {release}")
        pdf_anchors[release] = {}
        prior_pdf_anchors[release] = {}
        for code in SERIES:
            anchor = printed_anchor(text, era=era, code=code)
            require_pdf_rounding(table[latest][code], anchor)
            pdf_anchors[release][code] = str(anchor)
            if release in ("2007-06-07", "2023-06-08"):
                prior_quarter = "2006:Q4" if era == "legacy" else "2022:Q4"
                prior_anchor = printed_anchor(text, era=era, code=code, quarters_before_latest=1)
                require_pdf_rounding(table[prior_quarter][code], prior_anchor)
                prior_pdf_anchors[release][code] = str(prior_anchor)
        tables[release] = table
        metadata = original if default_raw == RAW else adjacent
        release_evidence[release] = {
            "latest_quarter": latest,
            "rows": len(table),
            "table_zip": zip_name,
            "table_member": member,
            "table_zip_sha256": metadata[zip_name]["sha256"],
            "pdf": pdf_name,
            "pdf_sha256": metadata[pdf_name]["sha256"],
            "conservative_not_before_screen_utc": max(
                metadata[zip_name]["conservative_available_not_before_utc"],
                metadata[pdf_name]["conservative_available_not_before_utc"],
            ),
        }

    series = []
    for code, spec in SERIES.items():
        values = {
            "2006_q4_march_2007": tables["2007-03-08"]["2006:Q4"][code],
            "2006_q4_june_2007": tables["2007-06-07"]["2006:Q4"][code],
            "2007_q1_june_2007": tables["2007-06-07"]["2007:Q1"][code],
            "2022_q4_march_2023": tables["2023-03-09"]["2022:Q4"][code],
            "2022_q4_june_2023": tables["2023-06-08"]["2022:Q4"][code],
            "2023_q1_june_2023": tables["2023-06-08"]["2023:Q1"][code],
        }
        if any(value is None or value < 0 for value in values.values()):
            raise ValueError(f"missing/negative selected deposit level: {code}")
        series.append({
            "code": code,
            "label": spec["label"],
            "values_millions": values,
            "same_quarter_difference_june_minus_march_2007_millions": values["2006_q4_june_2007"] - values["2006_q4_march_2007"],
            "same_quarter_difference_june_minus_march_2023_millions": values["2022_q4_june_2023"] - values["2022_q4_march_2023"],
            "latest_quarter_pdf_anchor_billions": {release: pdf_anchors[release][code] for release in pdf_anchors},
            "june_prior_quarter_pdf_anchor_billions": {
                release: prior_pdf_anchors[release][code]
                for release in ("2007-06-07", "2023-06-08")
            },
        })
    return {
        "schema_version": "owg.fed-z1-adjacent-deposits-probe.v1",
        "status": "bounded same-code/label/unit continuity and surviving-file difference probe; not calibrated",
        "grain": "release vintage x observation quarter x named household/nonprofit deposit instrument",
        "unit": "millions of dollars, end-of-quarter stock, not seasonally adjusted",
        "original_source_manifest_sha256": sha256(original_raw / "provenance.json"),
        "adjacent_source_manifest_sha256": adjacent_probe["manifest_sha256"],
        "releases": release_evidence,
        "series": series,
        "point_in_time_rule": (
            "Never substitute a later release's revised historical observation at an earlier actor time. "
            "The current June 2007 ZIP has HTTP Last-Modified 2008-01-29 and its PDF "
            "2007-09-17; these exact bytes fail the June 2007 actor-input timing "
            "screen. Passing a later not-before screen still does not authenticate "
            "that bytes were public on that later day."
        ),
        "comparability_limit": (
            "Matching codes and printed labels within these adjacent pairs do not prove "
            "invariant sector coverage or decompose differences. The 2007 files' "
            "post-release HTTP dates also prevent attribution to release-day revisions. "
            "Deposits and currency are "
            "household/nonprofit assets, not all bank deposit liabilities."
        ),
    }


def main() -> None:
    report = build_report()
    ARTIFACT.parent.mkdir(parents=True, exist_ok=True)
    ARTIFACT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"artifact": str(ARTIFACT), "releases": len(report["releases"]), "series": len(report["series"])}))


if __name__ == "__main__":
    main()
