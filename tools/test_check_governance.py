"""Self-tests for check_governance.

Each test builds a minimal workspace in a temporary directory and asserts the
checker either accepts it or rejects it for the stated reason. A checker that
has never been seen to fail is not evidence of anything, so every rule here has
a test that trips it.
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_governance import LICENCES, check

MANIFEST = (
    '[package]\nname = "c"\nlicense = "MIT OR Apache-2.0"\n'
    "\n[lints]\nworkspace = true\n"
)


def workspace(tmp: Path, crates: tuple[str, ...] = ("alpha", "beta")) -> Path:
    """A well-formed workspace: root licences, crates symlinked to them."""
    for name in LICENCES:
        (tmp / name).write_text(f"{name} text\n", encoding="utf-8")
    for crate in crates:
        directory = tmp / "crates" / crate
        directory.mkdir(parents=True)
        (directory / "Cargo.toml").write_text(MANIFEST, encoding="utf-8")
        for name in LICENCES:
            (directory / name).symlink_to(f"../../{name}")
    return tmp


class CheckGovernance(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = workspace(Path(self._tmp.name))

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def assert_rejects(self, fragment: str) -> None:
        problems = check(self.root)
        self.assertTrue(problems, "expected a problem, got none")
        self.assertTrue(
            any(fragment in p for p in problems),
            f"no problem mentioned {fragment!r}; got {problems}",
        )

    def test_well_formed_workspace_passes(self) -> None:
        self.assertEqual(check(self.root), [])

    def test_licence_replaced_by_a_copy_is_rejected(self) -> None:
        path = self.root / "crates" / "alpha" / "LICENSE-MIT"
        path.unlink()
        path.write_text("MIT text\n", encoding="utf-8")
        self.assert_rejects("not a copy")

    def test_missing_licence_is_rejected(self) -> None:
        (self.root / "crates" / "alpha" / "LICENSE-APACHE").unlink()
        self.assert_rejects("missing")

    def test_symlink_to_the_wrong_place_is_rejected(self) -> None:
        path = self.root / "crates" / "beta" / "LICENSE-MIT"
        path.unlink()
        path.symlink_to("../alpha/LICENSE-MIT")
        self.assert_rejects("expected ../../LICENSE-MIT")

    def test_dangling_symlink_is_rejected(self) -> None:
        (self.root / "LICENSE-MIT").unlink()
        self.assert_rejects("dangling")

    def test_per_crate_contributing_is_rejected(self) -> None:
        (self.root / "crates" / "alpha" / "CONTRIBUTING.md").write_text("x", encoding="utf-8")
        self.assert_rejects("shared at the repository root")

    def test_per_crate_security_is_rejected(self) -> None:
        (self.root / "crates" / "beta" / "SECURITY.md").write_text("x", encoding="utf-8")
        self.assert_rejects("shared at the repository root")

    def test_divergent_spdx_is_rejected(self) -> None:
        (self.root / "crates" / "alpha" / "Cargo.toml").write_text(
            '[package]\nname = "c"\nlicense = "Apache-2.0"\n'
            "\n[lints]\nworkspace = true\n",
            encoding="utf-8",
        )
        self.assert_rejects("expected `license =")

    def test_crate_not_inheriting_the_lint_standard_is_rejected(self) -> None:
        (self.root / "crates" / "alpha" / "Cargo.toml").write_text(
            '[package]\nname = "c"\nlicense = "MIT OR Apache-2.0"\n', encoding="utf-8"
        )
        self.assert_rejects("must inherit the workspace lint standard")

    def test_crate_with_its_own_lints_table_is_rejected(self) -> None:
        # Cargo would reject this manifest outright; the check names the reason and
        # points at the remedy rather than leaving a bare parse error.
        (self.root / "crates" / "beta" / "Cargo.toml").write_text(
            MANIFEST + '\n[lints.clippy]\nunwrap_used = "deny"\n', encoding="utf-8"
        )
        self.assert_rejects("replaces the workspace standard")

    def restate_lints(self, rust: str, crate: str = "uds_c") -> None:
        """Give the workspace a lint table and `crate` its own copy, `rust` changed."""
        directory = self.root / "crates" / crate
        if not directory.exists():
            directory.mkdir()
            for name in LICENCES:
                (directory / name).symlink_to(f"../../{name}")
        (self.root / "Cargo.toml").write_text(
            '[workspace]\n\n[workspace.lints.rust]\nunsafe_code = "forbid"\n'
            'missing_docs = "deny"\n\n[workspace.lints.clippy]\n'
            'pedantic = { level = "deny", priority = -1 }\n',
            encoding="utf-8",
        )
        (directory / "Cargo.toml").write_text(
            '[package]\nname = "c"\nlicense = "MIT OR Apache-2.0"\n\n'
            f"[lints.rust]\n{rust}\n\n[lints.clippy]\n"
            'pedantic = { level = "deny", priority = -1 }\n',
            encoding="utf-8",
        )

    def test_restated_lints_relaxing_only_unsafe_code_pass(self) -> None:
        self.restate_lints('unsafe_code = "deny"\nmissing_docs = "deny"')
        self.assertEqual(check(self.root), [])

    def test_restated_lints_dropping_an_entry_are_rejected(self) -> None:
        self.restate_lints('unsafe_code = "deny"')
        self.assert_rejects("may differ from [workspace.lints] only")

    def test_restated_lints_relaxing_another_entry_are_rejected(self) -> None:
        self.restate_lints('unsafe_code = "deny"\nmissing_docs = "warn"')
        self.assert_rejects("may differ from [workspace.lints] only")

    def test_restated_lints_identical_to_the_workspace_are_rejected(self) -> None:
        # Nothing to relax, so the crate inherits instead.
        self.restate_lints('unsafe_code = "forbid"\nmissing_docs = "deny"')
        self.assert_rejects("may differ from [workspace.lints] only")

    def test_restated_lints_in_another_crate_are_rejected(self) -> None:
        # The relaxation is the C binding's alone.
        self.restate_lints('unsafe_code = "deny"\nmissing_docs = "deny"', crate="alpha")
        self.assert_rejects("only the C binding, uds_c, may restate them")

    def test_empty_root_licence_is_rejected(self) -> None:
        (self.root / "LICENSE-APACHE").write_text("   \n", encoding="utf-8")
        self.assert_rejects("empty")

    def test_root_licence_that_is_itself_a_symlink_is_rejected(self) -> None:
        path = self.root / "LICENSE-MIT"
        path.unlink()
        path.symlink_to("LICENSE-APACHE")
        self.assert_rejects("must be a regular file")


if __name__ == "__main__":
    unittest.main()
