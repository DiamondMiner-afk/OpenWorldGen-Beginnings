//! Scheduled, accounting-constrained contract credit. This is a settlement
//! mechanism, not an empirically estimated underwriting/bankruptcy model.
use crate::contracts::ScenarioSpec;
use crate::credit::{CreditBook, CreditContractSpec, CreditEvent, CreditEventKind, CreditStatus};
use crate::economy::Economy;
use crate::error::{OwgError, OwgResult};
use crate::ledger::{AccountKey, AccountKind, JournalEntry, Posting};
use crate::money::Money;
use std::collections::BTreeMap;

const OWNERSHIP: &str = "OWNERSHIP_SECTOR";

#[derive(Clone, Copy)]
struct Customer {
    bank: usize,
    household: Option<usize>,
    active: bool,
}

impl Economy {
    pub(crate) fn validate_credit_context(
        &self,
        scenario: &ScenarioSpec,
        boundary: bool,
    ) -> OwgResult<()> {
        match (&self.contract_credit, &scenario.credit_market) {
            (None, None) => Ok(()),
            (Some(book), Some(spec)) if &book.specification == spec => {
                spec.validate(scenario)?;
                book.validate_structure()?;
                if boundary && (book.next_month != self.month || book.originations_done) {
                    return Err(invalid("credit calendar is not at the simulation boundary"));
                }
                Ok(())
            }
            _ => Err(invalid("credit state differs from scenario terms")),
        }
    }

    pub(crate) fn contract_debt(&self, owner: &str) -> OwgResult<Money> {
        Money::ZERO.checked_sub(
            self.ledger
                .balance(&key(owner, AccountKind::ContractCreditLiability)),
        )
    }

    pub fn assert_contract_credit_balanced(&self) -> OwgResult<()> {
        let mut expected = BTreeMap::<AccountKey, Money>::new();
        if let Some(book) = &self.contract_credit {
            book.validate_structure()?;
            let customers = self.credit_customers();
            for term in &book.specification.contracts {
                if !customers.contains_key(&term.borrower_id)
                    || (!customers.contains_key(&term.lender_id)
                        && !self.banks.ids.contains(&term.lender_id))
                {
                    return Err(invalid("credit book has an unknown lender/borrower"));
                }
                let total = book.positions[&term.id].total()?;
                for (account, amount) in [
                    (
                        key(&term.lender_id, AccountKind::ContractCreditAsset),
                        total,
                    ),
                    (
                        key(&term.borrower_id, AccountKind::ContractCreditLiability),
                        -total,
                    ),
                ] {
                    let value = expected.entry(account).or_default();
                    *value = value.checked_add(amount)?;
                }
            }
        }
        for (account, balance) in self.ledger.balances() {
            if matches!(
                account.kind,
                AccountKind::ContractCreditAsset | AccountKind::ContractCreditLiability
            ) && expected.get(account).copied().unwrap_or_default() != *balance
            {
                return Err(invalid(format!(
                    "credit counterparty mismatch at {}",
                    account.label()
                )));
            }
        }
        for (account, value) in expected {
            if self.ledger.balance(&account) != value {
                return Err(invalid(format!(
                    "credit book claim missing at {}",
                    account.label()
                )));
            }
        }
        Ok(())
    }

