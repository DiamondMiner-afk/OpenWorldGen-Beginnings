use crate::checkpoint::Checkpoint;
use crate::comparison::ComparisonReport;
use crate::contracts::{ComputeProfile, ExecutionMode, PolicyBranch, ScenarioSpec};
use crate::data::DataBundle;
use crate::economy::{EconomicParameters, Economy};
use crate::error::{OwgError, OwgResult};
use crate::governance::{GovernanceEvent, GovernanceState};
use crate::historical_finance::FinanceOpeningSpec;
use crate::historical_labor::HistoricalLaborSpec;
use crate::historical_production::ProductionOpeningSpec;
use crate::institutions::InstitutionGraph;
use crate::live::LiveRun;
use crate::manifest::{FidelityClass, RunManifest, ValidationStatus};
use crate::metrics::MetricRow;
use crate::model::ModelPack;
use crate::replay::{DECISION_TAPE_VERSION, DecisionStep, DecisionTape, RecordedPolicy};
use crate::scheduler::SCHEDULER_VERSION;
use crate::strategic_dispatch::{
    FirmActionLawBinding, RecordedFirmOutcome, dispatch_recorded_firm_action,
    dispatch_recorded_firm_action_with_law,
};
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct RunOptions {
    pub branch_ids: Option<Vec<String>>,
    pub replications: u32,
    pub end_month: Option<u32>,
}

impl RunOptions {
    #[must_use]
    pub fn for_scenario(scenario: &ScenarioSpec) -> Self {
        Self {
            branch_ids: None,
            replications: scenario.replications.minimum,
            end_month: None,
        }
    }
}

pub const RECORDED_FIRM_RUN_VERSION: &str = "owg.recorded-firm-run.v1";

/// An opt-in, single-firm experiment bound to one frozen decision tape.
/// The law binding is a scenario rule, not a real-world legal conclusion.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct RecordedFirmRunBinding {
    pub schema_version: String,
    pub scenario_hash: String,
    pub branch_id: String,
    pub replication: u32,
    pub firm_id: String,
    pub end_month: u32,
    pub decision_tape_hash: String,
    pub law_binding: Option<FirmActionLawBinding>,
    pub law_binding_hash: Option<String>,
}

impl RecordedFirmRunBinding {
    pub fn new(
        scenario: &ScenarioSpec,
        branch_id: impl Into<String>,
        replication: u32,
        firm_id: impl Into<String>,
        end_month: u32,
        tape: &DecisionTape,
        law_binding: Option<FirmActionLawBinding>,
    ) -> OwgResult<Self> {
        let law_binding_hash = law_binding.as_ref().map(hash_json).transpose()?;
        Ok(Self {
            schema_version: RECORDED_FIRM_RUN_VERSION.to_owned(),
            scenario_hash: scenario.hash()?,
            branch_id: branch_id.into(),
            replication,
            firm_id: firm_id.into(),
            end_month,
            decision_tape_hash: tape.hash()?,
            law_binding,
            law_binding_hash,
        })
    }
}

/// The action was accepted by this bounded dispatcher and accounting kernel.
/// It is not evidence that the model's stated rationale caused the outcome.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct RecordedFirmDecisionAudit {
    pub month: u32,
    pub actor_id: String,
    pub observation_hash: String,
    pub proposal_hash: String,
    pub accepted_outcome: RecordedFirmOutcome,
    pub journal_hash_after_decision: String,
    pub journal_hash_after_month: String,
    pub schedule_hash_after_month: String,
}

/// Experimental output, deliberately separate from the qualified result-bundle
/// and checkpoint contracts. The tape is included so inputs and action traces
/// can be inspected without relying on an unrecorded policy service.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct RecordedFirmRunResult {
    pub schema_version: String,
    pub binding: RecordedFirmRunBinding,
    pub binding_hash: String,
    pub decision_tape: DecisionTape,
    pub metrics: Vec<MetricRow>,
    pub metrics_hash: String,
    pub decision_audit: Vec<RecordedFirmDecisionAudit>,
    pub decision_audit_hash: String,
    pub governance_events: Vec<GovernanceEvent>,
    pub governance_events_hash: String,
    pub final_journal_hash: String,
    pub final_schedule_hash: String,
    pub final_economy_hash: String,
    pub next_firm_observation: crate::policy::Observation,
    pub next_firm_observation_hash: String,
}

