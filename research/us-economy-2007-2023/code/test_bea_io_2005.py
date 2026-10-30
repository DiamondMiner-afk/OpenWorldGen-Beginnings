"""Regression and hostile-input checks for the bounded 2005 BEA XLS audit."""

from __future__ import annotations

import json
import unittest
from decimal import Decimal

import xlrd

from audit_bea_io_2005 import (
    ARTIFACT,
    EXPECTED_SHA256,
    PROVENANCE,
    SOURCE,
    build_report,
    code,
    number,
    residual_group,
    unique_keys,
    verify_source,
)


class BeaIo2005AuditTests(unittest.TestCase):
    def test_real_xls_and_saved_report(self) -> None:
        report = build_report()
        self.assertEqual(report, json.loads(ARTIFACT.read_text(encoding="utf-8")))
        self.assertEqual(report["source"]["sha256"], EXPECTED_SHA256)
        self.assertEqual(report["source"]["audited_sheet"], "2005")
        self.assertEqual(report["grain"]["make_matrix_cells"], 4355)
        self.assertEqual(report["grain"]["use_matrix_cells"], 4485)
        self.assertEqual(report["grain"]["missing_or_nonnumeric_matrix_cells"], 0)
        self.assertEqual(report["grain"]["duplicate_industry_or_commodity_keys"], 0)
        self.assertEqual(report["grain"]["industry_keys"][:3], ["111CA", "113FF", "211"])
        self.assertEqual(report["grain"]["make_commodity_keys"][-2:], ["S002", "S004"])
        self.assertEqual(report["grain"]["use_commodity_keys"][-4:], ["S001", "S002", "S003", "S004"])
        self.assertEqual(report["published_totals_million"]["gross_output"], "22857142.6")
        self.assertEqual(report["residuals"]["use_commodity_intermediate_rows"]["max_abs_million"], "0.7")
        self.assertEqual(report["residuals"]["make_grand_from_industries"]["nonzero"][0]["residual_million"], "0.4")
        for name in (
            "make_use_industry_output",
            "make_use_commodity_output",
            "make_use_grand_output",
        ):
            self.assertEqual(report["residuals"][name]["nonzero_count"], 0)
        self.assertEqual(
            report["source"]["point_in_time_status_for_2007_01_01"],
            "release_available_exact_bytes_not_certified",
        )

    def test_hash_and_provenance_tampering_fail(self) -> None:
        raw = SOURCE.read_bytes()
        provenance = json.loads(PROVENANCE.read_text(encoding="utf-8"))
        self.assertEqual(verify_source(raw, provenance), EXPECTED_SHA256)
        changed = bytearray(raw)
        changed[-1] ^= 1
        with self.assertRaisesRegex(ValueError, "hash/size mismatch"):
            verify_source(bytes(changed), provenance)
        changed_provenance = {**provenance, "point_in_time_status_for_2007_01_01": "certified"}
        with self.assertRaisesRegex(ValueError, "provenance mismatch"):
            verify_source(raw, changed_provenance)

    def test_excel_numeric_codes_and_duplicate_grain(self) -> None:
        self.assertEqual(code(211.0), "211")
        self.assertEqual(code(" 311FT "), "311FT")
        with self.assertRaisesRegex(ValueError, "non-integral"):
            code(211.5)
        with self.assertRaisesRegex(ValueError, "duplicate"):
            unique_keys(["111CA", "211", "111CA"], "industry")

    def test_missing_numeric_cell_is_rejected(self) -> None:
        book = xlrd.open_workbook(SOURCE, on_demand=True)
        try:
            with self.assertRaisesRegex(ValueError, "missing or nonnumeric"):
                number(book.sheet_by_name("2005"), 0, 0)
        finally:
            book.release_resources()

    def test_signed_residuals_are_retained(self) -> None:
        group = residual_group("commodity", [
            ("A", Decimal("0.2")),
            ("B", Decimal("-0.3")),
            ("C", Decimal("0.0")),
        ])
        self.assertEqual(group["nonzero_count"], 2)
        self.assertEqual(group["max_abs_million"], "0.3")
        self.assertEqual(group["sum_abs_million"], "0.5")
        self.assertEqual(group["nonzero"], [
            {"key": "A", "residual_million": "0.2"},
            {"key": "B", "residual_million": "-0.3"},
        ])


if __name__ == "__main__":
    unittest.main()
