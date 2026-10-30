import unittest

from run_mvp import actor_spec, fixture


class MvpConfigurationTests(unittest.TestCase):
    def test_shock_decay_stays_inside_every_supported_small_horizon(self):
        for months in [6, 7, 12, 24, 120, 1200]:
            spec = fixture({"calendar": {}, "population": {}}, months)
            shock = spec["shocks"][0]
            self.assertLessEqual(shock["start_month"] + shock["duration_months"] + shock["decay_months"], months)
            for contract in spec["credit_market"]["contracts"]:
                self.assertLess(contract["origination_month"], months)
                self.assertGreater(contract["first_payment_month"], contract["origination_month"])

    def test_actor_menu_cannot_invent_transfer_amount_or_exceed_cash(self):
        for liquidity in [-50, 0, 1, 100, 100_000_000]:
            observation = {"liquidity": liquidity}
            spec = actor_spec(observation, 0)
            choices = spec["steps"][0]["actor_contexts"]["FIRM-00000"]["choices"]
            self.assertEqual(choices[0]["action"], {"type": "no_op"})
            for choice in choices[1:]:
                self.assertGreater(choice["action"]["amount"], 0)
                self.assertLessEqual(choice["action"]["amount"], min(5000, liquidity // 20))

    def test_governance_and_credit_are_explicit_synthetic_inputs(self):
        spec = fixture({"calendar": {}, "population": {}}, 120)
        self.assertIn("not a US forecast", spec["title"])
        self.assertGreater(spec["governance"]["groups"][1]["weight"], spec["governance"]["groups"][0]["weight"])
        self.assertEqual(spec["credit_market"]["origination_law"]["law_id"], "credit-permission")


if __name__ == "__main__":
    unittest.main()
