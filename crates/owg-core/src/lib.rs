//! Deterministic economic simulation kernel and public contracts.

pub mod capabilities;
pub mod checkpoint;
pub mod comparison;
pub mod contracts;
pub mod credit;
mod credit_settlement;
pub mod data;
pub mod economy;
pub mod engine;
pub mod error;
pub mod governance;
pub mod historical_finance;
pub mod historical_households;
pub mod historical_labor;
pub mod historical_production;
pub mod historical_recruiting;
pub mod institutions;
pub mod ledger;
pub mod live;
pub mod manifest;
pub mod metrics;
pub mod model;
pub mod money;
pub mod policy;
pub mod replay;
pub mod result;
pub mod rng;
pub mod scheduler;
pub mod strategic_dispatch;
pub mod vintage;

pub use capabilities::EngineCapabilities;
pub use checkpoint::{CHECKPOINT_VERSION, Checkpoint};
pub use comparison::{ComparisonReport, PolicyScore};
pub use contracts::{
    DataBundleRef, ModelPackRef, PolicyBranch, ScenarioSpec, ShockSpec, ValidationIssue,
};
pub use credit::{
    CREDIT_MARKET_VERSION, CREDIT_SCHEDULER_VERSION, ContractCreditMetrics, CreditAudit,
    CreditBook, CreditContractSpec, CreditLawBinding, CreditMarketSpec, CreditServiceOrder,
    CreditStatus,
};
pub use data::{DataBundle, DataProvenance, DataTable, SectorParameters, TableFormat};
pub use economy::{DirectOwnerCashTransfer, EconomicParameters, Economy};
pub use engine::{
    AccountingDiagnostic, EventAudit, MechanismDecomposition, RECORDED_FIRM_RUN_VERSION,
    RecordedFirmDecisionAudit, RecordedFirmRunBinding, RecordedFirmRunResult, RunOptions,
    SimulationEngine, SimulationResult,
};
pub use error::{OwgError, OwgResult};
pub use governance::{
    GOVERNANCE_AUTHORITY_VERSION, GOVERNANCE_INSTITUTIONAL_VERSION, GOVERNANCE_VERSION,
    GovernanceChange, GovernanceEvent, GovernanceProposal, GovernanceSpec, GovernanceState,
    GroupStance, InfluenceGroup, InstitutionalGate, LawRule,
};
pub use institutions::{
    AuthorityAppointment, EconomicRole, INSTITUTION_GRAPH_AUTHORITY_VERSION,
    INSTITUTION_GRAPH_VERSION, InstitutionGraph, InstitutionalEntity, LegalForm, OwnershipStake,
    TaxClassification,
};
pub use ledger::{AccountKey, AccountKind, JournalEntry, Ledger, LedgerAudit, Posting};
pub use manifest::{RunManifest, ValidationStatus};
pub use metrics::{IncomeAccounts, InstitutionalBalances, MetricRow};
pub use model::{AgentArchetype, MarketDefinition, ModelPack, ScheduleDefinition};
pub use money::Money;
pub use policy::{AgentAction, AgentPolicy, PolicyContract, PolicyDescriptor, PolicyKind};
pub use replay::{DECISION_TAPE_VERSION, DecisionStep, DecisionTape, RecordedPolicy};
pub use result::{MetricFiles, RESULT_BUNDLE_VERSION, ResultBundleContract};
pub use scheduler::{
    MultiRateScheduler, SCHEDULER_VERSION, ScheduleAudit, ScheduledEvent, SimulationEventKind,
    SimulationPhase,
};
pub use strategic_dispatch::{
    FIRM_ACTION_LAW_BINDING_VERSION, FirmActionLawBinding, RecordedFirmOutcome,
    RecordedLawAuthorization, dispatch_recorded_firm_action,
    dispatch_recorded_firm_action_with_law, observe_firm,
};
pub use vintage::{
    CaptureOriginVerifier, DATA_VINTAGE_STRICT_VERSION, DATA_VINTAGE_VERSION, DataVintageManifest,
    HistoricalByteAvailability, HistoricalSourceSnapshot, SourceSnapshot,
    StrictDataVintageManifest,
};

/// Public contract version implemented by this crate.
pub const CONTRACT_VERSION: &str = "owg.scenario.v1";

/// Engine version embedded in manifests and checkpoints.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
