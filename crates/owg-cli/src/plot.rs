use crate::output::verify_result_bundle;
use anyhow::{Context, Result, bail};
use owg_core::{ComparisonReport, MetricRow, ResultBundleContract, RunManifest, SimulationResult};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

const PLOT_VERSION: &str = "owg.plot.v1";
const TEMPLATE: &str = include_str!("plot_template.html");
const PAYLOAD_MARKER: &str = "__OWG_PLOT_PAYLOAD__";

#[derive(Clone, Copy, Debug)]
struct MetricSpec {
    key: &'static str,
    label: &'static str,
    axis_label: &'static str,
    format: &'static str,
    decimals: u8,
    scale: f64,
    include_zero: bool,
}

const METRIC_SPECS: &[MetricSpec] = &[
    MetricSpec {
        key: "cpi",
        label: "Consumer price index",
        axis_label: "Index",
        format: "number",
        decimals: 2,
        scale: 1.0,
        include_zero: false,
    },
    MetricSpec {
        key: "inflation_year_over_year",
        label: "Year-over-year inflation",
        axis_label: "Percent",
        format: "percent",
        decimals: 2,
        scale: 100.0,
        include_zero: true,
    },
    MetricSpec {
        key: "cpi_energy",
        label: "Energy CPI",
        axis_label: "Index",
        format: "number",
        decimals: 2,
        scale: 1.0,
        include_zero: false,
    },
    MetricSpec {
        key: "cpi_food",
        label: "Food CPI",
        axis_label: "Index",
        format: "number",
        decimals: 2,
        scale: 1.0,
        include_zero: false,
    },
    MetricSpec {
        key: "cpi_manufacturing",
        label: "Manufacturing CPI",
        axis_label: "Index",
        format: "number",
        decimals: 2,
        scale: 1.0,
        include_zero: false,
    },
    MetricSpec {
        key: "cpi_services",
        label: "Services CPI",
        axis_label: "Index",
        format: "number",
        decimals: 2,
        scale: 1.0,
        include_zero: false,
    },
    MetricSpec {
        key: "real_gdp",
        label: "Real GDP",
        axis_label: "Model units",
        format: "compact",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "nominal_gdp",
        label: "Nominal GDP",
        axis_label: "Model units",
        format: "compact",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "output_gap",
        label: "Output gap",
        axis_label: "Percent",
        format: "percent",
        decimals: 2,
        scale: 100.0,
        include_zero: true,
    },
    MetricSpec {
        key: "unemployment",
        label: "Unemployment",
        axis_label: "Percent",
        format: "percent",
        decimals: 2,
        scale: 100.0,
        include_zero: true,
    },
    MetricSpec {
        key: "average_wage",
        label: "Average wage",
        axis_label: "Model units",
        format: "number",
        decimals: 2,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "consumption_total",
        label: "Household consumption",
        axis_label: "Model units",
        format: "compact",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "gini",
        label: "Consumption Gini",
        axis_label: "Index",
        format: "number",
        decimals: 3,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "poverty_share",
        label: "Poverty proxy",
        axis_label: "Percent",
        format: "percent",
        decimals: 2,
        scale: 100.0,
        include_zero: true,
    },
    MetricSpec {
        key: "inventories",
        label: "Firm inventories",
        axis_label: "Model units",
        format: "compact",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "firm_defaults",
        label: "Firm defaults",
        axis_label: "Count per month",
        format: "number",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "household_defaults",
        label: "Household defaults",
        axis_label: "Count per month",
        format: "number",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "bank_resolutions",
        label: "Bank resolutions",
        axis_label: "Count per month",
        format: "number",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "active_firms",
        label: "Active firms",
        axis_label: "Count",
        format: "number",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "credit",
        label: "Outstanding credit",
        axis_label: "Model units",
        format: "compact",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "fiscal_balance",
        label: "Fiscal balance",
        axis_label: "Model units",
        format: "compact",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "government_debt",
        label: "Government debt",
        axis_label: "Model units",
        format: "compact",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "imports",
        label: "Imports",
        axis_label: "Model units",
        format: "compact",
        decimals: 1,
        scale: 1.0,
        include_zero: true,
    },
    MetricSpec {
        key: "policy_rate",
        label: "Policy interest rate",
        axis_label: "Percent",
        format: "percent",
        decimals: 2,
        scale: 100.0,
        include_zero: true,
    },
];

