#!/usr/bin/env python3
"""Non-identifying QC of the MatrAIx Persona 1M packed Parquet release.

Run with the Python SDK analysis extra installed (pyarrow). The output contains
aggregate counts only; it never exports persona text or source record IDs.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
from typing import Any

try:
    import pyarrow.parquet as pq
except ModuleNotFoundError:  # Decoder unit tests do not require this analysis extra.
    pq = None


DATA_ROOT = Path(__file__).resolve().parents[1] / "data/raw/matraix-persona-1m"
PROFILE_FIELDS = (
    "age_bracket",
    "region",
    "gender_identity",
    "urbanicity",
    "socioeconomic_band",
    "risk_tolerance",
    "att_taking_on_debt",
    "pref_save_vs_spend",
    "demo_household_income",
    "demo_employment_status",
    "dospert_financial_risk_tolerance",
)
READ_COLUMNS = (
    "source",
    "source_row_index",
    "attributes",
    "null_bitmap",
    "attribute_overrides",
    "populated_attribute_count",
    "has_description",
    "description_count",
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def decode_field(attributes: bytes, missing_bitmap: bytes | None,
                 overrides: list[dict[str, Any]] | None, field_index: int,
                 values: list[str]) -> str | None:
    """Decode one codebook field, honoring missing bits and exact overrides."""
    if missing_bitmap is not None and (missing_bitmap[field_index // 8] >> (field_index % 8)) & 1:
        return None
    if overrides:
        for override in overrides:
            if override["field_index"] == field_index:
                return override["value"]
    packed = attributes[field_index // 2]
    code = (packed & 0x0F) if field_index % 2 == 0 else (packed >> 4)
    return values[code] if code < len(values) else None


def audit(root: Path, verify_hashes: bool = False, profile: bool = False) -> dict[str, Any]:
    if pq is None:
        raise RuntimeError("MatrAIx Parquet audit requires pyarrow; install the SDK analysis extra")
    manifest = json.loads((root / "manifest.json").read_text())
    codebook = json.loads((root / "persona_codes.schema.json").read_text())
    columns = codebook["columns"]
    field_ids = [col["id"] for col in columns]
    field_map = {name: (field_ids.index(name), columns[field_ids.index(name)]["values"])
                 for name in PROFILE_FIELDS}
    errors: list[str] = []
    warnings: list[str] = []
    if len(columns) != 1290 or len(field_ids) != len(set(field_ids)):
        errors.append("codebook must contain 1,290 unique field IDs")
    if codebook.get("packing") != "nibble" or codebook.get("row_bytes") != 645:
        errors.append("unexpected packed attribute format")

    shard_reports = []
    total_rows = 0
    source_counts: Counter[str] = Counter()
    source_populated: Counter[str] = Counter()
    source_description: Counter[str] = Counter()
    field_counts: dict[str, Counter[str]] = {field: Counter() for field in PROFILE_FIELDS}
    field_nonmissing_by_source: dict[str, Counter[str]] = defaultdict(Counter)
    field_canonical_by_source: dict[str, Counter[str]] = defaultdict(Counter)
    field_noncanonical: Counter[str] = Counter()
    field_sentinel: Counter[str] = Counter()
    seen_keys: set[tuple[str, int]] = set()
    duplicate_keys = 0
    invalid_populated = 0
    inconsistent_description = 0
    expected_schema = None

    for item in manifest["files"]:
        path = root / item["path"]
        if not path.is_file():
            errors.append(f"missing shard: {item['path']}")
            continue
        file_bytes = path.stat().st_size
        if file_bytes != item["bytes"]:
            errors.append(f"byte-size mismatch: {item['path']}")
        digest = sha256_file(path) if verify_hashes else None
        if digest is not None and digest != item["sha256"]:
            errors.append(f"SHA-256 mismatch: {item['path']}")
        parquet = pq.ParquetFile(path)
        rows = parquet.metadata.num_rows
        if rows != item["rows"]:
            errors.append(f"Parquet row count mismatch: {item['path']}")
        if expected_schema is None:
            expected_schema = parquet.schema_arrow
            if expected_schema.field("attributes").type.byte_width != 645:
                errors.append("attributes are not fixed-size 645-byte values")
            if expected_schema.field("null_bitmap").type.byte_width != 162:
                errors.append("null_bitmap is not fixed-size 162-byte values")
        elif not expected_schema.equals(parquet.schema_arrow):
            errors.append(f"schema drift: {item['path']}")
        total_rows += rows
        shard_reports.append({"path": item["path"], "rows": rows, "bytes": file_bytes,
                              "sha256": digest, "hash_checked": verify_hashes})
        if not profile:
            continue
        for batch in parquet.iter_batches(batch_size=8192, columns=list(READ_COLUMNS)):
            data = batch.to_pydict()
            for i, source in enumerate(data["source"]):
                if source is None:
                    errors.append(f"null source in {item['path']}")
                    continue
                source_counts[source] += 1
                key = (source, data["source_row_index"][i])
                if key in seen_keys:
                    duplicate_keys += 1
                else:
                    seen_keys.add(key)
                populated = data["populated_attribute_count"][i]
                if not isinstance(populated, int) or not 0 <= populated <= len(columns):
                    invalid_populated += 1
                else:
                    source_populated[source] += populated
                has_description = data["has_description"][i]
                description_count = data["description_count"][i]
                if has_description != (description_count > 0):
                    inconsistent_description += 1
                if has_description:
                    source_description[source] += 1
                for field, (field_index, values) in field_map.items():
                    value = decode_field(
                        data["attributes"][i], data["null_bitmap"][i],
                        data["attribute_overrides"][i], field_index, values,
                    )
                    if value is None:
                        field_counts[field]["<missing>"] += 1
                    else:
                        field_nonmissing_by_source[field][source] += 1
                        if value in values:
                            field_counts[field][value] += 1
                            field_canonical_by_source[field][source] += 1
                        else:
                            field_noncanonical[field] += 1
                            # Exact overrides can be free text; export buckets, never raw values.
                            if value.strip().lower() in {"null", "none", "unknown", "unsupported", "n/a"}:
                                field_sentinel[field] += 1
                                field_counts[field]["<sentinel>"] += 1
                            else:
                                field_counts[field]["<noncanonical>"] += 1

    if total_rows != manifest["rows"]:
        errors.append("total rows differ from manifest")
    if profile:
        if dict(source_counts) != manifest["sources"]:
            errors.append("source counts differ from manifest")
        if duplicate_keys:
            errors.append(f"{duplicate_keys} duplicate (source, source_row_index) keys")
        if invalid_populated:
            errors.append(f"{invalid_populated} invalid populated_attribute_count values")
        if inconsistent_description:
            errors.append(f"{inconsistent_description} inconsistent description flags/counts")
    else:
        warnings.append("Profile not run: source/field frequencies and key integrity unverified")
    reported_calibration = json.loads((root / "audit.json").read_text()).get("calibration", {})
    calibration_known_deltas: dict[str, int] = {}
    if profile:
        for field in ("age_bracket", "region", "gender_identity", "urbanicity"):
            actual_canonical = sum(field_canonical_by_source[field].values())
            reported = reported_calibration.get(field, {}).get("known_rows")
            if isinstance(reported, int):
                calibration_known_deltas[field] = actual_canonical - reported
        if any(calibration_known_deltas.values()):
            warnings.append("published audit.json calibrated-known counts disagree with released shards")
        calculated_mean = sum(source_populated.values()) / total_rows
        reported_mean = manifest.get("validation", {}).get("mean_populated_attributes")
        if isinstance(reported_mean, (int, float)) and abs(calculated_mean - reported_mean) > 0.001:
            warnings.append("manifest validation mean populated attributes disagrees with released shards")
        calculated_description_coverage = sum(source_description.values()) / total_rows
        reported_coverage = manifest.get("validation", {}).get("description_coverage")
        if isinstance(reported_coverage, (int, float)) and abs(calculated_description_coverage - reported_coverage) > 0.00001:
            warnings.append("manifest validation description coverage disagrees with released shards")
    if not verify_hashes:
        warnings.append("SHA-256 verification not run")
    warnings.extend([
        "MatrAIx states this is not a representative sample of any population",
        "North America is not a United States identifier; no US household weights supplied",
        "Persona categories are not measured cash, debt, spending, or decision transitions",
        "Human-derived text/metadata requires privacy review before any row-level export",
    ])

    cache_trees = sorted((root / ".cache/huggingface/trees").glob("*.json"))
    source_revision = cache_trees[0].stem if len(cache_trees) == 1 else None
    result: dict[str, Any] = {
        "dataset": "MatrAIx Persona 1M",
        "root": str(root),
        "source_revision_from_download_cache": source_revision,
        "manifest_sha256": sha256_file(root / "manifest.json"),
        "codebook_sha256": sha256_file(root / "persona_codes.schema.json"),
        "codebook_fields": len(columns),
        "parquet_rows": total_rows,
        "manifest_rows": manifest["rows"],
        "shards": shard_reports,
        "profiled": profile,
        "hashes_verified": verify_hashes,
        "source_counts": dict(sorted(source_counts.items())) if profile else None,
        "source_mean_populated_attributes": {
            source: round(source_populated[source] / count, 3)
            for source, count in sorted(source_counts.items())
        } if profile else None,
        "mean_populated_attributes": sum(source_populated.values()) / total_rows if profile else None,
        "source_description_rows": dict(sorted(source_description.items())) if profile else None,
        "description_coverage": sum(source_description.values()) / total_rows if profile else None,
        "duplicate_source_index_keys": duplicate_keys if profile else None,
        "invalid_populated_counts": invalid_populated if profile else None,
        "inconsistent_description_flags": inconsistent_description if profile else None,
        "field_counts": {field: dict(sorted(counter.items()))
                         for field, counter in field_counts.items()} if profile else None,
        "field_nonmissing_by_source": {field: dict(sorted(counter.items()))
                                       for field, counter in field_nonmissing_by_source.items()} if profile else None,
        "field_canonical_by_source": {field: dict(sorted(counter.items()))
                                      for field, counter in field_canonical_by_source.items()} if profile else None,
        "field_noncanonical_counts": dict(sorted(field_noncanonical.items())) if profile else None,
        "field_sentinel_counts": dict(sorted(field_sentinel.items())) if profile else None,
        "published_calibration_known_count_deltas": calibration_known_deltas if profile else None,
        "errors": errors,
        "warnings": warnings,
        "structural_integrity_passed": not errors,
        "us_representativeness_validated": False,
        "passed": not errors,
    }
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DATA_ROOT)
    parser.add_argument("--verify-hashes", action="store_true", help="read every shard and compare SHA-256")
    parser.add_argument("--profile", action="store_true", help="stream source counts and selected codebook fields")
    parser.add_argument("--output", type=Path, help="write aggregate-only QC JSON")
    args = parser.parse_args()
    result = audit(args.root, verify_hashes=args.verify_hashes, profile=args.profile)
    output = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output)
    else:
        print(output, end="")
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
