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

These came out of authoring `uds_services`' architecture against ISO 14229-1 clause 8.7.
They cannot be settled in one repository alone, and each blocks something.

### `uds_on_ip`

**Its client is byte-level, and that is the seam a typed client sits on.** `Client::send`
takes `&[u8]` and returns a `Completion`; `send_functional` returns a lending sequence.
`uds_services` now owns the typed client surface above it, so two things need confirming
on that side: that the byte signatures are the intended long-term seam (they mirror the
handler seam, so probably yes), and that `Indication.ai` will keep carrying each
responder's source address — the typed client depends on it to attribute functional
responses, and it is the only way to tell them apart.

Note also that every method on that client is currently `todo!()`. The typed surface can
be designed against its shape but cannot be exercised until it is real.

`uds_services` reaches it through a transport trait *it* declares, rather than binding to
`uds_on_ip::Client` directly — the mirror of the handler seam, which that crate declares.
The rule behind both: **whoever is called declares the interface.** A binding calls into a
server, so the binding declares the byte seam; this crate calls out to a transport, so this
crate declares the transport seam. Nothing is needed from `uds_on_ip` for that beyond the
signatures it already has.

**Its documentation already delegates upward, in two places.** `Client::send` says a
negative response "is a response, and interpreting it belongs to a higher layer", and the
handler seam says it does not know what a service or a negative response code is. Both are
correct and both are now discharged by `uds_services`. Worth a cross-reference in that
crate's architecture so the delegation is visible from the declining side too.

**Its handler seam must become asynchronous, and now is when that is cheap.**
`RequestHandler::handle` is synchronous today. `UDSSVC_ARCH_0016` requires an asynchronous
one, and the gain is concrete: a handler that outruns `tP2_Server` yields at its await
points, so the driver keeps draining session actions and transmits the `0x78` that
`uds_session` decided to send. Without it, that crate's own architecture is left carrying
the constraint that "a slow handler has to run somewhere the driver can continue past" —
machinery every integrator has to build. The trait is declared and has no implementations,
so this costs a signature now and a migration once servers exist.

The premise is `UDSSVC_ARCH_0030`: an async executor is assumed everywhere, `embassy`
included, and no crate here depends on one. Note that `async` implies neither a runtime
nor `std` — the executor is the caller's — so this does not compromise a `no_std` build.
A tokio *dependency* would; an `async fn` does not.

**Its request context is missing two fields.** `Ctx` today carries an addressing triple,
the active session and the security level. Clause 8.7's mandatory validation sequence also
reads:

- **whether authentication has succeeded** — Figures 5 and 6 both place an authentication
  check on the *mandatory* path, producing 0x34, ahead of the session check; and
- **whether a response-pending (0x78) has already been sent** — clause 8.7.5 guards both
  suppression rules on this, and states that once one has gone out the final response
  shall be sent regardless.

Without the second, a functionally addressed request that took long enough to earn a 0x78
is answered with silence where the standard requires a final negative response. The
failure needs both a slow handler and functional addressing to appear, so it will not show
up in ordinary testing.

**Its `Ai` is DoIP-shaped, and cannot be the typed layer's addressing type.** It embeds
`simple_doip::LogicalAddress`. `uds_services` therefore defines its own two-variant
physical/functional distinction and converts in the `doip` adapter, because no clause 8.7
decision reads a source or target address. Worth asking once whether the ISO 14229-2
addressing triple belongs in a crate both can depend on.

### `uds_session`

**It must expose that a 0x78 has been sent.** It owns the `tP2_Server` timer and makes the
decision, so it is the only crate that knows; today that state is internal. See above for
what depends on it.

Also confirm the ownership table in `uds_services/docs/architecture/not-owned.rst` matches
what `uds_session` believes it owns — particularly that `tP4_Server` is a performance
requirement on the application rather than a timer to run.

### `simple_doip`

**Two server seams exist, and it is not settled which is canonical.** The bare-metal entity
exports a `fn(&[u8], &mut [u8]) -> i32` request callback in production code — the same byte
seam `uds_on_ip` declares, one layer further down. Either that callback is the canonical
seam for `no_std` targets and `uds_services` should target it too, or it is a bare-metal
convenience that does not compose with the typed path. This blocks the server story rather
than decorating it.

### `uds_protocol`

This crate turns out to be the most load-bearing of the four, because a single set of
message definitions has to serve both roles in both build environments.

**All four paths through a message definition are now in use.** A client constructs and
encodes requests and decodes responses; a server decodes requests and constructs and
encodes responses. Before, only the server's two were exercised. Confirm every modelled
service is constructible from native values and not only decodable from the wire — for the
four services of the first pass it is, each having a `new` alongside `Encode` and `Decode`,
and `ReadDataByIdentifierRequest` carries native and wire-backed variants precisely so a
caller can build one either way. Check this property first when adding a service.

**Both build environments must be covered.** The server compiles into embedded firmware
without `std` or `alloc`; the client compiles into host tooling with both. The existing
feature division is exactly right and is worth stating as an invariant rather than leaving
as a habit: `clap` and `utoipa` imply `std` and exist for host tooling, `serde` is wired
core-only as the one integration usable on a bare-metal target. The rest of the stack —
and the application's own identifier vocabulary — should follow that pattern.

**One clause that is easy to violate by being helpful.** ISO 14229-1:2020 11.2.1: a
request may contain the same data identifier more than once, and the server shall treat
each as a separate parameter and respond for each as often as requested. Anything that
collects requested identifiers into a set is non-conformant. Worth checking that
`ReadDataByIdentifierRequest::dids()` preserves duplicates and order, and worth a test
that says so.

**`Decode`'s front-split contract is now load-bearing above you.** Clause 11.2.1 makes the
dataRecord format vehicle-manufacturer specific and 11.2.3.1 shows no length field between
records, so a `ReadDataByIdentifier` response is splittable only by someone who knows each
record's extent. `uds_protocol` correctly leaves it opaque, and `uds_services` gets the
extent from the record type's own `Decode` impl — "decode from the FRONT of `buf`; return
`(value, unconsumed_remainder)`" — rather than from a hand-written length
(`UDSSVC_ARCH_0026`). Two consequences: keep re-exporting `Encode` and `Decode` (the
identifier traits reach them through you, so no new dependency on
`automotive-wire-codec` is needed), and treat that front-split-and-return-the-remainder
wording as the contract it now is.

That decision came from `automotive_wire_format`'s own reasoning, and it generalises:
`Encode::encoded_size` defaults to running `encode` against a counting sink because
"hand-maintained sizes cannot drift from `encode` — the bug class every migrated consumer
had". Any crate here tempted to add a length constant should read that note first. Do not
add a length inference either. Two things to confirm, both of which `uds_services` now depends on:

- **Its decode error taxonomy is the source of 0x13.** `uds_services` maps decode failures
  to `incorrectMessageLengthOrInvalidFormat` and needs the mapping to be total.
- **The unmodelled-service variant means 0x11, not 0x13.** A request naming a service
  `uds_protocol` does not model may be perfectly well formed; reaching that variant means
  the server has no implementation, which is `serviceNotSupported`. Confirm nothing in the
  decode path conflates the two.

Also worth a note in that crate: `responseTooLong` (0x14) appears in its permitted codes
for `ReadDataByIdentifier` but in neither Figure 5 nor Figure 6, and who produces it is
open — the maximum response length is a transport property.

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