#[derive(Clone, Debug)]
struct PlotInputRow {
    branch_id: String,
    replication: u32,
    month: u32,
    year: i32,
    month_of_year: u8,
    values: Vec<f64>,
}

impl PlotInputRow {
    fn from_metric(row: &MetricRow) -> Self {
        Self {
            branch_id: row.branch_id.clone(),
            replication: row.replication,
            month: row.month,
            year: row.year,
            month_of_year: row.month_of_year,
            values: [
                row.cpi,
                row.inflation_year_over_year,
                row.cpi_energy,
                row.cpi_food,
                row.cpi_manufacturing,
                row.cpi_services,
                row.real_gdp,
                row.nominal_gdp,
                row.output_gap,
                row.unemployment,
                row.average_wage,
                row.consumption_total,
                row.gini,
                row.poverty_share,
                row.inventories,
                f64::from(row.firm_defaults),
                f64::from(row.household_defaults),
                f64::from(row.bank_resolutions),
                f64::from(row.active_firms),
                row.credit,
                row.fiscal_balance,
                row.government_debt,
                row.imports,
                row.policy_rate,
            ]
            .into_iter()
            .map(csv_precision)
            .collect(),
        }
    }
}

fn csv_precision(value: f64) -> f64 {
    format!("{value:.12}").parse().unwrap_or(value)
}

#[derive(Debug, Deserialize)]
struct PlotCsvRow {
    branch_id: String,
    replication: u32,
    month: u32,
    year: i32,
    month_of_year: u8,
    cpi: f64,
    inflation_year_over_year: f64,
    cpi_energy: f64,
    cpi_food: f64,
    cpi_manufacturing: f64,
    cpi_services: f64,
    real_gdp: f64,
    nominal_gdp: f64,
    output_gap: f64,
    unemployment: f64,
    average_wage: f64,
    consumption_total: f64,
    gini: f64,
    poverty_share: f64,
    inventories: f64,
    firm_defaults: u32,
    household_defaults: u32,
    bank_resolutions: u32,
    active_firms: u32,
    credit: f64,
    fiscal_balance: f64,
    government_debt: f64,
    imports: f64,
    policy_rate: f64,
}

impl From<PlotCsvRow> for PlotInputRow {
    fn from(row: PlotCsvRow) -> Self {
        Self {
            branch_id: row.branch_id,
            replication: row.replication,
            month: row.month,
            year: row.year,
            month_of_year: row.month_of_year,
            values: vec![
                row.cpi,
                row.inflation_year_over_year,
                row.cpi_energy,
                row.cpi_food,
                row.cpi_manufacturing,
                row.cpi_services,
                row.real_gdp,
                row.nominal_gdp,
                row.output_gap,
                row.unemployment,
                row.average_wage,
                row.consumption_total,
                row.gini,
                row.poverty_share,
                row.inventories,
                f64::from(row.firm_defaults),
                f64::from(row.household_defaults),
                f64::from(row.bank_resolutions),
                f64::from(row.active_firms),
                row.credit,
                row.fiscal_balance,
                row.government_debt,
                row.imports,
                row.policy_rate,
            ],
        }
    }
}

