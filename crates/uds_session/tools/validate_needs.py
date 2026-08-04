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

The two are complementary, and this one is fast enough to run on every commit.

Stdlib only, so it works under `language: system` with no environment to build.
"""

from __future__ import annotations

import re
import sys
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

DIRECTIVE_OPEN = re.compile(r"^```\{(?P<type>[a-z_]+)\}\s*(?P<title>.*?)\s*$")
DIRECTIVE_CLOSE = re.compile(r"^```\s*$")
OPTION = re.compile(r"^:(?P<key>[a-z_]+):\s*(?P<value>.*?)\s*$")


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


def parse(path: Path) -> tuple[list[Need], list[Problem]]:
    """Extract MyST directive blocks. Malformed blocks are reported, not raised."""
    needs: list[Need] = []
    problems: list[Problem] = []
    lines = path.read_text(encoding="utf-8").splitlines()

    index = 0
    while index < len(lines):
        opened = DIRECTIVE_OPEN.match(lines[index])
        # Only need directives are of interest. Every other MyST directive in the tree —
        # `toctree`, `needtable`, `needflow`, admonitions — is skipped: whether a
        # directive exists and is well formed is Sphinx's job, and the CI build is where
        # that is enforced. A misspelled need type is therefore caught by Sphinx as an
        # unknown directive, not here.
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
        closed = False
        while index < len(lines):
            if DIRECTIVE_CLOSE.match(lines[index]):
                closed = True
                index += 1
                break
            body_lines.append(lines[index])
            index += 1

        if not closed:
            problems.append(Problem(path, need.line, f"unterminated `{need.type}` directive"))
            continue

        need.body = "\n".join(body_lines).strip()
        needs.append(need)

    return needs, problems


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


def main() -> int:
    if not REQUIREMENT_ROOT.is_dir():
        print(f"{REQUIREMENT_ROOT} does not exist; nothing to validate")
        return 0

    problems: list[Problem] = []
    seen: dict[str, Need] = {}

    for path in sorted(REQUIREMENT_ROOT.rglob("*.md")):
        needs, parse_problems = parse(path)
        problems.extend(parse_problems)
        problems.extend(check_locators(path))

        for need in needs:
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
