//! What this crate may and may not depend on.
//!
//! Naming a runtime would compromise the `no_std` build, and a bare-metal
//! AURIX `TC4x` is a qualification target. The I/O vocabulary is
//! `automotive-wire-codec`'s for the whole stack.

/// True if `name` is declared as a dependency, in either TOML form: an inline
/// or dotted key (`tokio = { … }`, `tokio.version = "1"`) or a section header
/// (`[dependencies.tokio]`, `[target.'cfg(…)'.dependencies.tokio]`). Checking
/// for the bare substring instead would match this crate's own comments, which
/// legitimately name both crates in explaining why they are absent.
fn declares_dependency(manifest: &str, name: &str) -> bool {
    manifest.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with(name)
            || line
                .strip_prefix('[')
                .and_then(|section| section.strip_suffix(']'))
                .is_some_and(|section| {
                    section.contains("dependencies") && section.ends_with(&format!(".{name}"))
                })
    })
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

#[test]
fn a_section_header_dependency_is_detected() {
    // The form the plain prefix check missed. Guarding against reintroduction
    // is this file's whole purpose, so the check must see every way a
    // dependency can be spelled.
    assert!(declares_dependency("[dependencies.tokio]", "tokio"));
    assert!(declares_dependency(
        "[target.'cfg(unix)'.dependencies.tokio]",
        "tokio"
    ));
    assert!(!declares_dependency("# tokio is deliberately absent", "tokio"));
    assert!(!declares_dependency("[dependencies]", "tokio"));
}
