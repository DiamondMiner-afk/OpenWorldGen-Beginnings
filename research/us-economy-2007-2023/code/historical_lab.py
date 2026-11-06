"""Fit a labor module from dated CPS flows and score the Rust engine."""

import argparse
import csv
import hashlib
import json
import statistics
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from backtest_philadelphia import VINTAGE_ROOT as PHILLY_ROOT, load_vintage as load_macro
from collect_bank_opening import load_bank_opening, load_bank_scoring
from collect_firm_labor_vintages import load_vintage as load_jolts
from collect_household_behavior import load as load_household_behavior
from collect_inventory_adjustment import load as load_inventory
from collect_production_opening import load_vintage as load_production
from collect_labor_vintages import FLOWS, ROOT as SOURCE_ROOT, VINTAGES, load_vintage

RESEARCH = Path(__file__).resolve().parents[1]
REPO = RESEARCH.parents[1]
OUTPUT = RESEARCH / "artifacts/historical-economy-v5"
DEMAND_OUTPUT = RESEARCH / "artifacts/historical-economy-v6"
BINARY = REPO / "target/debug/owg"
EVALUATION = "2024-02-02"
TRAIN_MONTHS = 60
SEEDS = tuple(range(8))
HORIZON = 12
DEFAULT_END_YEAR = 2028


