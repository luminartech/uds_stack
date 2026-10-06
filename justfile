# Command runner for the uds_stack workspace: five crates and one requirement set.
#
# The Python toolchain is pinned in pyproject.toml and locked in uv.lock, and every recipe
# below runs it through `uv run --frozen`. `--frozen` is deliberate: needs.json is consumed
# as evidence against a committed snapshot, so the toolchain that produced a given snapshot
# has to be recoverable. A recipe that silently re-resolved dependencies would break that.
#
# Each recipe carries a `[doc]` attribute rather than relying on the preceding comment,
# because `just --list` shows only the final comment line and would otherwise present the
# tail of an explanation as the summary.

# Where Sphinx writes its output. Gitignored.
build_dir := "docs/_build"

# The bare-metal target the no_std crates are checked against. thumbv7em-none-eabihf
# stands in for AURIX TC4x, which rustup does not ship: no std, no alloc by default.
embedded_target := "thumbv7em-none-eabihf"

# The target CI's Miri job runs on; see `miri`.
miri_target := "x86_64-unknown-linux-gnu"

[doc("Show the available recipes")]
default:
    @just --list

# --- documentation set -----------------------------------------------------------------

# Warnings are errors here, so a need excluded from every toctree, an unknown directive,
# or an unresolved link fails the build rather than passing quietly.
[doc("Build the need set and emit needs.json")]
docs:
    uv run --frozen sphinx-build -b needs -W -q docs {{ build_dir }}/needs

# Not part of any check. This is how the set is meant to be read: rendered, with the
# diagrams drawn, the generated ones clickable, and the tables resolved.
#
# Needs the `plantuml` command on PATH -- `brew install plantuml` -- or $PLANTUML set to
# a command such as `java -jar plantuml.jar`. Not the Ubuntu apt package: it is years
# behind and rejects the architecture diagrams' syntax. docs.yml pins the release CI
# uses. It is a Java program and cannot be pinned in uv.lock; `just doctor` checks for it.
[doc("Build browsable HTML documentation")]
html:
    uv run --frozen sphinx-build -b html -W -q docs {{ build_dir }}/html
    @echo "open {{ build_dir }}/html/index.html"

# The parts of the process Sphinx has no concept of: the ID scheme and global uniqueness,
# mandatory fields per type, permitted field values, forbidden fields per type, the
# integrity-level ratchet, and that a transcribed need records its source while a derived
# one states a rationale.
[doc("Run the requirement-set policy checks")]
validate:
    python3 tools/validate_needs.py

# Cargo has no workspace-level licence file: it packages what sits in the crate
# directory. Each crate therefore symlinks the root pair, which `cargo package`
# dereferences into the published archive. This checks the arrangement is intact.
[doc("Check the licence and governance files are shared, not copied")]
governance:
    python3 tools/check_governance.py

# Run this after adding needs. A count that did not move by the expected amount means a
# directive was skipped rather than rejected, which is the failure mode this stack has
# been bitten by before.
#
# The derived fraction is the number to watch. Clause 8.7 fixes the response codes and the
# validation order but says nothing about trait design, associated types or macros, so
# uds_services carries the highest derived fraction in the stack by some margin. A derived
# need records reasoning that cannot be reconstructed afterwards, which is why the split is
# reported on every run rather than counted when someone remembers to ask.
[doc("Summarise the built set: counts, source coverage, ID gaps, per prefix")]
summary: docs
    #!/usr/bin/env python3
    import json
    import pathlib
    path = pathlib.Path("{{ build_dir }}") / "needs" / "needs.json"
    data = json.loads(path.read_text())
    needs = data["versions"][data["current_version"]]["needs"]
    # Derived from the set itself rather than hardcoded, so a newly authored crate appears
    # here the day its first need lands instead of the day someone remembers this file.
    seen = sorted({
        k.rsplit("_", 2)[0]
        for k in needs
        if len(k.rsplit("_", 2)) == 3 and k.rsplit("_", 2)[1] in ("ARCH", "LLR")
    })
    for prefix in seen:
        for kind in ("ARCH", "LLR"):
            ids = sorted(k for k in needs if k.startswith(f"{prefix}_{kind}_"))
            if not ids:
                continue
            sourced = [k for k in ids if needs[k].get("source")]
            numbers = [int(k.rsplit('_', 1)[1]) for k in ids]
            gaps = sorted(set(range(min(numbers), max(numbers) + 1)) - set(numbers))
            print(f"{prefix}_{kind}:")
            print(f"  total:       {len(ids)}")
            print(f"  transcribed: {len(sourced)}")
            print(f"  derived:     {len(ids) - len(sourced)}")
            print(f"  range:       {min(numbers):04d}-{max(numbers):04d}")
            print(f"  gaps:        {gaps if gaps else 'none'}")

