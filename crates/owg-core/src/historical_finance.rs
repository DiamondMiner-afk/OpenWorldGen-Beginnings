use crate::economy::{
    CENTRAL_BANK_ID, Economy, GOVERNMENT_ID, OWNERSHIP_SECTOR_ID, REST_OF_WORLD_ID,
};
use crate::error::{OwgError, OwgResult};
use crate::ledger::{AccountKey, AccountKind, JournalEntry, Ledger, Posting};
use crate::money::Money;
use serde::{Deserialize, Serialize};

const VERSION: &str = "owg.historical-finance.v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinanceOpeningSpec {
    pub schema_version: String,
    pub source_sha256: String,
    pub banks: [BankOpeningRatios; 2],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BankOpeningRatios {
    pub cohort: String,
    pub deposit_share: f64,
    pub reserve_per_deposit: f64,
    pub household_loans_per_deposit: f64,
    pub firm_loans_per_deposit: f64,
    pub assets_per_deposit: f64,
    pub liabilities_per_deposit: f64,
}

#[derive(Debug, Serialize)]
pub struct OpeningFinanceReport {
    pub source_sha256: String,
    pub journal_sha256: String,
    pub banks: Vec<OpeningBankReport>,
}

#[derive(Debug, Serialize)]
pub struct OpeningBankReport {
    pub bank_id: String,
    pub cohort: String,
    pub deposits_minor: i64,
    pub reserves_minor: i64,
    pub household_loans_minor: i64,
    pub firm_loans_minor: i64,
    pub other_assets_minor: i64,
    pub other_liabilities_minor: i64,
    pub equity_minor: i64,
}

#[derive(Debug)]
struct Book {
    ledger: Ledger,
    sequence: u64,
}

impl Book {
    fn post(
        &mut self,
        name: &str,
        from: (&str, AccountKind),
        to: (&str, AccountKind),
        amount: Money,
    ) -> OwgResult<()> {
        if amount == Money::ZERO {
            return Ok(());
        }
        self.sequence += 1;
        self.ledger.post(JournalEntry::new(
            format!("opening-finance-{}", self.sequence),
            0,
            "opening",
            name,
            vec![
                Posting::new(AccountKey::new(from.0, from.1), amount),
                Posting::new(AccountKey::new(to.0, to.1), -amount),
            ],
        ))
    }
}

