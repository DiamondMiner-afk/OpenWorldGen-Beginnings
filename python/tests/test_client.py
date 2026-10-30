from __future__ import annotations

import json
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from openworldgen import OpenWorldGen, OpenWorldGenError, ResultBundle


class ClientTests(unittest.TestCase):
    def test_validate_parses_machine_output_without_shell(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "owg"
            fake.write_text(
                "#!/bin/sh\n"
                "printf '%s\\n' '{\"valid\":true,\"scenario_id\":\"s\","
                "\"scenario_hash\":\"abc\",\"issues\":[]}'\n",
                encoding="utf-8",
            )
            fake.chmod(fake.stat().st_mode | stat.S_IXUSR)
            scenario = root / "scenario.json"
            scenario.write_text(json.dumps({}), encoding="utf-8")

            result = OpenWorldGen(fake).validate(scenario)

        self.assertTrue(result.valid)
        self.assertEqual(result.scenario_id, "s")
        self.assertEqual(result.scenario_hash, "abc")

    def test_result_bundle_exposes_accounting_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "bundle.json").write_text(
                json.dumps({"accounting": "accounting.json"}),
                encoding="utf-8",
            )
            (root / "accounting.json").write_text(
                json.dumps(
                    [
                        {
                            "branch_id": "baseline",
                            "max_income_identity_relative_error": 0.0,
                        }
                    ]
                ),
                encoding="utf-8",
            )
            accounting = ResultBundle(root).accounting

        self.assertEqual(accounting[0]["branch_id"], "baseline")

    def test_result_bundle_exposes_declared_visualization(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "bundle.json").write_text(
                json.dumps({"visualization": "plots.html"}),
                encoding="utf-8",
            )
            visualization = ResultBundle(root).visualization

        self.assertEqual(visualization, root / "plots.html")

    def test_result_bundle_rejects_visualization_path_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "bundle.json").write_text(
                json.dumps({"visualization": "../plots.html"}),
                encoding="utf-8",
            )
            with self.assertRaises(OpenWorldGenError):
                _ = ResultBundle(root).visualization

    def test_capabilities_returns_machine_readable_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "owg"
            fake.write_text(
                "#!/bin/sh\n"
                "printf '%s\\n' '{\"capabilities_version\":\"owg.capabilities.v1\","
                "\"gpu_required\":false}'\n",
                encoding="utf-8",
            )
            fake.chmod(fake.stat().st_mode | stat.S_IXUSR)
            capabilities = OpenWorldGen(fake).capabilities()

        self.assertEqual(
            capabilities["capabilities_version"], "owg.capabilities.v1"
        )
        self.assertFalse(capabilities["gpu_required"])

    def test_plot_invokes_engine_without_a_shell(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "owg"
            fake.write_text("", encoding="utf-8")
            output = root / "plots.html"
            with patch(
                "openworldgen.client.subprocess.run",
                return_value=subprocess.CompletedProcess([], 0, "", ""),
            ) as run:
                result = OpenWorldGen(fake).plot(root / "bundle", output=output)

        self.assertEqual(result, output)
        command = run.call_args.args[0]
        self.assertEqual(
            command[1:],
            ["plot", str(root / "bundle"), "--output", str(output)],
        )
        self.assertFalse(run.call_args.kwargs["shell"])


if __name__ == "__main__":
    unittest.main()
