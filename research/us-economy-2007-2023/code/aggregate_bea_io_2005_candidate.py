"""Reviewable 2005 BEA Make/Use to four-sector *candidate* aggregation.

This preserves signed accounting adjustments outside the ordinary 65-code
commodity/industry matrices. It is not a US opening state or a domestic-firm
supplier network: ordinary commodity use includes unresolved import content.
Requires the original XLS and pinned xlrd==2.0.2; see requirements-bea-io.txt.
"""

from __future__ import annotations

import csv
import hashlib
import json
from collections import Counter
from decimal import Decimal
from pathlib import Path

import xlrd

from audit_bea_io_2005 import (
    ARTIFACT as SOURCE_QC,
    MAKE_COMMODITY_COLS,
    MAKE_INDUSTRY_ROWS,
    MAKE_OUTPUT_COL,
    MAKE_TOTAL_ROW,
    ROOT,
    SOURCE,
    USE_COMMODITY_OUTPUT_COL,
    USE_COMMODITY_ROWS,
    USE_FINAL_COLS,
    USE_FINAL_TOTAL_COL,
    USE_INDUSTRY_COLS,
    USE_INTERMEDIATE_COL,
    USE_INTERMEDIATE_ROW,
    USE_OUTPUT_ROW,
    build_report,
    decimal_text,
    label,
    number,
)

