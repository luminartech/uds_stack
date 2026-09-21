//! This crate declares no trait that a layer above it implements.
//!
//! `SessionLayer` and `RequestHandler` were deleted rather than relocated:
//! with `uds_services` driving, there is no binding-side driver to call
//! anything upward, and no seam for a response-pending to cross. The reasoning
//! is `ARCHITECTURE.md` §4.2's; the names must not come back either way.
//!
//! The scan below is scoped to declarations, not prose: documentation may
//! still name what was deleted (and Task 7 rewrites this crate's prose in
//! full), but neither name may reappear as a live declaration.

#[test]
fn the_crate_declares_no_upward_trait() {
    for line in include_str!("../src/lib.rs").lines() {
        let code = line.trim_start();
        if code.starts_with("//") {
            // Documentation may name what was deleted, and Task 7 rewrites
            // this crate's prose in full. Only declarations are the subject.
            continue;
        }
        assert!(
            !code.contains("SessionLayer"),
            "SessionLayer is deleted, not relocated: nothing above this crate is called from it"
        );
        assert!(
            !code.contains("RequestHandler"),
            "RequestHandler is deleted, not relocated: nothing hands this crate a request"
        );
    }
}

/// The "this crate appears twice / wraps the session layer" sandwich no longer
/// describes the design: the driver is above the session layer and this crate
/// is wholly below it (`ARCHITECTURE.md` §4.2).
#[test]
fn the_crate_docs_do_not_describe_the_reversed_design() {
    let source = include_str!("../src/lib.rs");
    assert!(
        !source.contains("appears twice"),
        "this crate is wholly below the session layer now"
    );
    assert!(
        !source.contains("wraps the session layer"),
        "the driver is above the session layer; this crate is below it"
    );
}

/// The crate is wholly below the session layer; only the *driver* sits above
/// it. Keying on that invariant, rather than on any particular phrasing,
/// is deliberate: the previous guard matched one exact substring and would
/// have passed "this half of the crate sits above the session layer" — the
/// very wording it was written to catch.
#[test]
fn only_the_driver_is_claimed_to_sit_above_the_session_layer() {
    let source = include_str!("../src/profile.rs");
    for (i, _) in source.match_indices("above the session layer") {
        let context = &source[i.saturating_sub(40)..i];
        assert!(
            context.contains("driver"),
            "a claim of sitting above the session layer must be about the \
             driver, not this crate — the crate is wholly below it"
        );
    }
}
