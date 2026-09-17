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

/// `lib.rs` is not the only file that can claim the reversed design:
/// `profile.rs`'s module doc made exactly this claim on its own, undetected
/// by the scan above because that scan only ever reads `lib.rs`.
///
/// The banned phrase is scoped to "the crate that sits", not the bare phrase
/// "above the session layer": `profile::service_ids`'s doc correctly says
/// "the driver sits above the session layer" when explaining *why* this
/// crate reads a `T_PDU` rather than an `A_PDU`, and that sentence is true —
/// the driver (`uds_services`) really is above the session layer. Only a
/// claim about *this crate's own* position is the defect.
#[test]
fn profile_docs_do_not_describe_the_reversed_design() {
    let source = include_str!("../src/profile.rs");
    assert!(
        !source.contains("the crate that sits"),
        "the crate is wholly below the session layer; profile.rs is no exception"
    );
}
