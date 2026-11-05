# Contributing

Pull requests are welcome, as are bug reports and questions in the issue
tracker.

Start with [`docs/`](docs), the sphinx-needs requirement and architecture set.
It is the specification the code is written against, and for anything beyond a
small fix it is where the answer to "should this behave like that?" lives. Two
crates also carry a provisional `ARCHITECTURE.md`; those are being replaced by
the set under `docs/`, and where the two disagree, `docs/` wins.

## Getting set up

The Rust side needs nothing but a toolchain. The documentation set has its own
pinned Python environment, managed by [uv](https://docs.astral.sh/uv/):

```console
$ uv sync --frozen
$ just doctor
```

`just doctor` checks for `plantuml`, which renders every diagram in the
documentation set. It is a Java program invoked as a subprocess, so it cannot
be pinned in `uv.lock` like everything else — `brew install plantuml`, or the
apt package of the same name.

`--frozen` appears in every recipe. `needs.json` is consumed
as evidence against a committed snapshot, so the toolchain that produced a
given snapshot has to be recoverable; a recipe that silently re-resolved its
dependencies would break that.

## The gate

```console
$ just check-all
```

That runs the documentation checks, the test suite, clippy, and the bare-metal
builds. Run it before pushing. Individually:

| Recipe            | What it does                                                         |
| ----------------- | -------------------------------------------------------------------- |
| `just test`       | `cargo test --workspace --all-features`                              |
| `just clippy`     | clippy over all targets, warnings denied                             |
| `just embedded`   | builds the `no_std` crates for a bare-metal target, then `just size` |
| `just size`       | holds the embedded probe's flash and RAM to its recorded baseline    |
| `just check-docs` | requirement-set and governance checks, tool self-tests, needs build  |
| `just html`       | browsable documentation, which is how the set is meant to be read    |
| `just check`      | every pre-commit hook over every file                                |

`just --list` shows the rest.

**`just embedded` is not optional to think about.** A workspace lockfile
unifies features across members, so a `std` dependency enabled by one crate can
reach a `no_std` sibling and nothing a host build does will notice. That check
is the only thing that catches it.

**A size change is recorded in the commit that makes it.** `just size` fails
when the probe's static RAM (`data`, `bss`), its `main`'s stack frame or the
client transport's size changes at all, or its flash moves past 5% either way.
If the change is intended,
`just size-baseline` records it, and the diff to
`testing/embedded-probe/size-baseline.json` goes in the same commit. The stack
that `main`'s callees use is not measured, and a toolchain bump re-records the
baseline.

Install the hooks so the fast checks run on every commit:

```console
$ pre-commit install
```

## House rules

**Commits are [Conventional Commits](https://www.conventionalcommits.org/).**
A hook enforces the format, and `release-plz` derives the version bump from it,
so the type decides the next version.

**The crates release in lockstep.** There is one version, `[workspace.package]`
in the root `Cargo.toml`, which every crate inherits; every release publishes all
six at it and is tagged once, `v<version>`. A crate's own `CHANGELOG.md` says what
changed in that crate; it is written by hand, under `## [Unreleased]`, in the change
that makes the change. The `<crate>/v<version>` tags in the history are from before
the consolidation.

**A crate's scope is decided by which standard specifies the behavior**, not
by convenience. If a change would make a crate read a standard it does not own,
the boundary is wrong and the change belongs elsewhere. Each seam is declared
by the crate that calls through it, so every Cargo edge runs from implementor
to declarer. `uds_c` is the one crate scoped by an interface rather than a
standard: the C ABI firmware serves the stack through. It implements the
protocol crates' traits and reads no standard of its own.

**One lint standard, declared once.** `[workspace.lints]` in the root
`Cargo.toml` holds it, and every crate opts in with `[lints] workspace = true`,
save the C binding below.
It denies `unwrap`, `expect`, `panic`, `unreachable`, `todo`, `unimplemented`,
`indexing_slicing`, silent arithmetic and `as` casts, denies `clippy::pedantic`,
and **forbids `unsafe`** — forbid rather than deny, so it cannot be switched off
locally. A panic in a diagnostic stack is an unhandled failure in a
safety-related component, and every byte these crates decode comes off a wire.

Cargo will not let a crate inherit that table and add to it, so the table is the
whole policy. A crate that needs to differ says so as a crate-root attribute in
its own `lib.rs`, carrying a reason — visible in the file a reader opens first,
rather than buried in a manifest. Three derogations exist today, two of them
recorded there, and one is in a manifest:

- `uds_protocol` and `simple_doip` allow `indexing_slicing`, silent arithmetic
  and `as` casts. A zero-copy decoder indexes borrowed slices and computes
  offsets on every path, and both crates predate the standard: 162 and 29
  production sites respectively. These are gaps to close, not exemptions.
- `uds_c`, the C binding, restates the table in its manifest with `unsafe_code`
  at `deny` rather than `forbid`: it declares and calls C, which no attribute can
  allow under `forbid`. Each of its modules that does so allows `unsafe_code` with a
  reason, and `tools/check_governance.py` holds the restated table to the
  workspace's, entry for entry.
- `uds_protocol` and `simple_doip` also relax panic freedom **in test code only** — `#![cfg_attr(test, allow(…))]`, so production builds stay strict. Integration tests and examples
  are separate crates, so each states its own relaxation at the top of the file.

A local `allow` or `expect` carrying a reviewed justification is a legitimate
outcome anywhere; reaching for `unwrap` silently is not.

## Writing for the reader

Every crate here is published to crates.io, so its rustdoc, comments, README,
`ARCHITECTURE.md`, `MIGRATING.md` and changelog are read by an integrator who
never saw the previous version and cannot ask. Write all of it for that reader.
The test for any sentence: would it still be true and useful to someone reading
the code for the first time in two years?

1. **Documentation states the contract.** What the item does, its inputs,
   errors, panics and the clause of the standard it implements, in the present
   tense. No history, no alternatives considered, no account of how the code
   got here. A crate's boundary is stated as what its neighbors own, not as a
   section headed "What this crate does not do".
1. **Rationale gets one sentence at the smallest scope where it is true.**
   "Bounded so a stalled entity cannot grow the queue." If the reason needs its
   own heading, it needs its own page.
1. **Six lines is the ceiling for an explanation in code.** Past that, the
   reasoning is a `Rationale:` paragraph on the need it derives from, or an
   architecture page under `docs/`, and the comment becomes one sentence and a
   reference to it.
1. **Invariants are asserted, not described.** A `debug_assert!`, a `const _`
   assertion or a test says it better than prose and cannot drift. The word
   "load-bearing" does not appear.
1. **No voice.** No "we" or "our". No "deliberately" or "intentionally": state
   what the code does, and the reader will assume it was on purpose. No
   review-tool names, pull-request or issue numbers. A need ID is a reference
   to the specification, not a ticket, and belongs wherever it helps.
1. **Tests have plain names.** A short behavior name, with a one-line doc
   comment naming the need it verifies if one helps. A test guards a behavior,
   not an incident.
1. **The changelog carries history.** User-visible behavior and how to
   migrate. It is the only place "previously" belongs.
1. **Nothing is written for the next editor.** Decision history, incident
   write-ups and release ordering are not kept in the code. When a change
   removes a comment that held real reasoning, the reasoning either derives
   from a need and becomes that need's `Rationale:` paragraph, or describes a
   crate's shape and goes on an architecture page under `docs/`; what fits
   neither is history, and the pull request that made the change is its record.
1. **Spelling is American.** Behavior, neighbor, acknowledgment, canceled,
   initialize and license, in prose and in test names. Renaming a public
   identifier to match is a breaking change and gets a changelog entry.

`just prose` runs `tools/prose_lint.py`, which counts the phrases rule 5
excludes, the scope heading rule 1 excludes, the comment blocks past rule 3's
ceiling and rule 9's British spellings, over every comment and Markdown heading
in the tree and, for spelling, every Markdown file but a changelog. CI fails on
a hard-fail phrase under a path that has had its writing pass (the `--strict`
list in `ci.yml`) and reports the rest.

## Requirements and architecture

Needs are authored as reStructuredText under `docs/`, one directive per need,
carrying an ID, a status, an origin and — where transcribed from a standard —
the clause it came from.

IDs are allocated, never renumbered and never reused. They become permanent as
soon as anything outside this repository links to one. `tools/validate_needs.py`
enforces the parts of that policy sphinx-needs has no concept of, and it has
its own tests; run `just check-docs` after adding a need.

A need either cites the standard it was transcribed from, or states in a
`Rationale:` paragraph why it was derived. Derived reasoning is the part that
cannot be recovered later, which is why the split is a field rather than a
habit — `just summary` reports it on every run.

Needs are written to be verifiable without the standard in hand. `source`
records where a requirement came from, not what it means.

## Security

Do not open a public issue for a security problem. See [SECURITY.md](SECURITY.md).

## Licensing of contributions

Every crate here is dual licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) at the user's option, from the one pair of files
at the repository root — each crate's `LICENSE-MIT` and `LICENSE-APACHE` is a
symlink to them, so there is one text to keep current and no way for a crate to
drift from it.

Unless you state otherwise, any contribution you intentionally submit for
inclusion in this work, as defined in the Apache-2.0 license, is dual licensed
on those same terms, with no additional conditions.

That covers the requirement and architecture set under `docs/` as well as the
code. It grants nothing in the ISO standards themselves, which remain ISO's —
see [Relationship to the standards](README.md#relationship-to-the-standards).
