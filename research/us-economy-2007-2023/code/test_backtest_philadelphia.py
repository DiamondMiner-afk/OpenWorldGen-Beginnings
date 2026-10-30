"""Adversarial tests for the real-time macro benchmark."""

from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

from backtest_philadelphia import (
    MIN_AR_TRAIN,
    SERIES,
    VINTAGE_ROOT,
    Vintage,
    conformal_radius,
    evaluate_forecasts,
    forecast_origin,
    load_vintage,
    origin_schedule,
    period_index,
    run_backtest,
)


class PhiladelphiaBacktestTests(unittest.TestCase):
    def test_full_window_and_actual_observation_horizons(self) -> None:
        rows, fingerprint = run_backtest()
        self.assertEqual(len(fingerprint), 64)
        persistence = [row for row in rows if row["model"] == "persistence"]
        ar = [row for row in rows if row["model"] == "expanding_ar1"]
        self.assertEqual(len(persistence), 68 * 3)
        self.assertEqual(len(ar), (68 - MIN_AR_TRAIN) * 3)
        self.assertEqual({row["target_vintage"] for row in persistence}, set(origin_schedule()[1:]))
        self.assertEqual({row["series"] for row in persistence}, set(SERIES))
        for row in rows:
            observed_horizon = period_index(row["target_period"]) - period_index(row["feature_period"])
            self.assertEqual(observed_horizon, row["horizon_observation_months"])
            self.assertIn(observed_horizon, (2, 3, 4) if row["series"] == "CPI" else (3,))
            self.assertLess(row["origin_as_of"], row["target_as_of"])
            self.assertEqual(row["error"], row["prediction"] - row["actual_next_vintage"])

    def test_future_label_does_not_change_pending_predictions(self) -> None:
        current = load_vintage(VINTAGE_ROOT, "2006Q4")
        target = load_vintage(VINTAGE_ROOT, "2007Q1")
        history = {series: [] for series in SERIES}
        pending = forecast_origin(current, target.origin, history)
        altered = Vintage(
            origin=target.origin,
            as_of=target.as_of,
            observations={
                series: rows[:-1] + ((rows[-1][0], rows[-1][1] * 1.1),)
                for series, rows in target.observations.items()
            },
            manifest_hash="altered-target-test",
        )
        original_scores, _ = evaluate_forecasts(pending, target)
        altered_scores, _ = evaluate_forecasts(pending, altered)
        self.assertEqual([r["prediction"] for r in original_scores], [r["prediction"] for r in altered_scores])
        self.assertNotEqual([r["actual_next_vintage"] for r in original_scores], [r["actual_next_vintage"] for r in altered_scores])

    def test_not_yet_released_training_target_rejected(self) -> None:
        current = load_vintage(VINTAGE_ROOT, "2006Q4")
        history = {series: [] for series in SERIES}
        history["RUC"].append({
            "feature": 4.4,
            "actual": 4.6,
            "target_as_of": datetime(2007, 2, 15, tzinfo=timezone.utc),
            "errors": {"persistence": -0.2},
        })
        with self.assertRaisesRegex(ValueError, "not yet released"):
            forecast_origin(current, "2007Q1", history)

    def test_modified_source_rejected_before_scoring(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copytree(VINTAGE_ROOT / "2006Q4", root / "2006Q4")
            source = root / "2006Q4" / "ruc.csv"
            with source.open("a", encoding="utf-8") as stream:
                stream.write("2006:11,4.3\n")
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                load_vintage(root, "2006Q4")

    def test_future_publication_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copytree(VINTAGE_ROOT / "2006Q4", root / "2006Q4")
            manifest_path = root / "2006Q4" / "data-vintage.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["sources"][0]["published_at"] = "2007-01-01T00:00:00Z"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "future publication"):
                load_vintage(root, "2006Q4")

    def test_interval_uses_past_residuals_only(self) -> None:
        self.assertIsNone(conformal_radius([1.0] * 11))
        self.assertEqual(conformal_radius(list(range(1, 13))), 11.0)


if __name__ == "__main__":
    unittest.main()
