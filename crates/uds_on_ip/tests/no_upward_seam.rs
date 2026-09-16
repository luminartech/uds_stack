//! This crate declares no trait that a layer above it implements.
//!
//! The boundary brief §2 deletes `SessionLayer` and `RequestHandler` rather
//! than relocating them: with `uds_services` driving, there is no
//! binding-side driver to call anything upward, and no seam for a
//! response-pending to cross.

#[test]
fn the_crate_declares_no_upward_trait() {
    let source = include_str!("../src/lib.rs");
    assert!(
        !source.contains("SessionLayer"),
        "SessionLayer is deleted, not relocated — see the boundary brief §2"
    );
    assert!(
        !source.contains("RequestHandler"),
        "RequestHandler is deleted, not relocated — see the boundary brief §2"
    );
}
