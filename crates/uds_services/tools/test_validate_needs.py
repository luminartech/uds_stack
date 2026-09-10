"""Self-tests for the authored need set policy checks.

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
.. llr:: An unsupported service identifier is rejected
   :id: UDSSVC_LLR_0101
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.3.2 Table 4

   On a physically addressed request naming a service identifier the server does not
   support, the server shall respond with negative response code 0x11.
"""

ARCH = """\
.. arch:: Negative response emitter
   :id: UDSSVC_ARCH_0001
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.5

   The dispatcher writes a negative response as the three bytes 0x7F, the request service
   identifier, and the negative response code.
"""

TWO_REQUIREMENTS = REQUIREMENT + """
.. llr:: An unsupported sub-function is rejected
   :id: UDSSVC_LLR_0104
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.3.2 Table 4

   The server shall respond with negative response code 0x12.
"""

NESTED = """\
.. llr:: Requirement carrying a nested block
   :id: UDSSVC_LLR_0102
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 8.7.3.2 Table 4

   The response shall be encoded as follows.

   .. code-block:: text

      [0x7F] [request SID] [0x11]

   That is the whole of it.
"""

SURROUNDED_BY_OTHER_DIRECTIVES = """\
Requirements
============

.. toctree::
   :maxdepth: 1

   llr-negative-response-codes

""" + REQUIREMENT + """
.. needtable::
   :types: llr
   :style: table
"""

INDENTED_NEED = """\
.. admonition:: Note

   .. llr:: An unsupported service identifier is rejected
      :id: UDSSVC_LLR_0101
      :status: draft
      :integrity_level: QM
      :target_level: D
      :origin: application-layer-standard

      The server shall respond with negative response code 0x11.
"""


class TempTree(unittest.TestCase):
    """A documentation root holding the authored sections `main` scans.

    Files go into a section directory rather than the root, because that is the shape
    `main` walks: it globs `docs/architecture` and `docs/requirements`, never `docs`
    itself. A test that wrote to the root would exercise nothing.
    """

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        (self.root / "architecture").mkdir()
        self.addCleanup(self._tmp.cleanup)

    def write(self, name: str, text: str, section: str = "architecture") -> Path:
        path = self.root / section / name
        path.parent.mkdir(parents=True, exist_ok=True)
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
        self.assertEqual(needs[0].title, "An unsupported service identifier is rejected")
        self.assertEqual(needs[0].options["id"], "UDSSVC_LLR_0101")
        self.assertEqual(needs[0].options["origin"], "application-layer-standard")
        self.assertIn("0x11", needs[0].body)

    def test_body_ends_at_the_next_directive(self) -> None:
        needs = vn.parse(self.write("r.rst", TWO_REQUIREMENTS))
        self.assertEqual(len(needs), 2)
        self.assertNotIn("0x12", needs[0].body)
        self.assertIn("0x12", needs[1].body)

    def test_nested_block_stays_inside_the_body(self) -> None:
        needs = vn.parse(self.write("r.rst", NESTED))
        self.assertEqual(len(needs), 1)
        self.assertIn("0x7F", needs[0].body)
        self.assertIn("That is the whole of it.", needs[0].body)

    def test_non_need_directives_are_skipped(self) -> None:
        needs = vn.parse(self.write("r.rst", SURROUNDED_BY_OTHER_DIRECTIVES))
        self.assertEqual(len(needs), 1)
        self.assertEqual(needs[0].options["id"], "UDSSVC_LLR_0101")

    def test_an_aou_directive_is_no_longer_a_recognised_need(self) -> None:
        self.write("r.rst", REQUIREMENT + """
.. aou:: An assumption of use
   :id: UDSSVC_AOU_0001
   :status: draft
   :origin: derived

   The caller shall supply the active diagnostic session.

   Rationale: assumptions of use are not authored in this set.
""")
        needs = vn.parse(self.root / "architecture" / "r.rst")
        self.assertEqual([n.type for n in needs], ["llr"])


