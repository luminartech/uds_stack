//! What this crate may and may not depend on.
//!
//! Naming a runtime would compromise the `no_std` build, and a bare-metal
//! AURIX `TC4x` is a qualification target. The I/O vocabulary is
//! `automotive-wire-codec`'s for the whole stack.

/// A dependency is a manifest line whose key is the crate name. Checking for
/// the bare substring instead would match this crate's own comments, which
/// legitimately name both crates in explaining why they are absent.
fn declares_dependency(manifest: &str, name: &str) -> bool {
    manifest
        .lines()
        .any(|line| line.trim_start().starts_with(name))
}

#[test]
fn no_runtime_is_named() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !declares_dependency(manifest, "tokio"),
        "a runtime dependency compromises the no_std build; an async fn does not"
    );
}

#[test]
fn the_io_vocabulary_is_the_codecs() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !declares_dependency(manifest, "embedded-io"),
        "awc 0.4 owns the sink trait; embedded-io leaves the dependency list"
    );
    assert!(
        declares_dependency(manifest, "automotive-wire-codec"),
        "the stack's I/O vocabulary is a mandatory dependency"
    );
}
