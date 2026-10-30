use anyhow::{Context, Result, bail};
use owg_core::ScenarioSpec;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::Path;

pub fn load_scenario(path: &Path) -> Result<ScenarioSpec> {
    let file =
        File::open(path).with_context(|| format!("failed to open scenario {}", path.display()))?;
    let reader = BufReader::new(file);
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::from_reader(reader)
            .with_context(|| format!("invalid JSON scenario {}", path.display())),
        Some("yaml" | "yml") => serde_yaml_ng::from_reader(reader)
            .with_context(|| format!("invalid YAML scenario {}", path.display())),
        extension => {
            bail!("unsupported scenario extension {extension:?}; use .json, .yaml, or .yml")
        }
    }
}

pub fn save_scenario(path: &Path, scenario: &ScenarioSpec) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let writer = BufWriter::new(File::create(path)?);
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::to_writer_pretty(writer, scenario)?,
        Some("yaml" | "yml") => serde_yaml_ng::to_writer(writer, scenario)?,
        extension => {
            bail!("unsupported scenario extension {extension:?}; use .json, .yaml, or .yml")
        }
    }
    Ok(())
}
