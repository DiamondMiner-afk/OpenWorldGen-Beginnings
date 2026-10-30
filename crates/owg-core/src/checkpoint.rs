use crate::ENGINE_VERSION;
use crate::economy::Economy;
use crate::error::{OwgError, OwgResult};
use crate::metrics::MetricRow;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

pub const CHECKPOINT_VERSION: &str = "owg.checkpoint.v2";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Checkpoint {
    pub checkpoint_version: String,
    pub engine_version: String,
    pub created_at: DateTime<Utc>,
    pub scenario_hash: String,
    pub branch_id: String,
    pub replication: u32,
    pub next_month: u32,
    pub journal_hash: String,
    pub economy: Economy,
    pub metrics: Vec<MetricRow>,
}

impl Checkpoint {
    #[must_use]
    pub fn new(
        scenario_hash: String,
        branch_id: String,
        replication: u32,
        economy: Economy,
        metrics: Vec<MetricRow>,
    ) -> Self {
        Self {
            checkpoint_version: CHECKPOINT_VERSION.to_owned(),
            engine_version: ENGINE_VERSION.to_owned(),
            created_at: Utc::now(),
            scenario_hash,
            branch_id,
            replication,
            next_month: economy.month,
            journal_hash: economy.ledger.journal_hash(),
            economy,
            metrics,
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> OwgResult<()> {
        let writer = BufWriter::new(File::create(path)?);
        serde_json::to_writer(writer, self)?;
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>) -> OwgResult<Self> {
        let reader = BufReader::new(File::open(path)?);
        let checkpoint: Self = serde_json::from_reader(reader)?;
        checkpoint.validate_integrity()?;
        Ok(checkpoint)
    }

    /// Apply the same replay and accounting preflight to saved checkpoints and
    /// checkpoints passed directly to an in-memory resume call.
    pub fn validate_integrity(&self) -> OwgResult<()> {
        let checkpoint = self;
        if checkpoint.checkpoint_version != CHECKPOINT_VERSION {
            return Err(OwgError::IncompatibleCheckpoint(format!(
                "unsupported checkpoint version {}",
                checkpoint.checkpoint_version
            )));
        }
        if checkpoint.engine_version != ENGINE_VERSION {
            return Err(OwgError::IncompatibleCheckpoint(format!(
                "engine {} cannot resume checkpoint from {}",
                ENGINE_VERSION, checkpoint.engine_version
            )));
        }
        if !valid_sha256(&checkpoint.scenario_hash)
            || !valid_sha256(&checkpoint.journal_hash)
            || checkpoint.next_month != checkpoint.economy.month
            || checkpoint.replication != checkpoint.economy.replication
            || checkpoint.metrics.len() != checkpoint.next_month as usize
            || checkpoint.metrics.iter().any(|row| {
                row.branch_id != checkpoint.branch_id || row.replication != checkpoint.replication
            })
        {
            return Err(OwgError::IncompatibleCheckpoint(
                "checkpoint metadata and state are inconsistent".to_owned(),
            ));
        }
        if checkpoint.journal_hash != checkpoint.economy.ledger.journal_hash() {
            return Err(OwgError::IncompatibleCheckpoint(
                "journal hash mismatch".to_owned(),
            ));
        }
        checkpoint
            .economy
            .ledger
            .assert_balanced()
            .map_err(|error| {
                OwgError::IncompatibleCheckpoint(format!(
                    "global checkpoint ledger does not reconcile: {error}"
                ))
            })?;
        checkpoint
            .economy
            .assert_population_structure()
            .map_err(|error| {
                OwgError::IncompatibleCheckpoint(format!(
                    "malformed checkpoint population: {error}"
                ))
            })?;
        checkpoint
            .economy
            .assert_reserve_claims_balanced()
            .map_err(|error| {
                OwgError::IncompatibleCheckpoint(format!(
                    "reserve settlement accounts do not reconcile: {error}"
                ))
            })?;
        checkpoint
            .economy
            .assert_deposit_claims_balanced()
            .map_err(|error| {
                OwgError::IncompatibleCheckpoint(format!(
                    "bank deposit claims do not reconcile: {error}"
                ))
            })?;
        checkpoint
            .economy
            .assert_loan_claims_balanced()
            .map_err(|error| {
                OwgError::IncompatibleCheckpoint(format!(
                    "bank loan claims do not reconcile: {error}"
                ))
            })?;
        checkpoint
            .economy
            .institutions
            .validate_population(
                &checkpoint.economy.households.ids,
                &checkpoint.economy.firms.ids,
            )
            .map_err(|error| {
                OwgError::IncompatibleCheckpoint(format!(
                    "invalid or legacy institution graph: {error}"
                ))
            })?;
        if checkpoint
            .economy
            .governance
            .as_ref()
            .is_some_and(|state| state.next_month != checkpoint.next_month)
        {
            return Err(OwgError::IncompatibleCheckpoint(
                "governance calendar does not match checkpoint month".to_owned(),
            ));
        }
        if checkpoint
            .economy
            .contract_credit
            .as_ref()
            .is_some_and(|book| book.next_month != checkpoint.next_month || book.originations_done)
        {
            return Err(OwgError::IncompatibleCheckpoint(
                "credit calendar does not match checkpoint boundary".to_owned(),
            ));
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
