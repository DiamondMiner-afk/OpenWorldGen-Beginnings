#!/usr/bin/env python3
"""Turn bounded local-LLM choices into a validated OpenWorldGen decision tape.

This is an inference/replay boundary, not an economic forecast. The caller owns
the point-in-time observation and actor-authority checks. Only option IDs from
its finite menu can be selected; the model never supplies executable amounts.
"""

from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
from typing import Any, Callable
from urllib.parse import urlsplit
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = ROOT.parents[1]
SCHEMA = REPOSITORY / "schemas/decision-tape-v1.schema.json"
DEFAULT_MODEL = REPOSITORY / "local-models/Qwen3.8-27B-UD-Q4_K_M.gguf"
DEFAULT_OWG = REPOSITORY / "target/debug/owg"
PROMPT_TEMPLATE = (
    "You are a bounded actor in an economic simulation. Consider ONLY the "
    "time-stamped observation and the supplied options. Select one option_id; "
    "do not invent actions, parameters, authority, or later facts. Return only "
    "a JSON object with exactly actor_id, option_id, rationale, expected_effect, "
    "and confidence. The rationale and expected_effect must be short nonempty "
    "sentences. Confidence is a number from 0 to 1. This is a proposal, not a "
    "validated causal explanation or forecast."
)
RESPONSE_KEYS = {"actor_id", "option_id", "rationale", "expected_effect", "confidence"}
OBSERVATION_KEYS = {
    "actor_id", "month", "inflation", "unemployment", "policy_rate",
    "liquidity", "debt", "inventory", "expected_demand", "sector",
}
ALLOWED_SECTORS = {"energy", "food", "manufacturing", "services"}
MAX_RESPONSE_BYTES = 65_536


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        return None


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode("utf-8")


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def strict_json(data: str) -> Any:
    return json.loads(
        data,
        object_pairs_hook=_unique_object,
        parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"invalid JSON number: {value}")),
    )


def _number(value: Any) -> bool:
    if type(value) not in (int, float):
        return False
    try:
        return math.isfinite(value)
    except OverflowError:
        # Python integers are unbounded; Rust f64 fields are not.
        return False


def _integer(value: Any, bits: int, unsigned: bool = False) -> bool:
    return type(value) is int and (0 if unsigned else -(1 << (bits - 1))) <= value <= ((1 << bits) - 1 if unsigned else (1 << (bits - 1)) - 1)


