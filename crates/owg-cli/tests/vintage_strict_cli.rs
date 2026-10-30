use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("owg-strict-vintage-{}-{stamp}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(manifest_path: &std::path::Path) -> std::process::Output {
    run_kind(manifest_path, "data-vintage-strict")
}

fn run_kind(manifest_path: &std::path::Path, kind: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_owg"))
        .args(["validate", "--kind", kind])
        .arg(manifest_path)
        .arg("--json")
        .output()
        .unwrap()
}

#[test]
fn strict_cli_never_confuses_local_claim_validation_with_origin_admission() {
    let scratch = Scratch::new();
    let data = b"historical source";
    let digest = hex::encode(Sha256::digest(data));
    fs::write(scratch.0.join("source.txt"), data).unwrap();
    fs::write(scratch.0.join("archive.txt"), data).unwrap();
    let manifest_path = scratch.0.join("manifest.json");
    let mut manifest = json!({
        "schema_version": "owg.data-vintage.v2",
        "id": "synthetic-historical-capture",
        "as_of": "2007-01-01T00:00:00Z",
        "geography": "US",
        "currency": "USD",
        "sources": [{
            "id": "example",
            "publisher": "test publisher",
            "source_url": "https://publisher.example/source",
            "release_id": "2006-12",
            "published_at": "2006-12-01T00:00:00Z",
            "retrieved_at": "2026-10-18T00:00:00Z",
            "observation_start": "2006-01-01",
            "observation_end": "2006-09-30",
            "relative_path": "source.txt",
            "sha256": digest,
            "licence": "test only",
            "geography": "US",
            "units": {"value": "USD"},
            "byte_availability": {
                "basis": "independent_capture",
                "archive_provider": "independent test archive",
                "archive_url": "https://archive.example/snapshot",
                "captured_at": "2006-12-02T00:00:00Z",
                "archived_sha256": digest,
                "archive_relative_path": "archive.txt",
                "capture_record_url": "https://archive.example/record"
            }
        }]
    });
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let output = run(&manifest_path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["evidence_claim_validated"], true);
    assert_eq!(result["external_origin_verified"], false);
    assert_eq!(result["historical_input_admitted"], false);

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = Scratch::new();
        let outside_source = outside.0.join("same-source.txt");
        fs::write(&outside_source, data).unwrap();
        fs::remove_file(scratch.0.join("source.txt")).unwrap();
        symlink(&outside_source, scratch.0.join("source.txt")).unwrap();
        let escaped_source = run(&manifest_path);
        assert!(!escaped_source.status.success());
        assert!(
            String::from_utf8_lossy(&escaped_source.stderr).contains("symbolic link"),
            "{}",
            String::from_utf8_lossy(&escaped_source.stderr)
        );

        let mut legacy = manifest.clone();
        legacy["schema_version"] = json!("owg.data-vintage.v1");
        legacy["sources"][0]
            .as_object_mut()
            .unwrap()
            .remove("byte_availability");
        let legacy_path = scratch.0.join("legacy-manifest.json");
        fs::write(&legacy_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let escaped_legacy_source = run_kind(&legacy_path, "data-vintage");
        assert!(!escaped_legacy_source.status.success());
        assert!(
            String::from_utf8_lossy(&escaped_legacy_source.stderr).contains("symbolic link"),
            "{}",
            String::from_utf8_lossy(&escaped_legacy_source.stderr)
        );
        fs::remove_file(scratch.0.join("source.txt")).unwrap();
        fs::write(scratch.0.join("source.txt"), data).unwrap();

        let outside_archive = outside.0.join("same-archive.txt");
        fs::write(&outside_archive, data).unwrap();
        fs::remove_file(scratch.0.join("archive.txt")).unwrap();
        symlink(&outside_archive, scratch.0.join("archive.txt")).unwrap();
        let escaped_archive = run(&manifest_path);
        assert!(!escaped_archive.status.success());
        assert!(
            String::from_utf8_lossy(&escaped_archive.stderr).contains("symbolic link"),
            "{}",
            String::from_utf8_lossy(&escaped_archive.stderr)
        );
        fs::remove_file(scratch.0.join("archive.txt")).unwrap();
        fs::write(scratch.0.join("archive.txt"), data).unwrap();
    }

    manifest["sources"][0]["byte_availability"] = json!({
        "basis": "publication_only",
        "explanation": "exact bytes not independently captured"
    });
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(!run(&manifest_path).status.success());

    manifest["sources"][0]["byte_availability"] = json!({
        "basis": "independent_capture",
        "archive_provider": "independent test archive",
        "archive_url": "https://archive.example/snapshot",
        "captured_at": "2017-03-18T00:00:00Z",
        "archived_sha256": digest,
        "archive_relative_path": "archive.txt",
        "capture_record_url": "https://archive.example/record"
    });
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(!run(&manifest_path).status.success());
}
