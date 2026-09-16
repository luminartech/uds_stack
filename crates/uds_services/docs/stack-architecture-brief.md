# Stack architecture brief

**Carry this to `uds_protocol`, `uds_session`, `uds_on_ip` and `simple_doip`.**

`uds_services` now renders its architecture as a sphinx-needs set. This brief says how to
do the same in each of the other repositories, so the four sets read as one stack and can
eventually be merged through `needs_external_needs`. It also lists the questions each
repository has to answer for the stack to fit together — those are in the last section,
and they are the reason this is being done before publication rather than after.

Excluded from this crate's Sphinx build: it instructs other repositories, and is not part
of `uds_services`' architecture.

## 1. What to copy

`uds_session` is the reference for the apparatus; `uds_services` is the reference for
having an architecture section in it. Copy from `uds_services`:

| File                           | Change on the way in                                                                    |
| ------------------------------ | --------------------------------------------------------------------------------------- |
| `pyproject.toml`               | Rename the project to `<crate>-docs`                                                    |
| `uv.lock`                      | Rename the one virtual root package to match, then `uv sync --frozen`                   |
| `.python-version`              | As is — 3.12, and the bound in `pyproject.toml` is load-bearing                         |
| `justfile`                     | Replace the crate name; keep every recipe                                               |
| `.pre-commit-config.yaml`      | As is                                                                                   |
| `.github/workflows/docs.yml`   | As is. Pages must be enabled with Source = GitHub Actions                               |
| `tools/validate_needs.py`      | Change the ID prefix (§2)                                                               |
| `tools/test_validate_needs.py` | Change the ID prefix and the fixture subject matter                                     |
| `docs/conf.py`                 | Change `project`, `version`, `needs_id_regex` (§2)                                      |
| `docs/_plantuml/style.puml`    | As is — the shared diagram style (§4)                                                   |
| `.gitignore` additions         | `.venv/`, `/docs/_build/`, `/docs/superpowers/`, `/.superpowers/`, `tools/__pycache__/` |

`uds_session` already has all of this except the `arch` type and the diagram toolchain;
it needs §2, §3 and §4.

Everything runs through `uv run --frozen`. Do not relax that: `needs.json` is consumed as
evidence against a committed snapshot, so the toolchain that produced a given snapshot has
to be recoverable.

## 2. ID prefixes — one per crate, and they must not collide

Sets are merged through `needs_external_needs`, and a collision there is silent. Allocate
one prefix per repository and put it in both `docs/conf.py` and `tools/validate_needs.py`:

| Crate          | Prefix    | Status         |
| -------------- | --------- | -------------- |
| `uds_session`  | `UDSS_`   | Already in use |
| `uds_services` | `UDSSVC_` | Allocated      |
| `uds_protocol` | —         | To allocate    |
| `uds_on_ip`    | —         | To allocate    |
| `simple_doip`  | —         | To allocate    |

Full pattern, with the crate's prefix substituted:

```python
needs_id_regex = r"^<PREFIX>_(ARCH|LLR)_\d{4}$|^<PREFIX>_(IMPL|TEST)_[A-Z0-9_]+$"
```

IDs are allocated, never renumbered, never reused, and permanent from the moment anything
outside the repository links to one. While a set is draft and nothing links to it,
renumbering is still free — say so on the index page, as both existing sets do.

## 3. The `arch` need type

Four changes to `uds_session`'s schema. All three are marked as divergences in
`uds_services/docs/conf.py`; copy them from there rather than from this description.

1. **A fourth need type**, directive `arch`, prefix `ARCH_`, authored by hand like `llr`
   and unlike the generated `impl`/`test`.

1. **`arch` requires `id`, `status`, `origin` — and must not carry `integrity_level` or
   `target_level`.** The forbidding half is the one that is easy to skip and the one that
   matters: requiring the pair on `llr` does not stop anyone adding it to an `arch`, and
   once one architecture element carries a level, the integrity report becomes a mix of
   substantiated claims and decoration. An architecture element is a structural decision,
   not a claim with evidence behind it.

