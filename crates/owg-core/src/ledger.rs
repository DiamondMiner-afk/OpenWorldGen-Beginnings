use crate::error::{OwgError, OwgResult};
use crate::money::Money;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Deposit,
    DepositLiability,
    Cash,
    Reserve,
    ReserveLiability,
    LoanAsset,
    DebtLiability,
    ContractCreditAsset,
    ContractCreditLiability,
    GovernmentDebtAsset,
    GovernmentDebtLiability,
    BankEquityAsset,
    Treasury,
    TaxReceivable,
    Equity,
    OpeningResidual,
    OpeningOtherAsset,
    OpeningOtherLiability,
}

#[derive(
    Clone, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct AccountKey {
    pub owner_id: String,
    pub kind: AccountKind,
}

impl AccountKey {
    #[must_use]
    pub fn new(owner_id: impl Into<String>, kind: AccountKind) -> Self {
        Self {
            owner_id: owner_id.into(),
            kind,
        }
    }

    #[must_use]
    pub fn label(&self) -> String {
        format!("{}/{}", self.owner_id, serde_name(self.kind))
    }
}

fn serde_name(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Deposit => "deposit",
        AccountKind::DepositLiability => "deposit_liability",
        AccountKind::Cash => "cash",
        AccountKind::Reserve => "reserve",
        AccountKind::ReserveLiability => "reserve_liability",
        AccountKind::LoanAsset => "loan_asset",
        AccountKind::DebtLiability => "debt_liability",
        AccountKind::ContractCreditAsset => "contract_credit_asset",
        AccountKind::ContractCreditLiability => "contract_credit_liability",
        AccountKind::GovernmentDebtAsset => "government_debt_asset",
        AccountKind::GovernmentDebtLiability => "government_debt_liability",
        AccountKind::BankEquityAsset => "bank_equity_asset",
        AccountKind::Treasury => "treasury",
        AccountKind::TaxReceivable => "tax_receivable",
        AccountKind::Equity => "equity",
        AccountKind::OpeningResidual => "opening_residual",
        AccountKind::OpeningOtherAsset => "opening_other_asset",
        AccountKind::OpeningOtherLiability => "opening_other_liability",
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct Posting {
    pub account: AccountKey,
    pub delta: Money,
}

impl Posting {
    #[must_use]
    pub fn new(account: AccountKey, delta: Money) -> Self {
        Self { account, delta }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct JournalEntry {
    pub id: String,
    pub month: u32,
    pub phase: String,
    pub memo: String,
    pub postings: Vec<Posting>,
}

impl JournalEntry {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        month: u32,
        phase: impl Into<String>,
        memo: impl Into<String>,
        postings: Vec<Posting>,
    ) -> Self {
        Self {
            id: id.into(),
            month,
            phase: phase.into(),
            memo: memo.into(),
            postings,
        }
    }

    pub fn residual(&self) -> OwgResult<Money> {
        self.postings
            .iter()
            .try_fold(Money::ZERO, |sum, posting| sum.checked_add(posting.delta))
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct Ledger {
    #[serde(with = "balance_map")]
    #[schemars(with = "Vec<BalanceRecord>")]
    balances: BTreeMap<AccountKey, Money>,
    audit: LedgerAudit,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
struct BalanceRecord {
    account: AccountKey,
    balance: Money,
}

mod balance_map {
    use super::{
        AccountKey, BTreeMap, BalanceRecord, Deserialize, Deserializer, Money, Serialize,
        Serializer,
    };
    use serde::de::Error as _;

    pub fn serialize<S>(
        balances: &BTreeMap<AccountKey, Money>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        balances
            .iter()
            .map(|(account, balance)| BalanceRecord {
                account: account.clone(),
                balance: *balance,
            })
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<AccountKey, Money>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let records = Vec::<BalanceRecord>::deserialize(deserializer)?;
        let mut balances = BTreeMap::new();
        for record in records {
            if balances
                .insert(record.account.clone(), record.balance)
                .is_some()
            {
                return Err(D::Error::custom(format!(
                    "duplicate ledger account {}",
                    record.account.label()
                )));
            }
        }
        Ok(balances)
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct LedgerAudit {
    rolling_hash: [u8; 32],
    pub entry_count: u64,
    pub posting_count: u64,
    pub entries_by_phase: BTreeMap<String, u64>,
    pub entries_by_month: BTreeMap<u32, u64>,
    pub first_entry_id: Option<String>,
    pub last_entry_id: Option<String>,
}

impl Ledger {
    #[must_use]
    pub fn balance(&self, account: &AccountKey) -> Money {
        self.balances.get(account).copied().unwrap_or(Money::ZERO)
    }

    #[must_use]
    pub fn audit(&self) -> &LedgerAudit {
        &self.audit
    }

    #[must_use]
    pub fn balances(&self) -> &BTreeMap<AccountKey, Money> {
        &self.balances
    }

    pub fn owner_balance(&self, owner_id: &str) -> OwgResult<Money> {
        self.balances
            .iter()
            .filter(|(account, _)| account.owner_id == owner_id)
            .map(|(_, balance)| *balance)
            .try_fold(Money::ZERO, Money::checked_add)
    }

    pub fn post(&mut self, entry: JournalEntry) -> OwgResult<()> {
        if entry.postings.len() < 2 {
            return Err(OwgError::InvalidState(
                "journal entry requires at least two postings".to_owned(),
            ));
        }
        if self.audit.last_entry_id.as_deref() == Some(entry.id.as_str()) {
            return Err(OwgError::InvalidState(format!(
                "immediate duplicate journal entry id {}",
                entry.id
            )));
        }
        let residual = entry.residual()?;
        if residual != Money::ZERO {
            return Err(OwgError::UnbalancedEntry {
                residual: residual.minor_units(),
            });
        }
        let audit_overflow = || OwgError::InvalidState("journal audit counter overflow".to_owned());
        let next_entry_count = self
            .audit
            .entry_count
            .checked_add(1)
            .ok_or_else(audit_overflow)?;
        let posting_increment =
            u64::try_from(entry.postings.len()).map_err(|_| audit_overflow())?;
        let next_posting_count = self
            .audit
            .posting_count
            .checked_add(posting_increment)
            .ok_or_else(audit_overflow)?;
        let next_phase_count = self
            .audit
            .entries_by_phase
            .get(&entry.phase)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(audit_overflow)?;
        let next_month_count = self
            .audit
            .entries_by_month
            .get(&entry.month)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(audit_overflow)?;
        let encoded = serde_json::to_vec(&entry)?;
        let mut rolling = Sha256::new();
        rolling.update(self.audit.rolling_hash);
        rolling.update((encoded.len() as u64).to_le_bytes());
        rolling.update(encoded);
        let next_rolling_hash = rolling.finalize().into();

        let mut staged = BTreeMap::<AccountKey, Money>::new();
        for posting in &entry.postings {
            let current = staged
                .get(&posting.account)
                .copied()
                .unwrap_or_else(|| self.balance(&posting.account));
            staged.insert(posting.account.clone(), current.checked_add(posting.delta)?);
        }

        for (account, value) in staged {
            self.balances.insert(account, value);
        }
        self.audit.rolling_hash = next_rolling_hash;
        self.audit.entry_count = next_entry_count;
        self.audit.posting_count = next_posting_count;
        self.audit
            .entries_by_phase
            .insert(entry.phase.clone(), next_phase_count);
        self.audit
            .entries_by_month
            .insert(entry.month, next_month_count);
        if self.audit.first_entry_id.is_none() {
            self.audit.first_entry_id = Some(entry.id.clone());
        }
        self.audit.last_entry_id = Some(entry.id);
        Ok(())
    }

    pub fn transfer(
        &mut self,
        id: impl Into<String>,
        month: u32,
        phase: impl Into<String>,
        memo: impl Into<String>,
        from: AccountKey,
        to: AccountKey,
        amount: Money,
    ) -> OwgResult<()> {
        if amount < Money::ZERO {
            return Err(OwgError::InvalidState(
                "transfer amount cannot be negative".to_owned(),
            ));
        }
        self.post(JournalEntry::new(
            id,
            month,
            phase,
            memo,
            vec![Posting::new(from, -amount), Posting::new(to, amount)],
        ))
    }

    pub fn audit_global_balance(&self) -> OwgResult<Money> {
        self.balances
            .values()
            .copied()
            .try_fold(Money::ZERO, Money::checked_add)
    }

    pub fn assert_balanced(&self) -> OwgResult<()> {
        let residual = self.audit_global_balance()?;
        if residual != Money::ZERO {
            return Err(OwgError::InvalidState(format!(
                "global ledger residual is {residual}"
            )));
        }
        Ok(())
    }

    #[must_use]
    pub fn journal_hash(&self) -> String {
        hex::encode(self.audit.rolling_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::{AccountKey, AccountKind, JournalEntry, Ledger, Posting};
    use crate::error::OwgError;
    use crate::money::Money;

    #[test]
    fn rejects_unbalanced_entry_without_partial_commit() {
        let mut ledger = Ledger::default();
        let cash_a = AccountKey::new("a", AccountKind::Deposit);
        let cash_b = AccountKey::new("b", AccountKind::Deposit);
        let result = ledger.post(JournalEntry::new(
            "bad",
            0,
            "test",
            "bad transfer",
            vec![
                Posting::new(cash_a.clone(), Money(-100)),
                Posting::new(cash_b.clone(), Money(99)),
            ],
        ));
        assert!(result.is_err());
        assert_eq!(ledger.balance(&cash_a), Money::ZERO);
        assert_eq!(ledger.balance(&cash_b), Money::ZERO);
    }

    #[test]
    fn conserves_balances_across_many_transfers() {
        let mut ledger = Ledger::default();
        let source = AccountKey::new("source", AccountKind::Deposit);
        let sink = AccountKey::new("sink", AccountKind::Deposit);
        let issuance = AccountKey::new("system", AccountKind::OpeningResidual);
        ledger
            .post(JournalEntry::new(
                "opening",
                0,
                "opening",
                "open",
                vec![
                    Posting::new(source.clone(), Money(1_000_000)),
                    Posting::new(issuance, Money(-1_000_000)),
                ],
            ))
            .unwrap();
        for index in 0..1_000 {
            ledger
                .transfer(
                    format!("t-{index}"),
                    0,
                    "test",
                    "transfer",
                    source.clone(),
                    sink.clone(),
                    Money(100),
                )
                .unwrap();
        }
        assert_eq!(ledger.balance(&source), Money(900_000));
        assert_eq!(ledger.balance(&sink), Money(100_000));
        ledger.assert_balanced().unwrap();
    }

    #[test]
    fn ledger_json_round_trip_preserves_structured_accounts() {
        let mut ledger = Ledger::default();
        ledger
            .post(JournalEntry::new(
                "opening",
                0,
                "opening",
                "serialization round trip",
                vec![
                    Posting::new(
                        AccountKey::new("household", AccountKind::Deposit),
                        Money(100),
                    ),
                    Posting::new(
                        AccountKey::new("bank", AccountKind::DepositLiability),
                        Money(-100),
                    ),
                ],
            ))
            .unwrap();
        let encoded = serde_json::to_vec(&ledger).unwrap();
        let decoded: Ledger = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.journal_hash(), ledger.journal_hash());
        assert_eq!(decoded.balances(), ledger.balances());
        decoded.assert_balanced().unwrap();
    }

    #[test]
    fn saturated_deserialized_audit_counters_reject_without_partial_commit() {
        for counter in [
            "entry_count",
            "posting_count",
            "entries_by_phase",
            "entries_by_month",
        ] {
            let mut encoded = serde_json::to_value(Ledger::default()).unwrap();
            match counter {
                "entries_by_phase" => {
                    encoded["audit"][counter]["test"] = serde_json::json!(u64::MAX);
                }
                "entries_by_month" => encoded["audit"][counter]["0"] = serde_json::json!(u64::MAX),
                _ => encoded["audit"][counter] = serde_json::json!(u64::MAX),
            }
            let mut ledger: Ledger = serde_json::from_value(encoded).unwrap();
            let before = serde_json::to_value(&ledger).unwrap();
            let result = ledger.post(JournalEntry::new(
                "transfer",
                0,
                "test",
                "counter overflow must not commit balances",
                vec![
                    Posting::new(AccountKey::new("a", AccountKind::Deposit), Money(-100)),
                    Posting::new(AccountKey::new("b", AccountKind::Deposit), Money(100)),
                ],
            ));
            assert!(
                matches!(result, Err(OwgError::InvalidState(_))),
                "{counter}"
            );
            assert_eq!(serde_json::to_value(&ledger).unwrap(), before, "{counter}");
        }
    }
}
