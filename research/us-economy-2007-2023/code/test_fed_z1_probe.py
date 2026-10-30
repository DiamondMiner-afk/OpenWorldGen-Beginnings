"""Archive integrity and as-of boundary checks for the two Z.1 release probes."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from probe_fed_z1_archives import RAW, inspect


class FedZ1ProbeTests(unittest.TestCase):
    def test_archives_have_expected_historical_formats(self) -> None:
        report = inspect()
        files = {item["filename"]: item for item in report["files"]}
        self.assertEqual(len(files), 4)
        self.assertEqual(files["2007-03-08_ltabs.zip"]["member_count"], 69)
        self.assertEqual(files["2023-03-09_z1_csv_files.zip"]["member_count"], 582)
        self.assertEqual(files["2007-03-08_ltabs.zip"]["member_extensions"], {".prn": 69})
        self.assertGreater(files["2023-03-09_z1_csv_files.zip"]["member_extensions"][".csv"], 100)
        for item in files.values():
            self.assertEqual(len(item["sha256"]), 64)
            self.assertLess(item["conservative_available_not_before_utc"], "2026-01-01")

    def test_duplicate_provenance_entry_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            temp = Path(folder)
            provenance = json.loads((RAW / "provenance.json").read_text())
            provenance["files"].append(provenance["files"][0])
            (temp / "provenance.json").write_text(json.dumps(provenance))
            with self.assertRaisesRegex(ValueError, "missing or duplicate"):
                inspect(temp)


if __name__ == "__main__":
    unittest.main()
