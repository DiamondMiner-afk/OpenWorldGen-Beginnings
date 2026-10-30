"""Send a frozen macro vintage to a local OpenAI-compatible LLM server.

This is a hypothesis probe, not a validated historical forecaster. Modern
model weights may contain knowledge of the supposedly held-out period.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import urllib.request
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODEL = ROOT.parents[1] / "local-models/Qwen3.8-27B-UD-Q4_K_M.gguf"


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def snapshot(manifest_path: Path) -> tuple[dict, dict]:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    cutoff = manifest["as_of"][:10]
    records = {}
    for source in manifest["sources"]:
        path = manifest_path.parent / source["relative_path"]
        if digest(path) != source["sha256"]:
            raise ValueError(f"source checksum mismatch: {source['id']}")
        if source["published_at"] > manifest["as_of"]:
            raise ValueError(f"future source release: {source['id']}")
        if source["observation_end"] > cutoff:
            raise ValueError(f"future observation: {source['id']}")
        with path.open(encoding="utf-8", newline="") as stream:
            rows = list(csv.DictReader(stream))
        if not rows or any(set(row) != {"observation_period", "value"} for row in rows):
            raise ValueError(f"bad CSV shape: {source['id']}")
        records[source["classifications"]["series"]] = {
            "units": source["units"]["value"],
            "release_id": source["release_id"],
            "recent_observations": rows[-12:] if source["classifications"]["frequency"] == "quarterly" else rows[-24:],
        }
    return manifest, records


def target_periods(origin: str) -> dict:
    year, q = int(origin[:4]), int(origin[-1])
    next_year, next_q = (year + 1, 1) if q == 4 else (year, q + 1)
    month = (next_q - 1) * 3 + 1
    return {"ROUTPUT": f"{next_year}:Q{next_q}", "RUC": f"{next_year}:{month:02d}", "CPI": f"{next_year}:{month:02d}"}


def build_prompt(manifest: dict, records: dict, origin: str) -> str:
    payload = {
        "as_of": manifest["as_of"],
        "origin_vintage": origin,
        "targets": target_periods(origin),
        "series": records,
    }
    return (
        "You are an economic research assistant. Use ONLY the dated observations below. "
        "Do not use knowledge of events after the as_of date. The numbers are a historical "
        "release vintage, not current revised data. Return one JSON object with keys: "
        "forecasts (object keyed ROUTPUT, RUC, CPI; each has period, point, low, high, units), "
        "mechanisms (array of objects with hypothesis, evidence_series, falsifier), "
        "unknowns (array of strings). Give numeric forecasts in the stated units. "
        "Mechanisms are hypotheses, not established causes. No markdown.\n\n"
        + json.dumps(payload, separators=(",", ":"))
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--origin", default="2006Q4")
    parser.add_argument("--server", default="http://127.0.0.1:18080")
    parser.add_argument("--model", type=Path, default=MODEL)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    manifest_path = ROOT / "data/processed/philadelphia-fed" / args.origin / "data-vintage.json"
    manifest, records = snapshot(manifest_path)
    prompt = build_prompt(manifest, records, args.origin)
    request_data = {
        "model": "owg-qwen-local",
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0,
        "seed": 17,
        "max_tokens": 700,
        "response_format": {"type": "json_object"},
    }
    if args.dry_run:
        print(json.dumps({"as_of": manifest["as_of"], "targets": target_periods(args.origin), "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest(), "prompt_chars": len(prompt)}, indent=2))
        return
    request = urllib.request.Request(
        args.server.rstrip("/") + "/v1/chat/completions",
        data=json.dumps(request_data).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=600) as response:
        raw = json.load(response)
    content = raw["choices"][0]["message"]["content"]
    answer = json.loads(content)
    if not isinstance(answer.get("forecasts"), dict) or not isinstance(answer.get("mechanisms"), list):
        raise ValueError("model response does not contain the required forecast and mechanism fields")
    for series, target in target_periods(args.origin).items():
        forecast = answer["forecasts"].get(series, {})
        if forecast.get("period") != target:
            raise ValueError(f"wrong {series} forecast period: {forecast.get('period')}")
        for key in ("point", "low", "high"):
            if not isinstance(forecast.get(key), (int, float)):
                raise ValueError(f"missing numeric {series}.{key}")
        if not forecast["low"] <= forecast["point"] <= forecast["high"]:
            raise ValueError(f"invalid {series} interval")
    output = {
        "status": "historical_hypothesis_probe_not_clean_forecast",
        "created_at": datetime.now(timezone.utc).isoformat(),
        "as_of": manifest["as_of"],
        "origin": args.origin,
        "manifest_sha256": digest(manifest_path),
        "model_path": str(args.model.resolve()),
        "model_sha256": digest(args.model),
        "request": request_data,
        "response": raw,
        "parsed": answer,
        "warning": "The model may know post-origin history from pretraining. Do not score as leak-free out-of-sample forecasting.",
    }
    destination = ROOT / "artifacts" / f"local-macro-probe-{args.origin}.json"
    destination.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"artifact": str(destination), "forecasts": answer["forecasts"], "mechanisms": answer["mechanisms"]}, indent=2))


if __name__ == "__main__":
    main()
