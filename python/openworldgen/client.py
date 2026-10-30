"""Safe subprocess client for the OpenWorldGen Rust engine."""

from __future__ import annotations

from dataclasses import dataclass
import csv
import json
import os
from pathlib import Path
import shutil
import subprocess
from typing import Any, Iterable, Mapping, Sequence


class OpenWorldGenError(RuntimeError):
    """Base exception raised by the SDK."""


class CommandFailed(OpenWorldGenError):
    """The engine rejected a command or failed an invariant."""

    def __init__(self, command: Sequence[str], returncode: int, stderr: str):
        super().__init__(
            f"OpenWorldGen command failed with exit code {returncode}: {stderr.strip()}"
        )
        self.command = tuple(command)
        self.returncode = returncode
        self.stderr = stderr


@dataclass(frozen=True, slots=True)
class ValidationResult:
    valid: bool
    scenario_id: str | None
    scenario_hash: str | None
    issues: tuple[Mapping[str, Any], ...]


@dataclass(frozen=True, slots=True)
class ResultBundle:
    path: Path

    @property
    def manifest(self) -> Mapping[str, Any]:
        return _read_object(self.path / "manifest.json")

    @property
    def comparison(self) -> Mapping[str, Any]:
        return _read_object(self.path / "comparison.json")

    @property
    def mechanisms(self) -> list[Mapping[str, Any]]:
        value = json.loads((self.path / "mechanisms.json").read_text(encoding="utf-8"))
        if not isinstance(value, list) or not all(
            isinstance(item, dict) for item in value
        ):
            raise OpenWorldGenError(
                f"expected JSON array of objects in {self.path / 'mechanisms.json'}"
            )
        return value

    @property
    def accounting(self) -> list[Mapping[str, Any]]:
        relative = self.package.get("accounting")
        if relative is None:
            return []
        if not isinstance(relative, str) or not relative:
            raise OpenWorldGenError("bundle accounting path must be a non-empty string")
        value = json.loads((self.path / relative).read_text(encoding="utf-8"))
        if not isinstance(value, list) or not all(
            isinstance(item, dict) for item in value
        ):
            raise OpenWorldGenError(
                f"expected JSON array of objects in {self.path / 'accounting.json'}"
            )
        return value

    @property
    def package(self) -> Mapping[str, Any]:
        return _read_object(self.path / "bundle.json")

    @property
    def visualization(self) -> Path | None:
        relative = self.package.get("visualization")
        if relative is None:
            return None
        if not isinstance(relative, str) or not relative:
            raise OpenWorldGenError(
                "bundle visualization path must be a non-empty string"
            )
        relative_path = Path(relative)
        if relative_path.is_absolute() or ".." in relative_path.parts:
            raise OpenWorldGenError("bundle visualization path must stay inside the bundle")
        return self.path / relative_path

    def metric_rows(self) -> list[dict[str, str]]:
        """Read metrics without an optional dataframe dependency."""
        with (self.path / "metrics.csv").open(
            "r", encoding="utf-8", newline=""
        ) as source:
            return list(csv.DictReader(source))


