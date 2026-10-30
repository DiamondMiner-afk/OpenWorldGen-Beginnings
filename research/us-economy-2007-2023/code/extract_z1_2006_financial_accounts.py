"""Extract the December 2006 Z.1 level tables without erasing source discrepancies.

The source ZIP is checked against an archived PDF at every displayed monetary
cell and against the official coded PDF for series-to-row alignment. Earlier
ZIP observations are preserved but do not inherit exact-byte certification
from the later displayed anchors. Nothing here admits historical actor input.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shlex
import subprocess
from decimal import Decimal
from pathlib import Path
from typing import Any
from zipfile import ZipFile


ROOT = Path(__file__).resolve().parents[1]
RAW = ROOT / "data/raw/fed-z1-preorigin"
REPORT = ROOT / "artifacts/z1_2006_financial_accounts_qc.json"
HISTORY = ROOT / "data/processed/fed-z1/2006-12-07/levels-candidate.json"
SOURCES = {
    "quarterly_zip": (
        "2006-12-07_ltabs.zip",
        "ddfa0a8dfe7d8ffa08382b431fa4975f16d6731ab37bdddd618fb736e91eb239",
    ),
    "archived_pdf": (
        "alias-capture-2026-10-20/z1-current-20061216235414.pdf",
        "1f2f72bb87bb3baf29379fa0bb42fdd15a56e2c95d57afb76a8c937827101f9e",
    ),
    "coded_pdf": (
        "definitions-2026-10-21/coded.pdf",
        "68ac8e31e62a75c8ecd4e71b4e378db8794ffc018e456f0ecc8432b726cd4f2b",
    ),
}
TABLES = (5,) + tuple(range(100, 131)) + tuple(range(200, 232))
DISPLAY_PERIODS = (
    "2001:Q4", "2002:Q4", "2003:Q4", "2004:Q4",
    "2005:Q1", "2005:Q2", "2005:Q3", "2005:Q4",
    "2006:Q1", "2006:Q2", "2006:Q3",
)
CODED_PERIODS = ("2002:Q4", "2003:Q4", "2004:Q4", "2005:Q4")
UNIT_HEADING = "Billions of dollars; amounts outstanding end of period, not seasonally adjusted"
HEADING = re.compile(r"^([A-Z])\.(\d{1,3})(\.[a-z])? (.+)$", re.MULTILINE)
CELL = re.compile(r"-?\d+\.\d+|ND|NA")
CODE = re.compile(r"[A-Z]{2}\d{9}\.Q")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pdf_text(path: Path) -> str:
    return subprocess.run(
        ["pdftotext", "-layout", str(path), "-"],
        check=True, capture_output=True, text=True,
    ).stdout


def parse_prn(content: str) -> dict[str, Any]:
    lines = [line for line in content.splitlines() if line.strip()]
    if not lines:
        raise ValueError("empty level table")
    header = shlex.split(lines[0])
    if not header or header[0] != "DATES" or len(header) < 2:
        raise ValueError("level table lacks DATES header")
    codes = header[1:]
    if any(not CODE.fullmatch(code) for code in codes):
        raise ValueError("invalid quarterly series code")
    periods: list[str] = []
    values: list[list[int | None]] = []
    missing_cells = []
    previous = None
    for line in lines[1:]:
        cells = shlex.split(line)
        if len(cells) != len(header) or not re.fullmatch(r"\d{4}0[1-4]", cells[0]):
            raise ValueError("invalid quarterly date or row width")
        ordinal = int(cells[0][:4]) * 4 + int(cells[0][-1]) - 1
        if previous is not None and ordinal != previous + 1:
            raise ValueError("duplicate, missing, or reordered quarter")
        previous = ordinal
        periods.append(f"{cells[0][:4]}:Q{cells[0][-1]}")
        row = []
        for column, cell in enumerate(cells[1:], 1):
            if cell.strip() in {"ND", "NA"}:
                row.append(None)
                missing_cells.append({"period": periods[-1], "printed_row": column, "raw_token": cell})
            elif re.fullmatch(r"-?\d+", cell):
                row.append(int(cell))
            else:
                raise ValueError(f"invalid million-dollar value: {cell!r}")
        values.append(row)
    if not periods:
        raise ValueError("level table has no observations")
    return {"codes": codes, "periods": periods, "values": values, "missing_cells": missing_cells}


def pdf_tables(text: str, *, coded: bool = False) -> dict[int, dict[str, Any]]:
    """Preserve base/supplementary identities and printed row positions.

    Some labels wrap above/below the value row, so label_fragment is explicitly
    a fragment. Full text and footnotes remain in the immutable source PDF.
    """
    headings = list(HEADING.finditer(text))
    found = {}
    width = len(CODED_PERIODS if coded else DISPLAY_PERIODS)
    for index, match in enumerate(headings):
        number = int(match[2])
        if match[1] != "L" or match[3] or number not in TABLES:
            continue
        if number in found:
            raise ValueError(f"duplicate base table L.{number}")
        end = headings[index + 1].start() if index + 1 < len(headings) else len(text)
        section = text[match.start():end]
        if UNIT_HEADING not in section:
            raise ValueError(f"stock/seasonality unit absent in L.{number}")
        rows = {}
        for line in section.splitlines():
            tokens = line.split()
            if len(tokens) < width + 3 or not tokens[0].isdigit() or tokens[-1] != tokens[0]:
                continue
            numeric = tokens[-width - 1:-1]
            if not all(CELL.fullmatch(cell) for cell in numeric):
                continue
            row = int(tokens[0])
            if row in rows:
                raise ValueError(f"duplicate printed row L.{number}/{row}")
            label_start = 1
            code = None
            if coded and re.fullmatch(r"[A-Z]{2}\d{9}", tokens[1]):
                code = tokens[1] + ".Q"
                label_start = 2
            rows[row] = {
                "label_fragment": " ".join(tokens[label_start:-width - 1]),
                "values": numeric,
                "code": code,
            }
        found[number] = {
            "title": match[4],
            "pdf_page": text[:match.start()].count("\f") + 1,
            "rows": rows,
        }
    if set(found) != set(TABLES):
        raise ValueError(f"missing base tables: {sorted(set(TABLES) - set(found))}")
    return found


def check_pdf_cells(table: dict, printed: dict, periods: tuple[str, ...], *, coded: bool) -> int:
    count = 0
    positions = {period: index for index, period in enumerate(table["periods"])}
    for row, code in enumerate(table["codes"], 1):
        if row not in printed["rows"]:
            raise ValueError(f"missing printed monetary row {row}")
        anchor = printed["rows"][row]
        if coded and anchor["code"] != code:
            raise ValueError(f"series/row mismatch at {row}: {code} vs {anchor['code']}")
        for column, period in enumerate(periods):
            if period not in positions:
                raise ValueError(f"missing displayed quarter {period}")
            raw = table["values"][positions[period]][row - 1]
            shown = anchor["values"][column]
            if raw is None or shown in {"ND", "NA"}:
                if raw is not None or shown not in {"ND", "NA"}:
                    raise ValueError(f"PDF missingness mismatch at {row}/{period}")
            elif abs(Decimal(raw) / 1000 - Decimal(shown)) > Decimal("0.05"):
                raise ValueError(f"PDF/ZIP mismatch at {row}/{period}: {raw} vs {shown}")
            count += 1
    return count


def value(tables: dict[int, dict], number: int, row: int, period: str) -> int | None:
    table = tables[number]
    return table["values"][table["periods"].index(period)][row - 1]


def check_repeated_codes(tables: dict[int, dict]) -> dict:
    seen: dict[tuple[str, str], int | None] = {}
    checked = 0
    repeated_codes = set()
    for table in tables.values():
        for row, code in enumerate(table["codes"]):
            for index, period in enumerate(table["periods"]):
                key = (code, period)
                current = table["values"][index][row]
                if key in seen:
                    checked += 1
                    repeated_codes.add(code)
                    if current != seen[key]:
                        raise ValueError(f"same series disagrees across table positions: {code}/{period}")
                else:
                    seen[key] = current
    return {"repeated_series_codes": len(repeated_codes), "repeated_cell_comparisons": checked}


# Each sum is stated by the displayed hierarchy. Aggregate/parent rows are
# deliberately not summed with their children. Tolerance is one reported
# million per term (including target), a conservative reporting-resolution
# screen, not a statistical confidence interval or reconciliation adjustment.
IDENTITIES = (
    ("household_deposits", (100, 2), ((100, 3), (100, 4), (100, 5), (100, 6))),
    ("household_financial_assets", (100, 1), tuple((100, r) for r in (2, 7, 16, 17, 18, 19, 20, 21, 22))),
    ("household_credit_assets", (100, 7), tuple((100, r) for r in (8, 9, 12, 13, 14, 15))),
    ("household_treasuries", (100, 9), ((100, 10), (100, 11))),
    ("household_liabilities", (100, 23), tuple((100, r) for r in (24, 31, 32, 33))),
    ("household_credit_liabilities", (100, 24), tuple((100, r) for r in range(25, 31))),
    ("nonfinancial_business_assets", (101, 1), tuple((101, r) for r in (2, 3, 4, 5, 6, 7, 14, 15, 16))),
    ("nonfinancial_business_liabilities", (101, 17), tuple((101, r) for r in (18, 25, 26, 27))),
    ("business_sector_asset_partition", (101, 1), ((102, 1), (103, 1), (104, 1))),
    ("business_sector_liability_partition", (101, 17), ((102, 21), (103, 14), (104, 6))),
    ("state_local_liabilities", (105, 18), ((105, 19), (105, 24))),
    ("federal_liabilities", (106, 13), tuple((106, r) for r in (14, 15, 16, 21, 22, 23))),
    ("central_bank_liabilities", (108, 14), tuple((108, r) for r in (15, 16, 17, 21))),
    ("checkable_deposit_issuers", (204, 1), tuple((204, r) for r in (2, 6, 10, 11))),
    ("checkable_deposit_holders_with_mail_float", (204, 12), tuple((204, r) for r in (13, 14, 18, 19, 20, 23, 36))),
    ("checkable_deposit_assets_liabilities", (204, 1), ((204, 12),)),
    ("time_deposit_small_large", (205, 1), ((205, 2), (205, 9))),
    ("time_deposit_holders", (205, 16), tuple((205, r) for r in (17, 18, 21, 22, 23, 24))),
    ("time_deposit_assets_liabilities", (205, 1), ((205, 16),)),
    ("money_market_fund_holders", (206, 1), tuple((206, r) for r in range(2, 10))),
    ("interbank_published_discrepancy", (203, 1), ((203, 35), (203, 45))),
    ("unidentified_claims_published_discrepancy", (231, 1), ((231, 20), (231, 43))),
)


def identity_checks(tables: dict[int, dict]) -> list[dict]:
    checks = []
    for name, target, terms in IDENTITIES:
        residuals = []
        for period in DISPLAY_PERIODS:
            cells = [value(tables, *target, period)] + [value(tables, *term, period) for term in terms]
            if any(cell is None for cell in cells):
                raise ValueError(f"missing displayed identity term: {name}/{period}")
            residual = cells[0] - sum(cells[1:])
            tolerance = len(cells)
            if abs(residual) > tolerance:
                raise ValueError(f"identity residual exceeds reporting-resolution screen: {name}/{period}/{residual}")
            residuals.append({"period": period, "residual_millions": residual})
        checks.append({
            "id": name, "target": list(target), "terms": [list(term) for term in terms],
            "tolerance_millions": len(terms) + 1,
            "max_abs_residual_millions": max(abs(item["residual_millions"]) for item in residuals),
            "residuals": residuals,
        })
    # L.5 explicitly adds financial assets omitted from liability totals and
    # subtracts unallocated claims/floats. These are source measurement and
    # coverage terms, not economic transactions or synthetic closing accounts.
    signed_terms = [(5, r, 1) for r in (19, 20, 21, 22)] + [(5, r, -1) for r in range(23, 32)]
    residuals = []
    for period in DISPLAY_PERIODS:
        total = value(tables, 5, 32, period)
        terms = [value(tables, number, row, period) for number, row, _ in signed_terms]
        if total is None or any(term is None for term in terms):
            raise ValueError("missing global asset/liability bridge term")
        residual = total - sum(cell * term[2] for cell, term in zip(terms, signed_terms))
        if abs(residual) > len(terms) + 1:
            raise ValueError("global asset/liability bridge exceeds reporting-resolution screen")
        residuals.append({"period": period, "residual_millions": residual})
    checks.append({
        "id": "global_assets_liabilities_with_published_adjustments",
        "target": [5, 32], "signed_terms": [list(term) for term in signed_terms],
        "tolerance_millions": len(signed_terms) + 1,
        "max_abs_residual_millions": max(abs(item["residual_millions"]) for item in residuals),
        "residuals": residuals,
    })
    return checks


def row_reference(tables: dict[int, dict], number: int, row: int) -> dict:
    table = tables[number]
    return {
        "table_id": f"L.{number}", "printed_row": row,
        "series_code": table["codes"][row - 1],
        "label_fragment": table["labels"][row - 1],
    }


def instrument_marginals(tables: dict[int, dict]) -> list[dict]:
    """Measured issuer/holder margins, without inventing bilateral links."""
    definitions = (
        ("checkable_deposits_and_currency", (204, 1),
         [("monetary_authority", ((204, 2),)), ("commercial_banking", ((204, 6),)),
          ("savings_institutions", ((204, 10),)), ("credit_unions", ((204, 11),))],
         [("households_nonprofits", ((204, 13),)), ("nonfinancial_business", ((204, 14),)),
          ("state_local_government", ((204, 18),)), ("federal_government", ((204, 19),)),
          ("rest_of_world", ((204, 20),)), ("financial_sectors", ((204, 23),)),
          ("mail_float_adjustment", ((204, 36),))]),
        ("time_and_savings_deposits", (205, 1),
         [("commercial_banking", ((205, 3), (205, 10))),
          ("savings_institutions", ((205, 7), (205, 14))),
          ("credit_unions", ((205, 8), (205, 15)))],
         [("households_nonprofits", ((205, 17),)), ("nonfinancial_business", ((205, 18),)),
          ("state_local_government", ((205, 21),)), ("federal_government", ((205, 22),)),
          ("rest_of_world", ((205, 23),)), ("financial_sectors", ((205, 24),))]),
        ("money_market_fund_shares", (206, 1),
         [("money_market_mutual_funds", ((121, 13),))],
         [(tables[206]["labels"][r - 1], ((206, r),)) for r in range(2, 10)]),
        ("treasury_securities", (209, 1),
         [("federal_government", ((106, 17), (106, 18)))],
         [(tables[209]["labels"][r - 1], ((209, r),))
          for r in (5, 8, 9, 10, 11, 12, 13, *range(18, 32))]),
        ("corporate_equities_market_value", (213, 1),
         [(tables[213]["labels"][r - 1], ((213, r),)) for r in (2, 3, 4)],
         [(tables[213]["labels"][r - 1], ((213, r),)) for r in range(6, 20)]),
        ("bank_loans_not_elsewhere_classified", (215, 12),
         [(tables[215]["labels"][r - 1], ((215, r),)) for r in (15, 16, 17, 18, 19, 24, 25, 26)],
         [(tables[215]["labels"][r - 1], ((215, r),)) for r in (7, 8, 9, 10, 11)]),
        ("mortgages", (217, 6),
         [(tables[217]["labels"][r - 1], ((217, r),)) for r in (7, 8, 12, 13)],
         [(tables[217]["labels"][r - 1], ((217, r),)) for r in range(15, 32)]),
    )
    result = []
    for name, target, issuers, holders in definitions:
        item = {
            "instrument": name, "period": "2006:Q3",
            "total_millions": value(tables, *target, "2006:Q3"),
            "total_reference": row_reference(tables, *target),
            "bilateral_links_identified": False,
        }
        for side, groups in (("issuers_or_borrowers", issuers), ("holders_or_lenders", holders)):
            output_groups = []
            for sector, terms in groups:
                output_groups.append({
                    "source_sector": sector,
                    "value_millions": sum(value(tables, *term, "2006:Q3") for term in terms),
                    "source_rows": [row_reference(tables, *term) for term in terms],
                })
            residuals = []
            term_count = sum(len(terms) for _, terms in groups)
            for period in DISPLAY_PERIODS:
                total = value(tables, *target, period)
                terms = [value(tables, *term, period) for _, refs in groups for term in refs]
                if total is None or any(term is None for term in terms):
                    raise ValueError(f"missing instrument margin: {name}/{side}/{period}")
                residual = total - sum(terms)
                if abs(residual) > term_count + 1:
                    raise ValueError(f"instrument margin does not reconcile: {name}/{side}/{period}/{residual}")
                residuals.append(residual)
            item[side] = output_groups
            item[side + "_max_abs_residual_millions"] = max(map(abs, residuals))
        result.append(item)
    return result


def build(raw: Path = RAW) -> tuple[dict, dict]:
    paths = {}
    for name, (relative, expected) in SOURCES.items():
        path = raw / relative
        if sha256(path) != expected:
            raise ValueError(f"source hash mismatch: {name}")
        paths[name] = path
    ordinary_text = pdf_text(paths["archived_pdf"])
    coded_text = pdf_text(paths["coded_pdf"])
    if "December 7, 2006" not in ordinary_text[:5000] or "Third Quarter 2006" not in ordinary_text[:5000]:
        raise ValueError("archived release cover mismatch")
    ordinary = pdf_tables(ordinary_text)
    coded = pdf_tables(coded_text, coded=True)
    tables = {}
    pdf_checks = coded_checks = 0
    with ZipFile(paths["quarterly_zip"]) as archive:
        if archive.testzip() is not None:
            raise ValueError("ZIP CRC failure")
        for number in TABLES:
            table = parse_prn(archive.read(f"ltab{number}d.prn").decode("ascii"))
            if table["periods"][0] != "1952:Q1" or table["periods"][-1] != "2006:Q3":
                raise ValueError(f"unexpected historical coverage L.{number}")
            pdf_checks += check_pdf_cells(table, ordinary[number], DISPLAY_PERIODS, coded=False)
            coded_checks += check_pdf_cells(table, coded[number], CODED_PERIODS, coded=True)
            table["title"] = ordinary[number]["title"]
            table["pdf_page"] = ordinary[number]["pdf_page"]
            table["labels"] = [ordinary[number]["rows"][r]["label_fragment"] for r in range(1, len(table["codes"]) + 1)]
            table["excluded_analytical_rows"] = sorted(set(ordinary[number]["rows"]) - set(range(1, len(table["codes"]) + 1)))
            tables[number] = table
    repeated = check_repeated_codes(tables)
    identities = identity_checks(tables)
    marginals = instrument_marginals(tables)
    rows = sum(len(table["codes"]) for table in tables.values())
    observations = sum(len(table["codes"]) * len(table["periods"]) for table in tables.values())
    missing = sum(cell is None for table in tables.values() for row in table["values"] for cell in row)
    missing_tokens: dict[str, int] = {}
    for table in tables.values():
        for cell in table["missing_cells"]:
            token = cell["raw_token"]
            missing_tokens[token] = missing_tokens.get(token, 0) + 1
    summary = {
        "schema_version": "owg.z1-2006-levels-qc.v1",
        "status": "candidate_measurement_not_historical_input_admission",
        "release_date": "2006-12-07", "last_observation": "2006:Q3",
        "unit": "millions of current US dollars, end-of-quarter stock, not seasonally adjusted",
        "source_sha256": {name: expected for name, (_, expected) in SOURCES.items()},
        "table_count": len(tables), "table_row_count": rows,
        "all_history_cells": observations, "missing_cells": missing,
        "missing_raw_token_counts": missing_tokens,
        "archived_pdf_displayed_cell_checks": pdf_checks,
        "coded_pdf_series_row_checks": rows,
        "coded_pdf_annual_cell_checks": coded_checks,
        "pdf_cell_tolerance_billions": "0.05",
        "displayed_periods": list(DISPLAY_PERIODS),
        "same_series_consistency": repeated,
        "identity_checks": identities,
        "instrument_marginals_2006_q3": marginals,
        "published_instrument_discrepancy_rows_2006_q3": [
            {**row_reference(tables, number, row + 1),
             "value_millions": value(tables, number, row + 1, "2006:Q3")}
            for number, table in tables.items() if number >= 200
            for row, label in enumerate(table["labels"]) if label.startswith("Discrepancy")
        ],
        "reported_unidentified_claims_discrepancy": {
            "table": "L.231", "line": 43,
            "period": "2006:Q3", "value_millions": value(tables, 231, 43, "2006:Q3"),
            "meaning": "Published liabilities minus assets in this instrument table; retained without a balancing plug.",
        },
        "global_asset_liability_bridge_2006_q3": {
            "table": "L.5",
            "total_liabilities_millions": value(tables, 5, 19, "2006:Q3"),
            "financial_assets_not_included_in_liabilities_millions": sum(value(tables, 5, row, "2006:Q3") for row in (20, 21, 22)),
            "liabilities_not_identified_as_assets_millions": sum(value(tables, 5, row, "2006:Q3") for row in range(23, 29)),
            "floats_not_included_in_assets_millions": sum(value(tables, 5, row, "2006:Q3") for row in range(29, 32)),
            "totals_identified_to_sectors_as_assets_millions": value(tables, 5, 32, "2006:Q3"),
            "treatment": "Retained published bridge; no source discrepancy is treated as spendable cash or a simulated transfer.",
        },
        "limitations": [
            "All-history ZIP cells outside the displayed PDF periods lack this independent exact-value anchor and are tagged machine-readable-only.",
            "Coded PDF was retrieved in 2026 and supplies code/row definitions, not independent pre-origin byte evidence.",
            "Repeated codes and aggregate/subsector tables overlap; row positions are preserved and tables must not be indiscriminately summed.",
            "Printed labels can wrap; labels are fragments and full headings/footnotes in the source PDF control interpretation.",
            "Monetary levels can be negative under netting/residual conventions; negative cells are not coerced to zero.",
            "Sector total liabilities exclude some equity claims shown as memo items; these are not ready-made simulation net-worth accounts.",
            "A trusted archive-origin verifier, full instrument/sector partition, measured agent allocation, and balance-sheet reconciliation remain open.",
        ],
    }
    history = {
        "schema_version": "owg.z1-2006-levels-candidate.v1",
        "status": summary["status"], "release_date": summary["release_date"],
        "unit": summary["unit"], "source_sha256": summary["source_sha256"],
        "grain": "table_id, printed_row, quarter; series_code alone is not a primary key",
        "pdf_anchored_periods": list(DISPLAY_PERIODS),
        "other_period_status": "machine_readable_only_not_independently_origin_verified",
        "tables": [{
            "table_id": f"L.{number}", "title": table["title"],
            "source_pdf_page": table["pdf_page"], "periods": table["periods"],
            "excluded_analytical_rows": table["excluded_analytical_rows"],
            "missing_cells": table["missing_cells"],
            "rows": [{
                "printed_row": row + 1, "series_code": code,
                "label_fragment": table["labels"][row],
                "values_millions": [values[row] for values in table["values"]],
            } for row, code in enumerate(table["codes"])],
        } for number, table in tables.items()],
    }
    return summary, history


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, default=REPORT)
    parser.add_argument("--history", type=Path, default=HISTORY)
    args = parser.parse_args()
    report, history = build()
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.history.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    args.history.write_text(json.dumps(history, separators=(",", ":")) + "\n", encoding="utf-8")
    print(json.dumps({key: report[key] for key in (
        "status", "table_count", "table_row_count", "all_history_cells",
        "missing_cells", "archived_pdf_displayed_cell_checks", "coded_pdf_annual_cell_checks",
    )}, indent=2))


if __name__ == "__main__":
    main()
