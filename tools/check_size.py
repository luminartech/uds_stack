#!/usr/bin/env python3
"""Hold the embedded probe's image to its recorded size.

``testing/embedded-probe`` links the sensor's whole server path for bare metal, and this
compares its sections, as ``llvm-size`` reports them, with the baseline recorded beside
it. Nothing else in the gate notices a change that costs flash or RAM.

The two kinds of memory are held differently:

  * **RAM** (``data`` and ``bss``) must match the baseline exactly. Their sizes follow
    from type layouts, not from the optimizer, so they do not drift between toolchains,
    and every change to them, either way, is recorded in the commit that makes it.
  * **Flash** (``text``) may grow by up to ``TEXT_GROWTH_PERCENT`` before the check
    fails, because code size moves with inlining decisions a change did not intend. A
    shrink of more than that passes, with a reminder to record the smaller baseline.

The baseline is measured on one pinned toolchain (the MSRV); a different compiler gives a
different ``text``.

Stdlib only, so it needs no environment.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

TEXT_GROWTH_PERCENT = 5


@dataclass(frozen=True)
class Sections:
    """An image's sizes in bytes, in ``llvm-size``'s Berkeley grouping."""

    text: int
    data: int
    bss: int


def parse_berkeley(output: str) -> Sections:
    """The sizes in ``llvm-size``'s default output for one file.

    That is a header line, then ``text data bss dec hex filename``.
    """
    lines = [line.split() for line in output.splitlines() if line.strip()]
    if len(lines) != 2 or lines[0][:3] != ["text", "data", "bss"]:
        raise ValueError(f"not llvm-size output for one file:\n{output}")
    text, data, bss = (int(field) for field in lines[1][:3])
    return Sections(text=text, data=data, bss=bss)


def compare(baseline: Sections, measured: Sections) -> tuple[list[str], list[str]]:
    """What fails, then what only needs noting, when ``measured`` is held to ``baseline``."""
    failures: list[str] = []
    notes: list[str] = []
    for name in ("data", "bss"):
        was, now = getattr(baseline, name), getattr(measured, name)
        if now != was:
            failures.append(f"{name}: {now} bytes, baseline {was} ({now - was:+d})")
    allowed = baseline.text * TEXT_GROWTH_PERCENT // 100
    change = measured.text - baseline.text
    if change > allowed:
        failures.append(
            f"text: {measured.text} bytes, baseline {baseline.text} ({change:+d}), "
            f"over the {TEXT_GROWTH_PERCENT}% allowed ({allowed} bytes)"
        )
    elif -change > allowed:
        notes.append(
            f"text: {measured.text} bytes, baseline {baseline.text} ({change:+d}); "
            "record the smaller baseline"
        )
    return failures, notes


def _measure(size_tool: str, image: Path) -> Sections:
    output = subprocess.run(
        [size_tool, str(image)], check=True, capture_output=True, text=True
    ).stdout
    return parse_berkeley(output)


def main(argv: list[str]) -> int:
    """Check ``image`` against ``baseline``, or with ``--record``, rewrite the baseline."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--size-tool", required=True, help="the llvm-size to run")
    parser.add_argument("--record", action="store_true", help="rewrite the baseline")
    parser.add_argument("baseline", type=Path)
    parser.add_argument("image", type=Path)
    args = parser.parse_args(argv)

    measured = _measure(args.size_tool, args.image)
    if args.record:
        args.baseline.write_text(json.dumps(asdict(measured), indent=2) + "\n")
        print(f"recorded {measured} in {args.baseline}")
        return 0

    baseline = Sections(**json.loads(args.baseline.read_text(encoding="utf-8")))
    failures, notes = compare(baseline, measured)
    print(f"{args.image}: {measured}")
    for line in notes:
        print(f"note: {line}")
    for line in failures:
        print(f"error: {line}", file=sys.stderr)
    if failures or notes:
        print(
            "\nIf the change is intended, record it in the same commit: "
            "`just size-baseline`.",
            file=sys.stderr,
        )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