#[derive(Debug)]
struct Cohort<'a> {
    households: Vec<(&'a str, Money)>,
    firms: Vec<(&'a str, Money)>,
    foreign: Vec<(&'a str, Money)>,
}

fn distribute(owners: &[(&str, Money)], total: Money) -> OwgResult<Vec<Money>> {
    let weight: i128 = owners
        .iter()
        .map(|(_, amount)| i128::from(amount.minor_units()))
        .sum();
    if weight == 0 {
        return if total == Money::ZERO {
            Ok(vec![Money::ZERO; owners.len()])
        } else {
            Err(OwgError::InvalidState(
                "opening balance has no eligible counterparties".to_owned(),
            ))
        };
    }
    let mut position = 0_i128;
    let mut assigned = 0_i128;
    Ok(owners
        .iter()
        .map(|(_, amount)| {
            position += i128::from(amount.minor_units());
            let cumulative = position * i128::from(total.minor_units()) / weight;
            let piece = Money((cumulative - assigned) as i64);
            assigned = cumulative;
            piece
        })
        .collect())
}

fn total_deposit(economy: &Economy, owner: &str) -> Money {
    economy
        .ledger
        .balance(&AccountKey::new(owner, AccountKind::Deposit))
}

pub(crate) fn install(
    economy: &mut Economy,
    spec: FinanceOpeningSpec,
) -> OwgResult<OpeningFinanceReport> {
    let invalid = || OwgError::InvalidState("invalid historical finance opening".to_owned());
    if spec.schema_version != VERSION
        || spec.source_sha256.len() != 64
        || !spec.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || economy.banks.len() != spec.banks.len()
        || (spec
            .banks
            .iter()
            .map(|bank| bank.deposit_share)
            .sum::<f64>()
            - 1.0)
            .abs()
            > 1e-9
    {
        return Err(invalid());
    }
    for (row, label) in spec.banks.iter().zip(["large", "other_domestic"]) {
        if row.cohort != label
            || !(row.deposit_share > 0.0 && row.deposit_share < 1.0)
            || ![
                row.reserve_per_deposit,
                row.household_loans_per_deposit,
                row.firm_loans_per_deposit,
                row.assets_per_deposit,
                row.liabilities_per_deposit,
            ]
            .into_iter()
            .all(|value| (0.0..=10.0).contains(&value))
            || row.assets_per_deposit < row.liabilities_per_deposit
            || row.liabilities_per_deposit < 1.0
            || row.assets_per_deposit
                < row.reserve_per_deposit
                    + row.household_loans_per_deposit
                    + row.firm_loans_per_deposit
        {
            return Err(invalid());
        }
    }

    let mut groups: [Cohort<'_>; 2] = std::array::from_fn(|_| Cohort {
        households: Vec::new(),
        firms: Vec::new(),
        foreign: Vec::new(),
    });
    for (index, id) in economy.households.ids.iter().enumerate() {
        groups[economy.households.bank_index[index]]
            .households
            .push((id, total_deposit(economy, id)));
    }
    for (index, id) in economy.firms.ids.iter().enumerate() {
        groups[economy.firms.bank_index[index]]
            .firms
            .push((id, total_deposit(economy, id)));
    }
    groups[0]
        .foreign
        .push((REST_OF_WORLD_ID, total_deposit(economy, REST_OF_WORLD_ID)));
    let modeled_total = groups
        .iter()
        .flat_map(|group| {
            group
                .households
                .iter()
                .chain(&group.firms)
                .chain(&group.foreign)
        })
        .map(|(_, amount)| *amount)
        .try_fold(Money::ZERO, Money::checked_add)?;
    let large = modeled_total.mul_rate(spec.banks[0].deposit_share)?;
    let mut book = Book {
        ledger: Ledger::default(),
        sequence: 0,
    };
    let treasury = economy
        .ledger
        .balance(&AccountKey::new(GOVERNMENT_ID, AccountKind::Treasury));
    book.post(
        "opening treasury balance",
        (GOVERNMENT_ID, AccountKind::Treasury),
        (CENTRAL_BANK_ID, AccountKind::DepositLiability),
        treasury,
    )?;

    let mut banks = Vec::new();
    for (((row, group), id), deposits) in spec
        .banks
        .into_iter()
        .zip(groups)
        .zip(&economy.banks.ids)
        .zip([large, modeled_total.checked_sub(large)?])
    {
        let owners: Vec<_> = group
            .households
            .iter()
            .chain(&group.firms)
            .chain(&group.foreign)
            .copied()
            .collect();
        for ((owner, _), amount) in owners.iter().zip(distribute(&owners, deposits)?) {
            book.post(
                "opening deposit",
                (owner, AccountKind::Deposit),
                (id, AccountKind::DepositLiability),
                amount,
            )?;
        }
        let household_loans = deposits.mul_rate(row.household_loans_per_deposit)?;
        let firm_loans = deposits.mul_rate(row.firm_loans_per_deposit)?;
        for (borrowers, total) in [
            (&group.households, household_loans),
            (&group.firms, firm_loans),
        ] {
            for ((borrower, _), amount) in borrowers.iter().zip(distribute(borrowers, total)?) {
                book.post(
                    "opening loan claim",
                    (id, AccountKind::LoanAsset),
                    (borrower, AccountKind::DebtLiability),
                    amount,
                )?;
            }
        }
        let reserves = deposits.mul_rate(row.reserve_per_deposit)?;
        let assets = deposits.mul_rate(row.assets_per_deposit)?;
        let liabilities = deposits.mul_rate(row.liabilities_per_deposit)?;
        let other_assets = assets
            .checked_sub(reserves)?
            .checked_sub(household_loans)?
            .checked_sub(firm_loans)?;
        let other_liabilities = liabilities.checked_sub(deposits)?;
        let equity = assets.checked_sub(liabilities)?;
        if [other_assets, other_liabilities, equity]
            .into_iter()
            .any(|value| value < Money::ZERO)
        {
            return Err(invalid());
        }
        book.post(
            "opening reserve claims",
            (id, AccountKind::Reserve),
            (CENTRAL_BANK_ID, AccountKind::ReserveLiability),
            reserves,
        )?;
        book.post(
            "observed other assets, counterparties unresolved",
            (id, AccountKind::OpeningOtherAsset),
            (OWNERSHIP_SECTOR_ID, AccountKind::OpeningOtherLiability),
            other_assets,
        )?;
        book.post(
            "observed other liabilities, counterparties unresolved",
            (OWNERSHIP_SECTOR_ID, AccountKind::OpeningOtherAsset),
            (id, AccountKind::OpeningOtherLiability),
            other_liabilities,
        )?;
        book.post(
            "opening net assets as bank equity proxy",
            (OWNERSHIP_SECTOR_ID, AccountKind::Equity),
            (id, AccountKind::Equity),
            equity,
        )?;
        if book.ledger.owner_balance(id)? != Money::ZERO {
            return Err(invalid());
        }
        banks.push(OpeningBankReport {
            bank_id: id.clone(),
            cohort: row.cohort,
            deposits_minor: deposits.minor_units(),
            reserves_minor: reserves.minor_units(),
            household_loans_minor: household_loans.minor_units(),
            firm_loans_minor: firm_loans.minor_units(),
            other_assets_minor: other_assets.minor_units(),
            other_liabilities_minor: other_liabilities.minor_units(),
            equity_minor: equity.minor_units(),
        });
    }
    for owner in [GOVERNMENT_ID, CENTRAL_BANK_ID] {
        let residual = book.ledger.owner_balance(owner)?;
        book.post(
            "opening entity equity",
            (OWNERSHIP_SECTOR_ID, AccountKind::Equity),
            (owner, AccountKind::Equity),
            residual,
        )?;
    }
    book.ledger.assert_balanced()?;
    economy.ledger = book.ledger;
    economy.journal_sequence = book.sequence;
    economy.assert_reserve_claims_balanced()?;
    economy.assert_deposit_claims_balanced()?;
    economy.assert_loan_claims_balanced()?;
    Ok(OpeningFinanceReport {
        source_sha256: spec.source_sha256,
        journal_sha256: economy.ledger.journal_hash(),
        banks,
    })
}
