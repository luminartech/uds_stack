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

`--frozen` is deliberate and appears in every recipe. `needs.json` is consumed
as evidence against a committed snapshot, so the toolchain that produced a
given snapshot has to be recoverable; a recipe that silently re-resolved its
dependencies would break that.

## The gate

```console
$ just check-all
```

That runs the documentation checks, the test suite, clippy, and the bare-metal
builds. Run it before pushing. Individually:

| Recipe            | What it does                                                        |
| ----------------- | ------------------------------------------------------------------- |
| `just test`       | `cargo test --workspace --all-features`                             |
| `just clippy`     | clippy over all targets, warnings denied                            |
| `just embedded`   | builds the `no_std` crates for a bare-metal target                  |
| `just check-docs` | requirement-set and governance checks, tool self-tests, needs build |
| `just html`       | browsable documentation, which is how the set is meant to be read   |
| `just check`      | every pre-commit hook over every file                               |

`just --list` shows the rest.

**`just embedded` is not optional to think about.** A workspace lockfile
unifies features across members, so a `std` dependency enabled by one crate can
reach a `no_std` sibling and nothing a host build does will notice. That check
is the only thing that catches it.

Install the hooks so the fast checks run on every commit:

```console
$ pre-commit install
```

## House rules

**Commits are [Conventional Commits](https://www.conventionalcommits.org/).**
A hook enforces the format, and `release-plz` derives changelogs and version
bumps from it, so `fix:` and `feat:` are load-bearing rather than decorative.

**Crate versions are independent.** A release of one crate says nothing about
the others, and tags are namespaced per crate — `uds_protocol/v0.1.0`.

**A crate's scope is decided by which standard specifies the behaviour**, not
by convenience. If a change would make a crate read a standard it does not own,
the boundary is wrong and the change belongs elsewhere. Each seam is declared
by the crate that calls through it, so every Cargo edge runs from implementor
to declarer.

**One lint standard, declared once.** `[workspace.lints]` in the root
`Cargo.toml` holds it, and every crate opts in with `[lints] workspace = true`.
It denies `unwrap`, `expect`, `panic`, `unreachable`, `todo`, `unimplemented`,
`indexing_slicing`, silent arithmetic and `as` casts, denies `clippy::pedantic`,
and **forbids `unsafe`** — forbid rather than deny, so it cannot be switched off
locally. A panic in a diagnostic stack is an unhandled failure in a
safety-related component, and every byte these crates decode comes off a wire.

Cargo will not let a crate inherit that table and add to it, so the table is the
whole policy. A crate that needs to differ says so as a crate-root attribute in
its own `lib.rs`, carrying a reason — visible in the file a reader opens first,
rather than buried in a manifest. Two such derogations exist today, both
recorded there:

- `uds_protocol` and `simple_doip` allow `indexing_slicing`, silent arithmetic
  and `as` casts. A zero-copy decoder indexes borrowed slices and computes
  offsets on every path, and both crates predate the standard: 162 and 29
  production sites respectively. These are gaps to close, not exemptions.
- The same two relax panic freedom **in test code only** — `#![cfg_attr(test, allow(…))]`, so production builds stay strict. Integration tests and examples
  are separate crates, so each states its own relaxation at the top of the file.

A local `allow` or `expect` carrying a reviewed justification is a legitimate
outcome anywhere; reaching for `unwrap` silently is not.

**Comments carry the reasoning.** Much of this codebase explains *why* a thing
is the way it is, including the rejected alternative. That is deliberate — the
reasoning behind a design decision cannot be reconstructed afterwards. Please
keep it up rather than trimming it away.

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
inclusion in this work, as defined in the Apache-2.0 licence, is dual licensed
on those same terms, with no additional conditions.

That covers the requirement and architecture set under `docs/` as well as the
code. It grants nothing in the ISO standards themselves, which remain ISO's —
see [Relationship to the standards](README.md#relationship-to-the-standards).
