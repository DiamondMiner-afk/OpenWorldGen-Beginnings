use owg_core::{AccountKey, AccountKind, JournalEntry, Ledger, Money, Posting};

#[test]
fn canonical_bank_money_circuit_closes_stock_flow_accounts() {
    let firm_deposit = AccountKey::new("firm", AccountKind::Deposit);
    let firm_debt = AccountKey::new("firm", AccountKind::DebtLiability);
    let household_deposit = AccountKey::new("household", AccountKind::Deposit);
    let bank_deposits = AccountKey::new("bank", AccountKind::DepositLiability);
    let bank_loans = AccountKey::new("bank", AccountKind::LoanAsset);
    let mut ledger = Ledger::default();

    ledger
        .post(JournalEntry::new(
            "loan-origination",
            0,
            "credit",
            "bank creates a matched loan and deposit",
            vec![
                Posting::new(firm_deposit.clone(), Money(100_000)),
                Posting::new(bank_deposits.clone(), Money(-100_000)),
                Posting::new(bank_loans.clone(), Money(100_000)),
                Posting::new(firm_debt.clone(), Money(-100_000)),
            ],
        ))
        .unwrap();
    ledger
        .transfer(
            "wage",
            0,
            "payroll",
            "firm pays household",
            firm_deposit.clone(),
            household_deposit.clone(),
            Money(60_000),
        )
        .unwrap();
    ledger
        .post(JournalEntry::new(
            "principal",
            1,
            "settlement",
            "firm repays principal",
            vec![
                Posting::new(firm_deposit.clone(), Money(-30_000)),
                Posting::new(bank_deposits.clone(), Money(30_000)),
                Posting::new(firm_debt.clone(), Money(30_000)),
                Posting::new(bank_loans.clone(), Money(-30_000)),
            ],
        ))
        .unwrap();

    assert_eq!(ledger.balance(&firm_deposit), Money(10_000));
    assert_eq!(ledger.balance(&household_deposit), Money(60_000));
    assert_eq!(ledger.balance(&bank_deposits), Money(-70_000));
    assert_eq!(ledger.balance(&bank_loans), Money(70_000));
    assert_eq!(ledger.balance(&firm_debt), Money(-70_000));
    ledger.assert_balanced().unwrap();
}

#[test]
fn randomized_transfer_sequences_preserve_the_global_identity() {
    for seed in 1_u64..=32 {
        let mut state = seed;
        let mut ledger = Ledger::default();
        let accounts: Vec<AccountKey> = (0..8)
            .map(|index| AccountKey::new(format!("actor-{index}"), AccountKind::Deposit))
            .collect();
        ledger
            .post(JournalEntry::new(
                format!("opening-{seed}"),
                0,
                "opening",
                "randomized property-test opening",
                vec![
                    Posting::new(accounts[0].clone(), Money(1_000_000)),
                    Posting::new(
                        AccountKey::new("system", AccountKind::OpeningResidual),
                        Money(-1_000_000),
                    ),
                ],
            ))
            .unwrap();
        for step in 0..500 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let from = (state as usize) % accounts.len();
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let mut to = (state as usize) % accounts.len();
            if to == from {
                to = (to + 1) % accounts.len();
            }
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let amount = Money(((state % 2_000) + 1) as i64);
            ledger
                .transfer(
                    format!("{seed}-{step}"),
                    step,
                    "property",
                    "randomized transfer",
                    accounts[from].clone(),
                    accounts[to].clone(),
                    amount,
                )
                .unwrap();
            assert_eq!(ledger.audit_global_balance().unwrap(), Money::ZERO);
        }
    }
}
