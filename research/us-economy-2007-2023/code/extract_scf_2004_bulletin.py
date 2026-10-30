"""Extract a bounded set of 2004 wealth anchors from the Fed SCF Bulletin.

This is a published-table probe, not a microdata distribution or an as-of-byte
certificate. It deliberately requires an exact PDF hash and an external
``pdftotext`` executable so parser drift fails closed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


EXPECTED_SHA256 = "5c134abfa2957d49c578ddc7dfed82879d69449ce52fd800c9503a03e9a6c071"
PDF_RELATIVE = Path("data/raw/fed-scf-2004/2026-10-18-retrieval/2004_bull0206.pdf")
EXPECTED_BINS = ("Less than 25", "25–49.9", "50–74.9", "75–89.9", "90–100")
BIN_SHARES = (0.25, 0.25, 0.25, 0.15, 0.10)


def _numbers(line: str) -> list[float]:
    # The PDF uses a Unicode minus for negative wealth and commas in thousands.
    tokens = re.findall(r"[−-]?\d[\d,]*(?:\.\d+)?", line)
    return [float(token.replace(",", "").replace("−", "-")) for token in tokens]


def parse_bulletin_text(text: str) -> dict:
    footnote_start = text.index("9. Here are selected percentiles of the distribution of net worth")
    footnote_end = text.index("3. Family net worth, by selected characteristics", footnote_start)
    footnote = text[footnote_start:footnote_end]
    cutoffs: dict[str, float] = {}
    for line in footnote.splitlines():
        match = re.match(r"^\s*(25|50|75|90)\s*(?:\.\s*){3,}", line)
        if match:
            values = _numbers(line)
            # This footnote shares a page with a second newspaper-style
            # column, whose text may continue on the same extracted line.
            if len(values) < 5:
                raise ValueError(f"unexpected percentile row: {line}")
            cutoffs[match.group(1)] = values[4]
    if set(cutoffs) != {"25", "50", "75", "90"}:
        raise ValueError(f"missing 2004 net-worth cutoffs: {cutoffs}")

    table_start = footnote_end
    table_end = text.index("Note: See note to table 1.", table_start)
    table = text[table_start:table_end]
    all_families_line = next(
        (line for line in table.splitlines() if re.match(r"^\s*All families\s*(?:\.\s*){3,}", line)),
        None,
    )
    if all_families_line is None:
        raise ValueError("Table 3 all-families row not found")
    all_values = _numbers(all_families_line)
    if len(all_values) != 8:
        raise ValueError(f"unexpected all-families row: {all_families_line}")
    all_median, all_mean = all_values[-2:]

    bin_start = table.index("Percentile of net worth")
    rows = table[bin_start:].splitlines()
    groups: dict[str, dict[str, float]] = {}
    for label in EXPECTED_BINS:
        line = next((row for row in rows if re.match(r"^\s*" + re.escape(label) + r"\s*(?:\.\s*){3,}", row)), None)
        if line is None:
            raise ValueError(f"Table 3 group missing: {label}")
        # Labels themselves contain numbers; the last pair is the 2004 pair.
        values = _numbers(line)
        if len(values) < 8:
            raise ValueError(f"unexpected Table 3 group row: {line}")
        groups[label] = {"median_thousand_2004_usd": values[-2], "mean_thousand_2004_usd": values[-1]}

    weighted_mean = sum(
        share * groups[label]["mean_thousand_2004_usd"]
        for label, share in zip(EXPECTED_BINS, BIN_SHARES, strict=True)
    )
    if abs(cutoffs["50"] / 1000 - all_median) > 0.05:
        raise ValueError("published 50th percentile does not match Table 3 median")
    if abs(weighted_mean - all_mean) > 0.2:
        raise ValueError("published group means do not reconstruct Table 3 all-family mean")

    return {
        "schema_version": "owg.scf-2004-bulletin-anchors.v1",
        "source_url": "https://www.federalreserve.gov/econres/files/2004_bull0206.pdf",
        "survey_year": 2004,
        "bulletin_publication": "February 2006",
        "exact_pdf_available_at_2007_01_01": "not_certified",
        "wealth_percentile_cutoffs_2004_usd": cutoffs,
        "table_3_all_families": {
            "median_thousand_2004_usd": all_median,
            "mean_thousand_2004_usd": all_mean,
        },
        "table_3_within_wealth_percentile_groups": groups,
        "checks": {
            "median_matches_50th_cutoff": True,
            "share_weighted_group_mean_thousand_2004_usd": round(weighted_mean, 3),
            "all_family_mean_matches_group_mean_within_rounding": True,
        },
        "interpretation_warning": "Group medians and means are conditional summaries, not group boundaries or a full household joint distribution.",
    }


def extract(pdf_path: Path) -> dict:
    digest = hashlib.sha256(pdf_path.read_bytes()).hexdigest()
    if digest != EXPECTED_SHA256:
        raise ValueError(f"PDF SHA-256 mismatch: {digest}")
    result = subprocess.run(
        ["pdftotext", "-layout", str(pdf_path), "-"],
        check=True,
        capture_output=True,
        text=True,
    )
    artifact = parse_bulletin_text(result.stdout)
    artifact["source_pdf_sha256"] = digest
    artifact["source_pdf_bytes"] = pdf_path.stat().st_size
    return artifact


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    root = Path(__file__).resolve().parents[1]
    parser.add_argument("--pdf", type=Path, default=root / PDF_RELATIVE)
    parser.add_argument("--output", type=Path, default=root / "artifacts/scf_2004_bulletin_wealth_anchors.json")
    args = parser.parse_args()
    artifact = extract(args.pdf)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(artifact, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
