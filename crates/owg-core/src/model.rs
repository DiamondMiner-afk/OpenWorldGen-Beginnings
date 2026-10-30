use crate::error::{OwgError, OwgResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

pub const MODEL_PACK_VERSION: &str = "owg.model-pack.v1";

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct AgentArchetype {
    pub id: String,
    pub policy_id: String,
    pub state_schema: String,
    pub observation_schema: String,
    pub action_schema: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct MarketDefinition {
    pub id: String,
    pub commodity: String,
    pub clearing_rule: String,
    pub settlement_schedule: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct ScheduleDefinition {
    pub id: String,
    pub cadence: String,
    pub phase_order: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct PosteriorReference {
    pub artifact_path: String,
    pub sha256: String,
    pub draws: u64,
    pub method: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ModelPack {
    pub schema_version: String,
    pub id: String,
    pub version: String,
    pub agent_archetypes: Vec<AgentArchetype>,
    pub markets: Vec<MarketDefinition>,
    pub accounting_rules: Vec<String>,
    pub schedules: Vec<ScheduleDefinition>,
    pub parameters: BTreeMap<String, f64>,
    pub calibrated_posterior: Option<PosteriorReference>,
    pub validation_status: String,
}

impl ModelPack {
    pub fn validate(&self) -> OwgResult<()> {
        if self.schema_version != MODEL_PACK_VERSION {
            return Err(OwgError::InvalidState(format!(
                "unsupported model pack version {}",
                self.schema_version
            )));
        }
        if self.id.trim().is_empty()
            || self.agent_archetypes.is_empty()
            || self.markets.is_empty()
            || self.accounting_rules.is_empty()
            || self.schedules.is_empty()
        {
            return Err(OwgError::InvalidState(
                "model pack is missing required structural definitions".to_owned(),
            ));
        }
        unique_ids(
            "agent archetype",
            self.agent_archetypes.iter().map(|value| value.id.as_str()),
        )?;
        unique_ids("market", self.markets.iter().map(|value| value.id.as_str()))?;
        unique_ids(
            "schedule",
            self.schedules.iter().map(|value| value.id.as_str()),
        )?;
        if self.agent_archetypes.iter().any(|value| {
            value.policy_id.trim().is_empty()
                || value.state_schema.trim().is_empty()
                || value.observation_schema.trim().is_empty()
                || value.action_schema.trim().is_empty()
        }) || self.markets.iter().any(|value| {
            value.commodity.trim().is_empty()
                || value.clearing_rule.trim().is_empty()
                || value.settlement_schedule.trim().is_empty()
        }) || self
            .schedules
            .iter()
            .any(|value| value.cadence.trim().is_empty())
            || self
                .accounting_rules
                .iter()
                .any(|rule| rule.trim().is_empty())
            || self.validation_status.trim().is_empty()
        {
            return Err(OwgError::InvalidState(
                "model pack contains incomplete structural definitions".to_owned(),
            ));
        }
        if self
            .parameters
            .iter()
            .any(|(name, value)| name.trim().is_empty() || !value.is_finite())
        {
            return Err(OwgError::InvalidState(
                "model parameters must be finite".to_owned(),
            ));
        }
        if let Some(posterior) = &self.calibrated_posterior {
            let path = Path::new(&posterior.artifact_path);
            if posterior.artifact_path.trim().is_empty()
                || path.is_absolute()
                || path
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
                || !valid_sha256(&posterior.sha256)
                || posterior.draws == 0
                || posterior.method.trim().is_empty()
            {
                return Err(OwgError::InvalidState(
                    "calibrated posterior reference is invalid".to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub fn hash(&self) -> OwgResult<String> {
        self.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn validate_posterior_content(&self, bytes: &[u8]) -> OwgResult<()> {
        let posterior = self.calibrated_posterior.as_ref().ok_or_else(|| {
            OwgError::InvalidState("model pack does not declare a calibrated posterior".to_owned())
        })?;
        let actual_hash = hex::encode(Sha256::digest(bytes));
        if actual_hash != posterior.sha256 {
            return Err(OwgError::InvalidState(format!(
                "posterior checksum mismatch: expected {}, found {actual_hash}",
                posterior.sha256
            )));
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unique_ids<'a>(kind: &str, values: impl Iterator<Item = &'a str>) -> OwgResult<()> {
    let mut ids = BTreeSet::new();
    for id in values {
        if id.trim().is_empty() || !ids.insert(id) {
            return Err(OwgError::InvalidState(format!(
                "invalid or duplicate {kind} id {id}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ModelPack;

    #[test]
    fn repository_model_pack_matches_contract() {
        let model: ModelPack = serde_json::from_str(include_str!(
            "../../../model-packs/small-open-economy-v0.1/model-pack.json"
        ))
        .unwrap();
        model.validate().unwrap();
        assert_eq!(model.hash().unwrap().len(), 64);
    }
}
