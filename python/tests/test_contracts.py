from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest

from openworldgen.contracts import DataBundle, DataTable, Provenance, ScenarioDocument


class ContractTests(unittest.TestCase):
    def test_scenario_round_trip_is_stable(self) -> None:
        scenario = ScenarioDocument.from_mapping(
            {"schema_version": "owg.scenario.v1", "root_seed": 7}
        )
        with tempfile.TemporaryDirectory() as directory:
            path = scenario.save(Path(directory) / "scenario.json")
            loaded = ScenarioDocument.load(path)
        self.assertEqual(loaded.value, scenario.value)
        self.assertEqual(loaded.canonical_sha256(), scenario.canonical_sha256())

    def test_data_bundle_hash_covers_provenance(self) -> None:
        table = DataTable(
            id="prices",
            relative_path="prices.parquet",
            format="parquet",
            sha256="1" * 64,
            row_count=None,
            units={"cpi": "index_2022_100"},
        )
        provenance = Provenance(
            sources=("synthetic",),
            licence="internal",
            retrieval_date="2026-07-24",
            checksum_sha256="0" * 64,
            confidentiality="public-synthetic",
        )
        bundle = DataBundle(
            schema_version="owg.data-bundle.v1",
            id="test",
            version="0.1.0",
            currency="OWG",
            base_year=2022,
            tables=(table,),
            provenance=provenance,
        )
        encoded = json.dumps(bundle.to_dict())
        self.assertIn("prices.parquet", encoded)
        self.assertEqual(len(bundle.canonical_sha256()), 64)

    def test_repository_data_bundle_hash_matches_rust(self) -> None:
        root = Path(__file__).resolve().parents[2]
        bundle = DataBundle.load(
            root / "data" / "synthetic-balanced-v0.1" / "data-bundle.json"
        )
        self.assertEqual(
            bundle.canonical_sha256(),
            "fecb688f45e18c1c10fcd2418247b0f72e0d754134e332399347b8350a420d35",
        )


if __name__ == "__main__":
    unittest.main()
