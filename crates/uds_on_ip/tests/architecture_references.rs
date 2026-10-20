//! Every `ARCHITECTURE.md` section this crate cites exists.
//!
//! `ARCHITECTURE.md` ships with the package, so a reader can follow these citations.
//! Section numbers move when a document is edited; this notices a number that has gone
//! off the end of the document. It checks that a cited section *exists*, not that it
//! says what the citing sentence claims: a wrong-but-existing citation passes, and only
//! a reader catches it.

use std::collections::BTreeSet;

/// Section numbers cited as `` `ARCHITECTURE.md` §N `` anywhere in the crate.
fn cited_sections(source: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    // `split` rather than `match_indices` + slicing: each piece after the first
    // already starts where the number does, so there is no offset to compute.
    for tail in source.split("ARCHITECTURE.md` §").skip(1) {
        let number: String = tail
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if !number.is_empty() {
            found.insert(number.trim_end_matches('.').to_owned());
        }
    }
    found
}

/// Section numbers `ARCHITECTURE.md` actually declares, from its own headings.
fn declared_sections(architecture: &str) -> BTreeSet<String> {
    architecture
        .lines()
        .filter_map(|line| line.strip_prefix('#'))
        .map(|rest| rest.trim_start_matches('#').trim_start())
        .filter_map(|heading| heading.split_whitespace().next())
        .filter(|first| first.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|first| first.trim_end_matches('.').to_owned())
        .collect()
}

#[test]
fn every_cited_architecture_section_exists() {
    let architecture = include_str!("../ARCHITECTURE.md");
    let declared = declared_sections(architecture);

    let sources = [
        include_str!("../src/client.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/mapping.rs"),
        include_str!("../src/profile.rs"),
        include_str!("../src/transport.rs"),
        include_str!("no_upward_seam.rs"),
    ];

    for source in sources {
        for cited in cited_sections(source) {
            assert!(
                declared.contains(&cited),
                "ARCHITECTURE.md has no section {cited}; declared sections are {declared:?}"
            );
        }
    }
}

#[test]
fn the_check_can_fail() {
    // The extractor must see a citation and the heading reader must not invent one,
    // or the check above passes vacuously.
    assert!(cited_sections("see `ARCHITECTURE.md` §10, invariant 1").contains("10"));
    assert!(cited_sections("see `ARCHITECTURE.md` §2.1.").contains("2.1"));
    assert!(cited_sections("no citation here").is_empty());
    assert!(declared_sections("## 6. Gap analysis").contains("6"));
    assert!(declared_sections("### 2.1 Transport seam").contains("2.1"));
    assert!(declared_sections("# Architecture").is_empty());
}