SECTORS = ("energy", "food", "manufacturing", "services")
MAPPING_VERSION = "bea-io-2005-four-sector-candidate-v1"
MAPPING = ROOT / "data/mappings/bea-io-2005-four-sector-candidate-v1.csv"
OUTPUT = ROOT / "artifacts/bea_io_2005_four_sector_candidate_v1.json"
MAPPING_FIELDS = ["code", "sector", "confidence", "reason"]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_mapping(path: Path, expected_codes: list[str]) -> dict[str, dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as stream:
        reader = csv.DictReader(stream)
        if reader.fieldnames != MAPPING_FIELDS:
            raise ValueError("mapping CSV columns do not match the versioned contract")
        rows = list(reader)
    codes = [row["code"] for row in rows]
    if codes != expected_codes:
        raise ValueError("mapping must contain all 65 ordinary codes once in workbook order")
    for row in rows:
        if (row["sector"] not in SECTORS
            or row["confidence"] not in {"high", "medium", "low"}
            or not row["reason"].strip()
            or any(not value or value != value.strip() for value in row.values())):
            raise ValueError(f"invalid mapping row for {row['code']}")
    return {row["code"]: row for row in rows}


def empty_matrix() -> dict[str, dict[str, Decimal]]:
    return {row: {col: Decimal(0) for col in SECTORS} for row in SECTORS}


def matrix_as_text(matrix: dict[str, dict[str, Decimal]]) -> dict[str, dict[str, str]]:
    return {
        row: {col: decimal_text(matrix[row][col]) for col in SECTORS}
        for row in SECTORS
    }


def matrix_total(matrix: dict[str, dict[str, Decimal]]) -> Decimal:
    return sum((value for row in matrix.values() for value in row.values()), Decimal(0))


def build_candidate(mapping_path: Path = MAPPING) -> dict:
    # The source audit checks the exact workbook hash, provenance, layout,
    # completeness, keys, labels, units, and all published margins first.
    audit = build_report()
    if audit != json.loads(SOURCE_QC.read_text(encoding="utf-8")):
        raise ValueError("saved BEA source QC no longer matches the archived XLS")
    industry_codes = audit["grain"]["industry_keys"]
    mapping = load_mapping(mapping_path, industry_codes)
    if len(industry_codes) != 65:
        raise ValueError("unexpected BEA industry dimension")
    workbook = xlrd.open_workbook(SOURCE, on_demand=True)
    try:
        sheet = workbook.sheet_by_name("2005")
        n = lambda row, col: number(sheet, row, col)
        mapping_assignments = [
            {
                "code": code,
                "source_label": label(sheet.cell_value(MAKE_INDUSTRY_ROWS[index], 1)),
                "sector": mapping[code]["sector"],
                "confidence": mapping[code]["confidence"],
                "reason": mapping[code]["reason"],
            }
            for index, code in enumerate(industry_codes)
        ]
        make = empty_matrix()  # producing industry sector -> commodity sector
        use = empty_matrix()   # used commodity sector -> consuming industry sector
        industry_output = {sector: Decimal(0) for sector in SECTORS}
        ordinary_make_cells = ordinary_use_cells = 0
        raw_use_values: list[list[Decimal]] = []
        for index, code in enumerate(industry_codes):
            sector = mapping[code]["sector"]
            industry_output[sector] += n(MAKE_INDUSTRY_ROWS[index], MAKE_OUTPUT_COL)
            raw_use_row: list[Decimal] = []
            for other_index, other_code in enumerate(industry_codes):
                other_sector = mapping[other_code]["sector"]
                make_value = n(MAKE_INDUSTRY_ROWS[index], MAKE_COMMODITY_COLS[other_index])
                use_value = n(USE_COMMODITY_ROWS[index], USE_INDUSTRY_COLS[other_index])
                if make_value < 0 or use_value < 0:
                    raise ValueError("negative ordinary matrix cell cannot become supplier use")
                make[sector][other_sector] += make_value
                use[sector][other_sector] += use_value
                raw_use_row.append(use_value)
                ordinary_make_cells += 1
                ordinary_use_cells += 1
            raw_use_values.append(raw_use_row)
        if (ordinary_make_cells, ordinary_use_cells) != (4225, 4225):
            raise ValueError("ordinary Make/Use cell coverage is incomplete")

        make_special: dict[str, dict[str, str]] = {}
        for index, code in enumerate(audit["grain"]["make_commodity_keys"][65:], 65):
            col = MAKE_COMMODITY_COLS[index]
            make_special[code] = {
                "displayed_commodity_output_million": decimal_text(n(MAKE_TOTAL_ROW, col)),
                "sum_industry_detail_million": decimal_text(
                    sum((n(row, col) for row in MAKE_INDUSTRY_ROWS), Decimal(0))
                ),
            }
        use_special: dict[str, dict[str, str]] = {}
        for index, code in enumerate(audit["grain"]["use_commodity_keys"][65:], 65):
            row = USE_COMMODITY_ROWS[index]
            use_special[code] = {
                "displayed_intermediate_million": decimal_text(n(row, USE_INTERMEDIATE_COL)),
                "displayed_final_million": decimal_text(n(row, USE_FINAL_TOTAL_COL)),
                "displayed_output_million": decimal_text(n(row, USE_COMMODITY_OUTPUT_COL)),
            }
        final_codes = audit["grain"]["final_use_keys"]
        import_col = USE_FINAL_COLS[final_codes.index("F050")]
        import_total = n(USE_OUTPUT_ROW, import_col)
        if import_total >= 0:
            raise ValueError("F050 imports must remain a signed final-use adjustment")

        ordinary_make_total = matrix_total(make)
        ordinary_use_total = matrix_total(use)
        special_make_detail = sum(
            (n(row, col) for row in MAKE_INDUSTRY_ROWS
             for col in MAKE_COMMODITY_COLS[65:]), Decimal(0)
        )
        special_use_detail = sum(
            (n(row, col) for row in USE_COMMODITY_ROWS[65:]
             for col in USE_INDUSTRY_COLS), Decimal(0)
        )
        published_make = n(MAKE_TOTAL_ROW, MAKE_OUTPUT_COL)
        published_use_intermediate = n(USE_INTERMEDIATE_ROW, USE_INTERMEDIATE_COL)
        confidence_counts = Counter(row["confidence"] for row in mapping.values())
        # This is a local structural sensitivity screen, not a probability
        # distribution or a simultaneous worst-case reassignment.
        reassignments = []
        for moved_index, code in enumerate(industry_codes):
            if mapping[code]["confidence"] != "low":
                continue
            original_sector = mapping[code]["sector"]
            for alternative in SECTORS:
                if alternative == original_sector:
                    continue
                alternative_sectors = [mapping[item]["sector"] for item in industry_codes]
                alternative_sectors[moved_index] = alternative
                alternative_use = empty_matrix()
                for commodity_index, commodity_sector in enumerate(alternative_sectors):
                    for industry_index, industry_sector in enumerate(alternative_sectors):
                        alternative_use[commodity_sector][industry_sector] += (
                            raw_use_values[commodity_index][industry_index]
                        )
                max_use_cell_delta = max(
                    abs(alternative_use[row][col] - use[row][col])
                    for row in SECTORS for col in SECTORS
                )
                reassignments.append({
                    "code": code,
                    "from": original_sector,
                    "to": alternative,
                    "reassigned_published_output_million": decimal_text(
                        n(MAKE_INDUSTRY_ROWS[moved_index], MAKE_OUTPUT_COL)
                    ),
                    "max_abs_ordinary_use_cell_delta_million": decimal_text(max_use_cell_delta),
                })
        reassignments.sort(
            key=lambda item: (
                -Decimal(item["max_abs_ordinary_use_cell_delta_million"]),
                item["code"], item["to"],
            )
        )
        return {
            "status": "candidate_aggregation_not_calibrated_or_origin_certified",
            "mapping_version": MAPPING_VERSION,
            "provenance": {
                "source_sha256": sha256(SOURCE),
                "source_qc_sha256": sha256(SOURCE_QC),
                "mapping_file": str(mapping_path.relative_to(ROOT)),
                "mapping_sha256": sha256(mapping_path),
                "observation_year": 2005,
                "exact_bytes_available_to_2007_01_01_actor": False,
                "unit": "millions of current U.S. dollars at producers' prices",
            },
            "coverage": {
                "ordinary_industries_mapped": len(mapping),
                "ordinary_commodities_mapped": len(industry_codes),
                "ordinary_make_cells": ordinary_make_cells,
                "ordinary_use_cells": ordinary_use_cells,
                "make_special_codes_separate": list(make_special),
                "use_special_codes_separate": list(use_special),
                "signed_import_final_category_separate": "F050",
                "confidence_counts": dict(sorted(confidence_counts.items())),
                "low_confidence_codes": [code for code, row in mapping.items()
                                         if row["confidence"] == "low"],
            },
            "mapping_assignments": mapping_assignments,
            "one_at_a_time_low_confidence_sensitivity": {
                "method": "move one low-confidence code to each other sector; not joint uncertainty",
                "alternative_count": len(reassignments),
                "largest_use_cell_change_million": (
                    reassignments[0]["max_abs_ordinary_use_cell_delta_million"]
                    if reassignments else "0"
                ),
                "alternatives_descending_use_cell_change": reassignments,
            },
            "ordinary_make_industry_by_commodity_million": matrix_as_text(make),
            "ordinary_use_commodity_by_industry_million": matrix_as_text(use),
            "published_industry_output_by_candidate_sector_million": {
                sector: decimal_text(industry_output[sector]) for sector in SECTORS
            },
            "separate_adjustments": {
                "make_special_commodities": make_special,
                "use_special_commodities": use_special,
                "F050_imports_final_use_million": decimal_text(import_total),
            },
            "reconciliation_million": {
                "ordinary_make_detail": decimal_text(ordinary_make_total),
                "special_make_detail": decimal_text(special_make_detail),
                "all_make_detail_minus_published_gross_output": decimal_text(
                    ordinary_make_total + special_make_detail - published_make
                ),
                "ordinary_use_intermediate_detail": decimal_text(ordinary_use_total),
                "special_use_intermediate_detail": decimal_text(special_use_detail),
                "all_use_intermediate_detail_minus_published_total": decimal_text(
                    ordinary_use_total + special_use_detail - published_use_intermediate
                ),
                "published_gross_output": decimal_text(published_make),
                "published_intermediate_inputs": decimal_text(published_use_intermediate),
                "candidate_sector_output_sum_minus_published_gross_output": decimal_text(
                    sum(industry_output.values(), Decimal(0)) - published_make
                ),
            },
            "interpretation": (
                "Four-sector displayed-value summary only. Signed imports and special "
                "commodity adjustments are separate; ordinary Use is not a domestic-only "
                "supplier network. Low-confidence industry assignments require sensitivity "
                "tests and institutional splits before simulation calibration."
            ),
        }
    finally:
        workbook.release_resources()


def main() -> None:
    candidate = build_candidate()
    OUTPUT.write_text(json.dumps(candidate, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {OUTPUT}")
    print(f"ordinary Make/Use cells {candidate['coverage']['ordinary_make_cells']}/"
          f"{candidate['coverage']['ordinary_use_cells']}")


if __name__ == "__main__":
    main()
