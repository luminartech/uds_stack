#!/usr/bin/env python3
"""Self-tests for :mod:`check_size`.

A guard rail is only verified by watching it fail, so every check below demonstrates the
thing it stops, not merely that a clean input passes.
"""

from __future__ import annotations

import unittest
from dataclasses import replace

from check_size import (
    TEXT_TOLERANCE_PERCENT,
    Sizes,
    compare,
    parse_berkeley,
    parse_main_frame,
)

BASELINE = Sizes(text=48_000, data=0, bss=9_324, main_frame=8_824)
ALLOWED = BASELINE.text * TEXT_TOLERANCE_PERCENT // 100

MAIN = """
000098d2 <_ZN14embedded_probe8firmware18__cortex_m_rt_main17h4a350782847883a9E>:
    98d2:      \tpush\t{r4, r5, r7, lr}
    98d4:      \tadd\tr7, sp, #0x8
    98d6:      \tsub.w\tsp, sp, #0x2240
    98da:      \tsub\tsp, #0x28
    98dc:      \tmovw\tr2, #0x2458
    98e0:      \tsub\tsp, #0x100
"""


def text(change: int) -> Sizes:
    return replace(BASELINE, text=BASELINE.text + change)


class LlvmSizeOutputIsParsed(unittest.TestCase):
    def test_the_berkeley_line_is_read(self) -> None:
        output = (
            "   text\t   data\t    bss\t    dec\t    hex\tfilename\n"
            "  48444\t      0\t   9324\t  57768\t   e1a8\tembedded-probe\n"
        )
        self.assertEqual(parse_berkeley(output), (48_444, 0, 9_324))

    def test_anything_else_is_refused(self) -> None:
        """Otherwise a changed output format would be read as some other sizes."""
        with self.assertRaises(ValueError):
            parse_berkeley("section size addr\n.text 48444 0\n")


class MainsFrameIsReadFromItsPrologue(unittest.TestCase):
    def test_pushes_and_stack_subtractions_are_summed(self) -> None:
        self.assertEqual(parse_main_frame(MAIN), 16 + 0x2240 + 0x28)

    def test_a_subtraction_after_the_prologue_is_not_counted(self) -> None:
        """The `sub sp, #0x100` after the first other instruction is not in the sum."""
        self.assertNotEqual(parse_main_frame(MAIN), 16 + 0x2240 + 0x28 + 0x100)

    def test_a_frame_pointer_moved_from_sp_is_part_of_the_prologue(self) -> None:
        main = (
            "0 <__cortex_m_rt_main>:\n  0: \tpush\t{r7, lr}\n  2: \tmov\tr7, sp\n"
            "  4: \tsub.w\tsp, sp, #0xad0\n  8: \tmovw\tr2, #0xc58\n"
        )
        self.assertEqual(parse_main_frame(main), 8 + 0xAD0)

    def test_floating_point_registers_are_eight_bytes(self) -> None:
        main = "0 <__cortex_m_rt_main>:\n  0: \tvpush\t{d8, d9}\n  4: \tbx\tlr\n"
        self.assertEqual(parse_main_frame(main), 16)

    def test_another_function_is_not_read(self) -> None:
        other = "0 <SysTick>:\n  0: \tsub.w\tsp, sp, #0x2240\n"
        with self.assertRaises(ValueError):
            parse_main_frame(other)


class RamMustMatch(unittest.TestCase):
    def test_bss_growth_fails(self) -> None:
        failures = compare(BASELINE, replace(BASELINE, bss=BASELINE.bss + 4))
        self.assertEqual(len(failures), 1)
        self.assertIn("bss", failures[0])

    def test_a_bss_shrink_fails_too(self) -> None:
        """So the smaller footprint is recorded in the commit that makes it."""
        failures = compare(BASELINE, replace(BASELINE, bss=BASELINE.bss - 4))
        self.assertEqual(len(failures), 1)

    def test_data_growth_fails(self) -> None:
        failures = compare(BASELINE, replace(BASELINE, data=8))
        self.assertIn("data", failures[0])

    def test_a_larger_main_frame_fails(self) -> None:
        failures = compare(BASELINE, replace(BASELINE, main_frame=BASELINE.main_frame + 8))
        self.assertEqual(len(failures), 1)
        self.assertIn("main_frame", failures[0])


class FlashMayDriftWithinTheTolerance(unittest.TestCase):
    def test_the_baseline_passes(self) -> None:
        self.assertEqual(compare(BASELINE, BASELINE), [])

    def test_growth_up_to_the_tolerance_passes(self) -> None:
        self.assertEqual(compare(BASELINE, text(ALLOWED)), [])

    def test_growth_past_the_tolerance_fails(self) -> None:
        failures = compare(BASELINE, text(ALLOWED + 1))
        self.assertEqual(len(failures), 1)
        self.assertIn("text", failures[0])

    def test_a_shrink_up_to_the_tolerance_passes(self) -> None:
        self.assertEqual(compare(BASELINE, text(-ALLOWED)), [])

    def test_a_shrink_past_the_tolerance_fails(self) -> None:
        """Otherwise later growth would be measured from the stale, larger baseline."""
        failures = compare(BASELINE, text(-ALLOWED - 1))
        self.assertEqual(len(failures), 1)
        self.assertIn("text", failures[0])


if __name__ == "__main__":
    unittest.main()
