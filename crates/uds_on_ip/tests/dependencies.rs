//! What this crate may and may not depend on.
//!
//! Naming a runtime would compromise the `no_std` build, and a bare-metal
//! AURIX `TC4x` is a qualification target. The codec vocabulary arrives
//! through the layer below rather than around it.

/// True if `name` is declared as a dependency of a build, in either TOML form: an
/// inline or dotted key (`tokio = { … }`, `tokio.version = "1"`) in a dependency table,
/// or a section header (`[dependencies.tokio]`, `[target.'cfg(…)'.dependencies.tokio]`).
/// Checking for the bare substring instead would match this crate's own comments, which
/// legitimately name both crates in explaining why they are absent. A
/// `[dev-dependencies]` table is not a build's, so it is not read.
fn declares_dependency(manifest: &str, name: &str) -> bool {
    let builds = |section: &str| {
        !section.contains("dev-dependencies") && section.contains("dependencies")
    };
    let mut section = "";
    manifest.lines().any(|line| {
        let line = line.trim_start();
        if let Some(header) = line
            .strip_prefix('[')
            .and_then(|header| header.strip_suffix(']'))
        {
            section = header;
            return builds(header) && header.ends_with(&format!(".{name}"));
        }
        builds(section) && line.starts_with(name)
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

/// A test drives its futures on a runtime, and a dev-dependency reaches no build of
/// the crate, so only the tables a build reads are guarded.
#[test]
fn a_dev_dependency_is_not_a_build_dependency() {
    let manifest =
        "[dependencies]\nthiserror = \"2\"\n\n[dev-dependencies]\ntokio = \"1\"\n";
    assert!(!declares_dependency(manifest, "tokio"));
    assert!(declares_dependency(
        "[dev-dependencies]\nanyhow = \"1\"\n[dependencies]\ntokio = \"1\"\n",
        "tokio"
    ));
    assert!(!declares_dependency("[dev-dependencies.tokio]", "tokio"));
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
