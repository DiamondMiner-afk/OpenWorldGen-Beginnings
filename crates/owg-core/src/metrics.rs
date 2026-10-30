use crate::error::OwgResult;
use crate::money::Money;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct IncomeAccounts {
    pub gross_value_added: f64,
    pub compensation_of_employees: f64,
    pub gross_operating_surplus: f64,
    pub taxes_on_products: f64,
    pub nominal_gdp_income: f64,
    pub identity_relative_error: f64,
}

impl IncomeAccounts {
    #[must_use]
    pub fn finite(&self) -> bool {
        [
            self.gross_value_added,
            self.compensation_of_employees,
            self.gross_operating_surplus,
            self.taxes_on_products,
            self.nominal_gdp_income,
            self.identity_relative_error,
        ]
        .into_iter()
        .all(f64::is_finite)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct InstitutionalBalances {
    pub households: Money,
    pub firms: Money,
    pub banks: Money,
    pub government: Money,
    pub central_bank: Money,
    pub rest_of_world: Money,
    pub ownership_sector: Money,
    pub unclassified: Money,
    pub residual: Money,
}

impl InstitutionalBalances {
    pub fn calculated_residual(&self) -> OwgResult<Money> {
        [
            self.households,
            self.firms,
            self.banks,
            self.government,
            self.central_bank,
            self.rest_of_world,
            self.ownership_sector,
            self.unclassified,
        ]
        .into_iter()
        .try_fold(Money::ZERO, Money::checked_add)
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct MetricRow {
    pub scenario_id: String,
    pub branch_id: String,
    pub replication: u32,
    pub month: u32,
    pub year: i32,
    pub month_of_year: u8,
    pub cpi: f64,
    pub cpi_energy: f64,
    pub cpi_food: f64,
    pub cpi_manufacturing: f64,
    pub cpi_services: f64,
    pub inflation_year_over_year: f64,
    pub real_gdp: f64,
    pub nominal_gdp: f64,
    #[serde(default)]
    pub income_accounts: IncomeAccounts,
    #[serde(default)]
    pub institutional_balances: InstitutionalBalances,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_credit: Option<crate::credit::ContractCreditMetrics>,
    pub output_gap: f64,
    pub unemployment: f64,
    pub average_wage: f64,
    pub consumption_total: f64,
    pub consumption_deciles: [f64; 10],
    pub gini: f64,
    pub poverty_share: f64,
    pub inventories: f64,
    pub firm_defaults: u32,
    pub firm_entries: u32,
    pub household_defaults: u32,
    pub bank_resolutions: u32,
    pub active_firms: u32,
    pub credit: f64,
    pub fiscal_balance: f64,
    pub government_debt: f64,
    pub imports: f64,
    pub policy_rate: f64,
    pub ledger_residual_minor_units: i64,
    pub accounting_identity_relative_error: f64,
}

impl MetricRow {
    #[must_use]
    pub fn finite(&self) -> bool {
        [
            self.cpi,
            self.cpi_energy,
            self.cpi_food,
            self.cpi_manufacturing,
            self.cpi_services,
            self.inflation_year_over_year,
            self.real_gdp,
            self.nominal_gdp,
            self.output_gap,
            self.unemployment,
            self.average_wage,
            self.consumption_total,
            self.gini,
            self.poverty_share,
            self.inventories,
            self.credit,
            self.fiscal_balance,
            self.government_debt,
            self.imports,
            self.policy_rate,
            self.accounting_identity_relative_error,
        ]
        .into_iter()
        .chain(self.consumption_deciles)
        .all(f64::is_finite)
            && self.income_accounts.finite()
            && self
                .institutional_balances
                .calculated_residual()
                .is_ok_and(|residual| residual == self.institutional_balances.residual)
    }
}

#[must_use]
pub fn gini(values: &[(f64, f64)]) -> f64 {
    let mut sorted: Vec<(f64, f64)> = values
        .iter()
        .copied()
        .filter(|(value, weight)| value.is_finite() && *weight > 0.0)
        .collect();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|left, right| left.0.total_cmp(&right.0));
    let total_weight: f64 = sorted.iter().map(|(_, weight)| weight).sum();
    let total_value: f64 = sorted.iter().map(|(value, weight)| value * weight).sum();
    if total_value <= 0.0 || total_weight <= 0.0 {
        return 0.0;
    }

    let mut cumulative_weight = 0.0;
    let mut cumulative_value = 0.0;
    let mut area = 0.0;
    for (value, weight) in sorted {
        let previous_weight_share = cumulative_weight / total_weight;
        let previous_value_share = cumulative_value / total_value;
        cumulative_weight += weight;
        cumulative_value += value * weight;
        let weight_share = cumulative_weight / total_weight;
        let value_share = cumulative_value / total_value;
        area += (value_share + previous_value_share) * (weight_share - previous_weight_share) / 2.0;
    }
    (1.0 - 2.0 * area).clamp(0.0, 1.0)
}

#[must_use]
pub fn weighted_deciles(values: &[(f64, f64)]) -> [f64; 10] {
    let mut sorted: Vec<(f64, f64)> = values
        .iter()
        .copied()
        .filter(|(value, weight)| value.is_finite() && *weight > 0.0)
        .collect();
    sorted.sort_by(|left, right| left.0.total_cmp(&right.0));
    let total_weight: f64 = sorted.iter().map(|(_, weight)| weight).sum();
    if total_weight <= 0.0 {
        return [0.0; 10];
    }

    let mut output = [0.0; 10];
    let mut weights = [0.0; 10];
    let mut cumulative_weight = 0.0;
    for (value, weight) in sorted {
        let midpoint = (cumulative_weight + weight / 2.0) / total_weight;
        let decile = ((midpoint * 10.0).floor() as usize).min(9);
        output[decile] += value * weight;
        weights[decile] += weight;
        cumulative_weight += weight;
    }
    for (value, weight) in output.iter_mut().zip(weights) {
        if weight > 0.0 {
            *value /= weight;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{gini, weighted_deciles};

    #[test]
    fn equality_has_zero_gini() {
        assert!(gini(&[(10.0, 1.0), (10.0, 1.0), (10.0, 1.0)]) < 1e-12);
    }

    #[test]
    fn deciles_are_monotonic_for_ordered_values() {
        let values: Vec<(f64, f64)> = (1..=100).map(|value| (f64::from(value), 1.0)).collect();
        let deciles = weighted_deciles(&values);
        assert!(deciles.windows(2).all(|window| window[0] <= window[1]));
    }
}