def save(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fit(vintage, *, recruiting=False):
    manifest_hash, data = load_vintage(SOURCE_ROOT, vintage)
    history = list(data["UNRATE"])[-TRAIN_MONTHS:]
    transitions = []
    for codes in FLOWS:
        counts = [sum(data[code][period] for period in history) for code in codes]
        transitions.append([value / sum(counts) for value in counts])
    opening = history[-1]
    unemployment = data["UNRATE"][opening] / 100
    participation = data["CIVPART"][opening] / 100
    input_data = {
        "schema_version": "owg.historical-labor.v1",
        "source_manifest_sha256": manifest_hash,
        "opening_unemployment": unemployment,
        "opening_participation": participation,
        "transitions": transitions,
    }
    if recruiting:
        firm_hash, firm_data = load_jolts(vintage)
        common = sorted(set.intersection(*(set(series) for series in firm_data.values())))
        recent = common[-12:]
        def rate(code, period):
            return firm_data[code][period] / firm_data["PAYEMS"][period]
        accessions = sum(data[code][period] for code in (FLOWS[1][0], FLOWS[2][0]) for period in history)
        previous_employed = sum(data[code][period] for code in FLOWS[0] for period in history)
        input_data["schema_version"] = "owg.historical-labor.v2"
        input_data["recruiting"] = {
            "source_manifest_sha256": firm_hash,
            "openings_per_payroll_job": rate("JTSJOL", recent[-1]),
            "hires_per_payroll_job": statistics.median(rate("JTSHIL", period) for period in recent),
            "layoffs_per_payroll_job": statistics.median(rate("JTSLDL", period) for period in recent),
            "cps_accessions_per_employed": accessions / previous_employed,
            "search_contacts": 4,
        }
    return input_data, [data["UNRATE"][period] / 100 for period in history], history[0]


def reference_scenario(binary, folder):
    template = folder / "scenario-template.json"
    subprocess.run([str(binary), "init-scenario", "--output", str(template)], check=True, capture_output=True)
    scenario = json.loads(template.read_text())
    template.unlink()
    scenario["shocks"] = []
    scenario["policy_branches"] = [
        {"id": "baseline", "label": "Endogenous policy reaction", "actions": []}
    ]
    scenario["calendar"]["months"] = HORIZON
    return scenario


def baseline_forecasts(history, labor, months):
    persistence = history[-1]
    x, y = history[:-1], history[1:]
    x_mean, y_mean = statistics.mean(x), statistics.mean(y)
    spread = sum((v - x_mean) ** 2 for v in x)
    slope = sum((a - x_mean) * (b - y_mean) for a, b in zip(x, y)) / spread if spread else 0
    intercept = y_mean - slope * x_mean
    ar = persistence
    participation = labor["opening_participation"]
    population = [participation * (1 - persistence), participation * persistence, 1 - participation]
    for _ in range(months):
        ar = min(1.0, max(0.0, intercept + slope * ar))
        population = [
            sum(population[i] * labor["transitions"][i][j] for i in range(3))
            for j in range(3)
        ]
        yield persistence, ar, population[1] / sum(population[:2])


def prepare(binary, folder, *, recruiting=False, finance=False, production=False, inventory=False, household_demand=False):
    folder.mkdir(parents=True, exist_ok=True)
    template = reference_scenario(binary, folder)
    eval_hash, evaluation = load_vintage(SOURCE_ROOT, EVALUATION)
    macro = load_macro(PHILLY_ROOT, "2023Q4")
    cpi = dict(macro.observations["CPI"])
    real_output = dict(macro.observations["ROUTPUT"])
    jolts_eval = load_jolts(EVALUATION)[1] if recruiting else None
    bank_eval, bank_eval_sha = load_bank_scoring() if finance else ({}, None)
    production_eval, production_eval_sha = load_production(EVALUATION) if production else ({}, None)
    if inventory and not production:
        raise ValueError("inventory experiment requires dated production")
    inventory_eval, inventory_eval_sha = load_inventory(EVALUATION) if inventory else ({}, None)
    if household_demand and not inventory:
        raise ValueError("household demand experiment requires dated inventory")
    demand_eval, demand_eval_sha = load_household_behavior(EVALUATION) if household_demand else ({}, None)
    origins = []
    for vintage in VINTAGES:
        if vintage == EVALUATION:
            continue
        year = int(vintage[:4])
        labor, history, train_from = fit(vintage, recruiting=recruiting)
        scenario = {
            **template,
            "scenario_id": f"us-labor-research-{year}",
            "title": f"U.S. labor research run · {year}",
            "calendar": {**template["calendar"], "start_year": year},
        }
        target = folder / str(year)
        target.mkdir(exist_ok=True)
        save(target / "scenario.json", scenario)
        save(target / "labor.json", labor)
        production_input = load_production(vintage) if production else None
        if production_input:
            production_data, production_sha = production_input
            production_input_data = {
                "schema_version": "owg.historical-production.v1",
                "source_sha256": production_sha,
                "consumption_shares": production_data["consumption_shares"],
                "payroll_shares": production_data["payroll_shares"],
            }
            save(target / "production.json", production_input_data)
        if inventory:
            inventory_data, inventory_sha = load_inventory(vintage)
            inventory_policy = {
                "source_sha256": inventory_sha,
                **{sector: {key: value for key, value in inventory_data[sector].items()
                            if key != "unconstrained_adjustment"} for sector in ("food", "manufacturing")},
            }
            inventory_input = {
                **production_input_data,
                "schema_version": "owg.historical-production.v2",
                "inventory": inventory_policy,
            }
            save(target / "inventory.json", inventory_input)
        if household_demand:
            demand_data, demand_sha = load_household_behavior(vintage)
            demand_policy = {
                "source_sha256": demand_sha,
                **{field: demand_data[field] for field in (
                    "average_spending_share", "income_response", "social_security_per_wage",
                )},
            }
            save(target / "demand.json", {
                **inventory_input,
                "schema_version": "owg.historical-production.v3",
                "household_demand": demand_policy,
            })
        bank_input = load_bank_opening(vintage) if finance else None
        if bank_input:
            bank_data, bank_sha = bank_input
            save(target / "finance.json", {
                "schema_version": "owg.historical-finance.v1",
                "source_sha256": bank_sha,
                "banks": bank_data["banks"],
            })
        if recruiting:
            worker_only = {key: value for key, value in labor.items() if key != "recruiting"}
            worker_only["schema_version"] = "owg.historical-labor.v1"
            save(target / "labor-worker-only.json", worker_only)
        live_months = (DEFAULT_END_YEAR - year + 1) * 12 if recruiting else min(36, (int(VINTAGES[EVALUATION][:4]) - year + 1) * 12)
        save(target / "live-scenario.json", {
            **scenario, "calendar": {**scenario["calendar"], "months": live_months}
        })
        observed = []
        for offset, (persistence, ar1, flows_only) in enumerate(baseline_forecasts(history, labor, live_months)):
            period = f"{year + offset // 12}-{1 + offset % 12:02d}-01"
            def observed_share(series):
                value = evaluation[series].get(period)
                return value / 100 if value is not None else None

            period_year, period_month = int(period[:4]), int(period[5:7])
            current_cpi = cpi.get(f"{period_year}:{period_month:02}")
            prior_cpi = cpi.get(f"{period_year - 1}:{period_month:02}")
            observed_inflation = current_cpi / prior_cpi - 1 if current_cpi and prior_cpi else None
            observed_gdp_growth = None
            if period_month % 3 == 0:
                quarter = period_month // 3
                current = real_output.get(f"{period_year}:Q{quarter}")
                previous = real_output.get(f"{period_year - 1}:Q4" if quarter == 1 else f"{period_year}:Q{quarter - 1}")
                if production:
                    series = production_eval["real_gdp"]
                    q_start = f"{period_year}-{period_month - 2:02d}"
                    prev_start = f"{period_year - 1}-10" if quarter == 1 else f"{period_year}-{period_month - 5:02d}"
                    current, previous = series.get(q_start), series.get(prev_start)
                if current and previous:
                    observed_gdp_growth = (current / previous) ** 4 - 1
            observed.append({
                "period": period[:7],
                "observed_unemployment": observed_share("UNRATE"),
                "observed_participation": observed_share("CIVPART"),
                "observed_inflation": observed_inflation,
                "observed_real_growth": observed_gdp_growth,
                "observed_bank_loan_to_deposit": bank_eval.get(period[:7]),
                "observed_inventory_ratios": inventory_eval.get("ratios", {}).get(period[:7]),
                **({"observed_household_spending_share": demand_eval["spending_share"].get(period[:7])}
                   if household_demand else {}),
                "observed_payroll_shares": production_eval.get("payroll", {}).get(period[:7]),
                "observed_consumption_shares": production_eval.get("pce", {}).get(f"{period_year}-{period_month - 2:02d}")
                    if period_month % 3 == 0 else None,
                "observed_opening_stock_per_payroll_job": (
                    jolts_eval["JTSJOL"][period] / jolts_eval["PAYEMS"][period]
                    if jolts_eval and period in jolts_eval["JTSJOL"] else None
                ),
                "persistence": persistence,
                "ar1": ar1,
                "flows_only": flows_only,
            })
        origin = {
            "year": year,
            "as_of": vintage,
            "opening_month": VINTAGES[vintage][:7],
            "training_start": train_from[:7],
            "training_months": TRAIN_MONTHS,
            "opening_unemployment": labor["opening_unemployment"],
            "opening_participation": labor["opening_participation"],
            "transitions": labor["transitions"],
            "source_manifest_sha256": labor["source_manifest_sha256"],
            "scoring_vintage": EVALUATION,
            "scoring_manifest_sha256": eval_hash,
            "horizon_months": HORIZON,
            "live_months": live_months,
            "observed_through": VINTAGES[EVALUATION][:7],
            "macro_scoring_vintage": "2023Q4",
            "macro_manifest_sha256": macro.manifest_hash,
            "recruiting": labor.get("recruiting"),
            "population": scenario["population"],
        }
        if production_input:
            origin["production"] = {
                "source_sha256": production_sha,
                "pce_quarters": production_data["pce_quarters"],
                "payroll_month": production_data["payroll_month"],
                "opening_gdp_growth": production_data["opening_gdp_growth"],
                "consumption_shares": production_data["consumption_shares"],
                "payroll_shares": production_data["payroll_shares"],
            }
        if inventory:
            origin["inventory"] = {**inventory_policy, "latest_observation": inventory_data["latest_observation"]}
        if household_demand:
            origin["household_demand"] = {**demand_policy, "latest_month": demand_data["latest_month"]}
        if bank_input:
            origin["finance"] = {
                "source_sha256": bank_sha,
                "h8_month": bank_data["h8_observation_month"],
                "z1_quarter": bank_data["z1"]["observation_quarter"],
                "z1_publication_date": bank_data["z1"]["publication_date"],
                "opening_loan_to_deposit": sum(
                    bank["deposit_share"] * (bank["household_loans_per_deposit"] + bank["firm_loans_per_deposit"])
                    for bank in bank_data["banks"]
                ),
            }
        save(target / "observed.json", observed)
        origin["files"] = {
            name: digest(target / name)
            for name in ("scenario.json", "live-scenario.json", "labor.json", "observed.json",
                         *(("labor-worker-only.json",) if recruiting else ()),
                         *(("finance.json",) if bank_input else ()),
                         *(("production.json",) if production_input else ()),
                         *(("inventory.json",) if inventory else ()),
                         *(("demand.json",) if household_demand else ()))
        }
        origins.append(origin)
    manifest = {
        "schema_version": "owg.historical-lab.v6" if household_demand else
                          "owg.historical-lab.v5" if inventory else
                          "owg.historical-lab.v4" if production else
                          "owg.historical-lab.v3" if finance else
                          "owg.historical-lab.v2" if recruiting else "owg.historical-lab.v1",
        "classification": "retrospective CPS and JOLTS labor experiment" if recruiting else "retrospective CPS labor experiment",
        "default_end_year": DEFAULT_END_YEAR,
        "recruiting": recruiting,
        "finance": finance,
        "production": production,
        "inventory": inventory,
        **({"household_demand": True, "household_scoring_sha256": demand_eval_sha} if household_demand else {}),
        "inventory_scoring_sha256": inventory_eval_sha,
        "production_scoring_sha256": production_eval_sha,
        "finance_scoring_sha256": bank_eval_sha,
        "forecast_qualified": False,
        "origins": origins,
        "primary": "mean absolute unemployment error, across origins and 12 months",
        "comparison_models": ["persistence", "ar1", "flows_only", *(["worker_only_engine"] if recruiting else [])],
        "seeds": SEEDS,
        "binary_sha256": digest(binary),
        "calibration_code_sha256": digest(Path(__file__)),
    }
    save(folder / "protocol.json", manifest)
    return manifest


def production_name(inventory, household_demand):
    return "demand.json" if household_demand else "inventory.json" if inventory else "production.json"


def engine_command(binary, target, seed, *, step_input=False, scenario_path=None, labor_name="labor.json", finance=False, production=False, inventory=False, household_demand=False):
    scenario = "live-scenario.json" if step_input else "scenario.json"
    return [
        str(binary), "run-live", str(scenario_path or target / scenario),
        "--labor", str(target / labor_name), "--replication", str(seed),
        *(["--finance", str(target / "finance.json")] if finance else []),
        *(["--production", str(target / production_name(inventory, household_demand))]
          if production or inventory or household_demand else []),
        *(["--step-input"] if step_input else []),
    ]


def run_replication(binary, target, seed, labor_name="labor.json", *, finance=False, production=False, inventory=False, household_demand=False):
    command = engine_command(binary, target, seed, labor_name=labor_name, finance=finance,
                             production=production, inventory=inventory, household_demand=household_demand)
    result = subprocess.run(command, capture_output=True, text=True, timeout=120)
    if result.returncode:
        raise RuntimeError(result.stderr.strip())
    header, *frames = map(json.loads, result.stdout.splitlines())
    if header["labor_sha256"] != digest(target / labor_name) or len(frames) != HORIZON:
        raise ValueError("run does not match the frozen input")
    if finance and header["finance_sha256"] != digest(target / "finance.json"):
        raise ValueError("run does not match the frozen finance input")
    if (production or inventory or household_demand) and header["production_sha256"] != digest(
        target / production_name(inventory, household_demand)
    ):
        raise ValueError("run does not match the frozen production input")
    return frames


def normalized(values):
    total = sum(values)
    if total <= 0:
        return [0.0] * len(values)
    return [value / total for value in values]


def sector_errors(actual, runs, month, control, opening):
    def profile(field, paths, start, stop):
        totals = [
            sum(path[index]["sectors"][sector][field] for index in range(start, stop))
            for path in paths for sector in range(4)
        ]
        return normalized([sum(totals[sector::4]) for sector in range(4)])

    def errors(target, variants):
        return [statistics.mean(abs(left - right) for left, right in zip(target, variant, strict=True)) * 100
                for variant in variants]

    row = {}
    payroll = actual["observed_payroll_shares"]
    if payroll is not None:
        row["payroll_share_error_pp"] = errors(payroll, [
            profile("workers", runs, month - 1, month),
            profile("workers", control, month - 1, month),
            opening["payroll_shares"],
        ])
    consumption = actual["observed_consumption_shares"]
    if consumption is not None:
        row["consumption_share_error_pp"] = errors(consumption, [
            profile("household_spending", runs, month - 3, month),
            profile("household_spending", control, month - 3, month),
            opening["consumption_shares"],
        ])
    if month >= 6 and month % 3 == 0 and actual["observed_real_growth"] is not None:
        def growth(paths):
            def real(start, stop):
                return sum(run[index]["metrics"]["real_gdp"] for run in paths for index in range(start, stop))
            return (real(month - 3, month) / real(month - 6, month - 3)) ** 4 - 1

        row["gdp_growth_error_pp"] = [
            abs(value - actual["observed_real_growth"]) * 100
            for value in (growth(runs), growth(control), opening["opening_gdp_growth"])
        ]
    return row


def production_diagnostics(actual, index, legacy, candidate, origin):
    def month_flow(paths):
        return {key: statistics.mean(run[index]["production_flows"][key] for run in paths)
                for key in paths[0][index]["production_flows"]}

    def ratios(paths):
        return [sum(run[index]["sectors"][sector]["inventory_value"] for run in paths) /
                max(sum(run[index]["sectors"][sector]["sales"] for run in paths), 1e-12)
                for sector in (1, 2)]

    def quarter(paths, start, end, field):
        return sum(run[i]["production_flows"][field] for run in paths for i in range(start, end))

    def growth(paths, corrected):
        before, now = index - 5, index - 2
        end = index + 1
        if not corrected:
            totals = [sum(run[i]["metrics"]["real_gdp"] for run in paths for i in range(a, b))
                      for a, b in ((before, now), (now, end))]
            return (totals[1] / totals[0]) ** 4 - 1, None
        output = [quarter(paths, a, b, "gross_output_fixed_prices") for a, b in ((before, now), (now, end))]
        imports = [quarter(paths, a, b, "imported_inputs_fixed_prices") for a, b in ((before, now), (now, end))]
        prior = output[0] - imports[0]
        contribution = [(output[1] - output[0]) / prior, -(imports[1] - imports[0]) / prior]
        return ((output[1] - imports[1]) / prior) ** 4 - 1, contribution

    prior_ratio = [origin["inventory"][sector]["opening_months"] for sector in ("food", "manufacturing")]
    observed_ratio = actual["observed_inventory_ratios"]
    baseline_ratio, candidate_ratio = ratios(legacy), ratios(candidate)
    row = {
        "origin": origin["as_of"], "period": actual["period"],
        "observed_inventory_ratios": observed_ratio,
        "legacy_inventory_ratios": baseline_ratio, "candidate_inventory_ratios": candidate_ratio,
        "legacy_flow": month_flow(legacy), "candidate_flow": month_flow(candidate),
    }
    if observed_ratio:
        row["inventory_ratio_mae_months"] = [
            statistics.mean(abs(a - b) for a, b in zip(values, observed_ratio, strict=True))
            for values in (candidate_ratio, baseline_ratio, prior_ratio)
        ]
    if index >= 5 and (index + 1) % 3 == 0 and actual["observed_real_growth"] is not None:
        observed_growth = actual["observed_real_growth"]
        options = [growth(candidate, True), growth(legacy, True), growth(legacy, False),
                   (origin["production"]["opening_gdp_growth"], None)]
        row["gdp_growth"] = [estimate for estimate, _ in options]
        row["gross_output_and_import_contributions"] = [details for _, details in options[:2]]
        row["observed_gdp_growth"] = observed_growth
        row["gdp_growth_mae_pp"] = [100 * abs(estimate - observed_growth) for estimate, _ in options]
    return row


def score(binary, folder, manifest):
    rows = []
    production_errors = []
    inventory_results = []
    demand_results = []
    for origin in manifest["origins"]:
        year = origin["year"]
        target = folder / str(year)
        observed = json.loads((target / "observed.json").read_text())
        seeds = manifest["seeds"]
        with ThreadPoolExecutor(max_workers=min(8, len(seeds))) as pool:
            futures = [pool.submit(run_replication, binary, target, seed) for seed in seeds]
            controls = [pool.submit(run_replication, binary, target, seed, "labor-worker-only.json")
                        for seed in seeds] if manifest["recruiting"] else []
            financial = [pool.submit(run_replication, binary, target, seed, finance=True)
                         for seed in seeds] if "finance" in origin else []
            production_jobs = [
                pool.submit(run_replication, binary, target, seed, finance="finance" in origin, production=True)
                for seed in seeds
            ] if "production" in origin else []
            inventory_jobs = [
                pool.submit(run_replication, binary, target, seed, finance="finance" in origin, inventory=True)
                for seed in seeds
            ] if "inventory" in origin else []
            demand_jobs = [
                pool.submit(run_replication, binary, target, seed, finance="finance" in origin, household_demand=True)
                for seed in seeds
            ] if "household_demand" in origin else []
            runs = [future.result() for future in futures]
            financial_runs = [future.result() for future in financial]
            worker_only = [future.result() for future in controls]
            production_runs = [future.result() for future in production_jobs]
            inventory_runs = [future.result() for future in inventory_jobs]
            demand_runs = [future.result() for future in demand_jobs]
        for month, (actual, *frames) in enumerate(zip(observed[:HORIZON], *runs, strict=True), 1):
            finance_frames = [run[month - 1] for run in financial_runs]
            production_frames = [run[month - 1] for run in production_runs]
            inventory_frames = [run[month - 1] for run in inventory_runs]
            demand_frames = [run[month - 1] for run in demand_runs]
            if any(
                (frame["metrics"]["year"], frame["metrics"]["month_of_year"]) != (year, month)
                or frame["metrics"]["ledger_residual_minor_units"] != 0
                or frame["labor"]["population"][0] != sum(sector["workers"] for sector in frame["sectors"])
                for frame in (*frames, *finance_frames, *production_frames, *inventory_frames, *demand_frames)
            ):
                raise ValueError(f"engine calendar, worker counts or accounting mismatch at {year}-{month}")
            if manifest["recruiting"]:
                for frame in (*frames, *finance_frames, *production_frames, *inventory_frames, *demand_frames):
                    labor = frame["labor"]
                    recruit = labor["recruiting"]
                    if recruit["opening_stock"] - recruit["withdrawn"] + recruit["new_postings"] - labor["matches"] != recruit["closing_stock"]:
                        raise ValueError(f"vacancy stock does not reconcile at {year}-{month}")
            row = {
                "origin": origin["as_of"],
                "horizon": month,
                **actual,
                "agent_engine": statistics.mean(frame["metrics"]["unemployment"] for frame in frames),
                "seed_min": min(frame["metrics"]["unemployment"] for frame in frames),
                "seed_max": max(frame["metrics"]["unemployment"] for frame in frames),
                "mean_applications": statistics.mean(frame["labor"]["applications"] for frame in frames),
                "mean_matches": statistics.mean(frame["labor"]["matches"] for frame in frames),
                "mean_vacancies": statistics.mean(frame["labor"]["vacancies"] for frame in frames),
            }
            if worker_only:
                row["worker_only_engine"] = statistics.mean(run[month - 1]["metrics"]["unemployment"] for run in worker_only)
                row["mean_vacancy_stock_per_employed"] = statistics.mean(
                    frame["labor"]["recruiting"]["closing_stock"] / max(1, frame["labor"]["population"][0])
                    for frame in frames
                )
            if manifest["finance"]:
                def credit_ratio(frame):
                    banks = frame["banks"]
                    return sum(bank["loans"] for bank in banks) / sum(bank["deposits"] for bank in banks)

                row.update({
                    "bank_opening_unemployment": statistics.mean(frame["metrics"]["unemployment"] for frame in finance_frames) if finance_frames else None,
                    "bank_opening_loan_to_deposit": statistics.mean(credit_ratio(frame) for frame in finance_frames) if finance_frames else None,
                    "synthetic_opening_loan_to_deposit": statistics.mean(credit_ratio(frame) for frame in frames) if finance_frames else None,
                    "persistent_opening_loan_to_deposit": origin.get("finance", {}).get("opening_loan_to_deposit"),
                })
            if production_runs:
                for frame in production_frames:
                    if abs(sum(sector["household_spending"] for sector in frame["sectors"])
                           - frame["metrics"]["consumption_total"]) > 1e-6:
                        raise ValueError("household sector spending does not reconcile")
                production_errors.append({
                    "origin": origin["as_of"], "period": actual["period"],
                    **sector_errors(actual, production_runs, month, financial_runs or runs, origin["production"]),
                })
            if inventory_runs:
                for frame in (*inventory_frames, *demand_frames):
                    if frame["sectors"][3]["inventory_value"] != 0 or frame["metrics"]["accounting_identity_relative_error"] > 1e-8:
                        raise ValueError("service expiry or nominal production accounting did not reconcile")
                    if abs(frame["metrics"]["real_gdp"] - frame["production_flows"]["value_added_fixed_prices"]) > 1e-8:
                        raise ValueError("fixed-price output bridge did not reconcile")
                inventory_results.append(production_diagnostics(
                    actual, month - 1, production_runs, inventory_runs, origin,
                ))
            if demand_runs:
                diagnostic = production_diagnostics(actual, month - 1, inventory_runs, demand_runs, origin)
                diagnostic.pop("inventory_ratio_mae_months", None)
                for name, paths in (("candidate", demand_frames), ("inventory", inventory_frames)):
                    income = sum(frame["production_flows"]["household_disposable_income"] for frame in paths)
                    consumption = sum(frame["metrics"]["consumption_total"] for frame in paths)
                    diagnostic[f"{name}_spending_share"] = consumption / income if income > 0 else None
                    diagnostic[f"{name}_unemployment"] = statistics.mean(
                        frame["metrics"]["unemployment"] for frame in paths
                    )
                diagnostic.update({
                    "observed_spending_share": actual["observed_household_spending_share"],
                    "observed_unemployment": actual["observed_unemployment"],
                    "persistent_unemployment": actual["persistence"],
                })
                demand_results.append(diagnostic)
            rows.append(row)
        print(f"Scored {year} across {len(runs)} seeds", flush=True)
    with (folder / "scores.csv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    models = ("agent_engine", *manifest["comparison_models"])
    def mae(group):
        return {
            model: 100 * statistics.mean(abs(row[model] - row["observed_unemployment"]) for row in group)
            for model in models
        }

    summary = {
        "protocol_sha256": digest(folder / "protocol.json"),
        "scores_sha256": digest(folder / "scores.csv"),
        "mean_absolute_error_percentage_points": mae(rows),
        "by_origin": {o["year"]: mae([r for r in rows if r["origin"] == o["as_of"]]) for o in manifest["origins"]},
        "by_horizon": {h: mae([r for r in rows if r["horizon"] == h]) for h in (1, 3, 12)},
        "scored_months": len(rows),
        "matching": {
            "mean_vacancies": statistics.mean(row["mean_vacancies"] for row in rows),
            "mean_applications": statistics.mean(row["mean_applications"] for row in rows),
            "application_fill_share": sum(row["mean_matches"] for row in rows) / sum(row["mean_applications"] for row in rows),
        },
        "forecast_qualified": False,
    }
    if manifest["recruiting"]:
        summary["matching"]["mean_vacancy_stock_per_employed"] = statistics.mean(row["mean_vacancy_stock_per_employed"] for row in rows)
        summary["matching"]["mean_observed_stock_per_payroll_job"] = statistics.mean(
            row["observed_opening_stock_per_payroll_job"] for row in rows
        )
    if manifest["finance"]:
        matched = [row for row in rows if row["bank_opening_unemployment"] is not None]
        def paired_mae(target, variants):
            return {name: 100 * statistics.mean(abs(row[name] - row[target]) for row in matched)
                    for name in variants}

        summary["bank_opening"] = {
            "matched_months": len(matched),
            "unemployment_mae_pp": paired_mae("observed_unemployment", [
                "bank_opening_unemployment", "agent_engine", "persistence",
            ]),
            "loan_to_deposit_mae_pp": paired_mae("observed_bank_loan_to_deposit", [
                "bank_opening_loan_to_deposit", "synthetic_opening_loan_to_deposit", "persistent_opening_loan_to_deposit",
            ]),
        }
    if production_errors:
        path = folder / "production-errors.json"
        save(path, production_errors)
        summary["production"] = {"errors_sha256": digest(path)}
        for metric in ("payroll_share_error_pp", "consumption_share_error_pp", "gdp_growth_error_pp"):
            comparisons = [row[metric] for row in production_errors if metric in row]
            summary["production"][metric] = {
                "periods": len(comparisons),
                **dict(zip(("dated", "synthetic", "opening_persistence"),
                           (statistics.mean(column) for column in zip(*comparisons)), strict=True)),
            }
    if inventory_results:
        path = folder / "inventory-diagnostics.json"
        save(path, inventory_results)
        summary["inventory"] = {"diagnostics_sha256": digest(path)}
        for metric, names in (
            ("inventory_ratio_mae_months", ("candidate", "legacy", "opening_persistence")),
            ("gdp_growth_mae_pp", ("candidate", "legacy_fixed_price", "legacy_published", "opening_persistence")),
        ):
            comparisons = [row[metric] for row in inventory_results if metric in row]
            summary["inventory"][metric] = {
                "periods": len(comparisons),
                **dict(zip(names, (statistics.mean(column) for column in zip(*comparisons)), strict=True)),
            }
    if demand_results:
        path = folder / "demand-diagnostics.json"
        save(path, demand_results)
        gdp_errors = [row["gdp_growth_mae_pp"] for row in demand_results if "gdp_growth_mae_pp" in row]
        summary["household_demand"] = {
            "diagnostics_sha256": digest(path),
            "gdp_growth_mae_pp": {
                "periods": len(gdp_errors),
                **{name: statistics.mean(row[index] for row in gdp_errors)
                   for name, index in (("candidate", 0), ("inventory", 1), ("persistence", 3))},
            },
            "unemployment_mae_pp": {
                "periods": len(demand_results),
                **{name: 100 * statistics.mean(abs(row[f"{name}_unemployment"] - row["observed_unemployment"])
                                              for row in demand_results)
                   for name in ("candidate", "inventory", "persistent")},
            },
            "spending_share_mae_pp": {
                name: 100 * statistics.mean(abs(row[f"{name}_spending_share"] - row["observed_spending_share"])
                                            for row in demand_results
                                            if row[f"{name}_spending_share"] is not None
                                            and row["observed_spending_share"] is not None)
                for name in ("candidate", "inventory")
            },
        }
    save(folder / "summary.json", summary)
    return summary


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=BINARY)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--score", action="store_true")
    parser.add_argument("--worker-only", action="store_true")
    parser.add_argument("--finance", action="store_true")
    parser.add_argument("--production", action="store_true")
    parser.add_argument("--inventory", action="store_true")
    parser.add_argument("--household-demand", action="store_true")
    args = parser.parse_args()
    output = (args.output or (DEMAND_OUTPUT if args.household_demand else OUTPUT)).resolve()
    protocol = prepare(args.binary.resolve(), output, recruiting=not args.worker_only, finance=args.finance,
                       production=args.production, inventory=args.inventory, household_demand=args.household_demand)
    if args.score:
        print(json.dumps(score(args.binary.resolve(), output, protocol), indent=2))
