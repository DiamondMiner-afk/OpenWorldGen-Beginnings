//! Exact, ordered replay for strategic policy proposals.

use crate::error::{OwgError, OwgResult};
use crate::policy::{ActionBatch, AgentPolicy, ObservationBatch, PolicyContract, PolicyDescriptor};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Mutex;

pub const DECISION_TAPE_VERSION: &str = "owg.decision-tape.v1";

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct DecisionStep {
    pub observations: ObservationBatch,
    pub actions: ActionBatch,
    /// Model-visible rationale is evidence, not proof of a causal mechanism.
    pub rationales: Vec<String>,
    pub expected_effects: Vec<String>,
    pub confidences: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct DecisionTape {
    pub schema_version: String,
    pub id: String,
    pub descriptor: PolicyDescriptor,
    pub steps: Vec<DecisionStep>,
}

impl DecisionTape {
    pub fn validate(&self) -> OwgResult<()> {
        if self.schema_version != DECISION_TAPE_VERSION || self.id.trim().is_empty() {
            return Err(OwgError::InvalidState(
                "decision tape requires a supported version and id".to_owned(),
            ));
        }
        if self.steps.is_empty() {
            return Err(OwgError::InvalidState(
                "decision tape requires at least one step".to_owned(),
            ));
        }
        let mut previous_month = None;
        for (index, step) in self.steps.iter().enumerate() {
            PolicyContract {
                descriptor: self.descriptor.clone(),
                observations: step.observations.clone(),
                actions: step.actions.clone(),
                learning_update: None,
            }
            .validate()?;
            let count = step.observations.observations.len();
            if count == 0
                || step.rationales.len() != count
                || step.expected_effects.len() != count
                || step.confidences.len() != count
                || step
                    .confidences
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                || step.rationales.iter().any(|value| value.trim().is_empty())
                || step
                    .expected_effects
                    .iter()
                    .any(|value| value.trim().is_empty())
            {
                return Err(OwgError::InvalidState(format!(
                    "decision tape step {index} has missing or invalid evidence"
                )));
            }
            let month = step.observations.observations[0].month;
            let mut actors = BTreeSet::new();
            if previous_month.is_some_and(|previous| month < previous)
                || step
                    .observations
                    .observations
                    .iter()
                    .any(|item| item.month != month || !actors.insert(item.actor_id.as_str()))
            {
                return Err(OwgError::InvalidState(format!(
                    "decision tape step {index} has unordered months or duplicate actors"
                )));
            }
            previous_month = Some(month);
        }
        Ok(())
    }

    pub fn hash(&self) -> OwgResult<String> {
        self.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }
}

/// Replays a frozen tape only when observations match exactly, in order.
#[derive(Debug)]
pub struct RecordedPolicy {
    tape: DecisionTape,
    next_step: Mutex<usize>,
}

impl RecordedPolicy {
    pub fn new(tape: DecisionTape) -> OwgResult<Self> {
        tape.validate()?;
        Ok(Self {
            tape,
            next_step: Mutex::new(0),
        })
    }

    /// Apply the next frozen action while holding the replay cursor. A failed
    /// feasibility or settlement callback does not consume the tape step.
    /// The caller must make its own world-state mutation transactional.
    pub fn apply_next<T>(
        &self,
        observations: &ObservationBatch,
        apply: impl FnOnce(&ActionBatch) -> OwgResult<T>,
    ) -> OwgResult<T> {
        let mut index = self
            .next_step
            .lock()
            .map_err(|_| OwgError::InvalidState("decision tape replay lock failed".to_owned()))?;
        let step = self
            .tape
            .steps
            .get(*index)
            .ok_or_else(|| OwgError::InvalidState("decision tape is exhausted".to_owned()))?;
        if &step.observations != observations {
            return Err(OwgError::InvalidState(format!(
                "decision tape observation mismatch at step {}",
                *index
            )));
        }
        let outcome = apply(&step.actions)?;
        *index += 1;
        Ok(outcome)
    }
}

impl AgentPolicy for RecordedPolicy {
    fn descriptor(&self) -> &PolicyDescriptor {
        &self.tape.descriptor
    }

    fn act(&self, observations: &ObservationBatch) -> OwgResult<ActionBatch> {
        self.apply_next(observations, |actions| Ok(actions.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::{DECISION_TAPE_VERSION, DecisionStep, DecisionTape, RecordedPolicy};
    use crate::Money;
    use crate::policy::{
        ActionBatch, AgentAction, AgentPolicy, Observation, ObservationBatch, PolicyDescriptor,
        PolicyKind,
    };

    fn fixture() -> DecisionTape {
        DecisionTape {
            schema_version: DECISION_TAPE_VERSION.to_owned(),
            id: "strategic-example".to_owned(),
            descriptor: PolicyDescriptor {
                id: "local-model".to_owned(),
                version: "1".to_owned(),
                sha256: "a".repeat(64),
                kind: PolicyKind::LargeLanguageModel,
                deterministic: false,
                requires_network: false,
                maximum_latency_ms: 10_000,
            },
            steps: vec![DecisionStep {
                observations: ObservationBatch {
                    observations: vec![Observation {
                        actor_id: "firm-1".to_owned(),
                        month: 2,
                        inflation: 0.02,
                        unemployment: 0.05,
                        policy_rate: 0.04,
                        liquidity: Money(10_000),
                        debt: Money::ZERO,
                        inventory: 5.0,
                        expected_demand: 6.0,
                        sector: None,
                    }],
                },
                actions: ActionBatch {
                    actions: vec![AgentAction::NoOp],
                    warnings: vec![],
                    used_fallback: false,
                },
                rationales: vec!["hold capacity".to_owned()],
                expected_effects: vec!["no immediate change".to_owned()],
                confidences: vec![0.6],
            }],
        }
    }

    #[test]
    fn exact_replay_rejects_changed_observations_without_advancing() {
        let tape = fixture();
        let original = tape.steps[0].observations.clone();
        let policy = RecordedPolicy::new(tape).unwrap();
        let mut changed = original.clone();
        changed.observations[0].liquidity = Money(10_001);
        assert!(policy.act(&changed).is_err());
        assert_eq!(policy.act(&original).unwrap().actions.len(), 1);
        assert!(policy.act(&original).is_err());
    }

    #[test]
    fn rejects_duplicate_actors_and_invalid_confidence() {
        let mut tape = fixture();
        tape.steps[0].confidences[0] = 1.1;
        assert!(tape.validate().is_err());
        tape = fixture();
        let duplicate = tape.steps[0].observations.observations[0].clone();
        tape.steps[0].observations.observations.push(duplicate);
        tape.steps[0].actions.actions.push(AgentAction::NoOp);
        tape.steps[0].rationales.push("duplicate".to_owned());
        tape.steps[0].expected_effects.push("duplicate".to_owned());
        tape.steps[0].confidences.push(0.5);
        assert!(tape.validate().is_err());
    }
}
