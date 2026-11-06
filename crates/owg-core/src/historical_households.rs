use crate::contracts::Sector;
use crate::economy::Economy;
use crate::error::{OwgError, OwgResult};
use crate::money::Money;
use serde::{Deserialize, Serialize};

pub(crate) const FOREIGN_ORDER_SHARE: f64 = 0.14;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HouseholdDemandPolicy {
    pub source_sha256: String,
    pub average_spending_share: f64,
    pub income_response: f64,
    pub social_security_per_wage: f64,
}

impl HouseholdDemandPolicy {
    pub(crate) fn valid(&self) -> bool {
        self.source_sha256.len() == 64
            && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && (0.0..=1.0).contains(&self.average_spending_share)
            && self.average_spending_share > 0.0
            && (0.0..=1.0).contains(&self.income_response)
            && (0.0..=1.0).contains(&self.social_security_per_wage)
    }
}

#[derive(Debug)]
pub(crate) struct HouseholdDemand {
    policy: HouseholdDemandPolicy,
    reference_income: Vec<f64>,
    average_share: Vec<f64>,
    social_benefit: Money,
}

#[derive(Debug, Serialize)]
pub struct HouseholdDemandOpening {
    pub source_sha256: String,
    pub projected_disposable_income: f64,
    pub planned_household_purchases: f64,
    pub firm_plans_before: f64,
    pub firm_plans_after: f64,
    pub social_benefit_per_recipient: f64,
}

impl HouseholdDemand {
    pub(crate) fn initialize(
        policy: HouseholdDemandPolicy,
        economy: &mut Economy,
        outside: &[bool],
    ) -> OwgResult<(Self, HouseholdDemandOpening)> {
        let mut payroll = Money::ZERO;
        let mut reference_income = Vec::with_capacity(economy.households.len());
        for (index, employer) in economy.households.employer.iter().enumerate() {
            let income = if let Some(firm) = employer {
                let gross = economy.firms.wage_offer[*firm]
                    .mul_rate(0.85 + 0.15 * economy.households.skill[index])?;
                payroll = payroll.checked_add(gross)?;
                gross.checked_sub(gross.mul_rate(economy.parameters.income_tax_rate)?)?
            } else if outside[index] {
                Money::ZERO
            } else {
                economy.parameters.monthly_unemployment_benefit
            };
            reference_income.push(income.as_major_f64());
        }
        let outside_count = outside.iter().filter(|&&status| status).count().max(1) as f64;
        let social_benefit = payroll.mul_rate(policy.social_security_per_wage / outside_count)?;
        for (income, &inactive) in reference_income.iter_mut().zip(outside) {
            if inactive {
                *income = social_benefit.as_major_f64();
            }
        }
        let average_share: Vec<_> = economy
            .households
            .consumption_propensity
            .iter()
            .map(|propensity| (policy.average_spending_share + propensity - 0.77).clamp(0.05, 1.0))
            .collect();
        let planned_household_purchases: f64 = reference_income
            .iter()
            .zip(&average_share)
            .map(|(income, share)| income * share)
            .sum();
        let (firm_plans_before, firm_plans_after) =
            rebase_sales(economy, planned_household_purchases)?;
        let opening = HouseholdDemandOpening {
            source_sha256: policy.source_sha256.clone(),
            projected_disposable_income: reference_income.iter().sum(),
            planned_household_purchases,
            firm_plans_before,
            firm_plans_after,
            social_benefit_per_recipient: social_benefit.as_major_f64(),
        };
        Ok((
            Self {
                policy,
                reference_income,
                average_share,
                social_benefit,
            },
            opening,
        ))
    }

    pub(crate) fn budget(&self, index: usize, income: f64) -> f64 {
        let reference = self.reference_income[index];
        (self.average_share[index] * reference + self.policy.income_response * (income - reference))
            .max(0.0)
    }

    pub(crate) fn update(&mut self, incomes: &[f64]) {
        for (reference, &income) in self.reference_income.iter_mut().zip(incomes) {
            *reference = 0.85 * *reference + 0.15 * income;
        }
    }

    pub(crate) fn social_benefit(&self, previous_cpi: f64) -> OwgResult<Money> {
        self.social_benefit.mul_rate(previous_cpi / 100.0)
    }
}

fn rebase_sales(economy: &mut Economy, household_purchases: f64) -> OwgResult<(f64, f64)> {
    let mut nominal = [0.0; 4];
    let mut firms = [0_usize; 4];
    for (index, &sector) in economy.firms.sector.iter().enumerate() {
        nominal[sector.index()] +=
            economy.firms.expected_demand[index] * economy.firms.price[index].as_major_f64();
        firms[sector.index()] += 1;
    }
    let eligible = firms[Sector::Manufacturing.index()] + firms[Sector::Services.index()];
    let procurement_per_firm = economy
        .parameters
        .monthly_government_procurement
        .as_major_f64()
        / eligible as f64;
    let target = Sector::ALL.map(|sector| {
        let i = sector.index();
        let household = household_purchases * economy.parameters.sector.consumption_share[i]
            / (1.0 + economy.base_indirect_tax[i]);
        let public = if matches!(sector, Sector::Manufacturing | Sector::Services) {
            procurement_per_firm * firms[i] as f64
        } else {
            0.0
        };
        (household + public) / (1.0 - FOREIGN_ORDER_SHARE)
    });
    let scales = Sector::ALL.map(|sector| target[sector.index()] / nominal[sector.index()]);
    if scales
        .into_iter()
        .any(|scale| !scale.is_finite() || scale <= 0.0)
    {
        return Err(OwgError::InvalidState(
            "household opening has no positive sector sales plan".to_owned(),
        ));
    }
    for (index, &sector) in economy.firms.sector.iter().enumerate() {
        let scale = scales[sector.index()];
        economy.firms.expected_demand[index] *= scale;
        economy.firms.capacity[index] *= scale;
        economy.firms.inventory[index] *= scale;
        economy.firms.productivity[index] *= scale;
    }
    Ok((nominal.into_iter().sum(), target.into_iter().sum()))
}
