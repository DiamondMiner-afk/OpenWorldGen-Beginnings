import copy
import json
import shutil
import tempfile
import unittest
from pathlib import Path

import extract_z1_2006_financial_accounts as z1


class FinancialAccountsExtractionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.report, cls.history = z1.build()
        cls.tables = {}
        for table in cls.history["tables"]:
            number = int(table["table_id"].split(".")[1])
            cls.tables[number] = {
                "codes": [row["series_code"] for row in table["rows"]],
                "labels": [row["label_fragment"] for row in table["rows"]],
                "periods": table["periods"],
                "values": list(map(list, zip(*(row["values_millions"] for row in table["rows"])))),
            }

    def test_full_sources_crosscheck_and_artifacts_are_reproducible(self):
        self.assertEqual(self.report["table_count"], 64)
        self.assertEqual(self.report["table_row_count"], 1453)
        self.assertEqual(self.report["all_history_cells"], 318207)
        self.assertEqual(self.report["archived_pdf_displayed_cell_checks"], 15983)
        self.assertEqual(self.report["coded_pdf_annual_cell_checks"], 5812)
        self.assertEqual(self.report, json.loads(z1.REPORT.read_text()))
        self.assertEqual(self.history, json.loads(z1.HISTORY.read_text()))

    def test_discrepancy_and_negative_net_positions_are_preserved(self):
        discrepancy = self.report["reported_unidentified_claims_discrepancy"]
        self.assertEqual(discrepancy["value_millions"], -3827706)
        self.assertEqual(z1.value(self.tables, 209, 31, "2006:Q3"), -96345)
        self.assertEqual(len(self.report["instrument_marginals_2006_q3"]), 7)
        self.assertTrue(all(not row["bilateral_links_identified"]
                            for row in self.report["instrument_marginals_2006_q3"]))
        self.assertTrue(all(row["max_abs_residual_millions"] <= row["tolerance_millions"]
                            for row in self.report["identity_checks"]))

    def test_missing_sentinels_and_repeated_codes_keep_their_positions(self):
        table = z1.parse_prn(
            '"DATES" FL153020005.Q FL153020005.Q\n'
            '"200601" -5 -5\n"200602" " NA" " NA"\n'
        )
        self.assertEqual(table["values"], [[-5, -5], [None, None]])
        self.assertEqual(table["missing_cells"][0]["raw_token"], " NA")
        self.assertEqual(self.report["missing_cells"], 930)
        self.assertEqual(self.report["missing_raw_token_counts"], {" NA": 930})
        table["values"][0][1] = 0
        with self.assertRaisesRegex(ValueError, "same series disagrees"):
            z1.check_repeated_codes({100: table})

    def test_duplicate_missing_or_malformed_quarters_fail(self):
        for rows in (
            '"200601" 1\n"200601" 2',
            '"200601" 1\n"200603" 2',
            '"200605" 1',
            '"200601" 1 2',
            '"200601" unknown',
        ):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                z1.parse_prn('"DATES" FL153020005.Q\n' + rows)

    def test_code_swap_is_rejected_even_when_values_are_identical(self):
        table = {"codes": ["FL153020005.Q"], "periods": ["2006:Q3"], "values": [[0]]}
        printed = {"rows": {1: {"code": "FL153030005.Q", "values": ["0.0"]}}}
        with self.assertRaisesRegex(ValueError, "series/row mismatch"):
            z1.check_pdf_cells(table, printed, ("2006:Q3",), coded=True)
        printed["rows"][1]["code"] = "FL153020005.Q"
        printed["rows"][1]["values"] = ["1.0"]
        with self.assertRaisesRegex(ValueError, "PDF/ZIP mismatch"):
            z1.check_pdf_cells(table, printed, ("2006:Q3",), coded=True)

    def test_supplementary_table_does_not_overwrite_base_table(self):
        text = z1.pdf_text(z1.RAW / z1.SOURCES["archived_pdf"][0])
        text += '\nL.100.a A supplementary table\n' + z1.UNIT_HEADING + '\n'
        text += '1 Wrong base value ' + ' '.join(['0.0'] * 11) + ' 1\n'
        parsed = z1.pdf_tables(text)
        self.assertEqual(parsed[100]["rows"][1]["values"][-1], "40516.1")
        self.assertEqual(len(parsed), 64)

    def test_omitting_mail_float_breaks_the_holder_margin(self):
        tables = copy.deepcopy(self.tables)
        tables[204]["values"][-1][35] = 0
        with self.assertRaisesRegex(ValueError, "instrument margin does not reconcile"):
            z1.instrument_marginals(tables)

    def test_source_tampering_fails_before_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            raw = Path(directory)
            for relative, _ in z1.SOURCES.values():
                target = raw / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(z1.RAW / relative, target)
            with (raw / z1.SOURCES["archived_pdf"][0]).open("ab") as stream:
                stream.write(b"changed")
            with self.assertRaisesRegex(ValueError, "source hash mismatch"):
                z1.build(raw)


if __name__ == "__main__":
    unittest.main()
