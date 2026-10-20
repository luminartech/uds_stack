//! Every trait at this crate's seams is a neighbor's (`ARCHITECTURE.md` §2.3).
//!
//! The crate implements `uds_services`' `UdsTransport` and `ClientTransport`, and is
//! generic over `simple_doip`'s `DiagnosticEntity` and `TesterConnection`. Every trait
//! it names is declared by one of those two crates, so a layer above has nothing of
//! this crate's to implement.

/// True if `code`, a line with its indentation removed, declares a trait.
fn declares_trait(code: &str) -> bool {
    let mut rest = code;
    if let Some(after_pub) = rest.strip_prefix("pub") {
        rest = after_pub
            .strip_prefix('(')
            .and_then(|scoped| scoped.split_once(')'))
            .map_or(after_pub, |(_, after_scope)| after_scope)
            .trim_start();
    }
    let rest = rest.strip_prefix("unsafe ").unwrap_or(rest);
    rest.starts_with("trait ")
}

#[test]
fn every_trait_is_a_neighbors() {
    let sources = [
        ("client.rs", include_str!("../src/client.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
        ("error.rs", include_str!("../src/error.rs")),
        ("mapping.rs", include_str!("../src/mapping.rs")),
        ("profile.rs", include_str!("../src/profile.rs")),
        ("transport.rs", include_str!("../src/transport.rs")),
    ];
    for (file, source) in sources {
        for (index, line) in source.lines().enumerate() {
            assert!(
                !declares_trait(line.trim_start()),
                "src/{file}:{}: the traits at this crate's seams are its neighbors'",
                index + 1
            );
        }
    }
}

#[test]
fn a_trait_declaration_is_recognized_at_any_visibility() {
    assert!(declares_trait("trait Seam {"));
    assert!(declares_trait("pub trait Seam {"));
    assert!(declares_trait("pub(crate) trait Seam {"));
    assert!(declares_trait("pub unsafe trait Seam {"));
    assert!(!declares_trait("/// A trait the layer above implements."));
    assert!(!declares_trait("pub fn traits() {}"));
}
