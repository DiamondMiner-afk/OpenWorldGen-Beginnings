use crate::contracts::{PolicyActionKind, PolicyBranch, ScenarioSpec, Sector, ShockKind};
use crate::data::SectorParameters;
use crate::error::{OwgError, OwgResult};
use crate::governance::GovernanceState;
use crate::historical_households::HouseholdDemand;
use crate::historical_labor::HistoricalLabor;
use crate::historical_production::InventoryPolicy;
use crate::institutions::{EconomicRole, InstitutionGraph, OwnershipStake};
use crate::ledger::{AccountKey, AccountKind, JournalEntry, Ledger, Posting};
use crate::metrics::{IncomeAccounts, InstitutionalBalances, MetricRow, gini, weighted_deciles};
use crate::model::ModelPack;
use crate::money::Money;
use crate::rng::SeedBook;
use crate::scheduler::{MultiRateScheduler, ScheduleAudit, ScheduledEvent, SimulationEventKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::array;
use std::collections::BTreeSet;

pub(crate) const GOVERNMENT_ID: &str = "GOVERNMENT";
pub(crate) const CENTRAL_BANK_ID: &str = "CENTRAL_BANK";
pub(crate) const REST_OF_WORLD_ID: &str = "REST_OF_WORLD";
pub(crate) const OWNERSHIP_SECTOR_ID: &str = "OWNERSHIP_SECTOR";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Households {
    pub ids: Vec<String>,
    pub weights: Vec<f64>,
    pub bank_index: Vec<usize>,
    pub employer: Vec<Option<usize>>,
    pub skill: Vec<f64>,
    pub consumption_propensity: Vec<f64>,
    pub habit_strength: Vec<f64>,
    pub risk_aversion: Vec<f64>,
    pub information_access: Vec<f64>,
    pub trust: Vec<f64>,
    pub expected_inflation: Vec<f64>,
    pub habit_consumption: Vec<f64>,
    pub last_consumption: Vec<f64>,
    pub monthly_income: Vec<f64>,
    pub delinquent_months: Vec<u16>,
}

impl Households {
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Firms {
    pub ids: Vec<String>,
    pub sector: Vec<Sector>,
    pub bank_index: Vec<usize>,
    pub productivity: Vec<f64>,
    pub capacity: Vec<f64>,
    pub inventory: Vec<f64>,
    pub price: Vec<Money>,
    pub wage_offer: Vec<Money>,
    pub expected_demand: Vec<f64>,
    pub last_sales_units: Vec<f64>,
    pub last_sales_value: Vec<f64>,
    pub markup: Vec<f64>,
    pub energy_intensity: Vec<f64>,
    pub active: Vec<bool>,
    pub default_streak: Vec<u16>,
    pub inactive_months: Vec<u16>,
}

impl Firms {
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Banks {
    pub ids: Vec<String>,
    pub lending_spread: Vec<f64>,
    pub approval_threshold: Vec<f64>,
    pub expected_loss_rate: Vec<f64>,
    pub minimum_capital_ratio: Vec<f64>,
    pub minimum_liquidity_ratio: Vec<f64>,
}

impl Banks {
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EconomicParameters {
    pub baseline_policy_rate: f64,
    pub income_tax_rate: f64,
    pub monthly_unemployment_benefit: Money,
    pub monthly_government_procurement: Money,
    pub firm_entry_monthly_probability_after_wait: f64,
    pub firm_entry_wait_months: u32,
    pub household_credit_income_multiple: f64,
    pub household_credit_minimum_limit: f64,
    pub household_debt_monthly_principal_rate: f64,
    pub household_credit_spread: f64,
    pub weekly_energy_subsistence_floor: f64,
    pub weekly_food_subsistence_floor: f64,
    pub bank_minimum_capital_ratio: f64,
    pub bank_minimum_liquidity_ratio: f64,
    pub sector: SectorParameters,
}

impl EconomicParameters {
    pub fn from_inputs(model: &ModelPack, sector: SectorParameters) -> OwgResult<Self> {
        let value = |id: &str| {
            model.parameters.get(id).copied().ok_or_else(|| {
                OwgError::InvalidState(format!("model pack is missing required parameter {id}"))
            })
        };
        let firm_entry_wait_months = value("firm_entry_wait_months")?;
        if firm_entry_wait_months.fract() != 0.0
            || !(0.0..=f64::from(u32::MAX)).contains(&firm_entry_wait_months)
        {
            return Err(OwgError::InvalidState(
                "firm_entry_wait_months must be a non-negative integer".to_owned(),
            ));
        }
        let parameters = Self {
            baseline_policy_rate: value("baseline_policy_rate")?,
            income_tax_rate: value("income_tax_rate")?,
            monthly_unemployment_benefit: Money::from_major_f64(value(
                "monthly_unemployment_benefit",
            )?)?,
            monthly_government_procurement: Money::from_major_f64(value(
                "monthly_government_procurement",
            )?)?,
            firm_entry_monthly_probability_after_wait: value(
                "firm_entry_monthly_probability_after_wait",
            )?,
            firm_entry_wait_months: firm_entry_wait_months as u32,
            household_credit_income_multiple: value("household_credit_income_multiple")?,
            household_credit_minimum_limit: value("household_credit_minimum_limit")?,
            household_debt_monthly_principal_rate: value("household_debt_monthly_principal_rate")?,
            household_credit_spread: value("household_credit_spread")?,
            weekly_energy_subsistence_floor: value("weekly_energy_subsistence_floor")?,
            weekly_food_subsistence_floor: value("weekly_food_subsistence_floor")?,
            bank_minimum_capital_ratio: value("bank_minimum_capital_ratio")?,
            bank_minimum_liquidity_ratio: value("bank_minimum_liquidity_ratio")?,
            sector,
        };
        parameters.validate()?;
        Ok(parameters)
    }

    fn validate(&self) -> OwgResult<()> {
        let rates = [
            self.baseline_policy_rate,
            self.income_tax_rate,
            self.firm_entry_monthly_probability_after_wait,
            self.household_debt_monthly_principal_rate,
            self.household_credit_spread,
            self.bank_minimum_capital_ratio,
            self.bank_minimum_liquidity_ratio,
        ];
        if rates
            .iter()
            .any(|rate| !rate.is_finite() || !(0.0..=1.0).contains(rate))
        {
            return Err(OwgError::InvalidState(
                "model pack rates must be finite and in [0, 1]".to_owned(),
            ));
        }
        let positive = [
            self.household_credit_income_multiple,
            self.household_credit_minimum_limit,
            self.weekly_energy_subsistence_floor,
            self.weekly_food_subsistence_floor,
        ];
        if positive
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(OwgError::InvalidState(
                "model pack scale parameters must be finite and non-negative".to_owned(),
            ));
        }
        Ok(())
    }

    fn development_defaults() -> Self {
        Self {
            baseline_policy_rate: 0.04,
            income_tax_rate: 0.10,
            monthly_unemployment_benefit: Money(14_000),
            monthly_government_procurement: Money(7_000_000),
            firm_entry_monthly_probability_after_wait: 0.20,
            firm_entry_wait_months: 6,
            household_credit_income_multiple: 2.5,
            household_credit_minimum_limit: 400.0,
            household_debt_monthly_principal_rate: 0.02,
            household_credit_spread: 0.02,
            weekly_energy_subsistence_floor: 10.0,
            weekly_food_subsistence_floor: 30.0,
            bank_minimum_capital_ratio: 0.08,
            bank_minimum_liquidity_ratio: 0.10,
            sector: SectorParameters {
                base_price_major: [16.0, 12.0, 24.0, 20.0],
                energy_intensity: [0.30, 0.16, 0.24, 0.07],
                consumption_share: [0.10, 0.30, 0.25, 0.35],
                indirect_tax_rate: [0.08, 0.05, 0.10, 0.12],
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
struct MonthFlows {
    opening_inventory_value: f64,
    inventory_losses_value: f64,
    production_nominal: f64,
    production_real: f64,
    real_import_inputs: f64,
    unused_service_slots_real: f64,
    consumption_producer: f64,
    consumption_tax: f64,
    consumption_total: f64,
    consumption_by_sector: [f64; 4],
    government_purchases: f64,
    exports: f64,
    imports: f64,
    wages: f64,
    income_tax: f64,
    transfers: f64,
    social_benefits: f64,
    household_purchase_intent: f64,
    new_credit: f64,
    principal_repaid: f64,
    interest_paid: f64,
    firm_defaults: u32,
    firm_entries: u32,
    household_defaults: u32,
    bank_resolutions: u32,
    bank_resolution_cost: f64,
    sector_sales_value: [f64; 4],
    sector_sales_units: [f64; 4],
}

#[derive(Debug, Serialize)]
pub struct ProductionFlows {
    pub gross_output_fixed_prices: f64,
    pub imported_inputs_fixed_prices: f64,
    pub value_added_fixed_prices: f64,
    pub unused_service_slots_fixed_prices: f64,
    pub inventory_opening_nominal: f64,
    pub inventory_closing_nominal: f64,
    pub government_purchases: f64,
    pub exports: f64,
    pub social_benefits: f64,
    pub household_purchase_intent: f64,
    pub household_disposable_income: f64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Economy {
    pub ledger: Ledger,
    pub households: Households,
    pub firms: Firms,
    pub banks: Banks,
    pub institutions: InstitutionGraph,
    pub governance: Option<GovernanceState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_credit: Option<crate::credit::CreditBook>,
    pub parameters: EconomicParameters,
    pub month: u32,
    pub replication: u32,
    pub root_seed: u64,
    pub base_policy_rate: f64,
    pub effective_policy_rate: f64,
    pub credit_spread_shock: f64,
    pub base_indirect_tax: [f64; 4],
    pub energy_efficiency_multiplier: f64,
    pub cpi_history: Vec<f64>,
    pub last_inflation: f64,
    pub last_unemployment: f64,
    pub potential_gdp: f64,
    pub base_sector_prices: [f64; 4],
    pub journal_sequence: u64,
    #[serde(default)]
    pub schedule_audit: ScheduleAudit,
}

/// A firm-initiated cash transfer to one of its direct household owners.
///
/// This is a settlement instruction, not a dividend, profit calculation,
/// tax determination, or proof of real-world corporate-law authority. The
/// authorizing simulation actor must be the paying firm itself; an actor
/// policy and legal-governance layer must decide whether to issue it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DirectOwnerCashTransfer {
    pub authorizing_entity_id: String,
    pub paying_firm_id: String,
    pub recipient_owner_id: String,
    pub amount: Money,
}

impl Economy {
    pub fn new(scenario: &ScenarioSpec, replication: u32) -> OwgResult<Self> {
        let parameters = EconomicParameters::development_defaults();
        Self::new_with_parameters(scenario, replication, &parameters)
    }

    pub fn new_with_parameters(
        scenario: &ScenarioSpec,
        replication: u32,
        parameters: &EconomicParameters,
    ) -> OwgResult<Self> {
        scenario.validated()?;
        parameters.validate()?;
        let replication_seed = scenario
            .root_seed
            .wrapping_add(u64::from(replication).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let seeds = SeedBook::new(replication_seed);

        let banks = Banks {
            ids: (0..scenario.population.banks)
                .map(|index| format!("BANK-{index:03}"))
                .collect(),
            lending_spread: (0..scenario.population.banks)
                .map(|index| seeds.range("bank-spread", index as u64, 0, 0, 0.025, 0.045))
                .collect(),
            approval_threshold: (0..scenario.population.banks)
                .map(|index| seeds.range("bank-threshold", index as u64, 0, 0, 0.45, 0.65))
                .collect(),
            expected_loss_rate: vec![0.02; scenario.population.banks],
            minimum_capital_ratio: vec![
                parameters.bank_minimum_capital_ratio;
                scenario.population.banks
            ],
            minimum_liquidity_ratio: vec![
                parameters.bank_minimum_liquidity_ratio;
                scenario.population.banks
            ],
        };

        let firm_count = scenario.population.firms;
        let mut firms = Firms {
            ids: Vec::with_capacity(firm_count),
            sector: Vec::with_capacity(firm_count),
            bank_index: Vec::with_capacity(firm_count),
            productivity: Vec::with_capacity(firm_count),
            capacity: Vec::with_capacity(firm_count),
            inventory: Vec::with_capacity(firm_count),
            price: Vec::with_capacity(firm_count),
            wage_offer: Vec::with_capacity(firm_count),
            expected_demand: Vec::with_capacity(firm_count),
            last_sales_units: vec![0.0; firm_count],
            last_sales_value: vec![0.0; firm_count],
            markup: Vec::with_capacity(firm_count),
            energy_intensity: Vec::with_capacity(firm_count),
            active: vec![true; firm_count],
            default_streak: vec![0; firm_count],
            inactive_months: vec![0; firm_count],
        };
        for index in 0..firm_count {
            let sector = Sector::ALL[index % Sector::ALL.len()];
            firms.ids.push(format!("FIRM-{index:05}"));
            firms.sector.push(sector);
            firms.bank_index.push(index % banks.len());
            firms.productivity.push(seeds.range(
                "firm-productivity",
                index as u64,
                0,
                0,
                38.0,
                52.0,
            ));
            firms
                .capacity
                .push(seeds.range("firm-capacity", index as u64, 0, 0, 420.0, 650.0));
            firms
                .inventory
                .push(seeds.range("firm-inventory", index as u64, 0, 0, 260.0, 420.0));
            firms.price.push(Money::from_major_f64(
                parameters.sector.base_price_major[sector.index()]
                    * seeds.range("firm-price", index as u64, 0, 0, 0.94, 1.06),
            )?);
            firms.wage_offer.push(Money::from_major_f64(seeds.range(
                "firm-wage",
                index as u64,
                0,
                0,
                460.0,
                560.0,
            ))?);
            firms.expected_demand.push(seeds.range(
                "firm-demand",
                index as u64,
                0,
                0,
                300.0,
                430.0,
            ));
            firms
                .markup
                .push(seeds.range("firm-markup", index as u64, 0, 0, 0.16, 0.28));
            firms.energy_intensity.push(
                parameters.sector.energy_intensity[sector.index()]
                    * seeds.range("firm-energy", index as u64, 0, 0, 0.85, 1.15),
            );
        }

        let household_count = scenario.population.households;
        let mut households = Households {
            ids: Vec::with_capacity(household_count),
            weights: vec![scenario.population.household_weight; household_count],
            bank_index: Vec::with_capacity(household_count),
            employer: Vec::with_capacity(household_count),
            skill: Vec::with_capacity(household_count),
            consumption_propensity: Vec::with_capacity(household_count),
            habit_strength: Vec::with_capacity(household_count),
            risk_aversion: Vec::with_capacity(household_count),
            information_access: Vec::with_capacity(household_count),
            trust: Vec::with_capacity(household_count),
            expected_inflation: vec![0.02; household_count],
            habit_consumption: vec![340.0; household_count],
            last_consumption: vec![340.0; household_count],
            monthly_income: vec![0.0; household_count],
            delinquent_months: vec![0; household_count],
        };
        for index in 0..household_count {
            households.ids.push(format!("HH-{index:06}"));
            households.bank_index.push(index % banks.len());
            let employed = seeds.unit("initial-employment", index as u64, 0, 0) < 0.94;
            households
                .employer
                .push(employed.then_some(index % firm_count));
            households
                .skill
                .push(seeds.range("household-skill", index as u64, 0, 0, 0.65, 1.35));
            households.consumption_propensity.push(seeds.range(
                "household-mpc",
                index as u64,
                0,
                0,
                0.68,
                0.86,
            ));
            households.habit_strength.push(seeds.range(
                "household-habit",
                index as u64,
                0,
                0,
                0.45,
                0.82,
            ));
            households.risk_aversion.push(seeds.range(
                "household-risk",
                index as u64,
                0,
                0,
                0.25,
                0.85,
            ));
            households.information_access.push(seeds.range(
                "household-information",
                index as u64,
                0,
                0,
                0.25,
                0.95,
            ));
            households
                .trust
                .push(seeds.range("household-trust", index as u64, 0, 0, 0.30, 0.90));
        }

        let institutions = if let Some(graph) = &scenario.initial_institutions {
            graph.clone()
        } else {
            InstitutionGraph::synthetic_firm_ownership(&households.ids, &firms.ids)?
        };
        institutions.validate_population(&households.ids, &firms.ids)?;
        let governance = scenario
            .governance
            .as_ref()
            .map(|spec| {
                GovernanceState::new_with_institutions(
                    spec,
                    scenario.calendar.months,
                    &institutions,
                    &scenario.geography.region_id,
                )
            })
            .transpose()?;
        let mut economy = Self {
            ledger: Ledger::default(),
            households,
            firms,
            banks,
            institutions,
            governance,
            contract_credit: scenario
                .credit_market
                .clone()
                .map(crate::credit::CreditBook::new),
            parameters: parameters.clone(),
            month: 0,
            replication,
            root_seed: replication_seed,
            base_policy_rate: parameters.baseline_policy_rate,
            effective_policy_rate: parameters.baseline_policy_rate,
            credit_spread_shock: 0.0,
            base_indirect_tax: parameters.sector.indirect_tax_rate,
            energy_efficiency_multiplier: 1.0,
            cpi_history: Vec::with_capacity(scenario.calendar.months as usize),
            last_inflation: 0.02,
            last_unemployment: 0.06,
            potential_gdp: 0.0,
            base_sector_prices: [0.0; 4],
            journal_sequence: 0,
            schedule_audit: ScheduleAudit::default(),
        };
        economy.assert_population_structure()?;
        economy.initialize_opening_balances(seeds)?;
        economy.base_sector_prices = economy.average_sector_prices();
        economy.ledger.assert_balanced()?;
        economy.assert_reserve_claims_balanced()?;
        economy.assert_deposit_claims_balanced()?;
        economy.assert_loan_claims_balanced()?;
        Ok(economy)
    }

    /// Commercial-bank reserve assets must equal the central bank's reserve
    /// liability. This stock identity is distinct from global journal closure:
    /// an unpaired reserve posting can still be hidden by an unrelated account.
    pub fn assert_reserve_claims_balanced(&self) -> OwgResult<()> {
        let mut reserve_assets = Money::ZERO;
        for bank_id in &self.banks.ids {
            reserve_assets = reserve_assets.checked_add(
                self.ledger
                    .balance(&AccountKey::new(bank_id, AccountKind::Reserve)),
            )?;
        }
        let reserve_liability = self.ledger.balance(&AccountKey::new(
            CENTRAL_BANK_ID,
            AccountKind::ReserveLiability,
        ));
        let residual = reserve_assets.checked_add(reserve_liability)?;
        if residual != Money::ZERO {
            return Err(OwgError::InvalidState(format!(
                "commercial-bank reserves and central-bank liability differ by {residual}"
            )));
        }
        Ok(())
    }

    /// A bank's aggregate deposit liability must be the exact counterpart of
    /// deposits held by the customers assigned to that bank. Global journal
    /// closure and aggregate reserves cannot detect a wrong-bank posting.
    pub fn assert_deposit_claims_balanced(&self) -> OwgResult<()> {
        self.assert_population_structure()?;
        let mut by_bank = vec![Money::ZERO; self.banks.len()];
        let mut known_owners = BTreeSet::new();
        for (owner_id, &bank_index) in self.households.ids.iter().zip(&self.households.bank_index) {
            known_owners.insert(owner_id.as_str());
            let deposit = self
                .ledger
                .balance(&AccountKey::new(owner_id, AccountKind::Deposit));
            if deposit < Money::ZERO {
                return Err(OwgError::InvalidState(format!(
                    "negative customer deposit for {owner_id}"
                )));
            }
            by_bank[bank_index] = by_bank[bank_index].checked_add(deposit)?;
        }
        for (owner_id, &bank_index) in self.firms.ids.iter().zip(&self.firms.bank_index) {
            known_owners.insert(owner_id.as_str());
            let deposit = self
                .ledger
                .balance(&AccountKey::new(owner_id, AccountKind::Deposit));
            if deposit < Money::ZERO {
                return Err(OwgError::InvalidState(format!(
                    "negative customer deposit for {owner_id}"
                )));
            }
            by_bank[bank_index] = by_bank[bank_index].checked_add(deposit)?;
        }
        known_owners.insert(REST_OF_WORLD_ID);
        let rest_of_world_deposit = self
            .ledger
            .balance(&AccountKey::new(REST_OF_WORLD_ID, AccountKind::Deposit));
        if rest_of_world_deposit < Money::ZERO {
            return Err(OwgError::InvalidState(
                "negative rest-of-world deposit".to_owned(),
            ));
        }
        by_bank[0] = by_bank[0].checked_add(rest_of_world_deposit)?;

        let bank_ids: BTreeSet<&str> = self.banks.ids.iter().map(String::as_str).collect();
        for account in self.ledger.balances().keys() {
            if account.kind == AccountKind::Deposit
                && !known_owners.contains(account.owner_id.as_str())
            {
                return Err(OwgError::InvalidState(format!(
                    "deposit account has no assigned bank: {}",
                    account.label()
                )));
            }
            if account.kind == AccountKind::DepositLiability
                && account.owner_id != CENTRAL_BANK_ID
                && !bank_ids.contains(account.owner_id.as_str())
            {
                return Err(OwgError::InvalidState(format!(
                    "deposit liability has no declared bank: {}",
                    account.label()
                )));
            }
        }
        for (index, bank_id) in self.banks.ids.iter().enumerate() {
            let liability = self
                .ledger
                .balance(&AccountKey::new(bank_id, AccountKind::DepositLiability));
            if liability > Money::ZERO {
                return Err(OwgError::InvalidState(format!(
                    "positive commercial-bank deposit liability for {bank_id}"
                )));
            }
            let residual = by_bank[index].checked_add(liability)?;
            if residual != Money::ZERO {
                return Err(OwgError::InvalidState(format!(
                    "bank {bank_id} deposit claims differ by {residual}"
                )));
            }
        }
        Ok(())
    }

    /// Each bank's loan asset must be the counterpart of debts owed by its
    /// assigned household and firm borrowers. An aggregate loan/debt check
    /// would miss an asset silently moved to the wrong bank.
    pub fn assert_loan_claims_balanced(&self) -> OwgResult<()> {
        self.assert_population_structure()?;
        let mut by_bank = vec![Money::ZERO; self.banks.len()];
        let mut known_borrowers = BTreeSet::new();
        for (borrower_id, &bank_index) in
            self.households.ids.iter().zip(&self.households.bank_index)
        {
            known_borrowers.insert(borrower_id.as_str());
            let liability = self
                .ledger
                .balance(&AccountKey::new(borrower_id, AccountKind::DebtLiability));
            if liability > Money::ZERO {
                return Err(OwgError::InvalidState(format!(
                    "positive borrower debt liability for {borrower_id}"
                )));
            }
            by_bank[bank_index] = by_bank[bank_index].checked_sub(liability)?;
        }
        for (borrower_id, &bank_index) in self.firms.ids.iter().zip(&self.firms.bank_index) {
            known_borrowers.insert(borrower_id.as_str());
            let liability = self
                .ledger
                .balance(&AccountKey::new(borrower_id, AccountKind::DebtLiability));
            if liability > Money::ZERO {
                return Err(OwgError::InvalidState(format!(
                    "positive borrower debt liability for {borrower_id}"
                )));
            }
            by_bank[bank_index] = by_bank[bank_index].checked_sub(liability)?;
        }

        let bank_ids: BTreeSet<&str> = self.banks.ids.iter().map(String::as_str).collect();
        for account in self.ledger.balances().keys() {
            if account.kind == AccountKind::DebtLiability
                && !known_borrowers.contains(account.owner_id.as_str())
            {
                return Err(OwgError::InvalidState(format!(
                    "debt liability has no assigned bank: {}",
                    account.label()
                )));
            }
            if account.kind == AccountKind::LoanAsset
                && !bank_ids.contains(account.owner_id.as_str())
            {
                return Err(OwgError::InvalidState(format!(
                    "loan asset has no declared bank: {}",
                    account.label()
                )));
            }
        }
        for (index, bank_id) in self.banks.ids.iter().enumerate() {
            let asset = self
                .ledger
                .balance(&AccountKey::new(bank_id, AccountKind::LoanAsset));
            if asset < Money::ZERO {
                return Err(OwgError::InvalidState(format!(
                    "negative commercial-bank loan asset for {bank_id}"
                )));
            }
            let residual = asset.checked_sub(by_bank[index])?;
            if residual != Money::ZERO {
                return Err(OwgError::InvalidState(format!(
                    "bank {bank_id} loan claims differ by {residual}"
                )));
            }
        }
        self.assert_contract_credit_balanced()?;
        Ok(())
    }

    /// Reject malformed deserialized or externally edited columnar populations
    /// before any indexed agent operation can panic or settle against the wrong bank.
    pub fn assert_population_structure(&self) -> OwgResult<()> {
        let household_count = self.households.len();
        let firm_count = self.firms.len();
        let bank_count = self.banks.len();
        if household_count == 0 || firm_count == 0 || bank_count == 0 {
            return Err(OwgError::InvalidState(
                "economy must contain households, firms, and banks".to_owned(),
            ));
        }
        let household_columns = [
            ("weights", self.households.weights.len()),
            ("bank_index", self.households.bank_index.len()),
            ("employer", self.households.employer.len()),
            ("skill", self.households.skill.len()),
            (
                "consumption_propensity",
                self.households.consumption_propensity.len(),
            ),
            ("habit_strength", self.households.habit_strength.len()),
            ("risk_aversion", self.households.risk_aversion.len()),
            (
                "information_access",
                self.households.information_access.len(),
            ),
            ("trust", self.households.trust.len()),
            (
                "expected_inflation",
                self.households.expected_inflation.len(),
            ),
            ("habit_consumption", self.households.habit_consumption.len()),
            ("last_consumption", self.households.last_consumption.len()),
            ("monthly_income", self.households.monthly_income.len()),
            ("delinquent_months", self.households.delinquent_months.len()),
        ];
        for (column, len) in household_columns {
            if len != household_count {
                return Err(OwgError::InvalidState(format!(
                    "household column {column} has {len} rows; expected {household_count}"
                )));
            }
        }
        let firm_columns = [
            ("sector", self.firms.sector.len()),
            ("bank_index", self.firms.bank_index.len()),
            ("productivity", self.firms.productivity.len()),
            ("capacity", self.firms.capacity.len()),
            ("inventory", self.firms.inventory.len()),
            ("price", self.firms.price.len()),
            ("wage_offer", self.firms.wage_offer.len()),
            ("expected_demand", self.firms.expected_demand.len()),
            ("last_sales_units", self.firms.last_sales_units.len()),
            ("last_sales_value", self.firms.last_sales_value.len()),
            ("markup", self.firms.markup.len()),
            ("energy_intensity", self.firms.energy_intensity.len()),
            ("active", self.firms.active.len()),
            ("default_streak", self.firms.default_streak.len()),
            ("inactive_months", self.firms.inactive_months.len()),
        ];
        for (column, len) in firm_columns {
            if len != firm_count {
                return Err(OwgError::InvalidState(format!(
                    "firm column {column} has {len} rows; expected {firm_count}"
                )));
            }
        }
        let bank_columns = [
            ("lending_spread", self.banks.lending_spread.len()),
            ("approval_threshold", self.banks.approval_threshold.len()),
            ("expected_loss_rate", self.banks.expected_loss_rate.len()),
            (
                "minimum_capital_ratio",
                self.banks.minimum_capital_ratio.len(),
            ),
            (
                "minimum_liquidity_ratio",
                self.banks.minimum_liquidity_ratio.len(),
            ),
        ];
        for (column, len) in bank_columns {
            if len != bank_count {
                return Err(OwgError::InvalidState(format!(
                    "bank column {column} has {len} rows; expected {bank_count}"
                )));
            }
        }
        if self
            .households
            .bank_index
            .iter()
            .chain(&self.firms.bank_index)
            .any(|&index| index >= bank_count)
            || self
                .households
                .employer
                .iter()
                .flatten()
                .any(|&index| index >= firm_count)
        {
            return Err(OwgError::InvalidState(
                "agent bank or employer index is out of range".to_owned(),
            ));
        }
        let mut ids = BTreeSet::new();
        for id in self
            .households
            .ids
            .iter()
            .chain(&self.firms.ids)
            .chain(&self.banks.ids)
        {
            if id.trim().is_empty()
                || [
                    GOVERNMENT_ID,
                    CENTRAL_BANK_ID,
                    REST_OF_WORLD_ID,
                    OWNERSHIP_SECTOR_ID,
                ]
                .contains(&id.as_str())
                || !ids.insert(id)
            {
                return Err(OwgError::InvalidState(format!(
                    "empty, reserved, or duplicate agent ID {id}"
                )));
            }
        }
        Ok(())
    }

    /// Change initial ownership before the first simulated month. Subsequent
    /// acquisitions require cash and securities settlement, not a graph edit.
    pub fn set_initial_firm_ownership(
        &mut self,
        firm_id: &str,
        stakes: Vec<OwnershipStake>,
    ) -> OwgResult<()> {
        if self.month != 0
            || self
                .ledger
                .audit()
                .entries_by_phase
                .get("ownership")
                .is_some_and(|&count| count > 0)
            || !self.firms.ids.iter().any(|id| id == firm_id)
        {
            return Err(OwgError::InvalidState(
                "initial ownership may only target a known firm before month zero closes"
                    .to_owned(),
            ));
        }
        self.institutions.set_initial_ownership(firm_id, stakes)
    }

    /// Settle a proposed firm-authorized transfer of already-held cash.
    /// `acting_entity_id` must come from a trusted actor dispatch, not from
    /// the proposal itself; this method does not verify legal board authority.
    /// Only bound household recipients are supported until non-agent legal
    /// entities have their own bank accounts and balance sheets.
    pub fn settle_direct_owner_cash_transfer(
        &mut self,
        acting_entity_id: &str,
        request: &DirectOwnerCashTransfer,
    ) -> OwgResult<()> {
        self.assert_population_structure()?;
        self.assert_deposit_claims_balanced()?;
        self.assert_loan_claims_balanced()?;
        if request.amount <= Money::ZERO {
            return Err(OwgError::InvalidState(
                "owner cash transfer must be positive".to_owned(),
            ));
        }
        if acting_entity_id != request.paying_firm_id
            || request.authorizing_entity_id != request.paying_firm_id
        {
            return Err(OwgError::InvalidState(
                "owner cash transfer must be authorized by the paying firm actor".to_owned(),
            ));
        }
        self.institutions
            .validate_population(&self.households.ids, &self.firms.ids)?;
        let firm_index = self
            .firms
            .ids
            .iter()
            .position(|id| id == &request.paying_firm_id)
            .ok_or_else(|| {
                OwgError::InvalidState("owner cash transfer payer is not a bound firm".to_owned())
            })?;
        let owner_index = self
            .households
            .ids
            .iter()
            .position(|id| id == &request.recipient_owner_id)
            .ok_or_else(|| {
                OwgError::InvalidState(
                    "owner cash transfer recipient is not a banked household".to_owned(),
                )
            })?;
        let payer_role = self
            .institutions
            .entities
            .iter()
            .find(|entity| entity.id == request.paying_firm_id)
            .map(|entity| entity.economic_role);
        let recipient_role = self
            .institutions
            .entities
            .iter()
            .find(|entity| entity.id == request.recipient_owner_id)
            .map(|entity| entity.economic_role);
        if payer_role != Some(EconomicRole::OperatingFirm)
            || recipient_role != Some(EconomicRole::Household)
            || !self.institutions.stakes.iter().any(|stake| {
                stake.owned_id == request.paying_firm_id
                    && stake.owner_id == request.recipient_owner_id
                    && stake.basis_points > 0
            })
        {
            return Err(OwgError::InvalidState(
                "owner cash transfer requires a direct positive ownership stake".to_owned(),
            ));
        }
        if !self.firms.active[firm_index] || self.firms.default_streak[firm_index] > 0 {
            return Err(OwgError::InvalidState(
                "inactive or defaulting firm cannot make an owner cash transfer".to_owned(),
            ));
        }
        let payer_deposit = AccountKey::new(&request.paying_firm_id, AccountKind::Deposit);
        let recipient_deposit = AccountKey::new(&request.recipient_owner_id, AccountKind::Deposit);
        if self.ledger.balance(&payer_deposit) < request.amount {
            return Err(OwgError::InvalidState(
                "insufficient firm deposit for owner cash transfer".to_owned(),
            ));
        }
        self.ledger
            .balance(&recipient_deposit)
            .checked_add(request.amount)?;
        let mut postings = vec![
            Posting::new(payer_deposit, -request.amount),
            Posting::new(recipient_deposit, request.amount),
        ];
        let payer_bank_index = self.firms.bank_index[firm_index];
        let recipient_bank_index = self.households.bank_index[owner_index];
        if payer_bank_index != recipient_bank_index {
            let payer_bank = &self.banks.ids[payer_bank_index];
            let recipient_bank = &self.banks.ids[recipient_bank_index];
            let payer_reserve = AccountKey::new(payer_bank, AccountKind::Reserve);
            let recipient_reserve = AccountKey::new(recipient_bank, AccountKind::Reserve);
            if self.ledger.balance(&payer_reserve) < request.amount {
                return Err(OwgError::InvalidState(
                    "payer bank lacks reserves for owner cash transfer".to_owned(),
                ));
            }
            self.ledger
                .balance(&recipient_reserve)
                .checked_add(request.amount)?;
            let payer_liability = AccountKey::new(payer_bank, AccountKind::DepositLiability);
            let recipient_liability =
                AccountKey::new(recipient_bank, AccountKind::DepositLiability);
            self.ledger
                .balance(&payer_liability)
                .checked_add(request.amount)?;
            self.ledger
                .balance(&recipient_liability)
                .checked_sub(request.amount)?;
            postings.extend([
                Posting::new(payer_liability, request.amount),
                Posting::new(recipient_liability, -request.amount),
                Posting::new(payer_reserve, -request.amount),
                Posting::new(recipient_reserve, request.amount),
            ]);
        }
        let next_sequence = self
            .journal_sequence
            .checked_add(1)
            .ok_or_else(|| OwgError::InvalidState("journal sequence overflow".to_owned()))?;
        let entry_id = format!(
            "owner-cash-transfer-r{}-m{}-{}",
            self.replication, self.month, self.journal_sequence
        );
        self.ledger.post(JournalEntry::new(
            entry_id,
            self.month,
            "ownership",
            "direct-owner cash transfer; profit and tax basis not assessed",
            postings,
        ))?;
        self.journal_sequence = next_sequence;
        self.assert_deposit_claims_balanced()?;
        Ok(())
    }

    fn initialize_opening_balances(&mut self, seeds: SeedBook) -> OwgResult<()> {
        for index in 0..self.households.len() {
            let amount = Money::from_major_f64(seeds.range(
                "household-opening-deposit",
                index as u64,
                0,
                0,
                800.0,
                2_200.0,
            ))?;
            self.open_deposit(
                self.households.ids[index].clone(),
                self.households.bank_index[index],
                amount,
            )?;
        }
        for index in 0..self.firms.len() {
            let amount = Money::from_major_f64(seeds.range(
                "firm-opening-deposit",
                index as u64,
                0,
                0,
                35_000.0,
                70_000.0,
            ))?;
            self.open_deposit(
                self.firms.ids[index].clone(),
                self.firms.bank_index[index],
                amount,
            )?;
        }
        self.open_deposit(REST_OF_WORLD_ID.to_owned(), 0, Money(200_000_000))?;

        let treasury = Money(100_000_000);
        let entry_id = self.next_entry_id("opening-treasury");
        self.ledger.post(JournalEntry::new(
            entry_id,
            0,
            "opening",
            "government treasury opening balance",
            vec![
                Posting::new(
                    AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury),
                    treasury,
                ),
                Posting::new(
                    AccountKey::new(CENTRAL_BANK_ID, AccountKind::DepositLiability),
                    -treasury,
                ),
            ],
        ))?;

        for bank_index in 0..self.banks.len() {
            let bank_id = self.banks.ids[bank_index].clone();
            let liabilities = -self
                .ledger
                .balance(&AccountKey::new(
                    bank_id.clone(),
                    AccountKind::DepositLiability,
                ))
                .minor_units();
            let reserves = Money(liabilities).checked_mul(110)?;
            let reserves = Money(reserves.minor_units() / 100);
            let entry_id = self.next_entry_id("opening-reserves");
            self.ledger.post(JournalEntry::new(
                entry_id,
                0,
                "opening",
                "commercial-bank reserve opening balance",
                vec![
                    Posting::new(
                        AccountKey::new(bank_id.clone(), AccountKind::Reserve),
                        reserves,
                    ),
                    Posting::new(
                        AccountKey::new(CENTRAL_BANK_ID, AccountKind::ReserveLiability),
                        -reserves,
                    ),
                ],
            ))?;
            self.close_entity_equity(&bank_id)?;
        }
        self.close_entity_equity(GOVERNMENT_ID)?;
        self.close_entity_equity(CENTRAL_BANK_ID)?;
        Ok(())
    }

    fn open_deposit(
        &mut self,
        owner_id: String,
        bank_index: usize,
        amount: Money,
    ) -> OwgResult<()> {
        let bank_id = self.banks.ids[bank_index].clone();
        let entry_id = self.next_entry_id("opening-deposit");
        self.ledger.post(JournalEntry::new(
            entry_id,
            0,
            "opening",
            "deposit opening balance",
            vec![
                Posting::new(AccountKey::new(owner_id, AccountKind::Deposit), amount),
                Posting::new(
                    AccountKey::new(bank_id, AccountKind::DepositLiability),
                    -amount,
                ),
            ],
        ))
    }

    fn close_entity_equity(&mut self, owner_id: &str) -> OwgResult<()> {
        let residual = self.ledger.owner_balance(owner_id)?;
        if residual == Money::ZERO {
            return Ok(());
        }
        let entry_id = self.next_entry_id("opening-equity");
        self.ledger.post(JournalEntry::new(
            entry_id,
            0,
            "opening",
            "opening balance-sheet equity closure",
            vec![
                Posting::new(AccountKey::new(owner_id, AccountKind::Equity), -residual),
                Posting::new(
                    AccountKey::new(OWNERSHIP_SECTOR_ID, AccountKind::Equity),
                    residual,
                ),
            ],
        ))
    }

    pub fn step_month(
        &mut self,
        scenario: &ScenarioSpec,
        branch: &PolicyBranch,
    ) -> OwgResult<MetricRow> {
        self.step_month_with_labor(scenario, branch, None)
            .map(|(metrics, _, _)| metrics)
    }

    pub(crate) fn step_month_with_labor(
        &mut self,
        scenario: &ScenarioSpec,
        branch: &PolicyBranch,
        mut historical_labor: Option<&mut HistoricalLabor>,
    ) -> OwgResult<(MetricRow, [f64; 4], ProductionFlows)> {
        self.assert_population_structure()?;
        self.assert_deposit_claims_balanced()?;
        self.assert_loan_claims_balanced()?;
        self.validate_credit_context(scenario, true)?;
        if self.month >= scenario.calendar.months {
            return Err(OwgError::InvalidState(
                "simulation already reached scenario horizon".to_owned(),
            ));
        }
        let month = self.month;
        let seeds = SeedBook::new(self.root_seed);
        if self.governance.is_some() != scenario.governance.is_some() {
            return Err(OwgError::InvalidState(
                "governance state does not match scenario configuration".to_owned(),
            ));
        }
        if let (Some(state), Some(spec)) = (&mut self.governance, &scenario.governance) {
            state.advance(spec, month)?;
        }
        let effects = MonthEffects::for_month(scenario, branch, self.governance.as_ref(), month)?;
        let mut flows = MonthFlows::default();
        let scheduler = MultiRateScheduler::new(scenario.calendar.weeks_per_month)?;
        let mut metric = None;
        for event in scheduler.events_for_month_with_credit(
            month,
            scenario
                .credit_market
                .as_ref()
                .map(|spec| spec.service_order),
        ) {
            self.execute_scheduled_event(
                &event,
                scenario,
                branch,
                seeds,
                &effects,
                &mut flows,
                &mut metric,
                historical_labor.as_deref_mut(),
            )?;
            self.schedule_audit.record(&event)?;
        }
        let metric = metric.ok_or_else(|| {
            OwgError::InvalidState(format!("scheduler omitted reporting at month {month}"))
        })?;
        if !metric.finite() {
            return Err(OwgError::InvalidState(format!(
                "non-finite metric at month {month}"
            )));
        }
        if metric.ledger_residual_minor_units != 0 {
            return Err(OwgError::InvalidState(format!(
                "ledger residual at month {month}: {}",
                metric.ledger_residual_minor_units
            )));
        }
        if metric.institutional_balances.residual != Money::ZERO
            || metric.institutional_balances.unclassified != Money::ZERO
        {
            return Err(OwgError::InvalidState(format!(
                "institutional accounts did not classify and close at month {month}"
            )));
        }
        self.assert_reserve_claims_balanced()?;
        self.assert_deposit_claims_balanced()?;
        self.assert_loan_claims_balanced()?;
        self.validate_credit_context(scenario, true)?;
        let production = ProductionFlows {
            gross_output_fixed_prices: flows.production_real,
            imported_inputs_fixed_prices: flows.real_import_inputs,
            value_added_fixed_prices: flows.production_real - flows.real_import_inputs,
            unused_service_slots_fixed_prices: flows.unused_service_slots_real,
            inventory_opening_nominal: flows.opening_inventory_value,
            inventory_closing_nominal: self.inventory_value(),
            government_purchases: flows.government_purchases,
            exports: flows.exports,
            social_benefits: flows.social_benefits,
            household_purchase_intent: flows.household_purchase_intent,
            household_disposable_income: self.households.monthly_income.iter().sum(),
        };
        Ok((metric, flows.consumption_by_sector, production))
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_scheduled_event(
        &mut self,
        event: &ScheduledEvent,
        scenario: &ScenarioSpec,
        branch: &PolicyBranch,
        seeds: SeedBook,
        effects: &MonthEffects,
        flows: &mut MonthFlows,
        metric: &mut Option<MetricRow>,
        historical_labor: Option<&mut HistoricalLabor>,
    ) -> OwgResult<()> {
        match event.kind {
            SimulationEventKind::OriginateContractCredit => {
                self.originate_contract_credit(scenario)?;
            }
            SimulationEventKind::ServiceContractCredit => self.service_contract_credit(scenario)?,
            SimulationEventKind::ResolveContractCredit => {
                self.resolve_inactive_contract_borrowers(scenario)?;
            }
            SimulationEventKind::ResetMonth => {
                *flows = MonthFlows::default();
                self.households.monthly_income.fill(0.0);
                self.households.last_consumption.fill(0.0);
                self.firms.last_sales_units.fill(0.0);
                self.firms.last_sales_value.fill(0.0);
            }
            SimulationEventKind::UpdatePolicy => self.update_policy(event.month, effects),
            SimulationEventKind::RestartInactiveFirms => {
                self.restart_inactive_firms(
                    event.month,
                    seeds,
                    flows,
                    historical_labor
                        .as_ref()
                        .is_some_and(|labor| labor.calibrated_production()),
                )?;
            }
            SimulationEventKind::MatchLabor => {
                self.update_labor_market(event.month, seeds, effects, historical_labor);
            }
            SimulationEventKind::ApplyTargetedTransfers => {
                self.apply_targeted_transfers(effects, flows)?;
            }
            SimulationEventKind::ApplyUnemploymentBenefits => {
                self.apply_unemployment_benefits(flows, historical_labor.as_deref())?;
                if let Some(labor) = historical_labor.as_deref() {
                    self.pay_historical_social_benefit(labor, flows)?;
                }
            }
            SimulationEventKind::UpdateFirmPrices => self.update_firm_prices(effects)?,
            SimulationEventKind::CaptureOpeningInventory => {
                flows.opening_inventory_value = self.inventory_value();
            }
            SimulationEventKind::ProduceAndImport => {
                self.produce_and_import(
                    effects,
                    flows,
                    historical_labor
                        .as_ref()
                        .and_then(|labor| labor.inventory()),
                )?;
            }
            SimulationEventKind::PayWages => self.pay_wages(flows)?,
            SimulationEventKind::ServiceHouseholdDebt => {
                self.service_household_debt(flows)?;
            }
            SimulationEventKind::GoodsMarket { week } => {
                self.household_consumption(
                    event.month,
                    week,
                    seeds,
                    effects,
                    flows,
                    historical_labor
                        .as_ref()
                        .and_then(|labor| labor.household_demand.as_ref())
                        .map(|demand| (demand, scenario.calendar.weeks_per_month)),
                )?;
            }
            SimulationEventKind::SettleForeignDemand => {
                self.foreign_demand(event.month, seeds, flows)?;
            }
            SimulationEventKind::SettleGovernmentDemand => {
                self.government_demand(effects, flows)?;
            }
            SimulationEventKind::ServiceFirmDebt => self.service_firm_debt(flows)?,
            SimulationEventKind::ResolveFirmDefaults => {
                if historical_labor
                    .as_ref()
                    .and_then(|labor| labor.inventory())
                    .is_some()
                {
                    self.expire_service_slots(flows);
                }
                self.resolve_defaults(flows)?;
            }
            SimulationEventKind::ResolveBanks => self.resolve_banks(flows)?,
            SimulationEventKind::UpdateLearning => {
                if let Some(demand) =
                    historical_labor.and_then(|labor| labor.household_demand.as_mut())
                {
                    demand.update(&self.households.monthly_income);
                }
                self.update_learning(event.month, seeds);
            }
            SimulationEventKind::CollectMetrics => {
                if metric.is_some() {
                    return Err(OwgError::InvalidState(format!(
                        "scheduler reported month {} more than once",
                        event.month
                    )));
                }
                *metric = Some(self.collect_metrics(
                    scenario,
                    branch,
                    effects,
                    flows,
                    historical_labor.as_deref(),
                )?);
            }
            SimulationEventKind::CloseMonth => {
                if metric.is_none() {
                    return Err(OwgError::InvalidState(format!(
                        "scheduler closed month {} before reporting",
                        event.month
                    )));
                }
                self.month += 1;
            }
        }
        Ok(())
    }

    fn update_policy(&mut self, month: u32, effects: &MonthEffects) {
        if month.is_multiple_of(3) {
            let target =
                0.03 + 1.20 * (self.last_inflation - 0.02) + 0.35 * (0.05 - self.last_unemployment);
            self.base_policy_rate = (0.75 * self.base_policy_rate + 0.25 * target).clamp(0.0, 0.20);
        }
        self.effective_policy_rate =
            (self.base_policy_rate + effects.policy_rate_delta).clamp(0.0, 0.25);
        self.credit_spread_shock = effects.credit_spread_delta;
    }

    fn update_labor_market(
        &mut self,
        month: u32,
        seeds: SeedBook,
        effects: &MonthEffects,
        historical_labor: Option<&mut HistoricalLabor>,
    ) {
        let mut employee_counts = vec![0_usize; self.firms.len()];
        for index in self.households.employer.iter().flatten() {
            if self.firms.active[*index] {
                employee_counts[*index] += 1;
            }
        }

        let mut desired = vec![0_usize; self.firms.len()];
        for (index, desired_workers) in desired.iter_mut().enumerate() {
            if !self.firms.active[index] {
                continue;
            }
            let inventory_pressure =
                self.firms.inventory[index] / self.firms.expected_demand[index].max(1.0);
            let rate_drag = (self.effective_policy_rate - 0.03).max(0.0) * 2.0;
            let mut variable_workers = self.firms.expected_demand[index]
                * (1.0 - rate_drag)
                * if inventory_pressure > 2.0 { 0.8 } else { 1.0 }
                / self.firms.productivity[index];
            if historical_labor.is_some() {
                let sector = self.firms.sector[index].index();
                variable_workers *= effects.supply_capacity[sector] / effects.productivity[sector];
            }
            // A fixed operating team models coordination, sales, and administration.
            // It also prevents implausibly frictionless mass layoffs after one weak month.
            *desired_workers = if historical_labor
                .as_ref()
                .is_some_and(|labor| labor.calibrated_production())
            {
                (variable_workers.ceil() as usize).clamp(1, 28)
            } else {
                (3 + variable_workers.ceil() as usize).clamp(3, 28)
            };
        }

        if let Some(labor) = historical_labor {
            labor.match_workers(self, &desired, employee_counts, month, seeds);
            return;
        }

        for household_index in 0..self.households.len() {
            let Some(firm_index) = self.households.employer[household_index] else {
                continue;
            };
            if !self.firms.active[firm_index]
                || employee_counts[firm_index] > desired[firm_index] + 1
            {
                self.households.employer[household_index] = None;
                employee_counts[firm_index] = employee_counts[firm_index].saturating_sub(1);
            }
        }

        for household_index in 0..self.households.len() {
            if self.households.employer[household_index].is_some() {
                continue;
            }
            let start = seeds.index(
                "job-search",
                household_index as u64,
                month,
                0,
                self.firms.len(),
            );
            let mut selected = None;
            for offset in 0..self.firms.len().min(12) {
                let firm_index = (start + offset) % self.firms.len();
                if self.firms.active[firm_index]
                    && employee_counts[firm_index] < desired[firm_index]
                {
                    selected = Some(firm_index);
                    break;
                }
            }
            if let Some(firm_index) = selected {
                self.households.employer[household_index] = Some(firm_index);
                employee_counts[firm_index] += 1;
            }
        }
    }

    fn update_firm_prices(&mut self, effects: &MonthEffects) -> OwgResult<()> {
        let employee_counts = self.employee_counts();
        let imported_energy_price = self.base_sector_prices[Sector::Energy.index()]
            * effects.import_price[Sector::Energy.index()];
        for (index, worker_count) in employee_counts.iter().copied().enumerate() {
            if !self.firms.active[index] {
                continue;
            }
            let workers = worker_count.max(1) as f64;
            let wage_cost_per_unit = self.firms.wage_offer[index].as_major_f64() * workers
                / (self.firms.productivity[index] * workers).max(1.0);
            let energy_cost = imported_energy_price
                * self.firms.energy_intensity[index]
                * self.energy_efficiency_multiplier;
            let unit_cost = wage_cost_per_unit + energy_cost + 0.75;
            let inventory_ratio =
                self.firms.inventory[index] / self.firms.expected_demand[index].max(1.0);
            let inventory_adjustment = (1.0 + 0.05 * (1.0 - inventory_ratio)).clamp(0.90, 1.10);
            let desired = unit_cost * (1.0 + self.firms.markup[index]) * inventory_adjustment;
            let old = self.firms.price[index].as_major_f64();
            self.firms.price[index] =
                Money::from_major_f64((0.78 * old + 0.22 * desired).max(0.01))?;
        }
        Ok(())
    }

    fn produce_and_import(
        &mut self,
        effects: &MonthEffects,
        flows: &mut MonthFlows,
        inventory: Option<&InventoryPolicy>,
    ) -> OwgResult<()> {
        let employee_counts = self.employee_counts();
        for (index, worker_count) in employee_counts.iter().copied().enumerate() {
            if !self.firms.active[index] {
                continue;
            }
            let sector = self.firms.sector[index];
            let workers = worker_count as f64;
            let labor_capacity =
                workers * self.firms.productivity[index] * effects.productivity[sector.index()];
            let physical_capacity =
                self.firms.capacity[index] * effects.supply_capacity[sector.index()];
            let demand = self.firms.expected_demand[index];
            let replenishment_plan = if let Some(rule) =
                inventory.and_then(|policy| policy.rule(sector))
            {
                (demand
                    + rule.adjustment * (rule.target_months * demand - self.firms.inventory[index]))
                    .max(0.0)
            } else if inventory.is_some() && sector == Sector::Services {
                demand
            } else {
                (demand + 0.35 * demand - self.firms.inventory[index]).max(0.10 * demand)
            };
            let prospective_output = labor_capacity
                .min(physical_capacity)
                .min(replenishment_plan);
            if prospective_output <= 0.0 {
                continue;
            }

            let imported_energy_price = self.base_sector_prices[Sector::Energy.index()]
                * effects.import_price[Sector::Energy.index()];
            let import_cost_per_unit = imported_energy_price
                * self.firms.energy_intensity[index]
                * self.energy_efficiency_multiplier;
            let desired_import_cost =
                Money::from_major_f64(prospective_output * import_cost_per_unit)?;
            self.ensure_firm_liquidity(index, desired_import_cost, flows)?;
            let available = self.firm_deposit(index);
            let paid = if available < desired_import_cost {
                available
            } else {
                desired_import_cost
            };
            let scale = if desired_import_cost > Money::ZERO {
                paid.as_major_f64() / desired_import_cost.as_major_f64()
            } else {
                1.0
            };
            let output = prospective_output * scale.clamp(0.0, 1.0);
            if paid > Money::ZERO {
                self.transfer_between_deposits(
                    self.firms.ids[index].clone(),
                    self.firms.bank_index[index],
                    REST_OF_WORLD_ID.to_owned(),
                    0,
                    paid,
                    "intermediate imported energy",
                )?;
                flows.imports += paid.as_major_f64();
                flows.real_import_inputs +=
                    paid.as_major_f64() / effects.import_price[Sector::Energy.index()];
            }
            self.firms.inventory[index] += output;
            let producer_value = output * self.firms.price[index].as_major_f64();
            flows.production_nominal += producer_value;
            flows.production_real += output * self.base_sector_prices[sector.index()];
        }
        Ok(())
    }

    fn expire_service_slots(&mut self, flows: &mut MonthFlows) {
        for (firm, &sector) in self.firms.sector.iter().enumerate() {
            if sector == Sector::Services {
                let slots = std::mem::take(&mut self.firms.inventory[firm]);
                let real = slots * self.base_sector_prices[sector.index()];
                flows.production_nominal -= slots * self.firms.price[firm].as_major_f64();
                flows.production_real -= real;
                flows.unused_service_slots_real += real;
            }
        }
    }

    fn pay_wages(&mut self, flows: &mut MonthFlows) -> OwgResult<()> {
        let mut employees_by_firm = vec![Vec::<usize>::new(); self.firms.len()];
        for (household_index, employer) in self.households.employer.iter().enumerate() {
            if let Some(firm_index) = employer.filter(|index| *index < self.firms.len()) {
                employees_by_firm[firm_index].push(household_index);
            }
        }
        for (firm_index, workers) in employees_by_firm.iter().enumerate() {
            if !self.firms.active[firm_index] {
                continue;
            }
            if workers.is_empty() {
                continue;
            }
            let payroll = workers
                .iter()
                .try_fold(Money::ZERO, |sum, household_index| {
                    sum.checked_add(
                        self.firms.wage_offer[firm_index]
                            .mul_rate(0.85 + 0.15 * self.households.skill[*household_index])?,
                    )
                })?;
            self.ensure_firm_liquidity(firm_index, payroll, flows)?;
            let available = self.firm_deposit(firm_index);
            let ratio = if payroll > Money::ZERO {
                (available.as_major_f64() / payroll.as_major_f64()).clamp(0.0, 1.0)
            } else {
                1.0
            };
            if ratio < 0.80 {
                self.firms.default_streak[firm_index] += 1;
            } else {
                self.firms.default_streak[firm_index] =
                    self.firms.default_streak[firm_index].saturating_sub(1);
            }

            for household_index in workers.iter().copied() {
                let gross = self.firms.wage_offer[firm_index]
                    .mul_rate(0.85 + 0.15 * self.households.skill[household_index])?;
                let scaled_gross = gross.mul_rate(ratio)?;
                let available_now = self.firm_deposit(firm_index);
                let paid_gross = if scaled_gross > available_now {
                    available_now
                } else {
                    scaled_gross
                };
                let tax = paid_gross.mul_rate(self.parameters.income_tax_rate)?;
                let net = paid_gross.checked_sub(tax)?;
                if net > Money::ZERO {
                    self.transfer_between_deposits(
                        self.firms.ids[firm_index].clone(),
                        self.firms.bank_index[firm_index],
                        self.households.ids[household_index].clone(),
                        self.households.bank_index[household_index],
                        net,
                        "net wage",
                    )?;
                }
                if tax > Money::ZERO {
                    self.transfer_deposit_to_treasury(
                        self.firms.ids[firm_index].clone(),
                        self.firms.bank_index[firm_index],
                        tax,
                        "withheld household income tax",
                    )?;
                }
                self.households.monthly_income[household_index] += net.as_major_f64();
                flows.wages += paid_gross.as_major_f64();
                flows.income_tax += tax.as_major_f64();
            }
        }
        Ok(())
    }

    fn household_consumption(
        &mut self,
        month: u32,
        week: u8,
        seeds: SeedBook,
        effects: &MonthEffects,
        flows: &mut MonthFlows,
        historical_demand: Option<(&HouseholdDemand, u8)>,
    ) -> OwgResult<()> {
        let mut sector_candidates: [Vec<usize>; 4] = std::array::from_fn(|_| Vec::new());
        for firm_index in 0..self.firms.len() {
            if self.firms.active[firm_index] && self.firms.inventory[firm_index] > 1e-9 {
                sector_candidates[self.firms.sector[firm_index].index()].push(firm_index);
            }
        }
        for household_index in 0..self.households.len() {
            let wealth = self.household_deposit(household_index).as_major_f64();
            let income = self.households.monthly_income[household_index];
            let rate_drag = (self.effective_policy_rate - 0.02).max(0.0)
                * self.households.risk_aversion[household_index]
                * 1.8;
            let confidence = effects.confidence
                * (1.0 - 0.20 * self.households.expected_inflation[household_index].max(0.0));
            let propensity = (self.households.consumption_propensity[household_index] - rate_drag)
                .clamp(0.35, 0.95);
            let habit_target = self.households.habit_consumption[household_index]
                * self.households.habit_strength[household_index];
            let desired_budget =
                (income * propensity * confidence + 0.10 * habit_target + 45.0).max(0.0);
            let liquidity_budget = wealth * 0.45
                + if self.households.employer[household_index].is_some() {
                    0.10 * income
                } else {
                    40.0
                };
            let monthly_budget = desired_budget.min(liquidity_budget.max(0.0));
            let weekly_budget =
                historical_demand.map_or(monthly_budget / 4.0, |(demand, weeks)| {
                    demand.budget(household_index, income) * confidence / f64::from(weeks)
                });
            flows.household_purchase_intent += weekly_budget;

            for sector in Sector::ALL {
                let discretionary_budget =
                    weekly_budget * self.parameters.sector.consumption_share[sector.index()];
                let budget = match sector {
                    Sector::Energy if historical_demand.is_none() => {
                        discretionary_budget.max(self.parameters.weekly_energy_subsistence_floor)
                    }
                    Sector::Food if historical_demand.is_none() => {
                        discretionary_budget.max(self.parameters.weekly_food_subsistence_floor)
                    }
                    _ => discretionary_budget,
                };
                if budget <= 0.01 {
                    continue;
                }
                let candidate_count = sector_candidates[sector.index()].len();
                if candidate_count == 0 {
                    continue;
                }
                let mut chosen_position = seeds.index(
                    "consumer-search",
                    household_index as u64,
                    month,
                    u32::from(week) * 8 + sector.index() as u32,
                    candidate_count,
                );
                for draw in 1..3 {
                    let sampled_position = seeds.index(
                        "consumer-search",
                        household_index as u64,
                        month,
                        u32::from(week) * 8 + sector.index() as u32 + draw,
                        candidate_count,
                    );
                    let sampled = sector_candidates[sector.index()][sampled_position];
                    let chosen = sector_candidates[sector.index()][chosen_position];
                    if self.firms.price[sampled] < self.firms.price[chosen] {
                        chosen_position = sampled_position;
                    }
                }
                let chosen = sector_candidates[sector.index()][chosen_position];
                let tax_rate = (self.base_indirect_tax[sector.index()]
                    + effects.tax_delta[sector.index()])
                .clamp(0.0, 0.50);
                let producer_unit_price = self.firms.price[chosen].as_major_f64();
                let gross_unit_price = producer_unit_price * (1.0 + tax_rate);
                let available_units = self.firms.inventory[chosen];
                let units = (budget / gross_unit_price).min(available_units);
                if units <= 1e-9 {
                    continue;
                }
                let requested_amount = Money::from_major_f64(units * producer_unit_price)?;
                let (producer_amount, units) = bounded_goods_settlement(
                    requested_amount,
                    available_units,
                    self.firms.price[chosen],
                )?;
                if producer_amount <= Money::ZERO || units <= 1e-9 {
                    continue;
                }
                let tax_amount = producer_amount.mul_rate(tax_rate)?;
                let gross = producer_amount.checked_add(tax_amount)?;
                if gross > self.household_deposit(household_index)
                    && matches!(sector, Sector::Energy | Sector::Food)
                {
                    self.ensure_household_liquidity(household_index, gross, flows)?;
                }
                if gross > self.household_deposit(household_index) {
                    continue;
                }
                self.transfer_between_deposits(
                    self.households.ids[household_index].clone(),
                    self.households.bank_index[household_index],
                    self.firms.ids[chosen].clone(),
                    self.firms.bank_index[chosen],
                    producer_amount,
                    "household final consumption",
                )?;
                if tax_amount > Money::ZERO {
                    self.transfer_deposit_to_treasury(
                        self.households.ids[household_index].clone(),
                        self.households.bank_index[household_index],
                        tax_amount,
                        "indirect consumption tax",
                    )?;
                }
                self.firms.inventory[chosen] -= units;
                if self.firms.inventory[chosen] <= 1e-9 {
                    sector_candidates[sector.index()].swap_remove(chosen_position);
                }
                self.firms.last_sales_units[chosen] += units;
                self.firms.last_sales_value[chosen] += producer_amount.as_major_f64();
                self.households.last_consumption[household_index] += gross.as_major_f64();
                flows.consumption_producer += producer_amount.as_major_f64();
                flows.consumption_tax += tax_amount.as_major_f64();
                flows.consumption_total += gross.as_major_f64();
                flows.consumption_by_sector[sector.index()] += gross.as_major_f64();
                flows.sector_sales_value[sector.index()] += producer_amount.as_major_f64();
                flows.sector_sales_units[sector.index()] += units;
            }
        }
        Ok(())
    }

    fn foreign_demand(
        &mut self,
        month: u32,
        seeds: SeedBook,
        flows: &mut MonthFlows,
    ) -> OwgResult<()> {
        for firm_index in 0..self.firms.len() {
            if !self.firms.active[firm_index] {
                continue;
            }
            let demand = self.firms.expected_demand[firm_index]
                * seeds.range("foreign-demand", firm_index as u64, month, 0, 0.10, 0.18);
            let requested_units = demand.min(self.firms.inventory[firm_index]);
            if requested_units <= 1e-9 {
                continue;
            }
            let requested_amount = Money::from_major_f64(
                requested_units * self.firms.price[firm_index].as_major_f64(),
            )?;
            let (amount, units) = bounded_goods_settlement(
                requested_amount,
                self.firms.inventory[firm_index],
                self.firms.price[firm_index],
            )?;
            if amount > self.owner_deposit(REST_OF_WORLD_ID) {
                continue;
            }
            self.transfer_between_deposits(
                REST_OF_WORLD_ID.to_owned(),
                0,
                self.firms.ids[firm_index].clone(),
                self.firms.bank_index[firm_index],
                amount,
                "exports to rest of world",
            )?;
            self.firms.inventory[firm_index] -= units;
            self.firms.last_sales_units[firm_index] += units;
            self.firms.last_sales_value[firm_index] += amount.as_major_f64();
            flows.exports += amount.as_major_f64();
        }
        Ok(())
    }

    fn government_demand(
        &mut self,
        effects: &MonthEffects,
        flows: &mut MonthFlows,
    ) -> OwgResult<()> {
        let baseline = self.parameters.monthly_government_procurement;
        let total = baseline.checked_add(effects.public_investment)?;
        self.ensure_government_liquidity(total)?;
        let eligible: Vec<usize> = (0..self.firms.len())
            .filter(|index| {
                self.firms.active[*index]
                    && matches!(
                        self.firms.sector[*index],
                        Sector::Manufacturing | Sector::Services
                    )
                    && self.firms.inventory[*index] > 1e-9
            })
            .collect();
        if eligible.is_empty() {
            return Ok(());
        }
        let per_firm = total.mul_rate(1.0 / eligible.len() as f64)?;
        for firm_index in eligible {
            let (amount, units) = bounded_goods_settlement(
                per_firm,
                self.firms.inventory[firm_index],
                self.firms.price[firm_index],
            )?;
            if amount <= Money::ZERO {
                continue;
            }
            self.transfer_treasury_to_deposit(
                self.firms.ids[firm_index].clone(),
                self.firms.bank_index[firm_index],
                amount,
                "government procurement and investment",
            )?;
            self.firms.inventory[firm_index] -= units;
            self.firms.last_sales_units[firm_index] += units;
            self.firms.last_sales_value[firm_index] += amount.as_major_f64();
            flows.government_purchases += amount.as_major_f64();
        }
        if effects.energy_intensity_reduction > 0.0 {
            self.energy_efficiency_multiplier *= 1.0 - effects.energy_intensity_reduction;
            self.energy_efficiency_multiplier = self.energy_efficiency_multiplier.clamp(0.55, 1.0);
        }
        Ok(())
    }

    fn apply_targeted_transfers(
        &mut self,
        effects: &MonthEffects,
        flows: &mut MonthFlows,
    ) -> OwgResult<()> {
        for (amount, bottom_share) in &effects.transfers {
            let mut indices: Vec<usize> = (0..self.households.len()).collect();
            indices.sort_by_key(|index| self.household_deposit(*index));
            let recipient_count = ((self.households.len() as f64 * bottom_share).ceil() as usize)
                .clamp(1, self.households.len());
            let total = amount.checked_mul(recipient_count as i64)?;
            self.ensure_government_liquidity(total)?;
            for household_index in indices.into_iter().take(recipient_count) {
                self.transfer_treasury_to_deposit(
                    self.households.ids[household_index].clone(),
                    self.households.bank_index[household_index],
                    *amount,
                    "targeted household transfer",
                )?;
                self.households.monthly_income[household_index] += amount.as_major_f64();
                flows.transfers += amount.as_major_f64();
            }
        }
        Ok(())
    }

    fn apply_unemployment_benefits(
        &mut self,
        flows: &mut MonthFlows,
        historical_labor: Option<&HistoricalLabor>,
    ) -> OwgResult<()> {
        let benefit = self.parameters.monthly_unemployment_benefit;
        let recipients: Vec<usize> = self
            .households
            .employer
            .iter()
            .enumerate()
            .filter_map(|(index, employer)| {
                (employer.is_none() && historical_labor.is_none_or(|labor| !labor.outside(index)))
                    .then_some(index)
            })
            .collect();
        let total = benefit.checked_mul(recipients.len() as i64)?;
        self.ensure_government_liquidity(total)?;
        for household_index in recipients {
            self.transfer_treasury_to_deposit(
                self.households.ids[household_index].clone(),
                self.households.bank_index[household_index],
                benefit,
                "automatic unemployment benefit",
            )?;
            self.households.monthly_income[household_index] += benefit.as_major_f64();
            flows.transfers += benefit.as_major_f64();
        }
        Ok(())
    }

    fn pay_historical_social_benefit(
        &mut self,
        labor: &HistoricalLabor,
        flows: &mut MonthFlows,
    ) -> OwgResult<()> {
        let Some(demand) = &labor.household_demand else {
            return Ok(());
        };
        let benefit = demand.social_benefit(self.cpi_history.last().copied().unwrap_or(100.0))?;
        if benefit == Money::ZERO {
            return Ok(());
        }
        let recipients: Vec<_> = (0..self.households.len())
            .filter(|&index| labor.outside(index))
            .collect();
        self.ensure_government_liquidity(benefit.checked_mul(recipients.len() as i64)?)?;
        for index in recipients {
            self.transfer_treasury_to_deposit(
                self.households.ids[index].clone(),
                self.households.bank_index[index],
                benefit,
                "historical social benefit proxy",
            )?;
            self.households.monthly_income[index] += benefit.as_major_f64();
            flows.transfers += benefit.as_major_f64();
            flows.social_benefits += benefit.as_major_f64();
        }
        Ok(())
    }

    fn ensure_firm_liquidity(
        &mut self,
        firm_index: usize,
        required: Money,
        flows: &mut MonthFlows,
    ) -> OwgResult<()> {
        let current = self.firm_deposit(firm_index);
        if current >= required {
            return Ok(());
        }
        let need = required.checked_sub(current)?;
        let debt = (-self.ledger.balance(&AccountKey::new(
            self.firms.ids[firm_index].clone(),
            AccountKind::DebtLiability,
        )))
        .checked_add(self.contract_debt(&self.firms.ids[firm_index])?)?;
        let collateral = Money::from_major_f64(
            self.firms.capacity[firm_index] * self.firms.price[firm_index].as_major_f64() * 2.0,
        )?;
        let score = 1.0 - debt.as_major_f64() / collateral.as_major_f64().max(1.0);
        let bank_index = self.firms.bank_index[firm_index];
        if score < self.banks.approval_threshold[bank_index] - 0.80 + self.credit_spread_shock * 4.0
        {
            return Ok(());
        }
        let maximum = collateral.mul_rate(0.50)?;
        let headroom = if maximum > debt {
            maximum.checked_sub(debt)?
        } else {
            Money::ZERO
        };
        let principal = if need < headroom { need } else { headroom };
        if principal <= Money::ZERO {
            return Ok(());
        }
        if !self.bank_can_lend(bank_index, principal) {
            return Ok(());
        }
        let bank_id = self.banks.ids[bank_index].clone();
        let firm_id = self.firms.ids[firm_index].clone();
        let entry_id = self.next_entry_id("firm-loan");
        self.ledger.post(JournalEntry::new(
            entry_id,
            self.month,
            "credit",
            "working-capital loan origination",
            vec![
                Posting::new(
                    AccountKey::new(firm_id.clone(), AccountKind::Deposit),
                    principal,
                ),
                Posting::new(
                    AccountKey::new(bank_id.clone(), AccountKind::DepositLiability),
                    -principal,
                ),
                Posting::new(AccountKey::new(bank_id, AccountKind::LoanAsset), principal),
                Posting::new(
                    AccountKey::new(firm_id, AccountKind::DebtLiability),
                    -principal,
                ),
            ],
        ))?;
        flows.new_credit += principal.as_major_f64();
        Ok(())
    }

    fn ensure_household_liquidity(
        &mut self,
        household_index: usize,
        required: Money,
        flows: &mut MonthFlows,
    ) -> OwgResult<()> {
        let current = self.household_deposit(household_index);
        if current >= required {
            return Ok(());
        }
        let household_id = self.households.ids[household_index].clone();
        let debt = (-self.ledger.balance(&AccountKey::new(
            household_id.clone(),
            AccountKind::DebtLiability,
        )))
        .checked_add(self.contract_debt(&household_id)?)?;
        let income = self.households.monthly_income[household_index];
        let limit = Money::from_major_f64(
            (income * self.parameters.household_credit_income_multiple)
                .max(self.parameters.household_credit_minimum_limit),
        )?;
        let headroom = if limit > debt {
            limit.checked_sub(debt)?
        } else {
            Money::ZERO
        };
        let employment_score = if self.households.employer[household_index].is_some() {
            0.30
        } else {
            0.0
        };
        let score =
            0.35 + employment_score + 0.20 * (1.0 - self.households.risk_aversion[household_index])
                - 0.05 * f64::from(self.households.delinquent_months[household_index]);
        let bank_index = self.households.bank_index[household_index];
        let threshold =
            self.banks.approval_threshold[bank_index] - 0.10 + self.credit_spread_shock * 4.0;
        if score < threshold {
            return Ok(());
        }
        let need = required.checked_sub(current)?;
        let principal = if need < headroom { need } else { headroom };
        if principal <= Money::ZERO {
            return Ok(());
        }
        if !self.bank_can_lend(bank_index, principal) {
            return Ok(());
        }
        let bank_id = self.banks.ids[bank_index].clone();
        let entry_id = self.next_entry_id("household-loan");
        self.ledger.post(JournalEntry::new(
            entry_id,
            self.month,
            "credit",
            "bounded household essential-consumption loan",
            vec![
                Posting::new(
                    AccountKey::new(household_id.clone(), AccountKind::Deposit),
                    principal,
                ),
                Posting::new(
                    AccountKey::new(bank_id.clone(), AccountKind::DepositLiability),
                    -principal,
                ),
                Posting::new(AccountKey::new(bank_id, AccountKind::LoanAsset), principal),
                Posting::new(
                    AccountKey::new(household_id, AccountKind::DebtLiability),
                    -principal,
                ),
            ],
        ))?;
        flows.new_credit += principal.as_major_f64();
        Ok(())
    }

    fn service_household_debt(&mut self, flows: &mut MonthFlows) -> OwgResult<()> {
        for household_index in 0..self.households.len() {
            let household_id = self.households.ids[household_index].clone();
            let debt = -self.ledger.balance(&AccountKey::new(
                household_id.clone(),
                AccountKind::DebtLiability,
            ));
            if debt <= Money::ZERO {
                self.households.delinquent_months[household_index] = 0;
                continue;
            }
            let bank_index = self.households.bank_index[household_index];
            let bank_id = self.banks.ids[bank_index].clone();
            let annual_rate = self.effective_policy_rate
                + self.banks.lending_spread[bank_index]
                + self.credit_spread_shock
                + self.parameters.household_credit_spread;
            let interest = debt.mul_rate(annual_rate / 12.0)?;
            let principal = debt.mul_rate(self.parameters.household_debt_monthly_principal_rate)?;
            let due = interest.checked_add(principal)?;
            if self.household_deposit(household_index) < due {
                self.households.delinquent_months[household_index] += 1;
                if self.households.delinquent_months[household_index] >= 6 {
                    let entry_id = self.next_entry_id("household-default");
                    self.ledger.post(JournalEntry::new(
                        entry_id,
                        self.month,
                        "resolution",
                        "household unsecured-debt write-off",
                        vec![
                            Posting::new(
                                AccountKey::new(bank_id.clone(), AccountKind::LoanAsset),
                                -debt,
                            ),
                            Posting::new(
                                AccountKey::new(household_id.clone(), AccountKind::DebtLiability),
                                debt,
                            ),
                            Posting::new(AccountKey::new(bank_id, AccountKind::Equity), debt),
                            Posting::new(
                                AccountKey::new(OWNERSHIP_SECTOR_ID, AccountKind::Equity),
                                -debt,
                            ),
                        ],
                    ))?;
                    self.households.delinquent_months[household_index] = 0;
                    flows.household_defaults += 1;
                }
                continue;
            }
            if interest > Money::ZERO {
                let entry_id = self.next_entry_id("household-interest");
                self.ledger.post(JournalEntry::new(
                    entry_id,
                    self.month,
                    "settlement",
                    "household loan interest",
                    vec![
                        Posting::new(
                            AccountKey::new(household_id.clone(), AccountKind::Deposit),
                            -interest,
                        ),
                        Posting::new(
                            AccountKey::new(bank_id.clone(), AccountKind::DepositLiability),
                            interest,
                        ),
                        Posting::new(
                            AccountKey::new(bank_id.clone(), AccountKind::Equity),
                            -interest,
                        ),
                        Posting::new(
                            AccountKey::new(OWNERSHIP_SECTOR_ID, AccountKind::Equity),
                            interest,
                        ),
                    ],
                ))?;
                flows.interest_paid += interest.as_major_f64();
            }
            if principal > Money::ZERO {
                let entry_id = self.next_entry_id("household-principal");
                self.ledger.post(JournalEntry::new(
                    entry_id,
                    self.month,
                    "settlement",
                    "household loan principal repayment",
                    vec![
                        Posting::new(
                            AccountKey::new(household_id.clone(), AccountKind::Deposit),
                            -principal,
                        ),
                        Posting::new(
                            AccountKey::new(bank_id.clone(), AccountKind::DepositLiability),
                            principal,
                        ),
                        Posting::new(
                            AccountKey::new(household_id, AccountKind::DebtLiability),
                            principal,
                        ),
                        Posting::new(AccountKey::new(bank_id, AccountKind::LoanAsset), -principal),
                    ],
                ))?;
                flows.principal_repaid += principal.as_major_f64();
            }
            self.households.delinquent_months[household_index] = 0;
        }
        Ok(())
    }

    fn service_firm_debt(&mut self, flows: &mut MonthFlows) -> OwgResult<()> {
        for firm_index in 0..self.firms.len() {
            let firm_id = self.firms.ids[firm_index].clone();
            let bank_index = self.firms.bank_index[firm_index];
            let bank_id = self.banks.ids[bank_index].clone();
            let debt = -self.ledger.balance(&AccountKey::new(
                firm_id.clone(),
                AccountKind::DebtLiability,
            ));
            if debt <= Money::ZERO {
                continue;
            }
            let interest_rate = self.effective_policy_rate
                + self.banks.lending_spread[bank_index]
                + self.credit_spread_shock;
            let interest = debt.mul_rate(interest_rate / 12.0)?;
            let principal = debt.mul_rate(0.01)?;
            let due = interest.checked_add(principal)?;
            let available = self.firm_deposit(firm_index);
            if available < due {
                self.firms.default_streak[firm_index] += 1;
                continue;
            }

            if interest > Money::ZERO {
                let entry_id = self.next_entry_id("loan-interest");
                self.ledger.post(JournalEntry::new(
                    entry_id,
                    self.month,
                    "settlement",
                    "firm loan interest",
                    vec![
                        Posting::new(
                            AccountKey::new(firm_id.clone(), AccountKind::Deposit),
                            -interest,
                        ),
                        Posting::new(
                            AccountKey::new(bank_id.clone(), AccountKind::DepositLiability),
                            interest,
                        ),
                        Posting::new(
                            AccountKey::new(bank_id.clone(), AccountKind::Equity),
                            -interest,
                        ),
                        Posting::new(
                            AccountKey::new(OWNERSHIP_SECTOR_ID, AccountKind::Equity),
                            interest,
                        ),
                    ],
                ))?;
                flows.interest_paid += interest.as_major_f64();
            }
            if principal > Money::ZERO {
                let entry_id = self.next_entry_id("loan-principal");
                self.ledger.post(JournalEntry::new(
                    entry_id,
                    self.month,
                    "settlement",
                    "firm loan principal repayment",
                    vec![
                        Posting::new(
                            AccountKey::new(firm_id.clone(), AccountKind::Deposit),
                            -principal,
                        ),
                        Posting::new(
                            AccountKey::new(bank_id.clone(), AccountKind::DepositLiability),
                            principal,
                        ),
                        Posting::new(
                            AccountKey::new(firm_id, AccountKind::DebtLiability),
                            principal,
                        ),
                        Posting::new(AccountKey::new(bank_id, AccountKind::LoanAsset), -principal),
                    ],
                ))?;
                flows.principal_repaid += principal.as_major_f64();
            }
            self.firms.default_streak[firm_index] =
                self.firms.default_streak[firm_index].saturating_sub(1);
        }
        Ok(())
    }

    fn resolve_defaults(&mut self, flows: &mut MonthFlows) -> OwgResult<()> {
        let mut defaulted_firms = vec![false; self.firms.len()];
        for (firm_index, defaulted_firm) in defaulted_firms.iter_mut().enumerate() {
            if !self.firms.active[firm_index] {
                self.firms.inactive_months[firm_index] += 1;
                continue;
            }
            let debt = -self.ledger.balance(&AccountKey::new(
                self.firms.ids[firm_index].clone(),
                AccountKind::DebtLiability,
            ));
            let deposit = self.firm_deposit(firm_index);
            let asset_proxy = Money::from_major_f64(
                self.firms.capacity[firm_index] * self.firms.price[firm_index].as_major_f64(),
            )?;
            let total_debt = debt.checked_add(self.contract_debt(&self.firms.ids[firm_index])?)?;
            if self.firms.default_streak[firm_index] < 4
                && !(total_debt > asset_proxy.mul_rate(1.5)? && deposit < Money(10_000))
            {
                continue;
            }
            let bank_id = self.banks.ids[self.firms.bank_index[firm_index]].clone();
            if debt > Money::ZERO {
                let entry_id = self.next_entry_id("firm-default");
                self.ledger.post(JournalEntry::new(
                    entry_id,
                    self.month,
                    "resolution",
                    "firm debt write-off",
                    vec![
                        Posting::new(
                            AccountKey::new(bank_id.clone(), AccountKind::LoanAsset),
                            -debt,
                        ),
                        Posting::new(
                            AccountKey::new(
                                self.firms.ids[firm_index].clone(),
                                AccountKind::DebtLiability,
                            ),
                            debt,
                        ),
                        Posting::new(AccountKey::new(bank_id, AccountKind::Equity), debt),
                        Posting::new(
                            AccountKey::new(OWNERSHIP_SECTOR_ID, AccountKind::Equity),
                            -debt,
                        ),
                    ],
                ))?;
            }
            flows.inventory_losses_value +=
                self.firms.inventory[firm_index] * self.firms.price[firm_index].as_major_f64();
            self.firms.active[firm_index] = false;
            self.firms.inventory[firm_index] = 0.0;
            self.firms.inactive_months[firm_index] = 0;
            *defaulted_firm = true;
            flows.firm_defaults += 1;
        }
        if defaulted_firms.iter().any(|defaulted| *defaulted) {
            for employer in &mut self.households.employer {
                if employer.is_some_and(|firm_index| defaulted_firms[firm_index]) {
                    *employer = None;
                }
            }
        }
        Ok(())
    }

    fn resolve_banks(&mut self, flows: &mut MonthFlows) -> OwgResult<()> {
        for bank_index in 0..self.banks.len() {
            let bank_id = self.banks.ids[bank_index].clone();
            let loans = self
                .ledger
                .balance(&AccountKey::new(bank_id.clone(), AccountKind::LoanAsset))
                .checked_add(self.ledger.balance(&AccountKey::new(
                    bank_id.clone(),
                    AccountKind::ContractCreditAsset,
                )))?
                .as_major_f64();
            let capital = (-self
                .ledger
                .balance(&AccountKey::new(bank_id.clone(), AccountKind::Equity)))
            .as_major_f64();
            let capital_ratio = capital / loans.max(1.0);
            let minimum_ratio = self.banks.minimum_capital_ratio[bank_index];
            if capital > 0.0 && (loans <= 0.0 || capital_ratio >= minimum_ratio) {
                continue;
            }
            let target_capital = (1.5 * minimum_ratio * loans).max(20_000.0);
            let amount = Money::from_major_f64((target_capital - capital).max(0.0))?;
            if amount <= Money::ZERO {
                continue;
            }
            self.ensure_government_liquidity(amount)?;
            let entry_id = self.next_entry_id("bank-resolution");
            self.ledger.post(JournalEntry::new(
                entry_id,
                self.month,
                "resolution",
                "government bank recapitalization with matched equity asset",
                vec![
                    Posting::new(
                        AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury),
                        -amount,
                    ),
                    Posting::new(
                        AccountKey::new(CENTRAL_BANK_ID, AccountKind::DepositLiability),
                        amount,
                    ),
                    Posting::new(
                        AccountKey::new(bank_id.clone(), AccountKind::Reserve),
                        amount,
                    ),
                    Posting::new(
                        AccountKey::new(CENTRAL_BANK_ID, AccountKind::ReserveLiability),
                        -amount,
                    ),
                    Posting::new(
                        AccountKey::new(GOVERNMENT_ID, AccountKind::BankEquityAsset),
                        amount,
                    ),
                    Posting::new(AccountKey::new(bank_id, AccountKind::Equity), -amount),
                ],
            ))?;
            flows.bank_resolutions += 1;
            flows.bank_resolution_cost += amount.as_major_f64();
        }
        Ok(())
    }

    fn restart_inactive_firms(
        &mut self,
        month: u32,
        seeds: SeedBook,
        flows: &mut MonthFlows,
        calibrated_production: bool,
    ) -> OwgResult<()> {
        for firm_index in 0..self.firms.len() {
            if self.firms.active[firm_index]
                || u32::from(self.firms.inactive_months[firm_index])
                    < self.parameters.firm_entry_wait_months
                || seeds.unit("firm-entry", firm_index as u64, month, 0)
                    >= self.parameters.firm_entry_monthly_probability_after_wait
            {
                continue;
            }
            self.firms.active[firm_index] = true;
            self.firms.default_streak[firm_index] = 0;
            self.firms.inactive_months[firm_index] = 0;
            self.firms.inventory[firm_index] = 0.0;
            if !calibrated_production {
                self.firms.expected_demand[firm_index] =
                    seeds.range("entrant-demand", firm_index as u64, month, 0, 240.0, 340.0);
                self.firms.productivity[firm_index] = seeds.range(
                    "entrant-productivity",
                    firm_index as u64,
                    month,
                    0,
                    38.0,
                    52.0,
                );
                self.firms.capacity[firm_index] = seeds.range(
                    "entrant-capacity",
                    firm_index as u64,
                    month,
                    0,
                    420.0,
                    650.0,
                );
            }
            self.firms.markup[firm_index] =
                seeds.range("entrant-markup", firm_index as u64, month, 0, 0.16, 0.28);
            let sector_index = self.firms.sector[firm_index].index();
            self.firms.price[firm_index] =
                Money::from_major_f64(self.average_sector_prices()[sector_index])?;
            self.ensure_firm_liquidity(firm_index, Money(1_200_000), flows)?;
            flows.firm_entries += 1;
        }
        Ok(())
    }

    fn update_learning(&mut self, month: u32, seeds: SeedBook) {
        let average_expected = if self.households.is_empty() {
            self.last_inflation
        } else {
            self.households.expected_inflation.iter().sum::<f64>() / self.households.len() as f64
        };
        for index in 0..self.households.len() {
            let information = self.households.information_access[index];
            let trust = self.households.trust[index];
            let signal_noise = seeds.normal("inflation-belief", index as u64, month, 0) * 0.003;
            let public_signal = self.last_inflation + signal_noise * (1.0 - information);
            let social_signal = trust * average_expected + (1.0 - trust) * public_signal;
            self.households.expected_inflation[index] =
                (0.78 * self.households.expected_inflation[index] + 0.22 * social_signal)
                    .clamp(-0.10, 0.50);
            let realized_consumption = self.households.last_consumption[index];
            let habit_strength = self.households.habit_strength[index];
            self.households.habit_consumption[index] = habit_strength
                * self.households.habit_consumption[index]
                + (1.0 - habit_strength) * realized_consumption;
            self.households.consumption_propensity[index] = (0.98
                * self.households.consumption_propensity[index]
                + 0.02
                    * if realized_consumption < 0.8 * self.households.monthly_income[index] {
                        0.82
                    } else {
                        0.72
                    })
            .clamp(0.40, 0.95);
        }
        for index in 0..self.firms.len() {
            self.firms.expected_demand[index] = (0.70 * self.firms.expected_demand[index]
                + 0.30 * self.firms.last_sales_units[index])
                .max(10.0);
        }
    }

    fn collect_metrics(
        &mut self,
        scenario: &ScenarioSpec,
        branch: &PolicyBranch,
        effects: &MonthEffects,
        flows: &MonthFlows,
        historical_labor: Option<&HistoricalLabor>,
    ) -> OwgResult<MetricRow> {
        let sector_prices = self.average_sector_prices();
        let producer_price_index: [f64; 4] = array::from_fn(|index| {
            100.0 * sector_prices[index] / self.base_sector_prices[index].max(1e-9)
        });
        let cpi_sector: [f64; 4] = array::from_fn(|index| {
            let current_tax =
                (self.base_indirect_tax[index] + effects.tax_delta[index]).clamp(0.0, 0.50);
            let current_purchaser_price = sector_prices[index] * (1.0 + current_tax);
            let base_purchaser_price =
                self.base_sector_prices[index] * (1.0 + self.base_indirect_tax[index]);
            100.0 * current_purchaser_price / base_purchaser_price.max(1e-9)
        });
        let cpi = cpi_sector
            .iter()
            .zip(self.parameters.sector.consumption_share)
            .map(|(index, weight)| index * weight)
            .sum();
        let inflation_year_over_year = if self.cpi_history.len() >= 12 {
            cpi / self.cpi_history[self.cpi_history.len() - 12] - 1.0
        } else {
            0.0
        };
        self.cpi_history.push(cpi);

        let inventory_change = self.inventory_value() - flows.opening_inventory_value;
        let resource_use = flows.consumption_producer
            + flows.government_purchases
            + flows.exports
            + inventory_change
            + flows.inventory_losses_value;
        let resource_error = (flows.production_nominal - resource_use).abs()
            / flows.production_nominal.abs().max(1.0);
        let nominal_gdp_production =
            flows.production_nominal - flows.imports + flows.consumption_tax;
        let nominal_gdp_expenditure =
            flows.consumption_total + flows.government_purchases + flows.exports - flows.imports
                + inventory_change
                + flows.inventory_losses_value;
        let expenditure_error = (nominal_gdp_production - nominal_gdp_expenditure).abs()
            / nominal_gdp_production.abs().max(1.0);
        let gross_value_added = flows.production_nominal - flows.imports;
        let gross_operating_surplus = gross_value_added - flows.wages;
        let nominal_gdp_income = flows.wages + gross_operating_surplus + flows.consumption_tax;
        let income_error = (nominal_gdp_production - nominal_gdp_income).abs()
            / nominal_gdp_production.abs().max(1.0);
        let income_accounts = IncomeAccounts {
            gross_value_added,
            compensation_of_employees: flows.wages,
            gross_operating_surplus,
            taxes_on_products: flows.consumption_tax,
            nominal_gdp_income,
            identity_relative_error: income_error,
        };
        let institutional_balances = self.institutional_balances()?;
        let identity_error = resource_error.max(expenditure_error).max(income_error);
        let real_gdp = if historical_labor
            .and_then(HistoricalLabor::inventory)
            .is_some()
        {
            flows.production_real - flows.real_import_inputs
        } else {
            flows.production_real
                - flows.imports * 100.0 / producer_price_index[Sector::Energy.index()].max(1e-9)
        };
        let first_shock_month = scenario
            .shocks
            .iter()
            .map(|shock| shock.start_month)
            .min()
            .unwrap_or(scenario.calendar.months);
        if self.month < first_shock_month {
            self.potential_gdp = self.potential_gdp.max(real_gdp).max(1.0);
        } else if self.potential_gdp <= 0.0 {
            self.potential_gdp = real_gdp.max(1.0);
        } else {
            self.potential_gdp *= 1.001;
        }
        let output_gap = real_gdp / self.potential_gdp.max(1.0) - 1.0;

        let employed_weight: f64 = self
            .households
            .employer
            .iter()
            .zip(&self.households.weights)
            .filter(|(employer, _)| employer.is_some())
            .map(|(_, weight)| weight)
            .sum();
        let total_weight: f64 = self.households.weights.iter().sum();
        let labor_force_weight = historical_labor.map_or(total_weight, |labor| {
            self.households
                .weights
                .iter()
                .enumerate()
                .filter(|(index, _)| !labor.outside(*index))
                .map(|(_, weight)| weight)
                .sum()
        });
        let unemployment = if labor_force_weight > 0.0 {
            1.0 - employed_weight / labor_force_weight
        } else {
            0.0
        };
        let average_wage = if employed_weight > 0.0 {
            flows.wages / employed_weight
        } else {
            0.0
        };
        let income_values: Vec<(f64, f64)> = self
            .households
            .monthly_income
            .iter()
            .copied()
            .zip(self.households.weights.iter().copied())
            .collect();
        let consumption_values: Vec<(f64, f64)> = self
            .households
            .last_consumption
            .iter()
            .copied()
            .zip(self.households.weights.iter().copied())
            .collect();
        let deciles = weighted_deciles(&consumption_values);
        let median_proxy = f64::midpoint(deciles[4], deciles[5]);
        let poverty_line = 0.60 * median_proxy;
        let poverty_weight: f64 = consumption_values
            .iter()
            .filter(|(value, _)| *value < poverty_line)
            .map(|(_, weight)| weight)
            .sum();
        let poverty_share = if total_weight > 0.0 {
            poverty_weight / total_weight
        } else {
            0.0
        };

        let contract_credit = self
            .contract_credit
            .as_ref()
            .map(crate::credit::CreditBook::metrics)
            .transpose()?;
        let credit = self
            .banks
            .ids
            .iter()
            .map(|bank_id| {
                self.ledger
                    .balance(&AccountKey::new(bank_id.clone(), AccountKind::LoanAsset))
                    .as_major_f64()
            })
            .sum::<f64>()
            + contract_credit
                .as_ref()
                .map_or(0.0, |credit| credit.principal.as_major_f64());
        let government_debt = (-self.ledger.balance(&AccountKey::new(
            GOVERNMENT_ID,
            AccountKind::GovernmentDebtLiability,
        )))
        .as_major_f64();
        let fiscal_balance = flows.income_tax + flows.consumption_tax
            - flows.transfers
            - flows.government_purchases
            - flows.bank_resolution_cost;
        let ledger_residual = self.ledger.audit_global_balance()?.minor_units();

        self.last_inflation = inflation_year_over_year;
        self.last_unemployment = unemployment;
        Ok(MetricRow {
            scenario_id: scenario.scenario_id.clone(),
            branch_id: branch.id.clone(),
            replication: self.replication,
            month: self.month,
            year: scenario.calendar.start_year + self.month as i32 / 12,
            month_of_year: (self.month % 12 + 1) as u8,
            cpi,
            cpi_energy: cpi_sector[0],
            cpi_food: cpi_sector[1],
            cpi_manufacturing: cpi_sector[2],
            cpi_services: cpi_sector[3],
            inflation_year_over_year,
            real_gdp,
            nominal_gdp: nominal_gdp_production,
            income_accounts,
            institutional_balances,
            contract_credit,
            output_gap,
            unemployment,
            average_wage,
            consumption_total: flows.consumption_total,
            consumption_deciles: deciles,
            gini: gini(&income_values),
            poverty_share,
            inventories: self.firms.inventory.iter().copied().sum::<f64>(),
            firm_defaults: flows.firm_defaults,
            firm_entries: flows.firm_entries,
            household_defaults: flows.household_defaults,
            bank_resolutions: flows.bank_resolutions,
            active_firms: self.firms.active.iter().filter(|active| **active).count() as u32,
            credit,
            fiscal_balance,
            government_debt,
            imports: flows.imports,
            policy_rate: self.effective_policy_rate,
            ledger_residual_minor_units: ledger_residual,
            accounting_identity_relative_error: identity_error,
        })
    }

    fn institutional_balances(&self) -> OwgResult<InstitutionalBalances> {
        let mut balances = InstitutionalBalances::default();
        for (account, balance) in self.ledger.balances() {
            let sector = match account.owner_id.as_str() {
                GOVERNMENT_ID => &mut balances.government,
                CENTRAL_BANK_ID => &mut balances.central_bank,
                REST_OF_WORLD_ID => &mut balances.rest_of_world,
                OWNERSHIP_SECTOR_ID => &mut balances.ownership_sector,
                owner if owner.starts_with("HH-") => &mut balances.households,
                owner if owner.starts_with("FIRM-") => &mut balances.firms,
                owner if owner.starts_with("BANK-") => &mut balances.banks,
                _ => &mut balances.unclassified,
            };
            *sector = sector.checked_add(*balance)?;
        }
        balances.residual = balances.calculated_residual()?;
        Ok(balances)
    }

    fn ensure_government_liquidity(&mut self, required: Money) -> OwgResult<()> {
        let treasury = self
            .ledger
            .balance(&AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury));
        if treasury >= required {
            return Ok(());
        }
        let amount = required
            .checked_sub(treasury)?
            .checked_add(Money(100_000))?;
        let entry_id = self.next_entry_id("government-debt");
        self.ledger.post(JournalEntry::new(
            entry_id,
            self.month,
            "financing",
            "central-bank purchase of government debt",
            vec![
                Posting::new(
                    AccountKey::new(CENTRAL_BANK_ID, AccountKind::GovernmentDebtAsset),
                    amount,
                ),
                Posting::new(
                    AccountKey::new(GOVERNMENT_ID, AccountKind::GovernmentDebtLiability),
                    -amount,
                ),
                Posting::new(
                    AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury),
                    amount,
                ),
                Posting::new(
                    AccountKey::new(CENTRAL_BANK_ID, AccountKind::DepositLiability),
                    -amount,
                ),
            ],
        ))
    }

    fn transfer_between_deposits(
        &mut self,
        from_owner: String,
        from_bank_index: usize,
        to_owner: String,
        to_bank_index: usize,
        amount: Money,
        memo: &str,
    ) -> OwgResult<()> {
        if amount < Money::ZERO {
            return Err(OwgError::InvalidState(
                "negative deposit transfer".to_owned(),
            ));
        }
        let from = AccountKey::new(from_owner, AccountKind::Deposit);
        if self.ledger.balance(&from) < amount {
            return Err(OwgError::InvalidState(format!(
                "insufficient funds for {memo}"
            )));
        }
        let mut postings = vec![
            Posting::new(from, -amount),
            Posting::new(AccountKey::new(to_owner, AccountKind::Deposit), amount),
        ];
        if from_bank_index != to_bank_index {
            postings.push(Posting::new(
                AccountKey::new(
                    self.banks.ids[from_bank_index].clone(),
                    AccountKind::DepositLiability,
                ),
                amount,
            ));
            postings.push(Posting::new(
                AccountKey::new(
                    self.banks.ids[to_bank_index].clone(),
                    AccountKind::DepositLiability,
                ),
                -amount,
            ));
            // A deposit transfer between banks also settles in central-bank
            // reserves. Without these postings, the paying bank gains equity
            // and the receiving bank loses equity merely because a customer
            // changed banks, despite the globally balanced journal entry.
            postings.push(Posting::new(
                AccountKey::new(
                    self.banks.ids[from_bank_index].clone(),
                    AccountKind::Reserve,
                ),
                -amount,
            ));
            postings.push(Posting::new(
                AccountKey::new(self.banks.ids[to_bank_index].clone(), AccountKind::Reserve),
                amount,
            ));
        }
        let entry_id = self.next_entry_id("deposit-transfer");
        self.ledger.post(JournalEntry::new(
            entry_id,
            self.month,
            "settlement",
            memo,
            postings,
        ))
    }

    fn transfer_deposit_to_treasury(
        &mut self,
        from_owner: String,
        from_bank_index: usize,
        amount: Money,
        memo: &str,
    ) -> OwgResult<()> {
        let from = AccountKey::new(from_owner, AccountKind::Deposit);
        if self.ledger.balance(&from) < amount {
            return Err(OwgError::InvalidState(format!(
                "insufficient funds for {memo}"
            )));
        }
        let entry_id = self.next_entry_id("tax-transfer");
        self.ledger.post(JournalEntry::new(
            entry_id,
            self.month,
            "settlement",
            memo,
            vec![
                Posting::new(from, -amount),
                Posting::new(
                    AccountKey::new(
                        self.banks.ids[from_bank_index].clone(),
                        AccountKind::DepositLiability,
                    ),
                    amount,
                ),
                Posting::new(
                    AccountKey::new(
                        self.banks.ids[from_bank_index].clone(),
                        AccountKind::Reserve,
                    ),
                    -amount,
                ),
                Posting::new(
                    AccountKey::new(CENTRAL_BANK_ID, AccountKind::ReserveLiability),
                    amount,
                ),
                Posting::new(
                    AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury),
                    amount,
                ),
                Posting::new(
                    AccountKey::new(CENTRAL_BANK_ID, AccountKind::DepositLiability),
                    -amount,
                ),
            ],
        ))
    }

    fn transfer_treasury_to_deposit(
        &mut self,
        to_owner: String,
        to_bank_index: usize,
        amount: Money,
        memo: &str,
    ) -> OwgResult<()> {
        let treasury = AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury);
        if self.ledger.balance(&treasury) < amount {
            return Err(OwgError::InvalidState(format!(
                "insufficient treasury funds for {memo}"
            )));
        }
        let entry_id = self.next_entry_id("government-transfer");
        self.ledger.post(JournalEntry::new(
            entry_id,
            self.month,
            "settlement",
            memo,
            vec![
                Posting::new(treasury, -amount),
                Posting::new(
                    AccountKey::new(CENTRAL_BANK_ID, AccountKind::DepositLiability),
                    amount,
                ),
                Posting::new(
                    AccountKey::new(CENTRAL_BANK_ID, AccountKind::ReserveLiability),
                    -amount,
                ),
                Posting::new(
                    AccountKey::new(self.banks.ids[to_bank_index].clone(), AccountKind::Reserve),
                    amount,
                ),
                Posting::new(AccountKey::new(to_owner, AccountKind::Deposit), amount),
                Posting::new(
                    AccountKey::new(
                        self.banks.ids[to_bank_index].clone(),
                        AccountKind::DepositLiability,
                    ),
                    -amount,
                ),
            ],
        ))
    }

    #[must_use]
    pub(crate) fn bank_can_lend(&self, bank_index: usize, principal: Money) -> bool {
        let bank_id = self.banks.ids[bank_index].clone();
        let loans = self
            .ledger
            .balance(&AccountKey::new(bank_id.clone(), AccountKind::LoanAsset))
            .as_major_f64()
            + self
                .ledger
                .balance(&AccountKey::new(
                    bank_id.clone(),
                    AccountKind::ContractCreditAsset,
                ))
                .as_major_f64();
        let deposits = (-self.ledger.balance(&AccountKey::new(
            bank_id.clone(),
            AccountKind::DepositLiability,
        )))
        .as_major_f64();
        let reserves = self
            .ledger
            .balance(&AccountKey::new(bank_id.clone(), AccountKind::Reserve))
            .as_major_f64();
        let capital = (-self
            .ledger
            .balance(&AccountKey::new(bank_id, AccountKind::Equity)))
        .as_major_f64();
        let projected_loans = loans + principal.as_major_f64();
        let projected_deposits = deposits + principal.as_major_f64();
        let loss_adjusted_capital =
            capital - self.banks.expected_loss_rate[bank_index] * projected_loans;
        let capital_ratio = loss_adjusted_capital / projected_loans.max(1.0);
        let liquidity_ratio = reserves / projected_deposits.max(1.0);
        capital_ratio >= self.banks.minimum_capital_ratio[bank_index]
            && liquidity_ratio >= self.banks.minimum_liquidity_ratio[bank_index]
    }

    #[must_use]
    fn household_deposit(&self, household_index: usize) -> Money {
        self.owner_deposit(&self.households.ids[household_index])
    }

    #[must_use]
    fn firm_deposit(&self, firm_index: usize) -> Money {
        self.owner_deposit(&self.firms.ids[firm_index])
    }

    #[must_use]
    fn owner_deposit(&self, owner_id: &str) -> Money {
        self.ledger
            .balance(&AccountKey::new(owner_id, AccountKind::Deposit))
    }

    #[must_use]
    fn employee_counts(&self) -> Vec<usize> {
        let mut counts = vec![0; self.firms.len()];
        for firm_index in self.households.employer.iter().flatten() {
            if self.firms.active[*firm_index] {
                counts[*firm_index] += 1;
            }
        }
        counts
    }

    #[must_use]
    pub(crate) fn average_sector_prices(&self) -> [f64; 4] {
        let mut sums = [0.0; 4];
        let mut counts = [0_u32; 4];
        for index in 0..self.firms.len() {
            if self.firms.active[index] {
                let sector = self.firms.sector[index].index();
                sums[sector] += self.firms.price[index].as_major_f64();
                counts[sector] += 1;
            }
        }
        array::from_fn(|index| {
            if counts[index] == 0 {
                self.base_sector_prices[index].max(1.0)
            } else {
                sums[index] / f64::from(counts[index])
            }
        })
    }

    fn inventory_value(&self) -> f64 {
        self.firms
            .inventory
            .iter()
            .zip(&self.firms.price)
            .map(|(units, price)| units * price.as_major_f64())
            .sum()
    }

    fn next_entry_id(&mut self, prefix: &str) -> String {
        let id = format!(
            "{}-r{}-m{}-{}",
            prefix, self.replication, self.month, self.journal_sequence
        );
        self.journal_sequence += 1;
        id
    }
}

fn bounded_goods_settlement(
    requested: Money,
    available_units: f64,
    unit_price: Money,
) -> OwgResult<(Money, f64)> {
    if requested < Money::ZERO
        || !available_units.is_finite()
        || available_units < 0.0
        || unit_price <= Money::ZERO
    {
        return Err(OwgError::InvalidState(
            "invalid goods-settlement amount, inventory, or price".to_owned(),
        ));
    }
    let maximum_minor = available_units * unit_price.minor_units() as f64;
    if maximum_minor > i64::MAX as f64 {
        return Err(OwgError::MoneyOverflow);
    }
    let inventory_cap = Money(maximum_minor.floor() as i64);
    let settled = requested.min(inventory_cap);
    let units = settled.minor_units() as f64 / unit_price.minor_units() as f64;
    Ok((settled, units))
}

#[derive(Clone, Debug)]
struct MonthEffects {
    import_price: [f64; 4],
    productivity: [f64; 4],
    supply_capacity: [f64; 4],
    confidence: f64,
    credit_spread_delta: f64,
    policy_rate_delta: f64,
    tax_delta: [f64; 4],
    transfers: Vec<(Money, f64)>,
    public_investment: Money,
    energy_intensity_reduction: f64,
}

impl MonthEffects {
    fn for_month(
        scenario: &ScenarioSpec,
        branch: &PolicyBranch,
        governance: Option<&GovernanceState>,
        month: u32,
    ) -> OwgResult<Self> {
        let mut effects = Self {
            import_price: [1.0; 4],
            productivity: [1.0; 4],
            supply_capacity: [1.0; 4],
            confidence: 1.0,
            credit_spread_delta: 0.0,
            policy_rate_delta: 0.0,
            tax_delta: [0.0; 4],
            transfers: Vec::new(),
            public_investment: Money::ZERO,
            energy_intensity_reduction: 0.0,
        };
        for shock in &scenario.shocks {
            let intensity = window_intensity(
                month,
                shock.start_month,
                shock.duration_months,
                shock.decay_months,
            );
            if intensity <= 0.0 {
                continue;
            }
            match shock.kind {
                ShockKind::ImportPrice { sector, multiplier } => {
                    effects.import_price[sector.index()] *= 1.0 + (multiplier - 1.0) * intensity;
                }
                ShockKind::Productivity { sector, multiplier } => {
                    effects.productivity[sector.index()] *= 1.0 + (multiplier - 1.0) * intensity;
                }
                ShockKind::SupplyCapacity { sector, multiplier } => {
                    effects.supply_capacity[sector.index()] *= 1.0 + (multiplier - 1.0) * intensity;
                }
                ShockKind::Confidence { multiplier } => {
                    effects.confidence *= 1.0 + (multiplier - 1.0) * intensity;
                }
                ShockKind::CreditConditions {
                    spread_basis_points,
                } => {
                    effects.credit_spread_delta +=
                        f64::from(spread_basis_points) / 10_000.0 * intensity;
                }
            }
        }
        for action in &branch.actions {
            if month < action.start_month || month >= action.start_month + action.duration_months {
                continue;
            }
            effects.apply_policy_action(&action.kind)?;
        }
        if let Some(governance) = governance {
            for policy in governance.active_policies(month) {
                effects.apply_policy_action(&policy.action)?;
            }
        }
        Ok(effects)
    }

    fn apply_policy_action(&mut self, kind: &PolicyActionKind) -> OwgResult<()> {
        match kind {
            PolicyActionKind::PolicyRateDelta { basis_points } => {
                self.policy_rate_delta += f64::from(*basis_points) / 10_000.0;
            }
            PolicyActionKind::TargetedTransfer {
                amount_per_household,
                bottom_share,
            } => self.transfers.push((*amount_per_household, *bottom_share)),
            PolicyActionKind::IndirectTaxDelta {
                sector,
                basis_points,
            } => self.tax_delta[sector.index()] += f64::from(*basis_points) / 10_000.0,
            PolicyActionKind::PublicInvestment {
                monthly_amount,
                energy_intensity_reduction,
            } => {
                self.public_investment = self.public_investment.checked_add(*monthly_amount)?;
                self.energy_intensity_reduction += energy_intensity_reduction;
            }
        }
        Ok(())
    }
}

fn window_intensity(month: u32, start: u32, duration: u32, decay: u32) -> f64 {
    if month < start {
        return 0.0;
    }
    let elapsed = month - start;
    if elapsed < duration {
        return 1.0;
    }
    if decay == 0 || elapsed >= duration + decay {
        return 0.0;
    }
    1.0 - f64::from(elapsed - duration + 1) / f64::from(decay)
}

#[cfg(test)]
mod tests {
    use super::{
        CENTRAL_BANK_ID, DirectOwnerCashTransfer, Economy, GOVERNMENT_ID, MonthFlows,
        OWNERSHIP_SECTOR_ID, bounded_goods_settlement, window_intensity,
    };
    use crate::contracts::{PolicyActionKind, ScenarioSpec};
    use crate::governance::{
        GOVERNANCE_VERSION, GovernanceChange, GovernanceProposal, GovernanceSpec, GroupStance,
        InfluenceGroup, InstitutionalGate, LawRule,
    };
    use crate::institutions::OwnershipStake;
    use crate::ledger::{AccountKey, AccountKind, JournalEntry, Posting};
    use crate::money::Money;

    #[test]
    fn shock_window_has_expected_shape() {
        assert_eq!(
            window_intensity(23, 24, 12, 12).to_bits(),
            0.0_f64.to_bits()
        );
        assert_eq!(
            window_intensity(24, 24, 12, 12).to_bits(),
            1.0_f64.to_bits()
        );
        assert_eq!(
            window_intensity(35, 24, 12, 12).to_bits(),
            1.0_f64.to_bits()
        );
        assert!(window_intensity(36, 24, 12, 12) < 1.0);
        assert_eq!(
            window_intensity(47, 24, 12, 12).to_bits(),
            0.0_f64.to_bits()
        );
    }

    #[test]
    fn goods_settlement_never_oversells_inventory_after_rounding() {
        let (amount, units) = bounded_goods_settlement(Money(10_000), 1.5, Money(199)).unwrap();
        assert_eq!(amount, Money(298));
        assert!(units <= 1.5);
        assert!((units * 1.99 - 2.98).abs() < 1e-12);
    }

    #[test]
    fn opening_economy_is_balanced() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let economy = Economy::new(&scenario, 0).unwrap();
        economy.ledger.assert_balanced().unwrap();
        economy.assert_deposit_claims_balanced().unwrap();
        assert_eq!(economy.households.len(), 1_000);
        assert_eq!(economy.firms.len(), 100);
        economy.institutions.validate().unwrap();
        assert_eq!(
            economy
                .institutions
                .ultimate_owners(&economy.firms.ids[0])
                .unwrap()
                .get(&economy.households.ids[0]),
            Some(&1.0)
        );
    }

    #[test]
    fn cross_bank_deposit_payment_settles_reserves_without_bank_windfall() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let from_owner = economy.households.ids[0].clone();
        let to_owner = economy.households.ids[1].clone();
        let from_bank = economy.banks.ids[0].clone();
        let to_bank = economy.banks.ids[1].clone();
        let amount = Money(101);
        let from_bank_before = economy.ledger.owner_balance(&from_bank).unwrap();
        let to_bank_before = economy.ledger.owner_balance(&to_bank).unwrap();
        let from_reserve_before = economy
            .ledger
            .balance(&AccountKey::new(&from_bank, AccountKind::Reserve));
        let to_reserve_before = economy
            .ledger
            .balance(&AccountKey::new(&to_bank, AccountKind::Reserve));
        let central_bank_before = economy.ledger.owner_balance(CENTRAL_BANK_ID).unwrap();
        let from_deposit_before = economy
            .ledger
            .balance(&AccountKey::new(&from_owner, AccountKind::Deposit));
        let to_deposit_before = economy
            .ledger
            .balance(&AccountKey::new(&to_owner, AccountKind::Deposit));

        economy
            .transfer_between_deposits(from_owner.clone(), 0, to_owner.clone(), 1, amount, "test")
            .unwrap();

        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(from_owner, AccountKind::Deposit)),
            from_deposit_before - amount
        );
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(to_owner, AccountKind::Deposit)),
            to_deposit_before + amount
        );
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(&from_bank, AccountKind::Reserve)),
            from_reserve_before - amount
        );
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(&to_bank, AccountKind::Reserve)),
            to_reserve_before + amount
        );
        assert_eq!(
            economy.ledger.owner_balance(&from_bank).unwrap(),
            from_bank_before
        );
        assert_eq!(
            economy.ledger.owner_balance(&to_bank).unwrap(),
            to_bank_before
        );
        assert_eq!(
            economy.ledger.owner_balance(CENTRAL_BANK_ID).unwrap(),
            central_bank_before
        );
        economy.ledger.assert_balanced().unwrap();
        economy.assert_deposit_claims_balanced().unwrap();
    }

    #[test]
    fn per_bank_deposit_audit_catches_globally_balanced_wrong_bank_transfer() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let from_owner = economy.households.ids[0].clone();
        let to_owner = economy.households.ids[1].clone();
        economy
            .ledger
            .post(JournalEntry::new(
                "wrong-bank-deposit-setup",
                0,
                "test",
                "balanced customer transfer that omits bank liabilities",
                vec![
                    Posting::new(AccountKey::new(from_owner, AccountKind::Deposit), Money(-1)),
                    Posting::new(AccountKey::new(to_owner, AccountKind::Deposit), Money(1)),
                ],
            ))
            .unwrap();
        economy.ledger.assert_balanced().unwrap();
        economy.assert_reserve_claims_balanced().unwrap();
        assert!(economy.assert_deposit_claims_balanced().is_err());
    }

    #[test]
    fn per_bank_deposit_audit_rejects_reassigned_or_unbanked_accounts() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        economy.households.bank_index[0] = 1;
        assert!(economy.assert_deposit_claims_balanced().is_err());
        economy.households.bank_index[0] = 0;
        economy
            .ledger
            .post(JournalEntry::new(
                "unbanked-deposit-setup",
                0,
                "test",
                "unknown depositor with balanced offset",
                vec![
                    Posting::new(AccountKey::new("UNBANKED", AccountKind::Deposit), Money(1)),
                    Posting::new(AccountKey::new("UNBANKED", AccountKind::Equity), Money(-1)),
                ],
            ))
            .unwrap();
        economy.ledger.assert_balanced().unwrap();
        assert!(economy.assert_deposit_claims_balanced().is_err());
    }

    #[test]
    fn per_bank_loan_audit_catches_globally_balanced_wrong_lender() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let borrower = economy.households.ids[0].clone();
        let bank_index = economy.households.bank_index[0];
        let other_index = (bank_index + 1) % economy.banks.len();
        assert_ne!(bank_index, other_index);
        let lender = economy.banks.ids[bank_index].clone();
        let other_bank = economy.banks.ids[other_index].clone();
        let principal = Money(1_000);
        economy
            .ledger
            .post(JournalEntry::new(
                "loan-counterparty-setup",
                0,
                "test",
                "matched test loan claim",
                vec![
                    Posting::new(AccountKey::new(&lender, AccountKind::LoanAsset), principal),
                    Posting::new(
                        AccountKey::new(&borrower, AccountKind::DebtLiability),
                        -principal,
                    ),
                ],
            ))
            .unwrap();
        economy.assert_loan_claims_balanced().unwrap();
        economy
            .ledger
            .post(JournalEntry::new(
                "wrong-lender-loan-setup",
                0,
                "test",
                "move asset to another bank without moving borrower relationship",
                vec![
                    Posting::new(AccountKey::new(lender, AccountKind::LoanAsset), -principal),
                    Posting::new(
                        AccountKey::new(other_bank, AccountKind::LoanAsset),
                        principal,
                    ),
                ],
            ))
            .unwrap();
        economy.ledger.assert_balanced().unwrap();
        economy.assert_reserve_claims_balanced().unwrap();
        economy.assert_deposit_claims_balanced().unwrap();
        assert!(economy.assert_loan_claims_balanced().is_err());
        let checkpoint = crate::checkpoint::Checkpoint::new(
            "a".repeat(64),
            "baseline".to_owned(),
            0,
            economy,
            Vec::new(),
        );
        assert!(checkpoint.validate_integrity().is_err());
    }

    #[test]
    fn per_bank_loan_audit_rejects_unassigned_claims_and_wrong_signs() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let bank_id = economy.banks.ids[0].clone();
        economy
            .ledger
            .post(JournalEntry::new(
                "unassigned-loan-setup",
                0,
                "test",
                "unknown borrower debt is not a valid bank asset counterpart",
                vec![
                    Posting::new(AccountKey::new(bank_id, AccountKind::LoanAsset), Money(1)),
                    Posting::new(
                        AccountKey::new("UNKNOWN-BORROWER", AccountKind::DebtLiability),
                        Money(-1),
                    ),
                ],
            ))
            .unwrap();
        economy.ledger.assert_balanced().unwrap();
        assert!(economy.assert_loan_claims_balanced().is_err());

        let mut economy = Economy::new(&scenario, 0).unwrap();
        let borrower = economy.households.ids[0].clone();
        let bank_id = economy.banks.ids[economy.households.bank_index[0]].clone();
        economy
            .ledger
            .post(JournalEntry::new(
                "wrong-sign-loan-setup",
                0,
                "test",
                "negative bank loan asset and positive borrower liability",
                vec![
                    Posting::new(AccountKey::new(bank_id, AccountKind::LoanAsset), Money(-1)),
                    Posting::new(
                        AccountKey::new(borrower, AccountKind::DebtLiability),
                        Money(1),
                    ),
                ],
            ))
            .unwrap();
        economy.ledger.assert_balanced().unwrap();
        assert!(economy.assert_loan_claims_balanced().is_err());
    }

    #[test]
    fn negative_customer_deposit_is_rejected_before_owner_cash_mutates() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let owner_id = economy.households.ids[0].clone();
        let same_bank_household = economy.households.ids[2].clone();
        let firm_id = economy.firms.ids[0].clone();
        let amount = economy
            .ledger
            .balance(&AccountKey::new(&owner_id, AccountKind::Deposit))
            + Money(1);
        economy
            .ledger
            .post(JournalEntry::new(
                "negative-customer-deposit-setup",
                0,
                "test",
                "balanced same-bank deposit move creates an unsupported overdraft",
                vec![
                    Posting::new(AccountKey::new(&owner_id, AccountKind::Deposit), -amount),
                    Posting::new(
                        AccountKey::new(same_bank_household, AccountKind::Deposit),
                        amount,
                    ),
                ],
            ))
            .unwrap();
        economy.ledger.assert_balanced().unwrap();
        economy.assert_reserve_claims_balanced().unwrap();
        assert!(economy.assert_deposit_claims_balanced().is_err());
        let before = economy.ledger.journal_hash();
        assert!(
            economy
                .settle_direct_owner_cash_transfer(
                    &firm_id,
                    &DirectOwnerCashTransfer {
                        authorizing_entity_id: firm_id.clone(),
                        paying_firm_id: firm_id.clone(),
                        recipient_owner_id: owner_id,
                        amount: Money(1),
                    },
                )
                .is_err()
        );
        assert_eq!(economy.ledger.journal_hash(), before);
    }

    #[test]
    fn fiscal_payments_settle_commercial_and_central_bank_reserves() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let taxpayer = economy.households.ids[0].clone();
        let recipient = economy.households.ids[1].clone();
        let from_bank = economy.banks.ids[0].clone();
        let to_bank = economy.banks.ids[1].clone();
        let amount = Money(101);
        let from_bank_before = economy.ledger.owner_balance(&from_bank).unwrap();
        let to_bank_before = economy.ledger.owner_balance(&to_bank).unwrap();
        let central_bank_before = economy.ledger.owner_balance(CENTRAL_BANK_ID).unwrap();
        let treasury_before = economy
            .ledger
            .balance(&AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury));
        let reserve_liability_before = economy.ledger.balance(&AccountKey::new(
            CENTRAL_BANK_ID,
            AccountKind::ReserveLiability,
        ));

        economy
            .transfer_deposit_to_treasury(taxpayer, 0, amount, "test tax")
            .unwrap();
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury)),
            treasury_before + amount
        );
        assert_eq!(
            economy.ledger.balance(&AccountKey::new(
                CENTRAL_BANK_ID,
                AccountKind::ReserveLiability
            )),
            reserve_liability_before + amount
        );
        assert_eq!(
            economy.ledger.owner_balance(&from_bank).unwrap(),
            from_bank_before
        );
        assert_eq!(
            economy.ledger.owner_balance(CENTRAL_BANK_ID).unwrap(),
            central_bank_before
        );

        economy
            .transfer_treasury_to_deposit(recipient, 1, amount, "test spending")
            .unwrap();
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury)),
            treasury_before
        );
        assert_eq!(
            economy.ledger.balance(&AccountKey::new(
                CENTRAL_BANK_ID,
                AccountKind::ReserveLiability
            )),
            reserve_liability_before
        );
        assert_eq!(
            economy.ledger.owner_balance(&from_bank).unwrap(),
            from_bank_before
        );
        assert_eq!(
            economy.ledger.owner_balance(&to_bank).unwrap(),
            to_bank_before
        );
        assert_eq!(
            economy.ledger.owner_balance(CENTRAL_BANK_ID).unwrap(),
            central_bank_before
        );
        economy.ledger.assert_balanced().unwrap();
    }

    #[test]
    fn reserve_claim_audit_rejects_globally_balanced_but_unpaired_reserves() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        economy
            .ledger
            .post(JournalEntry::new(
                "malformed-reserve-claim",
                0,
                "test",
                "reserve asset without a central-bank liability",
                vec![
                    Posting::new(
                        AccountKey::new(&economy.banks.ids[0], AccountKind::Reserve),
                        Money(100),
                    ),
                    Posting::new(
                        AccountKey::new(OWNERSHIP_SECTOR_ID, AccountKind::Equity),
                        Money(-100),
                    ),
                ],
            ))
            .unwrap();
        economy.ledger.assert_balanced().unwrap();
        assert!(economy.assert_reserve_claims_balanced().is_err());
    }

    #[test]
    fn initial_concentration_changes_ownership_not_cash_or_accounting() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[1].clone();
        let owner_id = economy.households.ids[0].clone();
        let journal_before = economy.ledger.journal_hash();
        let cash_before = economy
            .ledger
            .balance(&AccountKey::new(owner_id.clone(), AccountKind::Deposit));
        economy
            .set_initial_firm_ownership(
                &firm_id,
                vec![OwnershipStake {
                    owner_id: owner_id.clone(),
                    owned_id: firm_id.clone(),
                    basis_points: 10_000,
                }],
            )
            .unwrap();
        assert_eq!(
            economy
                .institutions
                .ultimate_owners(&firm_id)
                .unwrap()
                .get(&owner_id),
            Some(&1.0)
        );
        assert_eq!(economy.ledger.journal_hash(), journal_before);
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(owner_id, AccountKind::Deposit)),
            cash_before
        );
        economy.ledger.assert_balanced().unwrap();
        let branch = scenario
            .policy_branches
            .iter()
            .find(|branch| branch.id == "baseline")
            .unwrap();
        economy.step_month(&scenario, branch).unwrap();
        assert!(
            economy
                .set_initial_firm_ownership(
                    &firm_id,
                    vec![OwnershipStake {
                        owner_id: "HH-000000".to_owned(),
                        owned_id: firm_id.clone(),
                        basis_points: 10_000,
                    }],
                )
                .is_err()
        );
    }

    #[test]
    fn direct_owner_cash_transfer_moves_only_payer_and_direct_owner_cash() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[0].clone();
        let owner_id = economy.households.ids[1].clone();
        economy
            .set_initial_firm_ownership(
                &firm_id,
                vec![OwnershipStake {
                    owner_id: owner_id.clone(),
                    owned_id: firm_id.clone(),
                    basis_points: 10_000,
                }],
            )
            .unwrap();
        let payer_bank = economy.banks.ids[0].clone();
        let recipient_bank = economy.banks.ids[1].clone();
        let amount = Money(12_345);
        let firm_cash_before = economy
            .ledger
            .balance(&AccountKey::new(&firm_id, AccountKind::Deposit));
        let owner_cash_before = economy
            .ledger
            .balance(&AccountKey::new(&owner_id, AccountKind::Deposit));
        let payer_bank_before = economy.ledger.owner_balance(&payer_bank).unwrap();
        let recipient_bank_before = economy.ledger.owner_balance(&recipient_bank).unwrap();
        let payer_reserve_before = economy
            .ledger
            .balance(&AccountKey::new(&payer_bank, AccountKind::Reserve));
        let recipient_reserve_before = economy
            .ledger
            .balance(&AccountKey::new(&recipient_bank, AccountKind::Reserve));
        let central_bank_before = economy.ledger.owner_balance(CENTRAL_BANK_ID).unwrap();
        let journal_before = economy.ledger.audit().entry_count;

        economy
            .settle_direct_owner_cash_transfer(
                &firm_id,
                &DirectOwnerCashTransfer {
                    authorizing_entity_id: firm_id.clone(),
                    paying_firm_id: firm_id.clone(),
                    recipient_owner_id: owner_id.clone(),
                    amount,
                },
            )
            .unwrap();

        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(&firm_id, AccountKind::Deposit)),
            firm_cash_before - amount
        );
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(&owner_id, AccountKind::Deposit)),
            owner_cash_before + amount
        );
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(&payer_bank, AccountKind::Reserve)),
            payer_reserve_before - amount
        );
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(&recipient_bank, AccountKind::Reserve)),
            recipient_reserve_before + amount
        );
        assert_eq!(
            economy.ledger.owner_balance(&payer_bank).unwrap(),
            payer_bank_before
        );
        assert_eq!(
            economy.ledger.owner_balance(&recipient_bank).unwrap(),
            recipient_bank_before
        );
        assert_eq!(
            economy.ledger.owner_balance(CENTRAL_BANK_ID).unwrap(),
            central_bank_before
        );
        assert_eq!(economy.ledger.audit().entry_count, journal_before + 1);
        economy.ledger.assert_balanced().unwrap();
        economy.assert_reserve_claims_balanced().unwrap();
    }

    #[test]
    fn same_bank_owner_cash_transfer_needs_no_reserve_movement() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[0].clone();
        let owner_id = economy.households.ids[0].clone();
        let bank_id = economy.banks.ids[0].clone();
        let reserve_before = economy
            .ledger
            .balance(&AccountKey::new(&bank_id, AccountKind::Reserve));
        let liability_before = economy
            .ledger
            .balance(&AccountKey::new(&bank_id, AccountKind::DepositLiability));
        economy
            .settle_direct_owner_cash_transfer(
                &firm_id,
                &DirectOwnerCashTransfer {
                    authorizing_entity_id: firm_id.clone(),
                    paying_firm_id: firm_id.clone(),
                    recipient_owner_id: owner_id,
                    amount: Money(1),
                },
            )
            .unwrap();
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(&bank_id, AccountKind::Reserve)),
            reserve_before
        );
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(&bank_id, AccountKind::DepositLiability)),
            liability_before
        );
        economy.ledger.assert_balanced().unwrap();
        economy.assert_reserve_claims_balanced().unwrap();
    }

    #[test]
    fn direct_owner_cash_transfer_rejections_leave_journal_untouched() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[0].clone();
        let owner_id = economy.households.ids[0].clone();
        let unauthorized = DirectOwnerCashTransfer {
            authorizing_entity_id: economy.firms.ids[1].clone(),
            paying_firm_id: firm_id.clone(),
            recipient_owner_id: owner_id.clone(),
            amount: Money(1),
        };
        let unowned = DirectOwnerCashTransfer {
            authorizing_entity_id: firm_id.clone(),
            paying_firm_id: firm_id.clone(),
            recipient_owner_id: economy.households.ids[1].clone(),
            amount: Money(1),
        };
        let unbanked = DirectOwnerCashTransfer {
            recipient_owner_id: economy.firms.ids[1].clone(),
            ..unowned.clone()
        };
        let negative = DirectOwnerCashTransfer {
            recipient_owner_id: owner_id.clone(),
            amount: Money(-1),
            ..unowned.clone()
        };
        let too_large = DirectOwnerCashTransfer {
            recipient_owner_id: owner_id,
            amount: Money(i64::MAX),
            ..unowned.clone()
        };
        let hash_before = economy.ledger.journal_hash();
        let sequence_before = economy.journal_sequence;
        for request in [unauthorized, unowned, unbanked, negative, too_large] {
            assert!(
                economy
                    .settle_direct_owner_cash_transfer(&firm_id, &request)
                    .is_err()
            );
            assert_eq!(economy.ledger.journal_hash(), hash_before);
            assert_eq!(economy.journal_sequence, sequence_before);
        }
        let forged = DirectOwnerCashTransfer {
            authorizing_entity_id: firm_id.clone(),
            paying_firm_id: firm_id,
            recipient_owner_id: economy.households.ids[0].clone(),
            amount: Money(1),
        };
        let other_actor = economy.firms.ids[1].clone();
        assert!(
            economy
                .settle_direct_owner_cash_transfer(&other_actor, &forged)
                .is_err()
        );
        assert_eq!(economy.ledger.journal_hash(), hash_before);
        economy.firms.default_streak[0] = 1;
        assert!(
            economy
                .settle_direct_owner_cash_transfer(&forged.paying_firm_id, &forged)
                .is_err()
        );
        assert_eq!(economy.ledger.journal_hash(), hash_before);
    }

    #[test]
    fn direct_owner_cash_transfer_rejects_unfunded_reserve_settlement() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[0].clone();
        let owner_id = economy.households.ids[1].clone();
        economy
            .set_initial_firm_ownership(
                &firm_id,
                vec![OwnershipStake {
                    owner_id: owner_id.clone(),
                    owned_id: firm_id.clone(),
                    basis_points: 10_000,
                }],
            )
            .unwrap();
        let paying_bank_reserve = AccountKey::new(&economy.banks.ids[0], AccountKind::Reserve);
        let receiving_bank_reserve = AccountKey::new(&economy.banks.ids[1], AccountKind::Reserve);
        let drain = economy.ledger.balance(&paying_bank_reserve) - Money(1);
        economy
            .ledger
            .post(JournalEntry::new(
                "reserve-shortfall-setup",
                0,
                "test",
                "move reserves between banks while preserving aggregate claim",
                vec![
                    Posting::new(paying_bank_reserve, -drain),
                    Posting::new(receiving_bank_reserve, drain),
                ],
            ))
            .unwrap();
        economy.assert_reserve_claims_balanced().unwrap();
        let hash_before = economy.ledger.journal_hash();
        let payer_cash_before = economy
            .ledger
            .balance(&AccountKey::new(&firm_id, AccountKind::Deposit));
        let owner_cash_before = economy
            .ledger
            .balance(&AccountKey::new(&owner_id, AccountKind::Deposit));

        assert!(
            economy
                .settle_direct_owner_cash_transfer(
                    &firm_id,
                    &DirectOwnerCashTransfer {
                        authorizing_entity_id: firm_id.clone(),
                        paying_firm_id: firm_id.clone(),
                        recipient_owner_id: owner_id.clone(),
                        amount: Money(2),
                    },
                )
                .is_err()
        );
        assert_eq!(economy.ledger.journal_hash(), hash_before);
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(firm_id, AccountKind::Deposit)),
            payer_cash_before
        );
        assert_eq!(
            economy
                .ledger
                .balance(&AccountKey::new(owner_id, AccountKind::Deposit)),
            owner_cash_before
        );
    }

    #[test]
    fn settled_owner_cash_freezes_opening_ownership_even_in_month_zero() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[0].clone();
        let owner_id = economy.households.ids[0].clone();
        economy
            .settle_direct_owner_cash_transfer(
                &firm_id,
                &DirectOwnerCashTransfer {
                    authorizing_entity_id: firm_id.clone(),
                    paying_firm_id: firm_id.clone(),
                    recipient_owner_id: owner_id,
                    amount: Money(1),
                },
            )
            .unwrap();
        let original_stakes = economy.institutions.stakes.clone();
        let restored: Economy =
            serde_json::from_value(serde_json::to_value(economy).unwrap()).unwrap();
        let mut restored = restored;
        assert_eq!(
            restored.ledger.audit().entries_by_phase.get("ownership"),
            Some(&1)
        );
        assert!(
            restored
                .set_initial_firm_ownership(
                    &firm_id,
                    vec![OwnershipStake {
                        owner_id: restored.households.ids[1].clone(),
                        owned_id: firm_id.clone(),
                        basis_points: 10_000,
                    }],
                )
                .is_err()
        );
        assert_eq!(restored.institutions.stakes, original_stakes);
    }

    #[test]
    fn malformed_agent_columns_and_indices_fail_before_settlement_or_step() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let branch = &scenario.policy_branches[0];
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[0].clone();
        let owner_id = economy.households.ids[0].clone();
        let request = DirectOwnerCashTransfer {
            authorizing_entity_id: firm_id.clone(),
            paying_firm_id: firm_id.clone(),
            recipient_owner_id: owner_id,
            amount: Money(1),
        };
        let original_hash = economy.ledger.journal_hash();
        economy.firms.bank_index[0] = usize::MAX;
        assert!(
            economy
                .settle_direct_owner_cash_transfer(&firm_id, &request)
                .is_err()
        );
        assert!(economy.step_month(&scenario, branch).is_err());
        assert_eq!(economy.ledger.journal_hash(), original_hash);
        assert_eq!(economy.month, 0);

        economy.firms.bank_index[0] = 0;
        economy.households.monthly_income.pop();
        assert!(economy.assert_population_structure().is_err());
        assert!(
            economy
                .settle_direct_owner_cash_transfer(&firm_id, &request)
                .is_err()
        );
        assert_eq!(economy.ledger.journal_hash(), original_hash);
    }

    #[test]
    fn scenario_ownership_is_bound_at_opening_and_hashed() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        let default_hash = scenario.hash().unwrap();
        let baseline = Economy::new(&scenario, 0).unwrap();
        let firm_id = baseline.firms.ids[1].clone();
        let owner_id = baseline.households.ids[0].clone();
        let mut graph = baseline.institutions.clone();
        graph
            .set_initial_ownership(
                &firm_id,
                vec![OwnershipStake {
                    owner_id: owner_id.clone(),
                    owned_id: firm_id.clone(),
                    basis_points: 10_000,
                }],
            )
            .unwrap();
        scenario.initial_institutions = Some(graph);
        assert_ne!(scenario.hash().unwrap(), default_hash);
        let configured = Economy::new(&scenario, 0).unwrap();
        assert_eq!(
            configured.institutions,
            scenario.initial_institutions.unwrap()
        );
        assert_eq!(
            configured
                .institutions
                .ultimate_owners(&firm_id)
                .unwrap()
                .get(&owner_id),
            Some(&1.0)
        );
        assert_eq!(
            baseline.ledger.journal_hash(),
            configured.ledger.journal_hash()
        );
    }

    #[test]
    fn scenario_rejects_unbound_institution() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        let baseline = Economy::new(&scenario, 0).unwrap();
        let mut graph = baseline.institutions;
        let mut ghost = graph.entities[0].clone();
        ghost.id = "HH-GHOST".to_owned();
        graph.entities.push(ghost);
        graph.validate().unwrap();
        scenario.initial_institutions = Some(graph);
        assert!(Economy::new(&scenario, 0).is_err());
    }

    #[test]
    fn approved_governance_policy_affects_economy_after_review() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.governance = Some(GovernanceSpec {
            schema_version: GOVERNANCE_VERSION.to_owned(),
            groups: vec![
                InfluenceGroup {
                    id: "companies".to_owned(),
                    label: "Companies".to_owned(),
                    weight: 9.0,
                },
                InfluenceGroup {
                    id: "unions".to_owned(),
                    label: "Unions".to_owned(),
                    weight: 1.0,
                },
            ],
            gates: vec![InstitutionalGate {
                id: "monetary-gate".to_owned(),
                domain: "monetary".to_owned(),
                passage_threshold: 0.7,
                amendment_surcharge: 0.0,
                review_delay_months: 0,
            }],
            laws: vec![LawRule {
                id: "monetary-authority".to_owned(),
                domain: "monetary".to_owned(),
                enabled: true,
                resistance: 0.0,
            }],
            proposals: vec![GovernanceProposal {
                id: "rate-change".to_owned(),
                submitted_month: 0,
                domain: "monetary".to_owned(),
                sponsor_id: "companies".to_owned(),
                stances: vec![
                    GroupStance {
                        group_id: "companies".to_owned(),
                        value: 1.0,
                    },
                    GroupStance {
                        group_id: "unions".to_owned(),
                        value: -1.0,
                    },
                ],
                change: GovernanceChange::EconomicPolicy {
                    law_id: "monetary-authority".to_owned(),
                    duration_months: 1,
                    action: PolicyActionKind::PolicyRateDelta { basis_points: 100 },
                },
            }],
        });
        let mut baseline_scenario = scenario.clone();
        baseline_scenario.governance = None;
        let mut governed = Economy::new(&scenario, 0).unwrap();
        let mut baseline = Economy::new(&baseline_scenario, 0).unwrap();
        let branch = &scenario.policy_branches[0];
        let month_zero = governed.step_month(&scenario, branch).unwrap();
        let baseline_zero = baseline.step_month(&baseline_scenario, branch).unwrap();
        assert!((month_zero.policy_rate - baseline_zero.policy_rate).abs() < 1e-12);
        let month_one = governed.step_month(&scenario, branch).unwrap();
        let baseline_one = baseline.step_month(&baseline_scenario, branch).unwrap();
        assert!((month_one.policy_rate - baseline_one.policy_rate - 0.01).abs() < 1e-12);
        assert_eq!(month_one.ledger_residual_minor_units, 0);
        assert_eq!(governed.governance.as_ref().unwrap().events.len(), 2);
    }

    #[test]
    fn one_month_preserves_financial_balance() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let branch = scenario
            .policy_branches
            .iter()
            .find(|branch| branch.id == "baseline")
            .unwrap();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let metric = economy.step_month(&scenario, branch).unwrap();
        assert_eq!(metric.ledger_residual_minor_units, 0);
        assert!(metric.accounting_identity_relative_error < 1e-12);
        assert!(metric.income_accounts.identity_relative_error < 1e-12);
        assert_eq!(metric.institutional_balances.residual, Money::ZERO);
        assert_eq!(metric.institutional_balances.unclassified, Money::ZERO);
        assert!((metric.nominal_gdp - metric.income_accounts.nominal_gdp_income).abs() < 1e-9);
        assert_eq!(economy.schedule_audit.events_by_kind["goods_market"], 4);
        assert_eq!(economy.schedule_audit.event_count, 23);
    }

    #[test]
    fn indirect_tax_cut_reduces_tax_inclusive_energy_cpi() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let baseline = scenario
            .policy_branches
            .iter()
            .find(|branch| branch.id == "baseline")
            .unwrap()
            .clone();
        let mut tax_cut = scenario
            .policy_branches
            .iter()
            .find(|branch| branch.id == "essential-tax-cut")
            .unwrap()
            .clone();
        tax_cut.actions[0].start_month = 0;
        tax_cut.actions[0].duration_months = 1;

        let mut baseline_economy = Economy::new(&scenario, 0).unwrap();
        let baseline_metric = baseline_economy.step_month(&scenario, &baseline).unwrap();
        let mut tax_cut_economy = Economy::new(&scenario, 0).unwrap();
        let tax_cut_metric = tax_cut_economy.step_month(&scenario, &tax_cut).unwrap();

        assert!(tax_cut_metric.cpi_energy < baseline_metric.cpi_energy);
        assert!(tax_cut_metric.cpi < baseline_metric.cpi);
        assert_eq!(tax_cut_metric.ledger_residual_minor_units, 0);
        assert!(tax_cut_metric.accounting_identity_relative_error < 1e-12);
    }

    #[test]
    fn household_default_is_ledger_traceable() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        economy.households.employer[0] = Some(0);
        let mut flows = MonthFlows::default();
        let current = economy.household_deposit(0);
        economy
            .ensure_household_liquidity(0, current.checked_add(Money(30_000)).unwrap(), &mut flows)
            .unwrap();
        let debt_account = AccountKey::new(
            economy.households.ids[0].clone(),
            AccountKind::DebtLiability,
        );
        assert!(economy.ledger.balance(&debt_account) < Money::ZERO);
        let available = economy.household_deposit(0);
        economy
            .transfer_between_deposits(
                economy.households.ids[0].clone(),
                economy.households.bank_index[0],
                economy.firms.ids[0].clone(),
                economy.firms.bank_index[0],
                available,
                "test liquidity exhaustion",
            )
            .unwrap();
        for _ in 0..6 {
            economy.service_household_debt(&mut flows).unwrap();
            economy.month += 1;
        }
        assert_eq!(flows.household_defaults, 1);
        assert_eq!(economy.ledger.balance(&debt_account), Money::ZERO);
        economy.ledger.assert_balanced().unwrap();
    }

    #[test]
    fn firm_default_records_inventory_loss_and_releases_workers() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let firm_index = 0;
        economy.households.employer[0] = Some(firm_index);
        economy.firms.default_streak[firm_index] = 4;
        let expected_loss =
            economy.firms.inventory[firm_index] * economy.firms.price[firm_index].as_major_f64();
        let mut flows = MonthFlows::default();
        economy.resolve_defaults(&mut flows).unwrap();
        assert!(!economy.firms.active[firm_index]);
        assert_eq!(
            economy.firms.inventory[firm_index].to_bits(),
            0.0_f64.to_bits()
        );
        assert!(economy.households.employer[0].is_none());
        assert_eq!(flows.firm_defaults, 1);
        assert!((flows.inventory_losses_value - expected_loss).abs() < 1e-9);
        economy.ledger.assert_balanced().unwrap();
    }

    #[test]
    fn insolvent_bank_resolution_preserves_all_sector_balances() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut economy = Economy::new(&scenario, 0).unwrap();
        let bank_id = economy.banks.ids[0].clone();
        let bank_equity = AccountKey::new(bank_id.clone(), AccountKind::Equity);
        let current_equity = economy.ledger.balance(&bank_equity);
        let loss = Money(1_000_000).checked_sub(current_equity).unwrap();
        let entry_id = economy.next_entry_id("forced-bank-loss");
        economy
            .ledger
            .post(JournalEntry::new(
                entry_id,
                0,
                "test",
                "force bank insolvency",
                vec![
                    Posting::new(bank_equity.clone(), loss),
                    Posting::new(
                        AccountKey::new(OWNERSHIP_SECTOR_ID, AccountKind::Equity),
                        -loss,
                    ),
                ],
            ))
            .unwrap();
        let mut flows = MonthFlows::default();
        economy.resolve_banks(&mut flows).unwrap();
        assert_eq!(flows.bank_resolutions, 1);
        assert!(-economy.ledger.balance(&bank_equity) > Money::ZERO);
        assert!(
            economy.ledger.balance(&AccountKey::new(
                GOVERNMENT_ID,
                AccountKind::BankEquityAsset
            )) > Money::ZERO
        );
        economy.ledger.assert_balanced().unwrap();
    }
}
