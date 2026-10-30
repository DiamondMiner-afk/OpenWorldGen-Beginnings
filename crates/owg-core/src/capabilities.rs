use crate::{
    CHECKPOINT_VERSION, CONTRACT_VERSION, ENGINE_VERSION, RESULT_BUNDLE_VERSION, SCHEDULER_VERSION,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct EngineCapabilities {
    pub capabilities_version: String,
    pub engine_version: String,
    pub scenario_contract_version: String,
    pub checkpoint_version: String,
    pub result_bundle_version: String,
    pub scheduler_version: String,
    pub gpu_required: bool,
    pub network_required: bool,
    pub execution_modes: Vec<String>,
    pub compute_profiles: Vec<String>,
    pub agent_archetypes: Vec<String>,
    pub markets: Vec<String>,
    pub policy_kinds: Vec<String>,
    pub result_formats: Vec<String>,
    pub cli_commands: Vec<String>,
    pub python_sdk_methods: Vec<String>,
    pub implemented_systems: Vec<String>,
    pub gated_systems: Vec<String>,
}

impl EngineCapabilities {
    #[must_use]
    pub fn current() -> Self {
        Self {
            capabilities_version: "owg.capabilities.v1".to_owned(),
            engine_version: ENGINE_VERSION.to_owned(),
            scenario_contract_version: CONTRACT_VERSION.to_owned(),
            checkpoint_version: CHECKPOINT_VERSION.to_owned(),
            result_bundle_version: RESULT_BUNDLE_VERSION.to_owned(),
            scheduler_version: SCHEDULER_VERSION.to_owned(),
            gpu_required: false,
            network_required: false,
            execution_modes: vec!["reference".to_owned(), "fast".to_owned()],
            compute_profiles: vec![
                "minimal".to_owned(),
                "standard".to_owned(),
                "custom".to_owned(),
            ],
            agent_archetypes: vec![
                "household".to_owned(),
                "firm".to_owned(),
                "bank".to_owned(),
                "government".to_owned(),
                "central_bank".to_owned(),
                "rest_of_world".to_owned(),
            ],
            markets: vec![
                "sector_goods".to_owned(),
                "labor".to_owned(),
                "deposits".to_owned(),
                "credit".to_owned(),
                "government_debt".to_owned(),
            ],
            policy_kinds: vec![
                "rule".to_owned(),
                "utility".to_owned(),
                "reinforcement_learning_contract".to_owned(),
                "large_language_model_contract".to_owned(),
                "human_contract".to_owned(),
            ],
            result_formats: vec![
                "json".to_owned(),
                "csv".to_owned(),
                "parquet".to_owned(),
                "markdown".to_owned(),
                "self_contained_html".to_owned(),
            ],
            cli_commands: vec![
                "capabilities".to_owned(),
                "validate".to_owned(),
                "run".to_owned(),
                "compare".to_owned(),
                "checkpoint".to_owned(),
                "resume".to_owned(),
                "calibrate".to_owned(),
                "benchmark".to_owned(),
                "report".to_owned(),
                "plot".to_owned(),
                "init-scenario".to_owned(),
                "observe-firm".to_owned(),
                "run-recorded-firm".to_owned(),
                "schema".to_owned(),
            ],
            python_sdk_methods: vec![
                "capabilities".to_owned(),
                "validate".to_owned(),
                "verify_bundle".to_owned(),
                "init_scenario".to_owned(),
                "write_schema".to_owned(),
                "run".to_owned(),
                "compare".to_owned(),
                "checkpoint".to_owned(),
                "resume".to_owned(),
                "calibrate".to_owned(),
                "benchmark".to_owned(),
                "report".to_owned(),
                "plot".to_owned(),
            ],
            implemented_systems: vec![
                "optional_scheduled_bank_customer_credit_contracts".to_owned(),
                "bounded_recorded_firm_cli_experiment".to_owned(),
                "typed_multi_rate_scheduler".to_owned(),
                "named_random_streams".to_owned(),
                "fixed_point_double_entry_ledger".to_owned(),
                "institutional_balance_aggregation".to_owned(),
                "production_expenditure_income_accounts".to_owned(),
                "household_behavior_and_learning".to_owned(),
                "firm_pricing_production_inventory_and_entry".to_owned(),
                "bank_credit_defaults_and_resolution".to_owned(),
                "fiscal_and_monetary_policy".to_owned(),
                "rest_of_world_trade_boundary".to_owned(),
                "checkpoint_resume".to_owned(),
                "monte_carlo_policy_comparison".to_owned(),
                "checksum_verified_result_bundles".to_owned(),
                "interactive_result_graphs".to_owned(),
            ],
            gated_systems: vec![
                "empirical_india_calibration".to_owned(),
                "simulation_based_inference".to_owned(),
                "trained_rl_policies".to_owned(),
                "llm_runtime_adapters".to_owned(),
                "grpc_server".to_owned(),
                "minecraft_fabric_adapter".to_owned(),
                "multi_region_and_multi_country".to_owned(),
                "endogenous_fx_and_asset_markets".to_owned(),
                "climate_and_geopolitics".to_owned(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EngineCapabilities;

    #[test]
    fn current_capabilities_are_local_first_and_versioned() {
        let capabilities = EngineCapabilities::current();
        assert_eq!(capabilities.capabilities_version, "owg.capabilities.v1");
        assert!(!capabilities.gpu_required);
        assert!(!capabilities.network_required);
        assert!(
            capabilities
                .implemented_systems
                .contains(&"typed_multi_rate_scheduler".to_owned())
        );
        assert!(
            capabilities
                .gated_systems
                .contains(&"minecraft_fabric_adapter".to_owned())
        );
    }

    #[test]
    fn human_catalog_mentions_every_public_command_and_sdk_method() {
        let capabilities = EngineCapabilities::current();
        let catalog = include_str!("../../../docs/SYSTEMS_FUNCTIONS_AND_DECISIONS.md");
        for name in capabilities
            .cli_commands
            .iter()
            .chain(&capabilities.python_sdk_methods)
        {
            assert!(catalog.contains(name), "catalog is missing {name}");
        }
    }
}
