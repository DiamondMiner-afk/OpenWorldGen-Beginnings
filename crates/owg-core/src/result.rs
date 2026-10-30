use crate::error::{OwgError, OwgResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

pub const RESULT_BUNDLE_VERSION: &str = "owg.result-bundle.v2";
const LEGACY_RESULT_BUNDLE_VERSION: &str = "owg.result-bundle.v1";

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct MetricFiles {
    pub parquet: String,
    pub csv: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct ResultBundleContract {
    pub package_version: String,
    pub manifest: String,
    pub metrics: MetricFiles,
    pub comparison: String,
    pub mechanisms: String,
    #[serde(default)]
    pub accounting: String,
    pub journal_hashes: String,
    pub event_audit: String,
    pub report: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visualization: Option<String>,
    pub files: BTreeMap<String, String>,
}

impl ResultBundleContract {
    pub fn validate(&self) -> OwgResult<()> {
        if self.package_version != RESULT_BUNDLE_VERSION
            && self.package_version != LEGACY_RESULT_BUNDLE_VERSION
        {
            return Err(OwgError::InvalidState(format!(
                "unsupported result bundle version {}",
                self.package_version
            )));
        }
        if self.package_version == RESULT_BUNDLE_VERSION && self.accounting.trim().is_empty() {
            return Err(OwgError::InvalidState(
                "result bundle v2 requires accounting diagnostics".to_owned(),
            ));
        }
        let mut declared_paths = vec![
            self.manifest.as_str(),
            self.metrics.parquet.as_str(),
            self.metrics.csv.as_str(),
            self.comparison.as_str(),
            self.mechanisms.as_str(),
            self.journal_hashes.as_str(),
            self.event_audit.as_str(),
            self.report.as_str(),
        ];
        if !self.accounting.trim().is_empty() {
            declared_paths.push(self.accounting.as_str());
        }
        if let Some(visualization) = &self.visualization {
            declared_paths.push(visualization.as_str());
        }
        let unique: BTreeSet<&str> = declared_paths.iter().copied().collect();
        if unique.len() != declared_paths.len() {
            return Err(OwgError::InvalidState(
                "result bundle contains duplicate artifact paths".to_owned(),
            ));
        }
        for relative in declared_paths {
            if !safe_relative_path(relative) {
                return Err(OwgError::InvalidState(format!(
                    "unsafe result-bundle path {relative}"
                )));
            }
            let checksum = self.files.get(relative).ok_or_else(|| {
                OwgError::InvalidState(format!("missing checksum for result artifact {relative}"))
            })?;
            if !valid_sha256(checksum) {
                return Err(OwgError::InvalidState(format!(
                    "invalid checksum for result artifact {relative}"
                )));
            }
        }
        for (relative, checksum) in &self.files {
            if !safe_relative_path(relative) || !valid_sha256(checksum) {
                return Err(OwgError::InvalidState(format!(
                    "invalid result-bundle file declaration {relative}"
                )));
            }
        }
        Ok(())
    }
}

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.trim().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{MetricFiles, RESULT_BUNDLE_VERSION, ResultBundleContract};
    use std::collections::BTreeMap;

    fn bundle() -> ResultBundleContract {
        let names = [
            "manifest.json",
            "metrics.parquet",
            "metrics.csv",
            "comparison.json",
            "mechanisms.json",
            "accounting.json",
            "journal_hashes.json",
            "event_audit.json",
            "report.md",
            "plots.html",
        ];
        ResultBundleContract {
            package_version: RESULT_BUNDLE_VERSION.to_owned(),
            manifest: names[0].to_owned(),
            metrics: MetricFiles {
                parquet: names[1].to_owned(),
                csv: names[2].to_owned(),
            },
            comparison: names[3].to_owned(),
            mechanisms: names[4].to_owned(),
            accounting: names[5].to_owned(),
            journal_hashes: names[6].to_owned(),
            event_audit: names[7].to_owned(),
            report: names[8].to_owned(),
            visualization: Some(names[9].to_owned()),
            files: names
                .into_iter()
                .map(|name| (name.to_owned(), "a".repeat(64)))
                .collect(),
        }
    }

    #[test]
    fn complete_bundle_contract_is_valid() {
        bundle().validate().unwrap();
    }

    #[test]
    fn legacy_v1_bundle_without_accounting_remains_valid() {
        let mut value = bundle();
        value.package_version = "owg.result-bundle.v1".to_owned();
        value.files.remove(&value.accounting);
        value.accounting.clear();
        if let Some(visualization) = value.visualization.take() {
            value.files.remove(&visualization);
        }
        value.validate().unwrap();
    }

    #[test]
    fn current_v2_bundle_without_visualization_remains_valid() {
        let mut value = bundle();
        if let Some(visualization) = value.visualization.take() {
            value.files.remove(&visualization);
        }
        value.validate().unwrap();
    }

    #[test]
    fn bundle_contract_rejects_path_traversal() {
        let mut value = bundle();
        value.report = "../report.md".to_owned();
        value
            .files
            .insert("../report.md".to_owned(), "b".repeat(64));
        assert!(value.validate().is_err());
    }

    #[test]
    fn bundle_contract_rejects_missing_checksum() {
        let mut value = bundle();
        value.files = BTreeMap::new();
        assert!(value.validate().is_err());
    }
}
