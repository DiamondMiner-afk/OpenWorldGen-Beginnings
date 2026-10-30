"""Falsifiers for the explicitly provisional four-sector BEA display bridge."""

from __future__ import annotations

import json
import tempfile
import unittest
from decimal import Decimal
from pathlib import Path

import xlrd

from aggregate_bea_io_2005_candidate import (
    MAPPING,
    OUTPUT,
    SECTORS,
    build_candidate,
    load_mapping,
)
from audit_bea_io_2005 import SOURCE


class FourSectorCandidateTests(unittest.TestCase):
    def test_candidate_matches_saved_artifact_and_preserves_raw_detail(self) -> None:
        result = build_candidate()
        self.assertEqual(result, json.loads(OUTPUT.read_text(encoding="utf-8")))
        self.assertEqual(result["coverage"]["ordinary_industries_mapped"], 65)
        self.assertEqual(result["coverage"]["ordinary_commodities_mapped"], 65)
        self.assertEqual(result["coverage"]["ordinary_make_cells"], 4225)
        self.assertEqual(result["coverage"]["ordinary_use_cells"], 4225)
        self.assertEqual(sum(result["coverage"]["confidence_counts"].values()), 65)
        self.assertEqual(len(result["coverage"]["low_confidence_codes"]), 14)
        self.assertEqual(len(result["mapping_assignments"]), 65)
        self.assertEqual(result["mapping_assignments"][0]["source_label"], "Farms")
        self.assertEqual(
            result["one_at_a_time_low_confidence_sensitivity"]["alternative_count"],
            42,
        )
        self.assertFalse(result["provenance"]["exact_bytes_available_to_2007_01_01_actor"])
        for matrix_key, total_key in (
            ("ordinary_make_industry_by_commodity_million", "ordinary_make_detail"),
            ("ordinary_use_commodity_by_industry_million", "ordinary_use_intermediate_detail"),
        ):
            matrix = result[matrix_key]
            self.assertEqual(list(matrix), sorted(SECTORS))
            self.assertEqual(
                sum((Decimal(value) for row in matrix.values() for value in row.values()), Decimal(0)),
                Decimal(result["reconciliation_million"][total_key]),
            )

    def test_signed_import_and_special_adjustments_never_enter_ordinary_matrix(self) -> None:
        result = build_candidate()
        adjustments = result["separate_adjustments"]
        self.assertEqual(Decimal(adjustments["F050_imports_final_use_million"]), Decimal("-1882500.6"))
        self.assertEqual(list(adjustments["make_special_commodities"]), ["S002", "S004"])
        self.assertEqual(list(adjustments["use_special_commodities"]), ["S001", "S002", "S003", "S004"])
        self.assertEqual(
            Decimal(adjustments["use_special_commodities"]["S001"]["displayed_intermediate_million"]),
            Decimal("135688.1"),
        )
        self.assertEqual(
            Decimal(adjustments["make_special_commodities"]["S004"]["displayed_commodity_output_million"]),
            Decimal("-51333.0"),
        )
        # The 65 ordinary Use rows have no negative cell; negative adjustments
        # cannot be hidden in a nonnegative 4x4 commodity-use matrix.
        for row in result["ordinary_use_commodity_by_industry_million"].values():
            self.assertTrue(all(Decimal(value) >= 0 for value in row.values()))
        self.assertEqual(
            Decimal(result["reconciliation_million"]["all_use_intermediate_detail_minus_published_total"]),
            Decimal("-1.2"),
        )

    def test_manual_energy_use_cell_sum_matches_aggregated_sector(self) -> None:
        result = build_candidate()
        workbook = xlrd.open_workbook(SOURCE, on_demand=True)
        try:
            sheet = workbook.sheet_by_name("2005")
            energy_indices = [2, 4, 5, 13]  # 211, 213, 22, 324 in mapping order.
            direct = sum(
                (Decimal(str(sheet.cell_value(82 + commodity, 2 + industry)))
                 for commodity in energy_indices for industry in energy_indices),
                Decimal(0),
            )
            self.assertEqual(
                direct,
                Decimal(result["ordinary_use_commodity_by_industry_million"]["energy"]["energy"]),
            )
        finally:
            workbook.release_resources()

    def test_off_diagonal_use_and_published_margin_preserve_source_detail(self) -> None:
        result = build_candidate()
        assignments = result["mapping_assignments"]
        energy_rows = [index for index, row in enumerate(assignments) if row["sector"] == "energy"]
        service_columns = [index for index, row in enumerate(assignments) if row["sector"] == "services"]
        workbook = xlrd.open_workbook(SOURCE, on_demand=True)
        try:
            sheet = workbook.sheet_by_name("2005")
            energy_to_services = sum(
                (Decimal(str(sheet.cell_value(82 + row, 2 + col)))
                 for row in energy_rows for col in service_columns),
                Decimal(0),
            )
            self.assertEqual(
                energy_to_services,
                Decimal(result["ordinary_use_commodity_by_industry_million"]["energy"]["services"]),
            )
            all_displayed_intermediate = sum(
                (Decimal(str(sheet.cell_value(row, col)))
                 for row in range(82, 151) for col in range(2, 67)),
                Decimal(0),
            )
            published_intermediate = Decimal(str(sheet.cell_value(151, 67)))
            self.assertEqual(all_displayed_intermediate - published_intermediate, Decimal("-1.2"))
        finally:
            workbook.release_resources()

    def test_mapping_rejects_missing_duplicate_and_unreviewed_classification(self) -> None:
        expected = [line.split(",", 1)[0] for line in MAPPING.read_text(encoding="utf-8").splitlines()[1:]]
        source = MAPPING.read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="owg-bea-map-test-") as name:
            candidate = Path(name) / "mapping.csv"
            candidate.write_text("\n".join(source.splitlines()[:-1]) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "all 65 ordinary codes"):
                load_mapping(candidate, expected)
            candidate.write_text(source.replace("212,manufacturing,low", "212,energy,unreviewed"), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "invalid mapping row"):
                load_mapping(candidate, expected)
            candidate.write_text(source.replace("212,manufacturing,low", "211,manufacturing,low"), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "all 65 ordinary codes"):
                load_mapping(candidate, expected)


if __name__ == "__main__":
    unittest.main()
