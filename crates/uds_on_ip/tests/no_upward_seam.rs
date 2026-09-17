//! This crate declares no trait that a layer above it implements.
//!
//! The boundary brief §2 deletes `SessionLayer` and `RequestHandler` rather
//! than relocating them: with `uds_services` driving, there is no
//! binding-side driver to call anything upward, and no seam for a
//! response-pending to cross.
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
            "SessionLayer is deleted, not relocated — see the boundary brief §2"
        );
        assert!(
            !code.contains("RequestHandler"),
            "RequestHandler is deleted, not relocated — see the boundary brief §2"
        );
    }
}

/// The boundary brief §4: the "this crate appears twice / wraps the session
/// layer" sandwich no longer describes the design. The driver is above the
/// session layer and this crate is wholly below it.
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