1. **`validate_needs.py` scans both authored sections**, `docs/architecture` and
   `docs/requirements`, and its refuse-an-empty-set guard counts either type. A missing
   section is not a failure — during the prototype phase the architecture is the whole set.

1. **Two link types, `depends_on` and `part_of`.** These are what make the architecture a
   graph rather than a list, and they are what `needflow` renders. Populate them; an
   element with no edge in either direction is either genuinely standalone or
   under-described, and the generated diagram makes which one obvious. Use `part_of` only
   where containment is real — `uds_services` leaves its four seams without a parent
   rather than inventing one to tidy the diagram.

`origin`, `source` and the mandatory `Rationale:` paragraph on a derived need apply to
`arch` exactly as they do to `llr`. That rule is the point of the whole exercise for
architecture: an architecture element either cites the clause that forces it, or records
why we chose it. Reasoning that is not written down at the moment of choosing cannot be
reconstructed afterwards.

Ten of the self-tests in `uds_services/tools/test_validate_needs.py` cover these rules,
including both directions of the ID check. Copy them; a guard rail is only verified by
watching it fail.

## 4. Diagrams — PlantUML, and the traps in it

**Every diagram is PlantUML.** `sphinx.ext.graphviz` ships with Sphinx and needs no Python
dependency, but it cannot draw a sequence diagram and its raster output is not clickable,
so it would sit alongside PlantUML rather than replace any of it. One diagram language.

Three pieces to copy:

- **`sphinxcontrib-plantuml==0.31`** in `pyproject.toml`, then `uv lock`. It is the Python
  binding, not the renderer.
- **The renderer itself**, which uv cannot pin because it is a Java program invoked as a
  subprocess. `brew install plantuml` locally, `apt install plantuml` in CI — there is a
  step for it in `docs.yml` and a `just doctor` recipe that names it, because the failure
  without it is a Sphinx warning about a subprocess and nothing resembling "install
  plantuml".
- **`docs/_plantuml/style.puml`**, applied to every diagram through `-config` from
  `conf.py`. Copy it unchanged so the four sets look like one stack, and add it to
  `exclude_patterns` — it is renderer configuration, not a document, and warnings are
  errors.

In `conf.py`, set `needs_flow_engine = "plantuml"` and `plantuml_output_format = "svg_obj"`.

Four things cost time here. They are written down so nobody pays for them twice:

1. **`plantuml_output_format` must be `svg_obj`, not `svg` or `svg_img`.** `svg_img`
   embeds through `<img>`, which renders identically and swallows every hyperlink — the
   generated diagrams lose their whole point. Plain `svg` additionally writes a PNG
   fallback that `needflow` then asks to scale; scaling a raster needs Pillow, and without
   it the build warns, which is an error. `svg_obj` emits the SVG alone, through
   `<object>`, and keeps the links.
1. **Do not pass `:scale:` to a `needflow`.** Same Pillow warning, same failed build.
1. **`skinparam padding` is deprecated** and PlantUML renders the deprecation notice *into
   the image* — a yellow banner on every diagram in the set. It is already omitted from
   the shared style file; do not add it back.
1. **The documented `#colour:activity;` form for activity diagrams does not parse** in
   PlantUML 1.2026.8. It is a hard syntax error, not a silent fallback to plain, and the
   reported line number points at the enclosing `if` rather than at the offending line,
   which makes it look like unbalanced `if`/`endif`. Use the `<style>` classes in the
   shared file instead: `:0x13; <<negative>>`, with `negative`, `positive` and `silent`
   defined centrally. Note the placement — `:text; <<class>>` styles the step, while
   `:text <<class>>;` prints the class name into the label and styles nothing.

Also watch for a trap that is not PlantUML's: `sphinx-build ... | tail` reports `tail`'s
exit code, not the build's. Two builds in this repository were believed green while
failing under `-W`. Check the build's own status.

**What to draw.** `uds_services` has seven hand-drawn diagrams and four generated ones.
The ones that earned their place were the ones showing something the prose could only
assert:

