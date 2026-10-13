#!/usr/bin/env python3
"""Hold the embedded probe's image to its recorded size.

``testing/embedded-probe`` links the sensor's whole server path for bare metal, and this
compares it with the baseline recorded beside it: its sections, as ``llvm-size`` reports
them, the stack frame its ``main`` reserves, as ``llvm-objdump`` shows the prologue, and
the client transport's size, which the probe keeps in a ``static`` for this to read.
Nothing else in the gate notices a change that costs flash or RAM.

They are held differently:

  * **RAM** must match the baseline exactly: static RAM (``data`` and ``bss``),
    ``main``'s frame, which holds the server while it is built and the future ``run``
    returns, and the client transport, which a tester holds by value. A change either
    way is recorded in the commit that makes it. The match can be exact because the
    toolchain is pinned; layouts, inlining and which statics survive linking all move
    with the compiler, so a toolchain bump re-records the baseline.
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

CLIENT_TRANSPORT_SYMBOL = "CLIENT_TRANSPORT_SIZE"


@dataclass(frozen=True)
class Sizes:
    """An image's sizes in bytes: ``llvm-size``'s Berkeley grouping, ``main``'s frame, and
    the client transport's ``size_of``."""

    text: int
    data: int
    bss: int
    main_frame: int
    client_transport: int


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


def parse_symbol(table: str, name: str) -> tuple[int, str]:
    """Where the one symbol whose demangled name is ``name``, in any module and with or
    without the compiler's hash, is, and in which section, from
    ``llvm-objdump -t --demangle``: ``address flags section size name``."""
    named = re.compile(rf"(^|::){re.escape(name)}(::h[0-9a-f]+)?$")
    found = [
        (int(fields[0], 16), fields[-3])
        for line in table.splitlines()
        if len(fields := line.split()) >= 4 and named.search(fields[-1])
    ]
    if len(found) != 1:
        raise ValueError(f"{len(found)} symbols named {name} in the image")
    return found[0]


def parse_word(contents: str, at: int) -> int:
    """The little-endian 32-bit word at ``at``, a ``usize`` on the probe's target, from
    ``llvm-objdump -s``'s dump of its section: sixteen bytes a line as four hex words,
    the line's address first. Lines start at the section's address, which need not be
    16-aligned, so the word is read from the line whose sixteen bytes hold ``at``; ``at``
    must sit on a word of that line."""
    for line in contents.splitlines():
        fields = line.split()
        if len(fields) < 2:
            continue
        try:
            start = int(fields[0], 16)
        except ValueError:
            continue
        if not start <= at < start + 16:
            continue
        offset = at - start
        words = fields[1:5]
        if offset % 4 != 0 or offset // 4 >= len(words):
            raise ValueError(f"{at:#x} is not a word of the line at {start:#x}")
        return int.from_bytes(bytes.fromhex(words[offset // 4]), "little")
    raise ValueError(f"no line holds {at:#x} in the dump:\n{contents}")


def compare(baseline: Sizes, measured: Sizes) -> list[str]:
    """What fails when ``measured`` is held to ``baseline``."""
    failures: list[str] = []
    for name in ("data", "bss", "main_frame", "client_transport"):
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
    objdump = str(llvm_bin / "llvm-objdump")
    text, data, bss = parse_berkeley(_run(str(llvm_bin / "llvm-size"), str(image)))
    disassembly = _run(objdump, "-d", "--no-show-raw-insn", str(image))
    at, section = parse_symbol(
        _run(objdump, "-t", "--demangle", str(image)), CLIENT_TRANSPORT_SYMBOL
    )
    client_transport = parse_word(
        _run(objdump, "-s", f"--section={section}", str(image)), at
    )
    return Sizes(
        text=text,
        data=data,
        bss=bss,
        main_frame=parse_main_frame(disassembly),
        client_transport=client_transport,
    )


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
