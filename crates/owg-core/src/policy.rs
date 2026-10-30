use crate::contracts::Sector;
use crate::error::{OwgError, OwgResult};
use crate::money::Money;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyKind {
    Rule,
    Utility,
    ReinforcementLearning,
    LargeLanguageModel,
    Human,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct PolicyDescriptor {
    pub id: String,
    pub version: String,
    pub sha256: String,
    pub kind: PolicyKind,
    pub deterministic: bool,
    pub requires_network: bool,
    pub maximum_latency_ms: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Observation {
    pub actor_id: String,
    pub month: u32,
    pub inflation: f64,
    pub unemployment: f64,
    pub policy_rate: f64,
    pub liquidity: Money,
    pub debt: Money,
    pub inventory: f64,
    pub expected_demand: f64,
    pub sector: Option<Sector>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    NoOp,
    HouseholdBudget {
        consumption_share: f64,
        reservation_wage: Money,
    },
    FirmPlan {
        price_change: f64,
        production_change: f64,
        vacancy_change: i32,
        investment: Money,
    },
    BankCreditRule {
        spread_basis_points: i32,
        approval_threshold: f64,
    },
    PolicyProposal {
        policy_rate_basis_points: Option<i32>,
        transfer: Option<Money>,
        public_spending: Option<Money>,
    },
    /// A firm proposes a cash transfer to a direct household owner. This is
    /// neither a dividend nor a tax determination; the dispatcher and economy
    /// must still enforce actor identity, ownership, and available funds.
    DirectOwnerCashTransfer {
        recipient_owner_id: String,
        amount: Money,
    },
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ObservationBatch {
    pub observations: Vec<Observation>,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ActionBatch {
    pub actions: Vec<AgentAction>,
    pub warnings: Vec<String>,
    pub used_fallback: bool,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct TransitionBatch {
    pub rewards: Vec<f64>,
}

/// Serializable boundary shared by rule, utility, RL, LLM, and human adapters.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct PolicyContract {
    pub descriptor: PolicyDescriptor,
    pub observations: ObservationBatch,
    pub actions: ActionBatch,
    pub learning_update: Option<TransitionBatch>,
}

impl PolicyContract {
    pub fn validate(&self) -> OwgResult<()> {
        if self.descriptor.id.trim().is_empty()
            || self.descriptor.version.trim().is_empty()
            || !valid_sha256(&self.descriptor.sha256)
        {
            return Err(OwgError::InvalidState(
                "policy descriptor requires an id, version, and SHA-256".to_owned(),
            ));
        }
        if self.actions.actions.len() != self.observations.observations.len() {
            return Err(OwgError::InvalidState(
                "policy action batch must match observation batch length".to_owned(),
            ));
        }
        for observation in &self.observations.observations {
            let values = [
                observation.inflation,
                observation.unemployment,
                observation.policy_rate,
                observation.inventory,
                observation.expected_demand,
            ];
            if observation.actor_id.trim().is_empty()
                || values.iter().any(|value| !value.is_finite())
                || !(0.0..=1.0).contains(&observation.unemployment)
                || observation.inventory < 0.0
                || observation.expected_demand < 0.0
            {
                return Err(OwgError::InvalidState(format!(
                    "invalid observation for actor {}",
                    observation.actor_id
                )));
            }
        }
        for action in &self.actions.actions {
            validate_action(action)?;
        }
        if let Some(update) = &self.learning_update
            && (update.rewards.len() != self.observations.observations.len()
                || update.rewards.iter().any(|reward| !reward.is_finite()))
        {
            return Err(OwgError::InvalidState(
                "learning rewards must be finite and match the observation batch".to_owned(),
            ));
        }
        Ok(())
    }
}

fn validate_action(action: &AgentAction) -> OwgResult<()> {
    let valid = match action {
        AgentAction::NoOp => true,
        AgentAction::HouseholdBudget {
            consumption_share,
            reservation_wage,
        } => {
            consumption_share.is_finite()
                && (0.0..=1.0).contains(consumption_share)
                && *reservation_wage >= Money::ZERO
        }
        AgentAction::FirmPlan {
            price_change,
            production_change,
            investment,
            ..
        } => {
            price_change.is_finite() && production_change.is_finite() && *investment >= Money::ZERO
        }
        AgentAction::BankCreditRule {
            approval_threshold, ..
        } => approval_threshold.is_finite() && (0.0..=1.0).contains(approval_threshold),
        AgentAction::PolicyProposal {
            transfer,
            public_spending,
            ..
        } => {
            transfer.is_none_or(|amount| amount >= Money::ZERO)
                && public_spending.is_none_or(|amount| amount >= Money::ZERO)
        }
        AgentAction::DirectOwnerCashTransfer {
            recipient_owner_id,
            amount,
        } => !recipient_owner_id.trim().is_empty() && *amount > Money::ZERO,
    };
    if valid {
        Ok(())
    } else {
        Err(OwgError::InvalidState(
            "policy emitted an invalid typed action".to_owned(),
        ))
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Policies can propose actions but receive no mutable simulation state.
pub trait AgentPolicy: Send + Sync + std::fmt::Debug {
    fn descriptor(&self) -> &PolicyDescriptor;
    fn act(&self, observations: &ObservationBatch) -> OwgResult<ActionBatch>;

    fn learn(&mut self, _transitions: &TransitionBatch) -> OwgResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct NoOpPolicy {
    descriptor: PolicyDescriptor,
}

impl NoOpPolicy {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            descriptor: PolicyDescriptor {
                id: id.into(),
                version: "1".to_owned(),
                sha256: hex::encode(Sha256::digest(b"openworldgen.no-op-policy.v1")),
                kind: PolicyKind::Rule,
                deterministic: true,
                requires_network: false,
                maximum_latency_ms: 0,
            },
        }
    }
}

impl AgentPolicy for NoOpPolicy {
    fn descriptor(&self) -> &PolicyDescriptor {
        &self.descriptor
    }

    fn act(&self, observations: &ObservationBatch) -> OwgResult<ActionBatch> {
        Ok(ActionBatch {
            actions: vec![AgentAction::NoOp; observations.observations.len()],
            warnings: Vec::new(),
            used_fallback: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionBatch, AgentAction, AgentPolicy, NoOpPolicy, Observation, ObservationBatch,
        PolicyContract,
    };
    use crate::Money;

    #[test]
    fn no_op_policy_preserves_batch_shape() {
        let policy = NoOpPolicy::new("fallback");
        let result = policy.act(&ObservationBatch::default()).unwrap();
        assert!(result.actions.is_empty());
        assert!(!result.used_fallback);
    }

    #[test]
    fn policy_contract_rejects_mismatched_batches() {
        let policy = NoOpPolicy::new("fallback");
        let contract = PolicyContract {
            descriptor: policy.descriptor().clone(),
            observations: ObservationBatch {
                observations: vec![Observation {
                    actor_id: "firm-1".to_owned(),
                    month: 0,
                    inflation: 0.02,
                    unemployment: 0.05,
                    policy_rate: 0.04,
                    liquidity: Money::ZERO,
                    debt: Money::ZERO,
                    inventory: 1.0,
                    expected_demand: 1.0,
                    sector: None,
                }],
            },
            actions: ActionBatch {
                actions: Vec::new(),
                warnings: Vec::new(),
                used_fallback: false,
            },
            learning_update: None,
        };
        assert!(contract.validate().is_err());
    }

    #[test]
    fn policy_contract_accepts_bounded_typed_action() {
        let policy = NoOpPolicy::new("fallback");
        let contract = PolicyContract {
            descriptor: policy.descriptor().clone(),
            observations: ObservationBatch {
                observations: vec![Observation {
                    actor_id: "household-1".to_owned(),
                    month: 0,
                    inflation: 0.02,
                    unemployment: 0.05,
                    policy_rate: 0.04,
                    liquidity: Money(10_000),
                    debt: Money::ZERO,
                    inventory: 0.0,
                    expected_demand: 0.0,
                    sector: None,
                }],
            },
            actions: ActionBatch {
                actions: vec![AgentAction::HouseholdBudget {
                    consumption_share: 0.8,
                    reservation_wage: Money(1_000),
                }],
                warnings: Vec::new(),
                used_fallback: false,
            },
            learning_update: None,
        };
        contract.validate().unwrap();
    }
}
