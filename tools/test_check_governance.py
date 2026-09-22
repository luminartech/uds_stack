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

MANIFEST = '[package]\nname = "c"\nlicense = "MIT OR Apache-2.0"\n'


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
            '[package]\nname = "c"\nlicense = "Apache-2.0"\n', encoding="utf-8"
        )
        self.assert_rejects("expected `license =")

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
