//! What this crate may and may not depend on.
//!
//! Naming a runtime would compromise the `no_std` build, and a bare-metal
//! AURIX `TC4x` is a qualification target. The codec vocabulary arrives
//! through the layer below rather than around it.

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
                    section.contains("dependencies")
                        && section.ends_with(&format!(".{name}"))
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
fn the_codec_vocabulary_comes_through_the_layer_below() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !declares_dependency(manifest, "embedded-io"),
        "embedded-io left this crate's dependency list and has not been readmitted"
    );
    assert!(
        !declares_dependency(manifest, "automotive-wire-codec"),
        "Encode and Decode are re-exported by simple_doip::messages, and \
         Encode::encode_to_slice needs no sink named here — so a direct awc \
         dependency means something new is wanted from it. Say what, in the \
         manifest comment, and change this assertion deliberately."
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
    assert!(!declares_dependency(
        "# tokio is deliberately absent",
        "tokio"
    ));
    assert!(!declares_dependency("[dependencies]", "tokio"));
}
