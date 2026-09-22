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

| Recipe            | What it does                                                      |
| ----------------- | ----------------------------------------------------------------- |
| `just test`       | `cargo test --workspace --all-features`                           |
| `just clippy`     | clippy over all targets, warnings denied                          |
| `just embedded`   | builds the `no_std` crates for a bare-metal target                |
| `just check-docs` | requirement-set policy checks, tool self-tests, needs build       |
| `just html`       | browsable documentation, which is how the set is meant to be read |
| `just check`      | every pre-commit hook over every file                             |

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

**Panic freedom is enforced where it matters.** `uds_session`, `uds_services`
and `uds_on_ip` deny `unwrap`, `expect`, `panic`, `unreachable`, `todo`,
`indexing_slicing` and silent arithmetic. A panic in a diagnostic server is an
unhandled failure in a safety-related component. A local `allow` carrying a
reviewed justification is a legitimate outcome; reaching for `unwrap` silently
is not.

**`unsafe` is forbidden**, not merely denied, in those same three crates.

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