impl RecordedFirmRunResult {
    /// Check the fields carried by this experimental result against their
    /// recorded hashes. This is not independent authentication of the run.
    pub fn validate_integrity(&self) -> OwgResult<()> {
        let binding = &self.binding;
        if self.schema_version != RECORDED_FIRM_RUN_VERSION
            || binding.schema_version != RECORDED_FIRM_RUN_VERSION
            || self.binding_hash != hash_json(binding)?
            || binding.decision_tape_hash != self.decision_tape.hash()?
            || binding.law_binding_hash
                != binding.law_binding.as_ref().map(hash_json).transpose()?
            || self.metrics.len() != binding.end_month as usize
            || self.decision_audit.len() != binding.end_month as usize
            || self.metrics_hash != hash_json(&self.metrics)?
            || self.decision_audit_hash != hash_json(&self.decision_audit)?
            || self.governance_events_hash != hash_json(&self.governance_events)?
            || self.next_firm_observation_hash != hash_json(&self.next_firm_observation)?
            || self.next_firm_observation.actor_id != binding.firm_id
            || self.next_firm_observation.month != binding.end_month
            || !valid_hash(&self.final_economy_hash)
            || !valid_hash(&self.final_journal_hash)
            || !valid_hash(&self.final_schedule_hash)
        {
            return Err(OwgError::InvalidState(
                "recorded firm result hash or cardinality mismatch".to_owned(),
            ));
        }
        for (month, (row, audit)) in self.metrics.iter().zip(&self.decision_audit).enumerate() {
            if row.month != month as u32
                || row.branch_id != binding.branch_id
                || row.replication != binding.replication
                || audit.month != month as u32
                || audit.actor_id != binding.firm_id
                || !valid_hash(&audit.observation_hash)
                || !valid_hash(&audit.proposal_hash)
                || !valid_hash(&audit.journal_hash_after_decision)
                || !valid_hash(&audit.journal_hash_after_month)
                || !valid_hash(&audit.schedule_hash_after_month)
            {
                return Err(OwgError::InvalidState(
                    "recorded firm result has a mismatched month, actor, or audit hash".to_owned(),
                ));
            }
        }
        if self.decision_audit.last().is_some_and(|last| {
            last.journal_hash_after_month != self.final_journal_hash
                || last.schedule_hash_after_month != self.final_schedule_hash
        }) {
            return Err(OwgError::InvalidState(
                "recorded firm final hashes do not match last monthly audit".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SimulationResult {
    pub manifest: RunManifest,
    pub metrics: Vec<MetricRow>,
    pub comparison: ComparisonReport,
    pub mechanisms: Vec<MechanismDecomposition>,
    pub accounting: Vec<AccountingDiagnostic>,
    pub journal_hashes: BTreeMap<String, String>,
    pub event_audit: BTreeMap<String, EventAudit>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct MechanismDecomposition {
    pub branch_id: String,
    pub peak_inflation_month: u32,
    pub peak_inflation_mean: f64,
    pub cpi_sector_change_at_peak: [f64; 4],
    pub mean_output_gap: f64,
    pub mean_unemployment: f64,
    pub cumulative_firm_defaults_per_replication: f64,
    pub cumulative_household_defaults_per_replication: f64,
    pub cumulative_bank_resolutions_per_replication: f64,
    pub peak_credit_mean: f64,
    pub cumulative_fiscal_deficit_per_replication: f64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct AccountingDiagnostic {
    pub branch_id: String,
    pub observation_count: u64,
    pub max_national_accounts_relative_error: f64,
    pub max_income_identity_relative_error: f64,
    pub max_institutional_balance_residual_minor_units: u64,
    pub max_unclassified_balance_minor_units: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EventAudit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_credit_audit: Option<crate::credit::CreditAudit>,
    pub journal_hash: String,
    pub entry_count: u64,
    pub posting_count: u64,
    pub entries_by_phase: BTreeMap<String, u64>,
    pub entries_by_month: BTreeMap<u32, u64>,
    pub first_entry_id: Option<String>,
    pub last_entry_id: Option<String>,
    #[serde(default)]
    pub scheduler_version: String,
    #[serde(default)]
    pub schedule_hash: String,
    #[serde(default)]
    pub scheduled_event_count: u64,
    #[serde(default)]
    pub scheduled_events_by_kind: BTreeMap<String, u64>,
    #[serde(default)]
    pub scheduled_events_by_month: BTreeMap<u32, u64>,
    #[serde(default)]
    pub first_scheduled_event_id: Option<String>,
    #[serde(default)]
    pub last_scheduled_event_id: Option<String>,
    #[serde(default)]
    pub governance_events: Vec<GovernanceEvent>,
}

#[derive(Clone, Debug)]
struct BranchRun {
    branch_id: String,
    replication: u32,
    metrics: Vec<MetricRow>,
    economy: Economy,
}

#[derive(Clone, Debug)]
pub struct SimulationEngine {
    scenario: ScenarioSpec,
    model_pack: ModelPack,
    data_bundle: DataBundle,
    parameters: EconomicParameters,
}

impl SimulationEngine {
    pub fn new(scenario: ScenarioSpec) -> OwgResult<Self> {
        let model_pack: ModelPack = serde_json::from_str(include_str!(
            "../../../model-packs/small-open-economy-v0.1/model-pack.json"
        ))?;
        let data_bundle: DataBundle = serde_json::from_str(include_str!(
            "../../../data/synthetic-balanced-v0.1/data-bundle.json"
        ))?;
        Self::with_inputs(
            scenario,
            model_pack,
            data_bundle,
            include_bytes!("../../../data/synthetic-balanced-v0.1/sector_parameters.csv"),
        )
    }

    pub fn with_inputs(
        scenario: ScenarioSpec,
        model_pack: ModelPack,
        data_bundle: DataBundle,
        sector_parameter_csv: &[u8],
    ) -> OwgResult<Self> {
        scenario.validated()?;
        model_pack.validate()?;
        data_bundle.validate()?;
        let model_hash = model_pack.hash()?;
        let data_hash = data_bundle.hash()?;
        if scenario.model_pack.id != model_pack.id
            || scenario.model_pack.version != model_pack.version
            || scenario.model_pack.sha256 != model_hash
            || scenario.model_pack.validation_status != model_pack.validation_status
        {
            return Err(OwgError::InvalidState(format!(
                "model pack does not match scenario reference; resolved {}@{} ({model_hash})",
                model_pack.id, model_pack.version
            )));
        }
        if scenario.data_bundle.id != data_bundle.id
            || scenario.data_bundle.version != data_bundle.version
            || scenario.data_bundle.sha256 != data_hash
            || scenario.data_bundle.source != data_bundle.provenance.sources.join("; ")
            || scenario.data_bundle.licence != data_bundle.provenance.licence
            || scenario.data_bundle.retrieved_at != data_bundle.provenance.retrieval_date
            || scenario.data_bundle.confidentiality != data_bundle.provenance.confidentiality
        {
            return Err(OwgError::InvalidState(format!(
                "data bundle does not match scenario reference; resolved {}@{} ({data_hash})",
                data_bundle.id, data_bundle.version
            )));
        }
        if scenario.geography.currency != data_bundle.currency
            || scenario.geography.price_base_year != data_bundle.base_year
        {
            return Err(OwgError::InvalidState(
                "scenario currency or base year does not match data bundle".to_owned(),
            ));
        }
        let sector = data_bundle.sector_parameters_from_csv(sector_parameter_csv)?;
        let parameters = EconomicParameters::from_inputs(&model_pack, sector)?;
        Ok(Self {
            scenario,
            model_pack,
            data_bundle,
            parameters,
        })
    }

    #[must_use]
    pub fn scenario(&self) -> &ScenarioSpec {
        &self.scenario
    }

    #[must_use]
    pub fn model_pack(&self) -> &ModelPack {
        &self.model_pack
    }

    #[must_use]
    pub fn data_bundle(&self) -> &DataBundle {
        &self.data_bundle
    }

    pub fn live_run(
        &self,
        branch_id: &str,
        replication: u32,
        labor: Option<HistoricalLaborSpec>,
        finance: Option<FinanceOpeningSpec>,
        production: Option<ProductionOpeningSpec>,
    ) -> OwgResult<LiveRun<'_>> {
        let branch = self
            .scenario
            .policy_branches
            .iter()
            .find(|branch| branch.id == branch_id)
            .ok_or_else(|| OwgError::InvalidState(format!("unknown policy branch: {branch_id}")))?;
        let economy = Economy::new_with_parameters(&self.scenario, replication, &self.parameters)?;
        LiveRun::new(&self.scenario, branch, economy, labor, finance, production)
    }

    /// Replay exactly one frozen firm decision at each pre-month boundary.
    /// This deliberately does not use or alter ordinary `run`, checkpoint,
    /// resume, or result-bundle semantics. No live model is called here.
    pub fn run_recorded_firm(
        &self,
        binding: &RecordedFirmRunBinding,
        tape: &DecisionTape,
    ) -> OwgResult<RecordedFirmRunResult> {
        let scenario_hash = self.scenario.hash()?;
        let tape_hash = tape.hash()?;
        let expected_law_hash = binding.law_binding.as_ref().map(hash_json).transpose()?;
        if binding.schema_version != RECORDED_FIRM_RUN_VERSION
            || binding.scenario_hash != scenario_hash
            || binding.decision_tape_hash != tape_hash
            || binding.law_binding_hash != expected_law_hash
            || binding.branch_id.trim().is_empty()
            || binding.firm_id.trim().is_empty()
            || binding.end_month == 0
            || binding.end_month > self.scenario.calendar.months
            || tape.steps.len() != binding.end_month as usize
            || binding.law_binding.is_some() != self.scenario.governance.is_some()
        {
            return Err(OwgError::InvalidState(
                "recorded firm run binding, tape, law, or horizon does not match the scenario"
                    .to_owned(),
            ));
        }
        let branch = self
            .scenario
            .policy_branches
            .iter()
            .find(|candidate| candidate.id == binding.branch_id)
            .ok_or_else(|| OwgError::InvalidState("unknown recorded-run branch".to_owned()))?;
        for (month, step) in tape.steps.iter().enumerate() {
            if step.observations.observations.len() != 1
                || step.actions.actions.len() != 1
                || step.observations.observations[0].actor_id != binding.firm_id
                || step.observations.observations[0].month != month as u32
            {
                return Err(OwgError::InvalidState(format!(
                    "recorded firm tape step {month} is not the one bound actor at its consecutive month"
                )));
            }
        }
        let mut economy =
            Economy::new_with_parameters(&self.scenario, binding.replication, &self.parameters)?;
        if !economy.firms.ids.contains(&binding.firm_id) {
            return Err(OwgError::InvalidState(
                "recorded-run actor is not a firm in the opening economy".to_owned(),
            ));
        }
        let mut metrics = Vec::with_capacity(binding.end_month as usize);
        let mut decision_audit = Vec::with_capacity(binding.end_month as usize);
        while economy.month < binding.end_month {
            let step = &tape.steps[economy.month as usize];
            let (row, audit) = execute_recorded_firm_month(
                &mut economy,
                &self.scenario,
                branch,
                tape,
                step,
                &binding.firm_id,
                binding.law_binding.as_ref(),
            )?;
            metrics.push(row);
            decision_audit.push(audit);
        }
        let governance_events = economy
            .governance
            .as_ref()
            .map_or_else(Vec::new, |state| state.events.clone());
        let next_firm_observation =
            crate::strategic_dispatch::observe_firm(&economy, &binding.firm_id)?;
        let result = RecordedFirmRunResult {
            next_firm_observation_hash: hash_json(&next_firm_observation)?,
            next_firm_observation,
            schema_version: RECORDED_FIRM_RUN_VERSION.to_owned(),
            binding: binding.clone(),
            binding_hash: hash_json(binding)?,
            decision_tape: tape.clone(),
            metrics_hash: hash_json(&metrics)?,
            metrics,
            decision_audit_hash: hash_json(&decision_audit)?,
            decision_audit,
            governance_events_hash: hash_json(&governance_events)?,
            governance_events,
            final_journal_hash: economy.ledger.journal_hash(),
            final_schedule_hash: economy.schedule_audit.schedule_hash(),
            final_economy_hash: hash_json(&economy)?,
        };
        result.validate_integrity()?;
        Ok(result)
    }

    pub fn run(&self, options: &RunOptions) -> OwgResult<SimulationResult> {
        if options.replications == 0 {
            return Err(OwgError::InvalidState(
                "replications must be positive".to_owned(),
            ));
        }
        let branches = self.selected_branches(options)?;
        let end_month = options
            .end_month
            .unwrap_or(self.scenario.calendar.months)
            .min(self.scenario.calendar.months);
        if end_month == 0 {
            return Err(OwgError::InvalidState(
                "run horizon must contain at least one month".to_owned(),
            ));
        }
        let jobs: Vec<(&PolicyBranch, u32)> = branches
            .iter()
            .flat_map(|branch| {
                (0..options.replications).map(move |replication| (*branch, replication))
            })
            .collect();
        let branch_runs = self.execute_jobs(&jobs, end_month)?;
        self.assemble_result(&branches, &branch_runs, options, end_month)
    }

    /// Run all branches in batches until the declared Monte Carlo precision
    /// target is met or the scenario's maximum replication count is reached.
    pub fn run_to_precision(&self) -> OwgResult<SimulationResult> {
        let minimum = self.scenario.replications.minimum;
        let maximum = self.scenario.replications.maximum;
        let target = self.scenario.replications.relative_half_width_target;
        let options = RunOptions::for_scenario(&self.scenario);
        let branches = self.selected_branches(&options)?;
        let end_month = self.scenario.calendar.months;
        let mut branch_runs = Vec::new();
        let mut completed = 0_u32;
        let mut target_met = false;
        while completed < maximum {
            let next = completed.saturating_add(minimum).min(maximum);
            let jobs: Vec<(&PolicyBranch, u32)> = branches
                .iter()
                .flat_map(|branch| (completed..next).map(move |replication| (*branch, replication)))
                .collect();
            branch_runs.extend(self.execute_jobs(&jobs, end_month)?);
            completed = next;
            let metrics: Vec<MetricRow> = branch_runs
                .iter()
                .flat_map(|run| run.metrics.iter().cloned())
                .collect();
            let comparison = self.comparison(&metrics, end_month);
            target_met = precision_target_met(&comparison, target);
            if target_met {
                break;
            }
        }
        let final_options = RunOptions {
            branch_ids: None,
            replications: completed,
            end_month: None,
        };
        let mut result =
            self.assemble_result(&branches, &branch_runs, &final_options, end_month)?;
        result
            .manifest
            .invariants
            .insert("monte_carlo_replications".to_owned(), completed.to_string());
        result.manifest.invariants.insert(
            "monte_carlo_precision_target_met".to_owned(),
            target_met.to_string(),
        );
        if !target_met {
            result.manifest.warnings.push(format!(
                "Monte Carlo relative half-width target {target:.4} was not met at {completed} replications."
            ));
        }
        Ok(result)
    }

    fn execute_jobs(
        &self,
        jobs: &[(&PolicyBranch, u32)],
        end_month: u32,
    ) -> OwgResult<Vec<BranchRun>> {
        // Branches and replications have isolated state and named random streams.
        // They can run concurrently in reference mode without changing any
        // within-run reduction order or the final sorted output.
        let concurrency = self.execution_concurrency(jobs.len());
        if concurrency > 1 {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(concurrency)
                .build()
                .map_err(|error| {
                    OwgError::InvalidState(format!("failed to create worker pool: {error}"))
                })?;
            pool.install(|| {
                jobs.par_iter()
                    .map(|(branch, replication)| self.run_branch(branch, *replication, end_month))
                    .collect()
            })
        } else {
            jobs.iter()
                .map(|(branch, replication)| self.run_branch(branch, *replication, end_month))
                .collect()
        }
    }

    fn execution_concurrency(&self, job_count: usize) -> usize {
        let available = std::thread::available_parallelism().map_or(1, usize::from);
        let requested = match (self.scenario.compute_profile, self.scenario.execution_mode) {
            (ComputeProfile::Minimal, ExecutionMode::Reference) => 1,
            (ComputeProfile::Minimal, ExecutionMode::Fast) => 2,
            (ComputeProfile::Standard | ComputeProfile::Custom, ExecutionMode::Reference) => {
                (available / 2).max(1)
            }
            (ComputeProfile::Standard | ComputeProfile::Custom, ExecutionMode::Fast) => available,
        };
        requested.min(available).min(job_count.max(1))
    }

    fn assemble_result(
        &self,
        branches: &[&PolicyBranch],
        branch_runs: &[BranchRun],
        options: &RunOptions,
        end_month: u32,
    ) -> OwgResult<SimulationResult> {
        let mut metrics: Vec<MetricRow> = branch_runs
            .iter()
            .flat_map(|run| run.metrics.iter().cloned())
            .collect();
        metrics.sort_by(|left, right| {
            left.branch_id
                .cmp(&right.branch_id)
                .then(left.replication.cmp(&right.replication))
                .then(left.month.cmp(&right.month))
        });
        let journal_hashes = branch_runs
            .iter()
            .map(|run| {
                (
                    format!("{}:{}", run.branch_id, run.replication),
                    run.economy.ledger.journal_hash(),
                )
            })
            .collect();
        let event_audit: BTreeMap<String, EventAudit> = branch_runs
            .iter()
            .map(|run| {
                let key = format!("{}:{}", run.branch_id, run.replication);
                let audit = run.economy.ledger.audit();
                (
                    key,
                    EventAudit {
                        contract_credit_audit: run
                            .economy
                            .contract_credit
                            .as_ref()
                            .map(|book| book.audit.clone()),
                        journal_hash: run.economy.ledger.journal_hash(),
                        entry_count: audit.entry_count,
                        posting_count: audit.posting_count,
                        entries_by_phase: audit.entries_by_phase.clone(),
                        entries_by_month: audit.entries_by_month.clone(),
                        first_entry_id: audit.first_entry_id.clone(),
                        last_entry_id: audit.last_entry_id.clone(),
                        scheduler_version: self.scheduler_version().to_owned(),
                        schedule_hash: run.economy.schedule_audit.schedule_hash(),
                        scheduled_event_count: run.economy.schedule_audit.event_count,
                        scheduled_events_by_kind: run.economy.schedule_audit.events_by_kind.clone(),
                        scheduled_events_by_month: run
                            .economy
                            .schedule_audit
                            .events_by_month
                            .clone(),
                        first_scheduled_event_id: run.economy.schedule_audit.first_event_id.clone(),
                        last_scheduled_event_id: run.economy.schedule_audit.last_event_id.clone(),
                        governance_events: run
                            .economy
                            .governance
                            .as_ref()
                            .map_or_else(Vec::new, |state| state.events.clone()),
                    },
                )
            })
            .collect();

        let comparison = self.comparison(&metrics, end_month);
        let mechanisms = mechanism_decompositions(
            &metrics,
            comparison.evaluation_start_month,
            comparison.evaluation_end_month,
        );
        let accounting = accounting_diagnostics(&metrics);

        let scenario_hash = self.scenario.hash()?;
        let run_configuration_hash = hex::encode(Sha256::digest(serde_json::to_vec(options)?));
        let run_id = format!(
            "{}-{}-{}",
            self.scenario.scenario_id,
            &scenario_hash[..12],
            &run_configuration_hash[..12],
        );
        let mut manifest = RunManifest::new(
            run_id,
            &self.scenario,
            branches.iter().map(|branch| branch.id.clone()).collect(),
            run_configuration_hash,
        )?;
        manifest.output_hashes.insert(
            "metrics".to_owned(),
            hex::encode(Sha256::digest(serde_json::to_vec(&metrics)?)),
        );
        manifest.output_hashes.insert(
            "comparison".to_owned(),
            hex::encode(Sha256::digest(serde_json::to_vec(&comparison)?)),
        );
        manifest.output_hashes.insert(
            "accounting".to_owned(),
            hex::encode(Sha256::digest(serde_json::to_vec(&accounting)?)),
        );
        manifest.output_hashes.insert(
            "event_audit".to_owned(),
            hex::encode(Sha256::digest(serde_json::to_vec(&event_audit)?)),
        );
        let max_identity_error = metrics
            .iter()
            .map(|row| row.accounting_identity_relative_error)
            .fold(0.0_f64, f64::max);
        if max_identity_error > 1e-9 {
            manifest.validation_status = ValidationStatus::Invalid;
            manifest.warnings.push(format!(
                "Accounting identity error exceeded tolerance: {max_identity_error:e}"
            ));
        } else {
            manifest.fidelity_class = FidelityClass::Qualitative;
        }
        manifest
            .invariants
            .insert("ledger_residual_minor_units".to_owned(), "0".to_owned());
        manifest.invariants.insert(
            "national_accounts_max_relative_error".to_owned(),
            format!("{max_identity_error:.16e}"),
        );
        manifest.invariants.insert(
            "runtime_parallel_jobs".to_owned(),
            self.execution_concurrency(branches.len() * options.replications as usize)
                .to_string(),
        );
        manifest.invariants.insert(
            "scheduler_version".to_owned(),
            self.scheduler_version().to_owned(),
        );
        manifest.invariants.insert(
            "scheduled_event_count".to_owned(),
            branch_runs
                .iter()
                .map(|run| run.economy.schedule_audit.event_count)
                .sum::<u64>()
                .to_string(),
        );

        Ok(SimulationResult {
            manifest,
            metrics,
            comparison,
            mechanisms,
            accounting,
            journal_hashes,
            event_audit,
        })
    }

    fn comparison(&self, metrics: &[MetricRow], end_month: u32) -> ComparisonReport {
        let requested_start = self
            .scenario
            .shocks
            .iter()
            .map(|shock| shock.start_month)
            .min()
            .unwrap_or(0);
        let last_available_month = end_month.saturating_sub(1);
        let evaluation_start = if requested_start >= end_month {
            0
        } else {
            requested_start
        };
        let evaluation_end = self
            .scenario
            .shocks
            .iter()
            .map(|shock| shock.start_month + shock.duration_months + shock.decay_months + 12)
            .max()
            .unwrap_or(last_available_month)
            .min(last_available_month);
        ComparisonReport::from_metrics(
            self.scenario.scenario_id.clone(),
            metrics,
            evaluation_start,
            evaluation_end,
            self.scenario.replications.confidence_level,
        )
    }

    pub fn checkpoint(
        &self,
        branch_id: &str,
        replication: u32,
        end_month: u32,
    ) -> OwgResult<Checkpoint> {
        let branch = self
            .scenario
            .policy_branches
            .iter()
            .find(|branch| branch.id == branch_id)
            .ok_or_else(|| OwgError::InvalidState(format!("unknown policy branch {branch_id}")))?;
        let run = self.run_branch(
            branch,
            replication,
            end_month.min(self.scenario.calendar.months),
        )?;
        Ok(Checkpoint::new(
            self.scenario.hash()?,
            branch_id.to_owned(),
            replication,
            run.economy,
            run.metrics,
        ))
    }

    pub fn resume(
        &self,
        mut checkpoint: Checkpoint,
        end_month: Option<u32>,
    ) -> OwgResult<Checkpoint> {
        let scenario_hash = self.scenario.hash()?;
        if checkpoint.scenario_hash != scenario_hash {
            return Err(OwgError::IncompatibleCheckpoint(
                "scenario hash does not match".to_owned(),
            ));
        }
        checkpoint.validate_integrity()?;
        checkpoint
            .economy
            .validate_credit_context(&self.scenario, true)
            .map_err(|error| {
                OwgError::IncompatibleCheckpoint(format!("credit scenario preflight: {error}"))
            })?;
        if checkpoint.next_month > self.scenario.calendar.months {
            return Err(OwgError::IncompatibleCheckpoint(format!(
                "checkpoint month {} exceeds scenario horizon {}",
                checkpoint.next_month, self.scenario.calendar.months
            )));
        }
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
            .ledger
            .assert_balanced()
            .and_then(|()| checkpoint.economy.assert_reserve_claims_balanced())
            .and_then(|()| checkpoint.economy.assert_deposit_claims_balanced())
            .map_err(|error| {
                OwgError::IncompatibleCheckpoint(format!(
                    "checkpoint settlement or deposit accounts do not reconcile: {error}"
                ))
            })?;
        let household_ids: Vec<String> = (0..self.scenario.population.households)
            .map(|index| format!("HH-{index:06}"))
            .collect();
        let firm_ids: Vec<String> = (0..self.scenario.population.firms)
            .map(|index| format!("FIRM-{index:05}"))
            .collect();
        let expected_institutions = if let Some(graph) = &self.scenario.initial_institutions {
            graph.clone()
        } else {
            InstitutionGraph::synthetic_firm_ownership(&household_ids, &firm_ids)?
        };
        if checkpoint.economy.households.ids != household_ids
            || checkpoint.economy.firms.ids != firm_ids
            || checkpoint.economy.institutions != expected_institutions
            || checkpoint.economy.governance.is_some() != self.scenario.governance.is_some()
        {
            return Err(OwgError::IncompatibleCheckpoint(
                "institution graph or agent IDs do not match scenario opening state".to_owned(),
            ));
        }
        if let Some(spec) = &self.scenario.governance {
            let mut expected = GovernanceState::new_with_institutions(
                spec,
                self.scenario.calendar.months,
                &expected_institutions,
                &self.scenario.geography.region_id,
            )?;
            for month in 0..checkpoint.economy.month {
                expected.advance(spec, month)?;
            }
            if checkpoint.economy.governance.as_ref() != Some(&expected) {
                return Err(OwgError::IncompatibleCheckpoint(
                    "governance decisions do not replay from the scenario".to_owned(),
                ));
            }
        }
        let branch = self
            .scenario
            .policy_branches
            .iter()
            .find(|branch| branch.id == checkpoint.branch_id)
            .ok_or_else(|| {
                OwgError::IncompatibleCheckpoint(format!("unknown branch {}", checkpoint.branch_id))
            })?;
        let target = end_month
            .unwrap_or(self.scenario.calendar.months)
            .min(self.scenario.calendar.months);
        while checkpoint.economy.month < target {
            let row = checkpoint.economy.step_month(&self.scenario, branch)?;
            checkpoint.metrics.push(row);
        }
        checkpoint.next_month = checkpoint.economy.month;
        checkpoint.journal_hash = checkpoint.economy.ledger.journal_hash();
        Ok(checkpoint)
    }

    fn scheduler_version(&self) -> &'static str {
        if self.scenario.credit_market.is_some() {
            crate::credit::CREDIT_SCHEDULER_VERSION
        } else {
            SCHEDULER_VERSION
        }
    }

    fn selected_branches<'a>(&'a self, options: &RunOptions) -> OwgResult<Vec<&'a PolicyBranch>> {
        let requested: Option<BTreeSet<&str>> = options
            .branch_ids
            .as_ref()
            .map(|ids| ids.iter().map(String::as_str).collect());
        let branches: Vec<&PolicyBranch> = self
            .scenario
            .policy_branches
            .iter()
            .filter(|branch| {
                requested
                    .as_ref()
                    .is_none_or(|ids| ids.contains(branch.id.as_str()))
            })
            .collect();
        if branches.is_empty() {
            return Err(OwgError::InvalidState(
                "no policy branches selected".to_owned(),
            ));
        }
        if let Some(requested) = requested {
            let found: BTreeSet<&str> = branches.iter().map(|branch| branch.id.as_str()).collect();
            let missing: Vec<&str> = requested.difference(&found).copied().collect();
            if !missing.is_empty() {
                return Err(OwgError::InvalidState(format!(
                    "unknown policy branches: {}",
                    missing.join(", ")
                )));
            }
        }
        Ok(branches)
    }

    fn run_branch(
        &self,
        branch: &PolicyBranch,
        replication: u32,
        end_month: u32,
    ) -> OwgResult<BranchRun> {
        let mut economy =
            Economy::new_with_parameters(&self.scenario, replication, &self.parameters)?;
        let mut metrics = Vec::with_capacity(end_month as usize);
        while economy.month < end_month {
            metrics.push(economy.step_month(&self.scenario, branch)?);
        }
        Ok(BranchRun {
            branch_id: branch.id.clone(),
            replication,
            metrics,
            economy,
        })
    }
}

fn execute_recorded_firm_month(
    economy: &mut Economy,
    scenario: &ScenarioSpec,
    branch: &PolicyBranch,
    tape: &DecisionTape,
    step: &DecisionStep,
    firm_id: &str,
    law_binding: Option<&FirmActionLawBinding>,
) -> OwgResult<(MetricRow, RecordedFirmDecisionAudit)> {
    // The one-step replay policy and entire economic month are local to this
    // transaction. A rejected proposal or a later scheduler failure cannot
    // consume a shared cursor or partially commit the world.
    let mut staged = economy.clone();
    let month = staged.month;
    let monthly_tape = DecisionTape {
        schema_version: DECISION_TAPE_VERSION.to_owned(),
        id: format!("{}-month-{month}", tape.id),
        descriptor: tape.descriptor.clone(),
        steps: vec![step.clone()],
    };
    let policy = RecordedPolicy::new(monthly_tape)?;
    let accepted_outcome = if let Some(binding) = law_binding {
        dispatch_recorded_firm_action_with_law(&mut staged, &policy, firm_id, binding)?
    } else {
        dispatch_recorded_firm_action(&mut staged, &policy, firm_id)?
    };
    let journal_hash_after_decision = staged.ledger.journal_hash();
    let row = staged.step_month(scenario, branch)?;
    let audit = RecordedFirmDecisionAudit {
        month,
        actor_id: firm_id.to_owned(),
        observation_hash: hash_json(&step.observations)?,
        proposal_hash: hash_json(&step.actions)?,
        accepted_outcome,
        journal_hash_after_decision,
        journal_hash_after_month: staged.ledger.journal_hash(),
        schedule_hash_after_month: staged.schedule_audit.schedule_hash(),
    };
    *economy = staged;
    Ok((row, audit))
}

fn hash_json<T: Serialize>(value: &T) -> OwgResult<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn mechanism_decompositions(
    metrics: &[MetricRow],
    evaluation_start: u32,
    evaluation_end: u32,
) -> Vec<MechanismDecomposition> {
    let branch_ids: BTreeSet<&str> = metrics.iter().map(|row| row.branch_id.as_str()).collect();
    branch_ids
        .into_iter()
        .filter_map(|branch_id| {
            let branch_rows: Vec<&MetricRow> = metrics
                .iter()
                .filter(|row| row.branch_id == branch_id)
                .collect();
            let evaluation_rows: Vec<&MetricRow> = branch_rows
                .iter()
                .copied()
                .filter(|row| row.month >= evaluation_start && row.month <= evaluation_end)
                .collect();
            if evaluation_rows.is_empty() {
                return None;
            }
            let replications = branch_rows
                .iter()
                .map(|row| row.replication)
                .collect::<BTreeSet<_>>()
                .len()
                .max(1) as f64;
            let mut rows_by_month = BTreeMap::<u32, Vec<&MetricRow>>::new();
            for row in &evaluation_rows {
                rows_by_month.entry(row.month).or_default().push(*row);
            }
            let (peak_month, peak_rows) = rows_by_month
                .iter()
                .max_by(|(_, left), (_, right)| {
                    mean(left, |row| row.inflation_year_over_year)
                        .total_cmp(&mean(right, |row| row.inflation_year_over_year))
                })
                .expect("evaluation rows created at least one month");
            let baseline_month = evaluation_start.saturating_sub(1);
            let baseline_rows: Vec<&MetricRow> = branch_rows
                .iter()
                .copied()
                .filter(|row| row.month == baseline_month)
                .collect();
            let baseline_rows = if baseline_rows.is_empty() {
                rows_by_month
                    .values()
                    .next()
                    .expect("evaluation rows created at least one month")
                    .clone()
            } else {
                baseline_rows
            };
            let peak_sector = [
                mean(peak_rows, |row| row.cpi_energy),
                mean(peak_rows, |row| row.cpi_food),
                mean(peak_rows, |row| row.cpi_manufacturing),
                mean(peak_rows, |row| row.cpi_services),
            ];
            let baseline_sector = [
                mean(&baseline_rows, |row| row.cpi_energy),
                mean(&baseline_rows, |row| row.cpi_food),
                mean(&baseline_rows, |row| row.cpi_manufacturing),
                mean(&baseline_rows, |row| row.cpi_services),
            ];
            Some(MechanismDecomposition {
                branch_id: branch_id.to_owned(),
                peak_inflation_month: *peak_month,
                peak_inflation_mean: mean(peak_rows, |row| row.inflation_year_over_year),
                cpi_sector_change_at_peak: std::array::from_fn(|index| {
                    peak_sector[index] - baseline_sector[index]
                }),
                mean_output_gap: mean(&evaluation_rows, |row| row.output_gap),
                mean_unemployment: mean(&evaluation_rows, |row| row.unemployment),
                cumulative_firm_defaults_per_replication: evaluation_rows
                    .iter()
                    .map(|row| f64::from(row.firm_defaults))
                    .sum::<f64>()
                    / replications,
                cumulative_household_defaults_per_replication: evaluation_rows
                    .iter()
                    .map(|row| f64::from(row.household_defaults))
                    .sum::<f64>()
                    / replications,
                cumulative_bank_resolutions_per_replication: evaluation_rows
                    .iter()
                    .map(|row| f64::from(row.bank_resolutions))
                    .sum::<f64>()
                    / replications,
                peak_credit_mean: rows_by_month
                    .values()
                    .map(|rows| mean(rows, |row| row.credit))
                    .fold(0.0_f64, f64::max),
                cumulative_fiscal_deficit_per_replication: evaluation_rows
                    .iter()
                    .map(|row| (-row.fiscal_balance).max(0.0))
                    .sum::<f64>()
                    / replications,
            })
        })
        .collect()
}

fn accounting_diagnostics(metrics: &[MetricRow]) -> Vec<AccountingDiagnostic> {
    let branch_ids: BTreeSet<&str> = metrics.iter().map(|row| row.branch_id.as_str()).collect();
    branch_ids
        .into_iter()
        .map(|branch_id| {
            let rows: Vec<&MetricRow> = metrics
                .iter()
                .filter(|row| row.branch_id == branch_id)
                .collect();
            AccountingDiagnostic {
                branch_id: branch_id.to_owned(),
                observation_count: rows.len() as u64,
                max_national_accounts_relative_error: rows
                    .iter()
                    .map(|row| row.accounting_identity_relative_error)
                    .fold(0.0_f64, f64::max),
                max_income_identity_relative_error: rows
                    .iter()
                    .map(|row| row.income_accounts.identity_relative_error)
                    .fold(0.0_f64, f64::max),
                max_institutional_balance_residual_minor_units: rows
                    .iter()
                    .map(|row| {
                        row.institutional_balances
                            .residual
                            .minor_units()
                            .unsigned_abs()
                    })
                    .max()
                    .unwrap_or(0),
                max_unclassified_balance_minor_units: rows
                    .iter()
                    .map(|row| {
                        row.institutional_balances
                            .unclassified
                            .minor_units()
                            .unsigned_abs()
                    })
                    .max()
                    .unwrap_or(0),
            }
        })
        .collect()
}

fn mean(rows: &[&MetricRow], value: impl Fn(&MetricRow) -> f64) -> f64 {
    rows.iter().map(|row| value(row)).sum::<f64>() / rows.len().max(1) as f64
}

fn precision_target_met(comparison: &ComparisonReport, target: f64) -> bool {
    comparison.scores.iter().all(|score| {
        score.objective_intervals.values().all(|interval| {
            let half_width = (interval.upper - interval.lower).abs() / 2.0;
            if interval.mean.abs() <= 1e-12 {
                half_width <= 1e-12
            } else {
                half_width / interval.mean.abs() <= target
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{RecordedFirmRunBinding, RunOptions, SimulationEngine};
    use crate::contracts::{PolicyActionKind, ScenarioSpec};
    use crate::governance::{
        GOVERNANCE_AUTHORITY_VERSION, GOVERNANCE_INSTITUTIONAL_VERSION, GOVERNANCE_VERSION,
        GovernanceChange, GovernanceProposal, GovernanceSpec, GroupStance, InfluenceGroup,
        InstitutionalGate, LawRule,
    };
    use crate::institutions::{
        AuthorityAppointment, EconomicRole, INSTITUTION_GRAPH_AUTHORITY_VERSION, InstitutionGraph,
        InstitutionalEntity, LegalForm, TaxClassification,
    };
    use crate::policy::{ActionBatch, AgentAction, ObservationBatch, PolicyDescriptor, PolicyKind};
    use crate::replay::{DECISION_TAPE_VERSION, DecisionStep, DecisionTape};
    use crate::strategic_dispatch::{RecordedFirmOutcome, observe_firm};
    use crate::{
        AccountKey, AccountKind, Checkpoint, DataBundle, JournalEntry, ModelPack, Money, Posting,
    };

    #[test]
    fn recorded_firm_run_is_bound_replayable_and_separate_from_normal_run() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 1;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario.clone()).unwrap();
        let opening =
            crate::Economy::new_with_parameters(&scenario, 0, &engine.parameters).unwrap();
        let firm_id = opening.firms.ids[0].clone();
        let observation = observe_firm(&opening, &firm_id).unwrap();
        let tape = DecisionTape {
            schema_version: DECISION_TAPE_VERSION.to_owned(),
            id: "recorded-one-month".to_owned(),
            descriptor: PolicyDescriptor {
                id: "test-recorded-policy".to_owned(),
                version: "1".to_owned(),
                sha256: "a".repeat(64),
                kind: PolicyKind::Rule,
                deterministic: true,
                requires_network: false,
                maximum_latency_ms: 1,
            },
            steps: vec![DecisionStep {
                observations: ObservationBatch {
                    observations: vec![observation],
                },
                actions: ActionBatch {
                    actions: vec![AgentAction::NoOp],
                    warnings: vec![],
                    used_fallback: false,
                },
                rationales: vec!["synthetic replay check".to_owned()],
                expected_effects: vec!["no owner transfer".to_owned()],
                confidences: vec![1.0],
            }],
        };
        let binding =
            RecordedFirmRunBinding::new(&scenario, "baseline", 0, firm_id.clone(), 1, &tape, None)
                .unwrap();
        let first = engine.run_recorded_firm(&binding, &tape).unwrap();
        let second = engine.run_recorded_firm(&binding, &tape).unwrap();
        assert_eq!(first.metrics_hash, second.metrics_hash);
        assert_eq!(first.final_economy_hash, second.final_economy_hash);
        assert_eq!(first.decision_audit_hash, second.decision_audit_hash);
        first.validate_integrity().unwrap();
        let ordinary = engine
            .run(&RunOptions {
                branch_ids: Some(vec!["baseline".to_owned()]),
                replications: 1,
                end_month: Some(1),
            })
            .unwrap();
        assert_eq!(first.metrics, ordinary.metrics);

        let mut wrong_binding = binding.clone();
        wrong_binding.decision_tape_hash = "0".repeat(64);
        assert!(engine.run_recorded_firm(&wrong_binding, &tape).is_err());
        let mut wrong_month = tape.clone();
        wrong_month.steps[0].observations.observations[0].month = 1;
        let wrong_month_binding =
            RecordedFirmRunBinding::new(&scenario, "baseline", 0, firm_id, 1, &wrong_month, None)
                .unwrap();
        assert!(
            engine
                .run_recorded_firm(&wrong_month_binding, &wrong_month)
                .is_err()
        );
        let mut tampered_result = first;
        tampered_result.decision_audit[0].actor_id = "forged".to_owned();
        assert!(tampered_result.validate_integrity().is_err());

        let direct_owner = opening
            .institutions
            .stakes
            .iter()
            .find(|stake| stake.owned_id == binding.firm_id)
            .unwrap()
            .owner_id
            .clone();
        let mut cash_tape = tape.clone();
        cash_tape.steps[0].actions.actions[0] = AgentAction::DirectOwnerCashTransfer {
            recipient_owner_id: direct_owner,
            amount: Money(1),
        };
        let cash_binding = RecordedFirmRunBinding::new(
            &scenario,
            "baseline",
            0,
            binding.firm_id.clone(),
            1,
            &cash_tape,
            None,
        )
        .unwrap();
        let cash_result = engine.run_recorded_firm(&cash_binding, &cash_tape).unwrap();
        assert!(matches!(
            cash_result.decision_audit[0].accepted_outcome,
            RecordedFirmOutcome::DirectOwnerCashTransfer { .. }
        ));
        assert_ne!(cash_result.final_journal_hash, second.final_journal_hash);
    }

    #[test]
    fn repeated_reference_runs_match_metric_hashes() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 6;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario.clone()).unwrap();
        let options = RunOptions {
            branch_ids: Some(vec!["baseline".to_owned()]),
            replications: 1,
            end_month: None,
        };
        let first = engine.run(&options).unwrap();
        let second = engine.run(&options).unwrap();
        assert_eq!(first.metrics, second.metrics);
        assert_eq!(
            first.manifest.output_hashes["metrics"],
            second.manifest.output_hashes["metrics"]
        );
    }

    #[test]
    fn checkpoint_resume_matches_uninterrupted_run() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 8;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario).unwrap();
        let checkpoint = engine.checkpoint("baseline", 0, 3).unwrap();
        let path =
            std::env::temp_dir().join(format!("owg-checkpoint-{}.json", uuid::Uuid::new_v4()));
        checkpoint.save(&path).unwrap();
        let checkpoint = Checkpoint::load(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let resumed = engine.resume(checkpoint, None).unwrap();
        let direct = engine
            .checkpoint("baseline", 0, engine.scenario().calendar.months)
            .unwrap();
        assert_eq!(resumed.metrics, direct.metrics);
        assert_eq!(resumed.journal_hash, direct.journal_hash);
    }

    #[test]
    fn resume_rejects_unpaired_reserves_even_if_journal_is_balanced() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 2;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario).unwrap();
        let mut checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        checkpoint
            .economy
            .ledger
            .post(JournalEntry::new(
                "unpaired-reserve-tamper",
                1,
                "test",
                "balanced globally but reserve claim is missing",
                vec![
                    Posting::new(
                        AccountKey::new("BANK-000", AccountKind::Reserve),
                        Money(100),
                    ),
                    Posting::new(
                        AccountKey::new("OWNERSHIP_SECTOR", AccountKind::Equity),
                        Money(-100),
                    ),
                ],
            ))
            .unwrap();
        checkpoint.journal_hash = checkpoint.economy.ledger.journal_hash();
        checkpoint.economy.ledger.assert_balanced().unwrap();
        assert!(engine.resume(checkpoint, Some(1)).is_err());
    }

    #[test]
    fn zero_step_resume_rejects_wrong_bank_deposit_liability() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 2;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario).unwrap();
        let mut checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        let bank_a = checkpoint.economy.banks.ids[0].clone();
        let bank_b = checkpoint.economy.banks.ids[1].clone();
        checkpoint
            .economy
            .ledger
            .post(JournalEntry::new(
                "wrong-bank-deposit-tamper",
                1,
                "test",
                "aggregate liabilities unchanged but one bank owes the other's customer",
                vec![
                    Posting::new(
                        AccountKey::new(bank_a, AccountKind::DepositLiability),
                        Money(100),
                    ),
                    Posting::new(
                        AccountKey::new(bank_b, AccountKind::DepositLiability),
                        Money(-100),
                    ),
                ],
            ))
            .unwrap();
        checkpoint.journal_hash = checkpoint.economy.ledger.journal_hash();
        checkpoint.economy.ledger.assert_balanced().unwrap();
        checkpoint.economy.assert_reserve_claims_balanced().unwrap();
        assert!(checkpoint.economy.assert_deposit_claims_balanced().is_err());
        let path = std::env::temp_dir().join(format!(
            "owg-wrong-bank-checkpoint-{}.json",
            uuid::Uuid::new_v4()
        ));
        checkpoint.save(&path).unwrap();
        assert!(Checkpoint::load(&path).is_err());
        std::fs::remove_file(path).unwrap();
        assert!(engine.resume(checkpoint, Some(1)).is_err());
    }

    #[test]
    fn checkpoint_load_rejects_globally_unbalanced_non_deposit_account() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 2;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario).unwrap();
        let checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        let mut value = serde_json::to_value(checkpoint).unwrap();
        let balances = value["economy"]["ledger"]["balances"]
            .as_array_mut()
            .unwrap();
        let equity = balances
            .iter_mut()
            .find(|row| row["account"]["kind"] == "equity")
            .unwrap();
        let old = equity["balance"].as_i64().unwrap();
        equity["balance"] = serde_json::json!(old + 1);
        let tampered: Checkpoint = serde_json::from_value(value).unwrap();
        assert!(tampered.economy.assert_deposit_claims_balanced().is_ok());
        assert!(tampered.economy.assert_reserve_claims_balanced().is_ok());
        assert!(tampered.economy.ledger.assert_balanced().is_err());
        let path = std::env::temp_dir().join(format!(
            "owg-global-ledger-tamper-{}.json",
            uuid::Uuid::new_v4()
        ));
        tampered.save(&path).unwrap();
        assert!(Checkpoint::load(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn zero_step_resume_rejects_checkpoint_metadata_tampering() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 2;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario).unwrap();
        let checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        engine.resume(checkpoint.clone(), Some(1)).unwrap();

        let mut bad = checkpoint.clone();
        bad.journal_hash = "0".repeat(64);
        assert!(engine.resume(bad, Some(1)).is_err());
        let mut bad = checkpoint.clone();
        bad.next_month = 0;
        assert!(engine.resume(bad, Some(1)).is_err());
        let mut bad = checkpoint.clone();
        bad.metrics.clear();
        assert!(engine.resume(bad, Some(1)).is_err());
        let mut bad = checkpoint.clone();
        bad.metrics[0].branch_id = "wrong-branch".to_owned();
        assert!(engine.resume(bad, Some(1)).is_err());
        let mut bad = checkpoint;
        bad.replication = 42;
        assert!(engine.resume(bad, Some(1)).is_err());
    }

    #[test]
    fn resume_rejects_structurally_valid_checkpoint_past_scenario_horizon() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 2;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario).unwrap();
        let mut checkpoint = engine.checkpoint("baseline", 0, 2).unwrap();
        checkpoint
            .metrics
            .push(checkpoint.metrics.last().unwrap().clone());
        checkpoint.economy.month = 3;
        checkpoint.next_month = 3;
        checkpoint.validate_integrity().unwrap();
        let error = engine.resume(checkpoint, None).unwrap_err();
        assert!(error.to_string().contains("exceeds scenario horizon"));
    }

    #[test]
    fn resume_rejects_unrecorded_ownership_change() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 3;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario).unwrap();
        let mut checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        checkpoint.economy.institutions.stakes[0].owner_id = "HH-000001".to_owned();
        checkpoint.economy.institutions.validate().unwrap();
        assert!(engine.resume(checkpoint, None).is_err());
    }

    #[test]
    fn checkpoint_load_and_zero_step_resume_reject_malformed_population() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 2;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let engine = SimulationEngine::new(scenario).unwrap();
        let mut checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        checkpoint.economy.households.bank_index[0] = usize::MAX;
        assert!(engine.resume(checkpoint.clone(), Some(1)).is_err());
        let path = std::env::temp_dir().join(format!(
            "owg-malformed-checkpoint-{}.json",
            uuid::Uuid::new_v4()
        ));
        checkpoint.save(&path).unwrap();
        let loaded = Checkpoint::load(&path);
        std::fs::remove_file(path).unwrap();
        assert!(loaded.is_err());
    }

    #[test]
    fn governance_audit_and_checkpoint_replay_are_bound_to_scenario() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 3;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        scenario.governance = Some(GovernanceSpec {
            schema_version: GOVERNANCE_VERSION.to_owned(),
            groups: vec![InfluenceGroup {
                id: "public".to_owned(),
                label: "Public coalition".to_owned(),
                weight: 1.0,
            }],
            gates: vec![InstitutionalGate {
                id: "rate-gate".to_owned(),
                domain: "rates".to_owned(),
                passage_threshold: 0.8,
                amendment_surcharge: 0.0,
                review_delay_months: 0,
            }],
            laws: vec![LawRule {
                id: "rate-authority".to_owned(),
                domain: "rates".to_owned(),
                enabled: true,
                resistance: 0.0,
            }],
            proposals: vec![GovernanceProposal {
                id: "raise-rate".to_owned(),
                submitted_month: 0,
                domain: "rates".to_owned(),
                sponsor_id: "public".to_owned(),
                stances: vec![GroupStance {
                    group_id: "public".to_owned(),
                    value: 1.0,
                }],
                change: GovernanceChange::EconomicPolicy {
                    law_id: "rate-authority".to_owned(),
                    duration_months: 1,
                    action: PolicyActionKind::PolicyRateDelta { basis_points: 100 },
                },
            }],
        });
        let engine = SimulationEngine::new(scenario).unwrap();
        let result = engine
            .run(&RunOptions {
                branch_ids: Some(vec!["baseline".to_owned()]),
                replications: 1,
                end_month: None,
            })
            .unwrap();
        assert_eq!(result.event_audit["baseline:0"].governance_events.len(), 2);
        assert!(result.manifest.output_hashes.contains_key("event_audit"));
        let checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        let resumed = engine.resume(checkpoint.clone(), None).unwrap();
        let direct = engine.checkpoint("baseline", 0, 3).unwrap();
        assert_eq!(resumed.metrics, direct.metrics);
        assert_eq!(resumed.economy.governance, direct.economy.governance);
        let mut tampered = checkpoint;
        tampered.economy.governance.as_mut().unwrap().groups[0].weight = 100.0;
        assert!(engine.resume(tampered, None).is_err());
    }

    #[test]
    fn authority_v2_checkpoint_replay_rejects_office_tampering() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 3;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        scenario.governance = Some(GovernanceSpec {
            schema_version: GOVERNANCE_AUTHORITY_VERSION.to_owned(),
            groups: vec![InfluenceGroup {
                id: "public".to_owned(),
                label: "Public coalition".to_owned(),
                weight: 1.0,
            }],
            gates: vec![
                InstitutionalGate {
                    id: "rate-gate".to_owned(),
                    domain: "rates".to_owned(),
                    passage_threshold: 0.5,
                    amendment_surcharge: 0.0,
                    review_delay_months: 0,
                },
                InstitutionalGate {
                    id: "other-gate".to_owned(),
                    domain: "other".to_owned(),
                    passage_threshold: 0.5,
                    amendment_surcharge: 0.0,
                    review_delay_months: 0,
                },
            ],
            laws: vec![LawRule {
                id: "rate-authority".to_owned(),
                domain: "rates".to_owned(),
                enabled: true,
                resistance: 0.0,
            }],
            proposals: vec![GovernanceProposal {
                id: "raise-rate".to_owned(),
                submitted_month: 0,
                domain: "rates".to_owned(),
                sponsor_id: "public".to_owned(),
                stances: vec![GroupStance {
                    group_id: "public".to_owned(),
                    value: 1.0,
                }],
                change: GovernanceChange::AuthorityDecision {
                    authority_id: "rate-gate".to_owned(),
                    change: Box::new(GovernanceChange::EconomicPolicy {
                        law_id: "rate-authority".to_owned(),
                        duration_months: 1,
                        action: PolicyActionKind::PolicyRateDelta { basis_points: 100 },
                    }),
                },
            }],
        });
        let engine = SimulationEngine::new(scenario).unwrap();
        let checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        let resumed = engine.resume(checkpoint.clone(), None).unwrap();
        let direct = engine.checkpoint("baseline", 0, 3).unwrap();
        assert_eq!(resumed.metrics, direct.metrics);
        assert_eq!(resumed.economy.governance, direct.economy.governance);
        assert_eq!(
            direct
                .economy
                .governance
                .as_ref()
                .unwrap()
                .decision_authority("rates"),
            Some("rate-gate")
        );
        let mut value = serde_json::to_value(checkpoint).unwrap();
        value["economy"]["governance"]["authority_by_domain"]["rates"] =
            serde_json::json!("other-gate");
        let tampered: Checkpoint = serde_json::from_value(value).unwrap();
        assert!(engine.resume(tampered, Some(1)).is_err());
    }

    #[test]
    fn institutional_v3_checkpoint_replay_binds_actor_and_region() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 3;
        scenario.shocks.clear();
        for branch in &mut scenario.policy_branches {
            branch.actions.clear();
        }
        let household_ids: Vec<String> = (0..scenario.population.households)
            .map(|index| format!("HH-{index:06}"))
            .collect();
        let firm_ids: Vec<String> = (0..scenario.population.firms)
            .map(|index| format!("FIRM-{index:05}"))
            .collect();
        let mut graph =
            InstitutionGraph::synthetic_firm_ownership(&household_ids, &firm_ids).unwrap();
        graph.schema_version = INSTITUTION_GRAPH_AUTHORITY_VERSION.to_owned();
        graph.entities[0].jurisdiction = scenario.geography.region_id.clone();
        graph.entities.push(InstitutionalEntity {
            id: "rate-office".to_owned(),
            legal_form: LegalForm::GovernmentBody,
            tax_classification: TaxClassification::Exempt,
            economic_role: EconomicRole::PublicAuthority,
            jurisdiction: scenario.geography.region_id.clone(),
            formed_month: 0,
        });
        graph.authority_appointments.push(AuthorityAppointment {
            actor_id: household_ids[0].clone(),
            office_id: "rate-office".to_owned(),
            start_month: 0,
            end_month: 2,
        });
        scenario.governance = Some(GovernanceSpec {
            schema_version: GOVERNANCE_INSTITUTIONAL_VERSION.to_owned(),
            groups: vec![InfluenceGroup {
                id: "public".to_owned(),
                label: "Public coalition".to_owned(),
                weight: 1.0,
            }],
            gates: vec![InstitutionalGate {
                id: "rate-office".to_owned(),
                domain: "rates".to_owned(),
                passage_threshold: 0.5,
                amendment_surcharge: 0.0,
                review_delay_months: 0,
            }],
            laws: vec![LawRule {
                id: "rate-law".to_owned(),
                domain: "rates".to_owned(),
                enabled: true,
                resistance: 0.0,
            }],
            proposals: vec![GovernanceProposal {
                id: "raise-rate".to_owned(),
                submitted_month: 0,
                domain: "rates".to_owned(),
                sponsor_id: "public".to_owned(),
                stances: vec![GroupStance {
                    group_id: "public".to_owned(),
                    value: 1.0,
                }],
                change: GovernanceChange::ActorDecision {
                    actor_id: household_ids[0].clone(),
                    authority_id: "rate-office".to_owned(),
                    change: Box::new(GovernanceChange::EconomicPolicy {
                        law_id: "rate-law".to_owned(),
                        duration_months: 1,
                        action: PolicyActionKind::PolicyRateDelta { basis_points: 100 },
                    }),
                },
            }],
        });
        assert!(SimulationEngine::new(scenario.clone()).is_err());
        scenario.initial_institutions = Some(graph);
        let engine = SimulationEngine::new(scenario).unwrap();
        let checkpoint = engine.checkpoint("baseline", 0, 1).unwrap();
        assert_eq!(
            checkpoint.economy.governance.as_ref().unwrap().events[0].outcome,
            "approved"
        );
        let resumed = engine.resume(checkpoint.clone(), None).unwrap();
        let direct = engine.checkpoint("baseline", 0, 3).unwrap();
        assert_eq!(resumed.metrics, direct.metrics);
        assert_eq!(resumed.economy.governance, direct.economy.governance);
        let mut value = serde_json::to_value(checkpoint).unwrap();
        value["economy"]["governance"]["institutional_authority_roster"]["appointments"][0]["actor_id"] =
            serde_json::json!(household_ids[1]);
        let tampered: Checkpoint = serde_json::from_value(value).unwrap();
        assert!(engine.resume(tampered, Some(1)).is_err());
    }

    #[test]
    fn adaptive_run_respects_replication_bounds() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.calendar.months = 2;
        scenario.shocks.clear();
        scenario
            .policy_branches
            .retain(|branch| branch.id == "baseline");
        scenario.replications.minimum = 2;
        scenario.replications.maximum = 3;
        scenario.replications.relative_half_width_target = 0.99;
        let engine = SimulationEngine::new(scenario).unwrap();
        let result = engine.run_to_precision().unwrap();
        let completed: u32 = result.manifest.invariants["monte_carlo_replications"]
            .parse()
            .unwrap();
        assert!((2..=3).contains(&completed));
        assert_eq!(result.metrics.len(), completed as usize * 2);
        assert!(
            result
                .event_audit
                .values()
                .all(|audit| audit.entry_count > 0)
        );
    }

    #[test]
    fn minimal_reference_profile_limits_live_worlds() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let engine = SimulationEngine::new(scenario).unwrap();
        assert_eq!(engine.execution_concurrency(64), 1);
    }

    #[test]
    fn zero_month_run_is_rejected() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let engine = SimulationEngine::new(scenario).unwrap();
        assert!(
            engine
                .run(&RunOptions {
                    branch_ids: Some(vec!["baseline".to_owned()]),
                    replications: 1,
                    end_month: Some(0),
                })
                .is_err()
        );
    }

    #[test]
    fn pre_shock_partial_run_uses_available_evaluation_window() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let engine = SimulationEngine::new(scenario).unwrap();
        let result = engine
            .run(&RunOptions {
                branch_ids: Some(vec!["baseline".to_owned()]),
                replications: 1,
                end_month: Some(2),
            })
            .unwrap();
        assert_eq!(result.comparison.evaluation_start_month, 0);
        assert_eq!(result.comparison.evaluation_end_month, 1);
        assert_eq!(
            result.comparison.scores[0].objective_intervals["inflation"].replication_count,
            1
        );
    }

    #[test]
    fn engine_resolves_and_hash_checks_model_and_data_inputs() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let engine = SimulationEngine::new(scenario.clone()).unwrap();
        assert_eq!(engine.model_pack().id, scenario.model_pack.id);
        assert_eq!(engine.data_bundle().id, scenario.data_bundle.id);
        assert!((engine.parameters.baseline_policy_rate - 0.04).abs() < f64::EPSILON);
        for (actual, expected) in engine
            .parameters
            .sector
            .consumption_share
            .iter()
            .zip([0.10, 0.30, 0.25, 0.35])
        {
            assert!((*actual - expected).abs() < f64::EPSILON);
        }

        let mut model: ModelPack = serde_json::from_str(include_str!(
            "../../../model-packs/small-open-economy-v0.1/model-pack.json"
        ))
        .unwrap();
        model
            .parameters
            .insert("baseline_policy_rate".to_owned(), 0.05);
        let data: DataBundle = serde_json::from_str(include_str!(
            "../../../data/synthetic-balanced-v0.1/data-bundle.json"
        ))
        .unwrap();
        assert!(
            SimulationEngine::with_inputs(
                scenario,
                model,
                data,
                include_bytes!("../../../data/synthetic-balanced-v0.1/sector_parameters.csv"),
            )
            .is_err()
        );
    }
}
