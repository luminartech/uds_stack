#!/usr/bin/env python3
"""Self-tests for :mod:`check_size`.

A guard rail is only verified by watching it fail, so every check below demonstrates the
thing it stops, not merely that a clean input passes.
"""

from __future__ import annotations

import unittest

from check_size import TEXT_GROWTH_PERCENT, Sections, compare, parse_berkeley

BASELINE = Sections(text=48_000, data=0, bss=9_324)
ALLOWED = BASELINE.text * TEXT_GROWTH_PERCENT // 100


def text(change: int) -> Sections:
    return Sections(text=BASELINE.text + change, data=BASELINE.data, bss=BASELINE.bss)


class LlvmSizeOutputIsParsed(unittest.TestCase):
    def test_the_berkeley_line_is_read(self) -> None:
        output = (
            "   text\t   data\t    bss\t    dec\t    hex\tfilename\n"
            "  48444\t      0\t   9324\t  57768\t   e1a8\tembedded-probe\n"
        )
        self.assertEqual(parse_berkeley(output), Sections(48_444, 0, 9_324))

    def test_anything_else_is_refused(self) -> None:
        """Otherwise a changed output format would be read as some other sizes."""
        with self.assertRaises(ValueError):
            parse_berkeley("section size addr\n.text 48444 0\n")


class RamMustMatch(unittest.TestCase):
    def test_bss_growth_fails(self) -> None:
        failures, _ = compare(BASELINE, Sections(BASELINE.text, 0, BASELINE.bss + 4))
        self.assertEqual(len(failures), 1)
        self.assertIn("bss", failures[0])

    def test_a_bss_shrink_fails_too(self) -> None:
        """So the smaller footprint is recorded in the commit that makes it."""
        failures, _ = compare(BASELINE, Sections(BASELINE.text, 0, BASELINE.bss - 4))
        self.assertEqual(len(failures), 1)

    def test_data_growth_fails(self) -> None:
        failures, _ = compare(BASELINE, Sections(BASELINE.text, 8, BASELINE.bss))
        self.assertIn("data", failures[0])


class FlashMayDriftWithinTheAllowance(unittest.TestCase):
    def test_the_baseline_passes(self) -> None:
        self.assertEqual(compare(BASELINE, BASELINE), ([], []))

    def test_growth_up_to_the_allowance_passes(self) -> None:
        self.assertEqual(compare(BASELINE, text(ALLOWED)), ([], []))

    def test_growth_past_the_allowance_fails(self) -> None:
        failures, _ = compare(BASELINE, text(ALLOWED + 1))
        self.assertEqual(len(failures), 1)
        self.assertIn("text", failures[0])

    def test_a_large_shrink_passes_with_a_note(self) -> None:
        failures, notes = compare(BASELINE, text(-ALLOWED - 1))
        self.assertEqual(failures, [])
        self.assertEqual(len(notes), 1)


if __name__ == "__main__":
    unittest.main()
