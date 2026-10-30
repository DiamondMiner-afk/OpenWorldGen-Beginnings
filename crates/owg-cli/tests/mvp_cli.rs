use owg_core::{CreditContractSpec, CreditMarketSpec, CreditServiceOrder, Money, ScenarioSpec};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;

fn call(args: &[&str], root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_owg"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}
fn success(args: &[&str], root: &Path) {
    let result = call(args, root);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn read(root: &Path, file: &str) -> Value {
    serde_json::from_slice(&fs::read(root.join(file)).unwrap()).unwrap()
}
fn write(root: &Path, file: &str, value: &impl serde::Serialize) {
    fs::write(root.join(file), serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn credit_bundle_and_adaptive_firm_cli_replay_with_rejection_controls() {
    let root = std::env::temp_dir().join(format!(
        "owg-mvp-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let mut spec = ScenarioSpec::inflation_lab_v0_1();
    spec.calendar.months = 6;
    spec.population.households = 12;
    spec.population.firms = 4;
    spec.shocks.clear();
    for branch in &mut spec.policy_branches {
        branch.actions.clear();
    }
    spec.credit_market = Some(CreditMarketSpec {
        schema_version: owg_core::CREDIT_MARKET_VERSION.into(),
        service_order: CreditServiceOrder::AfterLegacyDebt,
        origination_law: None,
        contracts: vec![CreditContractSpec {
            id: "test-credit".into(),
            lender_id: "BANK-001".into(),
            borrower_id: "FIRM-00000".into(),
            principal: Money(10_000),
            origination_month: 0,
            first_payment_month: 1,
            maturity_month: 5,
            annual_interest_basis_points: 1200,
            monthly_principal_basis_points: 1000,
            missed_payment_limit: 2,
            payment_priority: 0,
        }],
    });
    write(&root, "scenario.json", &spec);
    success(
        &[
            "run",
            "scenario.json",
            "--branch",
            "baseline",
            "--output",
            "bundle",
        ],
        &root,
    );
    success(&["validate", "bundle", "--kind", "result-bundle"], &root);
    let csv = fs::read_to_string(root.join("bundle/metrics.csv")).unwrap();
    assert!(csv.contains("contract_credit_principal_minor_units"));
    let audit = read(&root, "bundle/event_audit.json");
    assert_eq!(
        audit["baseline:0"]["scheduler_version"],
        owg_core::CREDIT_SCHEDULER_VERSION
    );
    assert_eq!(
        audit["baseline:0"]["contract_credit_audit"]["bank_originated_principal"],
        10_000
    );

    success(
        &[
            "observe-firm",
            "scenario.json",
            "--output",
            "observation.json",
        ],
        &root,
    );
    let mut observation = read(&root, "observation.json");
    let mut tape = json!({"schema_version": "owg.decision-tape.v1", "id": "mvp-cli",
        "descriptor": {"id": "test-policy", "version": "1", "sha256": "a".repeat(64),
            "kind": "rule", "deterministic": true, "requires_network": false, "maximum_latency_ms": 1}, "steps": []});
    for month in 0..2 {
        let action = if month == 0 {
            json!({"type": "no_op"})
        } else {
            json!({"type": "direct_owner_cash_transfer", "recipient_owner_id": "HH-000000", "amount": 100})
        };
        tape["steps"].as_array_mut().unwrap().push(json!({"observations": {"observations": [observation]},
            "actions": {"actions": [action], "warnings": [], "used_fallback": false},
            "rationales": ["Synthetic test."], "expected_effects": ["Bounded accounting action."], "confidences": [1.0]}));
        write(&root, "tape.json", &tape);
        success(
            &[
                "run-recorded-firm",
                "scenario.json",
                "tape.json",
                "--output",
                "strategic.json",
            ],
            &root,
        );
        observation = read(&root, "strategic.json")["next_firm_observation"].clone();
        assert_eq!(observation["month"], month + 1);
    }
    success(
        &[
            "run-recorded-firm",
            "scenario.json",
            "tape.json",
            "--output",
            "replay.json",
        ],
        &root,
    );
    assert_eq!(read(&root, "strategic.json"), read(&root, "replay.json"));
    tape["steps"][0]["observations"]["observations"][0]["liquidity"] = json!(1);
    write(&root, "bad-tape.json", &tape);
    assert!(
        !call(
            &[
                "run-recorded-firm",
                "scenario.json",
                "bad-tape.json",
                "--output",
                "bad.json"
            ],
            &root
        )
        .status
        .success()
    );
    assert!(!root.join("bad.json").exists());

    // Rehashing the package file list must not bypass the manifest audit hash.
    let mut changed = audit;
    changed["baseline:0"]["contract_credit_audit"]["paid_interest"] = json!(99999);
    write(&root, "bundle/event_audit.json", &changed);
    let mut package = read(&root, "bundle/bundle.json");
    package["files"]["event_audit.json"] = json!(hex::encode(Sha256::digest(
        fs::read(root.join("bundle/event_audit.json")).unwrap()
    )));
    write(&root, "bundle/bundle.json", &package);
    assert!(
        !call(&["validate", "bundle", "--kind", "result-bundle"], &root)
            .status
            .success()
    );
    fs::remove_dir_all(&root).unwrap();
}
