"""Bounded, read-only audit of the archived 2005 BEA annual Make/Use sheet.

Requires xlrd==2.0.2 for the original BIFF .xls bytes. No workbook conversion,
balancing adjustment, sector aggregation, or point-in-time certification occurs.
Run this file to regenerate artifacts/bea_io_2005_matrix_qc.json.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
from decimal import Decimal
from pathlib import Path

import xlrd


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / (
    "data/raw/bea-io/2007-06-05-capture/"
    "Annual_IOMakeUse_Before_Redfinitions_1998-2005.xls"
)
PROVENANCE = SOURCE.with_name("source-provenance.json")
ARTIFACT = ROOT / "artifacts/bea_io_2005_matrix_qc.json"
EXPECTED_SHA256 = "144482d30279707c6f4ee43b91a52f95ec3e851ba8754844cb5735d7b4df92fa"
EXPECTED_SIZE = 877_056
EXPECTED_SHEETS = ["MakeUse_ReadMe", "NAICS codes", *map(str, range(1998, 2006))]
MAKE_SPECIAL = ["S002", "S004"]
USE_SPECIAL = ["S001", "S002", "S003", "S004"]
FINAL_CODES = [
    "F010", "F020", "F030", "F040", "F050", "F06C", "F06I",
    "F07C", "F07I", "F08C", "F08I", "F09C", "F09I",
]

# Coordinates below are zero-based. The workbook's printed rows are one-based:
# Make industries 8:72, commodity columns C:BQ, margins row 73/column BR;
# Use commodities 83:151, industry columns C:BO, final uses BQ:CC.
MAKE_INDUSTRY_ROWS = range(7, 72)
MAKE_COMMODITY_COLS = range(2, 69)
MAKE_TOTAL_ROW = 72
MAKE_OUTPUT_COL = 69
USE_COMMODITY_ROWS = range(82, 151)
USE_INDUSTRY_COLS = range(2, 67)
USE_INTERMEDIATE_COL = 67
USE_FINAL_COLS = range(68, 81)
USE_FINAL_TOTAL_COL = 81
USE_COMMODITY_OUTPUT_COL = 82
USE_INTERMEDIATE_ROW = 151
USE_VALUE_ADDED_ROWS = range(152, 155)
USE_VALUE_ADDED_ROW = 155
USE_OUTPUT_ROW = 156


def verify_source(raw: bytes, provenance: dict) -> str:
    """Check both a fixed file fingerprint and its separate provenance record."""
    digest = hashlib.sha256(raw).hexdigest()
    if len(raw) != EXPECTED_SIZE or digest != EXPECTED_SHA256:
        raise ValueError(f"archived BEA XLS hash/size mismatch: {digest}, {len(raw)} bytes")
    required = {
        "source_file_name": SOURCE.name,
        "sha256": EXPECTED_SHA256,
        "byte_size": EXPECTED_SIZE,
        "archive_capture_timestamp_utc": "2007-06-05T12:35:34Z",
        "point_in_time_status_for_2007_01_01": "release_available_exact_bytes_not_certified",
        "units": "millions of current U.S. dollars",
        "classification": "1997 NAICS, 65 annual industries and commodities",
        "tables": "standard make and use before redefinitions",
    }
    for field, expected in required.items():
        if provenance.get(field) != expected:
            raise ValueError(f"BEA provenance mismatch: {field}")
    return digest


def code(value: object) -> str:
    """Excel stores some NAICS-like codes as integral numbers, others as text."""
    if isinstance(value, str):
        result = value.strip()
    elif isinstance(value, (int, float)) and not isinstance(value, bool):
        if not math.isfinite(value) or int(value) != value:
            raise ValueError(f"non-integral BEA key: {value!r}")
        result = str(int(value))
    else:
        raise ValueError(f"invalid BEA key: {value!r}")
    if not re.fullmatch(r"[A-Z0-9]+", result):
        raise ValueError(f"invalid BEA key: {value!r}")
    return result


def unique_keys(keys: list[str], name: str) -> None:
    seen: set[str] = set()
    duplicates: set[str] = set()
    for key in keys:
        if key in seen:
            duplicates.add(key)
        seen.add(key)
    if duplicates:
        raise ValueError(f"duplicate {name} keys: {sorted(duplicates)}")


def label(value: object) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"blank or invalid BEA label: {value!r}")
    return " ".join(value.split())


def number(sheet: xlrd.sheet.Sheet, row: int, col: int) -> Decimal:
    cell = sheet.cell(row, col)
    if cell.ctype != xlrd.XL_CELL_NUMBER or not math.isfinite(cell.value):
        raise ValueError(f"missing or nonnumeric 2005 cell: {xlrd.formula.colname(col)}{row + 1}")
    return Decimal(str(cell.value))


def decimal_text(value: Decimal) -> str:
    return "0" if value == 0 else format(value, "f")


def residual_group(grain: str, items: list[tuple[str, Decimal]]) -> dict:
    """Preserve every nonzero signed residual; no tolerance or rebalance."""
    nonzero = [
        {"key": key, "residual_million": decimal_text(residual)}
        for key, residual in items
        if residual != 0
    ]
    return {
        "grain": grain,
        "compared": len(items),
        "nonzero_count": len(nonzero),
        "max_abs_million": decimal_text(max((abs(v) for _, v in items), default=Decimal(0))),
        "sum_abs_million": decimal_text(sum((abs(v) for _, v in items), Decimal(0))),
        "nonzero": nonzero,
    }


def build_report(source: Path = SOURCE, provenance_path: Path = PROVENANCE) -> dict:
    raw = source.read_bytes()
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    digest = verify_source(raw, provenance)
    workbook = xlrd.open_workbook(file_contents=raw, on_demand=True)
    try:
        names = workbook.sheet_names()
        if names != EXPECTED_SHEETS:
            raise ValueError(f"unexpected BEA XLS sheet names: {names}")
        readme = workbook.sheet_by_name("MakeUse_ReadMe")
        readme_text = "\n".join(str(readme.cell_value(r, 0)) for r in range(readme.nrows))
        for phrase in (
            'includes both the "make" and "use" matrices',
            "Values represent millions of dollars at producers' prices.",
            "millions of current dollars",
        ):
            if phrase not in readme_text:
                raise ValueError(f"BEA workbook unit/layout statement absent: {phrase}")
        sheet = workbook.sheet_by_name("2005")
        if (sheet.nrows, sheet.ncols) != (160, 83):
            raise ValueError(f"unexpected 2005 sheet shape: {(sheet.nrows, sheet.ncols)}")
        if not str(sheet.cell_value(1, 0)).startswith("The Make of Commodities by Industries"):
            raise ValueError("2005 Make title absent")
        if not str(sheet.cell_value(77, 0)).startswith("The Use of Commodities by Industries"):
            raise ValueError("2005 Use title absent")
        for position, expected in {
            (6, 0): "PubIndCode", (6, 1): "indLabel",
            (6, 69): "T008", (72, 0): "T007",
            (81, 0): "PubComCode", (81, 1): "comLabel",
            (81, 67): "T001", (81, 81): "T004", (81, 82): "T007",
            (151, 0): "T005", (155, 0): "T006", (156, 0): "T008",
        }.items():
            if str(sheet.cell_value(*position)).strip() != expected:
                raise ValueError(f"2005 structural anchor mismatch at {position}")

        make_industries = [code(sheet.cell_value(r, 0)) for r in MAKE_INDUSTRY_ROWS]
        make_industry_labels = [label(sheet.cell_value(r, 1)) for r in MAKE_INDUSTRY_ROWS]
        make_commodities = [code(sheet.cell_value(6, c)) for c in MAKE_COMMODITY_COLS]
        make_commodity_labels = [label(sheet.cell_value(5, c)) for c in MAKE_COMMODITY_COLS]
        use_industries = [code(sheet.cell_value(81, c)) for c in USE_INDUSTRY_COLS]
        use_industry_labels = [label(sheet.cell_value(80, c)) for c in USE_INDUSTRY_COLS]
        use_commodities = [code(sheet.cell_value(r, 0)) for r in USE_COMMODITY_ROWS]
        use_commodity_labels = [label(sheet.cell_value(r, 1)) for r in USE_COMMODITY_ROWS]
        final_codes = [code(sheet.cell_value(81, c)) for c in USE_FINAL_COLS]
        for name, keys in (
            ("Make industry", make_industries), ("Make commodity", make_commodities),
            ("Use industry", use_industries), ("Use commodity", use_commodities),
            ("Use final-category", final_codes),
        ):
            unique_keys(keys, name)
        if len(make_industries) != 65 or make_commodities != make_industries + MAKE_SPECIAL:
            raise ValueError("Make industry/commodity code layout mismatch")
        if use_industries != make_industries or use_commodities != make_industries + USE_SPECIAL:
            raise ValueError("Make/Use industry or commodity code layout mismatch")
        if final_codes != FINAL_CODES:
            raise ValueError("Use final-category code layout mismatch")
        if make_industry_labels != use_industry_labels:
            raise ValueError("Make/Use industry label mismatch")
        if make_commodity_labels != [
            use_commodity_labels[use_commodities.index(key)] for key in make_commodities
        ]:
            raise ValueError("Make/Use commodity label mismatch")
        if len({(i, c) for i in make_industries for c in make_commodities}) != 4355:
            raise ValueError("duplicate Make industry/commodity cell key")
        if len({(c, i) for c in use_commodities for i in use_industries}) != 4485:
            raise ValueError("duplicate Use commodity/industry cell key")
        # Reject absent or text-valued matrix cells. Explicit zeroes are data.
        for r in MAKE_INDUSTRY_ROWS:
            for c in MAKE_COMMODITY_COLS:
                number(sheet, r, c)
        for r in USE_COMMODITY_ROWS:
            for c in USE_INDUSTRY_COLS:
                number(sheet, r, c)

        n = lambda r, c: number(sheet, r, c)
        groups: dict[str, dict] = {}

        def add(name: str, grain: str, items: list[tuple[str, Decimal]]) -> None:
            groups[name] = residual_group(grain, items)

        add("make_industry_rows", "industry", [
            (key, sum((n(r, c) for c in MAKE_COMMODITY_COLS), Decimal(0)) - n(r, MAKE_OUTPUT_COL))
            for r, key in zip(MAKE_INDUSTRY_ROWS, make_industries)
        ])
        add("make_commodity_columns", "commodity", [
            (key, sum((n(r, c) for r in MAKE_INDUSTRY_ROWS), Decimal(0)) - n(MAKE_TOTAL_ROW, c))
            for c, key in zip(MAKE_COMMODITY_COLS, make_commodities)
        ])
        add("make_grand_from_industries", "national", [
            ("T008", sum((n(r, MAKE_OUTPUT_COL) for r in MAKE_INDUSTRY_ROWS), Decimal(0)) - n(MAKE_TOTAL_ROW, MAKE_OUTPUT_COL))
        ])
        add("make_grand_from_commodities", "national", [
            ("T007", sum((n(MAKE_TOTAL_ROW, c) for c in MAKE_COMMODITY_COLS), Decimal(0)) - n(MAKE_TOTAL_ROW, MAKE_OUTPUT_COL))
        ])
        add("use_commodity_intermediate_rows", "commodity", [
            (key, sum((n(r, c) for c in USE_INDUSTRY_COLS), Decimal(0)) - n(r, USE_INTERMEDIATE_COL))
            for r, key in zip(USE_COMMODITY_ROWS, use_commodities)
        ])
        add("use_commodity_final_rows", "commodity", [
            (key, sum((n(r, c) for c in USE_FINAL_COLS), Decimal(0)) - n(r, USE_FINAL_TOTAL_COL))
            for r, key in zip(USE_COMMODITY_ROWS, use_commodities)
        ])
        add("use_commodity_output_rows", "commodity", [
            (key, n(r, USE_INTERMEDIATE_COL) + n(r, USE_FINAL_TOTAL_COL) - n(r, USE_COMMODITY_OUTPUT_COL))
            for r, key in zip(USE_COMMODITY_ROWS, use_commodities)
        ])
        add("use_industry_intermediate_columns", "industry", [
            (key, sum((n(r, c) for r in USE_COMMODITY_ROWS), Decimal(0)) - n(USE_INTERMEDIATE_ROW, c))
            for c, key in zip(USE_INDUSTRY_COLS, use_industries)
        ])
        add("use_industry_value_added_columns", "industry", [
            (key, sum((n(r, c) for r in USE_VALUE_ADDED_ROWS), Decimal(0)) - n(USE_VALUE_ADDED_ROW, c))
            for c, key in zip(USE_INDUSTRY_COLS, use_industries)
        ])
        add("use_industry_output_columns", "industry", [
            (key, n(USE_INTERMEDIATE_ROW, c) + n(USE_VALUE_ADDED_ROW, c) - n(USE_OUTPUT_ROW, c))
            for c, key in zip(USE_INDUSTRY_COLS, use_industries)
        ])
        add("use_final_category_columns", "final-use category", [
            (key, sum((n(r, c) for r in USE_COMMODITY_ROWS), Decimal(0)) - n(USE_OUTPUT_ROW, c))
            for c, key in zip(USE_FINAL_COLS, final_codes)
        ])
        add("use_value_added_category_rows", "value-added category", [
            (code(sheet.cell_value(r, 0)), sum((n(r, c) for c in USE_INDUSTRY_COLS), Decimal(0)) - n(r, USE_INTERMEDIATE_COL))
            for r in USE_VALUE_ADDED_ROWS
        ])
        add("use_national_totals", "national", [
            ("intermediate_by_industry", sum((n(USE_INTERMEDIATE_ROW, c) for c in USE_INDUSTRY_COLS), Decimal(0)) - n(USE_INTERMEDIATE_ROW, USE_INTERMEDIATE_COL)),
            ("intermediate_by_commodity", sum((n(r, USE_INTERMEDIATE_COL) for r in USE_COMMODITY_ROWS), Decimal(0)) - n(USE_INTERMEDIATE_ROW, USE_INTERMEDIATE_COL)),
            ("value_added_components", sum((n(r, USE_INTERMEDIATE_COL) for r in USE_VALUE_ADDED_ROWS), Decimal(0)) - n(USE_VALUE_ADDED_ROW, USE_FINAL_TOTAL_COL)),
            ("gdp_from_final_categories", sum((n(USE_OUTPUT_ROW, c) for c in USE_FINAL_COLS), Decimal(0)) - n(USE_VALUE_ADDED_ROW, USE_FINAL_TOTAL_COL)),
            ("gdp_from_commodity_final_uses", sum((n(r, USE_FINAL_TOTAL_COL) for r in USE_COMMODITY_ROWS), Decimal(0)) - n(USE_VALUE_ADDED_ROW, USE_FINAL_TOTAL_COL)),
            ("output_by_industry", sum((n(USE_OUTPUT_ROW, c) for c in USE_INDUSTRY_COLS), Decimal(0)) - n(USE_OUTPUT_ROW, USE_COMMODITY_OUTPUT_COL)),
            ("output_by_commodity", sum((n(r, USE_COMMODITY_OUTPUT_COL) for r in USE_COMMODITY_ROWS), Decimal(0)) - n(USE_OUTPUT_ROW, USE_COMMODITY_OUTPUT_COL)),
            ("intermediate_plus_value_added", n(USE_INTERMEDIATE_ROW, USE_INTERMEDIATE_COL) + n(USE_VALUE_ADDED_ROW, USE_FINAL_TOTAL_COL) - n(USE_OUTPUT_ROW, USE_COMMODITY_OUTPUT_COL)),
        ])
        add("make_use_industry_output", "industry", [
            (key, n(r, MAKE_OUTPUT_COL) - n(USE_OUTPUT_ROW, c))
            for key, r, c in zip(make_industries, MAKE_INDUSTRY_ROWS, USE_INDUSTRY_COLS)
        ])
        add("make_use_commodity_output", "commodity", [
            (key, n(MAKE_TOTAL_ROW, c) - n(USE_COMMODITY_ROWS[use_commodities.index(key)], USE_COMMODITY_OUTPUT_COL))
            for key, c in zip(make_commodities, MAKE_COMMODITY_COLS)
        ])
        add("make_use_grand_output", "national", [
            ("T008", n(MAKE_TOTAL_ROW, MAKE_OUTPUT_COL) - n(USE_OUTPUT_ROW, USE_COMMODITY_OUTPUT_COL))
        ])

        return {
            "source": {
                "file": str(SOURCE.relative_to(ROOT)),
                "sha256": digest,
                "byte_size": len(raw),
                "capture_timestamp_utc": provenance["archive_capture_timestamp_utc"],
                "point_in_time_status_for_2007_01_01": provenance["point_in_time_status_for_2007_01_01"],
                "workbook_sheets": names,
                "audited_sheet": "2005",
                "classification": "1997 NAICS annual industry/commodity categories",
                "unit": "millions of current U.S. dollars at producers' prices",
            },
            "grain": {
                "make_industries": 65, "make_commodities": 67,
                "use_commodities": 69, "use_industries": 65,
                "make_matrix_cells": 4355, "use_matrix_cells": 4485,
                "missing_or_nonnumeric_matrix_cells": 0,
                "duplicate_industry_or_commodity_keys": 0,
                "industry_keys": make_industries,
                "make_commodity_keys": make_commodities,
                "use_commodity_keys": use_commodities,
                "final_use_keys": final_codes,
                "use_only_adjustment_codes": ["S001", "S003"],
                "use_only_adjustment_output_million": {
                    key: decimal_text(n(USE_COMMODITY_ROWS[use_commodities.index(key)], USE_COMMODITY_OUTPUT_COL))
                    for key in ("S001", "S003")
                },
            },
            "published_totals_million": {
                "gross_output": decimal_text(n(MAKE_TOTAL_ROW, MAKE_OUTPUT_COL)),
                "intermediate_inputs": decimal_text(n(USE_INTERMEDIATE_ROW, USE_INTERMEDIATE_COL)),
                "value_added_gdp": decimal_text(n(USE_VALUE_ADDED_ROW, USE_FINAL_TOTAL_COL)),
            },
            "residual_sign_rule": "sum of displayed parts minus displayed total; cross-table is Make minus Use",
            "residuals": groups,
            "interpretation": "Displayed detail has nonzero rounding residuals; no balancing adjustment was applied. Exact bytes are not certified available on 2007-01-01.",
        }
    finally:
        workbook.release_resources()


def main() -> None:
    report = build_report()
    ARTIFACT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {ARTIFACT}")
    print(f"SHA-256 {report['source']['sha256']}")
    print(f"nonzero residual groups {sum(group['nonzero_count'] > 0 for group in report['residuals'].values())}")


if __name__ == "__main__":
    main()
