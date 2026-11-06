use crate::contracts::{PolicyBranch, ScenarioSpec, Sector};
use crate::economy::{Economy, ProductionFlows};
use crate::error::{OwgError, OwgResult};
use crate::historical_finance::{self, FinanceOpeningSpec, OpeningFinanceReport};
use crate::historical_households::HouseholdDemandOpening;
use crate::historical_labor::{HistoricalLabor, HistoricalLaborSpec, LaborFrame};
use crate::historical_production::{self, ProductionOpeningReport, ProductionOpeningSpec};
use crate::ledger::{AccountKey, AccountKind};
use crate::metrics::MetricRow;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SectorFrame {
    pub sector: Sector,
    pub active_firms: usize,
    pub workers: usize,
    pub sales: f64,
    pub household_spending: f64,
    pub inventory_value: f64,
}

#[derive(Debug, Serialize)]
pub struct BankFrame {
    pub id: String,
    pub deposits: f64,
    pub reserves: f64,
    pub loans: f64,
    pub equity: f64,
}

#[derive(Debug, Serialize)]
pub struct LiveFrame {
    pub metrics: MetricRow,
    pub labor: Option<LaborFrame>,
    pub sectors: Vec<SectorFrame>,
    pub banks: Vec<BankFrame>,
    pub production_flows: ProductionFlows,
    pub journal_sha256: String,
    pub schedule_sha256: String,
}

/// Incremental access to the scheduled engine for the local research console.
/// Historical inputs and state are bound to this run rather than a checkpoint.
#[derive(Debug)]
pub struct LiveRun<'a> {
    scenario: &'a ScenarioSpec,
    branch: &'a PolicyBranch,
    economy: Economy,
    labor: Option<HistoricalLabor>,
    opening_finance: Option<OpeningFinanceReport>,
    opening_production: Option<ProductionOpeningReport>,
    opening_household_demand: Option<HouseholdDemandOpening>,
}

impl<'a> LiveRun<'a> {
    pub(crate) fn new(
        scenario: &'a ScenarioSpec,
        branch: &'a PolicyBranch,
        mut economy: Economy,
        spec: Option<HistoricalLaborSpec>,
        finance: Option<FinanceOpeningSpec>,
        production: Option<ProductionOpeningSpec>,
    ) -> OwgResult<Self> {
        let opening_finance = finance
            .map(|input| historical_finance::install(&mut economy, input))
            .transpose()?;
        let opening_production = production
            .map(|input| {
                let labor = spec.as_ref().ok_or_else(|| {
                    OwgError::InvalidState(
                        "historical production requires a labor opening".to_owned(),
                    )
                })?;
                let (workers, jobless) = labor.opening_population(economy.households.len());
                historical_production::install(&mut economy, input, workers - jobless)
            })
            .transpose()?;
        let mut labor = spec
            .map(|input| {
                HistoricalLabor::initialize(input, &mut economy, opening_production.as_ref())
            })
            .transpose()?;
        let opening_household_demand = opening_production
            .as_ref()
            .and_then(|report| report.household_demand.clone())
            .zip(labor.as_mut())
            .map(|(policy, labor)| labor.bind_demand(&mut economy, policy))
            .transpose()?;
        Ok(Self {
            scenario,
            branch,
            economy,
            labor,
            opening_finance,
            opening_production,
            opening_household_demand,
        })
    }

    #[must_use]
    pub fn opening_finance(&self) -> Option<&OpeningFinanceReport> {
        self.opening_finance.as_ref()
    }

    #[must_use]
    pub fn opening_production(&self) -> Option<&ProductionOpeningReport> {
        self.opening_production.as_ref()
    }

    #[must_use]
    pub fn opening_household_demand(&self) -> Option<&HouseholdDemandOpening> {
        self.opening_household_demand.as_ref()
    }

    pub fn step(&mut self) -> OwgResult<LiveFrame> {
        let before = self
            .labor
            .as_ref()
            .map(|labor| labor.statuses(&self.economy));
        let (metrics, spending, production_flows) =
            self.economy
                .step_month_with_labor(self.scenario, self.branch, self.labor.as_mut())?;
        let labor = self
            .labor
            .as_ref()
            .zip(before)
            .map(|(labor, before)| labor.frame(&before, &self.economy));

        let mut sectors = Sector::ALL.map(|sector| SectorFrame {
            sector,
            active_firms: 0,
            workers: 0,
            sales: 0.0,
            household_spending: spending[sector.index()],
            inventory_value: 0.0,
        });
        for (firm, &sector) in self.economy.firms.sector.iter().enumerate() {
            let row = &mut sectors[sector.index()];
            row.active_firms += usize::from(self.economy.firms.active[firm]);
            row.sales += self.economy.firms.last_sales_value[firm];
            row.inventory_value +=
                self.economy.firms.inventory[firm] * self.economy.firms.price[firm].as_major_f64();
        }
        for &firm in self.economy.households.employer.iter().flatten() {
            sectors[self.economy.firms.sector[firm].index()].workers += 1;
        }
        let banks = self
            .economy
            .banks
            .ids
            .iter()
            .map(|id| {
                let balance = |kind| {
                    self.economy
                        .ledger
                        .balance(&AccountKey::new(id, kind))
                        .as_major_f64()
                };
                BankFrame {
                    id: id.clone(),
                    deposits: -balance(AccountKind::DepositLiability),
                    equity: -balance(AccountKind::Equity),
                    reserves: balance(AccountKind::Reserve),
                    loans: balance(AccountKind::LoanAsset)
                        + balance(AccountKind::ContractCreditAsset),
                }
            })
            .collect();
        Ok(LiveFrame {
            metrics,
            labor,
            sectors: sectors.into(),
            banks,
            production_flows,
            journal_sha256: self.economy.ledger.journal_hash(),
            schedule_sha256: self.economy.schedule_audit.schedule_hash(),
        })
    }
}