- a **sequence diagram** per interesting path — one for the ordinary request, one for the
  response-pending case. This is what PlantUML is for and what graphviz cannot do.
- an **activity diagram** for anything with a validation order, with each failure exit
  labelled. Nesting the checks makes the short-circuit visible.
- a **component diagram** for the layering, and a separate small one for any place where
  call direction and dependency direction disagree. That contrast is invisible in prose.
- a **`needflow`** per page, filtered to that page's elements, plus one over the whole set
  as a clickable map.

One warning from doing it here: a diagram is where a missing role shows up. This set's
layering diagram had a single "application" box for a long time, and drawing the client
and server applications as the two separate programs they are is what made the gap in
§7 obvious.

## 5. Where the architecture document goes

**`docs/architecture/`, inside the Sphinx set, in reStructuredText.**

`uds_on_ip` and `simple_doip` each already have a root `ARCHITECTURE.md` — 779 and 618
lines — that nothing renders and nothing checks. Move it in and convert it. The
`validate-needs` pre-commit hook deliberately globs `.md` inside the authored sections, so
a file moved but not converted is reported rather than silently skipped.

Converting is also the opportunity to turn the prose into architecture elements. Not every
paragraph should become one. What earns an ID is a component, a seam, or a decision that
something else depends on — the things a reader needs to point at, and the things whose
reasoning is worth pinning down. `uds_services` produced eighteen from a crate that has no
code yet, split seven transcribed to eleven derived; `just summary` reports that split, and
watching it is the point.

Suggested page structure, which `uds_services` follows:

```
docs/architecture/
  index.rst            # the organising rule, needtables over all elements
  <structure pages>    # components and stages, one page per cohesive area
  seams.rst            # what crosses each boundary, and who owns each side
  not-owned.rst        # what lives elsewhere, and why it was decided against
  open-questions.rst   # no needs; deleted when its last entry is answered
```

Keep `open-questions.rst`. Both existing sets have one, and it is where a reader looks
first.

## 6. What each repository has to answer

**Rewritten 2026-09-16.** The version this replaces was written before the driver moved into
`uds_services` and before the boundary briefs were issued, and it had gone stale in five
places — it gave "whoever is called declares the interface" as the ownership rule, described
`uds_on_ip`'s `Ai` as DoIP-shaped and unusable by the typed layer, asked `uds_session` to
expose that a `0x78` had been sent, described a `Ctx` that crosses a crate boundary, and had
`uds_services` defining its own physical/functional addressing type. None of those is now
true. **The per-crate boundary briefs under `diagnostics/briefs/` are current; this section
is the index to them, not a second source.**

Two rules replaced the ones this section used to carry, and both are worth stating because
the superseded versions are the more plausible ones:

- **Ownership follows the specifying document, not the direction of the call.** "Whoever is
  called declares the interface" produced a tidy symmetry and was wrong: a seam carrying an
  addressing triple, bytes and a sink is not transport-shaped, so every binding would have
  declared the same trait separately.
- **`uds_services` drives.** It owns the `uds_session::Session` instance, supplies every
  input, drains every action, and calls a transport through a trait it declares. `uds_session`
  encodes the ISO 14229-2 state machine and drives nothing; a binding is a transport
  implementation with no driver and no notion of a service. The driver was the one component
  that went a full design cycle with no owner, each crate declining it in turn.

### `uds_on_ip`

See `briefs/uds_on_ip-boundary-brief.md`. It becomes a transport implementation: most of the
ISO 14229-2 vocabulary it currently holds moves to `uds_session`, and what remains is the
ISO 14229-5 profile, the DoIP mapping, and an impl of `uds_services::UdsTransport`.

Still open and blocking: its client is entirely `todo!()`, so the typed client surface can be
designed against its shape but not exercised. Each `DataInd` must carry the responding
server's `S_AI[SA]` — it is the only way to attribute functional responses, and functional
addressing currently collapses to a single response, which is a shape change rather than a
fix.

