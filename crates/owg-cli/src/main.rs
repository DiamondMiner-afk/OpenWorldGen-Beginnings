mod output;
mod plot;
mod scenario_io;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use owg_core::vintage::read_confined_vintage_file;
use owg_core::{
    Checkpoint, DataBundle, DataVintageManifest, DecisionTape, EngineCapabilities,
    HistoricalByteAvailability, ModelPack, PolicyContract, ResultBundleContract, RunManifest,
    RunOptions, ScenarioSpec, SimulationEngine, SimulationResult, StrictDataVintageManifest,
};
use scenario_io::{load_scenario, save_scenario};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Parser)]
#[command(name = "owg", version, about = "OpenWorldGen scenario laboratory")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SchemaKind {
    Capabilities,
    Scenario,
    DataBundle,
    DataVintage,
    DataVintageStrict,
    ModelPack,
    Policy,
    DecisionTape,
    RunManifest,
    ResultBundle,
    SimulationResult,
    Checkpoint,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ValidationKind {
    Scenario,
    DataBundle,
    DataVintage,
    DataVintageStrict,
    ModelPack,
    Policy,
    DecisionTape,
    RunManifest,
    Checkpoint,
    ResultBundle,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print machine-readable or human-readable engine capabilities.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
    /// Validate a versioned contract without running a simulation.
    Validate {
        document: PathBuf,
        #[arg(long, value_enum, default_value_t = ValidationKind::Scenario)]
        kind: ValidationKind,
        #[arg(long)]
        json: bool,
    },
    /// Write the preregistered v0.1 scenario.
    InitScenario {
        #[arg(long)]
        output: PathBuf,
    },
    /// Observe an actual opening firm for a bounded strategic-agent experiment.
    ObserveFirm {
        scenario: PathBuf,
        #[arg(long, default_value = "FIRM-00000")]
        firm: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Replay one firm's consecutive recorded choices into the economic engine.
    /// Experimental result, not a qualified ordinary result bundle.
    RunRecordedFirm {
        scenario: PathBuf,
        tape: PathBuf,
        #[arg(long, default_value = "FIRM-00000")]
        firm: String,
        #[arg(long, default_value = "baseline")]
        branch: String,
        #[arg(long)]
        law_binding: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Generate a versioned public-contract JSON Schema.
    Schema {
        #[arg(long, value_enum, default_value_t = SchemaKind::Scenario)]
        kind: SchemaKind,
        #[arg(long)]
        output: PathBuf,
    },
    /// Run one or more policy branches.
    Run {
        scenario: PathBuf,
        #[arg(long = "branch")]
        branches: Vec<String>,
        #[arg(long, default_value_t = 1)]
        replications: u32,
        #[arg(long)]
        end_month: Option<u32>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Run all branches with common random numbers and compare them.
    Compare {
        scenario: PathBuf,
        #[arg(long)]
        replications: Option<u32>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Run a branch to a month and save an auditable checkpoint.
    Checkpoint {
        scenario: PathBuf,
        #[arg(long, default_value = "baseline")]
        branch: String,
        #[arg(long, default_value_t = 0)]
        replication: u32,
        #[arg(long)]
        month: u32,
        #[arg(long)]
        output: PathBuf,
    },
    /// Resume a compatible checkpoint.
    Resume {
        scenario: PathBuf,
        checkpoint: PathBuf,
        #[arg(long)]
        end_month: Option<u32>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Benchmark a baseline run on this machine.
    Benchmark {
        scenario: PathBuf,
        #[arg(long)]
        months: Option<u32>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Produce a Markdown report from a result bundle.
    Report {
        bundle: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Produce a self-contained interactive graph dashboard from a result bundle.
    Plot {
        bundle: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Run the bounded synthetic calibration/recovery workflow.
    Calibrate {
        scenario: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Capabilities { json } => print_capabilities(json),
        Command::Validate {
            document,
            kind,
            json,
        } => validate(&document, kind, json),
        Command::InitScenario { output } => {
            save_scenario(&output, &ScenarioSpec::inflation_lab_v0_1())?;
            println!("wrote {}", output.display());
            Ok(())
        }
        Command::Schema { kind, output } => write_schema(&output, kind),
        Command::ObserveFirm {
            scenario,
            firm,
            output,
        } => {
            let engine = SimulationEngine::new(load_scenario(&scenario)?)?;
            let opening = engine.checkpoint("baseline", 0, 0)?;
            let observation = owg_core::observe_firm(&opening.economy, &firm)?;
            save_json(&output, &observation)
        }
        Command::RunRecordedFirm {
            scenario,
            tape,
            firm,
            branch,
            law_binding,
            output,
        } => {
            let scenario = load_scenario(&scenario)?;
            let tape: DecisionTape = serde_json::from_slice(&fs::read(tape)?)?;
            let law_binding = law_binding
                .map(|path| -> Result<owg_core::FirmActionLawBinding> {
                    Ok(serde_json::from_slice(&fs::read(path)?)?)
                })
                .transpose()?;
            let binding = owg_core::RecordedFirmRunBinding::new(
                &scenario,
                branch,
                0,
                firm,
                u32::try_from(tape.steps.len())?,
                &tape,
                law_binding,
            )?;
            let result = SimulationEngine::new(scenario)?.run_recorded_firm(&binding, &tape)?;
            result.validate_integrity()?;
            save_json(&output, &result)
        }
        Command::Run {
            scenario,
            branches,
            replications,
            end_month,
            output,
        } => with_failure_bundle(&output, "run", &scenario, || {
            let scenario = load_scenario(&scenario)?;
            let engine = SimulationEngine::new(scenario)?;
            let options = RunOptions {
                branch_ids: (!branches.is_empty()).then_some(branches),
                replications,
                end_month,
            };
            let result = engine.run(&options)?;
            output::write_result_bundle(&output, &result)?;
            print_run_summary(&output, &result);
            Ok(())
        }),
        Command::Compare {
            scenario,
            replications,
            output,
        } => with_failure_bundle(&output, "compare", &scenario, || {
            let scenario = load_scenario(&scenario)?;
            let engine = SimulationEngine::new(scenario)?;
            let result = if let Some(replications) = replications {
                let mut options = RunOptions::for_scenario(engine.scenario());
                options.replications = replications;
                engine.run(&options)?
            } else {
                engine.run_to_precision()?
            };
            output::write_result_bundle(&output, &result)?;
            print_run_summary(&output, &result);
            Ok(())
        }),
        Command::Checkpoint {
            scenario,
            branch,
            replication,
            month,
            output,
        } => {
            let scenario = load_scenario(&scenario)?;
            let engine = SimulationEngine::new(scenario)?;
            let checkpoint = engine.checkpoint(&branch, replication, month)?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            checkpoint.save(&output)?;
            println!(
                "saved checkpoint at month {} to {}",
                checkpoint.next_month,
                output.display()
            );
            Ok(())
        }
        Command::Resume {
            scenario,
            checkpoint,
            end_month,
            output,
        } => {
            let scenario = load_scenario(&scenario)?;
            let engine = SimulationEngine::new(scenario)?;
            let checkpoint = Checkpoint::load(checkpoint)?;
            let resumed = engine.resume(checkpoint, end_month)?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            resumed.save(&output)?;
            println!(
                "resumed {} to month {}",
                resumed.branch_id, resumed.next_month
            );
            Ok(())
        }
        Command::Benchmark {
            scenario,
            months,
            output,
        } => benchmark(&scenario, months, output.as_deref()),
        Command::Report { bundle, output } => output::write_report(&bundle, &output),
        Command::Plot { bundle, output } => {
            plot::write_plot_dashboard_from_bundle(&bundle, &output)?;
            println!("wrote {}", output.display());
            Ok(())
        }
        Command::Calibrate { scenario, output } => calibrate(&scenario, &output),
    }
}

fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    serde_json::to_writer_pretty(BufWriter::new(File::create(path)?), value)?;
    println!("wrote {}", path.display());
    Ok(())
}

fn print_capabilities(json: bool) -> Result<()> {
    let capabilities = EngineCapabilities::current();
    if json {
        println!("{}", serde_json::to_string_pretty(&capabilities)?);
        return Ok(());
    }
    println!("OpenWorldGen {}", capabilities.engine_version);
    println!(
        "contracts: scenario={}, checkpoint={}, result={}, scheduler={}",
        capabilities.scenario_contract_version,
        capabilities.checkpoint_version,
        capabilities.result_bundle_version,
        capabilities.scheduler_version
    );
    println!(
        "execution modes: {}",
        capabilities.execution_modes.join(", ")
    );
    println!(
        "compute profiles: {}",
        capabilities.compute_profiles.join(", ")
    );
    println!("agents: {}", capabilities.agent_archetypes.join(", "));
    println!("markets: {}", capabilities.markets.join(", "));
    println!("CLI commands: {}", capabilities.cli_commands.join(", "));
    println!(
        "Python SDK methods: {}",
        capabilities.python_sdk_methods.join(", ")
    );
    println!(
        "implemented systems: {}",
        capabilities.implemented_systems.join(", ")
    );
    println!("gated systems: {}", capabilities.gated_systems.join(", "));
    Ok(())
}

fn validate(path: &Path, kind: ValidationKind, json: bool) -> Result<()> {
    match kind {
        ValidationKind::Scenario => validate_scenario(path, json),
        ValidationKind::DataBundle => {
            let bundle: DataBundle = serde_json::from_reader(File::open(path)?)
                .with_context(|| format!("invalid data bundle {}", path.display()))?;
            bundle.validate()?;
            let bundle_directory = path.parent().unwrap_or_else(|| Path::new("."));
            for table in &bundle.tables {
                let table_path = bundle_directory.join(&table.relative_path);
                let bytes = fs::read(&table_path).with_context(|| {
                    format!(
                        "failed to read data table {} at {}",
                        table.id,
                        table_path.display()
                    )
                })?;
                bundle.validate_table_content(&table.id, &bytes)?;
            }
            print_contract_validation(json, &bundle.id, &bundle.hash()?);
            Ok(())
        }
        ValidationKind::DataVintage => {
            let vintage: DataVintageManifest = serde_json::from_reader(File::open(path)?)
                .with_context(|| format!("invalid data vintage {}", path.display()))?;
            vintage.validate()?;
            let directory = path.parent().unwrap_or_else(|| Path::new("."));
            for source in &vintage.sources {
                let source_path = directory.join(&source.relative_path);
                let bytes = read_confined_vintage_file(directory, &source.relative_path)
                    .with_context(|| {
                        format!(
                            "failed to read vintage source {} at {}",
                            source.id,
                            source_path.display()
                        )
                    })?;
                vintage.validate_source_content(&source.id, &bytes)?;
            }
            print_contract_validation(json, &vintage.id, &vintage.hash()?);
            Ok(())
        }
        ValidationKind::DataVintageStrict => {
            let vintage: StrictDataVintageManifest = serde_json::from_reader(File::open(path)?)
                .with_context(|| format!("invalid strict data vintage {}", path.display()))?;
            vintage.validate()?;
            let directory = path.parent().unwrap_or_else(|| Path::new("."));
            for item in &vintage.sources {
                let HistoricalByteAvailability::IndependentCapture {
                    archive_relative_path,
                    ..
                } = &item.byte_availability
                else {
                    bail!(
                        "source {} has publication-level evidence only; strict historical input admission requires an independent exact-byte capture",
                        item.source.id
                    );
                };
                let source_path = directory.join(&item.source.relative_path);
                let archive_path = directory.join(archive_relative_path);
                let source_bytes =
                    read_confined_vintage_file(directory, &item.source.relative_path)
                        .with_context(|| {
                            format!("failed to read source {}", source_path.display())
                        })?;
                let archive_bytes = read_confined_vintage_file(directory, archive_relative_path)
                    .with_context(|| {
                        format!("failed to read archive {}", archive_path.display())
                    })?;
                vintage.validate_capture_claim(&item.source.id, &source_bytes, &archive_bytes)?;
            }
            let hash = vintage.hash()?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "valid": true,
                        "id": vintage.id,
                        "sha256": hash,
                        "evidence_claim_validated": true,
                        "external_origin_verified": false,
                        "historical_input_admitted": false,
                        "issues": [],
                    })
                );
            } else {
                println!(
                    "capture claims structurally valid: {} ({}); external origin unverified, historical input NOT admitted",
                    vintage.id, hash
                );
            }
            Ok(())
        }
        ValidationKind::ModelPack => {
            let model: ModelPack = serde_json::from_reader(File::open(path)?)
                .with_context(|| format!("invalid model pack {}", path.display()))?;
            model.validate()?;
            if let Some(posterior) = &model.calibrated_posterior {
                let model_directory = path.parent().unwrap_or_else(|| Path::new("."));
                let artifact_path = model_directory.join(&posterior.artifact_path);
                let bytes = fs::read(&artifact_path).with_context(|| {
                    format!(
                        "failed to read calibrated posterior at {}",
                        artifact_path.display()
                    )
                })?;
                model.validate_posterior_content(&bytes)?;
            }
            print_contract_validation(json, &model.id, &model.hash()?);
            Ok(())
        }
        ValidationKind::Policy => {
            let policy: PolicyContract = serde_json::from_reader(File::open(path)?)
                .with_context(|| format!("invalid policy contract {}", path.display()))?;
            policy.validate()?;
            let hash = hex::encode(Sha256::digest(serde_json::to_vec(&policy)?));
            print_contract_validation(json, &policy.descriptor.id, &hash);
            Ok(())
        }
        ValidationKind::DecisionTape => {
            let tape: DecisionTape = serde_json::from_reader(File::open(path)?)
                .with_context(|| format!("invalid decision tape {}", path.display()))?;
            tape.validate()?;
            print_contract_validation(json, &tape.id, &tape.hash()?);
            Ok(())
        }
        ValidationKind::RunManifest => {
            let manifest: RunManifest = serde_json::from_reader(File::open(path)?)
                .with_context(|| format!("invalid run manifest {}", path.display()))?;
            manifest.validate()?;
            print_contract_validation(json, &manifest.run_id, &manifest.scenario_hash);
            Ok(())
        }
        ValidationKind::Checkpoint => {
            let checkpoint = Checkpoint::load(path)?;
            print_contract_validation(
                json,
                &format!("{}:{}", checkpoint.branch_id, checkpoint.replication),
                &checkpoint.scenario_hash,
            );
            Ok(())
        }
        ValidationKind::ResultBundle => {
            let verification = output::verify_result_bundle(path)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "valid": true,
                        "id": verification.run_id,
                        "sha256": verification.scenario_hash,
                        "verified_files": verification.verified_files,
                        "package_version": verification.package_version,
                        "issues": [],
                    })
                );
            } else {
                println!(
                    "valid: {} ({} files verified)",
                    verification.run_id, verification.verified_files
                );
            }
            Ok(())
        }
    }
}