class MainTests(TempTree):
    def test_a_valid_set_passes(self) -> None:
        self.write("r.rst", REQUIREMENT)
        self.assertEqual(vn.main(self.root), 0)

    def test_an_empty_need_set_fails(self) -> None:
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("no needs found", stderr)

    def test_a_missing_documentation_root_fails(self) -> None:
        code, stderr = self.run_main(self.root / "absent")
        self.assertEqual(code, 1)
        self.assertIn("does not exist", stderr)

    def test_a_root_with_no_authored_section_fails(self) -> None:
        """A root that exists but holds neither section is a misconfiguration, not an
        empty set. Reporting it separately says which of the two went wrong."""
        bare = self.root / "bare"
        bare.mkdir()
        code, stderr = self.run_main(bare)
        self.assertEqual(code, 1)
        self.assertIn("none of architecture, requirements exists", stderr)

    def test_the_requirements_section_is_scanned_too(self) -> None:
        """`docs/requirements` does not exist yet, so nothing else in this file would
        notice if it were dropped from the scan."""
        self.write("r.rst", REQUIREMENT.replace("UDSSVC_LLR_0101", "UDSSVC_LLR_0900"),
                   section="requirements")
        self.write("a.rst", ARCH)
        self.assertEqual(vn.main(self.root), 0)

    def test_a_set_of_architecture_alone_passes(self) -> None:
        """The prototype phase: the architecture is the whole set, and an absent
        requirements section is not a failure."""
        self.write("a.rst", ARCH)
        self.assertEqual(vn.main(self.root), 0)

    def test_a_stray_markdown_file_fails(self) -> None:
        self.write("r.rst", REQUIREMENT)
        self.write("leftover.md", "```{llr} Something\n:id: UDSSVC_LLR_0199\n```\n")
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
            ":origin: application-layer-standard", ":origin: derived",
        ).replace(
            "   :source: ISO 14229-1:2020 8.7.3.2 Table 4\n", "",
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
            ":origin: application-layer-standard", ":origin: derived",
        ).replace(
            "   :source: ISO 14229-1:2020 8.7.3.2 Table 4\n", "",
        ) + "\n   Rationale: the session layer's own state must stay consistent.\n")
        self.assertEqual(vn.main(self.root), 0)

    def test_a_requirement_with_a_nested_block_validates_clean(self) -> None:
        """The whole case for reStructuredText: a requirement carrying a nested
        directive must still be validated, not merely parsed."""
        self.write("r.rst", NESTED)
        code, _ = self.run_main()
        self.assertEqual(code, 0)

    def test_an_aou_id_on_a_requirement_is_rejected(self) -> None:
        self.write("r.rst", REQUIREMENT.replace("UDSSVC_LLR_0101", "UDSSVC_AOU_0001"))
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
   :id: UDSSVC_LLR_0101
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: application-layer-standard
   :source: {source}

   The server shall do the thing.
"""

TRANSCRIBED_NO_SOURCE = """\
.. llr:: A transcribed requirement with no source
   :id: UDSSVC_LLR_0103
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: application-layer-standard

   The server shall do the third thing.
