#!/usr/bin/env python3
"""Policy checks on the requirement set.

This is not a Sphinx build and does not try to be one. `sphinx-build -b needs -W`
validates directive syntax, link integrity, and sphinx-needs' own schema; it knows
nothing about the parts of our process that are ours:

  * the ID scheme, and that IDs are unique across the whole set
  * which fields are mandatory for which requirement type
  * the permitted values of those fields
  * the integrity-level ratchet: a requirement may not claim more than it targets
  * that a requirement we derived, rather than transcribed, carries a rationale
  * the clause-locator prohibition
  * that a requirement document is reStructuredText, and that the set is not empty

The two are complementary, and this one is fast enough to run on every commit.

Stdlib only, so it works under `language: system` with no environment to build.
"""

from __future__ import annotations

import re
import sys
import textwrap
from dataclasses import dataclass, field
from pathlib import Path

REQUIREMENT_ROOT = Path("docs/requirements")

# Directive types that carry requirements.
AUTHORED_TYPES = {"llr", "aou"}

# Generated from source annotations by the extractor and imported, never hand-authored.
# Recognised so that authoring one by hand is reported rather than silently accepted.
GENERATED_TYPES = {"impl", "test"}

NEED_TYPES = AUTHORED_TYPES | GENERATED_TYPES

ID_PATTERN = re.compile(r"^UDSS_(LLR|AOU)_\d{4}$")

STATUSES = {"draft", "review", "approved", "obsolete"}

# Ordered weakest to strongest; the index is the comparison key for the ratchet.
LEVELS = ["QM", "A", "B", "C", "D"]

ORIGINS = {
    "session-layer-standard",
    "ip-profile-standard",
    "application-layer-standard",
    "derived",
}

REQUIRED_FIELDS = {
    "llr": {"id", "status", "integrity_level", "target_level", "origin"},
    "aou": {"id", "status", "origin"},
}

# Locator shapes, not the word "ISO". Naming a standard is permitted and desirable;
# citing a numbered position inside one is not, because a reader of this repository
# does not hold the document and cannot check it.
LOCATOR_PATTERNS = [
    re.compile(r"\b(?:Table|Figure|Annex|Clause|Section)\s+[A-Z]?\.?\d", re.IGNORECASE),
    re.compile(r"\bREQ\s+\d"),
    re.compile(r"§\s*\d"),
]

# A labelled rationale paragraph, at the start of a line.
RATIONALE = re.compile(r"^\s*Rationale\b.*:", re.MULTILINE)

# RST directives at column 0; the body runs to the next non-blank line at column 0.
# Indentation-based extent means a directive cannot be "unterminated", and a nested
# block cannot silently end its parent — the failure mode MyST fences had.
DIRECTIVE_OPEN = re.compile(r"^\.\.\s+(?P<type>[a-z_]+)::\s*(?P<title>.*?)\s*$")
OPTION = re.compile(r"^\s+:(?P<key>[a-z_]+):\s*(?P<value>.*?)\s*$")

# A need directive nested inside another directive is invisible to `parse`, while Sphinx
# still creates the need — so it would enter needs.json unvalidated. Reported, not skipped.
INDENTED_NEED = re.compile(r"^\s+\.\.\s+(?P<type>[a-z_]+)::")


@dataclass
class Need:
    type: str
    title: str
    path: Path
    line: int
    options: dict[str, str] = field(default_factory=dict)
    body: str = ""


@dataclass
class Problem:
    path: Path
    line: int
    message: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line}: {self.message}"


def parse(path: Path) -> list[Need]:
    """Extract need directives. Directive extent is set by indentation."""
    needs: list[Need] = []
    lines = path.read_text(encoding="utf-8").splitlines()

    index = 0
    while index < len(lines):
        opened = DIRECTIVE_OPEN.match(lines[index])
        # Only need directives are of interest. Every other directive in the tree —
        # `toctree`, `needtable`, `needflow` — is skipped: whether a directive exists
        # and is well formed is Sphinx's job, enforced by the CI build.
        if not opened or opened.group("type") not in NEED_TYPES:
            index += 1
            continue

        need = Need(
            type=opened.group("type"),
            title=opened.group("title"),
            path=path,
            line=index + 1,
        )
        index += 1

        while index < len(lines):
            option = OPTION.match(lines[index])
            if not option:
                break
            need.options[option.group("key")] = option.group("value")
            index += 1

        body_lines: list[str] = []
        while index < len(lines):
            line = lines[index]
            if line.strip() and not line[:1].isspace():
                break
            body_lines.append(line)
            index += 1

        need.body = textwrap.dedent("\n".join(body_lines)).strip()
        needs.append(need)

    return needs


