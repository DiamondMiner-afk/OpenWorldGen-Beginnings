"""Small hostile checks for the first real-data/LLM boundary."""

import json
import shutil
import tempfile
import unittest
from pathlib import Path

from extract_philadelphia_vintage import ROOT, period_end
from local_macro_probe import snapshot, target_periods


class VintagePipelineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        original = ROOT / "data/processed/philadelphia-fed/2006Q4"
        for path in original.iterdir():
            shutil.copy2(path, self.folder / path.name)
        self.manifest_path = self.folder / "data-vintage.json"

    def test_2006_snapshot_and_target_dates(self):
        manifest, records = snapshot(self.manifest_path)
        self.assertEqual(manifest["as_of"], "2006-11-15T23:59:59Z")
        self.assertEqual(records["ROUTPUT"]["recent_observations"][-1]["observation_period"], "2006:Q3")
        self.assertEqual(target_periods("2006Q4"), {"ROUTPUT": "2007:Q1", "RUC": "2007:01", "CPI": "2007:01"})
        self.assertEqual(period_end("2008:02").isoformat(), "2008-02-29")

    def test_tampered_csv_rejected(self):
        with (self.folder / "ruc.csv").open("a", encoding="utf-8") as stream:
            stream.write("2007:01,0\n")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            snapshot(self.manifest_path)

    def test_future_release_rejected(self):
        manifest = json.loads(self.manifest_path.read_text())
        manifest["sources"][0]["published_at"] = "2007-01-01T00:00:00Z"
        self.manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "future source release"):
            snapshot(self.manifest_path)


if __name__ == "__main__":
    unittest.main()
