#!/usr/bin/env python3
"""Enforce ``rustfmt.toml``'s ``max_width`` on the lines rustfmt will not enforce it on.

``cargo fmt`` wraps code to ``max_width`` and leaves comments alone: ``wrap_comments`` is
a nightly-only option, so on a stable toolchain ``cargo fmt --check`` passes on a doc
comment of any length. The declared limit is therefore enforced for code and left to a
reviewer's eye for prose, which is exactly the arrangement ``rustfmt.toml``'s own comment
says it exists to avoid.

This closes that gap. It reads the limit from ``rustfmt.toml`` rather than restating it,
so the width is declared once; a second copy here would be the drift this check exists to
catch, one level up.

Only comment lines are measured, and not all of them. This check exists for the lines
rustfmt declines to wrap; a code line rustfmt left long is one it *could not* wrap — an
unbroken string literal, most often — and reporting it asks for the literal to be
falsified or split awkwardly rather than for anything to be fixed. Several of this
stack's are verbatim ISO table titles, where shortening the string would misquote the
standard it cites.

Three kinds of comment line are exempt for the same reason, each because wrapping it
would break something rather than tidy it:

  * a markdown table row, whose columns stop aligning and whose cells stop parsing
  * a banner separator, which is ``////`` and therefore an ordinary comment anyway
  * a line with no whitespace to break at, such as a bare URL or one long intra-doc link
  * anything inside a ``` fence, which is a doc test: code again, and wrapping it breaks
    the example it is there to demonstrate

What this gives up is the long-string-literal case, which no longer has a checker. That
is deliberate: it was reporting things that should not change.

Lengths are counted in characters, not bytes: an em dash is one column.

Stdlib only, so it needs no environment and runs in milliseconds.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Rustfmt's own default, used only when rustfmt.toml states no max_width. Stated here
# because a repository with no rustfmt.toml still has a width, and silently checking
# nothing would be worse than checking the default.
RUSTFMT_DEFAULT_MAX_WIDTH = 100

_MAX_WIDTH = re.compile(r"^\s*max_width\s*=\s*(\d+)\s*(?:#.*)?$", re.MULTILINE)


def max_width(config: str) -> int:
    """The ``max_width`` ``config`` declares, or rustfmt's default where it declares none.

    ``config`` is the text of a ``rustfmt.toml``. A commented-out setting does not count:
    the pattern anchors to the start of a line, so ``# max_width = 92`` is not a
    declaration.
    """
    found = _MAX_WIDTH.search(config)
    return int(found.group(1)) if found else RUSTFMT_DEFAULT_MAX_WIDTH


# A comment line rustfmt will not wrap: `//`, `///` or `//!`, but not `////` and longer,
# which Rust treats as an ordinary comment and which is how banner separators are written.
_COMMENT = re.compile(r"^\s*(?://!|///(?!/)|//(?![/!]))(?P<body>.*)$")
_TABLE_ROW = re.compile(r"^\s*\|.*\|\s*$")
_FENCE = re.compile(r"^```")


def _wrappable(line: str) -> bool:
    """Whether ``line`` is a comment this check can reasonably ask anyone to rewrap."""
    found = _COMMENT.match(line)
    if not found:
        return False
    body = found.group("body").strip()
    if not body or _TABLE_ROW.match(body):
        return False
    # Nothing to break at: one long token, such as a URL or a single intra-doc link.
    return " " in body


def violations(path: str, text: str, limit: int) -> list[tuple[int, int, str]]:
    """Every wrappable comment line of ``text`` longer than ``limit``.

    Returned as ``(line number, width, line)``. Line numbers are 1-based, matching what an
    editor and a compiler diagnostic show. See the module docstring for what is exempt and
    why.
    """
    found: list[tuple[int, int, str]] = []
    fenced = False
    for number, line in enumerate(text.splitlines(), start=1):
        comment = _COMMENT.match(line)
        if comment and _FENCE.match(comment.group("body").strip()):
            fenced = not fenced
            continue
        if not fenced and len(line) > limit and _wrappable(line):
            found.append((number, len(line), line))
    return found


def _config_text(root: Path) -> str:
    config = root / "rustfmt.toml"
    return config.read_text(encoding="utf-8") if config.is_file() else ""


def main(argv: list[str]) -> int:
    """Check every path in ``argv``. Returns 1 where any line exceeds the limit."""
    paths = [Path(arg) for arg in argv]
    if not paths:
        return 0

    limit = max_width(_config_text(Path.cwd()))
    failed = False

    for path in paths:
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as error:
            print(f"{path}: cannot read: {error}", file=sys.stderr)
            failed = True
            continue
        for number, width, line in violations(str(path), text, limit):
            print(f"{path}:{number}: {width} columns, limit is {limit}")
            print(f"    {line.strip()}")
            failed = True

    if failed:
        print(
            "\nrustfmt does not wrap comments, so `cargo fmt` will not fix these.\n"
            "Rewrap them by hand, or change max_width in rustfmt.toml.",
            file=sys.stderr,
        )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