fn validate_scenario(path: &Path, json: bool) -> Result<()> {
    let scenario = load_scenario(path)?;
    match scenario.validate() {
        Ok(()) => {
            if let Err(error) = SimulationEngine::new(scenario.clone()) {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "valid": false,
                            "scenario_id": scenario.scenario_id,
                            "issues": [{
                                "path": "/model_pack",
                                "code": "unresolved_inputs",
                                "message": error.to_string(),
                                "severity": "error",
                            }],
                        })
                    );
                }
                bail!("scenario inputs could not be resolved: {error}");
            }
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "valid": true,
                        "scenario_id": scenario.scenario_id,
                        "scenario_hash": scenario.hash()?,
                        "issues": [],
                    })
                );
            } else {
                println!("valid: {} ({})", scenario.scenario_id, scenario.hash()?);
            }
            Ok(())
        }
        Err(issues) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "valid": false,
                        "issues": issues,
                    })
                );
            } else {
                for issue in &issues {
                    eprintln!("{} [{}]: {}", issue.path, issue.code, issue.message);
                }
            }
            bail!("scenario is invalid")
        }
    }
}

fn print_contract_validation(json: bool, id: &str, hash: &str) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "valid": true,
                "id": id,
                "sha256": hash,
                "issues": [],
            })
        );
    } else {
        println!("valid: {id} ({hash})");
    }
}

