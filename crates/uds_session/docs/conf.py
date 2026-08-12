"""Sphinx configuration for the uds_session requirement set.

The requirement schema lives here for now. It is a contract shared with the private
qualification repository, which links to these requirement IDs, so it will move to the
shared process repository once that exists and be consumed from there. Until then,
changes here are changes to a cross-repository interface.

`tools/validate_needs.py` enforces the parts of the schema that sphinx-needs has no
concept of. Keep the two in step: the permitted values below and the constants in that
script describe the same policy.
"""

project = "uds_session"
author = "MicroVision"
copyright = "2026, MicroVision"  # noqa: A001 - Sphinx requires this name
release = "0.1.0"
# sphinx-needs keys needs.json by `version`, and the qualification repository pins to that
# key through needs_external_needs. Leaving it unset produces an empty key, which makes a
# committed snapshot impossible to pin to. Keep this in step with the crate version.
version = "0.1.0"

extensions = [
    "sphinx_needs",
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

html_theme = "alabaster"

# --- sphinx-needs schema ------------------------------------------------------------

needs_types = [
    {
        "directive": "llr",
        "title": "Low-Level Requirement",
        "prefix": "LLR_",
        "color": "#BFD8D2",
        "style": "node",
    },
    {
        "directive": "aou",
        "title": "Assumption of Use",
        "prefix": "AOU_",
        "color": "#FEDCD2",
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
# moment the qualification repository first links to one, so auto-generation is refused:
# a generated ID changes when its surrounding content does.
needs_id_required = True
needs_id_regex = r"^UDSS_(LLR|AOU)_\d{4}$|^UDSS_(IMPL|TEST)_[A-Z0-9_]+$"

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

# Emit needs.json so the qualification repository can consume this set through
# needs_external_needs against a committed snapshot.
needs_build_json = True
