"""Hostile parser and release-level checks for the bounded Z.1 bridge."""

from __future__ import annotations

import json
import unittest
from decimal import Decimal
from zipfile import ZipFile

from bridge_fed_z1_household_deposits import (
    ARTIFACT,
    NEW_DICTIONARY,
    NEW_ZIP,
    RAW,
    build_report,
    parse_csv,
    parse_prn,
    require_pdf_rounding,
    validate_dictionary,
)


OLD_HEADER = (
    '"DATES" X1 X2 X3 FL153020005.Q FL153030005.Q\n'
)
NEW_HEADER = (
    "date,x1,x2,FL153020005.Q,FL153030005.Q\n"
)


class FedZ1HouseholdDepositsBridgeTests(unittest.TestCase):
    def test_real_release_bridge_is_bounded_and_reproducible(self) -> None:
        report = build_report()
        self.assertEqual(report["rows_2007_table"], 220)
        self.assertEqual(report["rows_2023_table"], 309)
        self.assertEqual(report["matched_observation_quarter"], "2006:Q4")
        self.assertEqual(report["old_member"], "ltab100d.prn")
        self.assertEqual(report["new_member"], "csv/l101.csv")
        by_code = {item["code"]: item for item in report["series"]}
        self.assertEqual(set(by_code), {"FL153020005.Q", "FL153030005.Q"})
        self.assertEqual(
            by_code["FL153020005.Q"]["2006_q4_as_2007_release_millions"],
            188420,
        )
        self.assertEqual(
            by_code["FL153020005.Q"]["2006_q4_as_2023_release_millions"],
            317815,
        )
        self.assertEqual(
            by_code["FL153030005.Q"]["historical_revision_millions"],
            12615,
        )
        self.assertIn("not a calibrated", report["status"])
        self.assertIn("not authorized", report["point_in_time_rule"])
        self.assertEqual(
            json.loads(ARTIFACT.read_text(encoding="utf-8")), report
        )

    def test_prn_rejects_duplicate_quarter_and_swapped_series_line(self) -> None:
        row = '"200604" 0 0 0 188420 5302159\n'
        with self.assertRaisesRegex(ValueError, "duplicate.*quarter"):
            parse_prn(OLD_HEADER + row + row)
        bad_header = (
            '"DATES" X1 X2 X3 FL153030005.Q FL153020005.Q\n'
        )
        with self.assertRaisesRegex(ValueError, "code/PDF line mismatch"):
            parse_prn(bad_header + row)

    def test_prn_rejects_missing_interior_quarter(self) -> None:
        rows = (
            '"200602" 0 0 0 188420 5302159\n'
            '"200604" 0 0 0 188420 5302159\n'
        )
        with self.assertRaisesRegex(ValueError, "missing or unordered"):
            parse_prn(OLD_HEADER + rows)

    def test_csv_rejects_duplicate_quarter_and_wrong_grain(self) -> None:
        row = "2006:Q4,0,0,317815,5314774\n"
        with self.assertRaisesRegex(ValueError, "duplicate.*quarter"):
            parse_csv(NEW_HEADER + row + row)
        with self.assertRaisesRegex(ValueError, "row width mismatch"):
            parse_csv(NEW_HEADER + "2006:Q4,0,0,317815\n")

    def test_csv_rejects_missing_interior_quarter(self) -> None:
        rows = (
            "2006:Q2,0,0,317815,5314774\n"
            "2006:Q4,0,0,317815,5314774\n"
        )
        with self.assertRaisesRegex(ValueError, "missing or unordered"):
            parse_csv(NEW_HEADER + rows)

    def test_dictionary_rejects_unit_change(self) -> None:
        with ZipFile(RAW / NEW_ZIP) as archive:
            content = archive.read(NEW_DICTIONARY).decode("utf-8-sig")
        lines = content.splitlines(keepends=True)
        selected = next(
            index for index, line in enumerate(lines)
            if line.startswith("FL153020005.Q\t")
        )
        lines[selected] = lines[selected].replace(
            "Millions of dollars; not seasonally adjusted",
            "Billions of dollars; not seasonally adjusted",
        )
        changed = "".join(lines)
        with self.assertRaisesRegex(ValueError, "unit mismatch"):
            validate_dictionary(changed)

    def test_pdf_anchor_rejects_thousandfold_unit_mismatch(self) -> None:
        with self.assertRaisesRegex(ValueError, "unit or value mismatch"):
            require_pdf_rounding(188420, Decimal("188420.0"))
        with self.assertRaisesRegex(ValueError, "missing"):
            require_pdf_rounding(None, Decimal("188.4"))


if __name__ == "__main__":
    unittest.main()
