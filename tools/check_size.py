#!/usr/bin/env python3
"""Hold the embedded probe's image to its recorded size.

``testing/embedded-probe`` links the sensor's whole server path for bare metal, and this
compares it with the baseline recorded beside it: its sections, as ``llvm-size`` reports
them, and the stack frame its ``main`` reserves, as ``llvm-objdump`` shows the prologue.
Nothing else in the gate notices a change that costs flash or RAM.

They are held differently:

  * **RAM** must match the baseline exactly: static RAM (``data`` and ``bss``) and
    ``main``'s frame, which holds the server while it is built and the future ``run``
    returns. A change either way is recorded in the commit that makes it. The match can
    be exact because the toolchain is pinned; layouts, inlining and which statics survive
    linking all move with the compiler, so a toolchain bump re-records the baseline.
  * **Flash** (``text``) may move by up to ``TEXT_TOLERANCE_PERCENT`` either way before the
    check fails, because code size moves with inlining decisions a change did not intend.
    A larger shrink fails too, so that the smaller baseline is recorded and later growth
    is measured from it.

The stack ``main``'s callees use is not measured; ``main``'s own frame is, because it is
where the probe's stack cost is.

Stdlib only, so it needs no environment.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

TEXT_TOLERANCE_PERCENT = 5

MAIN_SYMBOL = "__cortex_m_rt_main"


@dataclass(frozen=True)
class Sizes:
    """An image's sizes in bytes: ``llvm-size``'s Berkeley grouping, and ``main``'s frame."""

    text: int
    data: int
    bss: int
    main_frame: int


def parse_berkeley(output: str) -> tuple[int, int, int]:
    """``text``, ``data`` and ``bss`` from ``llvm-size``'s default output for one file.

    That is a header line, then ``text data bss dec hex filename``.
    """
    lines = [line.split() for line in output.splitlines() if line.strip()]
    if len(lines) != 2 or lines[0][:3] != ["text", "data", "bss"]:
        raise ValueError(f"not llvm-size output for one file:\n{output}")
    text, data, bss = (int(field) for field in lines[1][:3])
    return text, data, bss


_LABEL = re.compile(r"^[0-9a-f]+ <(?P<name>.+)>:$")
_PUSH = re.compile(r"^v?push(\.w)?\s+\{(?P<regs>[^}]*)\}")
_FRAME_POINTER = re.compile(r"^(add(\.w)?\s+r7, sp, #|mov\s+r7, sp$)")
_SUB_SP = re.compile(r"^sub(\.w|w)?\s+sp, (sp, )?#(?P<imm>0x[0-9a-f]+|\d+)")


def _register_bytes(register: str) -> int:
    return 8 if register.startswith("d") else 4


def parse_main_frame(disassembly: str) -> int:
    """The bytes ``main``'s prologue reserves, from ``llvm-objdump -d --no-show-raw-insn``.

    Those are its pushed registers and every ``sub sp`` before its first other
    instruction.
    """
    lines = iter(disassembly.splitlines())
    for line in lines:
        label = _LABEL.match(line.strip())
        if label and MAIN_SYMBOL in label["name"]:
            break
    else:
        raise ValueError(f"no {MAIN_SYMBOL} in the disassembly")

    frame = 0
    for line in lines:
        _, _, instruction = line.partition(":")
        instruction = instruction.strip()
        if push := _PUSH.match(instruction):
            frame += sum(_register_bytes(r.strip()) for r in push["regs"].split(","))
        elif sub := _SUB_SP.match(instruction):
            frame += int(sub["imm"], 0)
        elif not _FRAME_POINTER.match(instruction):
            break
    return frame


def compare(baseline: Sizes, measured: Sizes) -> list[str]:
    """What fails when ``measured`` is held to ``baseline``."""
    failures: list[str] = []
    for name in ("data", "bss", "main_frame"):
        was, now = getattr(baseline, name), getattr(measured, name)
        if now != was:
            failures.append(f"{name}: {now} bytes, baseline {was} ({now - was:+d})")
    allowed = baseline.text * TEXT_TOLERANCE_PERCENT // 100
    change = measured.text - baseline.text
    if abs(change) > allowed:
        failures.append(
            f"text: {measured.text} bytes, baseline {baseline.text} ({change:+d}), "
            f"past the {TEXT_TOLERANCE_PERCENT}% allowed ({allowed} bytes)"
        )
    return failures


def _run(tool: str, *args: str) -> str:
    return subprocess.run(
        [tool, *args], check=True, capture_output=True, text=True
    ).stdout


def _measure(llvm_bin: Path, image: Path) -> Sizes:
    text, data, bss = parse_berkeley(_run(str(llvm_bin / "llvm-size"), str(image)))
    disassembly = _run(
        str(llvm_bin / "llvm-objdump"), "-d", "--no-show-raw-insn", str(image)
    )
    return Sizes(text=text, data=data, bss=bss, main_frame=parse_main_frame(disassembly))


def main(argv: list[str]) -> int:
    """Check ``image`` against ``baseline``, or with ``--record``, rewrite the baseline."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--llvm-bin", required=True, type=Path, help="where llvm-size and llvm-objdump are"
    )
    parser.add_argument("--record", action="store_true", help="rewrite the baseline")
    parser.add_argument("baseline", type=Path)
    parser.add_argument("image", type=Path)
    args = parser.parse_args(argv)

    measured = _measure(args.llvm_bin, args.image)
    if args.record:
        args.baseline.write_text(json.dumps(asdict(measured), indent=2) + "\n")
        print(f"recorded {measured} in {args.baseline}")
        return 0

    baseline = Sizes(**json.loads(args.baseline.read_text(encoding="utf-8")))
    failures = compare(baseline, measured)
    print(f"{args.image}: {measured}")
    for line in failures:
        print(f"error: {line}", file=sys.stderr)
    if failures:
        print(
            "\nIf the change is intended, record it in the same commit: "
            "`just size-baseline`.",
            file=sys.stderr,
        )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