class OpenWorldGen:
    """Python control surface for the versioned ``owg`` executable."""

    def __init__(self, binary: str | Path | None = None):
        self.binary = _resolve_binary(binary)

    def capabilities(self) -> Mapping[str, Any]:
        completed = self._run("capabilities", "--json")
        value = json.loads(completed.stdout)
        if not isinstance(value, dict):
            raise OpenWorldGenError("expected capabilities JSON object")
        return value

    def validate(
        self, scenario: str | Path, *, kind: str = "scenario"
    ) -> ValidationResult:
        completed = self._run(
            "validate", str(Path(scenario)), "--kind", kind, "--json"
        )
        payload = json.loads(completed.stdout)
        return ValidationResult(
            valid=bool(payload["valid"]),
            scenario_id=payload.get("scenario_id") or payload.get("id"),
            scenario_hash=payload.get("scenario_hash") or payload.get("sha256"),
            issues=tuple(payload.get("issues", ())),
        )

    def verify_bundle(self, bundle: str | Path) -> ValidationResult:
        return self.validate(bundle, kind="result-bundle")

    def init_scenario(self, output: str | Path) -> Path:
        destination = Path(output)
        self._run("init-scenario", "--output", str(destination))
        return destination

    def write_schema(self, output: str | Path, *, kind: str = "scenario") -> Path:
        destination = Path(output)
        self._run("schema", "--kind", kind, "--output", str(destination))
        return destination

    def run(
        self,
        scenario: str | Path,
        *,
        output: str | Path,
        branches: Iterable[str] = (),
        replications: int = 1,
        end_month: int | None = None,
    ) -> ResultBundle:
        args = ["run", str(Path(scenario))]
        for branch in branches:
            args.extend(("--branch", branch))
        args.extend(("--replications", str(replications)))
        if end_month is not None:
            args.extend(("--end-month", str(end_month)))
        destination = Path(output)
        args.extend(("--output", str(destination)))
        self._run(*args)
        return ResultBundle(destination)

    def compare(
        self,
        scenario: str | Path,
        *,
        output: str | Path,
        replications: int | None = None,
    ) -> ResultBundle:
        args = ["compare", str(Path(scenario))]
        if replications is not None:
            args.extend(("--replications", str(replications)))
        destination = Path(output)
        args.extend(("--output", str(destination)))
        self._run(*args)
        return ResultBundle(destination)

    def checkpoint(
        self,
        scenario: str | Path,
        *,
        output: str | Path,
        month: int,
        branch: str = "baseline",
        replication: int = 0,
    ) -> Path:
        destination = Path(output)
        self._run(
            "checkpoint",
            str(Path(scenario)),
            "--branch",
            branch,
            "--replication",
            str(replication),
            "--month",
            str(month),
            "--output",
            str(destination),
        )
        return destination

    def resume(
        self,
        scenario: str | Path,
        checkpoint: str | Path,
        *,
        output: str | Path,
        end_month: int | None = None,
    ) -> Path:
        destination = Path(output)
        args = ["resume", str(Path(scenario)), str(Path(checkpoint))]
        if end_month is not None:
            args.extend(("--end-month", str(end_month)))
        args.extend(("--output", str(destination)))
        self._run(*args)
        return destination

    def calibrate(self, scenario: str | Path, *, output: str | Path) -> Path:
        destination = Path(output)
        self._run("calibrate", str(Path(scenario)), "--output", str(destination))
        return destination

    def benchmark(
        self,
        scenario: str | Path,
        *,
        months: int | None = None,
        output: str | Path | None = None,
    ) -> Mapping[str, Any]:
        args = ["benchmark", str(Path(scenario))]
        if months is not None:
            args.extend(("--months", str(months)))
        if output is not None:
            args.extend(("--output", str(Path(output))))
        completed = self._run(*args)
        return json.loads(completed.stdout)

    def report(self, bundle: str | Path, *, output: str | Path) -> Path:
        destination = Path(output)
        self._run("report", str(Path(bundle)), "--output", str(destination))
        return destination

    def plot(self, bundle: str | Path, *, output: str | Path) -> Path:
        destination = Path(output)
        self._run("plot", str(Path(bundle)), "--output", str(destination))
        return destination

    def _run(self, *args: str) -> subprocess.CompletedProcess[str]:
        command = [str(self.binary), *args]
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            shell=False,
        )
        if completed.returncode != 0:
            raise CommandFailed(command, completed.returncode, completed.stderr)
        return completed


def _resolve_binary(binary: str | Path | None) -> Path:
    candidate = binary or os.environ.get("OWG_BIN")
    if candidate is not None:
        path = Path(candidate).expanduser().resolve()
        if not path.is_file():
            raise OpenWorldGenError(f"owg executable not found: {path}")
        return path
    located = shutil.which("owg")
    if located is None:
        raise OpenWorldGenError(
            "owg executable not found; install it or set OWG_BIN"
        )
    return Path(located).resolve()


def _read_object(path: Path) -> Mapping[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise OpenWorldGenError(f"expected JSON object in {path}")
    return value
