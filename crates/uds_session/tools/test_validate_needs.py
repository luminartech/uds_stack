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
   :source: ISO 14229-2:2021 9.5 Table 6

   On initialisation, the server shall be in the default session.
"""

TWO_REQUIREMENTS = REQUIREMENT + """
.. llr:: Session timer stops when a request begins
   :id: UDSS_LLR_0104
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6

   The server shall stop the timer.
"""

NESTED = """\
.. llr:: Requirement carrying a nested block
   :id: UDSS_LLR_0102
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6

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

    def run_main(self, root: Path | None = None) -> tuple[int, str]:
        """Run `main`, capturing stderr so a test can check *why* it failed,
        not just that it did. An assertion on the exit code alone can pass
        for the wrong reason: any other check that also fails the run would
        make the test green without exercising the behaviour it names.
        """
        buffer = io.StringIO()
        with contextlib.redirect_stderr(buffer):
            code = vn.main(self.root if root is None else root)
        return code, buffer.getvalue()


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

    def test_an_aou_directive_is_no_longer_a_recognised_need(self) -> None:
        self.write("r.rst", REQUIREMENT + """
.. aou:: An assumption of use
   :id: UDSS_AOU_0001
   :status: draft
   :origin: derived

   The caller shall supply a monotonic timestamp.

   Rationale: assumptions of use live in the qualification repository.
""")
        needs = vn.parse(self.root / "r.rst")
        self.assertEqual([n.type for n in needs], ["llr"])


class MainTests(TempTree):
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
            ":origin: session-layer-standard", ":origin: derived",
        ).replace(
            "   :source: ISO 14229-2:2021 9.5 Table 6\n", "",
        ))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("no `Rationale:` paragraph", stderr)

    def test_an_indented_need_fails(self) -> None:
        self.write("r.rst", INDENTED_NEED)
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("is indented", stderr)

    def test_derived_with_a_rationale_passes(self) -> None:
        self.write("r.rst", REQUIREMENT.replace(
            ":origin: session-layer-standard", ":origin: derived",
        ).replace(
            "   :source: ISO 14229-2:2021 9.5 Table 6\n", "",
        ) + "\n   Rationale: the session layer's own state must stay consistent.\n")
        self.assertEqual(vn.main(self.root), 0)

    def test_a_requirement_with_a_nested_block_validates_clean(self) -> None:
        """The whole case for reStructuredText: a requirement carrying a nested
        directive must still be validated, not merely parsed."""
        self.write("r.rst", NESTED)
        code, _ = self.run_main()
        self.assertEqual(code, 0)

    def test_an_aou_id_on_a_requirement_is_rejected(self) -> None:
        self.write("r.rst", REQUIREMENT.replace("UDSS_LLR_0101", "UDSS_AOU_0001"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("does not match the registered scheme", stderr)

    def test_a_missing_mandatory_field_is_reported(self) -> None:
        self.write("r.rst", REQUIREMENT.replace("   :status: draft\n", ""))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("missing mandatory field", stderr)

    def test_an_unknown_status_is_rejected(self) -> None:
        self.write("r.rst", REQUIREMENT.replace(":status: draft", ":status: provisional"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("status", stderr)


TRANSCRIBED = """\
.. llr:: A transcribed requirement
   :id: UDSS_LLR_0101
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: {source}

   The server shall do the thing.
"""

TRANSCRIBED_NO_SOURCE = """\
.. llr:: A transcribed requirement with no source
   :id: UDSS_LLR_0103
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard

   The server shall do the third thing.
"""

DERIVED = """\
.. llr:: A derived requirement
   :id: UDSS_LLR_0102
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
{extra}
   The server shall do the other thing.

   Rationale: internal consistency demands it.
"""


class SourceTests(TempTree):
    def test_a_well_formed_source_passes(self) -> None:
        self.write("r.rst", TRANSCRIBED.format(source="ISO 14229-2:2021 9.5 Table 6"))
        self.assertEqual(vn.main(self.root), 0)

    def test_multiple_semicolon_separated_sources_pass(self) -> None:
        self.write("r.rst", TRANSCRIBED.format(
            source="ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 10.1.4.1"))
        self.assertEqual(vn.main(self.root), 0)

    def test_a_transcribed_requirement_without_a_source_fails(self) -> None:
        self.write("r.rst", TRANSCRIBED_NO_SOURCE)
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("no `source` is given", stderr)

    def test_an_unrecognised_standard_fails(self) -> None:
        self.write("r.rst", TRANSCRIBED.format(source="ISO 26262-6:2018 8.4.4"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("which is not one of", stderr)

    def test_a_source_without_a_locator_fails(self) -> None:
        self.write("r.rst", TRANSCRIBED.format(source="ISO 14229-2:2021"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("is not of the form", stderr)

    def test_a_derived_requirement_without_a_source_passes(self) -> None:
        self.write("r.rst", DERIVED.format(extra=""))
        self.assertEqual(vn.main(self.root), 0)

    def test_a_derived_requirement_carrying_a_source_fails(self) -> None:
        self.write("r.rst", DERIVED.format(extra="   :source: ISO 14229-2:2021 9.5\n"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("a `source` is present", stderr)

    def test_a_trailing_semicolon_in_a_source_fails(self) -> None:
        self.write("r.rst", TRANSCRIBED.format(source="ISO 14229-2:2021 9.5;"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("empty entry", stderr)

    def test_an_unrecognised_origin_is_not_double_reported(self) -> None:
        self.write("r.rst", REQUIREMENT.replace(
            ":origin: session-layer-standard", ":origin: invented-standard"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("origin", stderr)
        self.assertNotIn("no `source` is given", stderr)


if __name__ == "__main__":
    unittest.main()
