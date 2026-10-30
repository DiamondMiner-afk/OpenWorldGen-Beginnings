"""Versioned research-side contracts.

The Rust-generated JSON Schema remains authoritative for ``ScenarioSpec``.
These Python objects provide safe construction and provenance handling without
duplicating every Rust enum.
"""

from __future__ import annotations

from dataclasses import dataclass, field
import hashlib
import json
from pathlib import Path
from typing import Any, Mapping, Self


@dataclass(frozen=True, slots=True)
class Provenance:
    sources: tuple[str, ...]
    licence: str
    retrieval_date: str
    checksum_sha256: str
    confidentiality: str
    transformations: tuple[str, ...] = ()
    imputations: tuple[str, ...] = ()

    def to_dict(self) -> dict[str, Any]:
        return {
            "sources": list(self.sources),
            "licence": self.licence,
            "retrieval_date": self.retrieval_date,
            "checksum_sha256": self.checksum_sha256,
            "confidentiality": self.confidentiality,
            "transformations": list(self.transformations),
            "imputations": list(self.imputations),
        }


@dataclass(frozen=True, slots=True)
class DataTable:
    id: str
    relative_path: str
    format: str
    sha256: str
    row_count: int | None
    units: Mapping[str, str]
    classifications: Mapping[str, str] = field(default_factory=dict)
    geography: str = ""

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "relative_path": self.relative_path,
            "format": self.format,
            "sha256": self.sha256,
            "row_count": self.row_count,
            "units": dict(sorted(self.units.items())),
            "classifications": dict(sorted(self.classifications.items())),
            "geography": self.geography,
        }


@dataclass(frozen=True, slots=True)
class DataBundle:
    schema_version: str
    id: str
    version: str
    currency: str
    base_year: int
    tables: tuple[DataTable, ...]
    provenance: Provenance

    def to_dict(self) -> dict[str, Any]:
        """Return the exact Rust ``DataBundle`` JSON field shape."""
        return {
            "schema_version": self.schema_version,
            "id": self.id,
            "version": self.version,
            "currency": self.currency,
            "base_year": self.base_year,
            "tables": [table.to_dict() for table in self.tables],
            "provenance": self.provenance.to_dict(),
        }

    @classmethod
    def load(cls, path: str | Path) -> Self:
        value = json.loads(Path(path).read_text(encoding="utf-8"))
        if not isinstance(value, dict):
            raise ValueError("data bundle root must be an object")
        tables = value.get("tables")
        provenance = value.get("provenance")
        if not isinstance(tables, list) or not isinstance(provenance, dict):
            raise ValueError("data bundle tables or provenance are malformed")
        return cls(
            schema_version=str(value["schema_version"]),
            id=str(value["id"]),
            version=str(value["version"]),
            currency=str(value["currency"]),
            base_year=int(value["base_year"]),
            tables=tuple(
                DataTable(
                    id=str(table["id"]),
                    relative_path=str(table["relative_path"]),
                    format=str(table["format"]),
                    sha256=str(table["sha256"]),
                    row_count=(
                        None
                        if table.get("row_count") is None
                        else int(table["row_count"])
                    ),
                    units=dict(table["units"]),
                    classifications=dict(table.get("classifications", {})),
                    geography=str(table["geography"]),
                )
                for table in tables
            ),
            provenance=Provenance(
                sources=tuple(str(source) for source in provenance["sources"]),
                licence=str(provenance["licence"]),
                retrieval_date=str(provenance["retrieval_date"]),
                checksum_sha256=str(provenance["checksum_sha256"]),
                confidentiality=str(provenance["confidentiality"]),
                transformations=tuple(
                    str(item) for item in provenance.get("transformations", ())
                ),
                imputations=tuple(
                    str(item) for item in provenance.get("imputations", ())
                ),
            ),
        )

    def canonical_sha256(self) -> str:
        """Hash the same ordered JSON object used by the Rust contract."""
        payload = json.dumps(
            self.to_dict(),
            separators=(",", ":"),
            ensure_ascii=False,
        ).encode("utf-8")
        return hashlib.sha256(payload).hexdigest()


@dataclass(slots=True)
class ScenarioDocument:
    """Lossless JSON scenario document governed by the Rust JSON Schema."""

    value: dict[str, Any]

    @classmethod
    def load(cls, path: str | Path) -> Self:
        source = Path(path)
        if source.suffix.lower() != ".json":
            raise ValueError(
                "ScenarioDocument handles JSON; pass YAML paths directly to OpenWorldGen"
            )
        value = json.loads(source.read_text(encoding="utf-8"))
        if not isinstance(value, dict):
            raise ValueError("scenario root must be an object")
        return cls(value=value)

    @classmethod
    def from_mapping(cls, value: Mapping[str, Any]) -> Self:
        detached = json.loads(json.dumps(value))
        if not isinstance(detached, dict):
            raise ValueError("scenario root must be an object")
        return cls(value=detached)

    def save(self, path: str | Path) -> Path:
        destination = Path(path)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(
            json.dumps(self.value, indent=2, sort_keys=False) + "\n",
            encoding="utf-8",
        )
        return destination

    def canonical_sha256(self) -> str:
        payload = json.dumps(
            self.value,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
        ).encode("utf-8")
        return hashlib.sha256(payload).hexdigest()
