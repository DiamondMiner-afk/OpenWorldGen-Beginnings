//! Release-vintage evidence for historical, no-lookahead experiments.

use crate::error::{OwgError, OwgResult};
use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

pub const DATA_VINTAGE_VERSION: &str = "owg.data-vintage.v1";
pub const DATA_VINTAGE_STRICT_VERSION: &str = "owg.data-vintage.v2";

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct SourceSnapshot {
    pub id: String,
    pub publisher: String,
    pub source_url: String,
    pub release_id: String,
    pub published_at: DateTime<Utc>,
    pub retrieved_at: DateTime<Utc>,
    pub observation_start: NaiveDate,
    pub observation_end: NaiveDate,
    pub relative_path: String,
    pub sha256: String,
    pub licence: String,
    pub geography: String,
    pub units: BTreeMap<String, String>,
    #[serde(default)]
    pub classifications: BTreeMap<String, String>,
    #[serde(default)]
    pub transformations: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct DataVintageManifest {
    pub schema_version: String,
    pub id: String,
    /// The latest publication time permitted in a historical run.
    pub as_of: DateTime<Utc>,
    pub geography: String,
    pub currency: String,
    pub sources: Vec<SourceSnapshot>,
}

impl DataVintageManifest {
    pub fn validate(&self) -> OwgResult<()> {
        if self.schema_version != DATA_VINTAGE_VERSION {
            return Err(OwgError::InvalidState(format!(
                "unsupported data vintage version {}",
                self.schema_version
            )));
        }
        if self.id.trim().is_empty()
            || self.geography.trim().is_empty()
            || self.currency.trim().is_empty()
            || self.sources.is_empty()
        {
            return Err(OwgError::InvalidState(
                "data vintage requires id, geography, currency, and sources".to_owned(),
            ));
        }
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for source in &self.sources {
            if source.id.trim().is_empty()
                || !ids.insert(source.id.as_str())
                || !paths.insert(source.relative_path.as_str())
                || source.publisher.trim().is_empty()
                || source.source_url.trim().is_empty()
                || source.release_id.trim().is_empty()
                || source.licence.trim().is_empty()
                || source.geography.trim().is_empty()
                || source.units.is_empty()
                || source
                    .units
                    .iter()
                    .any(|(name, unit)| name.trim().is_empty() || unit.trim().is_empty())
                || !safe_relative_path(&source.relative_path)
                || !valid_sha256(&source.sha256)
                || source.published_at > self.as_of
                || source.retrieved_at < source.published_at
                || source.observation_start > source.observation_end
                || source.observation_end > source.published_at.date_naive()
            {
                return Err(OwgError::InvalidState(format!(
                    "invalid, duplicated, or future source snapshot {}",
                    source.id
                )));
            }
        }
        Ok(())
    }

    pub fn hash(&self) -> OwgResult<String> {
        self.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn validate_source_content(&self, source_id: &str, bytes: &[u8]) -> OwgResult<()> {
        let source = self
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .ok_or_else(|| OwgError::InvalidState(format!("unknown vintage source {source_id}")))?;
        let actual = hex::encode(Sha256::digest(bytes));
        if actual != source.sha256 {
            return Err(OwgError::InvalidState(format!(
                "vintage source {source_id} checksum mismatch: expected {}, found {actual}",
                source.sha256
            )));
        }
        Ok(())
    }
}

/// Publication timing and exact-byte timing are different claims. A later
/// retrieval of a release-labelled file is not proof of its historical bytes.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(tag = "basis", rename_all = "snake_case")]
pub enum HistoricalByteAvailability {
    /// The release was known by the cutoff, but its surviving bytes are not
    /// independently evidenced at that cutoff. Never admissible as input.
    PublicationOnly { explanation: String },
    /// A separately archived copy of the exact source bytes, captured by a
    /// third party no later than the historical cutoff.
    IndependentCapture {
        archive_provider: String,
        archive_url: String,
        captured_at: DateTime<Utc>,
        archived_sha256: String,
        archive_relative_path: String,
        /// Human-auditable reference to the archive's original capture record.
        capture_record_url: String,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct HistoricalSourceSnapshot {
    #[serde(flatten)]
    pub source: SourceSnapshot,
    pub byte_availability: HistoricalByteAvailability,
}

/// Strict, opt-in historical input contract. V1 manifests remain valid for
/// legacy research but cannot be promoted implicitly to this contract.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct StrictDataVintageManifest {
    pub schema_version: String,
    pub id: String,
    pub as_of: DateTime<Utc>,
    pub geography: String,
    pub currency: String,
    pub sources: Vec<HistoricalSourceSnapshot>,
}

/// The trusted caller supplies an implementation that independently checks
/// archive-origin metadata and content. A manifest, URL, or local file alone
/// cannot authenticate the claimed historical capture time.
pub trait CaptureOriginVerifier {
    fn verify_capture(
        &self,
        source: &HistoricalSourceSnapshot,
        archive_bytes: &[u8],
    ) -> OwgResult<()>;
}

impl StrictDataVintageManifest {
    /// Validate claims and paths. This does not authenticate a third-party
    /// archive's identity or capture date; that remains a provenance review.
    pub fn validate(&self) -> OwgResult<()> {
        if self.schema_version != DATA_VINTAGE_STRICT_VERSION {
            return Err(OwgError::InvalidState(format!(
                "unsupported strict data vintage version {}",
                self.schema_version
            )));
        }
        let legacy = DataVintageManifest {
            schema_version: DATA_VINTAGE_VERSION.to_owned(),
            id: self.id.clone(),
            as_of: self.as_of,
            geography: self.geography.clone(),
            currency: self.currency.clone(),
            sources: self
                .sources
                .iter()
                .map(|item| item.source.clone())
                .collect(),
        };
        legacy.validate()?;
        let mut capture_paths = BTreeSet::new();
        for item in &self.sources {
            match &item.byte_availability {
                HistoricalByteAvailability::PublicationOnly { explanation } => {
                    if explanation.trim().is_empty() {
                        return Err(OwgError::InvalidState(format!(
                            "publication-only source {} requires an explanation",
                            item.source.id
                        )));
                    }
                }
                HistoricalByteAvailability::IndependentCapture {
                    archive_provider,
                    archive_url,
                    captured_at,
                    archived_sha256,
                    archive_relative_path,
                    capture_record_url,
                } => {
                    if archive_provider.trim().is_empty()
                        || archive_provider == &item.source.publisher
                        || !archive_url.starts_with("https://")
                        || archive_url == &item.source.source_url
                        || !capture_record_url.starts_with("https://")
                        || !valid_sha256(archived_sha256)
                        || archived_sha256 != &item.source.sha256
                        || *captured_at < item.source.published_at
                        || *captured_at > self.as_of
                        || *captured_at > item.source.retrieved_at
                        || !safe_relative_path(archive_relative_path)
                        || archive_relative_path == &item.source.relative_path
                        || !capture_paths.insert(archive_relative_path.as_str())
                        || self
                            .sources
                            .iter()
                            .any(|other| archive_relative_path == &other.source.relative_path)
                    {
                        return Err(OwgError::InvalidState(format!(
                            "invalid exact-byte capture for source {}",
                            item.source.id
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// Check that a *claimed* capture is internally consistent with local
    /// copies. This is not origin authentication or historical admission.
    pub fn validate_capture_claim(
        &self,
        source_id: &str,
        source_bytes: &[u8],
        archive_bytes: &[u8],
    ) -> OwgResult<()> {
        self.validate()?;
        let item = self
            .sources
            .iter()
            .find(|item| item.source.id == source_id)
            .ok_or_else(|| OwgError::InvalidState(format!("unknown vintage source {source_id}")))?;
        let HistoricalByteAvailability::IndependentCapture {
            archived_sha256, ..
        } = &item.byte_availability
        else {
            return Err(OwgError::InvalidState(format!(
                "source {source_id} has publication-level evidence only; exact historical bytes are not admissible"
            )));
        };
        let source_hash = hex::encode(Sha256::digest(source_bytes));
        let archive_hash = hex::encode(Sha256::digest(archive_bytes));
        if source_hash != item.source.sha256 || archive_hash != *archived_sha256 {
            return Err(OwgError::InvalidState(format!(
                "source {source_id} exact-byte capture checksum mismatch"
            )));
        }
        Ok(())
    }

    /// Historical input admission additionally requires an independently
    /// implemented origin verifier. The CLI currently has no such verifier,
    /// so it never promotes a self-declared capture claim to admitted input.
    pub fn admit_exact_source<V: CaptureOriginVerifier>(
        &self,
        source_id: &str,
        source_bytes: &[u8],
        archive_bytes: &[u8],
        verifier: &V,
    ) -> OwgResult<()> {
        self.validate_capture_claim(source_id, source_bytes, archive_bytes)?;
        let source = self
            .sources
            .iter()
            .find(|item| item.source.id == source_id)
            .ok_or_else(|| OwgError::InvalidState(format!("unknown vintage source {source_id}")))?;
        verifier.verify_capture(source, archive_bytes)
    }

    pub fn hash(&self) -> OwgResult<String> {
        self.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }
}

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.trim().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Read a manifest-relative source while rejecting symlink components and
/// resolved paths outside the manifest directory. This is a local-file check,
/// not historical-origin authentication or a race-proof defense against a
/// concurrent process replacing filesystem entries during validation. The
/// caller must still validate the digest and, for v2, archive provenance.
pub fn read_confined_vintage_file(directory: &Path, relative_path: &str) -> OwgResult<Vec<u8>> {
    if !safe_relative_path(relative_path) {
        return Err(OwgError::InvalidState(format!(
            "unsafe vintage file path {relative_path}"
        )));
    }
    let root = fs::canonicalize(directory)?;
    if !root.is_dir() {
        return Err(OwgError::InvalidState(format!(
            "vintage directory is not a directory: {}",
            directory.display()
        )));
    }
    let mut path = root.clone();
    for component in Path::new(relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(OwgError::InvalidState(format!(
                "unsafe vintage file path {relative_path}"
            )));
        };
        path.push(name);
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(OwgError::InvalidState(format!(
                "vintage file path contains a symbolic link: {relative_path}"
            )));
        }
    }
    let resolved = fs::canonicalize(&path)?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err(OwgError::InvalidState(format!(
            "vintage file path escapes its directory or is not a file: {relative_path}"
        )));
    }
    Ok(fs::read(resolved)?)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureOriginVerifier, DATA_VINTAGE_STRICT_VERSION, DATA_VINTAGE_VERSION,
        DataVintageManifest, HistoricalByteAvailability, HistoricalSourceSnapshot, SourceSnapshot,
        StrictDataVintageManifest, read_confined_vintage_file,
    };
    use crate::error::{OwgError, OwgResult};
    use chrono::{DateTime, NaiveDate, Utc};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "owg-vintage-confined-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> DataVintageManifest {
        let content = b"historical snapshot";
        DataVintageManifest {
            schema_version: DATA_VINTAGE_VERSION.to_owned(),
            id: "us-2007-origin".to_owned(),
            as_of: "2007-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            geography: "US".to_owned(),
            currency: "USD".to_owned(),
            sources: vec![SourceSnapshot {
                id: "example".to_owned(),
                publisher: "example publisher".to_owned(),
                source_url: "https://example.org/release".to_owned(),
                release_id: "2006-q3".to_owned(),
                published_at: "2006-12-01T00:00:00Z".parse().unwrap(),
                retrieved_at: "2026-10-14T00:00:00Z".parse().unwrap(),
                observation_start: NaiveDate::from_ymd_opt(2006, 7, 1).unwrap(),
                observation_end: NaiveDate::from_ymd_opt(2006, 9, 30).unwrap(),
                relative_path: "data/example.csv".to_owned(),
                sha256: hex::encode(Sha256::digest(content)),
                licence: "test only".to_owned(),
                geography: "US".to_owned(),
                units: BTreeMap::from([("value".to_owned(), "USD".to_owned())]),
                classifications: BTreeMap::new(),
                transformations: vec![],
                limitations: vec![],
            }],
        }
    }

    #[test]
    fn accepts_a_frozen_release_and_checks_content() {
        let manifest = fixture();
        manifest.validate().unwrap();
        manifest
            .validate_source_content("example", b"historical snapshot")
            .unwrap();
        assert!(
            manifest
                .validate_source_content("example", b"revised")
                .is_err()
        );
        assert_eq!(manifest.hash().unwrap().len(), 64);
    }

    #[test]
    fn rejects_lookahead_and_unsafe_files() {
        let mut manifest = fixture();
        manifest.sources[0].published_at = "2007-02-01T00:00:00Z".parse().unwrap();
        assert!(manifest.validate().is_err());
        manifest = fixture();
        manifest.sources[0].relative_path = "../later.csv".to_owned();
        assert!(manifest.validate().is_err());
        manifest = fixture();
        manifest.sources.push(manifest.sources[0].clone());
        assert!(manifest.validate().is_err());
    }

    fn strict_fixture() -> StrictDataVintageManifest {
        let legacy = fixture();
        StrictDataVintageManifest {
            schema_version: DATA_VINTAGE_STRICT_VERSION.to_owned(),
            id: legacy.id,
            as_of: legacy.as_of,
            geography: legacy.geography,
            currency: legacy.currency,
            sources: legacy
                .sources
                .into_iter()
                .map(|source| HistoricalSourceSnapshot {
                    source,
                    byte_availability: HistoricalByteAvailability::IndependentCapture {
                        archive_provider: "independent archive".to_owned(),
                        archive_url: "https://archive.example.net/snapshot/20061202".to_owned(),
                        captured_at: "2006-12-02T00:00:00Z".parse().unwrap(),
                        archived_sha256: hex::encode(Sha256::digest(b"historical snapshot")),
                        archive_relative_path: "archive/example.csv".to_owned(),
                        capture_record_url: "https://archive.example.net/record/20061202"
                            .to_owned(),
                    },
                })
                .collect(),
        }
    }

    struct FixtureVerifier;

    impl CaptureOriginVerifier for FixtureVerifier {
        fn verify_capture(
            &self,
            source: &HistoricalSourceSnapshot,
            archive_bytes: &[u8],
        ) -> OwgResult<()> {
            let HistoricalByteAvailability::IndependentCapture {
                archive_url,
                captured_at,
                ..
            } = &source.byte_availability
            else {
                return Err(OwgError::InvalidState("not an archival capture".to_owned()));
            };
            if archive_url != "https://archive.example.net/snapshot/20061202"
                || *captured_at != "2006-12-02T00:00:00Z".parse::<DateTime<Utc>>().unwrap()
                || archive_bytes != b"historical snapshot"
            {
                return Err(OwgError::InvalidState(
                    "origin independently failed fixture verification".to_owned(),
                ));
            }
            Ok(())
        }
    }

    struct RejectingVerifier;

    impl CaptureOriginVerifier for RejectingVerifier {
        fn verify_capture(
            &self,
            _source: &HistoricalSourceSnapshot,
            _archive_bytes: &[u8],
        ) -> OwgResult<()> {
            Err(OwgError::InvalidState(
                "capture origin could not be independently checked".to_owned(),
            ))
        }
    }

    #[test]
    fn strict_manifest_admits_only_after_origin_verification() {
        let manifest = strict_fixture();
        manifest.validate().unwrap();
        manifest
            .validate_capture_claim("example", b"historical snapshot", b"historical snapshot")
            .unwrap();
        assert!(
            manifest
                .admit_exact_source(
                    "example",
                    b"historical snapshot",
                    b"historical snapshot",
                    &RejectingVerifier,
                )
                .is_err()
        );
        manifest
            .admit_exact_source(
                "example",
                b"historical snapshot",
                b"historical snapshot",
                &FixtureVerifier,
            )
            .unwrap();
        assert_eq!(manifest.hash().unwrap().len(), 64);
        assert!(
            manifest
                .validate_capture_claim("example", b"revised", b"historical snapshot")
                .is_err()
        );
        assert!(
            manifest
                .validate_capture_claim("example", b"historical snapshot", b"revised")
                .is_err()
        );
    }

    #[test]
    fn publication_level_source_cannot_be_promoted_to_historical_input() {
        let mut manifest = strict_fixture();
        manifest.sources[0].byte_availability = HistoricalByteAvailability::PublicationOnly {
            explanation: "PDF was published in 2006, but surviving bytes changed in 2017"
                .to_owned(),
        };
        manifest.validate().unwrap();
        let error = manifest
            .validate_capture_claim("example", b"historical snapshot", b"historical snapshot")
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("publication-level evidence only")
        );
    }

    #[test]
    fn later_capture_and_self_archive_do_not_prove_origin_bytes() {
        let mut manifest = strict_fixture();
        if let HistoricalByteAvailability::IndependentCapture { captured_at, .. } =
            &mut manifest.sources[0].byte_availability
        {
            *captured_at = "2017-03-18T00:00:00Z".parse().unwrap();
        }
        assert!(manifest.validate().is_err());

        let mut manifest = strict_fixture();
        let publisher = manifest.sources[0].source.publisher.clone();
        if let HistoricalByteAvailability::IndependentCapture {
            archive_provider, ..
        } = &mut manifest.sources[0].byte_availability
        {
            *archive_provider = publisher;
        }
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn strict_manifest_rejects_mismatched_digest_or_unsafe_archive_path() {
        let mut manifest = strict_fixture();
        if let HistoricalByteAvailability::IndependentCapture {
            archived_sha256, ..
        } = &mut manifest.sources[0].byte_availability
        {
            *archived_sha256 = hex::encode(Sha256::digest(b"later PDF"));
        }
        assert!(manifest.validate().is_err());

        let mut manifest = strict_fixture();
        if let HistoricalByteAvailability::IndependentCapture {
            archive_relative_path,
            ..
        } = &mut manifest.sources[0].byte_availability
        {
            *archive_relative_path = "../untrusted.pdf".to_owned();
        }
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn legacy_v1_manifest_does_not_deserialize_as_strict_v2() {
        let encoded = serde_json::to_vec(&fixture()).unwrap();
        assert!(serde_json::from_slice::<StrictDataVintageManifest>(&encoded).is_err());
    }

    #[test]
    fn confined_reader_rejects_traversal_and_reads_regular_files() {
        let scratch = Scratch::new();
        let root = scratch.0.join("manifest");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("source.txt"), b"expected bytes").unwrap();
        fs::write(scratch.0.join("outside.txt"), b"private bytes").unwrap();
        assert_eq!(
            read_confined_vintage_file(&root, "source.txt").unwrap(),
            b"expected bytes"
        );
        assert!(read_confined_vintage_file(&root, "../outside.txt").is_err());
        assert!(read_confined_vintage_file(&root, "/etc/passwd").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn confined_reader_rejects_file_and_parent_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let scratch = Scratch::new();
        let root = scratch.0.join("manifest");
        let outside = scratch.0.join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("source.txt"), b"private bytes").unwrap();
        symlink(outside.join("source.txt"), root.join("source.txt")).unwrap();
        symlink(&outside, root.join("linked-directory")).unwrap();

        let file_error = read_confined_vintage_file(&root, "source.txt")
            .unwrap_err()
            .to_string();
        assert!(file_error.contains("symbolic link"), "{file_error}");
        let parent_error = read_confined_vintage_file(&root, "linked-directory/source.txt")
            .unwrap_err()
            .to_string();
        assert!(parent_error.contains("symbolic link"), "{parent_error}");
    }
}