# --- crates ----------------------------------------------------------------------------

[doc("Run the workspace test suite")]
test:
    cargo test --workspace --all-features

# `-D warnings` promotes anything a crate's Cargo.toml lint table leaves at warn level, and
# `--all-targets` means the lints are enforced in tests too, not just the libraries.
[doc("Lint the workspace at the level pre-commit enforces")]
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# A broken intra-doc link renders as plain text and fails nothing else, so a renamed item
# leaves its references silently dangling. Denied here so the rename fails instead.
[doc("Build the workspace's API docs, failing on a broken intra-doc link")]
doc:
    RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D warnings" cargo doc --workspace --all-features --no-deps

# The check host builds cannot make. A workspace Cargo.lock unifies features across
# members, so a std dependency enabled by one crate can reach a no_std sibling; nothing
# below `cargo test` on the host would notice.
[doc("Build the no_std crates for a bare-metal target")]
embedded:
    rustup target add {{ embedded_target }}
    cargo build -p uds_protocol --target {{ embedded_target }} --no-default-features
    cargo build -p uds_session  --target {{ embedded_target }} --no-default-features
    cargo build -p uds_services --target {{ embedded_target }} --no-default-features
    cargo build -p uds_on_ip    --target {{ embedded_target }} --no-default-features
    cargo build -p simple_doip  --target {{ embedded_target }} --no-default-features --features connection
    cargo build -p embassy-net-entity --target {{ embedded_target }}
    cargo build -p uds_protocol --target {{ embedded_target }} --no-default-features --features alloc
    cargo build -p uds_on_ip    --target {{ embedded_target }} --no-default-features --features alloc

# CI's Miri job, run as rust-ci.yml runs it: default members and features, with proptest
# told not to write regression files from inside the interpreter, which has no filesystem
# access. Plain `cargo test` passes a leak or an out-of-bounds read that Miri fails, so a
# change that only `check-all` has seen can still fail CI here. Not part of `check-all`:
# it needs a nightly toolchain and takes several minutes.
#
# The target is CI's, on any host: Miri interprets rather than runs, so it needs no linker
# for it, and a macOS host target would fail on tokio's `kqueue`, which Miri does not
# emulate. Linux's `epoll` it does.
[doc("Run the test suite under Miri, as CI does")]
miri:
    rustup toolchain install nightly --component miri --profile minimal --no-self-update
    PROPTEST_DISABLE_FAILURE_PERSISTENCE=1 \
        MIRIFLAGS="-Zmiri-env-forward=PROPTEST_DISABLE_FAILURE_PERSISTENCE" \
        cargo +nightly miri test --target {{ miri_target }}

# A guard rail is only verified by watching it fail, so each check in validate_needs.py is
# demonstrated stopping something.
[doc("Run the self-tests for the policy checkers")]
test-tools:
    python3 -m unittest discover --start-directory tools --pattern 'test_*.py'

# --- aggregate --------------------------------------------------------------------------

# Every hook, over every file, in configured order. Run this before pushing: CI builds the
# documentation set but runs none of these hooks, so a failure they would have caught
# reaches the pull request instead.
[doc("Run every pre-commit hook over every file (the gate before pushing)")]
check:
    pre-commit run --all-files

[doc("The fast subset a documentation change needs; use `check` before pushing")]
check-docs: test-tools validate governance docs

[doc("The full gate: crates and docs")]
check-all: check-docs test clippy embedded doc

[doc("Remove build output")]
clean:
    rm -rf {{ build_dir }}
    cargo clean

# The documentation build has one dependency uv cannot manage, so it gets one check that
# names it plainly. Without the renderer the failure is a Sphinx warning about a subprocess,
# which is a long way from "install plantuml".
[doc("Check the tools the docs build needs but cannot pin")]
doctor:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -n "${PLANTUML:-}" ]]; then
        # conf.py prefers $PLANTUML over the PATH, so check what it will actually run.
        echo "plantuml: \$PLANTUML = $PLANTUML"
        $PLANTUML -version | head -n 1
    elif command -v plantuml > /dev/null; then
        echo "plantuml: $(command -v plantuml)"
        plantuml -version | head -n 1
    else
        echo "plantuml: MISSING — 'brew install plantuml', or set PLANTUML to 'java -jar <plantuml.jar>' using the release docs.yml pins (the apt package is too old)" >&2
        exit 1
    fi