**New from the conformance pass:** `max_payload()` cannot cite ISO 14229-5 REQ 7.17, which is
a *ReadDataByPeriodicIdentifier* requirement. The real bound is DoIP's Max. data size, which
ISO 13400-2:2019 Table 11 marks **optional** and defines as the maximum size of one logical
*request*. Both facts need an answer before the trait is written.

### `uds_session`

See `briefs/uds_session-boundary-brief.md` and `briefs/uds_session-id-remap-2026-09-16.md`.
It is ISO 14229-2 whole, sans-io, with no dependencies and no outward traits. It declares no
`RequestHandler`, no `PendingResponder`, no `DiagnosticClient` and no `Ctx` — the earlier
arrangement that gave it all four is discarded, and is recorded in that brief because it is
more plausible than the real one and will otherwise be re-proposed.

It owes the stack one thing: **publish `needs.json`**. It is built at
`docs/_build/needs/needs.json` and `.gitignore` puts it out of reach, so no sibling can wire
`needs_external_needs` against it. Until that lands, every cross-repo citation in the stack is
unverifiable prose — which is exactly how `uds_services` came to carry nine broken ones
through a renumber, all of them correct when written.

Confirm also that `not-owned.rst`'s ownership table matches what this crate believes it owns,
particularly that `tP4_Server` is a performance requirement rather than a timer to run. *(This
one is now checked: ISO 14229-2:2021 REQ 5.3 types it exactly that way.)*

### `simple_doip`

See `briefs/simple_doip-boundary-brief.md`. Two server seams exist and it is not settled which
is canonical: the bare-metal entity exports a `fn(&[u8], &mut [u8]) -> i32` request callback
in production code. Under the current arrangement that callback has no place — the driver is
two layers above it — so the question is whether it is deleted or kept as a bare-metal
convenience that does not compose with the typed path.

Also owed upward: the entity's Max. data size, with the qualification above.

### `uds_protocol`

See `briefs/uds_protocol-boundary-brief.md`. It is the most load-bearing of the four, because
one set of message definitions has to serve both roles in both build environments.

All four paths through a message definition are now in use, so every modelled service must be
constructible from native values and not only decodable from the wire — check that first when
adding one. Keep re-exporting `Encode` and `Decode`: `UDSSVC_ARCH_0026` gets a data record's
extent from the record type's own decoder rather than from a hand-written length, so
`Decode`'s "decode from the FRONT of `buf`, return `(value, unconsumed_remainder)`" wording is
a contract now, not a convention.

Two decode properties `uds_services` depends on: the error taxonomy must map totally onto
`0x13`, and the unmodelled-service variant must mean `0x11` rather than `0x13` — a request
naming a service it does not model may be perfectly well formed.

*(Checked in the conformance pass: ISO 14229-1:2020 11.2.1 does say a request "may contain the
same dataIdentifier multiple times" and that the server "shall respond with data for each
dataIdentifier as often as requested", so anything collecting identifiers into a set is
non-conformant. Worth the test that says so.)*

The content gap bounds the release rather than the boundary work: eight services are named in
`UdsServiceType` with no message type. Four of them — `0x2A`, `0x2F`, `0x87`, `0x84` — are
among the twelve that ISO 14229-1 Table 23 fixes as unavailable in the defaultSession, so the
session-permission rule `uds_services` is taking on is partially unreachable for the same
reason its session-transition matrix is.

## 7. The scope trap, which cost this repository a rewrite

Worth reading before you write your own `ARCH_0001`, because every crate in this stack is
positioned to make the same mistake.

`uds_services`' scope was first written as "clause 8.7 and nothing else". That is true
about what it *implements* and badly incomplete about what it *is*: it is also the point
where an application meets the stack, in both directions, and the client half was missing
from the architecture entirely — no elements, no seams, no diagrams.

The evidence had been read and not followed up. `uds_on_ip`'s own architecture names three
portable surfaces — "defining identifiers, implementing handlers, and *exchanging
requests*" — and only two were modelled. Its client returns raw bytes and says
interpreting them belongs to a higher layer. The crate is called `uds_services`, not
`uds_server`.

