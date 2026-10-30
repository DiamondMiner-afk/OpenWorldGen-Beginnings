"""Hostile local-integrity checks for the research evidence register."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from audit_research_state import audit


class ResearchStateAuditTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.content = b"bounded evidence\n"
        (self.root / "note.md").write_bytes(self.content)
        self.state = {
            "reproducibility": {
                "artifact_hashes": {
                    "note.md": hashlib.sha256(self.content).hexdigest(),
                }
            },
            "evidence": [{"claim_id": "C-ONE", "artifact_path": "note.md"}],
        }
        self.save_state()

    def save_state(self) -> None:
        (self.root / "research_state.json").write_text(
            json.dumps(self.state), encoding="utf-8"
        )

    def test_valid_register_and_mutated_bytes(self) -> None:
        self.assertTrue(audit(self.root)["passed"])
        (self.root / "note.md").write_bytes(b"changed\n")
        result = audit(self.root)
        self.assertFalse(result["passed"])
        self.assertIn("artifact hash mismatch: note.md", result["failures"])

    def test_rejects_path_escape_and_symlink_even_with_matching_hash(self) -> None:
        self.state["reproducibility"]["artifact_hashes"] = {
            "../outside.md": hashlib.sha256(self.content).hexdigest()
        }
        self.state["evidence"][0]["artifact_path"] = "../outside.md"
        self.save_state()
        self.assertFalse(audit(self.root)["passed"])

        (self.root / "linked.md").symlink_to(self.root / "note.md")
        self.state["reproducibility"]["artifact_hashes"] = {
            "linked.md": hashlib.sha256(self.content).hexdigest()
        }
        self.state["evidence"][0]["artifact_path"] = "linked.md"
        self.save_state()
        result = audit(self.root)
        self.assertIn("missing or unsafe artifact path: linked.md", result["failures"])

    def test_rejects_unregistered_or_duplicate_evidence(self) -> None:
        self.state["evidence"].append(
            {"claim_id": "C-ONE", "artifact_path": "unhashed.md"}
        )
        self.save_state()
        result = audit(self.root)
        self.assertIn("missing or duplicate claim_id: 'C-ONE'", result["failures"])
        self.assertIn("evidence has no hashed artifact: 'C-ONE'", result["failures"])


if __name__ == "__main__":
    unittest.main()
