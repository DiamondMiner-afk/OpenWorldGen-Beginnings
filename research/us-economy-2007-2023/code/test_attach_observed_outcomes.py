import csv
from pathlib import Path
import tempfile
import unittest

from attach_observed_outcomes import ROOT, checked_rows, table_section


class ObservedOutcomeTests(unittest.TestCase):
    def setUp(self):
        self.source = ROOT / "artifacts/backtest_philadelphia_2007_2023.csv"
        with self.source.open(newline="", encoding="utf-8") as stream:
            self.rows = list(csv.DictReader(stream))

    def check_bad(self, rows):
        with tempfile.TemporaryDirectory(prefix="owg-observed-test-") as directory:
            path = Path(directory) / "bad.csv"
            with path.open("w", newline="", encoding="utf-8") as stream:
                writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
                writer.writeheader()
                writer.writerows(rows)
            with self.assertRaises(ValueError):
                checked_rows(path)

    def test_real_rows_reconcile_and_render_actual_next_to_prediction(self):
        rows, audit = checked_rows(self.source)
        self.assertEqual(audit["rows_checked"], 372)
        self.assertEqual(audit["horizon_observation_months"], [2, 3, 4])
        rendered = table_section(rows, self.source)
        self.assertIn("Predicted (%)", rendered)
        self.assertIn("Observed next vintage (%)", rendered)
        self.assertIn("not predictions from the agent economy or Qwen", rendered)
        self.assertIn("Unavailable (insufficient prior errors)", rendered)

    def test_duplicate_forecast_grain_is_rejected(self):
        self.check_bad([self.rows[0], self.rows[0]])

    def test_changed_actual_cannot_be_presented_as_an_observed_value(self):
        row = self.rows[0].copy()
        row["actual_next_vintage"] = str(float(row["actual_next_vintage"]) + 10)
        self.check_bad([row])

    def test_future_or_wrong_origin_date_is_rejected(self):
        row = self.rows[0].copy()
        row["origin_as_of"] = row["target_as_of"]
        self.check_bad([row])


if __name__ == "__main__":
    unittest.main()