    fn credit_customers(&self) -> BTreeMap<String, Customer> {
        self.households
            .ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                (
                    id.clone(),
                    Customer {
                        bank: self.households.bank_index[i],
                        household: Some(i),
                        active: true,
                    },
                )
            })
            .chain(self.firms.ids.iter().enumerate().map(|(i, id)| {
                (
                    id.clone(),
                    Customer {
                        bank: self.firms.bank_index[i],
                        household: None,
                        active: self.firms.active[i],
                    },
                )
            }))
            .collect()
    }

    /// Stage the complete phase, including counters, positions and postings.
    /// A failed interest/audit calculation cannot commit a partial settlement.
    fn credit_transaction(
        &mut self,
        operation: impl FnOnce(&mut Self, &mut CreditBook, &BTreeMap<String, Customer>) -> OwgResult<()>,
    ) -> OwgResult<()> {
        self.assert_population_structure()?;
        self.assert_loan_claims_balanced()?;
        let mut staged = self.clone();
        let customers = staged.credit_customers();
        let mut book = staged
            .contract_credit
            .take()
            .ok_or_else(|| invalid("missing contract credit book"))?;
        operation(&mut staged, &mut book, &customers)?;
        staged.contract_credit = Some(book);
        staged.ledger.assert_balanced()?;
        staged.assert_reserve_claims_balanced()?;
        staged.assert_deposit_claims_balanced()?;
        staged.assert_loan_claims_balanced()?;
        *self = staged;
        Ok(())
    }

    pub(crate) fn originate_contract_credit(&mut self, scenario: &ScenarioSpec) -> OwgResult<()> {
        self.validate_credit_context(scenario, true)?;
        self.credit_transaction(|world, book, customers| {
            let allowed = match (&book.specification.origination_law, &world.governance) {
                (None, None) => true,
                (Some(binding), Some(state))
                    if scenario.governance.as_ref().is_some_and(|spec| {
                        spec.schema_version == binding.governance_schema_version
                    }) && state.next_month
                        == world
                            .month
                            .checked_add(1)
                            .ok_or_else(|| invalid("month overflow"))? =>
                {
                    let matching: Vec<_> = state
                        .laws
                        .iter()
                        .filter(|law| law.id == binding.law_id && law.domain == binding.domain)
                        .collect();
                    if matching.len() != 1 {
                        return Err(invalid("missing/ambiguous current credit law"));
                    }
                    matching[0].enabled
                }
                _ => {
                    return Err(invalid(
                        "credit origination requires advanced, bound governance",
                    ));
                }
            };
            for term in ordered_terms(book) {
                if term.origination_month != world.month {
                    continue;
                }
                if book.positions[&term.id].status != CreditStatus::Scheduled {
                    return Err(invalid("credit originated more than once"));
                }
                let borrower = customers[&term.borrower_id];
                let lender_bank = world.banks.ids.iter().position(|id| id == &term.lender_id);
                let funded = if let Some(bank) = lender_bank {
                    borrower.active
                        && world.credit_bank_can_fund(bank, borrower.bank, term.principal)?
                } else {
                    let lender = customers[&term.lender_id];
                    borrower.active
                        && lender.active
                        && world.credit_available_cash(&term.lender_id, lender.bank, borrower.bank)
                            >= term.principal
                };
                let (status, kind) = if !allowed {
                    (CreditStatus::RejectedLaw, CreditEventKind::RejectedLaw)
                } else if !funded {
                    (
                        CreditStatus::RejectedFunding,
                        CreditEventKind::RejectedFunding,
                    )
                } else {
                    let mut postings = vec![
                        posting(
                            &term.lender_id,
                            AccountKind::ContractCreditAsset,
                            term.principal,
                        ),
                        posting(
                            &term.borrower_id,
                            AccountKind::ContractCreditLiability,
                            -term.principal,
                        ),
                    ];
                    if let Some(bank) = lender_bank {
                        postings.push(posting(
                            &term.borrower_id,
                            AccountKind::Deposit,
                            term.principal,
                        ));
                        postings.push(posting(
                            &world.banks.ids[borrower.bank],
                            AccountKind::DepositLiability,
                            -term.principal,
                        ));
                        world.credit_reserve_transfer(
                            &mut postings,
                            bank,
                            borrower.bank,
                            term.principal,
                        );
                    } else {
                        let lender = customers[&term.lender_id];
                        world.credit_customer_transfer(
                            &mut postings,
                            &term.lender_id,
                            lender.bank,
                            &term.borrower_id,
                            borrower.bank,
                            term.principal,
                        );
                    }
                    world.post_credit("origination", &term.id, postings)?;
                    book.positions
                        .get_mut(&term.id)
                        .expect("validated credit ID")
                        .principal = term.principal;
                    (
                        CreditStatus::Active,
                        if lender_bank.is_some() {
                            CreditEventKind::BankOrigination
                        } else {
                            CreditEventKind::CustomerOrigination
                        },
                    )
                };
                book.positions
                    .get_mut(&term.id)
                    .expect("validated credit ID")
                    .status = status;
                book.audit.record(event(
                    &term,
                    world.month,
                    kind,
                    if status == CreditStatus::Active {
                        term.principal
                    } else {
                        Money::ZERO
                    },
                    Money::ZERO,
                ))?;
            }
            book.originations_done = true;
            Ok(())
        })
    }

    pub(crate) fn service_contract_credit(&mut self, scenario: &ScenarioSpec) -> OwgResult<()> {
        self.validate_credit_context(scenario, false)?;
        self.credit_transaction(|world, book, customers| {
            if book.next_month != world.month || !book.originations_done {
                return Err(invalid("credit service is out of sequence"));
            }
            for term in ordered_terms(book) {
                if book.positions[&term.id].status != CreditStatus::Active {
                    continue;
                }
                let lender_bank = world.banks.ids.iter().position(|id| id == &term.lender_id);
                let position = book
                    .positions
                    .get_mut(&term.id)
                    .expect("validated credit ID");
                // No same-month interest, no interest on interest, integer cents.
                let interest = if world.month > term.origination_month {
                    position
                        .principal
                        .mul_ratio(term.annual_interest_basis_points, 120_000)?
                } else {
                    Money::ZERO
                };
                if interest > Money::ZERO {
                    position.interest_receivable =
                        position.interest_receivable.checked_add(interest)?;
                    let mut postings = vec![
                        posting(&term.lender_id, AccountKind::ContractCreditAsset, interest),
                        posting(
                            &term.borrower_id,
                            AccountKind::ContractCreditLiability,
                            -interest,
                        ),
                    ];
                    if lender_bank.is_some() {
                        postings.push(posting(&term.lender_id, AccountKind::Equity, -interest));
                        postings.push(posting(OWNERSHIP, AccountKind::Equity, interest));
                    }
                    world.post_credit("interest-accrual", &term.id, postings)?;
                    book.audit.record(event(
                        &term,
                        world.month,
                        CreditEventKind::InterestAccrued,
                        Money::ZERO,
                        interest,
                    ))?;
                }
                if world.month < term.first_payment_month {
                    continue;
                }
                let principal_due = if world.month >= term.maturity_month {
                    position.principal
                } else {
                    position
                        .principal
                        .mul_ratio(u32::from(term.monthly_principal_basis_points), 10_000)?
                        .min(position.principal)
                };
                let borrower = customers[&term.borrower_id];
                let target_bank = lender_bank.unwrap_or_else(|| customers[&term.lender_id].bank);
                let available =
                    world.credit_available_cash(&term.borrower_id, borrower.bank, target_bank);
                let paid_interest = available.min(position.interest_receivable);
                let paid_principal = available.checked_sub(paid_interest)?.min(principal_due);
                let total = paid_interest.checked_add(paid_principal)?;
                if total > Money::ZERO {
                    let mut postings = vec![
                        posting(&term.lender_id, AccountKind::ContractCreditAsset, -total),
                        posting(
                            &term.borrower_id,
                            AccountKind::ContractCreditLiability,
                            total,
                        ),
                    ];
                    if let Some(bank) = lender_bank {
                        postings.push(posting(&term.borrower_id, AccountKind::Deposit, -total));
                        postings.push(posting(
                            &world.banks.ids[borrower.bank],
                            AccountKind::DepositLiability,
                            total,
                        ));
                        world.credit_reserve_transfer(&mut postings, borrower.bank, bank, total);
                    } else {
                        let lender = customers[&term.lender_id];
                        world.credit_customer_transfer(
                            &mut postings,
                            &term.borrower_id,
                            borrower.bank,
                            &term.lender_id,
                            lender.bank,
                            total,
                        );
                        if let Some(index) = lender.household {
                            world.households.monthly_income[index] += paid_interest.as_major_f64();
                        }
                    }
                    world.post_credit("payment", &term.id, postings)?;
                    position.principal = position.principal.checked_sub(paid_principal)?;
                    position.interest_receivable =
                        position.interest_receivable.checked_sub(paid_interest)?;
                    book.audit.record(event(
                        &term,
                        world.month,
                        CreditEventKind::Payment,
                        paid_principal,
                        paid_interest,
                    ))?;
                }
                if position.total()? == Money::ZERO {
                    position.status = CreditStatus::Repaid;
                    position.missed_payments = 0;
                } else if position.interest_receivable > Money::ZERO
                    || paid_principal < principal_due
                {
                    position.missed_payments = position
                        .missed_payments
                        .checked_add(1)
                        .ok_or_else(|| invalid("missed payment count overflow"))?;
                    if position.missed_payments >= term.missed_payment_limit {
                        world.write_off_credit(book, &term)?;
                    }
                } else {
                    position.missed_payments = 0;
                }
            }
            book.next_month = world
                .month
                .checked_add(1)
                .ok_or_else(|| invalid("month overflow"))?;
            book.originations_done = false;
            Ok(())
        })
    }

    pub(crate) fn resolve_inactive_contract_borrowers(
        &mut self,
        scenario: &ScenarioSpec,
    ) -> OwgResult<()> {
        self.validate_credit_context(scenario, false)?;
        self.credit_transaction(|world, book, customers| {
            if book.next_month
                != world
                    .month
                    .checked_add(1)
                    .ok_or_else(|| invalid("month overflow"))?
                || book.originations_done
            {
                return Err(invalid("credit resolution must follow service"));
            }
            for term in ordered_terms(book) {
                if book.positions[&term.id].status == CreditStatus::Active
                    && !customers[&term.borrower_id].active
                {
                    world.write_off_credit(book, &term)?;
                }
            }
            Ok(())
        })
    }

    fn write_off_credit(
        &mut self,
        book: &mut CreditBook,
        term: &CreditContractSpec,
    ) -> OwgResult<()> {
        let position = book
            .positions
            .get_mut(&term.id)
            .expect("validated credit ID");
        let total = position.total()?;
        let mut postings = vec![
            posting(&term.lender_id, AccountKind::ContractCreditAsset, -total),
            posting(
                &term.borrower_id,
                AccountKind::ContractCreditLiability,
                total,
            ),
        ];
        if self.banks.ids.contains(&term.lender_id) {
            postings.push(posting(&term.lender_id, AccountKind::Equity, total));
            postings.push(posting(OWNERSHIP, AccountKind::Equity, -total));
        }
        self.post_credit("write-off", &term.id, postings)?;
        book.audit.record(event(
            term,
            self.month,
            CreditEventKind::WriteOff,
            position.principal,
            position.interest_receivable,
        ))?;
        position.principal = Money::ZERO;
        position.interest_receivable = Money::ZERO;
        position.status = CreditStatus::WrittenOff;
        Ok(())
    }

    fn credit_available_cash(&self, owner: &str, from_bank: usize, to_bank: usize) -> Money {
        let deposit = self
            .ledger
            .balance(&key(owner, AccountKind::Deposit))
            .max(Money::ZERO);
        if from_bank == to_bank {
            deposit
        } else {
            deposit.min(
                self.ledger
                    .balance(&key(&self.banks.ids[from_bank], AccountKind::Reserve))
                    .max(Money::ZERO),
            )
        }
    }

    fn credit_bank_can_fund(
        &self,
        bank: usize,
        borrower_bank: usize,
        principal: Money,
    ) -> OwgResult<bool> {
        let id = &self.banks.ids[bank];
        let reserves = self.ledger.balance(&key(id, AccountKind::Reserve));
        let deposits = Money::ZERO
            .checked_sub(self.ledger.balance(&key(id, AccountKind::DepositLiability)))?;
        let (projected_reserves, projected_deposits) = if bank == borrower_bank {
            (reserves, deposits.checked_add(principal)?)
        } else {
            if reserves < principal {
                return Ok(false);
            }
            (reserves.checked_sub(principal)?, deposits)
        };
        let loans = self
            .ledger
            .balance(&key(id, AccountKind::LoanAsset))
            .checked_add(
                self.ledger
                    .balance(&key(id, AccountKind::ContractCreditAsset)),
            )?
            .checked_add(principal)?
            .as_major_f64();
        let capital = Money::ZERO
            .checked_sub(self.ledger.balance(&key(id, AccountKind::Equity)))?
            .as_major_f64();
        Ok(
            (capital - self.banks.expected_loss_rate[bank] * loans) / loans.max(1.0)
                >= self.banks.minimum_capital_ratio[bank]
                && projected_reserves.as_major_f64() / projected_deposits.as_major_f64().max(1.0)
                    >= self.banks.minimum_liquidity_ratio[bank],
        )
    }

    fn credit_reserve_transfer(
        &self,
        postings: &mut Vec<Posting>,
        from: usize,
        to: usize,
        amount: Money,
    ) {
        if from != to {
            postings.push(posting(
                &self.banks.ids[from],
                AccountKind::Reserve,
                -amount,
            ));
            postings.push(posting(&self.banks.ids[to], AccountKind::Reserve, amount));
        }
    }

    fn credit_customer_transfer(
        &self,
        postings: &mut Vec<Posting>,
        from: &str,
        from_bank: usize,
        to: &str,
        to_bank: usize,
        amount: Money,
    ) {
        postings.push(posting(from, AccountKind::Deposit, -amount));
        postings.push(posting(to, AccountKind::Deposit, amount));
        if from_bank != to_bank {
            postings.push(posting(
                &self.banks.ids[from_bank],
                AccountKind::DepositLiability,
                amount,
            ));
            postings.push(posting(
                &self.banks.ids[to_bank],
                AccountKind::DepositLiability,
                -amount,
            ));
            self.credit_reserve_transfer(postings, from_bank, to_bank, amount);
        }
    }

    fn post_credit(
        &mut self,
        phase: &str,
        contract: &str,
        postings: Vec<Posting>,
    ) -> OwgResult<()> {
        let next = self
            .journal_sequence
            .checked_add(1)
            .ok_or_else(|| invalid("credit journal sequence overflow"))?;
        self.ledger.post(JournalEntry::new(
            format!(
                "contract-credit-{}-{}-{}",
                self.replication, self.month, self.journal_sequence
            ),
            self.month,
            "contract-credit",
            format!("{phase}: {contract}"),
            postings,
        ))?;
        self.journal_sequence = next;
        Ok(())
    }
}

