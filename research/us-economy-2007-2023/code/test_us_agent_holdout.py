import unittest

from backtest_philadelphia import VINTAGE_ROOT, indicator, load_vintage
from us_agent_holdout import TEST_ORIGINS, histories, proposal


class AgentHoldoutTests(unittest.TestCase):
    def test_frozen_window_and_inputs_have_only_origin_histories(self):
        self.assertEqual(len(TEST_ORIGINS), 15)
        self.assertEqual((TEST_ORIGINS[0], TEST_ORIGINS[-1]), ("2020Q1", "2023Q3"))
        source = load_vintage(VINTAGE_ROOT, "2020Q1")
        payload = histories(source)
        self.assertEqual(set(payload), {"ROUTPUT", "RUC", "CPI"})
        for series, record in payload.items():
            self.assertEqual(set(record), {"history", "latest"})
            self.assertEqual(len(record["history"]), 32)
            self.assertAlmostEqual(record["latest"], indicator(source, series)[1])
            self.assertAlmostEqual(record["history"][-1], record["latest"])

    def raw(self, content):
        return {"choices": [{"message": {"content": content}}]}

    def test_valid_numeric_forecasts_are_accepted(self):
        content = '{"ROUTPUT":{"prediction":2,"lower_80":-2,"upper_80":6},"RUC":{"prediction":4,"lower_80":3,"upper_80":6},"CPI":{"prediction":2,"lower_80":0,"upper_80":4}}'
        self.assertEqual(proposal(self.raw(content))["RUC"]["prediction"], 4)

    def test_missing_metric_and_invalid_interval_are_rejected(self):
        for content in ['{"ROUTPUT":{}}', '{"ROUTPUT":{"prediction":2,"lower_80":3,"upper_80":6},"RUC":{"prediction":4,"lower_80":3,"upper_80":6},"CPI":{"prediction":2,"lower_80":0,"upper_80":4}}']:
            with self.assertRaises(ValueError):
                proposal(self.raw(content))


if __name__ == "__main__":
    unittest.main()
