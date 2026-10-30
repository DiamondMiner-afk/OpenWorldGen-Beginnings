//! Bounded dispatch of recorded firm choices into existing accounting.
//!
//! This is a reusable component, not yet part of `SimulationEngine`'s monthly
//! schedule or checkpoint contract. The opt-in law screen below is an
//! explicitly caller-bound scenario rule; it does not establish real-world
//! corporate-law permission, dividend qualification, tax treatment, or LLM
//! authenticity.

use crate::economy::{DirectOwnerCashTransfer, Economy};
use crate::error::{OwgError, OwgResult};
use crate::governance::GovernanceState;
use crate::ledger::{AccountKey, AccountKind};
use crate::money::Money;
use crate::policy::{AgentAction, Observation, ObservationBatch};
use crate::replay::RecordedPolicy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const FIRM_ACTION_LAW_BINDING_VERSION: &str = "owg.firm-action-law-binding.v1";

/// Caller-supplied mapping from the supported firm action to a mutable law.
/// The mapping must eventually be bound to a versioned scenario/checkpoint;
/// this component currently verifies only its contents and current state.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct FirmActionLawBinding {
    pub schema_version: String,
    pub governance_schema_version: String,
    pub domain: String,
    pub law_id: String,
}

impl FirmActionLawBinding {
    fn validate_against(&self, governance: &GovernanceState) -> OwgResult<()> {
        if self.schema_version != FIRM_ACTION_LAW_BINDING_VERSION
            || self.governance_schema_version != governance.dispatch_schema_version()
            || self.domain.trim().is_empty()
            || self.law_id.trim().is_empty()
        {
            return Err(OwgError::InvalidState(
                "firm action law binding is invalid or mismatched to governance state".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct RecordedLawAuthorization {
    pub binding_version: String,
    pub governance_version: String,
    pub domain: String,
    pub law_id: String,
    pub effective_month: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum RecordedFirmOutcome {
    NoOp {
        actor_id: String,
    },
    DirectOwnerCashTransfer {
        actor_id: String,
        recipient_owner_id: String,
        amount: Money,
        /// `None` means the action ran in a governance-free synthetic economy.
        /// A populated value records the scenario law used for screening; it
        /// is not an assertion of real-world legal compliance.
        law_authorization: Option<RecordedLawAuthorization>,
    },
}

/// Build a pre-month observation from state actually held by the economy.
/// No future-month metrics, scenario narrative, or model-supplied state enter it.
pub fn observe_firm(economy: &Economy, firm_id: &str) -> OwgResult<Observation> {
    economy.assert_population_structure()?;
    economy.ledger.assert_balanced()?;
    economy.assert_reserve_claims_balanced()?;
    economy.assert_deposit_claims_balanced()?;
    economy.assert_loan_claims_balanced()?;
    let index = economy
        .firms
        .ids
        .iter()
        .position(|id| id == firm_id)
        .ok_or_else(|| OwgError::InvalidState("recorded actor is not a bound firm".to_owned()))?;
    let debt_balance = economy
        .ledger
        .balance(&AccountKey::new(firm_id, AccountKind::DebtLiability));
    let debt = Money::ZERO
        .checked_sub(debt_balance)?
        .checked_add(economy.contract_debt(firm_id)?)?;
    if debt < Money::ZERO {
        return Err(OwgError::InvalidState(
            "firm debt account has an unsupported positive liability balance".to_owned(),
        ));
    }
    Ok(Observation {
        actor_id: firm_id.to_owned(),
        month: economy.month,
        inflation: economy.last_inflation,
        unemployment: economy.last_unemployment,
        policy_rate: economy.effective_policy_rate,
        liquidity: economy
            .ledger
            .balance(&AccountKey::new(firm_id, AccountKind::Deposit)),
        debt,
        inventory: economy.firms.inventory[index],
        expected_demand: economy.firms.expected_demand[index],
        sector: Some(economy.firms.sector[index]),
    })
}

/// Exact-match one recorded firm decision and apply only supported actions.
/// Failed actions leave both the economic state and the tape cursor untouched.
/// The paying entity comes from the generated observation, not the model.
pub fn dispatch_recorded_firm_action(
    economy: &mut Economy,
    policy: &RecordedPolicy,
    firm_id: &str,
) -> OwgResult<RecordedFirmOutcome> {
    dispatch_recorded_firm_action_inner(economy, policy, firm_id, None)
}

/// A governed economy can apply a transfer only through an explicit,
/// caller-supplied law binding. The ordinary dispatcher fails closed for
/// transfers when governance is present. The binding is not yet persisted in
/// the scenario, checkpoint, or result bundle, so this is an opt-in component
/// rather than the engine's authoritative legal decision path.
pub fn dispatch_recorded_firm_action_with_law(
    economy: &mut Economy,
    policy: &RecordedPolicy,
    firm_id: &str,
    binding: &FirmActionLawBinding,
) -> OwgResult<RecordedFirmOutcome> {
    let governance = economy.governance.as_ref().ok_or_else(|| {
        OwgError::InvalidState("law-screened dispatch requires governance state".to_owned())
    })?;
    binding.validate_against(governance)?;
    dispatch_recorded_firm_action_inner(economy, policy, firm_id, Some(binding))
}

fn dispatch_recorded_firm_action_inner(
    economy: &mut Economy,
    policy: &RecordedPolicy,
    firm_id: &str,
    binding: Option<&FirmActionLawBinding>,
) -> OwgResult<RecordedFirmOutcome> {
    let observation = observe_firm(economy, firm_id)?;
    let observations = ObservationBatch {
        observations: vec![observation],
    };
    policy.apply_next(&observations, |batch| {
        let action = batch.actions.as_slice();
        if action.len() != 1 {
            return Err(OwgError::InvalidState(
                "recorded firm dispatch requires exactly one action".to_owned(),
            ));
        }
        match &action[0] {
            AgentAction::NoOp => Ok(RecordedFirmOutcome::NoOp {
                actor_id: firm_id.to_owned(),
            }),
            AgentAction::DirectOwnerCashTransfer {
                recipient_owner_id,
                amount,
            } => {
                let law_authorization = match (&economy.governance, binding) {
                    (Some(governance), Some(binding)) => {
                        binding.validate_against(governance)?;
                        if !governance.law_enabled_for_dispatch(
                            &binding.law_id,
                            &binding.domain,
                            economy.month,
                        )? {
                            return Err(OwgError::InvalidState(
                                "direct owner transfer is disabled by the effective scenario law"
                                    .to_owned(),
                            ));
                        }
                        Some(RecordedLawAuthorization {
                            binding_version: binding.schema_version.clone(),
                            governance_version: binding.governance_schema_version.clone(),
                            domain: binding.domain.clone(),
                            law_id: binding.law_id.clone(),
                            effective_month: economy.month,
                        })
                    }
                    (Some(_), None) => {
                        return Err(OwgError::InvalidState(
                            "governed owner transfer requires an explicit law binding".to_owned(),
                        ));
                    }
                    (None, Some(_)) => {
                        return Err(OwgError::InvalidState(
                            "law binding cannot be used without governance state".to_owned(),
                        ));
                    }
                    (None, None) => None,
                };
                // The existing settlement primitive performs its own actor,
                // ownership, funds, and reserve checks. Commit its state only
                // after all checks and the final accounting audit succeed.
                let mut candidate = economy.clone();
                let request = DirectOwnerCashTransfer {
                    authorizing_entity_id: firm_id.to_owned(),
                    paying_firm_id: firm_id.to_owned(),
                    recipient_owner_id: recipient_owner_id.clone(),
                    amount: *amount,
                };
                candidate.settle_direct_owner_cash_transfer(firm_id, &request)?;
                candidate.ledger.assert_balanced()?;
                candidate.assert_reserve_claims_balanced()?;
                candidate.assert_deposit_claims_balanced()?;
                candidate.assert_loan_claims_balanced()?;
                *economy = candidate;
                Ok(RecordedFirmOutcome::DirectOwnerCashTransfer {
                    actor_id: firm_id.to_owned(),
                    recipient_owner_id: recipient_owner_id.clone(),
                    amount: *amount,
                    law_authorization,
                })
            }
            _ => Err(OwgError::InvalidState(
                "recorded firm action is unsupported by the bounded dispatcher".to_owned(),
            )),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        FIRM_ACTION_LAW_BINDING_VERSION, FirmActionLawBinding, RecordedFirmOutcome,
        RecordedLawAuthorization, dispatch_recorded_firm_action,
        dispatch_recorded_firm_action_with_law, observe_firm,
    };
    use crate::contracts::ScenarioSpec;
    use crate::economy::Economy;
    use crate::governance::{
        GOVERNANCE_VERSION, GovernanceChange, GovernanceProposal, GovernanceSpec, GroupStance,
        InfluenceGroup, InstitutionalGate, LawRule,
    };
    use crate::ledger::{AccountKey, AccountKind};
    use crate::money::Money;
    use crate::policy::{ActionBatch, AgentAction, ObservationBatch, PolicyDescriptor, PolicyKind};
    use crate::replay::{DECISION_TAPE_VERSION, DecisionStep, DecisionTape, RecordedPolicy};

    fn fixture() -> (Economy, String, String) {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[0].clone();
        let owner_id = economy
            .institutions
            .stakes
            .iter()
            .find(|stake| {
                stake.owned_id == firm_id && economy.households.ids.contains(&stake.owner_id)
            })
            .unwrap()
            .owner_id
            .clone();
        (economy, firm_id, owner_id)
    }

    fn policy_for(economy: &Economy, firm_id: &str, action: AgentAction) -> RecordedPolicy {
        let tape = DecisionTape {
            schema_version: DECISION_TAPE_VERSION.to_owned(),
            id: "firm-choice-fixture".to_owned(),
            descriptor: PolicyDescriptor {
                id: "recorded-local-choice".to_owned(),
                version: "1".to_owned(),
                sha256: "a".repeat(64),
                kind: PolicyKind::LargeLanguageModel,
                deterministic: false,
                requires_network: false,
                maximum_latency_ms: 10_000,
            },
            steps: vec![DecisionStep {
                observations: ObservationBatch {
                    observations: vec![observe_firm(economy, firm_id).unwrap()],
                },
                actions: ActionBatch {
                    actions: vec![action],
                    warnings: Vec::new(),
                    used_fallback: false,
                },
                rationales: vec!["synthetic test choice".to_owned()],
                expected_effects: vec!["bounded transfer".to_owned()],
                confidences: vec![0.5],
            }],
        };
        RecordedPolicy::new(tape).unwrap()
    }

    #[test]
    fn recorded_direct_owner_transfer_changes_only_balanced_accounts() {
        let (mut economy, firm_id, owner_id) = fixture();
        let amount = Money(100);
        let policy = policy_for(
            &economy,
            &firm_id,
            AgentAction::DirectOwnerCashTransfer {
                recipient_owner_id: owner_id.clone(),
                amount,
            },
        );
        let payer_key = AccountKey::new(&firm_id, AccountKind::Deposit);
        let owner_key = AccountKey::new(&owner_id, AccountKind::Deposit);
        let payer_before = economy.ledger.balance(&payer_key);
        let owner_before = economy.ledger.balance(&owner_key);
        let journal_before = economy.ledger.audit().entry_count;
        let result = dispatch_recorded_firm_action(&mut economy, &policy, &firm_id).unwrap();
        assert_eq!(
            result,
            RecordedFirmOutcome::DirectOwnerCashTransfer {
                actor_id: firm_id.clone(),
                recipient_owner_id: owner_id,
                amount,
                law_authorization: None,
            }
        );
        assert_eq!(economy.ledger.balance(&payer_key), payer_before - amount);
        assert_eq!(economy.ledger.balance(&owner_key), owner_before + amount);
        assert_eq!(economy.ledger.audit().entry_count, journal_before + 1);
        economy.ledger.assert_balanced().unwrap();
        economy.assert_reserve_claims_balanced().unwrap();
        economy.assert_deposit_claims_balanced().unwrap();
        economy.assert_loan_claims_balanced().unwrap();
        assert!(dispatch_recorded_firm_action(&mut economy, &policy, &firm_id).is_err());
    }

    #[test]
    fn no_op_is_consumed_without_mutating_economy() {
        let (mut economy, firm_id, _) = fixture();
        let before = serde_json::to_value(&economy).unwrap();
        let policy = policy_for(&economy, &firm_id, AgentAction::NoOp);
        let result = dispatch_recorded_firm_action(&mut economy, &policy, &firm_id).unwrap();
        assert_eq!(result, RecordedFirmOutcome::NoOp { actor_id: firm_id });
        assert_eq!(serde_json::to_value(&economy).unwrap(), before);
    }

    #[test]
    fn unsupported_and_infeasible_actions_commit_nothing_and_keep_cursor() {
        let (mut economy, firm_id, owner_id) = fixture();
        let before = serde_json::to_value(&economy).unwrap();
        let unsupported = policy_for(
            &economy,
            &firm_id,
            AgentAction::FirmPlan {
                price_change: 0.1,
                production_change: 0.1,
                vacancy_change: 0,
                investment: Money::ZERO,
            },
        );
        assert!(dispatch_recorded_firm_action(&mut economy, &unsupported, &firm_id).is_err());
        assert_eq!(serde_json::to_value(&economy).unwrap(), before);
        let observations = ObservationBatch {
            observations: vec![observe_firm(&economy, &firm_id).unwrap()],
        };
        unsupported.apply_next(&observations, |_| Ok(())).unwrap();

        let wrong_owner = economy
            .households
            .ids
            .iter()
            .find(|id| **id != owner_id)
            .unwrap()
            .clone();
        let wrong_owner_policy = policy_for(
            &economy,
            &firm_id,
            AgentAction::DirectOwnerCashTransfer {
                recipient_owner_id: wrong_owner,
                amount: Money(100),
            },
        );
        assert!(
            dispatch_recorded_firm_action(&mut economy, &wrong_owner_policy, &firm_id).is_err()
        );
        assert_eq!(serde_json::to_value(&economy).unwrap(), before);
        wrong_owner_policy
            .apply_next(&observations, |_| Ok(()))
            .unwrap();

        let excessive = observe_firm(&economy, &firm_id)
            .unwrap()
            .liquidity
            .checked_add(Money(1))
            .unwrap();
        let funds_policy = policy_for(
            &economy,
            &firm_id,
            AgentAction::DirectOwnerCashTransfer {
                recipient_owner_id: owner_id,
                amount: excessive,
            },
        );
        assert!(dispatch_recorded_firm_action(&mut economy, &funds_policy, &firm_id).is_err());
        assert_eq!(serde_json::to_value(&economy).unwrap(), before);
        funds_policy.apply_next(&observations, |_| Ok(())).unwrap();
    }

    #[test]
    fn changed_observation_and_wrong_actor_cannot_dispatch() {
        let (mut economy, firm_id, _) = fixture();
        let before = serde_json::to_value(&economy).unwrap();
        let policy = policy_for(&economy, &firm_id, AgentAction::NoOp);
        let original_inflation = economy.last_inflation;
        economy.last_inflation += 0.01;
        let changed = serde_json::to_value(&economy).unwrap();
        assert!(dispatch_recorded_firm_action(&mut economy, &policy, &firm_id).is_err());
        assert!(serde_json::to_value(&economy).unwrap() == changed);
        let other_firm = economy.firms.ids[1].clone();
        assert!(dispatch_recorded_firm_action(&mut economy, &policy, &other_firm).is_err());
        assert!(serde_json::to_value(&economy).unwrap() == changed);
        economy.last_inflation = original_inflation;
        assert!(serde_json::to_value(&economy).unwrap() == before);
        assert!(dispatch_recorded_firm_action(&mut economy, &policy, &firm_id).is_ok());
    }

    fn governed_fixture() -> (ScenarioSpec, Economy, String, String, FirmActionLawBinding) {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        let proposal = |id: &str, submitted_month: u32, enabled: bool| GovernanceProposal {
            id: id.to_owned(),
            submitted_month,
            domain: "corporate-distribution".to_owned(),
            sponsor_id: "legislature-coalition".to_owned(),
            stances: vec![GroupStance {
                group_id: "legislature-coalition".to_owned(),
                value: 1.0,
            }],
            change: GovernanceChange::AmendLaw {
                law_id: "owner-transfer-permission".to_owned(),
                enabled,
                resistance: 0.0,
            },
        };
        scenario.governance = Some(GovernanceSpec {
            schema_version: GOVERNANCE_VERSION.to_owned(),
            groups: vec![InfluenceGroup {
                id: "legislature-coalition".to_owned(),
                label: "Synthetic coalition".to_owned(),
                weight: 1.0,
            }],
            gates: vec![InstitutionalGate {
                id: "distribution-gate".to_owned(),
                domain: "corporate-distribution".to_owned(),
                passage_threshold: 0.5,
                amendment_surcharge: 0.0,
                review_delay_months: 0,
            }],
            laws: vec![LawRule {
                id: "owner-transfer-permission".to_owned(),
                domain: "corporate-distribution".to_owned(),
                enabled: true,
                resistance: 0.0,
            }],
            proposals: vec![
                proposal("disable-transfer", 0, false),
                proposal("reenable-transfer", 1, true),
            ],
        });
        let economy = Economy::new(&scenario, 0).unwrap();
        let firm_id = economy.firms.ids[0].clone();
        let owner_id = economy
            .institutions
            .stakes
            .iter()
            .find(|stake| stake.owned_id == firm_id)
            .unwrap()
            .owner_id
            .clone();
        let binding = FirmActionLawBinding {
            schema_version: FIRM_ACTION_LAW_BINDING_VERSION.to_owned(),
            governance_schema_version: GOVERNANCE_VERSION.to_owned(),
            domain: "corporate-distribution".to_owned(),
            law_id: "owner-transfer-permission".to_owned(),
        };
        (scenario, economy, firm_id, owner_id, binding)
    }

    #[test]
    fn governed_transfer_requires_law_binding_and_tracks_effective_amendments() {
        let (scenario, mut economy, firm_id, owner_id, binding) = governed_fixture();
        let action = AgentAction::DirectOwnerCashTransfer {
            recipient_owner_id: owner_id.clone(),
            amount: Money(100),
        };
        let policy = policy_for(&economy, &firm_id, action.clone());
        let before = serde_json::to_value(&economy).unwrap();
        assert!(dispatch_recorded_firm_action(&mut economy, &policy, &firm_id).is_err());
        assert_eq!(serde_json::to_value(&economy).unwrap(), before);
        let accepted =
            dispatch_recorded_firm_action_with_law(&mut economy, &policy, &firm_id, &binding)
                .unwrap();
        assert_eq!(
            accepted,
            RecordedFirmOutcome::DirectOwnerCashTransfer {
                actor_id: firm_id.clone(),
                recipient_owner_id: owner_id.clone(),
                amount: Money(100),
                law_authorization: Some(RecordedLawAuthorization {
                    binding_version: FIRM_ACTION_LAW_BINDING_VERSION.to_owned(),
                    governance_version: GOVERNANCE_VERSION.to_owned(),
                    domain: "corporate-distribution".to_owned(),
                    law_id: "owner-transfer-permission".to_owned(),
                    effective_month: 0,
                }),
            }
        );

        let branch = &scenario.policy_branches[0];
        economy.step_month(&scenario, branch).unwrap();
        assert_eq!(economy.month, 1);
        assert!(economy.governance.as_ref().unwrap().laws[0].enabled);
        // The amendment passed in month 0 is already effective at the month 1
        // boundary, even though `step_month(1)` has not yet mutated the state.
        assert!(
            !economy
                .governance
                .as_ref()
                .unwrap()
                .law_enabled_for_dispatch(&binding.law_id, &binding.domain, 1)
                .unwrap()
        );
        let policy = policy_for(&economy, &firm_id, action.clone());
        let before = serde_json::to_value(&economy).unwrap();
        assert!(
            dispatch_recorded_firm_action_with_law(&mut economy, &policy, &firm_id, &binding)
                .is_err()
        );
        assert_eq!(serde_json::to_value(&economy).unwrap(), before);
        let observations = ObservationBatch {
            observations: vec![observe_firm(&economy, &firm_id).unwrap()],
        };
        policy.apply_next(&observations, |_| Ok(())).unwrap();

        economy.step_month(&scenario, branch).unwrap();
        assert_eq!(economy.month, 2);
        assert!(!economy.governance.as_ref().unwrap().laws[0].enabled);
        assert!(
            economy
                .governance
                .as_ref()
                .unwrap()
                .law_enabled_for_dispatch(&binding.law_id, &binding.domain, 2)
                .unwrap()
        );
        let policy = policy_for(&economy, &firm_id, action);
        let accepted =
            dispatch_recorded_firm_action_with_law(&mut economy, &policy, &firm_id, &binding)
                .unwrap();
        assert!(matches!(
            accepted,
            RecordedFirmOutcome::DirectOwnerCashTransfer {
                law_authorization: Some(RecordedLawAuthorization {
                    effective_month: 2,
                    ..
                }),
                ..
            }
        ));
    }

    #[test]
    fn law_binding_rejects_wrong_domain_version_and_phase_without_consuming_tape() {
        let (_, mut economy, firm_id, owner_id, binding) = governed_fixture();
        let policy = policy_for(
            &economy,
            &firm_id,
            AgentAction::DirectOwnerCashTransfer {
                recipient_owner_id: owner_id,
                amount: Money(100),
            },
        );
        let before = serde_json::to_value(&economy).unwrap();
        let mut wrong = binding.clone();
        wrong.domain = "monetary".to_owned();
        assert!(
            dispatch_recorded_firm_action_with_law(&mut economy, &policy, &firm_id, &wrong)
                .is_err()
        );
        wrong = binding.clone();
        wrong.governance_schema_version = "owg.governance.v2".to_owned();
        assert!(
            dispatch_recorded_firm_action_with_law(&mut economy, &policy, &firm_id, &wrong)
                .is_err()
        );
        wrong = binding.clone();
        wrong.schema_version = "unsupported".to_owned();
        assert!(
            dispatch_recorded_firm_action_with_law(&mut economy, &policy, &firm_id, &wrong)
                .is_err()
        );
        economy.governance.as_mut().unwrap().next_month = 1;
        let wrong_phase = serde_json::to_value(&economy).unwrap();
        assert!(
            dispatch_recorded_firm_action_with_law(&mut economy, &policy, &firm_id, &binding)
                .is_err()
        );
        assert_eq!(serde_json::to_value(&economy).unwrap(), wrong_phase);
        economy.governance.as_mut().unwrap().next_month = 0;
        assert_eq!(serde_json::to_value(&economy).unwrap(), before);
        assert!(
            dispatch_recorded_firm_action_with_law(&mut economy, &policy, &firm_id, &binding)
                .is_ok()
        );
    }
}
