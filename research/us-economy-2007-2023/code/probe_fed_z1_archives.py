"""Verify local dated Federal Reserve Z.1 archives without interpreting tables.

These files are a format/vintage probe, not a calibrated financial-accounts
panel. The 2007 correction notice makes intraday as-of claims unsafe here.
"""

from __future__ import annotations

import hashlib
import json
from collections import Counter
from pathlib import Path, PurePosixPath
from zipfile import ZipFile


ROOT = Path(__file__).resolve().parents[1]
RAW = ROOT / "data/raw/fed-z1"
ARTIFACT = ROOT / "artifacts/fed_z1_two_release_probe.json"


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def inspect(raw: Path = RAW) -> dict:
    provenance = json.loads((raw / "provenance.json").read_text(encoding="utf-8"))
    if provenance.get("schema_version") != "owg.raw-source.v1":
        raise ValueError("unexpected Z.1 provenance schema")
    expected_names = {
        "2007-03-08_ltabs.zip", "2007-03-08_z1.pdf",
        "2023-03-09_z1_csv_files.zip", "2023-03-09_z1.pdf",
    }
    entries = provenance.get("files", [])
    if {item["filename"] for item in entries} != expected_names or len(entries) != 4:
        raise ValueError("missing or duplicate Z.1 archive file")
    results = []
    for item in entries:
        path = raw / item["filename"]
        if path.stat().st_size != item["bytes"] or digest(path) != item["sha256"]:
            raise ValueError(f"Z.1 source size/hash mismatch: {path.name}")
        if not item["source_url"].startswith("https://www.federalreserve.gov/releases/z1/"):
            raise ValueError(f"non-official Z.1 URL: {path.name}")
        report = {
            "filename": path.name,
            "source_url": item["source_url"],
            "sha256": item["sha256"],
            "bytes": item["bytes"],
            "conservative_available_not_before_utc": item[
                "conservative_available_not_before_utc"
            ],
        }
        if path.suffix == ".pdf":
            with path.open("rb") as stream:
                if stream.read(5) != b"%PDF-":
                    raise ValueError(f"invalid PDF signature: {path.name}")
            report["format"] = "pdf"
        else:
            with ZipFile(path) as archive:
                names = archive.namelist()
                if not names or len(names) != len(set(names)):
                    raise ValueError(f"empty or duplicate ZIP names: {path.name}")
                for name in names:
                    member = PurePosixPath(name)
                    if member.is_absolute() or ".." in member.parts:
                        raise ValueError(f"unsafe ZIP member: {path.name}")
                bad_member = archive.testzip()
                if bad_member is not None:
                    raise ValueError(f"ZIP CRC failure: {path.name}/{bad_member}")
                report["format"] = "zip"
                report["member_count"] = len(names)
                report["member_extensions"] = dict(sorted(Counter(
                    PurePosixPath(name).suffix.lower() for name in names if not name.endswith("/")
                ).items()))
        results.append(report)
    return {
        "schema_version": "owg.fed-z1-format-probe.v1",
        "interpretation_status": "format-and-hash-only; no mapped financial-account series",
        "verified_at_utc": provenance["verified_at_utc"],
        "point_in_time_caveat": provenance["point_in_time_caveat"],
        "files": results,
    }


def main() -> None:
    report = inspect()
    ARTIFACT.parent.mkdir(parents=True, exist_ok=True)
    ARTIFACT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"files": len(report["files"]), "artifact": str(ARTIFACT)}, indent=2))


if __name__ == "__main__":
    main()
