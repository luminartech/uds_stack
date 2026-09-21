#!/usr/bin/env python3
"""Self-tests for :mod:`check_line_width`.

A guard rail is only verified by watching it fail, so every check below demonstrates the
thing it stops, not merely that a clean input passes.
"""

from __future__ import annotations

import unittest

from check_line_width import (
    RUSTFMT_DEFAULT_MAX_WIDTH,
    max_width,
    violations,
)


class MaxWidthIsReadFromTheConfig(unittest.TestCase):
    """The limit is declared once, in rustfmt.toml, and read from there."""

    def test_a_declared_width_is_used(self) -> None:
        self.assertEqual(max_width("max_width = 92\n"), 92)

    def test_surrounding_settings_and_comments_do_not_confuse_it(self) -> None:
        config = (
            "# The requirement set and the crate share one width.\n"
            "edition = '2024'\n"
            "max_width = 92  # declared here rather than left to the eye\n"
        )
        self.assertEqual(max_width(config), 92)

    def test_a_commented_out_width_is_not_a_declaration(self) -> None:
        """Otherwise a disabled setting would silently become the enforced limit."""
        self.assertEqual(
            max_width("# max_width = 40\n"), RUSTFMT_DEFAULT_MAX_WIDTH
        )

    def test_an_absent_setting_falls_back_to_rustfmt_s_default(self) -> None:
        self.assertEqual(max_width("edition = '2024'\n"), RUSTFMT_DEFAULT_MAX_WIDTH)


class OverlongLinesAreReported(unittest.TestCase):
    """The case that motivated this check: a doc comment `cargo fmt` will not touch."""

    def test_a_long_doc_comment_is_caught(self) -> None:
        text = "/// " + "x" * 100 + "\n"
        found = violations("src/lib.rs", text, 92)
        self.assertEqual(len(found), 1)
        number, width, _line = found[0]
        self.assertEqual((number, width), (1, 104))

    def test_a_line_exactly_at_the_limit_passes(self) -> None:
        """The limit is inclusive, as rustfmt's is."""
        self.assertEqual(violations("src/lib.rs", "x" * 92 + "\n", 92), [])

    def test_line_numbers_are_one_based(self) -> None:
        text = "fine\n" + "y" * 93 + "\nfine\n"
        self.assertEqual([number for number, _, _ in violations("f.rs", text, 92)], [2])

    def test_every_offender_is_reported_not_just_the_first(self) -> None:
        """A report that stopped at the first would hide the rest behind one fix."""
        text = "a" * 93 + "\n" + "b" * 94 + "\n"
        self.assertEqual(len(violations("f.rs", text, 92)), 2)

    def test_width_is_counted_in_characters_not_bytes(self) -> None:
        """An em dash is one column. Counting bytes would flag prose that fits."""
        text = "/// " + "—" * 88 + "\n"
        self.assertEqual(violations("src/lib.rs", text, 92), [])

    def test_a_clean_file_reports_nothing(self) -> None:
        self.assertEqual(violations("src/lib.rs", "/// fine\nfn f() {}\n", 92), [])


if __name__ == "__main__":
    unittest.main()
