import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("extract_scf_2004_bulletin.py")
SPEC = importlib.util.spec_from_file_location("extract_scf_2004_bulletin", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ScfBulletinExtractionTests(unittest.TestCase):
    def test_published_anchors_and_cross_checks(self):
        root = SCRIPT.parents[1]
        artifact = MODULE.extract(root / MODULE.PDF_RELATIVE)
        self.assertEqual(artifact["wealth_percentile_cutoffs_2004_usd"]["25"], 13300.0)
        self.assertEqual(artifact["wealth_percentile_cutoffs_2004_usd"]["90"], 831600.0)
        self.assertEqual(artifact["table_3_all_families"]["mean_thousand_2004_usd"], 448.2)
        self.assertEqual(
            artifact["table_3_within_wealth_percentile_groups"]["Less than 25"]["mean_thousand_2004_usd"],
            -1.4,
        )
        self.assertTrue(artifact["checks"]["all_family_mean_matches_group_mean_within_rounding"])

    def test_missing_table_fails_closed(self):
        with self.assertRaises(ValueError):
            MODULE.parse_bulletin_text("No Table 3")


if __name__ == "__main__":
    unittest.main()