fn ordered_terms(book: &CreditBook) -> Vec<CreditContractSpec> {
    let mut terms = book.specification.contracts.clone();
    terms.sort_by(|left, right| {
        (left.payment_priority, &left.id).cmp(&(right.payment_priority, &right.id))
    });
    terms
}
fn key(owner: &str, kind: AccountKind) -> AccountKey {
    AccountKey::new(owner, kind)
}
fn posting(owner: &str, kind: AccountKind, amount: Money) -> Posting {
    Posting::new(key(owner, kind), amount)
}
fn invalid(message: impl Into<String>) -> OwgError {
    OwgError::InvalidState(message.into())
}
fn event(
    term: &CreditContractSpec,
    month: u32,
    kind: CreditEventKind,
    principal: Money,
    interest: Money,
) -> CreditEvent {
    CreditEvent {
        contract_id: term.id.clone(),
        lender_id: term.lender_id.clone(),
        borrower_id: term.borrower_id.clone(),
        month,
        kind,
        principal,
        interest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credit::{
        CREDIT_MARKET_VERSION, CreditLawBinding, CreditMarketSpec, CreditServiceOrder,
    };
    use crate::{RunOptions, SimulationEngine};

    fn term(id: &str, lender: &str, borrower: &str) -> CreditContractSpec {
        CreditContractSpec {
            id: id.into(),
            lender_id: lender.into(),
            borrower_id: borrower.into(),
            principal: Money(10_000),
            origination_month: 0,
            first_payment_month: 1,
            maturity_month: 5,
            annual_interest_basis_points: 1200,
            monthly_principal_basis_points: 2500,
            missed_payment_limit: 2,
            payment_priority: 0,
        }
    }

    fn scenario(contracts: Vec<CreditContractSpec>) -> ScenarioSpec {
        let mut spec = ScenarioSpec::inflation_lab_v0_1();
        spec.population.households = 12;
        spec.population.firms = 4;
        spec.calendar.months = 6;
        spec.shocks.clear();
        for branch in &mut spec.policy_branches {
            branch.actions.clear();
        }
        spec.credit_market = Some(CreditMarketSpec {
            schema_version: CREDIT_MARKET_VERSION.into(),
            service_order: CreditServiceOrder::BeforeLegacyDebt,
            origination_law: None,
            contracts,
        });
        spec
    }

    fn snapshot(world: &Economy) -> Vec<u8> {
        serde_json::to_vec(world).unwrap()
    }
    fn deposits(world: &Economy) -> Money {
        world
            .ledger
            .balances()
            .iter()
            .filter(|(key, _)| key.kind == AccountKind::Deposit)
            .try_fold(Money::ZERO, |sum, (_, value)| sum.checked_add(*value))
            .unwrap()
    }
    fn finish_credit_month(world: &mut Economy, spec: &ScenarioSpec) {
        world.service_contract_credit(spec).unwrap();
        world.resolve_inactive_contract_borrowers(spec).unwrap();
        world.month += 1;
    }

    #[test]
    fn bank_and_customer_origination_have_different_deposit_effects() {
        for lender in [
            "BANK-000",
            "BANK-001",
            "HH-000001",
            "HH-000002",
            "FIRM-00001",
        ] {
            let spec = scenario(vec![term("loan", lender, "HH-000000")]);
            let mut world = Economy::new(&spec, 0).unwrap();
            let initial = deposits(&world);
            let b0 = world.ledger.balance(&key("BANK-000", AccountKind::Reserve));
            let b1 = world.ledger.balance(&key("BANK-001", AccountKind::Reserve));
            world.originate_contract_credit(&spec).unwrap();
            assert_eq!(
                world.contract_credit.as_ref().unwrap().positions["loan"].status,
                CreditStatus::Active
            );
            assert_eq!(
                deposits(&world).checked_sub(initial).unwrap(),
                if lender.starts_with("BANK") {
                    Money(10_000)
                } else {
                    Money::ZERO
                }
            );
            let from = usize::from(
                lender == "BANK-001" || lender == "HH-000001" || lender == "FIRM-00001",
            );
            assert_eq!(
                world.ledger.balance(&key("BANK-000", AccountKind::Reserve)),
                b0.checked_add(if from == 1 {
                    Money(10_000)
                } else {
                    Money::ZERO
                })
                .unwrap()
            );
            assert_eq!(
                world.ledger.balance(&key("BANK-001", AccountKind::Reserve)),
                b1.checked_sub(if from == 1 {
                    Money(10_000)
                } else {
                    Money::ZERO
                })
                .unwrap()
            );
            finish_credit_month(&mut world, &spec);
            world.originate_contract_credit(&spec).unwrap();
            finish_credit_month(&mut world, &spec);
            let book = world.contract_credit.as_ref().unwrap();
            assert_eq!(book.positions["loan"].principal, Money(7_500));
            assert_eq!(book.audit.paid_interest, Money(100));
            assert_eq!(book.audit.paid_principal, Money(2_500));
            world.assert_contract_credit_balanced().unwrap();
        }
    }

    #[test]
    fn partial_interest_and_default_recognize_bank_loss_without_compounding() {
        let mut loan = term("loan", "BANK-001", "HH-000000");
        loan.monthly_principal_basis_points = 0;
        let spec = scenario(vec![loan]);
        let mut world = Economy::new(&spec, 0).unwrap();
        world.originate_contract_credit(&spec).unwrap();
        finish_credit_month(&mut world, &spec);
        // Leave 50 cents; transfer the rest with all bank counterparts.
        let balance = world
            .ledger
            .balance(&key("HH-000000", AccountKind::Deposit));
        let mut postings = vec![];
        world.credit_customer_transfer(
            &mut postings,
            "HH-000000",
            0,
            "HH-000002",
            0,
            balance.checked_sub(Money(50)).unwrap(),
        );
        world.post_credit("test-drain", "loan", postings).unwrap();
        let equity = world.ledger.balance(&key("BANK-001", AccountKind::Equity));
        world.originate_contract_credit(&spec).unwrap();
        finish_credit_month(&mut world, &spec);
        assert_eq!(
            world.contract_credit.as_ref().unwrap().positions["loan"].interest_receivable,
            Money(50)
        );
        world.originate_contract_credit(&spec).unwrap();
        finish_credit_month(&mut world, &spec);
        let book = world.contract_credit.as_ref().unwrap();
        assert_eq!(book.positions["loan"].status, CreditStatus::WrittenOff);
        assert_eq!(book.audit.accrued_interest, Money(200));
        assert_eq!(book.audit.paid_interest, Money(50));
        assert_eq!(book.audit.written_off, Money(10_150));
        assert_eq!(
            world.ledger.balance(&key("BANK-001", AccountKind::Equity)),
            equity.checked_add(Money(9_950)).unwrap()
        );
        assert_eq!(
            world
                .ledger
                .balance(&key("HH-000000", AccountKind::Deposit)),
            Money::ZERO
        );
    }

    #[test]
    fn payment_priority_is_explicit_and_not_contract_file_order() {
        let mut first = term("senior", "HH-000001", "HH-000000");
        first.payment_priority = -1;
        first.monthly_principal_basis_points = 10_000;
        first.annual_interest_basis_points = 0;
        let mut second = first.clone();
        second.id = "junior".into();
        second.lender_id = "HH-000002".into();
        second.payment_priority = 1;
        let mut hashes = vec![];
        for terms in [vec![first.clone(), second.clone()], vec![second, first]] {
            let spec = scenario(terms);
            let mut world = Economy::new(&spec, 0).unwrap();
            world.originate_contract_credit(&spec).unwrap();
            finish_credit_month(&mut world, &spec);
            let cash = world
                .ledger
                .balance(&key("HH-000000", AccountKind::Deposit));
            let mut postings = vec![];
            world.credit_customer_transfer(
                &mut postings,
                "HH-000000",
                0,
                "HH-000002",
                0,
                cash.checked_sub(Money(10_000)).unwrap(),
            );
            world
                .post_credit("test-drain", "priority", postings)
                .unwrap();
            world.originate_contract_credit(&spec).unwrap();
            finish_credit_month(&mut world, &spec);
            let book = world.contract_credit.as_ref().unwrap();
            assert_eq!(book.positions["senior"].status, CreditStatus::Repaid);
            assert_eq!(book.positions["junior"].principal, Money(10_000));
            hashes.push(world.ledger.journal_hash());
        }
        assert_eq!(hashes[0], hashes[1]);
    }

    #[test]
    fn changed_creditor_is_rejected_even_when_global_balance_closes() {
        let spec = scenario(vec![term("loan", "BANK-001", "HH-000000")]);
        let mut world = Economy::new(&spec, 0).unwrap();
        world.originate_contract_credit(&spec).unwrap();
        world
            .post_credit(
                "wrong-creditor",
                "loan",
                vec![
                    posting("BANK-001", AccountKind::ContractCreditAsset, Money(-10_000)),
                    posting("BANK-000", AccountKind::ContractCreditAsset, Money(10_000)),
                ],
            )
            .unwrap();
        world.ledger.assert_balanced().unwrap();
        assert!(world.assert_contract_credit_balanced().is_err());
    }

    #[test]
    fn failed_credit_phase_does_not_mutate_world_or_journal_sequence() {
        let spec = scenario(vec![term("loan", "BANK-001", "HH-000000")]);
        for audit_overflow in [true, false] {
            let mut world = Economy::new(&spec, 0).unwrap();
            if audit_overflow {
                world.contract_credit.as_mut().unwrap().audit.event_count = u64::MAX;
            } else {
                world.journal_sequence = u64::MAX;
            }
            let before = snapshot(&world);
            assert!(world.originate_contract_credit(&spec).is_err());
            assert_eq!(snapshot(&world), before);
        }
    }

    #[test]
    fn cash_and_reserve_constraints_reject_unfunded_customer_loan() {
        let mut loan = term("loan", "HH-000001", "HH-000000");
        loan.principal = Money(1_000_000_000_000);
        let spec = scenario(vec![loan]);
        let mut world = Economy::new(&spec, 0).unwrap();
        let before = deposits(&world);
        world.originate_contract_credit(&spec).unwrap();
        assert_eq!(
            world.contract_credit.as_ref().unwrap().positions["loan"].status,
            CreditStatus::RejectedFunding
        );
        assert_eq!(deposits(&world), before);
    }

    #[test]
    fn scheduled_run_and_serialized_checkpoint_resume_are_identical() {
        let spec = scenario(vec![
            term("bank", "BANK-001", "FIRM-00000"),
            term("customer", "HH-000001", "HH-000000"),
        ]);
        let engine = SimulationEngine::new(spec.clone()).unwrap();
        let checkpoint = engine.checkpoint("baseline", 0, 2).unwrap();
        let encoded = serde_json::to_vec(&checkpoint).unwrap();
        let restored = serde_json::from_slice(&encoded).unwrap();
        let resumed = engine.resume(restored, None).unwrap();
        let direct = engine.checkpoint("baseline", 0, 6).unwrap();
        assert_eq!(resumed.metrics, direct.metrics);
        assert_eq!(snapshot(&resumed.economy), snapshot(&direct.economy));
        assert_eq!(resumed.journal_hash, direct.journal_hash);
        let mut tampered = checkpoint.clone();
        tampered
            .economy
            .contract_credit
            .as_mut()
            .unwrap()
            .specification
            .contracts[0]
            .annual_interest_basis_points += 1;
        assert!(engine.resume(tampered, Some(2)).is_err());
        let mut tampered = checkpoint;
        tampered
            .economy
            .contract_credit
            .as_mut()
            .unwrap()
            .next_month += 1;
        assert!(tampered.validate_integrity().is_err());
        let result = engine
            .run(&RunOptions {
                branch_ids: Some(vec!["baseline".into()]),
                replications: 1,
                end_month: None,
            })
            .unwrap();
        assert_eq!(result.metrics, direct.metrics);
        assert_eq!(
            result.event_audit["baseline:0"].scheduler_version,
            crate::credit::CREDIT_SCHEDULER_VERSION
        );
        assert_eq!(
            result.event_audit["baseline:0"].scheduled_event_count,
            26 * 6
        );
        assert!(
            result.event_audit["baseline:0"]
                .contract_credit_audit
                .is_some()
        );
    }

    #[test]
    fn mutable_law_blocks_new_credit_without_erasing_existing_claims() {
        use crate::governance::*;
        let early = term("early", "BANK-001", "HH-000000");
        let mut late = early.clone();
        late.id = "late".into();
        late.origination_month = 1;
        late.first_payment_month = 2;
        let mut spec = scenario(vec![early, late]);
        spec.governance = Some(GovernanceSpec {
            schema_version: GOVERNANCE_VERSION.into(),
            groups: vec![InfluenceGroup {
                id: "public".into(),
                label: "Public".into(),
                weight: 1.0,
            }],
            gates: vec![InstitutionalGate {
                id: "credit-office".into(),
                domain: "credit".into(),
                passage_threshold: 0.5,
                amendment_surcharge: 0.0,
                review_delay_months: 0,
            }],
            laws: vec![LawRule {
                id: "credit-authority".into(),
                domain: "credit".into(),
                enabled: true,
                resistance: 0.0,
            }],
            proposals: vec![GovernanceProposal {
                id: "stop-new-credit".into(),
                submitted_month: 0,
                domain: "credit".into(),
                sponsor_id: "public".into(),
                stances: vec![GroupStance {
                    group_id: "public".into(),
                    value: 1.0,
                }],
                change: GovernanceChange::AmendLaw {
                    law_id: "credit-authority".into(),
                    enabled: false,
                    resistance: 0.0,
                },
            }],
        });
        spec.credit_market.as_mut().unwrap().origination_law = Some(CreditLawBinding {
            governance_schema_version: GOVERNANCE_VERSION.into(),
            law_id: "credit-authority".into(),
            domain: "credit".into(),
        });
        let engine = SimulationEngine::new(spec).unwrap();
        let checkpoint = engine.checkpoint("baseline", 0, 2).unwrap();
        let book = checkpoint.economy.contract_credit.unwrap();
        assert_eq!(book.positions["early"].status, CreditStatus::Active);
        assert_eq!(book.positions["late"].status, CreditStatus::RejectedLaw);
        assert_eq!(book.audit.rejected_contracts, 1);
    }

    #[test]
    fn inactive_firm_claims_are_resolved_before_bank_resolution() {
        let spec = scenario(vec![term("loan", "BANK-001", "FIRM-00000")]);
        let mut world = Economy::new(&spec, 0).unwrap();
        world.originate_contract_credit(&spec).unwrap();
        world.service_contract_credit(&spec).unwrap();
        world.firms.active[0] = false;
        world.resolve_inactive_contract_borrowers(&spec).unwrap();
        assert_eq!(
            world.contract_credit.as_ref().unwrap().positions["loan"].status,
            CreditStatus::WrittenOff
        );
        world.assert_contract_credit_balanced().unwrap();
    }
}
