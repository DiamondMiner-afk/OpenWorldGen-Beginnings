use crate::CONTRACT_VERSION;
use crate::error::{OwgError, OwgResult};
use crate::money::Money;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct ModelPackRef {
    pub id: String,
    pub version: String,
    pub sha256: String,
    pub validation_status: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct DataBundleRef {
    pub id: String,
    pub version: String,
    pub sha256: String,
    pub source: String,
    pub licence: String,
    pub retrieved_at: String,
    pub confidentiality: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct CalendarSpec {
    pub start_year: i32,
    pub months: u32,
    #[serde(default = "default_weeks_per_month")]
    pub weeks_per_month: u8,
}

const fn default_weeks_per_month() -> u8 {
    4
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct GeographySpec {
    pub region_id: String,
    pub currency: String,
    pub price_base_year: i32,
    #[serde(default = "default_rest_of_world")]
    pub rest_of_world_id: String,
}

fn default_rest_of_world() -> String {
    "ROW".to_owned()
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct PopulationSpec {
    pub households: usize,
    pub firms: usize,
    pub banks: usize,
    #[serde(default = "default_household_weight")]
    pub household_weight: f64,
}

const fn default_household_weight() -> f64 {
    1.0
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeProfile {
    Minimal,
    Standard,
    Custom,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Reference,
    Fast,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Sector {
    Energy,
    Food,
    Manufacturing,
    Services,
}

impl Sector {
    pub const ALL: [Self; 4] = [
        Self::Energy,
        Self::Food,
        Self::Manufacturing,
        Self::Services,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Energy => 0,
            Self::Food => 1,
            Self::Manufacturing => 2,
            Self::Services => 3,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Energy => "energy",
            Self::Food => "food",
            Self::Manufacturing => "manufacturing",
            Self::Services => "services",
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ShockSpec {
    pub id: String,
    pub start_month: u32,
    pub duration_months: u32,
    #[serde(default)]
    pub decay_months: u32,
    #[serde(flatten)]
    pub kind: ShockKind,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShockKind {
    ImportPrice { sector: Sector, multiplier: f64 },
    Productivity { sector: Sector, multiplier: f64 },
    SupplyCapacity { sector: Sector, multiplier: f64 },
    Confidence { multiplier: f64 },
    CreditConditions { spread_basis_points: i32 },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct PolicyBranch {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub actions: Vec<PolicyActionSpec>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct PolicyActionSpec {
    pub id: String,
    pub start_month: u32,
    pub duration_months: u32,
    #[serde(flatten)]
    pub kind: PolicyActionKind,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PolicyActionKind {
    PolicyRateDelta {
        basis_points: i32,
    },
    TargetedTransfer {
        amount_per_household: Money,
        bottom_share: f64,
    },
    IndirectTaxDelta {
        sector: Sector,
        basis_points: i32,
    },
    PublicInvestment {
        monthly_amount: Money,
        energy_intensity_reduction: f64,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ReplicationSpec {
    pub minimum: u32,
    pub maximum: u32,
    pub confidence_level: f64,
    pub relative_half_width_target: f64,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum MetricName {
    Cpi,
    InflationYearOverYear,
    RealGdp,
    NominalGdp,
    IncomeAccounts,
    InstitutionalBalances,
    OutputGap,
    Unemployment,
    AverageWage,
    ConsumptionDeciles,
    Gini,
    PovertyShare,
    Inventories,
    FirmDefaults,
    HouseholdDefaults,
    BankResolutions,
    Credit,
    FiscalBalance,
    GovernmentDebt,
    Imports,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct MetricSpec {
    pub names: Vec<MetricName>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ScenarioSpec {
    pub schema_version: String,
    pub scenario_id: String,
    pub title: String,
    pub model_pack: ModelPackRef,
    pub data_bundle: DataBundleRef,
    pub calendar: CalendarSpec,
    pub geography: GeographySpec,
    pub population: PopulationSpec,
    /// Optional political-process experiment. Legacy policy branches remain
    /// exogenous interventions and are not screened by this process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance: Option<crate::governance::GovernanceSpec>,
    /// Optional, fully specified initial ownership. This changes the
    /// institution graph, not initial cash or the absence of equity flows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_institutions: Option<crate::institutions::InstitutionGraph>,
    /// Opt-in, scenario-bound bank/customer credit contracts and service order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit_market: Option<crate::credit::CreditMarketSpec>,
    pub root_seed: u64,
    pub compute_profile: ComputeProfile,
    pub execution_mode: ExecutionMode,
    pub shocks: Vec<ShockSpec>,
    pub policy_branches: Vec<PolicyBranch>,
    pub replications: ReplicationSpec,
    pub metrics: MetricSpec,
}

impl ScenarioSpec {
    pub fn validate(&self) -> Result<(), Vec<ValidationIssue>> {
        let mut issues = Vec::new();

        if self.schema_version != CONTRACT_VERSION {
            issues.push(ValidationIssue::error(
                "/schema_version",
                "unsupported_contract",
                format!("expected {CONTRACT_VERSION}, found {}", self.schema_version),
            ));
        }
        if self.scenario_id.trim().is_empty() {
            issues.push(ValidationIssue::error(
                "/scenario_id",
                "required",
                "scenario_id cannot be empty",
            ));
        }
        if self.title.trim().is_empty() {
            issues.push(ValidationIssue::error(
                "/title",
                "required",
                "title cannot be empty",
            ));
        }
        if self.model_pack.id.trim().is_empty()
            || self.model_pack.version.trim().is_empty()
            || self.model_pack.validation_status.trim().is_empty()
            || !valid_sha256(&self.model_pack.sha256)
        {
            issues.push(ValidationIssue::error(
                "/model_pack",
                "invalid_reference",
                "model pack requires an id, version, validation status, and SHA-256",
            ));
        }
        if self.data_bundle.id.trim().is_empty()
            || self.data_bundle.version.trim().is_empty()
            || self.data_bundle.source.trim().is_empty()
            || self.data_bundle.licence.trim().is_empty()
            || self.data_bundle.retrieved_at.trim().is_empty()
            || self.data_bundle.confidentiality.trim().is_empty()
            || !valid_sha256(&self.data_bundle.sha256)
        {
            issues.push(ValidationIssue::error(
                "/data_bundle",
                "invalid_reference",
                "data bundle requires complete provenance and a SHA-256",
            ));
        }
        if self.geography.region_id.trim().is_empty()
            || self.geography.currency.trim().is_empty()
            || self.geography.rest_of_world_id.trim().is_empty()
        {
            issues.push(ValidationIssue::error(
                "/geography",
                "required",
                "region, currency, and rest-of-world identifiers are required",
            ));
        }
        if self.calendar.months == 0 {
            issues.push(ValidationIssue::error(
                "/calendar/months",
                "range",
                "months must be positive",
            ));
        }
        if self.calendar.weeks_per_month == 0 {
            issues.push(ValidationIssue::error(
                "/calendar/weeks_per_month",
                "range",
                "weeks_per_month must be positive",
            ));
        }
        if self.population.households == 0
            || self.population.firms < Sector::ALL.len()
            || self.population.banks == 0
        {
            issues.push(ValidationIssue::error(
                "/population",
                "minimum_population",
                "requires households > 0, at least four firms, and banks > 0",
            ));
        }
        if let Some(graph) = &self.initial_institutions {
            let household_ids: Vec<String> = (0..self.population.households)
                .map(|index| format!("HH-{index:06}"))
                .collect();
            let firm_ids: Vec<String> = (0..self.population.firms)
                .map(|index| format!("FIRM-{index:05}"))
                .collect();
            if let Err(error) = graph.validate_population(&household_ids, &firm_ids) {
                issues.push(ValidationIssue::error(
                    "/initial_institutions",
                    "invalid_institution_graph",
                    error.to_string(),
                ));
            }
        }
        if let Some(governance) = &self.governance
            && let Err(error) = governance.validate(self.calendar.months)
        {
            issues.push(ValidationIssue::error(
                "/governance",
                "invalid_governance",
                error.to_string(),
            ));
        }
        if let Some(credit) = &self.credit_market
            && let Err(error) = credit.validate(self)
        {
            issues.push(ValidationIssue::error(
                "/credit_market",
                "invalid_credit_market",
                error.to_string(),
            ));
        }
        if let Some(governance) = &self.governance
            && governance.schema_version == crate::governance::GOVERNANCE_INSTITUTIONAL_VERSION
        {
            let result = self
                .initial_institutions
                .as_ref()
                .ok_or_else(|| {
                    crate::error::OwgError::InvalidState(
                        "v3 governance requires an explicit institution graph".to_owned(),
                    )
                })
                .and_then(|graph| {
                    governance.validate_with_institutions(
                        self.calendar.months,
                        graph,
                        &self.geography.region_id,
                    )
                });
            if let Err(error) = result {
                issues.push(ValidationIssue::error(
                    "/governance",
                    "invalid_institutional_governance",
                    error.to_string(),
                ));
            }
        }
        if !self.population.household_weight.is_finite() || self.population.household_weight <= 0.0
        {
            issues.push(ValidationIssue::error(
                "/population/household_weight",
                "range",
                "household_weight must be finite and positive",
            ));
        }
        validate_identifiers(
            "/shocks",
            self.shocks.iter().map(|shock| shock.id.as_str()),
            &mut issues,
        );
        validate_identifiers(
            "/policy_branches",
            self.policy_branches.iter().map(|branch| branch.id.as_str()),
            &mut issues,
        );

        let baseline = self
            .policy_branches
            .iter()
            .find(|branch| branch.id == "baseline");
        if baseline.is_none() {
            issues.push(ValidationIssue::error(
                "/policy_branches",
                "baseline_required",
                "a branch with id 'baseline' is required",
            ));
        } else if baseline.is_some_and(|branch| !branch.actions.is_empty()) {
            issues.push(ValidationIssue::error(
                "/policy_branches/baseline/actions",
                "baseline_must_be_empty",
                "baseline cannot contain discretionary actions",
            ));
        }

        for (index, shock) in self.shocks.iter().enumerate() {
            let window_end = shock
                .start_month
                .checked_add(shock.duration_months)
                .and_then(|end| end.checked_add(shock.decay_months));
            if shock.duration_months == 0
                || shock.start_month >= self.calendar.months
                || window_end.is_none_or(|end| end > self.calendar.months)
            {
                issues.push(ValidationIssue::error(
                    format!("/shocks/{index}"),
                    "invalid_window",
                    "shock window must be non-empty and inside the calendar",
                ));
            }
            let valid = match shock.kind {
                ShockKind::ImportPrice { multiplier, .. }
                | ShockKind::Productivity { multiplier, .. }
                | ShockKind::SupplyCapacity { multiplier, .. }
                | ShockKind::Confidence { multiplier } => {
                    multiplier.is_finite() && multiplier > 0.0
                }
                ShockKind::CreditConditions { .. } => true,
            };
            if !valid {
                issues.push(ValidationIssue::error(
                    format!("/shocks/{index}"),
                    "invalid_multiplier",
                    "shock multiplier must be finite and positive",
                ));
            }
        }

        for (branch_index, branch) in self.policy_branches.iter().enumerate() {
            validate_identifiers(
                &format!("/policy_branches/{branch_index}/actions"),
                branch.actions.iter().map(|action| action.id.as_str()),
                &mut issues,
            );
            for (action_index, action) in branch.actions.iter().enumerate() {
                let path = format!("/policy_branches/{branch_index}/actions/{action_index}");
                let window_end = action.start_month.checked_add(action.duration_months);
                if action.duration_months == 0
                    || action.start_month >= self.calendar.months
                    || window_end.is_none_or(|end| end > self.calendar.months)
                {
                    issues.push(ValidationIssue::error(
                        path.clone(),
                        "invalid_window",
                        "policy window must be non-empty and inside the calendar",
                    ));
                }
                match action.kind {
                    PolicyActionKind::TargetedTransfer {
                        amount_per_household,
                        bottom_share,
                    } => {
                        if amount_per_household < Money::ZERO
                            || !bottom_share.is_finite()
                            || !(0.0 < bottom_share && bottom_share <= 1.0)
                        {
                            issues.push(ValidationIssue::error(
                                path,
                                "invalid_transfer",
                                "transfer must be non-negative and bottom_share in [0, 1]",
                            ));
                        }
                    }
                    PolicyActionKind::PublicInvestment {
                        monthly_amount,
                        energy_intensity_reduction,
                    } => {
                        if monthly_amount < Money::ZERO
                            || !energy_intensity_reduction.is_finite()
                            || !(0.0..=0.25).contains(&energy_intensity_reduction)
                        {
                            issues.push(ValidationIssue::error(
                                path,
                                "invalid_investment",
                                "investment must be non-negative and reduction in [0, 0.25]",
                            ));
                        }
                    }
                    PolicyActionKind::PolicyRateDelta { .. }
                    | PolicyActionKind::IndirectTaxDelta { .. } => {}
                }
            }
        }

        if self.replications.minimum == 0
            || self.replications.maximum < self.replications.minimum
            || !(0.5..1.0).contains(&self.replications.confidence_level)
            || !(0.0..1.0).contains(&self.replications.relative_half_width_target)
        {
            issues.push(ValidationIssue::error(
                "/replications",
                "invalid_replication_plan",
                "invalid minimum, maximum, confidence, or precision target",
            ));
        }
        if self.metrics.names.is_empty() {
            issues.push(ValidationIssue::error(
                "/metrics/names",
                "required",
                "at least one metric is required",
            ));
        } else if self.metrics.names.iter().collect::<BTreeSet<_>>().len()
            != self.metrics.names.len()
        {
            issues.push(ValidationIssue::error(
                "/metrics/names",
                "duplicate_metric",
                "metric names must be unique",
            ));
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }

    pub fn validated(&self) -> OwgResult<()> {
        self.validate()
            .map_err(|issues| OwgError::Validation(issues.len(), issues))
    }

    pub fn hash(&self) -> OwgResult<String> {
        let bytes = serde_json::to_vec(self)?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }

    /// Preregistered v0.1 experiment with six common-random-number branches.
    #[must_use]
    pub fn inflation_lab_v0_1() -> Self {
        let shock_start = 24;
        let policy_start = 24;
        Self {
            schema_version: CONTRACT_VERSION.to_owned(),
            scenario_id: "inflation-lab-v0.1".to_owned(),
            title: "Imported energy inflation policy laboratory".to_owned(),
            model_pack: ModelPackRef {
                id: "owg-small-open-economy".to_owned(),
                version: "0.1.0".to_owned(),
                sha256: "b157d5a578e289fcf216dcb7e551e597b10a4f1d4825ee5e095175aa411ab482"
                    .to_owned(),
                validation_status: "development".to_owned(),
            },
            data_bundle: DataBundleRef {
                id: "owg-synthetic-balanced".to_owned(),
                version: "0.1.0".to_owned(),
                sha256: "fecb688f45e18c1c10fcd2418247b0f72e0d754134e332399347b8350a420d35"
                    .to_owned(),
                source: "OpenWorldGen preregistered synthetic generator".to_owned(),
                licence: "internal-research".to_owned(),
                retrieved_at: "not-applicable".to_owned(),
                confidentiality: "public-synthetic".to_owned(),
            },
            calendar: CalendarSpec {
                start_year: 2020,
                months: 120,
                weeks_per_month: 4,
            },
            geography: GeographySpec {
                region_id: "SYNTHETICA".to_owned(),
                currency: "OWG".to_owned(),
                price_base_year: 2020,
                rest_of_world_id: "ROW".to_owned(),
            },
            population: PopulationSpec {
                households: 1_000,
                firms: 100,
                banks: 2,
                household_weight: 1.0,
            },
            initial_institutions: None,
            governance: None,
            credit_market: None,
            root_seed: 20_260_724,
            compute_profile: ComputeProfile::Minimal,
            execution_mode: ExecutionMode::Reference,
            shocks: vec![ShockSpec {
                id: "imported-energy-price".to_owned(),
                start_month: shock_start,
                duration_months: 12,
                decay_months: 12,
                kind: ShockKind::ImportPrice {
                    sector: Sector::Energy,
                    multiplier: 1.30,
                },
            }],
            policy_branches: vec![
                PolicyBranch {
                    id: "baseline".to_owned(),
                    label: "Baseline reaction rule".to_owned(),
                    actions: Vec::new(),
                },
                PolicyBranch {
                    id: "monetary-tightening".to_owned(),
                    label: "Temporary 100 basis point tightening".to_owned(),
                    actions: vec![PolicyActionSpec {
                        id: "tighten-100bp".to_owned(),
                        start_month: policy_start,
                        duration_months: 12,
                        kind: PolicyActionKind::PolicyRateDelta { basis_points: 100 },
                    }],
                },
                PolicyBranch {
                    id: "targeted-transfers".to_owned(),
                    label: "Transfers to bottom 40 percent".to_owned(),
                    actions: vec![PolicyActionSpec {
                        id: "bottom-40-transfer".to_owned(),
                        start_month: policy_start,
                        duration_months: 12,
                        kind: PolicyActionKind::TargetedTransfer {
                            amount_per_household: Money(2_500),
                            bottom_share: 0.40,
                        },
                    }],
                },
                PolicyBranch {
                    id: "essential-tax-cut".to_owned(),
                    label: "Temporary essential energy tax reduction".to_owned(),
                    actions: vec![PolicyActionSpec {
                        id: "energy-tax-minus-500bp".to_owned(),
                        start_month: policy_start,
                        duration_months: 12,
                        kind: PolicyActionKind::IndirectTaxDelta {
                            sector: Sector::Energy,
                            basis_points: -500,
                        },
                    }],
                },
                PolicyBranch {
                    id: "public-investment".to_owned(),
                    label: "Energy-efficiency public investment".to_owned(),
                    actions: vec![PolicyActionSpec {
                        id: "energy-efficiency-investment".to_owned(),
                        start_month: policy_start,
                        duration_months: 24,
                        kind: PolicyActionKind::PublicInvestment {
                            monthly_amount: Money(250_000),
                            energy_intensity_reduction: 0.005,
                        },
                    }],
                },
                PolicyBranch {
                    id: "combined".to_owned(),
                    label: "Targeted transfer plus tightening and investment".to_owned(),
                    actions: vec![
                        PolicyActionSpec {
                            id: "combined-tighten".to_owned(),
                            start_month: policy_start,
                            duration_months: 9,
                            kind: PolicyActionKind::PolicyRateDelta { basis_points: 75 },
                        },
                        PolicyActionSpec {
                            id: "combined-transfer".to_owned(),
                            start_month: policy_start,
                            duration_months: 9,
                            kind: PolicyActionKind::TargetedTransfer {
                                amount_per_household: Money(1_500),
                                bottom_share: 0.40,
                            },
                        },
                        PolicyActionSpec {
                            id: "combined-investment".to_owned(),
                            start_month: policy_start,
                            duration_months: 24,
                            kind: PolicyActionKind::PublicInvestment {
                                monthly_amount: Money(150_000),
                                energy_intensity_reduction: 0.003,
                            },
                        },
                    ],
                },
            ],
            replications: ReplicationSpec {
                minimum: 8,
                maximum: 64,
                confidence_level: 0.90,
                relative_half_width_target: 0.05,
            },
            metrics: MetricSpec {
                names: vec![
                    MetricName::Cpi,
                    MetricName::InflationYearOverYear,
                    MetricName::RealGdp,
                    MetricName::NominalGdp,
                    MetricName::IncomeAccounts,
                    MetricName::InstitutionalBalances,
                    MetricName::OutputGap,
                    MetricName::Unemployment,
                    MetricName::AverageWage,
                    MetricName::ConsumptionDeciles,
                    MetricName::Gini,
                    MetricName::PovertyShare,
                    MetricName::Inventories,
                    MetricName::FirmDefaults,
                    MetricName::HouseholdDefaults,
                    MetricName::BankResolutions,
                    MetricName::Credit,
                    MetricName::FiscalBalance,
                    MetricName::GovernmentDebt,
                    MetricName::Imports,
                ],
            },
        }
    }
}

fn validate_identifiers<'a>(
    path: &str,
    identifiers: impl Iterator<Item = &'a str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut seen = BTreeSet::new();
    for identifier in identifiers {
        if identifier.trim().is_empty() {
            issues.push(ValidationIssue::error(
                path,
                "required",
                "identifier cannot be empty",
            ));
        } else if !seen.insert(identifier) {
            issues.push(ValidationIssue::error(
                path,
                "duplicate_identifier",
                format!("duplicate identifier '{identifier}'"),
            ));
        }
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct ValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
    pub severity: IssueSeverity,
}

impl ValidationIssue {
    #[must_use]
    pub fn error(
        path: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            code: code.into(),
            message: message.into(),
            severity: IssueSeverity::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PolicyBranch, ScenarioSpec};
    use crate::{DataBundle, ModelPack};

    #[test]
    fn preregistered_scenario_is_valid() {
        ScenarioSpec::inflation_lab_v0_1().validate().unwrap();
    }

    #[test]
    fn duplicate_branch_is_rejected() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.policy_branches.push(PolicyBranch {
            id: "baseline".to_owned(),
            label: "duplicate".to_owned(),
            actions: Vec::new(),
        });
        let issues = scenario.validate().unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "duplicate_identifier")
        );
    }

    #[test]
    fn overflowing_windows_are_rejected_without_panicking() {
        let mut scenario = ScenarioSpec::inflation_lab_v0_1();
        scenario.shocks[0].start_month = u32::MAX - 2;
        scenario.shocks[0].duration_months = 10;
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn scenario_references_repository_contract_hashes() {
        let scenario = ScenarioSpec::inflation_lab_v0_1();
        let data: DataBundle = serde_json::from_str(include_str!(
            "../../../data/synthetic-balanced-v0.1/data-bundle.json"
        ))
        .unwrap();
        let model: ModelPack = serde_json::from_str(include_str!(
            "../../../model-packs/small-open-economy-v0.1/model-pack.json"
        ))
        .unwrap();
        assert_eq!(scenario.data_bundle.sha256, data.hash().unwrap());
        assert_eq!(scenario.model_pack.sha256, model.hash().unwrap());
    }
}
