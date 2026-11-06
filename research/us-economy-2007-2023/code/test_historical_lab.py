import json
import os
import subprocess
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from collect_bank_opening import load_bank_opening, z1_anchors
from collect_labor_vintages import parse_series
from collect_production_opening import load_vintage as load_production
from historical_lab import BINARY, digest, engine_command, fit, prepare
from serve_historical_lab import scenario_with_events


class VintageTests(unittest.TestCase):
    def test_reject_wrong_edition_and_lookahead(self):
        with self.assertRaisesRegex(ValueError, "unexpected vintage"):
            parse_series(b"observation_date,UNRATE_20240202\n", "UNRATE", "2020-01-10", "2019-12-01")
        with self.assertRaisesRegex(ValueError, "invalid period"):
            parse_series(b"observation_date,UNRATE_20200110\n2020-01-01,3.6\n", "UNRATE", "2020-01-10", "2019-12-01")

    def test_fitted_rows_are_probabilities_and_opening_matches_vintage(self):
        labor, history, start = fit("2020-01-10")
        self.assertEqual(len(history), 60)
        self.assertEqual(start, "2015-01-01")
        self.assertEqual(labor["opening_unemployment"], .035)
        self.assertTrue(all(abs(sum(row) - 1) < 1e-12 for row in labor["transitions"]))


    def test_late_z1_archives_stay_out_of_opening_state(self):
        with self.assertRaisesRegex(ValueError, "postdates opening"):
            z1_anchors("2016-01-08", "20151210")
        with self.assertRaisesRegex(ValueError, "postdates opening"):
            z1_anchors("2018-01-05", "20171207")
        self.assertIsNone(load_bank_opening("2018-01-05"))


class LiveTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = Path(os.environ.get("OWG_LAB_TEST_BINARY", BINARY)).resolve()

    def test_live_frames_reconcile_and_replay(self):
        with TemporaryDirectory() as tmp:
            folder = Path(tmp)
            prepare(self.binary, folder)
            origin = folder / "2020"
            output = subprocess.run(engine_command(self.binary, origin, 2, step_input=True),
                                    input="step\nstep\n", check=True, capture_output=True, text=True)
            header, first, second = map(json.loads, output.stdout.splitlines())
            self.assertEqual(header["labor_sha256"], digest(origin / "labor.json"))
            self.assertEqual(header["months"], 36)
            self.assertEqual(second["metrics"]["month_of_year"], 2)
            for frame in (first, second):
                labor = frame["labor"]
                population = labor["population"]
                self.assertEqual(sum(population), 1000)
                self.assertEqual([sum(column) for column in zip(*labor["transitions"])], population)
                self.assertEqual(sum(sector["workers"] for sector in frame["sectors"]), population[0])
                self.assertLessEqual(labor["matches"], min(labor["applications"], labor["vacancies"]))
                self.assertAlmostEqual(frame["metrics"]["unemployment"], population[1] / sum(population[:2]))
                self.assertEqual(frame["metrics"]["ledger_residual_minor_units"], 0)
            # Historical truth is not read by the Rust simulation.
            (origin / "observed.json").write_text("poisoned outcomes")
            replay = subprocess.run(engine_command(self.binary, origin, 2, step_input=True),
                                    input="step\nstep\n", check=True, capture_output=True, text=True)
            self.assertEqual(replay.stdout, output.stdout)

    def test_recruiting_stock_reconciles_and_carries(self):
        with TemporaryDirectory() as tmp:
            folder = Path(tmp)
            prepare(self.binary, folder, recruiting=True)
            origin = folder / "2020"
            result = subprocess.run(engine_command(self.binary, origin, 0, step_input=True),
                                    input="step\nstep\nstep\n", check=True, capture_output=True, text=True)
            header, *frames = map(json.loads, result.stdout.splitlines())
            self.assertEqual(header["months"], 108)
            for before, after in zip(frames, frames[1:]):
                self.assertEqual(before["labor"]["recruiting"]["closing_stock"],
                                 after["labor"]["recruiting"]["opening_stock"])
            for frame in frames:
                labor = frame["labor"]
                stock = labor["recruiting"]
                self.assertEqual(stock["opening_stock"] + stock["new_postings"] - stock["withdrawn"] -
                                 labor["matches"], stock["closing_stock"])
                self.assertLessEqual(labor["matches"], labor["applications"])
                self.assertEqual(len(frame["banks"]), 2)
                self.assertEqual(frame["metrics"]["ledger_residual_minor_units"], 0)

    def test_inventory_trial_expires_services_and_deflates_imports(self):
        with TemporaryDirectory() as tmp:
            folder = Path(tmp)
            prepare(self.binary, folder, recruiting=True, finance=True, production=True, inventory=True)
            origin = folder / "2020"
            path = origin / "live-scenario.json"
            scenario = json.loads(path.read_text())
            scenario["shocks"] = [{
                "id": "test-import-prices", "start_month": 0, "duration_months": 1,
                "type": "import_price", "sector": "energy", "multiplier": 1.25,
            }]
            path.write_text(json.dumps(scenario))
            command = engine_command(self.binary, origin, 0, step_input=True, finance=True, inventory=True)
            result = subprocess.run(command, input="step\n", check=True, capture_output=True, text=True)
            header, frame = map(json.loads, result.stdout.splitlines())
            self.assertEqual(header["production_sha256"], digest(origin / "inventory.json"))
            flow = frame["production_flows"]
            metric = frame["metrics"]
            self.assertEqual(frame["sectors"][3]["inventory_value"], 0)
            self.assertGreater(flow["unused_service_slots_fixed_prices"], 0)
            self.assertAlmostEqual(flow["imported_inputs_fixed_prices"] * 1.25, metric["imports"], places=6)
            self.assertAlmostEqual(flow["gross_output_fixed_prices"] - flow["imported_inputs_fixed_prices"],
                                   metric["real_gdp"], places=6)
            self.assertLess(metric["accounting_identity_relative_error"], 1e-8)
            self.assertEqual(metric["ledger_residual_minor_units"], 0)

    def test_production_opening_reaches_agents_and_household_totals(self):
        with TemporaryDirectory() as tmp:
            folder = Path(tmp)
            prepare(self.binary, folder, recruiting=True, finance=True, production=True)
            origin = folder / "2020"
            def step(production):
                return subprocess.run(engine_command(self.binary, origin, 0, step_input=True,
                                                     finance=True, production=production),
                                      input="step\n", check=True, capture_output=True, text=True)
            header, frame = map(json.loads, step(True).stdout.splitlines())
            _, synthetic = map(json.loads, step(False).stdout.splitlines())
            self.assertEqual(header["production_sha256"], digest(origin / "production.json"))
            opening = header["opening_production"]["sectors"]
            source, _ = load_production("2020-01-10")
            self.assertEqual([s["consumption_share"] for s in opening], source["consumption_shares"])
            self.assertEqual(sum(s["workers"] for s in opening), 610)
            self.assertEqual(sum(s["firms"] for s in opening), 100)
            self.assertEqual([s["active_firms"] for s in frame["sectors"]], [s["firms"] for s in opening])
            self.assertNotEqual(frame["sectors"], synthetic["sectors"])
            self.assertAlmostEqual(sum(s["household_spending"] for s in frame["sectors"]),
                                   frame["metrics"]["consumption_total"], places=6)
            self.assertEqual(frame["metrics"]["ledger_residual_minor_units"], 0)
            path = origin / "production.json"
            invalid = json.loads(path.read_text())
            invalid["payroll_shares"][0] = 0.5
            path.write_text(json.dumps(invalid))
            with self.assertRaisesRegex(subprocess.CalledProcessError, "non-zero"):
                step(True)

    def test_finance_books_reconcile_and_enter_engine(self):
        with TemporaryDirectory() as tmp:
            folder = Path(tmp)
            prepare(self.binary, folder, recruiting=True, finance=True)
            origin = folder / "2020"

            def first_month(finance):
                command = engine_command(self.binary, origin, 0, step_input=True, finance=finance)
                return subprocess.run(command, input="step\n", check=True, capture_output=True, text=True)

            result = first_month(True)
            header, frame = map(json.loads, result.stdout.splitlines())
            synthetic_header, synthetic = map(json.loads, first_month(False).stdout.splitlines())
            self.assertIsNone(synthetic_header["opening_finance"])
            self.assertEqual(header["finance_sha256"], digest(origin / "finance.json"))
            books = header["opening_finance"]["banks"]
            self.assertEqual([b["cohort"] for b in books], ["large", "other_domestic"])
            for bank in books:
                assets = sum(bank[k] for k in ("reserves_minor", "household_loans_minor",
                                                "firm_loans_minor", "other_assets_minor"))
                claims = sum(bank[k] for k in ("deposits_minor", "other_liabilities_minor", "equity_minor"))
                self.assertEqual(assets, claims)
            self.assertGreater(frame["metrics"]["credit"], synthetic["metrics"]["credit"])
            self.assertEqual(frame["metrics"]["ledger_residual_minor_units"], 0)
            finance_path = origin / "finance.json"
            finance = json.loads(finance_path.read_text())
            finance["banks"][0]["reserve_per_deposit"] = 9
            finance_path.write_text(json.dumps(finance))
            with self.assertRaisesRegex(subprocess.CalledProcessError, "non-zero"):
                first_month(True)

    def test_presets_use_calendar_offsets_and_clip_the_window(self):
        template = {"calendar": {"start_year": 2020, "months": 108}}
        scenario, events = scenario_with_events(template, 2020, 2028, "covid-ai")
        self.assertEqual([event["month"] for event in events], [3, 72])
        self.assertEqual([shock["duration_months"] for shock in scenario["shocks"][-2:]], [36, 36])
        self.assertEqual(template["calendar"]["months"], 108)
        short, events = scenario_with_events(template, 2020, 2020, "covid-ai")
        self.assertEqual([event["month"] for event in events], [3])
        self.assertTrue(all(s["start_month"] + s["duration_months"] + s["decay_months"] == 12
                            for s in short["shocks"]))

    def test_no_vacancy_means_no_hire(self):
        with TemporaryDirectory() as tmp:
            folder = Path(tmp)
            prepare(self.binary, folder)
            origin = folder / "2020"
            scenario_path = origin / "live-scenario.json"
            scenario = json.loads(scenario_path.read_text())
            scenario["population"]["firms"] = 4
            scenario_path.write_text(json.dumps(scenario))
            result = subprocess.run(engine_command(self.binary, origin, 0, step_input=True),
                                    input="step\n", check=True, capture_output=True, text=True)
            frame = json.loads(result.stdout.splitlines()[1])
            self.assertEqual(frame["labor"]["vacancies"], 0)
            self.assertGreater(frame["labor"]["applications"], 0)
            self.assertEqual(frame["labor"]["matches"], 0)


if __name__ == "__main__":
    unittest.main()
