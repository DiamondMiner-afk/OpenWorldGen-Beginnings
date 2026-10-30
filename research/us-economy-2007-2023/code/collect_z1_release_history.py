"""Resume a bounded collection of official Z.1 releases for the US study.

Dates and reported quarters come from the saved official release calendar.
Files remain separate by release. Retrieval integrity never implies exact
historical availability or permission to feed later revisions to past actors.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import subprocess
import tempfile
from datetime import datetime, timezone
from urllib.parse import urljoin, urlparse
from zipfile import BadZipFile, ZipFile


ROOT = Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "data/raw/fed-z1-release-collection/2026-10-21"
CALENDAR = "https://www.federalreserve.gov/releases/z1/release-dates.htm"
PACKAGES = {
    "atabs.zip": "flows_seasonally_adjusted_annual_rates",
    "utabs.zip": "flows_unadjusted_quarterly_rates",
    "ltabs.zip": "levels_end_of_period_not_seasonally_adjusted",
    "btabs.zip": "balance_sheets_source_specific_units",
}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []

    def handle_starttag(self, tag, attrs):
        if tag == "a":
            href = dict(attrs).get("href")
            if href:
                self.links.append(href)


class ReleaseCalendar(HTMLParser):
    def __init__(self):
        super().__init__()
        self.depth = 0
        self.active = []
        self.rows = []

    def handle_starttag(self, tag, attrs):
        attributes = dict(attrs)
        if tag == "div":
            self.depth += 1
            if "row" in attributes.get("class", "").split():
                self.active.append({"depth": self.depth, "text": [], "links": []})
        if tag == "a" and attributes.get("href"):
            for row in self.active:
                row["links"].append(attributes["href"])

    def handle_data(self, data):
        for row in self.active:
            row["text"].append(data)

    def handle_endtag(self, tag):
        if tag == "div":
            finished = [row for row in self.active if row["depth"] == self.depth]
            self.rows.extend(finished)
            self.active = [row for row in self.active if row["depth"] != self.depth]
            self.depth -= 1


def parse_calendar(content: str, start: str = "2006:Q3", end: str = "2023:Q4") -> list[dict]:
    parser = ReleaseCalendar()
    parser.feed(content)
    found = {}
    for row in parser.rows:
        quarters = set(re.findall(r"\b\d{4}:Q[1-4]\b", " ".join(row["text"])))
        pdfs = [urljoin(CALENDAR, href) for href in row["links"]
                if re.search(r"/releases/z1/\d{8}/z1\.pdf$", href, re.IGNORECASE)]
        if len(quarters) != 1 or len(set(pdfs)) != 1:
            continue
        quarter = quarters.pop()
        if not start <= quarter <= end:
            continue
        pdf = pdfs[0]
        date = re.search(r"/(\d{8})/z1\.pdf$", pdf, re.IGNORECASE)[1]
        datetime.strptime(date, "%Y%m%d")
        csvs = [urljoin(CALENDAR, href) for href in row["links"]
                if re.search(r"/releases/z1/" + date + r"/z1_csv_files\.zip$", href, re.IGNORECASE)]
        record = {"release_path_id": date, "reported_through": quarter, "pdf_url": pdf,
                  "csv_url": csvs[0] if csvs else None}
        if quarter in found and found[quarter] != record:
            raise ValueError(f"calendar has conflicting releases for {quarter}")
        found[quarter] = record
    records = [found[quarter] for quarter in sorted(found)]
    if not records or records[0]["reported_through"] != start or records[-1]["reported_through"] != end:
        raise ValueError("calendar does not cover requested boundary quarters")
    ordinals = [int(row["reported_through"][:4]) * 4 + int(row["reported_through"][-1]) - 1
                for row in records]
    if any(current != previous + 1 for previous, current in zip(ordinals, ordinals[1:])):
        raise ValueError("calendar has a missing reporting quarter")
    if len({row["release_path_id"] for row in records}) != len(records):
        raise ValueError("calendar repeats a release date for different quarters")
    return records


def require_official_url(url: str, release: str | None = None) -> None:
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.hostname != "www.federalreserve.gov" or parsed.username or parsed.password:
        raise ValueError(f"unexpected source origin: {url}")
    if release is not None and not parsed.path.lower().startswith(f"/releases/z1/{release}/"):
        raise ValueError(f"download changed release path: {url}")


def validate_file(path: Path, kind: str) -> dict:
    if path.stat().st_size == 0:
        raise ValueError("empty downloaded file")
    if kind == "pdf":
        if path.read_bytes()[:5] != b"%PDF-":
            raise ValueError("download is not a PDF")
        return {"format": "pdf", "signature_valid": True}
    if kind == "zip":
        try:
            with ZipFile(path) as archive:
                members = archive.namelist()
                if not members or archive.testzip() is not None:
                    raise ValueError("empty or CRC-invalid ZIP")
                return {"format": "zip", "members": len(members), "crc_valid": True}
        except BadZipFile as error:
            raise ValueError("download is not a valid ZIP") from error
    if b"<html" not in path.read_bytes().lower()[:10000]:
        raise ValueError("download is not an HTML page")
    return {"format": "html", "signature_valid": True}


def cover_identity(path: Path) -> dict:
    text = subprocess.run(["pdftotext", "-f", "1", "-l", "1", str(path), "-"],
                          check=True, capture_output=True, text=True).stdout
    months = "January|February|March|April|May|June|July|August|September|October|November|December"
    dates = {datetime.strptime(" ".join(match), "%B %d %Y").date().isoformat()
             for match in re.findall(rf"\b({months})\s+(\d{{1,2}})\s*,?\s*(\d{{4}})\b", text)}
    quarters = {f"{year}:Q{('First', 'Second', 'Third', 'Fourth').index(part) + 1}"
                for part, year in re.findall(r"\b(First|Second|Third|Fourth)\s+Quarter\s+(\d{4})\b", text)}
    if len(dates) != 1 or len(quarters) != 1:
        raise ValueError("PDF cover does not uniquely identify a publication date and quarter")
    return {"publication_date": dates.pop(), "reported_through": quarters.pop()}


def fetch(url: str, target: Path, kind: str, *, release: str | None = None,
          previous: dict | None = None) -> dict:
    require_official_url(url, release)
    if target.exists():
        if previous is None or previous.get("source_url") != url or digest(target) != previous.get("sha256"):
            raise ValueError(f"existing download has no matching receipt: {target}")
        validate_file(target, kind)
        return previous
    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=target.parent, prefix=".z1-body-", delete=False) as stream:
        temporary = Path(stream.name)
    with tempfile.NamedTemporaryFile(dir=target.parent, prefix=".z1-headers-", delete=False) as stream:
        headers = Path(stream.name)
    try:
        completed = subprocess.run([
            "curl", "--location", "--fail", "--silent", "--show-error",
            "--retry", "2", "--retry-delay", "1", "--connect-timeout", "15",
            "--max-time", "60", "--max-filesize", "33554432",
            "--dump-header", str(headers), "--output", str(temporary),
            "--write-out", "%{http_code}\n%{url_effective}\n", url,
        ], capture_output=True, text=True)
        if completed.returncode:
            raise RuntimeError(completed.stderr.strip() or "curl download failed")
        status, final_url = completed.stdout.strip().splitlines()[-2:]
        if status != "200":
            raise ValueError(f"unexpected HTTP status {status}")
        require_official_url(final_url, release)
        validation = validate_file(temporary, kind)
        fields = {}
        for line in headers.read_text(errors="replace").splitlines():
            if ":" in line:
                name, value = line.split(":", 1)
                fields[name.lower()] = value.strip()
        receipt = {
            "source_url": url, "final_url": final_url, "retrieved_at_utc": utc_now(),
            "sha256": digest(temporary), "bytes": temporary.stat().st_size,
            "http_last_modified": fields.get("last-modified"),
            "http_content_type": fields.get("content-type"),
            "validation": validation,
            "historical_input_admitted": False,
        }
        temporary.rename(target)
        return receipt
    finally:
        temporary.unlink(missing_ok=True)
        headers.unlink(missing_ok=True)


def collect_release(record: dict, destination: Path, previous: dict | None) -> dict:
    date = record["release_path_id"]
    root = destination / date
    old_files = (previous or {}).get("files", {})
    files = {}
    errors = []

    def acquire(name, url, kind, category):
        try:
            receipt = fetch(url, root / name, kind, release=date, previous=old_files.get(name))
            files[name] = {**receipt, "category": category}
        except (OSError, ValueError, RuntimeError) as error:
            errors.append({"filename": name, "source_url": url, "error": str(error)})

    acquire("z1.pdf", record["pdf_url"], "pdf", "release_document")
    if record["csv_url"]:
        acquire("z1_csv_files.zip", record["csv_url"], "zip", "all_csv_tables_mixed_units")
    else:
        page_url = f"https://www.federalreserve.gov/releases/z1/{date}/data.htm"
        acquire("data.htm", page_url, "html", "historical_download_index")
        if "data.htm" in files:
            parser = Links()
            parser.feed((root / "data.htm").read_text(errors="replace"))
            links = {Path(urlparse(href).path).name.lower(): urljoin(page_url, href)
                     for href in parser.links}
            for filename, category in PACKAGES.items():
                if filename in links:
                    acquire(filename, links[filename], "zip", category)
                else:
                    errors.append({"filename": filename, "error": "package absent from official index"})
    cover = None
    if "z1.pdf" in files:
        try:
            cover = cover_identity(root / "z1.pdf")
            if cover["reported_through"] != record["reported_through"]:
                raise ValueError("PDF cover quarter differs from calendar")
        except (ValueError, OSError, subprocess.CalledProcessError) as error:
            errors.append({"filename": "z1.pdf", "error": str(error)})
    expected = 2 if record["csv_url"] else 6
    return {**record, "files": files, "errors": errors,
            "publication_date_from_cover": cover["publication_date"] if cover else None,
            "status": "retrieved" if len(files) == expected and not errors else "incomplete"}


def save(path: Path, manifest: dict) -> None:
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def audit_collection(destination: Path) -> dict:
    manifest_path = destination / "collection.json"
    manifest = json.loads(manifest_path.read_text())
    failures = []
    calendar = destination / "release-dates.htm"
    if digest(calendar) != manifest["calendar"]["sha256"]:
        raise ValueError("saved calendar hash mismatch")
    records = parse_calendar(calendar.read_text())
    expected_dates = {row["release_path_id"] for row in records}
    if set(manifest["releases"]) != expected_dates:
        failures.append("manifest release dates differ from the complete calendar range")
    checked_files = checked_bytes = checked_covers = 0
    cover_identities = []
    for record in records:
        date = record["release_path_id"]
        release = manifest["releases"].get(date)
        if release is None:
            failures.append(f"missing release {date}")
            continue
        if any(release.get(key) != record[key] for key in record):
            failures.append(f"release metadata differs from saved calendar: {date}")
        expected_names = {"z1.pdf", "z1_csv_files.zip"} if record["csv_url"] else {"z1.pdf", "data.htm", *PACKAGES}
        if set(release["files"]) != expected_names or release["status"] != "retrieved":
            failures.append(f"incomplete file set: {date}")
        for name, receipt in release["files"].items():
            if name not in expected_names:
                failures.append(f"unexpected file receipt: {date}/{name}")
                continue
            file = destination / date / name
            try:
                require_official_url(receipt["source_url"], date)
                require_official_url(receipt["final_url"], date)
                if file.is_symlink() or file.parent.is_symlink():
                    raise ValueError("symlinked collection file")
                if file.stat().st_size != receipt["bytes"] or digest(file) != receipt["sha256"]:
                    raise ValueError("size/hash mismatch")
                kind = "pdf" if name.endswith(".pdf") else "zip" if name.endswith(".zip") else "html"
                validate_file(file, kind)
                checked_files += 1
                checked_bytes += file.stat().st_size
                if kind == "pdf":
                    identity = cover_identity(file)
                    if identity["reported_through"] != record["reported_through"]:
                        raise ValueError("PDF cover quarter differs from calendar")
                    if identity["publication_date"] != release.get("publication_date_from_cover"):
                        raise ValueError("PDF cover publication date differs from collection receipt")
                    cover_identities.append({"release_path_id": date, **identity})
                    checked_covers += 1
            except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
                failures.append(f"{date}/{name}: {error}")
    return {
        "schema_version": "owg.z1-release-collection-audit.v1",
        "collection_manifest_sha256": digest(manifest_path),
        "calendar_sha256": digest(calendar),
        "release_count_expected": len(records), "release_count_present": len(manifest["releases"]),
        "files_hash_checked": checked_files, "bytes_hash_checked": checked_bytes,
        "pdf_release_covers_checked": checked_covers,
        "cover_identities": cover_identities,
        "path_label_date_disagreements": [row for row in cover_identities
            if row["release_path_id"] != row["publication_date"].replace("-", "")],
        "first_reported_quarter": records[0]["reported_through"],
        "last_reported_quarter": records[-1]["reported_through"],
        "failures": failures, "passed": not failures,
        "historical_input_admitted": False,
        "boundary": "Complete local retrieval and cover-date checks only. URL path IDs are not publication dates. Exact historical origin, per-table units, revisions, and economic reconciliation remain separate gates.",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, default=DESTINATION)
    parser.add_argument("--workers", type=int, choices=(1, 2, 3), default=3)
    parser.add_argument("--audit", action="store_true")
    parser.add_argument("--audit-output", type=Path)
    args = parser.parse_args()
    destination = args.destination
    if args.audit:
        result = audit_collection(destination)
        if args.audit_output:
            args.audit_output.parent.mkdir(parents=True, exist_ok=True)
            args.audit_output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({key: value for key, value in result.items() if key != "cover_identities"}, indent=2))
        return 0 if result["passed"] else 1
    destination.mkdir(parents=True, exist_ok=True)
    path = destination / "collection.json"
    manifest = json.loads(path.read_text()) if path.exists() else {
        "schema_version": "owg.z1-release-collection.v1", "created_at_utc": utc_now(),
        "scope": "2006Q3 through 2023Q4 reported quarters, preserving separate release editions",
        "status": "active", "releases": {}, "attempt_errors": [],
        "historical_input_admitted": False,
        "limitation": "Current official historical downloads can contain later corrections. No archive-origin or semantic/table-level admission is implied by a successful fetch, PDF signature, or ZIP CRC.",
    }
    calendar_receipt = fetch(CALENDAR, destination / "release-dates.htm", "html",
                             previous=manifest.get("calendar"))
    manifest["calendar"] = calendar_receipt
    records = parse_calendar((destination / "release-dates.htm").read_text())
    manifest["release_count_expected"] = len(records)
    save(path, manifest)
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        jobs = {pool.submit(collect_release, record, destination,
                            manifest["releases"].get(record["release_path_id"])): record
                for record in records}
        for future in concurrent.futures.as_completed(jobs):
            record = jobs[future]
            result = future.result()
            manifest["releases"][record["release_path_id"]] = result
            if result["errors"]:
                manifest["attempt_errors"].append({"at_utc": utc_now(),
                    "release_path_id": record["release_path_id"], "errors": result["errors"]})
            manifest["updated_at_utc"] = utc_now()
            save(path, manifest)
            print(f"path={record['release_path_id']} published={result['publication_date_from_cover']} {record['reported_through']}: {result['status']} ({len(result['files'])} files)", flush=True)
    complete = sum(row["status"] == "retrieved" for row in manifest["releases"].values())
    manifest["status"] = "retrieved" if complete == len(records) else "incomplete"
    manifest["release_count_retrieved"] = complete
    manifest["bytes_retrieved"] = sum(file["bytes"] for release in manifest["releases"].values()
                                      for file in release["files"].values())
    save(path, manifest)
    print(json.dumps({key: manifest[key] for key in (
        "status", "release_count_expected", "release_count_retrieved", "bytes_retrieved")}), flush=True)
    return 0 if manifest["status"] == "retrieved" else 1


if __name__ == "__main__":
    raise SystemExit(main())
