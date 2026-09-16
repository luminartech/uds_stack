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
