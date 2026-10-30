use crate::metrics::MetricRow;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const OBJECTIVE_NAMES: [&str; 6] = [
    "inflation",
    "output",
    "unemployment",
    "distribution",
    "defaults",
    "fiscal",
];

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ConfidenceInterval {
    pub mean: f64,
    pub lower: f64,
    pub upper: f64,
    pub replication_count: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct PolicyScore {
    pub branch_id: String,
    pub inflation_loss: f64,
    pub output_loss: f64,
    pub unemployment_loss: f64,
    pub distribution_loss: f64,
    pub default_loss: f64,
    pub fiscal_cost: f64,
    pub objective_intervals: BTreeMap<String, ConfidenceInterval>,
    pub normalized_regrets: BTreeMap<String, f64>,
    pub maximum_regret: f64,
    pub pareto_efficient: bool,
}

impl PolicyScore {
    fn objectives(&self) -> [f64; 6] {
        [
            self.inflation_loss,
            self.output_loss,
            self.unemployment_loss,
            self.distribution_loss,
            self.default_loss,
            self.fiscal_cost,
        ]
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ComparisonReport {
    pub scenario_id: String,
    pub evaluation_start_month: u32,
    pub evaluation_end_month: u32,
    pub confidence_level: f64,
    pub scores: Vec<PolicyScore>,
    pub pareto_frontier: Vec<String>,
    pub minimax_regret_branch: Option<String>,
    pub caveat: String,
}

impl ComparisonReport {
    #[must_use]
    pub fn from_metrics(
        scenario_id: impl Into<String>,
        metrics: &[MetricRow],
        evaluation_start_month: u32,
        evaluation_end_month: u32,
        confidence_level: f64,
    ) -> Self {
        let scenario_id = scenario_id.into();
        let branch_ids: BTreeSet<&str> = metrics.iter().map(|row| row.branch_id.as_str()).collect();
        let mut scores: Vec<PolicyScore> = branch_ids
            .into_iter()
            .map(|branch_id| {
                let rows: Vec<&MetricRow> = metrics
                    .iter()
                    .filter(|row| {
                        row.branch_id == branch_id
                            && row.month >= evaluation_start_month
                            && row.month <= evaluation_end_month
                    })
                    .collect();
                let mut by_replication = BTreeMap::<u32, Vec<&MetricRow>>::new();
                for row in rows {
                    by_replication.entry(row.replication).or_default().push(row);
                }
                let replication_objectives: Vec<[f64; 6]> = by_replication
                    .values()
                    .map(|replication_rows| objectives(replication_rows))
                    .collect();
                let objective_intervals: BTreeMap<String, ConfidenceInterval> = OBJECTIVE_NAMES
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let values: Vec<f64> = replication_objectives
                            .iter()
                            .map(|value| value[index])
                            .collect();
                        ((*name).to_owned(), interval(&values, confidence_level))
                    })
                    .collect();
                let means: [f64; 6] =
                    std::array::from_fn(|index| objective_intervals[OBJECTIVE_NAMES[index]].mean);
                PolicyScore {
                    branch_id: branch_id.to_owned(),
                    inflation_loss: means[0],
                    output_loss: means[1],
                    unemployment_loss: means[2],
                    distribution_loss: means[3],
                    default_loss: means[4],
                    fiscal_cost: means[5],
                    objective_intervals,
                    normalized_regrets: BTreeMap::new(),
                    maximum_regret: 0.0,
                    pareto_efficient: true,
                }
            })
            .collect();

        mark_pareto_frontier(&mut scores);
        add_normalized_regrets(&mut scores);
        scores.sort_by(|left, right| left.branch_id.cmp(&right.branch_id));

        let pareto_frontier = scores
            .iter()
            .filter(|score| score.pareto_efficient)
            .map(|score| score.branch_id.clone())
            .collect();
        let minimax_regret_branch = scores
            .iter()
            .min_by(|left, right| left.maximum_regret.total_cmp(&right.maximum_regret))
            .map(|score| score.branch_id.clone());

        Self {
            scenario_id,
            evaluation_start_month,
            evaluation_end_month,
            confidence_level,
            scores,
            pareto_frontier,
            minimax_regret_branch,
            caveat: "Minimax regret is a robustness diagnostic, not a recommendation. \
                     A recommendation requires decision-owner weights, constraints, and risk tolerance."
                .to_owned(),
        }
    }
}

fn objectives(rows: &[&MetricRow]) -> [f64; 6] {
    let denominator = rows.len().max(1) as f64;
    [
        rows.iter()
            .map(|row| row.inflation_year_over_year.abs())
            .sum::<f64>()
            / denominator,
        rows.iter()
            .map(|row| (-row.output_gap).max(0.0))
            .sum::<f64>()
            / denominator,
        rows.iter().map(|row| row.unemployment).sum::<f64>() / denominator,
        rows.iter()
            .map(|row| 0.5 * row.gini + 0.5 * row.poverty_share)
            .sum::<f64>()
            / denominator,
        rows.iter()
            .map(|row| {
                f64::from(row.firm_defaults)
                    + f64::from(row.household_defaults)
                    + 10.0 * f64::from(row.bank_resolutions)
            })
            .sum::<f64>()
            / denominator,
        rows.iter()
            .map(|row| (-row.fiscal_balance).max(0.0))
            .sum::<f64>()
            / denominator,
    ]
}

fn interval(values: &[f64], confidence_level: f64) -> ConfidenceInterval {
    if values.is_empty() {
        return ConfidenceInterval {
            mean: 0.0,
            lower: 0.0,
            upper: 0.0,
            replication_count: 0,
        };
    }
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
    ConfidenceInterval {
        mean,
        lower: mean - half_width,
        upper: mean + half_width,
        replication_count: values.len() as u32,
    }
}

fn mark_pareto_frontier(scores: &mut [PolicyScore]) {
    for index in 0..scores.len() {
        let current = scores[index].objectives();
        scores[index].pareto_efficient = !scores.iter().enumerate().any(|(other_index, other)| {
            if index == other_index {
                return false;
            }
            let candidate = other.objectives();
            let no_worse = candidate
                .iter()
                .zip(current)
                .all(|(candidate_value, current_value)| *candidate_value <= current_value);
            let strictly_better = candidate
                .iter()
                .zip(current)
                .any(|(candidate_value, current_value)| *candidate_value < current_value);
            no_worse && strictly_better
        });
    }
}

fn add_normalized_regrets(scores: &mut [PolicyScore]) {
    let objectives: Vec<[f64; 6]> = scores.iter().map(PolicyScore::objectives).collect();
    let mut minima = [f64::INFINITY; 6];
    let mut maxima = [f64::NEG_INFINITY; 6];
    for values in &objectives {
        for index in 0..6 {
            minima[index] = minima[index].min(values[index]);
            maxima[index] = maxima[index].max(values[index]);
        }
    }
    for (score, values) in scores.iter_mut().zip(objectives) {
        let mut maximum: f64 = 0.0;
        for index in 0..6 {
            let span = maxima[index] - minima[index];
            let regret = if span <= f64::EPSILON {
                0.0
            } else {
                (values[index] - minima[index]) / span
            };
            maximum = maximum.max(regret);
            score
                .normalized_regrets
                .insert(OBJECTIVE_NAMES[index].to_owned(), regret);
        }
        score.maximum_regret = maximum;
    }
}

#[cfg(test)]
mod tests {
    use super::{PolicyScore, interval, mark_pareto_frontier};
    use std::collections::BTreeMap;

    fn score(id: &str, inflation: f64, output: f64) -> PolicyScore {
        PolicyScore {
            branch_id: id.to_owned(),
            inflation_loss: inflation,
            output_loss: output,
            unemployment_loss: 1.0,
            distribution_loss: 1.0,
            default_loss: 1.0,
            fiscal_cost: 1.0,
            objective_intervals: BTreeMap::new(),
            normalized_regrets: BTreeMap::new(),
            maximum_regret: 0.0,
            pareto_efficient: true,
        }
    }

    #[test]
    fn identifies_dominated_policy() {
        let mut scores = vec![
            score("a", 1.0, 1.0),
            score("b", 2.0, 2.0),
            score("c", 0.5, 3.0),
        ];
        mark_pareto_frontier(&mut scores);
        assert!(scores[0].pareto_efficient);
        assert!(!scores[1].pareto_efficient);
        assert!(scores[2].pareto_efficient);
    }

    #[test]
    fn confidence_interval_uses_replication_variation() {
        let result = interval(&[1.0, 3.0, 2.0], 0.90);
        assert_eq!(result.replication_count, 3);
        assert!((result.mean - 2.0).abs() < 1e-12);
        assert!(result.lower < result.mean);
        assert!(result.upper > result.mean);
    }
}