**The generalisation.** A crate's ISO clause tells you what it implements. It does not tell
you what it *is* to the crates and applications around it, and the one-crate-per-ISO-document
rule quietly encourages conflating the two. So write both claims, separately:

- one element bounding the standard behaviour implemented, which stays checkable;
- one element stating the crate's architectural role — who integrates against it, in which
  direction, and what they can no longer do for themselves.

Do not merge them. A scope statement stretched to cover the second claim stops being
usable for answering "does this belong here?", which is the only thing it was for.

**A third claim to write down where it applies**, which came out of the same
conversation: a crate may serve two *build environments* as well as two roles. Here the
server role compiles into embedded firmware without `std` or `alloc` and the client role
into host tooling with both, from one set of definitions. That is not a deployment detail
to be discovered at CI time — it constrains the types, and `UDSSVC_ARCH_0027` states it.
If your crate is on both sides of that line, say so in an element and make sure both
configurations are actually built.

**Author from the PDF, not from the markdown conversion.** This is the rule the 2026-09-16
conformance pass produced, and it cost `uds_services` one substantively wrong element to
learn.

The markdown under `~/dev/luminar/iso_specs/markdown/` is reliable for prose and unreliable
for exactly the structures an architecture is authored from. Confirmed in one afternoon
against ISO 14229-1:2020 alone:

- **Figures are images with no text.** Figure 5 and Figure 6 — the whole of clause 8.7's
  validation order — are PNGs. So are Figure 7 and Figure I.1. Four `uds_services` elements
  carried `:origin: application-layer-standard` citing Figure 5 or Figure 6 while the working
  copy contained neither, and nothing in the set said so. They have since been read and all
  four are confirmed; the point is that nobody could have known that.
- **Table footnotes vanish.** Table 23's footnotes `a`–`e` are referenced from the cells and
  present in neither `clean.md` nor `raw.md`. They are what separates the rows the standard
  fixes from the rows it leaves to the manufacturer — i.e. the whole decision.
- **Figure keys get spliced.** Figure 7's Key note 4 is interleaved across two bullets and
  reads as nonsense.

`pdftotext -layout <spec>.pdf` recovered every one of them, and the figures themselves are
readable as images directly. Three habits follow:

1. Use `pdftotext -layout` as the source of record for any table, figure key or normative
   annex. Use the markdown for prose.
1. **Read the figure.** If an element cites one, open the PNG. `uds_services` had a state
   chart wrong in a way that changed behaviour, in the one element whose warning said the
   figure had not been read — the warning was right.
1. **Say when you have not.** An element resting on an unread source should carry a warning
   the way `UDSSVC_ARCH_0037` did. It is the only reason that defect was findable.

A related trap for `validate_needs.py`: it checks that `origin` and `source` are present, not
that what they name exists. `UDSSVC_ARCH_0038` cited "10.2 Figure 6" through several drafts,
and clause 10.2 has no Figure 6.

**Two questions to ask of your own crate**, both of which found something here:

1. Does any crate's documentation delegate something *to* you? Search the others for
   "higher layer", "belongs to", "the caller must". A delegation in writing with no layer
   between is a responsibility you own whether it is in your scope statement or not.
1. Is your crate symmetric in a way your architecture is not? Client and server, encode
   and decode, send and receive. If the standard defines both and you have modelled one,
   say why in an element rather than leaving the asymmetry to be discovered.

## 8. Order

1. Allocate the three remaining ID prefixes (§2) before anyone authors an ID.
1. `uds_session` takes §2 and §3 — it has the apparatus already, and it is the crate the
   others copy from.
1. `uds_on_ip` and `simple_doip` take the whole of §1, then move and convert their
   `ARCHITECTURE.md` per §5.
1. `uds_protocol` last; it has the least to add and nothing blocking.
1. Settle §6's cross-repository questions with all four sets rendered side by side. That
   is what this is for.
