//! Every `ARCHITECTURE.md` section this crate cites exists.
//!
//! `ARCHITECTURE.md` ships with the package, so a reader can follow these —
//! which is why they survived when references to documents outside the package
//! were removed. Section numbers move when a document is edited, and nothing
//! checked them; this is the cheapest thing that notices.
//!
//! # What this does not catch, and the case that proves it
//!
//! It checks that a cited section *exists*, not that it says what the citing
//! sentence claims. `profile::service_ids` cited "§11" for the invariant that
//! `uds_on_ip` never learns what a service is. §11 exists — it is "`no_std`
//! scope" — and the invariant is §13, so the citation was wrong and this test
//! passes on it. Verified by reintroducing it.
//!
//! That limit is worth stating rather than papering over: a wrong-but-existing
//! citation is the more likely defect of the two, because section numbers shift
//! under edits while the prose around them stays. Only a reader catches it, and
//! a neighbouring crate caught this one. What the test buys is the other half —
//! a number that has gone off the end of the document — which is cheap and
//! which no reader reliably notices.

use std::collections::BTreeSet;

/// Section numbers cited as `` `ARCHITECTURE.md` §N `` anywhere in the crate.
fn cited_sections(source: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (index, _) in source.match_indices("ARCHITECTURE.md` §") {
        let tail = &source[index + "ARCHITECTURE.md` §".len()..];
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
    // Guarding against the reintroduction of a stale number is this file's
    // whole purpose, so the extractor must see a citation and the heading
    // reader must not invent one.
    assert!(cited_sections("see `ARCHITECTURE.md` §13, invariant 5").contains("13"));
    assert!(cited_sections("see `ARCHITECTURE.md` §3.1.").contains("3.1"));
    assert!(cited_sections("no citation here").is_empty());
    assert!(declared_sections("## 9. Gap analysis").contains("9"));
    assert!(declared_sections("### 3.1 `simple_doip`").contains("3.1"));
    assert!(declared_sections("# Architecture").is_empty());
}