def _utc_time(value: Any) -> datetime:
    if not isinstance(value, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", value):
        raise ValueError("as_of must be a UTC timestamp like 2020-01-31T23:59:59Z")
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ValueError("invalid as_of timestamp") from exc


def _schema_value(value: Any, definition: dict[str, Any], defs: dict[str, Any]) -> bool:
    if "$ref" in definition:
        ref = definition["$ref"]
        if not isinstance(ref, str) or not ref.startswith("#/$defs/"):
            raise ValueError(f"unsupported schema reference: {ref}")
        return _schema_value(value, defs[ref.removeprefix("#/$defs/")], defs)
    if "anyOf" in definition:
        return any(_schema_value(value, part, defs) for part in definition["anyOf"])
    if "const" in definition:
        return value == definition["const"] and type(value) is type(definition["const"])
    declared = definition.get("type")
    if isinstance(declared, list):
        return any(_schema_value(value, {**definition, "type": item}, defs) for item in declared)
    if declared == "null":
        return value is None
    if declared == "string":
        return isinstance(value, str)
    if declared == "number":
        return _number(value)
    if declared == "integer":
        if definition.get("format") == "int32":
            return _integer(value, 32)
        if definition.get("format") == "int64":
            return _integer(value, 64)
        if definition.get("format") == "uint32":
            return _integer(value, 32, unsigned=True)
        return type(value) is int
    raise ValueError(f"unsupported action schema definition: {definition}")


def validate_action(action: Any, schema: dict[str, Any]) -> None:
    """Fail closed on unknown or stale schema variants; Rust is the final gate."""
    if not isinstance(action, dict) or not isinstance(action.get("type"), str):
        raise ValueError("action must have a string type")
    defs = schema["$defs"]
    variants = defs["AgentAction"]["oneOf"]
    branch = next((item for item in variants if item["properties"]["type"].get("const") == action["type"]), None)
    if branch is None:
        raise ValueError(f"action type missing from generated schema: {action['type']}")
    properties = branch["properties"]
    if not set(branch["required"]) <= set(action) or not set(action) <= set(properties):
        raise ValueError(f"action has missing or extra fields: {action['type']}")
    if any(not _schema_value(value, properties[key], defs) for key, value in action.items()):
        raise ValueError(f"action field fails schema type/range: {action['type']}")
    kind = action["type"]
    if kind == "household_budget" and not (0 <= action["consumption_share"] <= 1 and action["reservation_wage"] >= 0):
        raise ValueError("household budget fails Rust semantic bounds")
    if kind == "firm_plan" and action["investment"] < 0:
        raise ValueError("firm investment cannot be negative")
    if kind == "bank_credit_rule" and not 0 <= action["approval_threshold"] <= 1:
        raise ValueError("credit approval threshold must lie in [0,1]")
    if kind == "policy_proposal" and any(action.get(key) is not None and action[key] < 0 for key in ("transfer", "public_spending")):
        raise ValueError("policy transfer/spending cannot be negative")
    if kind == "owner_distribution" and action["amount"] < 0:
        raise ValueError("owner distribution cannot be negative")


def validate_spec(spec: Any, schema: dict[str, Any], max_calls: int) -> None:
    if not isinstance(spec, dict) or set(spec) != {"id", "steps"}:
        raise ValueError("input requires exactly id and steps")
    if not isinstance(spec["id"], str) or not spec["id"].strip() or len(spec["id"]) > 120:
        raise ValueError("invalid tape id")
    steps = spec["steps"]
    if not isinstance(steps, list) or not steps:
        raise ValueError("input requires at least one step")
    previous_time: datetime | None = None
    previous_month = -1
    calls = 0
    for step_number, step in enumerate(steps):
        if not isinstance(step, dict) or set(step) != {"as_of", "observations", "actor_contexts"}:
            raise ValueError(f"step {step_number} requires as_of, observations, actor_contexts")
        as_of = _utc_time(step["as_of"])
        if previous_time is not None and as_of <= previous_time:
            raise ValueError("as_of timestamps must increase")
        previous_time = as_of
        batch = step["observations"]
        if not isinstance(batch, dict) or set(batch) != {"observations"} or not isinstance(batch["observations"], list) or not batch["observations"]:
            raise ValueError(f"step {step_number} has no observation batch")
        contexts = step["actor_contexts"]
        if not isinstance(contexts, dict):
            raise ValueError("actor_contexts must be an object")
        actors: set[str] = set()
        months: set[int] = set()
        for observation in batch["observations"]:
            if not isinstance(observation, dict) or not set(observation) <= OBSERVATION_KEYS or OBSERVATION_KEYS - {"sector"} - set(observation):
                raise ValueError("observation has missing or extra fields")
            actor_id = observation["actor_id"]
            if not isinstance(actor_id, str) or not 0 < len(actor_id.strip()) <= 120 or actor_id in actors:
                raise ValueError("actor IDs must be nonempty and unique within a step")
            actors.add(actor_id)
            month = observation["month"]
            if not _integer(month, 32, unsigned=True):
                raise ValueError("month must be uint32")
            months.add(month)
            if any(not _number(observation[key]) for key in ("inflation", "unemployment", "policy_rate", "inventory", "expected_demand")):
                raise ValueError("observation has a nonfinite or nonnumeric field")
            if not (0 <= observation["unemployment"] <= 1 and observation["inventory"] >= 0 and observation["expected_demand"] >= 0):
                raise ValueError("observation fails Rust semantic bounds")
            if not all(_integer(observation[key], 64) for key in ("liquidity", "debt")):
                raise ValueError("liquidity/debt must be int64 minor units")
            if observation.get("sector") not in ALLOWED_SECTORS | {None}:
                raise ValueError("unknown sector")
        if len(months) != 1 or min(months) <= previous_month:
            raise ValueError("one strictly increasing simulation month per step is required")
        previous_month = min(months)
        if set(contexts) != actors:
            raise ValueError("actor_contexts must match observed actors exactly")
        for actor_id, context in contexts.items():
            if not isinstance(context, dict) or set(context) != {"role", "constraints", "choices"}:
                raise ValueError(f"invalid context for {actor_id}")
            if not isinstance(context["role"], str) or not 0 < len(context["role"].strip()) <= 200:
                raise ValueError(f"invalid role for {actor_id}")
            constraints = context["constraints"]
            if not isinstance(constraints, list) or len(constraints) > 20 or any(not isinstance(item, str) or not 0 < len(item.strip()) <= 500 for item in constraints):
                raise ValueError(f"invalid constraints for {actor_id}")
            choices = context["choices"]
            if not isinstance(choices, list) or not 1 <= len(choices) <= 16:
                raise ValueError(f"actor {actor_id} needs 1..16 bounded choices")
            option_ids: set[str] = set()
            for choice in choices:
                if not isinstance(choice, dict) or set(choice) != {"id", "action"}:
                    raise ValueError(f"invalid choice for {actor_id}")
                option_id = choice["id"]
                if not isinstance(option_id, str) or not 0 < len(option_id.strip()) <= 80 or option_id in option_ids:
                    raise ValueError(f"duplicate or invalid option ID for {actor_id}")
                option_ids.add(option_id)
                validate_action(choice["action"], schema)
            calls += 1
    if not 1 <= max_calls <= 100_000 or calls > max_calls:
        raise ValueError(f"{calls} requested calls exceed configured max_calls={max_calls}")


def validate_server(server: str) -> str:
    if not isinstance(server, str) or any(ord(char) < 32 for char in server) or server != server.strip():
        raise ValueError("server must be a clean loopback HTTP origin")
    parsed = urlsplit(server)
    port = parsed.port
    if parsed.scheme != "http" or parsed.hostname not in {"127.0.0.1", "::1"} or parsed.username or parsed.password or parsed.path not in {"", "/"} or parsed.query or parsed.fragment or port is None or port == 0:
        raise ValueError("server must be an explicit loopback HTTP origin with a port")
    host = "[::1]" if parsed.hostname == "::1" else "127.0.0.1"
    return f"http://{host}:{port}"


def request_completion(request_data: dict[str, Any], server: str, timeout_seconds: float) -> dict[str, Any]:
    payload = canonical(request_data)
    request = urllib.request.Request(
        validate_server(server) + "/v1/chat/completions",
        data=payload, headers={"Content-Type": "application/json"}, method="POST",
    )
    # Never inherit a machine-wide HTTP proxy for an ostensibly local model.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), _NoRedirect())
    try:
        with opener.open(request, timeout=timeout_seconds) as response:
            body = response.read(MAX_RESPONSE_BYTES + 1)
    except urllib.error.HTTPError as exc:
        exc.close()
        raise
    if len(body) > MAX_RESPONSE_BYTES:
        raise ValueError("model response exceeds byte limit")
    result = strict_json(body.decode("utf-8"))
    if not isinstance(result, dict):
        raise ValueError("server response must be a JSON object")
    return result


def _proposal(raw: dict[str, Any], actor_id: str, choices: list[dict[str, Any]]) -> tuple[dict[str, Any], dict[str, Any]]:
    content = raw["choices"][0]["message"]["content"]
    if not isinstance(content, str) or len(content.encode("utf-8")) > 8192:
        raise ValueError("completion content must be a short string")
    answer = strict_json(content)
    if not isinstance(answer, dict) or set(answer) != RESPONSE_KEYS or answer["actor_id"] != actor_id:
        raise ValueError("completion keys or actor identity mismatch")
    options = {choice["id"]: choice["action"] for choice in choices}
    if type(answer["option_id"]) is not str or answer["option_id"] not in options:
        raise ValueError("unoffered option_id")
    for key in ("rationale", "expected_effect"):
        if not isinstance(answer[key], str) or not 0 < len(answer[key].strip()) <= 400:
            raise ValueError(f"{key} must be a short nonempty string")
    if not _number(answer["confidence"]) or not 0 <= answer["confidence"] <= 1:
        raise ValueError("confidence must be finite and in [0,1]")
    return answer, options[answer["option_id"]]


def generate_tape(
    spec: dict[str, Any], *, model_sha256: str, model_path: str,
    server: str, timeout_seconds: float = 30, max_calls: int = 32,
    request_fn: Callable[[dict[str, Any], str, float], dict[str, Any]] = request_completion,
) -> tuple[dict[str, Any], dict[str, Any]]:
    validate_server(server)
    if not _number(timeout_seconds) or not 0 < timeout_seconds <= 600:
        raise ValueError("timeout_seconds must be in (0,600]")
    if not re.fullmatch(r"[0-9a-f]{64}", model_sha256):
        raise ValueError("model_sha256 must be a lowercase SHA-256")
    schema = strict_json(SCHEMA.read_text(encoding="utf-8"))
    validate_spec(spec, schema, max_calls)
    adapter_sha = sha256_file(Path(__file__))
    schema_sha = sha256_file(SCHEMA)
    template_sha = sha256_bytes(PROMPT_TEMPLATE.encode("utf-8"))
    descriptor_input = {
        "adapter_sha256": adapter_sha, "model_sha256": model_sha256,
        "schema_sha256": schema_sha, "prompt_template_sha256": template_sha,
        "temperature": 0, "max_tokens": 192, "response_format": "json_object",
    }
    descriptor = {
        "id": "local-llm-discrete-menu", "version": "1",
        "sha256": sha256_bytes(canonical(descriptor_input)),
        "kind": "large_language_model", "deterministic": False,
        "requires_network": True, "maximum_latency_ms": int(timeout_seconds * 1000),
    }
    tape: dict[str, Any] = {"schema_version": "owg.decision-tape.v1", "id": spec["id"], "descriptor": descriptor, "steps": []}
    audit: dict[str, Any] = {
        "status": "bounded_model_proposals_not_forecast_validation",
        "transport_kind": "loopback_http" if request_fn is request_completion else "injected_callable",
        "input_spec_sha256": sha256_bytes(canonical(spec)),
        "model_path": model_path, "model_sha256": model_sha256,
        "server_origin": server, "timeout_seconds": timeout_seconds,
        "max_calls": max_calls,
        "adapter_sha256": adapter_sha, "schema_sha256": schema_sha,
        "prompt_template_sha256": template_sha, "descriptor_input": descriptor_input,
        "steps": [],
        "limitations": [
            "Actor authority, point-in-time source provenance, feasibility, and economic effects are not established by this adapter.",
            "Model weights may contain future historical knowledge; a dated prompt is not a clean historical holdout.",
            "Recorded rationales and expected effects are claims by the model, not causal evidence.",
        ],
    }
    for step_index, step in enumerate(spec["steps"]):
        actions: list[dict[str, Any]] = []
        rationales: list[str] = []
        effects: list[str] = []
        confidences: list[float] = []
        warnings: list[str] = []
        entries: list[dict[str, Any]] = []
        for observation in step["observations"]["observations"]:
            actor_id = observation["actor_id"]
            context = step["actor_contexts"][actor_id]
            prompt_input = {
                "as_of": step["as_of"], "actor_id": actor_id,
                "observation": observation, "role": context["role"],
                "constraints": context["constraints"], "choices": context["choices"],
            }
            messages = [
                {"role": "system", "content": PROMPT_TEMPLATE},
                {"role": "user", "content": canonical(prompt_input).decode("utf-8")},
            ]
            seed = int(sha256_bytes(f"{spec['id']}:{step_index}:{actor_id}".encode())[:8], 16)
            request_data = {
                "model": "owg-qwen-local", "messages": messages,
                "temperature": 0, "seed": seed, "max_tokens": 192,
                "response_format": {"type": "json_object"}, "stream": False,
            }
            entry: dict[str, Any] = {
                "actor_id": actor_id, "as_of": step["as_of"],
                "observation_sha256": sha256_bytes(canonical(observation)),
                "prompt_sha256": sha256_bytes(canonical(messages)),
                "request_sha256": sha256_bytes(canonical(request_data)),
                "request": request_data,
            }
            try:
                raw = request_fn(request_data, server, timeout_seconds)
                if not isinstance(raw, dict) or len(canonical(raw)) > MAX_RESPONSE_BYTES:
                    raise ValueError("server response must be a bounded JSON object")
                entry["raw_response"] = raw
                entry["response_sha256"] = sha256_bytes(canonical(raw))
                answer, action = _proposal(raw, actor_id, context["choices"])
                entry["status"] = "accepted"
                entry["model_proposal"] = answer
                entry["selected_action"] = action
                rationale = answer["rationale"].strip()
                effect = answer["expected_effect"].strip()
                confidence = float(answer["confidence"])
            except (ValueError, KeyError, IndexError, TypeError, OSError, urllib.error.URLError, TimeoutError) as exc:
                action = {"type": "no_op"}
                rationale = "Safe no-op fallback after rejected or unavailable model proposal."
                effect = "No direct actor action applied by this policy step."
                confidence = 0.0
                warning = f"{actor_id}: fallback ({type(exc).__name__}: {str(exc)[:160]})"
                warnings.append(warning)
                entry["status"] = "fallback"
                entry["failure"] = warning
                entry["selected_action"] = action
            actions.append(action)
            rationales.append(rationale)
            effects.append(effect)
            confidences.append(confidence)
            entries.append(entry)
        tape["steps"].append({
            "observations": step["observations"],
            "actions": {"actions": actions, "warnings": warnings, "used_fallback": bool(warnings)},
            "rationales": rationales, "expected_effects": effects, "confidences": confidences,
        })
        audit["steps"].append({"as_of": step["as_of"], "entries": entries})
    accepted_count = sum(entry["status"] == "accepted" for step in audit["steps"] for entry in step["entries"])
    fallback_count = sum(entry["status"] == "fallback" for step in audit["steps"] for entry in step["entries"])
    audit["accepted_count"] = accepted_count
    audit["fallback_count"] = fallback_count
    if accepted_count == 0:
        audit["status"] = "no_model_proposals_accepted"
    return tape, audit


def validate_with_rust(tape_path: Path, owg: Path = DEFAULT_OWG) -> str:
    if not owg.is_file():
        raise FileNotFoundError(f"Rust validator missing: {owg}; build `cargo build -p owg-cli` first")
    completed = subprocess.run(
        [str(owg), "validate", "--kind", "decision-tape", "--json", str(tape_path)],
        capture_output=True, text=True, check=False,
    )
    if completed.returncode != 0:
        raise ValueError(f"Rust decision-tape validation failed: {completed.stderr.strip()}")
    return completed.stdout.strip()


def write_bundle(tape: dict[str, Any], audit: dict[str, Any], tape_path: Path, audit_path: Path, owg: Path = DEFAULT_OWG) -> dict[str, Any]:
    if tape_path.resolve() == audit_path.resolve():
        raise ValueError("tape and audit paths must differ")
    if tape_path.exists() or audit_path.exists():
        raise FileExistsError("tape or audit already exists; choose new artifact paths")
    tape_path.parent.mkdir(parents=True, exist_ok=True)
    audit_path.parent.mkdir(parents=True, exist_ok=True)
    # Validate a temporary output first, leaving the named artifact untouched on failure.
    import tempfile
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", suffix=".json", prefix=".decision-tape-", dir=tape_path.parent, delete=True) as stream:
        stream.write(json.dumps(tape, indent=2, ensure_ascii=False, allow_nan=False) + "\n")
        stream.flush()
        rust_result = validate_with_rust(Path(stream.name), owg)
    tape_bytes = (json.dumps(tape, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode("utf-8")
    audit["tape_file_sha256"] = sha256_bytes(tape_bytes)
    audit["rust_validation"] = strict_json(rust_result)
    audit["rust_validator_sha256"] = sha256_file(owg)
    with tape_path.open("xb") as stream:
        stream.write(tape_bytes)
    with audit_path.open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(audit, indent=2, ensure_ascii=False, allow_nan=False) + "\n")
    return {
        "tape": str(tape_path.resolve()), "audit": str(audit_path.resolve()),
        "tape_file_sha256": audit["tape_file_sha256"],
        "transport_kind": audit["transport_kind"],
        "accepted_count": audit["accepted_count"], "fallback_count": audit["fallback_count"],
        "rust_validation": audit["rust_validation"],
    }


def verify_bundle(tape_path: Path, audit_path: Path, owg: Path = DEFAULT_OWG) -> dict[str, Any]:
    tape = strict_json(tape_path.read_text(encoding="utf-8"))
    audit = strict_json(audit_path.read_text(encoding="utf-8"))
    if sha256_file(tape_path) != audit["tape_file_sha256"]:
        raise ValueError("tape file checksum mismatch")
    descriptor_input = audit["descriptor_input"]
    if (
        sha256_bytes(canonical(descriptor_input)) != tape["descriptor"]["sha256"]
        or descriptor_input["model_sha256"] != audit["model_sha256"]
        or descriptor_input["adapter_sha256"] != audit["adapter_sha256"]
        or descriptor_input["schema_sha256"] != audit["schema_sha256"]
        or descriptor_input["prompt_template_sha256"] != audit["prompt_template_sha256"]
    ):
        raise ValueError("descriptor/provenance mismatch")
    if len(tape["steps"]) != len(audit["steps"]):
        raise ValueError("tape/audit step count mismatch")
    count = 0
    reconstructed_steps: list[dict[str, Any]] = []
    for step, logged in zip(tape["steps"], audit["steps"]):
        observations = step["observations"]["observations"]
        entries = logged["entries"]
        if len(observations) != len(entries):
            raise ValueError("tape/audit actor count mismatch")
        actor_contexts: dict[str, Any] = {}
        fallback_count = 0
        for index, (observation, entry) in enumerate(zip(observations, entries)):
            if entry["actor_id"] != observation["actor_id"] or sha256_bytes(canonical(observation)) != entry["observation_sha256"]:
                raise ValueError("observation/audit mismatch")
            if sha256_bytes(canonical(entry["request"])) != entry["request_sha256"] or sha256_bytes(canonical(entry["request"]["messages"])) != entry["prompt_sha256"]:
                raise ValueError("request/prompt checksum mismatch")
            if sha256_bytes(entry["request"]["messages"][0]["content"].encode("utf-8")) != audit["prompt_template_sha256"]:
                raise ValueError("prompt template checksum mismatch")
            prompt_input = strict_json(entry["request"]["messages"][1]["content"])
            if (
                prompt_input["observation"] != observation
                or prompt_input["actor_id"] != observation["actor_id"]
                or prompt_input["as_of"] != logged["as_of"]
                or entry["as_of"] != logged["as_of"]
            ):
                raise ValueError("prompt observation or time mismatch")
            actor_contexts[observation["actor_id"]] = {
                "role": prompt_input["role"],
                "constraints": prompt_input["constraints"],
                "choices": prompt_input["choices"],
            }
            if "raw_response" in entry and sha256_bytes(canonical(entry["raw_response"])) != entry["response_sha256"]:
                raise ValueError("response checksum mismatch")
            if step["actions"]["actions"][index] != entry["selected_action"]:
                raise ValueError("selected action mismatch")
            if entry["status"] == "accepted":
                proposal, offered_action = _proposal(entry["raw_response"], observation["actor_id"], prompt_input["choices"])
                if (
                    offered_action != entry["selected_action"]
                    or proposal != entry["model_proposal"]
                    or step["rationales"][index] != proposal["rationale"].strip()
                    or step["expected_effects"][index] != proposal["expected_effect"].strip()
                    or step["confidences"][index] != float(proposal["confidence"])
                ):
                    raise ValueError("accepted model proposal/audit mismatch")
            elif entry["status"] == "fallback":
                fallback_count += 1
                if entry["selected_action"] != {"type": "no_op"} or step["confidences"][index] != 0.0 or not step["actions"]["used_fallback"]:
                    raise ValueError("fallback/audit mismatch")
            else:
                raise ValueError("unknown audit entry status")
            count += 1
        if step["actions"]["used_fallback"] != (fallback_count > 0) or len(step["actions"]["warnings"]) != fallback_count:
            raise ValueError("fallback warning count mismatch")
        reconstructed_steps.append({"as_of": logged["as_of"], "observations": step["observations"], "actor_contexts": actor_contexts})
    reconstructed = {"id": tape["id"], "steps": reconstructed_steps}
    if sha256_bytes(canonical(reconstructed)) != audit["input_spec_sha256"]:
        raise ValueError("input spec checksum mismatch")
    accepted_count = count - sum(entry["status"] == "fallback" for step in audit["steps"] for entry in step["entries"])
    fallback_count = count - accepted_count
    if accepted_count != audit["accepted_count"] or fallback_count != audit["fallback_count"]:
        raise ValueError("accepted/fallback count mismatch")
    expected_status = "no_model_proposals_accepted" if accepted_count == 0 else "bounded_model_proposals_not_forecast_validation"
    if audit["status"] != expected_status:
        raise ValueError("audit status mismatch")
    rust_result = strict_json(validate_with_rust(tape_path, owg))
    same_validator = sha256_file(owg) == audit["rust_validator_sha256"]
    if same_validator and rust_result != audit["rust_validation"]:
        raise ValueError("same-build Rust validation result mismatch")
    return {
        "status": "recorded_bundle_verified_no_model_calls", "actors": count,
        "transport_kind": audit["transport_kind"],
        "accepted_count": accepted_count, "fallback_count": fallback_count,
        "same_rust_validator_binary": same_validator,
        "rust_validation": rust_result,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, help="timestamped observation and finite-choice JSON")
    parser.add_argument("--model", type=Path, default=DEFAULT_MODEL)
    parser.add_argument("--server", default="http://127.0.0.1:18082")
    parser.add_argument("--output", type=Path, help="DecisionTape JSON destination")
    parser.add_argument("--audit", type=Path, help="inference provenance sidecar JSON")
    parser.add_argument("--owg", type=Path, default=DEFAULT_OWG)
    parser.add_argument("--timeout-seconds", type=float, default=30)
    parser.add_argument("--max-calls", type=int, default=32)
    parser.add_argument("--verify-tape", type=Path, help="verify a saved tape without model calls")
    parser.add_argument("--verify-audit", type=Path, help="audit sidecar for --verify-tape")
    args = parser.parse_args()
    if args.verify_tape or args.verify_audit:
        if not args.verify_tape or not args.verify_audit:
            parser.error("--verify-tape and --verify-audit are required together")
        print(json.dumps(verify_bundle(args.verify_tape, args.verify_audit, args.owg), indent=2))
        return 0
    if not args.input or not args.output or not args.audit:
        parser.error("--input, --output, and --audit are required for generation")
    if not args.model.is_file():
        parser.error(f"model file missing: {args.model}")
    spec = strict_json(args.input.read_text(encoding="utf-8"))
    validate_server(args.server)
    validate_spec(spec, strict_json(SCHEMA.read_text(encoding="utf-8")), args.max_calls)
    tape, audit = generate_tape(
        spec, model_sha256=sha256_file(args.model), model_path=str(args.model.resolve()),
        server=args.server, timeout_seconds=args.timeout_seconds, max_calls=args.max_calls,
    )
    print(json.dumps(write_bundle(tape, audit, args.output, args.audit, args.owg), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
