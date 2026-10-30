use crate::ENGINE_VERSION;
use crate::contracts::{ComputeProfile, ExecutionMode, ScenarioSpec};
use crate::error::OwgResult;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Development,
    SyntheticValidated,
    EmpiricallyCalibrated,
    ScenarioQualified,
    ForecastQualified,
    Invalid,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FidelityClass {
    Unvalidated,
    Qualitative,
    PolicyComparable,
    Reference,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct SeedStreamManifest {
    pub namespace: String,
    pub derivation: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct RunManifest {
    pub manifest_version: String,
    pub run_id: String,
    pub created_at: DateTime<Utc>,
    pub engine_version: String,
    pub engine_build_hash: String,
    pub scenario_id: String,
    pub scenario_hash: String,
    pub run_configuration_hash: String,
    pub model_pack_id: String,
    pub model_pack_version: String,
    pub model_pack_hash: String,
    pub data_bundle_id: String,
    pub data_bundle_version: String,
    pub data_bundle_hash: String,
    pub policy_branches: Vec<String>,
    pub policy_hashes: BTreeMap<String, String>,
    pub learned_weight_hashes: BTreeMap<String, String>,
    pub root_seed: u64,
    pub random_stream_version: String,
    pub seed_streams: Vec<SeedStreamManifest>,
    pub execution_mode: ExecutionMode,
    pub compute_profile: ComputeProfile,
    pub determinism_class: String,
    pub fidelity_class: FidelityClass,
    pub validation_status: ValidationStatus,
    pub platform: String,
    pub warnings: Vec<String>,
    pub invariants: BTreeMap<String, String>,
    pub output_hashes: BTreeMap<String, String>,
}

impl RunManifest {
    pub fn new(
        run_id: impl Into<String>,
        scenario: &ScenarioSpec,
        branches: Vec<String>,
        run_configuration_hash: String,
    ) -> OwgResult<Self> {
        let policy_hashes = scenario
            .policy_branches
            .iter()
            .filter(|branch| branches.contains(&branch.id))
            .map(|branch| {
                Ok((
                    branch.id.clone(),
                    hex::encode(Sha256::digest(serde_json::to_vec(branch)?)),
                ))
            })
            .collect::<OwgResult<BTreeMap<_, _>>>()?;
        Ok(Self {
            manifest_version: "owg.run-manifest.v1".to_owned(),
            run_id: run_id.into(),
            created_at: Utc::now(),
            engine_version: ENGINE_VERSION.to_owned(),
            engine_build_hash: option_env!("OWG_ENGINE_BUILD_HASH")
                .unwrap_or("development")
                .to_owned(),
            scenario_id: scenario.scenario_id.clone(),
            scenario_hash: scenario.hash()?,
            run_configuration_hash,
            model_pack_id: scenario.model_pack.id.clone(),
            model_pack_version: scenario.model_pack.version.clone(),
            model_pack_hash: scenario.model_pack.sha256.clone(),
            data_bundle_id: scenario.data_bundle.id.clone(),
            data_bundle_version: scenario.data_bundle.version.clone(),
            data_bundle_hash: scenario.data_bundle.sha256.clone(),
            policy_branches: branches,
            policy_hashes,
            learned_weight_hashes: BTreeMap::new(),
            root_seed: scenario.root_seed,
            random_stream_version: "openworldgen.seed-stream.v1".to_owned(),
            seed_streams: seed_stream_manifest(),
            execution_mode: scenario.execution_mode,
            compute_profile: scenario.compute_profile,
            determinism_class: match scenario.execution_mode {
                ExecutionMode::Reference => {
                    "exact-same-build-architecture; cross-platform-tolerance".to_owned()
                }
                ExecutionMode::Fast => "statistical-equivalence".to_owned(),
            },
            fidelity_class: FidelityClass::Unvalidated,
            validation_status: ValidationStatus::Development,
            platform: format!(
                "{}-{}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH,
                std::env::consts::FAMILY
            ),
            warnings: vec![
                "Development model: conditional scenarios only; no forecast qualification."
                    .to_owned(),
            ],
            invariants: BTreeMap::new(),
            output_hashes: BTreeMap::new(),
        })
    }

    pub fn validate(&self) -> OwgResult<()> {
        if self.manifest_version != "owg.run-manifest.v1"
            || self.run_id.trim().is_empty()
            || self.engine_version.trim().is_empty()
            || self.scenario_id.trim().is_empty()
            || self.model_pack_id.trim().is_empty()
            || self.model_pack_version.trim().is_empty()
            || self.data_bundle_id.trim().is_empty()
            || self.data_bundle_version.trim().is_empty()
        {
            return Err(crate::OwgError::InvalidState(
                "run manifest identity or version fields are incomplete".to_owned(),
            ));
        }
        let required_hashes = [
            self.scenario_hash.as_str(),
            self.run_configuration_hash.as_str(),
            self.model_pack_hash.as_str(),
            self.data_bundle_hash.as_str(),
        ];
        if required_hashes.into_iter().any(|hash| !valid_sha256(hash))
            || self.policy_hashes.values().any(|hash| !valid_sha256(hash))
            || self
                .learned_weight_hashes
                .values()
                .any(|hash| !valid_sha256(hash))
            || self.output_hashes.values().any(|hash| !valid_sha256(hash))
        {
            return Err(crate::OwgError::InvalidState(
                "run manifest contains an invalid SHA-256".to_owned(),
            ));
        }
        let branch_ids: BTreeSet<&str> = self.policy_branches.iter().map(String::as_str).collect();
        if branch_ids.len() != self.policy_branches.len()
            || branch_ids.iter().any(|branch| branch.trim().is_empty())
            || self.policy_hashes.len() != branch_ids.len()
            || self
                .policy_hashes
                .keys()
                .any(|id| !branch_ids.contains(id.as_str()))
        {
            return Err(crate::OwgError::InvalidState(
                "run manifest policy branch declarations are inconsistent".to_owned(),
            ));
        }
        if self.seed_streams.is_empty()
            || self.seed_streams.iter().any(|stream| {
                stream.namespace.trim().is_empty() || stream.derivation.trim().is_empty()
            })
        {
            return Err(crate::OwgError::InvalidState(
                "run manifest random-stream declarations are incomplete".to_owned(),
            ));
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn seed_stream_manifest() -> Vec<SeedStreamManifest> {
    [
        "bank",
        "firm",
        "household",
        "labor-market",
        "goods-market",
        "foreign-demand",
        "expectations",
        "firm-entry",
    ]
    .into_iter()
    .map(|namespace| SeedStreamManifest {
        namespace: namespace.to_owned(),
        derivation: "sha256(root_seed, namespace, actor_id, time_index, draw_index)".to_owned(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::RunManifest;
    use crate::ScenarioSpec;

    #[test]
    fn generated_manifest_validates_and_tampering_does_not() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let mut manifest = RunManifest::new(
            "test-run",
            &scenario,
            vec!["baseline".to_owned()],
            "a".repeat(64),
        )
        .unwrap();
        manifest.validate().unwrap();
        manifest.scenario_hash = "not-a-hash".to_owned();
        assert!(manifest.validate().is_err());
    }
}
