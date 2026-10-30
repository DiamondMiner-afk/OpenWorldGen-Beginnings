use crate::contracts::Sector;
use crate::error::{OwgError, OwgResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

pub const DATA_BUNDLE_VERSION: &str = "owg.data-bundle.v1";

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TableFormat {
    Csv,
    Parquet,
    ArrowIpc,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct DataTable {
    pub id: String,
    pub relative_path: String,
    pub format: TableFormat,
    pub sha256: String,
    pub row_count: Option<u64>,
    pub units: BTreeMap<String, String>,
    pub classifications: BTreeMap<String, String>,
    pub geography: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct DataProvenance {
    pub sources: Vec<String>,
    pub licence: String,
    pub retrieval_date: String,
    pub checksum_sha256: String,
    pub confidentiality: String,
    pub transformations: Vec<String>,
    pub imputations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct DataBundle {
    pub schema_version: String,
    pub id: String,
    pub version: String,
    pub currency: String,
    pub base_year: i32,
    pub tables: Vec<DataTable>,
    pub provenance: DataProvenance,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct SectorParameters {
    pub base_price_major: [f64; 4],
    pub energy_intensity: [f64; 4],
    pub consumption_share: [f64; 4],
    pub indirect_tax_rate: [f64; 4],
}

#[derive(Debug, Deserialize)]
struct SectorParameterRecord {
    sector: Sector,
    base_price_major: f64,
    energy_intensity: f64,
    consumption_share: f64,
    indirect_tax_rate: f64,
}

impl DataBundle {
    pub fn validate(&self) -> OwgResult<()> {
        if self.schema_version != DATA_BUNDLE_VERSION {
            return Err(OwgError::InvalidState(format!(
                "unsupported data bundle version {}",
                self.schema_version
            )));
        }
        if self.id.trim().is_empty() || self.version.trim().is_empty() {
            return Err(OwgError::InvalidState(
                "data bundle id and version are required".to_owned(),
            ));
        }
        if self.tables.is_empty() {
            return Err(OwgError::InvalidState(
                "data bundle requires at least one table".to_owned(),
            ));
        }
        let mut ids = BTreeSet::new();
        for table in &self.tables {
            let table_path = Path::new(&table.relative_path);
            if table.id.trim().is_empty()
                || table.relative_path.trim().is_empty()
                || !valid_sha256(&table.sha256)
                || !ids.insert(table.id.as_str())
                || table_path.is_absolute()
                || table_path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(OwgError::InvalidState(format!(
                    "invalid or duplicate data table {}",
                    table.id
                )));
            }
            if table.units.is_empty() {
                return Err(OwgError::InvalidState(format!(
                    "data table {} requires explicit units",
                    table.id
                )));
            }
        }
        if self.provenance.sources.is_empty()
            || self.provenance.licence.trim().is_empty()
            || !valid_sha256(&self.provenance.checksum_sha256)
        {
            return Err(OwgError::InvalidState(
                "data provenance is incomplete".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn hash(&self) -> OwgResult<String> {
        self.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn validate_table_content(&self, table_id: &str, bytes: &[u8]) -> OwgResult<()> {
        let table = self
            .tables
            .iter()
            .find(|table| table.id == table_id)
            .ok_or_else(|| {
                OwgError::InvalidState(format!("data bundle does not declare table {table_id}"))
            })?;
        let actual_hash = hex::encode(Sha256::digest(bytes));
        if actual_hash != table.sha256 {
            return Err(OwgError::InvalidState(format!(
                "{table_id} checksum mismatch: expected {}, found {actual_hash}",
                table.sha256
            )));
        }
        Ok(())
    }

    pub fn sector_parameters_from_csv(&self, bytes: &[u8]) -> OwgResult<SectorParameters> {
        let table = self
            .tables
            .iter()
            .find(|table| table.id == "sector_parameters")
            .ok_or_else(|| {
                OwgError::InvalidState("data bundle does not declare sector_parameters".to_owned())
            })?;
        if table.format != TableFormat::Csv {
            return Err(OwgError::InvalidState(
                "sector_parameters must use CSV format".to_owned(),
            ));
        }
        self.validate_table_content("sector_parameters", bytes)?;

        let mut base_price_major = [0.0; 4];
        let mut energy_intensity = [0.0; 4];
        let mut consumption_share = [0.0; 4];
        let mut indirect_tax_rate = [0.0; 4];
        let mut seen = [false; 4];
        let mut row_count = 0_u64;
        let mut reader = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(bytes);
        for record in reader.deserialize::<SectorParameterRecord>() {
            let record = record?;
            let index = record.sector.index();
            if seen[index] {
                return Err(OwgError::InvalidState(format!(
                    "duplicate sector row for {:?}",
                    record.sector
                )));
            }
            let values = [
                record.base_price_major,
                record.energy_intensity,
                record.consumption_share,
                record.indirect_tax_rate,
            ];
            if values.iter().any(|value| !value.is_finite())
                || record.base_price_major <= 0.0
                || record.energy_intensity < 0.0
                || !(0.0..=1.0).contains(&record.consumption_share)
                || !(0.0..=1.0).contains(&record.indirect_tax_rate)
            {
                return Err(OwgError::InvalidState(format!(
                    "invalid sector parameters for {:?}",
                    record.sector
                )));
            }
            seen[index] = true;
            base_price_major[index] = record.base_price_major;
            energy_intensity[index] = record.energy_intensity;
            consumption_share[index] = record.consumption_share;
            indirect_tax_rate[index] = record.indirect_tax_rate;
            row_count += 1;
        }
        if !seen.into_iter().all(|value| value) {
            return Err(OwgError::InvalidState(
                "sector_parameters must contain every sector exactly once".to_owned(),
            ));
        }
        if table
            .row_count
            .is_some_and(|expected| expected != row_count)
        {
            return Err(OwgError::InvalidState(format!(
                "sector_parameters row count mismatch: expected {}, found {row_count}",
                table.row_count.unwrap_or_default()
            )));
        }
        let share_sum = consumption_share.iter().sum::<f64>();
        if (share_sum - 1.0).abs() > 1e-9 {
            return Err(OwgError::InvalidState(format!(
                "consumption shares must sum to one, found {share_sum:.12}"
            )));
        }

        Ok(SectorParameters {
            base_price_major,
            energy_intensity,
            consumption_share,
            indirect_tax_rate,
        })
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{
        DATA_BUNDLE_VERSION, DataBundle, DataProvenance, DataTable, SectorParameters, TableFormat,
    };
    use std::collections::BTreeMap;

    #[test]
    fn provenance_and_units_are_mandatory() {
        let bundle = DataBundle {
            schema_version: DATA_BUNDLE_VERSION.to_owned(),
            id: "synthetic".to_owned(),
            version: "0.1.0".to_owned(),
            currency: "OWG".to_owned(),
            base_year: 2020,
            tables: vec![DataTable {
                id: "prices".to_owned(),
                relative_path: "prices.parquet".to_owned(),
                format: TableFormat::Parquet,
                sha256: "0".repeat(64),
                row_count: Some(1),
                units: BTreeMap::from([("cpi".to_owned(), "index_2020_100".to_owned())]),
                classifications: BTreeMap::new(),
                geography: "SYNTHETICA".to_owned(),
            }],
            provenance: DataProvenance {
                sources: vec!["synthetic generator".to_owned()],
                licence: "internal-research".to_owned(),
                retrieval_date: "not-applicable".to_owned(),
                checksum_sha256: "1".repeat(64),
                confidentiality: "public-synthetic".to_owned(),
                transformations: Vec::new(),
                imputations: Vec::new(),
            },
        };
        assert!(bundle.validate().is_ok());
        assert_eq!(bundle.hash().unwrap().len(), 64);
    }

    #[test]
    fn repository_synthetic_bundle_matches_contract() {
        let bundle: DataBundle = serde_json::from_str(include_str!(
            "../../../data/synthetic-balanced-v0.1/data-bundle.json"
        ))
        .unwrap();
        bundle.validate().unwrap();
        let sectors = bundle
            .sector_parameters_from_csv(include_bytes!(
                "../../../data/synthetic-balanced-v0.1/sector_parameters.csv"
            ))
            .unwrap();
        assert_eq!(
            sectors,
            SectorParameters {
                base_price_major: [16.0, 12.0, 24.0, 20.0],
                energy_intensity: [0.30, 0.16, 0.24, 0.07],
                consumption_share: [0.10, 0.30, 0.25, 0.35],
                indirect_tax_rate: [0.08, 0.05, 0.10, 0.12],
            }
        );
    }

    #[test]
    fn sector_table_checksum_is_enforced() {
        let bundle: DataBundle = serde_json::from_str(include_str!(
            "../../../data/synthetic-balanced-v0.1/data-bundle.json"
        ))
        .unwrap();
        assert!(
            bundle
                .sector_parameters_from_csv(b"sector,base_price_major\nenergy,1.0\n")
                .is_err()
        );
    }

    #[test]
    fn data_table_paths_cannot_escape_the_bundle() {
        let mut bundle: DataBundle = serde_json::from_str(include_str!(
            "../../../data/synthetic-balanced-v0.1/data-bundle.json"
        ))
        .unwrap();
        bundle.tables[0].relative_path = "../secret.csv".to_owned();
        assert!(bundle.validate().is_err());
    }
}