def check_need(need: Need) -> list[Problem]:
    problems: list[Problem] = []

    def fail(message: str) -> None:
        problems.append(Problem(need.path, need.line, message))

    if need.type in GENERATED_TYPES:
        fail(
            f"`{need.type}` needs are generated from source annotations by the extractor "
            "and must not be hand-authored"
        )
        return problems

    if not need.title:
        fail(f"{need.type} has no title")

    missing = REQUIRED_FIELDS[need.type] - need.options.keys()
    if missing:
        fail(f"missing mandatory field(s): {', '.join(sorted(missing))}")

    need_id = need.options.get("id", "")
    if need_id and not ID_PATTERN.match(need_id):
        fail(f"id {need_id!r} does not match the registered scheme UDSS_(LLR|AOU)_####")

    status = need.options.get("status")
    if status is not None and status not in STATUSES:
        fail(f"status {status!r} is not one of {sorted(STATUSES)}")

    origin = need.options.get("origin")
    if origin is not None and origin not in ORIGINS:
        fail(f"origin {origin!r} is not one of {sorted(ORIGINS)}")

    claimed = need.options.get("integrity_level")
    target = need.options.get("target_level")
    for label, value in (("integrity_level", claimed), ("target_level", target)):
        if value is not None and value not in LEVELS:
            fail(f"{label} {value!r} is not one of {LEVELS}")

    if claimed in LEVELS and target in LEVELS:
        if LEVELS.index(claimed) > LEVELS.index(target):
            fail(
                f"integrity_level {claimed} exceeds target_level {target}: "
                "a requirement may not claim more than it targets"
            )

    # A requirement we derived is not traceable to a clause, so the justification has to
    # live with the requirement itself or it lives nowhere. A labelled paragraph is
    # required rather than the word appearing somewhere in the prose, which any sentence
    # mentioning a rationale would satisfy.
    if origin == "derived" and not RATIONALE.search(need.body):
        fail(
            "origin is `derived` but the body has no `Rationale:` paragraph stating why "
            "this requirement exists"
        )

    return problems


def check_locators(path: Path) -> list[Problem]:
    problems: list[Problem] = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        for pattern in LOCATOR_PATTERNS:
            match = pattern.search(line)
            if match:
                problems.append(
                    Problem(
                        path,
                        number,
                        f"clause locator {match.group(0).strip()!r} is not permitted in "
                        "published material; name the standard without a locator and let "
                        "the qualification repository hold the mapping",
                    )
                )
    return problems


def check_indented_needs(path: Path) -> list[Problem]:
    problems: list[Problem] = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        matched = INDENTED_NEED.match(line)
        if matched and matched.group("type") in NEED_TYPES:
            problems.append(
                Problem(
                    path,
                    number,
                    f"`{matched.group('type')}` directive is indented; a need nested inside "
                    "another directive is not validated. Move it to column 0.",
                )
            )
    return problems


def main(root: Path = REQUIREMENT_ROOT) -> int:
    problems: list[Problem] = []
    seen: dict[str, Need] = {}

    if not root.is_dir():
        print(f"{root} does not exist", file=sys.stderr)
        return 1

    # A requirement document left as Markdown would simply not be globbed, and the set
    # would validate clean while ignoring it. Report it instead.
    for path in sorted(root.rglob("*.md")):
        problems.append(
            Problem(
                path,
                1,
                "requirement documents are reStructuredText; convert this file to .rst",
            )
        )

    for path in sorted(root.rglob("*.rst")):
        # The locator prohibition stays live until it is deliberately replaced by the
        # inverse rule. Dropping the call here would let it lapse silently for a commit,
        # while the module docstring still advertises it — the exact defect this file's
        # rewrite exists to remove.
        problems.extend(check_locators(path))
        problems.extend(check_indented_needs(path))

        for need in parse(path):
            problems.extend(check_need(need))

            need_id = need.options.get("id")
            if not need_id:
                continue
            if need_id in seen:
                first = seen[need_id]
                problems.append(
                    Problem(
                        need.path,
                        need.line,
                        f"duplicate id {need_id} (first defined at {first.path}:{first.line})",
                    )
                )
            else:
                seen[need_id] = need

    # Reporting success over an empty set is the failure mode this whole file exists to
    # avoid: a guard rail that silently stops guarding is worse than none.
    if not seen:
        problems.append(
            Problem(root, 0, "no requirements found; refusing to report an empty set valid")
        )

    if problems:
        for problem in sorted(problems, key=lambda p: (str(p.path), p.line)):
            print(problem, file=sys.stderr)
        count = len(problems)
        print(
            f"\n{count} problem{'s' if count != 1 else ''} in the requirement set",
            file=sys.stderr,
        )
        return 1

    print(f"requirement set valid: {len(seen)} requirements")
    return 0


if __name__ == "__main__":
    sys.exit(main())