#[derive(Debug, Serialize)]
struct PlotPayload {
    plot_version: &'static str,
    scenario_id: String,
    run_id: String,
    validation_status: String,
    confidence_level: f64,
    evaluation_start_month: u32,
    evaluation_end_month: u32,
    branches: Vec<String>,
    periods: Vec<PlotPeriod>,
    metrics: Vec<PlotMetric>,
    policy_scores: Vec<PlotPolicyScore>,
    minimax_regret_branch: Option<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PlotMetric {
    key: &'static str,
    label: &'static str,
    axis_label: &'static str,
    format: &'static str,
    decimals: u8,
    include_zero: bool,
    series: Vec<PlotSeries>,
}

#[derive(Debug, Serialize)]
struct PlotSeries {
    branch_id: String,
    replication_count: u32,
    points: Vec<PlotPoint>,
}

#[derive(Debug, Serialize)]
struct PlotPoint {
    month: u32,
    mean: f64,
    lower: f64,
    upper: f64,
}

#[derive(Debug, Serialize)]
struct PlotPeriod {
    month: u32,
    label: String,
}

#[derive(Debug, Serialize)]
struct PlotPolicyScore {
    branch_id: String,
    inflation_loss_percent: f64,
    output_loss_percent: f64,
    unemployment_percent: f64,
    maximum_regret: f64,
    pareto_efficient: bool,
}

pub fn write_plot_dashboard_from_result(path: &Path, result: &SimulationResult) -> Result<()> {
    let rows = result
        .metrics
        .iter()
        .map(PlotInputRow::from_metric)
        .collect::<Vec<_>>();
    let payload = build_payload(&result.manifest, &result.comparison, &rows)?;
    write_dashboard(path, &payload)
}

pub fn write_plot_dashboard_from_bundle(bundle: &Path, output: &Path) -> Result<()> {
    verify_result_bundle(bundle)?;
    let package: ResultBundleContract = read_json(&bundle.join("bundle.json"))?;
    let manifest: RunManifest = read_json(&bundle.join(&package.manifest))?;
    let comparison: ComparisonReport = read_json(&bundle.join(&package.comparison))?;
    let mut reader = csv::Reader::from_path(bundle.join(&package.metrics.csv))
        .with_context(|| format!("failed to read metrics from {}", bundle.display()))?;
    let rows = reader
        .deserialize::<PlotCsvRow>()
        .map(|row| row.map(PlotInputRow::from))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let payload = build_payload(&manifest, &comparison, &rows)?;
    write_dashboard(output, &payload)
}

fn build_payload(
    manifest: &RunManifest,
    comparison: &ComparisonReport,
    rows: &[PlotInputRow],
) -> Result<PlotPayload> {
    if rows.is_empty() {
        bail!("cannot plot an empty result");
    }
    let mut seen = BTreeSet::new();
    let mut periods_by_month = BTreeMap::<u32, (i32, u8)>::new();
    for row in rows {
        if row.values.len() != METRIC_SPECS.len() {
            bail!("plot metric registry and observation width disagree");
        }
        if !row.values.iter().all(|value| value.is_finite()) {
            bail!(
                "plot observation contains a non-finite value for {} replication {} month {}",
                row.branch_id,
                row.replication,
                row.month
            );
        }
        if !seen.insert((row.branch_id.as_str(), row.replication, row.month)) {
            bail!(
                "duplicate plot observation for {} replication {} month {}",
                row.branch_id,
                row.replication,
                row.month
            );
        }
        if periods_by_month
            .insert(row.month, (row.year, row.month_of_year))
            .is_some_and(|period| period != (row.year, row.month_of_year))
        {
            bail!(
                "plot observations disagree on the calendar label for month {}",
                row.month
            );
        }
    }

    let branches = rows
        .iter()
        .map(|row| row.branch_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let periods = periods_by_month
        .into_iter()
        .map(|(month, (year, month_of_year))| PlotPeriod {
            month,
            label: format!("{year:04}-{month_of_year:02}"),
        })
        .collect();
    let metrics = METRIC_SPECS
        .iter()
        .enumerate()
        .map(|(metric_index, spec)| -> Result<PlotMetric> {
            let series = branches
                .iter()
                .map(|branch_id| -> Result<PlotSeries> {
                    let mut by_month = BTreeMap::<u32, Vec<&PlotInputRow>>::new();
                    for row in rows.iter().filter(|row| row.branch_id == *branch_id) {
                        by_month.entry(row.month).or_default().push(row);
                    }
                    let replication_counts =
                        by_month.values().map(Vec::len).collect::<BTreeSet<_>>();
                    if replication_counts.len() != 1 {
                        bail!(
                            "plot series {branch_id} has inconsistent replication counts by month"
                        );
                    }
                    let replication_count = replication_counts.first().copied().unwrap_or(0) as u32;
                    let points = by_month
                        .into_iter()
                        .map(|(month, observations)| {
                            let values = observations
                                .iter()
                                .map(|row| row.values[metric_index] * spec.scale)
                                .collect::<Vec<_>>();
                            let (mean, lower, upper) =
                                confidence_interval(&values, comparison.confidence_level);
                            PlotPoint {
                                month,
                                mean,
                                lower,
                                upper,
                            }
                        })
                        .collect();
                    Ok(PlotSeries {
                        branch_id: branch_id.clone(),
                        replication_count,
                        points,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(PlotMetric {
                key: spec.key,
                label: spec.label,
                axis_label: spec.axis_label,
                format: spec.format,
                decimals: spec.decimals,
                include_zero: spec.include_zero,
                series,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let policy_scores = comparison
        .scores
        .iter()
        .map(|score| PlotPolicyScore {
            branch_id: score.branch_id.clone(),
            inflation_loss_percent: score.inflation_loss * 100.0,
            output_loss_percent: score.output_loss * 100.0,
            unemployment_percent: score.unemployment_loss * 100.0,
            maximum_regret: score.maximum_regret,
            pareto_efficient: score.pareto_efficient,
        })
        .collect();

    Ok(PlotPayload {
        plot_version: PLOT_VERSION,
        scenario_id: comparison.scenario_id.clone(),
        run_id: manifest.run_id.clone(),
        validation_status: format!("{:?}", manifest.validation_status),
        confidence_level: comparison.confidence_level,
        evaluation_start_month: comparison.evaluation_start_month,
        evaluation_end_month: comparison.evaluation_end_month,
        branches,
        periods,
        metrics,
        policy_scores,
        minimax_regret_branch: comparison.minimax_regret_branch.clone(),
        warnings: manifest.warnings.clone(),
    })
}

fn confidence_interval(values: &[f64], confidence_level: f64) -> (f64, f64, f64) {
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let standard_error = if values.len() > 1 {
        let variance = values
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / (count - 1.0);
        variance.sqrt() / count.sqrt()
    } else {
        0.0
    };
    let z = if confidence_level >= 0.99 {
        2.576
    } else if confidence_level >= 0.95 {
        1.960
    } else if confidence_level >= 0.90 {
        1.645
    } else {
        1.282
    };
    let half_width = z * standard_error;
    (mean, mean - half_width, mean + half_width)
}

fn write_dashboard(path: &Path, payload: &PlotPayload) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_string(payload)?
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let html = TEMPLATE.replacen(PAYLOAD_MARKER, &payload, 1);
    if html == TEMPLATE {
        bail!("plot template payload marker is missing");
    }
    let mut writer = BufWriter::new(File::create(path)?);
    writer.write_all(html.as_bytes())?;
    writer.flush()?;
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_reader(BufReader::new(File::open(path)?))
        .with_context(|| format!("invalid JSON in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{METRIC_SPECS, PlotInputRow, build_payload};
    use owg_core::{ComparisonReport, RunManifest, ScenarioSpec};

    #[test]
    fn plot_payload_aggregates_replications_and_scales_rates() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut manifest = RunManifest::new(
            "plot-test".to_owned(),
            &scenario,
            vec!["baseline".to_owned()],
            "a".repeat(64),
        )
        .unwrap();
        manifest.warnings.clear();
        let rows = vec![
            PlotInputRow {
                branch_id: "baseline".to_owned(),
                replication: 0,
                month: 0,
                year: 2020,
                month_of_year: 1,
                values: vec![0.10; METRIC_SPECS.len()],
            },
            PlotInputRow {
                branch_id: "baseline".to_owned(),
                replication: 1,
                month: 0,
                year: 2020,
                month_of_year: 1,
                values: vec![0.20; METRIC_SPECS.len()],
            },
        ];
        let comparison = ComparisonReport::from_metrics(
            scenario.scenario_id,
            &[],
            0,
            0,
            scenario.replications.confidence_level,
        );

        let payload = build_payload(&manifest, &comparison, &rows).unwrap();

        assert_eq!(payload.branches, ["baseline"]);
        let inflation = payload
            .metrics
            .iter()
            .find(|metric| metric.key == "inflation_year_over_year")
            .unwrap();
        assert!((inflation.series[0].points[0].mean - 15.0).abs() < 1e-12);
        assert_eq!(inflation.series[0].replication_count, 2);
        assert!(inflation.series[0].points[0].lower < 15.0);
        assert!(inflation.series[0].points[0].upper > 15.0);
    }
}
