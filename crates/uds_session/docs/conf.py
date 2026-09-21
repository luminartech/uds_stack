"""Sphinx configuration for the uds_session requirement set.

The requirement schema lives here. It is a contract with anything that consumes this set
through needs.json and links to these requirement IDs, so a change here is a change to a
published interface rather than to local configuration.

`tools/validate_needs.py` enforces the parts of the schema that sphinx-needs has no
concept of. Keep the two in step: the permitted values below and the constants in that
script describe the same policy.
"""

project = "uds_session"
author = "MicroVision"
copyright = "2026, MicroVision"  # noqa: A001 - Sphinx requires this name
release = "0.1.0"
# sphinx-needs keys needs.json by `version`, and a consumer pins to that key through
# needs_external_needs. Leaving it unset produces an empty key, which makes a committed
# snapshot impossible to pin to. Keep this in step with the crate version.
version = "0.1.0"

extensions = [
    "sphinx_needs",
    "sphinx_hextra",
]

source_suffix = {
    ".rst": "restructuredtext",
}

exclude_patterns = [
    "_build",
    # Working design documents, not part of the published requirement set. They are not
    # version controlled, they go stale, and rules written for a single work item leak
    # into later ones if they are read as current. Never include them in published output.
    "superpowers",
    "sphinx-requirements.txt",
]

html_theme = "sphinx_hextra"

# The published site is HTML only, for the same reason the workflow prunes needs.json: that
# export is consumed as evidence against a committed snapshot, so no machine-consumable form
# of the requirements may sit at a stable URL rebuilt on every push, where it could be
# pointed at instead. Sphinx otherwise copies every source document to
# `_sources/*.rst.txt` and links it from each page — the full requirement text, the same
# content as needs.json in another format. Pruning needs.json alone does not close that.
html_copy_source = False
html_show_sourcelink = False

# --- sphinx-needs schema ------------------------------------------------------------

needs_types = [
    {
        "directive": "llr",
        "title": "Low-Level Requirement",
        "prefix": "LLR_",
        "color": "#BFD8D2",
        "style": "node",
    },
    # `impl` and `test` needs are generated from source annotations by the extractor and
    # imported, never hand-authored. They exist as types so the generated links resolve
    # and so traceability tables can be rendered against them.
    {
        "directive": "impl",
        "title": "Implementation",
        "prefix": "IMPL_",
        "color": "#DF744A",
        "style": "node",
    },
    {
        "directive": "test",
        "title": "Test Case",
        "prefix": "TEST_",
        "color": "#DCB239",
        "style": "node",
    },
]

# IDs are allocated, never renumbered, and never reused. They are permanent from the
# moment anything outside this repository first links to one, so auto-generation is
# refused: a generated ID changes when its surrounding content does.
needs_id_required = True
needs_id_regex = r"^UDSS_LLR_\d{4}$|^UDSS_(IMPL|TEST)_[A-Z0-9_]+$"

needs_extra_options = [
    # What is currently substantiated by evidence.
    "integrity_level",
    # What this requirement is expected to reach. The gap between the two is the
    # outstanding work, and is reportable rather than tacit.
    "target_level",
    # Whether this requirement was transcribed from a standard or derived by us.
    "origin",
    # Where in that standard it came from: clause, table or figure. Required when
    # `origin` names a standard, and absent when it is `derived`, which has no clause to
    # cite. Enforced by tools/validate_needs.py.
    "source",
]

needs_extra_links = [
    {
        "option": "implements",
        "incoming": "implemented by",
        "outgoing": "implements",
        "style": "#777",
    },
    {
        "option": "verifies",
        "incoming": "verified by",
        "outgoing": "verifies",
        "style": "#0b0",
    },
]

needs_statuses = [
    {"name": "draft", "description": "Authored, not yet reviewed. IDs may still move."},
    {"name": "review", "description": "Under review."},
    {"name": "approved", "description": "Reviewed and accepted. The ID is now permanent."},
    {"name": "obsolete", "description": "Withdrawn. The ID is retained and never reused."},
]

# Emit needs.json so this set can be consumed through needs_external_needs against a
# committed snapshot.
needs_build_json = True
