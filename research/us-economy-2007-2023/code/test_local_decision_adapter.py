"""Hostile, model-free tests for the bounded local decision-tape adapter."""

from copy import deepcopy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
import urllib.error

import local_decision_adapter as adapter


def fixture() -> dict:
    return {
        "id": "fictional-two-actor-month-2",
        "steps": [{
            "as_of": "2020-02-01T00:00:00Z",
            "observations": {"observations": [
                {
                    "actor_id": "firm-1", "month": 2, "inflation": 0.02,
                    "unemployment": 0.05, "policy_rate": 0.04,
                    "liquidity": 100_000, "debt": 20_000,
                    "inventory": 5.0, "expected_demand": 6.0,
                    "sector": "manufacturing",
                },
                {
                    "actor_id": "government-1", "month": 2, "inflation": 0.02,
                    "unemployment": 0.05, "policy_rate": 0.04,
                    "liquidity": 1_000_000, "debt": 5_000_000,
                    "inventory": 0.0, "expected_demand": 0.0,
                    "sector": None,
                },
            ]},
            "actor_contexts": {
                "firm-1": {
                    "role": "fictional manufacturing executive",
                    "constraints": ["No coordination with rivals."],
                    "choices": [
                        {"id": "hold", "action": {"type": "no_op"}},
                        {"id": "invest-small", "action": {
                            "type": "firm_plan", "price_change": 0.01,
                            "production_change": 0.02, "vacancy_change": 1,
                            "investment": 10_000,
                        }},
                    ],
                },
                "government-1": {
                    "role": "fictional elected official",
                    "constraints": ["Any proposal still needs the external legal gate."],
                    "choices": [
                        {"id": "hold", "action": {"type": "no_op"}},
                        {"id": "transfer", "action": {
                            "type": "policy_proposal", "transfer": 50_000,
                            "public_spending": None, "policy_rate_basis_points": None,
                        }},
                    ],
                },
            },
        }],
    }


def completion(actor_id: str, option_id: str, confidence: float = 0.6) -> dict:
    content = {
        "actor_id": actor_id, "option_id": option_id,
        "rationale": "The offered action fits the visible constraint.",
        "expected_effect": "The proposal may change near-term spending.",
        "confidence": confidence,
    }
    return {"choices": [{"message": {"content": json.dumps(content)}}], "usage": {"completion_tokens": 35}}


class LocalDecisionAdapterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        self.model = self.folder / "fake-model.gguf"
        self.model.write_bytes(b"mock weights, not a real model")
        self.hash = adapter.sha256_file(self.model)
        self.spec = fixture()

    def generate(self, mock):
        return adapter.generate_tape(
            self.spec, model_sha256=self.hash, model_path=str(self.model),
            server="http://127.0.0.1:18082", request_fn=mock,
        )

    def test_two_actor_tape_passes_rust_validator_and_replays_without_model(self):
        called = []

        def mock(request, _server, _timeout):
            prompt = json.loads(request["messages"][1]["content"])
            called.append(prompt["actor_id"])
            return completion(prompt["actor_id"], "invest-small" if prompt["actor_id"] == "firm-1" else "transfer")

        tape, audit = self.generate(mock)
        self.assertEqual(called, ["firm-1", "government-1"])
        self.assertEqual(audit["transport_kind"], "injected_callable")
        self.assertEqual((audit["accepted_count"], audit["fallback_count"]), (2, 0))
        self.assertFalse(tape["steps"][0]["actions"]["used_fallback"])
        self.assertEqual(tape["steps"][0]["actions"]["actions"][0]["type"], "firm_plan")
        self.assertEqual(tape["steps"][0]["actions"]["actions"][1]["transfer"], 50_000)
        tape_path = self.folder / "decision-tape.json"
        audit_path = self.folder / "decision-audit.json"
        adapter.write_bundle(tape, audit, tape_path, audit_path)
        verified = adapter.verify_bundle(tape_path, audit_path)
        self.assertEqual(verified["actors"], 2)
        self.assertEqual(verified["status"], "recorded_bundle_verified_no_model_calls")
        self.assertTrue(verified["same_rust_validator_binary"])

    def test_unoffered_option_invalid_confidence_and_server_failure_fall_back(self):
        for case, response in enumerate((
            lambda *_: completion("firm-1", "secret-price-collusion"),
            lambda *_: completion("firm-1", "hold", confidence=float("nan")),
            lambda *_: completion("firm-1", "hold", confidence=10**1000),
            lambda *_: (_ for _ in ()).throw(ConnectionRefusedError("offline")),
        )):
            tape, audit = self.generate(response)
            step = tape["steps"][0]
            self.assertTrue(step["actions"]["used_fallback"])
            self.assertEqual(step["actions"]["actions"], [{"type": "no_op"}, {"type": "no_op"}])
            self.assertEqual(step["confidences"], [0.0, 0.0])
            self.assertEqual([entry["status"] for entry in audit["steps"][0]["entries"]], ["fallback", "fallback"])
            self.assertEqual(audit["status"], "no_model_proposals_accepted")
            self.assertEqual((audit["accepted_count"], audit["fallback_count"]), (0, 2))
            adapter.write_bundle(tape, audit, self.folder / f"fallback-tape-{case}.json", self.folder / f"fallback-audit-{case}.json")

    def test_invalid_spec_fails_before_any_model_request(self):
        called = []

        def mock(*args):
            called.append(args)
            return completion("firm-1", "hold")

        tampered = deepcopy(self.spec)
        tampered["steps"][0]["actor_contexts"]["firm-1"]["choices"][1]["action"]["investment"] = -1
        with self.assertRaisesRegex(ValueError, "investment"):
            adapter.generate_tape(tampered, model_sha256=self.hash, model_path=str(self.model), server="http://127.0.0.1:18082", request_fn=mock)
        self.assertEqual(called, [])

        tampered = deepcopy(self.spec)
        tampered["steps"][0]["actor_contexts"]["firm-1"]["choices"][1]["action"] = {"type": "invented_action"}
        with self.assertRaisesRegex(ValueError, "missing from generated schema"):
            adapter.generate_tape(tampered, model_sha256=self.hash, model_path=str(self.model), server="http://127.0.0.1:18082", request_fn=mock)
        self.assertEqual(called, [])

        tampered = deepcopy(self.spec)
        tampered["steps"][0]["observations"]["observations"][0]["unemployment"] = 1.2
        with self.assertRaisesRegex(ValueError, "semantic bounds"):
            adapter.generate_tape(tampered, model_sha256=self.hash, model_path=str(self.model), server="http://127.0.0.1:18082", request_fn=mock)
        self.assertEqual(called, [])

        tampered = deepcopy(self.spec)
        tampered["steps"][0]["observations"]["observations"][0]["inflation"] = 10**1000
        with self.assertRaisesRegex(ValueError, "nonfinite or nonnumeric"):
            adapter.generate_tape(tampered, model_sha256=self.hash, model_path=str(self.model), server="http://127.0.0.1:18082", request_fn=mock)
        self.assertEqual(called, [])

    def test_call_budget_and_timestamp_order_are_preflight_gates(self):
        with self.assertRaisesRegex(ValueError, "max_calls=1"):
            adapter.generate_tape(self.spec, model_sha256=self.hash, model_path=str(self.model), server="http://127.0.0.1:18082", max_calls=1, request_fn=lambda *_: None)
        second = deepcopy(self.spec["steps"][0])
        second["as_of"] = "2020-01-01T00:00:00Z"
        second["observations"]["observations"][0]["month"] = 3
        second["observations"]["observations"][1]["month"] = 3
        self.spec["steps"].append(second)
        with self.assertRaisesRegex(ValueError, "as_of timestamps must increase"):
            adapter.generate_tape(self.spec, model_sha256=self.hash, model_path=str(self.model), server="http://127.0.0.1:18082", request_fn=lambda *_: None)

    def test_json_and_loopback_hardening(self):
        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            adapter.strict_json('{"actor_id":"a","actor_id":"b"}')
        with self.assertRaisesRegex(ValueError, "invalid JSON number"):
            adapter.strict_json('{"confidence":NaN}')
        with self.assertRaisesRegex(ValueError, "loopback"):
            adapter.validate_server("https://example.com:443")
        with self.assertRaisesRegex(ValueError, "clean loopback"):
            adapter.validate_server("http://127.0.0.1:18082\n")
        self.assertEqual(adapter.validate_server("http://127.0.0.1:018082/"), "http://127.0.0.1:18082")

    def test_saved_tape_tampering_is_detected(self):
        tape, audit = self.generate(lambda request, *_: completion(json.loads(request["messages"][1]["content"])["actor_id"], "hold"))
        tape_path = self.folder / "tape.json"
        audit_path = self.folder / "audit.json"
        adapter.write_bundle(tape, audit, tape_path, audit_path)
        changed = json.loads(tape_path.read_text())
        changed["steps"][0]["actions"]["actions"][0] = {"type": "firm_plan", "price_change": 0.1, "production_change": 0.0, "vacancy_change": 0, "investment": 0}
        tape_path.write_text(json.dumps(changed))
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            adapter.verify_bundle(tape_path, audit_path)

    def test_audit_tampering_and_existing_output_are_rejected(self):
        tape, audit = self.generate(lambda request, *_: completion(json.loads(request["messages"][1]["content"])["actor_id"], "hold"))
        tape_path = self.folder / "tape.json"
        audit_path = self.folder / "audit.json"
        adapter.write_bundle(tape, audit, tape_path, audit_path)
        with self.assertRaises(FileExistsError):
            adapter.write_bundle(tape, audit, tape_path, audit_path)
        changed = json.loads(audit_path.read_text())
        changed["steps"][0]["entries"][0]["selected_action"] = {"type": "firm_plan", "price_change": 0, "production_change": 0, "vacancy_change": 0, "investment": 0}
        audit_path.write_text(json.dumps(changed))
        with self.assertRaisesRegex(ValueError, "selected action mismatch"):
            adapter.verify_bundle(tape_path, audit_path)

    def test_loopback_openai_compatible_transport_with_mock_server(self):
        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                if self.path != "/v1/chat/completions":
                    self.send_error(404)
                    return
                request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                actor_id = json.loads(request["messages"][1]["content"])["actor_id"]
                payload = json.dumps(completion(actor_id, "hold")).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, *_):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            tape, audit = adapter.generate_tape(
                self.spec, model_sha256=self.hash, model_path=str(self.model),
                server=f"http://127.0.0.1:{server.server_port}", timeout_seconds=2,
            )
            self.assertFalse(tape["steps"][0]["actions"]["used_fallback"])
            self.assertEqual([entry["status"] for entry in audit["steps"][0]["entries"]], ["accepted", "accepted"])
            self.assertEqual(audit["transport_kind"], "loopback_http")
            input_path = self.folder / "input.json"
            tape_path = self.folder / "cli-tape.json"
            audit_path = self.folder / "cli-audit.json"
            input_path.write_text(json.dumps(self.spec), encoding="utf-8")
            command = [
                sys.executable, str(Path(adapter.__file__)),
                "--input", str(input_path), "--model", str(self.model),
                "--server", f"http://127.0.0.1:{server.server_port}",
                "--output", str(tape_path), "--audit", str(audit_path),
            ]
            completed = subprocess.run(command, capture_output=True, text=True, check=True)
            summary = json.loads(completed.stdout)
            self.assertEqual(summary["tape_file_sha256"], adapter.sha256_file(tape_path))
            self.assertEqual((summary["accepted_count"], summary["fallback_count"]), (2, 0))
            self.assertEqual(adapter.verify_bundle(tape_path, audit_path)["actors"], 2)
        finally:
            server.shutdown()
            worker.join(timeout=3)
            server.server_close()

    def test_new_action_variant_requires_generated_schema(self):
        schema = adapter.strict_json(adapter.SCHEMA.read_text())
        supported = {
            part["properties"]["type"]["const"]
            for part in schema["$defs"]["AgentAction"]["oneOf"]
        }
        action = {"type": "owner_distribution", "amount": 100}
        if "owner_distribution" in supported:
            adapter.validate_action(action, schema)
        else:
            with self.assertRaisesRegex(ValueError, "missing from generated schema"):
                adapter.validate_action(action, schema)

    def test_loopback_server_redirect_cannot_escape_to_network(self):
        class RedirectHandler(BaseHTTPRequestHandler):
            def do_POST(self):
                self.send_response(302)
                self.send_header("Location", "https://example.com/untrusted-endpoint")
                self.end_headers()

            def log_message(self, *_):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), RedirectHandler)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            with self.assertRaises(urllib.error.HTTPError) as caught:
                adapter.request_completion({"messages": []}, f"http://127.0.0.1:{server.server_port}", 2)
            self.assertEqual(caught.exception.code, 302)
            caught.exception.close()
        finally:
            server.shutdown()
            worker.join(timeout=3)
            server.server_close()


if __name__ == "__main__":
    unittest.main()
