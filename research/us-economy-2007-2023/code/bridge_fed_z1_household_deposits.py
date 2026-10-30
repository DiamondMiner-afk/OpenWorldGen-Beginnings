"""Audit two named household-deposit Z.1 series across dated release formats.

This is a *vintage comparison*, not a single revised panel. In particular,
the March 2023 historical observations must never be fed to a March 2007 actor.
Only the two source ZIP members and their corresponding printed PDF pages are
read after the existing archive integrity/CRC probe succeeds.
"""

from __future__ import annotations

import csv
import io
import json
import re
import shlex
import subprocess
from decimal import Decimal
from pathlib import Path
from zipfile import ZipFile

from probe_fed_z1_archives import RAW, inspect as inspect_archives


ROOT = Path(__file__).resolve().parents[1]
ARTIFACT = ROOT / "artifacts/fed_z1_household_deposits_bridge.json"
OLD_ZIP = "2007-03-08_ltabs.zip"
NEW_ZIP = "2023-03-09_z1_csv_files.zip"
OLD_MEMBER = "ltab100d.prn"
NEW_MEMBER = "csv/l101.csv"
NEW_DICTIONARY = "data_dictionary/l101.txt"
OVERLAP = "2006:Q4"
NEW_ENDPOINT = "2022:Q4"
OLD_START = "1952:Q1"
NEW_START = "1945:Q4"
SERIES = {
    "FL153020005.Q": {
        "label": "Checkable deposits and currency",
        "old_pdf_line": 4,
        "new_pdf_line": 3,
    },
    "FL153030005.Q": {
        "label": "Time and savings deposits",
        "old_pdf_line": 5,
        "new_pdf_line": 4,
    },
}
UNIT = "millions of dollars, end of quarter, not seasonally adjusted"


def normalize_old_date(value: str) -> str:
    if not re.fullmatch(r"\d{4}0[1-4]", value):
        raise ValueError(f"invalid old Z.1 quarterly date: {value!r}")
    return f"{value[:4]}:Q{value[-1]}"


def require_contiguous_quarters(quarters: list[str]) -> None:
    """Reject missing or reordered rows at this table's quarterly grain."""
    previous = None
    for quarter in quarters:
        year, part = quarter.split(":Q")
        ordinal = int(year) * 4 + int(part) - 1
        if previous is not None and ordinal != previous + 1:
            raise ValueError(f"missing or unordered Z.1 quarter before {quarter}")
        previous = ordinal


def parse_prn(content: str) -> dict[str, dict[str, int | None]]:
    """Read one bounded 2007 level table; reject ambiguous headers/grains."""
    lines = content.splitlines()
    if not lines:
        raise ValueError("empty 2007 PRN table")
    header = shlex.split(lines[0])
    if header[0] != "DATES" or len(header) != len(set(header)):
        raise ValueError("invalid or duplicate 2007 PRN header")
    if not SERIES.keys() <= set(header):
        raise ValueError("missing mapped series in 2007 PRN table")
    indexes = {code: header.index(code) for code in SERIES}
    for code, index in indexes.items():
        if index != SERIES[code]["old_pdf_line"]:
            raise ValueError(f"2007 PRN code/PDF line mismatch: {code}")
    result: dict[str, dict[str, int | None]] = {}
    for line in lines[1:]:
        if not line.strip():
            continue
        cells = shlex.split(line)
        if len(cells) != len(header):
            raise ValueError("2007 PRN row width mismatch or repeated header")
        quarter = normalize_old_date(cells[0])
        if quarter in result:
            raise ValueError(f"duplicate 2007 PRN quarter: {quarter}")
        result[quarter] = {
            code: parse_million(cells[indexes[code]]) for code in SERIES
        }
    if not result:
        raise ValueError("no 2007 PRN observations")
    require_contiguous_quarters(list(result))
    return result


def parse_csv(content: str) -> dict[str, dict[str, int | None]]:
    """Read one bounded 2023 level table; enforce one row per quarter."""
    reader = csv.DictReader(io.StringIO(content))
    header = reader.fieldnames or []
    if not header or header[0] != "date" or len(header) != len(set(header)):
        raise ValueError("invalid or duplicate 2023 CSV header")
    if not SERIES.keys() <= set(header):
        raise ValueError("missing mapped series in 2023 CSV table")
    for code, spec in SERIES.items():
        if header.index(code) != spec["new_pdf_line"]:
            raise ValueError(f"2023 CSV code/PDF line mismatch: {code}")
    result: dict[str, dict[str, int | None]] = {}
    for row in reader:
        if None in row or any(value is None for value in row.values()):
            raise ValueError("2023 CSV row width mismatch")
        quarter = row["date"]
        if not re.fullmatch(r"\d{4}:Q[1-4]", quarter):
            raise ValueError(f"invalid 2023 CSV quarter: {quarter!r}")
        if quarter in result:
            raise ValueError(f"duplicate 2023 CSV quarter: {quarter}")
        result[quarter] = {code: parse_million(row[code]) for code in SERIES}
    if not result:
        raise ValueError("no 2023 CSV observations")
    require_contiguous_quarters(list(result))
    return result


