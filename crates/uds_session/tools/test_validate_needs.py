"""Self-tests for the requirement set policy checks.

Stdlib only, matching `validate_needs.py` itself: the hook runs under
`language: system` with no environment to build.

Run: python3 -m unittest discover --start-directory tools --pattern 'test_*.py'
"""

from __future__ import annotations

import contextlib
import io
import tempfile
import unittest
from pathlib import Path

import validate_needs as vn

REQUIREMENT = """\
.. llr:: Server starts in the default session
   :id: UDSS_LLR_0101
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard

   On initialisation, the server shall be in the default session.
"""

TWO_REQUIREMENTS = REQUIREMENT + """
.. llr:: Session timer stops when a request begins
   :id: UDSS_LLR_0104
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard

   The server shall stop the timer.
"""

NESTED = """\
.. llr:: Requirement carrying a nested block
   :id: UDSS_LLR_0102
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard

   The primitive shall be as follows.

   .. code-block:: text

      S_Data.req ( S_Mtype, S_AI[TAtype] )

   That is the whole of it.
"""

SURROUNDED_BY_OTHER_DIRECTIVES = """\
Requirements
============

.. toctree::
   :maxdepth: 1

   llr-server-session-timer

""" + REQUIREMENT + """
.. needtable::
   :types: llr
   :style: table
"""

INDENTED_NEED = """\
.. admonition:: Note

   .. llr:: Server starts in the default session
      :id: UDSS_LLR_0101
      :status: draft
      :integrity_level: QM
      :target_level: D
      :origin: session-layer-standard

      On initialisation, the server shall be in the default session.
"""


class TempTree(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, name: str, text: str) -> Path:
        path = self.root / name
        path.write_text(text, encoding="utf-8")
        return path


class ParseTests(TempTree):
    def test_parses_a_requirement_with_options_and_body(self) -> None:
        needs = vn.parse(self.write("r.rst", REQUIREMENT))
        self.assertEqual(len(needs), 1)
        self.assertEqual(needs[0].type, "llr")
        self.assertEqual(needs[0].title, "Server starts in the default session")
        self.assertEqual(needs[0].options["id"], "UDSS_LLR_0101")
        self.assertEqual(needs[0].options["origin"], "session-layer-standard")
        self.assertIn("default session", needs[0].body)

    def test_body_ends_at_the_next_directive(self) -> None:
        needs = vn.parse(self.write("r.rst", TWO_REQUIREMENTS))
        self.assertEqual(len(needs), 2)
        self.assertNotIn("stop the timer", needs[0].body)
        self.assertIn("stop the timer", needs[1].body)

    def test_nested_block_stays_inside_the_body(self) -> None:
        needs = vn.parse(self.write("r.rst", NESTED))
        self.assertEqual(len(needs), 1)
        self.assertIn("S_Data.req", needs[0].body)
        self.assertIn("That is the whole of it.", needs[0].body)

    def test_non_need_directives_are_skipped(self) -> None:
        needs = vn.parse(self.write("r.rst", SURROUNDED_BY_OTHER_DIRECTIVES))
        self.assertEqual(len(needs), 1)
        self.assertEqual(needs[0].options["id"], "UDSS_LLR_0101")


class MainTests(TempTree):
    def run_main(self, root: Path | None = None) -> tuple[int, str]:
        """Run `main`, capturing stderr so a test can check *why* it failed,
        not just that it did. Without this, a test can pass for the wrong
        reason: if the empty-set guard misfired, the stray-.md test would
        still be green.
        """
        buffer = io.StringIO()
        with contextlib.redirect_stderr(buffer):
            code = vn.main(self.root if root is None else root)
        return code, buffer.getvalue()

    def test_a_valid_set_passes(self) -> None:
        self.write("r.rst", REQUIREMENT)
        self.assertEqual(vn.main(self.root), 0)

    def test_an_empty_requirement_set_fails(self) -> None:
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("no requirements found", stderr)

    def test_a_missing_requirement_directory_fails(self) -> None:
        code, stderr = self.run_main(self.root / "absent")
        self.assertEqual(code, 1)
        self.assertIn("does not exist", stderr)

    def test_a_stray_markdown_file_fails(self) -> None:
        self.write("r.rst", REQUIREMENT)
        self.write("leftover.md", "```{llr} Something\n:id: UDSS_LLR_0199\n```\n")
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("convert this file to .rst", stderr)

    def test_duplicate_ids_fail(self) -> None:
        self.write("a.rst", REQUIREMENT)
        self.write("b.rst", REQUIREMENT)
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("duplicate id", stderr)

    def test_claiming_more_than_the_target_fails(self) -> None:
        self.write("r.rst", REQUIREMENT.replace(":integrity_level: QM", ":integrity_level: D")
                                       .replace(":target_level: D", ":target_level: B"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("exceeds target_level", stderr)

    def test_derived_without_a_rationale_fails(self) -> None:
        self.write("r.rst", REQUIREMENT.replace(
            ":origin: session-layer-standard", ":origin: derived"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("no `Rationale:` paragraph", stderr)

    def test_a_clause_locator_in_a_requirement_fails(self) -> None:
        self.write("r.rst", REQUIREMENT.replace(
            "On initialisation, the server shall be in the default session.",
            "As specified in Table 6, the server shall start in the default session."))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("clause locator", stderr)

    def test_an_indented_need_fails(self) -> None:
        self.write("r.rst", INDENTED_NEED)
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("is indented", stderr)

    def test_derived_with_a_rationale_passes(self) -> None:
        self.write("r.rst", REQUIREMENT.replace(
            ":origin: session-layer-standard", ":origin: derived",
        ) + "\n   Rationale: the session layer's own state must stay consistent.\n")
        self.assertEqual(vn.main(self.root), 0)


if __name__ == "__main__":
    unittest.main()
