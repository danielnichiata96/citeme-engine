//! The date normalizer against every production style.
//!
//! `normalize_csl_xml` rewrites a `<date>`'s `delimiter` into per-part
//! prefixes to work around a hayagriva bug. That is only equivalent while
//! the first date-part is always present: on day- or month-first dates the
//! prefix of the first *rendered* part survived, and six production styles
//! printed "( March 2018)", "[cited  2018]" or "consulté le  mai 2024".
//! Rendering every corpus style both ways pins the contract: normalizing may
//! only move whitespace, and must never add an artifact.

use citeme_engine_core::normalize::normalize_csl_xml;
use hayagriva::citationberg::{Locale, LocaleFile, Style};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest,
};
use serde_json::json;

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// The first bibliography entry for `item`, straight through hayagriva —
/// `CitationEngine` always normalizes, so it can't render the raw style.
fn bibliography_entry(style_xml: &str, locales: &[Locale], item: &serde_json::Value) -> String {
    let Style::Independent(style) = Style::from_xml(style_xml).unwrap() else {
        panic!("corpus styles are independent");
    };
    let item: hayagriva::citationberg::json::Item = serde_json::from_value(item.clone()).unwrap();
    let mut driver = BibliographyDriver::new();
    driver.citation(CitationRequest::new(
        vec![CitationItem::with_entry(&item)],
        &style,
        None,
        locales,
        None,
    ));
    let rendered = driver.finish(BibliographyRequest {
        style: &style,
        locale: None,
        locale_files: locales,
    });
    let mut entry = String::new();
    if let Some(bibliography) = rendered.bibliography {
        if let Some(first) = bibliography.items.first() {
            first
                .content
                .write_buf(&mut entry, BufWriteFormat::Plain)
                .unwrap();
        }
    }
    entry
}

/// Spacing no style asks for: doubled spaces, or a space just inside a
/// bracket or before a comma or semicolon.
fn spacing_artifact(text: &str) -> bool {
    ["  ", "( ", "[ ", " )", " ]", " ,", " ;"]
        .iter()
        .any(|artifact| text.contains(artifact))
}

fn without_whitespace(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn normalizing_a_corpus_style_only_ever_removes_spacing_artifacts() {
    let locales: Vec<Locale> = std::fs::read_dir(fixtures().join("locales"))
        .unwrap()
        .map(|entry| {
            let xml = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            LocaleFile::from_xml(&xml).unwrap().into()
        })
        .collect();

    let mut styles: Vec<_> = std::fs::read_dir(fixtures().join("styles/corpus"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "csl"))
        .collect();
    styles.sort();

    let mut failures = Vec::new();
    for path in styles {
        let raw_xml = std::fs::read_to_string(&path).unwrap();
        let normalized_xml = normalize_csl_xml(&raw_xml);
        if normalized_xml == raw_xml {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        for date in [json!([[2018]]), json!([[2018, 3]]), json!([[2018, 3, 15]])] {
            for kind in [
                "article-journal",
                "webpage",
                "book",
                "article-newspaper",
                "report",
            ] {
                let item = json!({
                    "id": "x", "type": kind, "title": "Title",
                    "author": [{"family": "Smith", "given": "John"}],
                    "container-title": "Journal", "URL": "https://x.org", "publisher": "Pub",
                    "issued": {"date-parts": date}, "accessed": {"date-parts": date}
                });
                let raw = bibliography_entry(&raw_xml, &locales, &item);
                let normalized = bibliography_entry(&normalized_xml, &locales, &item);
                let only_whitespace_moved =
                    without_whitespace(&raw) == without_whitespace(&normalized);
                let added_artifact = spacing_artifact(&normalized) && !spacing_artifact(&raw);
                if !only_whitespace_moved || added_artifact {
                    failures.push(format!(
                        "{name} / {kind} / {date}:\n    raw:        {raw:?}\n    normalized: {normalized:?}"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} normalized render(s) differ from the raw style by more than removed spacing:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
