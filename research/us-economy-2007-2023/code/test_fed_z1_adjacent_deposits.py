"""Adversarial checks for the bounded adjacent Z.1 deposit-series probe."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from probe_fed_z1_adjacent_deposits import (
    ADJACENT_RAW,
    build_report,
    inspect_adjacent,
    printed_anchor,
    release_passes_not_before_screen,
    source_passes_not_before_screen,
)


class FedZ1AdjacentDepositsTests(unittest.TestCase):
    def test_official_file_integrity_and_coverage(self) -> None:
        inspected = inspect_adjacent()
        files = {item["filename"]: item for item in inspected["files"]}
        self.assertEqual(len(files), 4)
        self.assertGreater(files["2007-06-07_ltabs.zip"]["member_count"], 60)
        self.assertGreater(files["2023-06-08_z1_csv_files.zip"]["member_count"], 500)

    def test_adjacent_revision_values_and_pdf_anchors(self) -> None:
        report = build_report()
        self.assertEqual([item["code"] for item in report["series"]], [
            "FL153020005.Q", "FL153030005.Q",
        ])
        self.assertEqual(report["releases"]["2007-06-07"]["rows"], 221)
        self.assertEqual(report["releases"]["2023-06-08"]["rows"], 310)
        checkable, savings = report["series"]
        self.assertEqual(checkable["same_quarter_difference_june_minus_march_2007_millions"], -79099)
        self.assertEqual(checkable["same_quarter_difference_june_minus_march_2023_millions"], -443632)
        self.assertEqual(savings["same_quarter_difference_june_minus_march_2007_millions"], -147113)
        self.assertEqual(savings["same_quarter_difference_june_minus_march_2023_millions"], -97157)
        self.assertEqual(checkable["latest_quarter_pdf_anchor_billions"]["2007-06-07"], "92.3")
        self.assertEqual(checkable["june_prior_quarter_pdf_anchor_billions"]["2007-06-07"], "109.3")
        self.assertEqual(checkable["june_prior_quarter_pdf_anchor_billions"]["2023-06-08"], "4507.8")
        self.assertEqual(savings["latest_quarter_pdf_anchor_billions"]["2023-06-08"], "9885.1")

    def test_release_day_fails_not_before_screen_for_surviving_bytes(self) -> None:
        files = inspect_adjacent()["files"]
        self.assertFalse(release_passes_not_before_screen(files, "2007-06-07", "2007-06-07T17:00:00Z"))
        self.assertFalse(release_passes_not_before_screen(files, "2007-06-07", "2008-01-29T23:59:59Z"))
        self.assertTrue(release_passes_not_before_screen(files, "2007-06-07", "2008-01-30T00:00:00Z"))
        self.assertFalse(release_passes_not_before_screen(files, "2023-06-08", "2023-06-08T16:01:00Z"))
        self.assertTrue(release_passes_not_before_screen(files, "2023-06-08", "2023-06-09T00:00:00Z"))

    def test_actor_time_requires_timezone(self) -> None:
        entry = inspect_adjacent()["files"][0]
        with self.assertRaisesRegex(ValueError, "UTC offset"):
            source_passes_not_before_screen(entry, "2008-02-01T12:00:00")

    def test_tampered_manifest_or_source_is_rejected(self) -> None:
        original = json.loads((ADJACENT_RAW / "source-provenance.json").read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)
            for item in original["files"]:
                (path / item["filename"]).symlink_to(ADJACENT_RAW / item["filename"])
            original["files"][0]["sha256"] = "0" * 64
            (path / "source-provenance.json").write_text(json.dumps(original), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "size/hash mismatch"):
                inspect_adjacent(path)

    def test_pdf_table_definition_cannot_silently_change(self) -> None:
        fake = (
            "L.100 Households and Nonprofit Organizations (1)\n"
            "4 Checkable deposits and currency 92.3\n"
            "L.101 Nonfinancial Business (1)\n"
        )
        with self.assertRaisesRegex(ValueError, "unit definition absent"):
            printed_anchor(fake, era="legacy", code="FL153020005.Q")


if __name__ == "__main__":
    unittest.main()
