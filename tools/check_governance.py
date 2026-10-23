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
  * every crate inherits the workspace lint standard rather than declaring its own,
    save a C binding, which restates it with `unsafe_code` at `deny`
  * the root pair exists and is not empty
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

LICENCES = ("LICENSE-MIT", "LICENSE-APACHE")
SPDX = 'license = "MIT OR Apache-2.0"'
# Shared at the repository root; a per-crate copy is the defect, not the absence.
NOT_PER_CRATE = ("CONTRIBUTING.md", "SECURITY.md", "CODE_OF_CONDUCT.md")
# Cargo rejects a manifest that inherits `[workspace.lints]` and also adds to it, so a
# local `[lints.*]` table is not an extension of the standard -- it is a replacement for
# it, and a silent one. A crate that needs more or less says so as a crate-root
# attribute in its own lib.rs, where a reader opening the file sees it.
INHERITS_LINTS = "[lints]\nworkspace = true"
# The one replacement admitted. A crate that binds to C declares and calls C, which
# `forbid(unsafe_code)` rules out and no attribute can relax, so it restates the table
# with that entry at `deny` and allows `unsafe` module by module, with a reason. Every
# other entry must equal the workspace's, so the standard is still declared once.
RELAXED = ("rust", "unsafe_code", "deny")


def lint_entries(manifest: str, prefix: str) -> set[tuple[str, str, str]]:
    """Every `(group, lint, value)` in the `[<prefix>.<group>]` tables of a manifest.

    A line-based reading, enough for the flat tables a lint standard is written in, so
    the check runs on any Python 3 without a TOML parser.
    """
    entries: set[tuple[str, str, str]] = set()
    group = None
    for raw in manifest.splitlines():
        line = raw.split("#", 1)[0].strip()
        header = re.fullmatch(r"\[([^\]]+)\]", line)
        if header:
            name = header.group(1).strip()
            group = name[len(prefix) + 1 :] if name.startswith(prefix + ".") else None
        elif group and "=" in line:
            lint, value = (part.strip() for part in line.split("=", 1))
            entries.add((group, lint, re.sub(r"\s+", " ", value)))
    return entries


def restated_lints_problem(root: Path, manifest: str) -> str | None:
    """Why a crate's own lint tables are not the workspace's with RELAXED applied."""
    try:
        workspace = (root / "Cargo.toml").read_text(encoding="utf-8")
    except OSError as error:
        return f"cannot compare its lint tables with the workspace's: {error}"
    group, lint, level = RELAXED
    expected = {
        (g, name, f'"{level}"' if (g, name) == (group, lint) else value)
        for g, name, value in lint_entries(workspace, "workspace.lints")
    }
    if lint_entries(manifest, "lints") != expected:
        return (
            "declares its own lint tables, which may differ from [workspace.lints] "
            f'only in `{lint} = "{level}"`; restate the workspace\'s entry for entry'
        )
    return None


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
        if INHERITS_LINTS not in manifest and "[lints." in manifest:
            problem = restated_lints_problem(root, manifest)
            if problem:
                problems.append(f"{crate.name}/Cargo.toml: {problem}")
        elif INHERITS_LINTS not in manifest:
            problems.append(
                f"{crate.name}/Cargo.toml: must inherit the workspace lint standard "
                f"with `[lints]` / `workspace = true`"
            )
        elif "[lints." in manifest:
            problems.append(
                f"{crate.name}/Cargo.toml: declares its own [lints.*] table, which "
                f"replaces the workspace standard instead of extending it; put the "
                f"difference in lib.rs as a crate-root attribute with a reason"
            )

    return problems


def main(argv: list[str]) -> int:
    root = Path(argv[1]) if len(argv) > 1 else Path(__file__).resolve().parent.parent
    problems = check(root)
    for problem in problems:
        print(f"{problem}", file=sys.stderr)
    if problems:
        print(f"\n{len(problems)} governance problem(s)", file=sys.stderr)
        return 1
    print(
        "governance files shared: licences symlinked, no per-crate copies, "
        "one lint standard"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
