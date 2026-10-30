"""Small adversarial checks for the packed MatrAIx decoder."""

import unittest

from matraix_qc import decode_field


class DecodeFieldTests(unittest.TestCase):
    def test_low_and_high_nibble(self) -> None:
        self.assertEqual(decode_field(bytes([0x21]), None, None, 0, ["zero", "one", "two"]), "one")
        self.assertEqual(decode_field(bytes([0x21]), None, None, 1, ["zero", "one", "two"]), "two")

    def test_set_bit_means_missing(self) -> None:
        self.assertIsNone(decode_field(bytes([0x21]), bytes([0x01]), None, 0, ["zero", "one"]))

    def test_override_precedes_packed_code(self) -> None:
        self.assertEqual(decode_field(bytes([0x01]), None, [{"field_index": 0, "value": "exact"}],
                                      0, ["zero", "one"]), "exact")

    def test_out_of_codebook_is_unresolved(self) -> None:
        self.assertIsNone(decode_field(bytes([0x0F]), None, None, 0, ["zero"]))


if __name__ == "__main__":
    unittest.main()