"""

DERIVED = """\
.. llr:: A derived requirement
   :id: UDSSVC_LLR_0102
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
        self.write("r.rst", TRANSCRIBED.format(source="ISO 14229-1:2020 8.7.3.2 Table 4"))
        self.assertEqual(vn.main(self.root), 0)

    def test_multiple_semicolon_separated_sources_pass(self) -> None:
        self.write("r.rst", TRANSCRIBED.format(
            source="ISO 14229-1:2020 8.7.3.2 Table 4; ISO 14229-1:2020 8.7.5"))
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
        self.write("r.rst", TRANSCRIBED.format(source="ISO 14229-1:2020"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("is not of the form", stderr)

    def test_a_derived_requirement_without_a_source_passes(self) -> None:
        self.write("r.rst", DERIVED.format(extra=""))
        self.assertEqual(vn.main(self.root), 0)

    def test_a_derived_requirement_carrying_a_source_fails(self) -> None:
        self.write("r.rst", DERIVED.format(extra="   :source: ISO 14229-1:2020 8.7.5\n"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("a `source` is present", stderr)

    def test_a_trailing_semicolon_in_a_source_fails(self) -> None:
        self.write("r.rst", TRANSCRIBED.format(source="ISO 14229-1:2020 8.7.5;"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("empty entry", stderr)

    def test_an_unrecognised_origin_is_not_double_reported(self) -> None:
        self.write("r.rst", REQUIREMENT.replace(
            ":origin: application-layer-standard", ":origin: invented-standard"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("origin", stderr)
        self.assertNotIn("no `source` is given", stderr)


class ArchTests(TempTree):
    """The `arch` type and the rules that separate it from `llr`.

    Both directions of the ID rule are covered. Only checking that an architecture element
    rejects a requirement ID would leave the reverse — a requirement filed under an
    architecture ID — passing, and the two IDs differ by one word.
    """

    def test_an_architecture_element_passes(self) -> None:
        self.write("a.rst", ARCH)
        self.assertEqual(vn.main(self.root), 0)

    def test_an_architecture_element_is_parsed_as_its_own_type(self) -> None:
        needs = vn.parse(self.write("a.rst", ARCH))
        self.assertEqual([n.type for n in needs], ["arch"])
        self.assertEqual(needs[0].options["id"], "UDSSVC_ARCH_0001")

    def test_an_architecture_element_carrying_an_integrity_level_fails(self) -> None:
        self.write("a.rst", ARCH.replace(
            "   :origin:", "   :integrity_level: QM\n   :origin:"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("must not carry integrity_level", stderr)

    def test_an_architecture_element_carrying_a_target_level_fails(self) -> None:
        self.write("a.rst", ARCH.replace(
            "   :origin:", "   :target_level: D\n   :origin:"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("must not carry target_level", stderr)

    def test_an_architecture_element_under_a_requirement_id_fails(self) -> None:
        self.write("a.rst", ARCH.replace("UDSSVC_ARCH_0001", "UDSSVC_LLR_0001"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("does not match the registered scheme for `arch`", stderr)

    def test_a_requirement_under_an_architecture_id_fails(self) -> None:
        self.write("r.rst", REQUIREMENT.replace("UDSSVC_LLR_0101", "UDSSVC_ARCH_0101"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("does not match the registered scheme for `llr`", stderr)

    def test_an_architecture_element_without_a_status_fails(self) -> None:
        self.write("a.rst", ARCH.replace("   :status: draft\n", ""))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("missing mandatory field", stderr)

    def test_a_derived_architecture_element_needs_a_rationale(self) -> None:
        """The rule that matters most for this crate: clause 8.7 fixes the response codes
        but says nothing about trait design, so most architecture elements here are
        derived and their reasoning cannot be reconstructed afterwards."""
        self.write("a.rst", ARCH.replace(
            ":origin: application-layer-standard", ":origin: derived",
        ).replace("   :source: ISO 14229-1:2020 8.5\n", ""))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("no `Rationale:` paragraph", stderr)

    def test_a_derived_architecture_element_with_a_rationale_passes(self) -> None:
        self.write("a.rst", ARCH.replace(
            ":origin: application-layer-standard", ":origin: derived",
        ).replace("   :source: ISO 14229-1:2020 8.5\n", "")
            + "\n   Rationale: the encoding is the only shape uds_protocol accepts.\n")
        self.assertEqual(vn.main(self.root), 0)

    def test_an_architecture_id_collides_with_a_requirement_id_check(self) -> None:
        """IDs are unique across the whole set, not per type."""
        self.write("a.rst", ARCH)
        self.write("b.rst", ARCH.replace("Negative response emitter", "A different name"))
        code, stderr = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("duplicate id", stderr)


if __name__ == "__main__":
    unittest.main()
