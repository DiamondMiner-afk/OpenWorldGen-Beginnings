import importlib.util
import shutil
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("probe_fed_z1_preorigin.py")
SPEC = importlib.util.spec_from_file_location("probe_fed_z1_preorigin", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DecemberZ1ProbeTests(unittest.TestCase):
    def test_same_quarter_levels_and_pdf(self):
        report = MODULE.build_report()
        values = {row["code"]: row for row in report["series"]}
        self.assertEqual(values["FL153020005.Q"]["december_file_2006_q3_millions"], 272149)
        self.assertEqual(values["FL153030005.Q"]["december_pdf_printed_billions"], "5040.8")
        self.assertEqual(report["selected_two_series_combined_millions"]["march_minus_december"], 1874)
        self.assertEqual(report["actor_input_status_for_2007_01_01"], "blocked_exact_bytes_not_certified")

    def test_modified_source_fails(self):
        with tempfile.TemporaryDirectory() as folder:
            raw = Path(folder)
            for name in ("source-provenance.json", MODULE.ZIP_NAME, MODULE.PDF_NAME):
                shutil.copyfile(MODULE.RAW / name, raw / name)
            with (raw / MODULE.PDF_NAME).open("ab") as stream:
                stream.write(b"modified")
            with self.assertRaisesRegex(ValueError, "source size/hash mismatch"):
                MODULE.inspect(raw)


if __name__ == "__main__":
    unittest.main()
