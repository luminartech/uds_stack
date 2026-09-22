#!/usr/bin/env python3
"""Check that every crate shares the workspace's one set of governance files.

The licence, contributing and security texts are the workspace's, not each
crate's. Cargo has no notion of a workspace-level licence file -- it packages
whatever sits in the crate directory -- so each crate carries `LICENSE-MIT` and
`LICENSE-APACHE` as a *symlink* to the root pair. `cargo package` dereferences
symlinks, so the published `.crate` still ships the full text, and there is one
text to keep current rather than five that drift.

That arrangement is invisible in a diff: replacing a symlink with a copy looks
like an ordinary file, and the copy is wrong the first time the root text
changes. This is what notices.

Checked:

  * every crate has both licence files, as symlinks, pointing at the root pair
  * no crate carries its own CONTRIBUTING / SECURITY / CODE_OF_CONDUCT -- those
    are the repository's, and a per-crate copy would go stale unread
  * every crate manifest declares the same SPDX expression
  * the root pair exists and is not empty
"""

from __future__ import annotations

import sys
from pathlib import Path

LICENCES = ("LICENSE-MIT", "LICENSE-APACHE")
SPDX = 'license = "MIT OR Apache-2.0"'
# Shared at the repository root; a per-crate copy is the defect, not the absence.
NOT_PER_CRATE = ("CONTRIBUTING.md", "SECURITY.md", "CODE_OF_CONDUCT.md")


def check(root: Path) -> list[str]:
    problems: list[str] = []

    for name in LICENCES:
        path = root / name
        if not path.is_file() or path.is_symlink():
            problems.append(f"{name}: the root copy must be a regular file")
        elif not path.read_text(encoding="utf-8").strip():
            problems.append(f"{name}: the root copy is empty")

    crates = sorted(p for p in (root / "crates").iterdir() if (p / "Cargo.toml").is_file())
    if not crates:
        problems.append("crates/: no crates found -- is this the workspace root?")

    for crate in crates:
        for name in LICENCES:
            path = crate / name
            rel = path.relative_to(root)
            if not path.is_symlink():
                problems.append(
                    f"{rel}: must be a symlink to ../../{name}, not a copy"
                    if path.exists()
                    else f"{rel}: missing"
                )
                continue
            target = path.readlink()
            if str(target) != f"../../{name}":
                problems.append(f"{rel}: points at {target}, expected ../../{name}")
            elif not path.resolve().is_file():
                problems.append(f"{rel}: dangling symlink")

        for name in NOT_PER_CRATE:
            if (crate / name).exists():
                problems.append(
                    f"{crate.relative_to(root) / name}: {name} is shared at the "
                    f"repository root; remove this copy and link to it"
                )

        manifest = (crate / "Cargo.toml").read_text(encoding="utf-8")
        if SPDX not in manifest:
            problems.append(f"{crate.name}/Cargo.toml: expected `{SPDX}`")

    return problems


def main(argv: list[str]) -> int:
    root = Path(argv[1]) if len(argv) > 1 else Path(__file__).resolve().parent.parent
    problems = check(root)
    for problem in problems:
        print(f"{problem}", file=sys.stderr)
    if problems:
        print(f"\n{len(problems)} governance problem(s)", file=sys.stderr)
        return 1
    print("governance files shared: licences symlinked, no per-crate copies")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