def parse_million(value: str) -> int | None:
    if value == "ND":
        return None
    if not re.fullmatch(r"-?\d+", value):
        raise ValueError(f"non-integer Z.1 million-dollar cell: {value!r}")
    return int(value)


def validate_dictionary(content: str) -> dict[str, str]:
    found: dict[str, str] = {}
    for line in content.splitlines():
        fields = line.split("\t")
        if fields[0] not in SERIES:
            continue
        code = fields[0]
        if code in found or len(fields) < 5:
            raise ValueError(f"duplicate or malformed 2023 dictionary code: {code}")
        if SERIES[code]["label"].casefold() not in fields[1].casefold():
            # The official dictionary inserts 'total' before time deposits.
            if code != "FL153030005.Q" or "total time and savings deposits" not in fields[1].casefold():
                raise ValueError(f"2023 dictionary label mismatch: {code}")
        if fields[3] != "Table L.101 Households and Nonprofit Organizations":
            raise ValueError(f"2023 dictionary sector/table mismatch: {code}")
        if fields[4] != "Millions of dollars; not seasonally adjusted":
            raise ValueError(f"2023 dictionary unit mismatch: {code}")
        found[code] = fields[1]
    if set(found) != set(SERIES):
        raise ValueError("2023 dictionary lacks a mapped series")
    return found