fn write_schema(path: &Path, kind: SchemaKind) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let schema = match kind {
        SchemaKind::Capabilities => {
            serde_json::to_value(schemars::schema_for!(EngineCapabilities))?
        }
        SchemaKind::Scenario => serde_json::to_value(schemars::schema_for!(ScenarioSpec))?,
        SchemaKind::DataBundle => serde_json::to_value(schemars::schema_for!(DataBundle))?,
        SchemaKind::DataVintage => {
            serde_json::to_value(schemars::schema_for!(DataVintageManifest))?
        }
        SchemaKind::DataVintageStrict => {
            serde_json::to_value(schemars::schema_for!(StrictDataVintageManifest))?
        }
        SchemaKind::ModelPack => serde_json::to_value(schemars::schema_for!(ModelPack))?,
        SchemaKind::Policy => serde_json::to_value(schemars::schema_for!(PolicyContract))?,
        SchemaKind::DecisionTape => serde_json::to_value(schemars::schema_for!(DecisionTape))?,
        SchemaKind::RunManifest => serde_json::to_value(schemars::schema_for!(RunManifest))?,
        SchemaKind::ResultBundle => {
            serde_json::to_value(schemars::schema_for!(ResultBundleContract))?
        }
        SchemaKind::SimulationResult => {
            serde_json::to_value(schemars::schema_for!(SimulationResult))?
        }
        SchemaKind::Checkpoint => serde_json::to_value(schemars::schema_for!(Checkpoint))?,
    };
    serde_json::to_writer_pretty(BufWriter::new(File::create(path)?), &schema)?;
    println!("wrote {}", path.display());
    Ok(())
}

