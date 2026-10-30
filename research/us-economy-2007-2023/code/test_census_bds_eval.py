"""Quality and no-lookahead guard tests for the Census BDS evaluation slice."""

from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from census_bds_eval import RAW, build, parse_file, parse_measure, verify_raw_sources


class CensusBdsEvaluationTests(unittest.TestCase):
    def test_full_release_and_selected_2007_2023_coverage(self) -> None:
        economy, economy_qc = parse_file(RAW / "bds2023.csv", sector_table=False)
        sectors, sector_qc = parse_file(RAW / "bds2023_sec.csv", sector_table=True)
        self.assertEqual(len(economy), 46)
        self.assertEqual(len(sectors), 46 * 19)
        self.assertEqual(economy_qc["selected_rows"], 17)
        self.assertEqual(sector_qc["selected_rows"], 17 * 19)
        self.assertEqual(verify_raw_sources(RAW)["release_date"], "2025-09-25")

    def test_suppression_is_not_silently_zero(self) -> None:
        self.assertEqual(parse_measure("D", "emp", "2007/11"), (None, "D"))
        self.assertEqual(parse_measure("", "emp", "2007/11"), (None, "blank"))
        with self.assertRaisesRegex(ValueError, "negative nonnegative"):
            parse_measure("-1", "emp", "2007/11")

    def test_mutated_raw_data_fails_hash_check(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            copy = Path(folder)
            for name in ("provenance.json", "bds2023.csv", "bds2023_sec.csv"):
                shutil.copy2(RAW / name, copy / name)
            with (copy / "bds2023.csv").open("a", encoding="utf-8") as stream:
                stream.write("\n")
            with self.assertRaisesRegex(ValueError, "size/hash mismatch"):
                verify_raw_sources(copy)

    def test_generated_slice_is_evaluation_only_and_balances_counts(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            temp = Path(folder)
            with patch("census_bds_eval.RESEARCH_ROOT", temp):
                audit = build(RAW, temp / "processed", temp / "artifacts")
            manifest = json.loads(
                (temp / "processed/evaluation-only-manifest.json").read_text()
            )
            self.assertEqual(manifest["role"], "retrospective_target_only")
            self.assertEqual(manifest["available_not_before"], "2025-09-25")
            self.assertEqual(audit["processed"]["economy"]["rows"], 17)
            self.assertEqual(audit["processed"]["sector"]["rows"], 323)
            self.assertEqual(audit["cross_table_2007_2023"]["emp"]["nonzero_differences"], 0)
            self.assertEqual(audit["cross_table_2007_2023"]["estabs"]["nonzero_differences"], 0)
            self.assertEqual(
                audit["economy"]["identities_2007_2023"][
                    "net_jobs_equals_creation_minus_destruction"
                ]["failures"],
                0,
            )


if __name__ == "__main__":
    unittest.main()
