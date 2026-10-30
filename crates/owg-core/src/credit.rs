//! Explicit customer/bank credit contracts. Terms are scenario inputs; this
//! mechanical layer makes no claim of calibrated lending or real legal power.

use crate::contracts::ScenarioSpec;
use crate::error::{OwgError, OwgResult};
use crate::money::Money;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CREDIT_MARKET_VERSION: &str = "owg.credit-market.v1";
pub const CREDIT_SCHEDULER_VERSION: &str = "openworldgen.scheduler.credit.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CreditServiceOrder {
    BeforeLegacyDebt,
    AfterLegacyDebt,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CreditLawBinding {
    pub governance_schema_version: String,
    pub law_id: String,
    pub domain: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CreditContractSpec {
    pub id: String,
    pub lender_id: String,
    pub borrower_id: String,
    pub principal: Money,
    pub origination_month: u32,
    pub first_payment_month: u32,
    pub maturity_month: u32,
    pub annual_interest_basis_points: u32,
    /// Percentage of outstanding principal due each month; maturity is a
    /// balloon for any remaining principal. Zero permits interest-only terms.
    pub monthly_principal_basis_points: u16,
    pub missed_payment_limit: u16,
    /// Lower numbers are serviced first; ties use contract ID, not file order.
    pub payment_priority: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CreditMarketSpec {
    pub schema_version: String,
    pub service_order: CreditServiceOrder,
    pub origination_law: Option<CreditLawBinding>,
    pub contracts: Vec<CreditContractSpec>,
}

impl CreditMarketSpec {
    pub fn validate(&self, scenario: &ScenarioSpec) -> OwgResult<()> {
        if self.schema_version != CREDIT_MARKET_VERSION || self.contracts.is_empty() {
            return Err(invalid("unsupported or empty credit market"));
        }
        match (&scenario.governance, &self.origination_law) {
            (Some(governance), Some(binding)) => {
                if binding.governance_schema_version != governance.schema_version
                    || binding.law_id.trim().is_empty()
                    || binding.domain.trim().is_empty()
                    || !governance
                        .laws
                        .iter()
                        .any(|law| law.id == binding.law_id && law.domain == binding.domain)
                {
                    return Err(invalid(
                        "credit origination law is not bound to scenario governance",
                    ));
                }
            }
            (None, None) => {}
            _ => return Err(invalid("credit law binding must match governance presence")),
        }
        let customers: BTreeSet<String> = (0..scenario.population.households)
            .map(|index| format!("HH-{index:06}"))
            .chain((0..scenario.population.firms).map(|index| format!("FIRM-{index:05}")))
            .collect();
        let banks: BTreeSet<String> = (0..scenario.population.banks)
            .map(|index| format!("BANK-{index:03}"))
            .collect();
        let mut ids = BTreeSet::new();
        for contract in &self.contracts {
            if contract.id.trim().is_empty()
                || !ids.insert(contract.id.as_str())
                || contract.lender_id == contract.borrower_id
                || !customers.contains(&contract.borrower_id)
                || (!customers.contains(&contract.lender_id)
                    && !banks.contains(&contract.lender_id))
                || contract.principal <= Money::ZERO
                || contract.origination_month >= scenario.calendar.months
                || contract.first_payment_month <= contract.origination_month
                || contract.first_payment_month > contract.maturity_month
                || contract.maturity_month <= contract.origination_month
                || contract.monthly_principal_basis_points > 10_000
                || contract.missed_payment_limit == 0
            {
                return Err(invalid(format!("invalid credit contract {}", contract.id)));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CreditStatus {
    Scheduled,
    Active,
    Repaid,
    WrittenOff,
    RejectedFunding,
    RejectedLaw,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CreditPosition {
    pub principal: Money,
    /// Simple interest on principal, kept separate from principal and not
    /// compounded. It is a recorded credit claim even before cash payment.
    pub interest_receivable: Money,
    pub missed_payments: u16,
    pub status: CreditStatus,
}

impl CreditPosition {
    pub fn total(&self) -> OwgResult<Money> {
        self.principal.checked_add(self.interest_receivable)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CreditEventKind {
    BankOrigination,
    CustomerOrigination,
    RejectedFunding,
    RejectedLaw,
    InterestAccrued,
    Payment,
    WriteOff,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CreditEvent {
    pub contract_id: String,
    pub lender_id: String,
    pub borrower_id: String,
    pub month: u32,
    pub kind: CreditEventKind,
    pub principal: Money,
    pub interest: Money,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CreditAudit {
    pub event_count: u64,
    pub rolling_hash: [u8; 32],
    pub bank_originated_principal: Money,
    pub customer_funded_principal: Money,
    pub accrued_interest: Money,
    pub paid_interest: Money,
    pub paid_principal: Money,
    pub written_off: Money,
    pub rejected_contracts: u64,
    pub last_event: Option<CreditEvent>,
}

impl CreditAudit {
    pub fn record(&mut self, event: CreditEvent) -> OwgResult<()> {
        let mut staged = self.clone();
        staged.record_inner(event)?;
        *self = staged;
        Ok(())
    }

    fn record_inner(&mut self, event: CreditEvent) -> OwgResult<()> {
        if event.principal < Money::ZERO || event.interest < Money::ZERO {
            return Err(invalid("negative credit audit amount"));
        }
        let count = self
            .event_count
            .checked_add(1)
            .ok_or_else(|| invalid("credit event count overflow"))?;
        let encoded = serde_json::to_vec(&event)?;
        let mut hash = Sha256::new();
        hash.update(self.rolling_hash);
        hash.update((encoded.len() as u64).to_le_bytes());
        hash.update(encoded);
        match event.kind {
            CreditEventKind::BankOrigination => {
                self.bank_originated_principal = self
                    .bank_originated_principal
                    .checked_add(event.principal)?;
            }
            CreditEventKind::CustomerOrigination => {
                self.customer_funded_principal = self
                    .customer_funded_principal
                    .checked_add(event.principal)?;
            }
            CreditEventKind::InterestAccrued => {
                self.accrued_interest = self.accrued_interest.checked_add(event.interest)?;
            }
            CreditEventKind::Payment => {
                self.paid_principal = self.paid_principal.checked_add(event.principal)?;
                self.paid_interest = self.paid_interest.checked_add(event.interest)?;
            }
            CreditEventKind::WriteOff => {
                self.written_off = self
                    .written_off
                    .checked_add(event.principal)?
                    .checked_add(event.interest)?;
            }
            CreditEventKind::RejectedFunding | CreditEventKind::RejectedLaw => {
                self.rejected_contracts = self
                    .rejected_contracts
                    .checked_add(1)
                    .ok_or_else(|| invalid("credit rejection count overflow"))?;
            }
        }
        self.event_count = count;
        self.rolling_hash = hash.finalize().into();
        self.last_event = Some(event);
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct CreditBook {
    pub specification: CreditMarketSpec,
    pub positions: BTreeMap<String, CreditPosition>,
    pub next_month: u32,
    pub originations_done: bool,
    pub audit: CreditAudit,
}

impl CreditBook {
    #[must_use]
    pub fn new(specification: CreditMarketSpec) -> Self {
        let positions = specification
            .contracts
            .iter()
            .map(|contract| {
                (
                    contract.id.clone(),
                    CreditPosition {
                        principal: Money::ZERO,
                        interest_receivable: Money::ZERO,
                        missed_payments: 0,
                        status: CreditStatus::Scheduled,
                    },
                )
            })
            .collect();
        Self {
            specification,
            positions,
            next_month: 0,
            originations_done: false,
            audit: CreditAudit::default(),
        }
    }

    pub fn validate_structure(&self) -> OwgResult<()> {
        let expected: BTreeSet<&str> = self
            .specification
            .contracts
            .iter()
            .map(|term| term.id.as_str())
            .collect();
        if self.specification.schema_version != CREDIT_MARKET_VERSION
            || expected.len() != self.specification.contracts.len()
            || self
                .positions
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                != expected
        {
            return Err(invalid(
                "credit book contract set differs from specification",
            ));
        }
        for term in &self.specification.contracts {
            let position = &self.positions[&term.id];
            if position.principal < Money::ZERO
                || position.interest_receivable < Money::ZERO
                || position.principal > term.principal
                || (position.status != CreditStatus::Active && position.total()? != Money::ZERO)
                || (position.status == CreditStatus::Active && position.total()? == Money::ZERO)
                || (position.status == CreditStatus::Scheduled
                    && (term.origination_month < self.next_month
                        || (self.originations_done && term.origination_month == self.next_month)))
                || (position.status != CreditStatus::Scheduled
                    && (term.origination_month > self.next_month
                        || (!self.originations_done && term.origination_month == self.next_month)))
            {
                return Err(invalid("invalid credit position balances/status"));
            }
        }
        let outstanding = self
            .positions
            .values()
            .try_fold(Money::ZERO, |sum, position| {
                sum.checked_add(position.total()?)
            })?;
        let expected = self
            .audit
            .bank_originated_principal
            .checked_add(self.audit.customer_funded_principal)?
            .checked_add(self.audit.accrued_interest)?
            .checked_sub(self.audit.paid_principal)?
            .checked_sub(self.audit.paid_interest)?
            .checked_sub(self.audit.written_off)?;
        if outstanding != expected {
            return Err(invalid("credit stock and cumulative flow audit disagree"));
        }
        Ok(())
    }

    pub fn metrics(&self) -> OwgResult<ContractCreditMetrics> {
        let mut result = ContractCreditMetrics {
            audit: self.audit.clone(),
            ..ContractCreditMetrics::default()
        };
        for position in self.positions.values() {
            result.principal = result.principal.checked_add(position.principal)?;
            result.interest_receivable = result
                .interest_receivable
                .checked_add(position.interest_receivable)?;
            if position.status == CreditStatus::Active {
                result.active_contracts += 1;
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ContractCreditMetrics {
    pub principal: Money,
    pub interest_receivable: Money,
    pub active_contracts: u64,
    pub audit: CreditAudit,
}

fn invalid(message: impl Into<String>) -> OwgError {
    OwgError::InvalidState(message.into())
}