fn benchmark(path: &Path, months: Option<u32>, output: Option<&Path>) -> Result<()> {
    let scenario = load_scenario(path)?;
    let engine = SimulationEngine::new(scenario)?;
    let options = RunOptions {
        branch_ids: Some(vec!["baseline".to_owned()]),
        replications: 1,
        end_month: months,
    };
    let started = Instant::now();
    let result = engine.run(&options)?;
    let elapsed = started.elapsed();
    let months_run = result.metrics.len();
    let journal_entries = result
        .event_audit
        .values()
        .map(|audit| audit.entry_count)
        .sum::<u64>();
    let report = serde_json::json!({
        "benchmark_version": "owg.benchmark.v1",
        "scenario_id": engine.scenario().scenario_id,
        "scenario_hash": engine.scenario().hash()?,
        "engine_version": owg_core::ENGINE_VERSION,
        "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "compute_profile": engine.scenario().compute_profile,
        "execution_mode": engine.scenario().execution_mode,
        "branch": "baseline",
        "replications": 1,
        "households": engine.scenario().population.households,
        "firms": engine.scenario().population.firms,
        "banks": engine.scenario().population.banks,
        "months": months_run,
        "elapsed_seconds": elapsed.as_secs_f64(),
        "target_elapsed_seconds": 120.0,
        "elapsed_target_met": elapsed.as_secs_f64() < 120.0,
        "months_per_second": months_run as f64 / elapsed.as_secs_f64().max(f64::MIN_POSITIVE),
        "journal_entries_hashed": journal_entries,
        "runtime_parallel_jobs": result.manifest.invariants["runtime_parallel_jobs"],
        "metrics_sha256": result.manifest.output_hashes["metrics"],
        "platform": result.manifest.platform,
        "ledger_valid": result.metrics.iter().all(|row| row.ledger_residual_minor_units == 0),
        "accounting_error_max": result.metrics.iter().map(|row| row.accounting_identity_relative_error).fold(0.0_f64, f64::max),
    });
    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        serde_json::to_writer_pretty(BufWriter::new(File::create(output)?), &report)?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[derive(Debug, Serialize)]
struct CalibrationReport {
    calibration_version: &'static str,
    scenario_id: String,
    status: &'static str,
    method: &'static str,
    seeds: Vec<u64>,
    baseline_cpi_terminal_mean: f64,
    baseline_unemployment_terminal_mean: f64,
    accounting_gate_passed: bool,
    note: &'static str,
}

fn calibrate(path: &Path, output: &Path) -> Result<()> {
    let mut scenario = load_scenario(path)?;
    let original_seed = scenario.root_seed;
    let seeds = vec![
        original_seed,
        original_seed.wrapping_add(1),
        original_seed.wrapping_add(2),
        original_seed.wrapping_add(3),
    ];
    let mut terminal_cpi = Vec::new();
    let mut terminal_unemployment = Vec::new();
    let mut accounting_gate_passed = true;
    for seed in &seeds {
        scenario.root_seed = *seed;
        let engine = SimulationEngine::new(scenario.clone())?;
        let result = engine.run(&RunOptions {
            branch_ids: Some(vec!["baseline".to_owned()]),
            replications: 1,
            end_month: Some(24),
        })?;
        let terminal = result
            .metrics
            .last()
            .context("calibration run returned no metrics")?;
        terminal_cpi.push(terminal.cpi);
        terminal_unemployment.push(terminal.unemployment);
        accounting_gate_passed &= result
            .metrics
            .iter()
            .all(|row| row.accounting_identity_relative_error <= 1e-9);
    }
    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
    let report = CalibrationReport {
        calibration_version: "owg.synthetic-recovery.v1",
        scenario_id: scenario.scenario_id,
        status: "bounded_synthetic_smoke_test",
        method: "four-seed baseline moment check",
        seeds,
        baseline_cpi_terminal_mean: mean(&terminal_cpi),
        baseline_unemployment_terminal_mean: mean(&terminal_unemployment),
        accounting_gate_passed,
        note: "This verifies the calibration workflow boundary only. It is not empirical calibration or forecast qualification.",
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    serde_json::to_writer_pretty(BufWriter::new(File::create(output)?), &report)?;
    println!("wrote {}", output.display());
    Ok(())
}

fn print_run_summary(output: &Path, result: &SimulationResult) {
    println!("run: {}", result.manifest.run_id);
    println!("rows: {}", result.metrics.len());
    println!(
        "pareto frontier: {}",
        result.comparison.pareto_frontier.join(", ")
    );
    println!("bundle: {}", output.display());
}

fn with_failure_bundle(
    output_directory: &Path,
    operation: &str,
    input: &Path,
    action: impl FnOnce() -> Result<()>,
) -> Result<()> {
    match action() {
        Ok(()) => Ok(()),
        Err(error) => {
            if let Err(write_error) =
                output::write_failure_manifest(output_directory, operation, input, &error)
            {
                eprintln!("also failed to write failure manifest: {write_error:#}");
            }
            Err(error)
        }
    }
}