def pdf_text(path: Path) -> str:
    try:
        completed = subprocess.run(
            ["pdftotext", "-layout", str(path), "-"],
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as error:
        raise RuntimeError("pdftotext is required for the independent PDF unit check") from error
    return completed.stdout


def printed_pdf_anchor(text: str, *, release: str, code: str) -> Decimal:
    """Read the last displayed quarter from a single official PDF table line."""
    if release == "2007-03-08":
        heading = "L.100 Households and Nonprofit Organizations (1)"
        next_heading = "L.101 Nonfinancial Business (1)"
        line_no = SERIES[code]["old_pdf_line"]
        label = SERIES[code]["label"]
        pattern = rf"^\s*{line_no}\s+{re.escape(label)}\s+"
    elif release == "2023-03-09":
        heading = "L.101 Households and Nonprofit Organizations (1)"
        next_heading = "L.102 Nonfinancial Business (1)"
        line_no = SERIES[code]["new_pdf_line"]
        label = SERIES[code]["label"]
        pattern = rf"^\s*{line_no}\s+{re.escape(code[:-2])}\s+{re.escape(label)}\s+"
    else:
        raise ValueError(f"unsupported release: {release}")
    start = text.find(heading)
    end = text.find(next_heading, start + len(heading))
    if start < 0 or end < 0:
        raise ValueError(f"PDF household level table not located: {release}")
    section = text[start:end]
    unit_heading = "Billions of dollars; amounts outstanding end of period, not seasonally adjusted"
    if unit_heading not in section:
        raise ValueError(f"PDF unit/stock definition not found: {release}")
    matches = [line for line in section.splitlines() if re.search(pattern, line)]
    if len(matches) != 1:
        raise ValueError(f"PDF series line not uniquely located: {release}/{code}")
    numbers = re.findall(r"(?<!\S)-?\d+\.\d+(?!\S)", matches[0])
    if not numbers:
        raise ValueError(f"PDF displayed values absent: {release}/{code}")
    return Decimal(numbers[-1])


def require_pdf_rounding(raw_millions: int | None, printed_billions: Decimal) -> None:
    if raw_millions is None:
        raise ValueError("PDF anchor quarter is missing in machine-readable table")
    value_billions = Decimal(raw_millions) / Decimal(1000)
    if abs(value_billions - printed_billions) > Decimal("0.05"):
        raise ValueError(
            f"PDF/ZIP unit or value mismatch: {value_billions} vs {printed_billions} billion"
        )


def build_report(raw: Path = RAW) -> dict:
    integrity = inspect_archives(raw)
    file_info = {entry["filename"]: entry for entry in integrity["files"]}
    with ZipFile(raw / OLD_ZIP) as archive:
        old = parse_prn(archive.read(OLD_MEMBER).decode("ascii"))
    with ZipFile(raw / NEW_ZIP) as archive:
        new = parse_csv(archive.read(NEW_MEMBER).decode("utf-8-sig"))
        dictionary = validate_dictionary(
            archive.read(NEW_DICTIONARY).decode("utf-8-sig")
        )
    if (next(iter(old)), max(old)) != (OLD_START, OVERLAP) or (
        next(iter(new)), max(new)
    ) != (NEW_START, NEW_ENDPOINT):
        raise ValueError("unexpected Z.1 vintage span")
    for release, table in (("2007", old), ("2023", new)):
        for code in SERIES:
            if any(row[code] is not None and row[code] < 0 for row in table.values()):
                raise ValueError(f"negative deposit asset level: {release}/{code}")
    old_pdf = pdf_text(raw / "2007-03-08_z1.pdf")
    new_pdf = pdf_text(raw / "2023-03-09_z1.pdf")

    items = []
    for code, spec in SERIES.items():
        old_overlap = old[OVERLAP][code]
        new_overlap = new[OVERLAP][code]
        new_endpoint = new[NEW_ENDPOINT][code]
        if None in (old_overlap, new_overlap, new_endpoint):
            raise ValueError(f"mapped quarter is ND: {code}")
        old_printed = printed_pdf_anchor(old_pdf, release="2007-03-08", code=code)
        new_printed = printed_pdf_anchor(new_pdf, release="2023-03-09", code=code)
        require_pdf_rounding(old_overlap, old_printed)
        require_pdf_rounding(new_endpoint, new_printed)
        assert isinstance(old_overlap, int)
        assert isinstance(new_overlap, int)
        assert isinstance(new_endpoint, int)
        items.append({
            "code": code,
            "label_2007_pdf": spec["label"],
            "label_2023_dictionary": dictionary[code],
            "2006_q4_as_2007_release_millions": old_overlap,
            "2006_q4_as_2023_release_millions": new_overlap,
            "historical_revision_millions": new_overlap - old_overlap,
            "historical_revision_percent_of_2007_value": round(
                100 * (new_overlap - old_overlap) / old_overlap, 3
            ),
            "2022_q4_as_2023_release_millions": new_endpoint,
            "pdf_anchor_2007_billions": str(old_printed),
            "pdf_anchor_2023_billions": str(new_printed),
            "missing_2007_table_rows": sum(row[code] is None for row in old.values()),
            "missing_2023_table_rows": sum(row[code] is None for row in new.values()),
        })

    return {
        "schema_version": "owg.fed-z1-household-deposits-bridge.v1",
        "status": "code-label-unit match; not a calibrated point-in-time panel",
        "grain": "release vintage x observation quarter x named household/nonprofit deposit instrument",
        "unit": UNIT,
        "matched_observation_quarter": OVERLAP,
        "old_release_table": "L.100 Households and Nonprofit Organizations",
        "old_member": OLD_MEMBER,
        "new_release_table": "L.101 Households and Nonprofit Organizations",
        "new_member": NEW_MEMBER,
        "new_dictionary_member": NEW_DICTIONARY,
        "vintage_archive_evidence": {
            name: {
                "sha256": file_info[name]["sha256"],
                "conservative_available_not_before_utc": file_info[name][
                    "conservative_available_not_before_utc"
                ],
            }
            for name in (OLD_ZIP, "2007-03-08_z1.pdf", NEW_ZIP, "2023-03-09_z1.pdf")
        },
        "rows_2007_table": len(old),
        "rows_2023_table": len(new),
        "series": items,
        "point_in_time_rule": (
            "The 2023 release's historical rows are revisions and are unavailable "
            "to a 2007 actor. The surviving 2007 bytes are conservatively not "
            "authorized as actor inputs before 2007-06-08."
        ),
        "comparability_limit": (
            "The 2007 and 2023 PDFs identify the same titled sector and deposit "
            "instruments, but their footnotes describe different constituents; "
            "whether coverage changed is unresolved and revision causes are "
            "not decomposed. Matching series IDs do not "
            "establish an invariant measurement population."
        ),
    }


def main() -> None:
    report = build_report()
    ARTIFACT.parent.mkdir(parents=True, exist_ok=True)
    ARTIFACT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"artifact": str(ARTIFACT), "series": len(report["series"])}))


if __name__ == "__main__":
    main()
