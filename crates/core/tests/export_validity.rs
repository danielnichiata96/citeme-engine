//! Exporter validity: an exported file must be accepted by its own consumer.
//!
//! BibTeX/BibLaTeX output must re-import through the engine's own parser;
//! Hayagriva output must load through hayagriva's YAML reader. This matters
//! because the CiteMe adapter's JS fallback only fires when the Wasm call
//! throws — silently corrupt output sails straight to the user's file, so
//! these tests are the release gate for the export surface.

use citeme_engine_core::export::biblatex::csl_json_array_to_biblatex;
use citeme_engine_core::export::bibtex::{csl_json_array_to_bibtex, csl_json_to_bibtex};
use citeme_engine_core::export::hayagriva::csl_json_array_to_hayagriva;
use citeme_engine_core::parsers::bibtex::parse_bibtex;
use citeme_engine_core::parsers::detect::{detect_format, InputFormat};
use citeme_engine_core::parsers::ParseOptions;
use serde_json::{json, Value};

fn reimport(bib: &str) -> (usize, usize) {
    let res = parse_bibtex(bib, &ParseOptions::default());
    (res.entries.len(), res.errors.len())
}

fn hostile_items() -> Vec<Value> {
    vec![
        json!({
            "type": "article-journal",
            "id": "bad,key",
            "title": "Deep {learning for {unbalanced imports",
            "author": [{"family": "Silva", "given": "Ana"}],
            "issued": {"date-parts": [[2024]]}
        }),
        json!({
            "type": "article-journal",
            "id": "ok2024",
            "title": r"The \alpha } factor & a 100% $gain",
            "author": [{"family": "Souza", "given": "Bruno"}],
            "issued": {"date-parts": [[2023]]}
        }),
        json!({
            "type": "book",
            "id": "id:10.1000/x{y},z",
            "title": "Plain title",
            "publisher": "Norm & Co. {Press}",
            "issued": {"date-parts": [[2020]]}
        }),
    ]
}

// ── BibTeX ───────────────────────────────────────────────────────────

#[test]
fn bibtex_export_reimports_comma_id_and_unbalanced_braces() {
    for item in hostile_items() {
        let bib = csl_json_to_bibtex(&item);
        let (entries, errors) = reimport(&bib);
        assert_eq!(entries, 1, "must re-import as exactly one entry:\n{bib}");
        assert_eq!(errors, 0, "must re-import without errors:\n{bib}");
    }
}

#[test]
fn bibtex_array_export_reimports_full_batch() {
    let items = hostile_items();
    let bib = csl_json_array_to_bibtex(&items);
    let (entries, errors) = reimport(&bib);
    assert_eq!(
        entries,
        items.len(),
        "batch must survive round-trip:\n{bib}"
    );
    assert_eq!(errors, 0, "batch must re-import cleanly:\n{bib}");
}

#[test]
fn bibtex_cite_key_is_sanitized() {
    let item = json!({
        "type": "book",
        "id": "id:10.1000/a,b {c}(d)=e\\f#g%h\"i~j k",
        "title": "T"
    });
    let bib = csl_json_to_bibtex(&item);
    let first = bib.lines().next().expect("non-empty output");
    let key = first
        .trim_start_matches(|c| c != '{')
        .trim_start_matches('{')
        .trim_end_matches(',');
    assert!(!key.is_empty(), "sanitized key must not be empty: {first}");
    for bad in [',', '{', '}', '(', ')', '=', '\\', '#', '%', '"', '~', ' '] {
        assert!(!key.contains(bad), "key {key:?} contains {bad:?}: {first}");
    }
}

#[test]
fn bibtex_cite_key_falls_back_when_id_fully_invalid() {
    let item = json!({
        "type": "article-journal",
        "id": ",,{}\\ ",
        "title": "T",
        "author": [{"family": "Lima", "given": "Caio"}],
        "issued": {"date-parts": [[2021]]}
    });
    let bib = csl_json_to_bibtex(&item);
    let (entries, errors) = reimport(&bib);
    assert_eq!(
        (entries, errors),
        (1, 0),
        "fallback key must keep entry importable:\n{bib}"
    );
}

// ── BibLaTeX ─────────────────────────────────────────────────────────

#[test]
fn biblatex_export_reimports_hostile_batch() {
    let items = hostile_items();
    let bib = csl_json_array_to_biblatex(&items);
    let (entries, errors) = reimport(&bib);
    assert_eq!(
        entries,
        items.len(),
        "biblatex batch must survive round-trip:\n{bib}"
    );
    assert_eq!(errors, 0, "biblatex batch must re-import cleanly:\n{bib}");
}

// ── Hayagriva ────────────────────────────────────────────────────────

#[test]
fn hayagriva_yaml_survives_scalar_lookalikes() {
    let items = vec![
        json!({"type": "article-journal", "id": "a1", "title": "true"}),
        json!({"type": "article-journal", "id": "a2", "title": "null"}),
        json!({"type": "article-journal", "id": "a3", "title": "T", "abstract": "- leading dash"}),
        json!({"type": "article-journal", "id": "a4", "title": "@misc looks like a tag"}),
        json!({"type": "article-journal", "id": "a5", "title": "*star: &anchor !tag"}),
        json!({"type": "article-journal", "id": "a6", "title": "T", "volume": "3", "issue": "2-3"}),
    ];
    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml);
    assert!(
        lib.is_ok(),
        "exported YAML must load: {:?}\n{yaml}",
        lib.err()
    );
    assert_eq!(
        lib.unwrap().len(),
        items.len(),
        "all entries must survive:\n{yaml}"
    );
}

#[test]
fn hayagriva_invalid_language_is_omitted_not_corrupting() {
    // `language` maps to hayagriva's LanguageIdentifier — junk values must be
    // dropped, never emitted (raw OR quoted junk both fail the YAML load).
    let items = vec![
        json!({"type": "article-journal", "id": "l1", "title": "T", "language": "null"}),
        json!({"type": "article-journal", "id": "l2", "title": "T", "language": "en: US"}),
        json!({"type": "article-journal", "id": "l3", "title": "T", "language": "pt-BR"}),
    ];
    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml);
    assert!(
        lib.is_ok(),
        "junk language must not corrupt YAML: {:?}\n{yaml}",
        lib.err()
    );
    assert!(
        yaml.contains("language: pt-BR"),
        "valid language must be kept:\n{yaml}"
    );
}

// ── Format detection ─────────────────────────────────────────────────

#[test]
fn detect_accepts_any_bibtex_entry_type() {
    assert_eq!(
        detect_format("@dataset{d1, title={T}}"),
        InputFormat::Bibtex
    );
    assert_eq!(
        detect_format("@software{s1, title={T}}"),
        InputFormat::Bibtex
    );
    assert_eq!(
        detect_format("@Collection(c1, title={T})"),
        InputFormat::Bibtex
    );
}

#[test]
fn detect_does_not_false_positive_on_plain_text() {
    assert_eq!(
        detect_format("email me @ home {sometime}"),
        InputFormat::Unknown
    );
    assert_eq!(
        detect_format("mention @user in the thread"),
        InputFormat::Unknown
    );
}

// ── Parse error bounding ─────────────────────────────────────────────

#[test]
fn parse_errors_are_capped() {
    let mut input = String::new();
    for i in 0..300 {
        input.push_str(&format!("@article{{k{i}, title = {{broken\n\n"));
    }
    let res = parse_bibtex(&input, &ParseOptions::default());
    assert!(
        res.errors.len() <= 100,
        "error array must be bounded, got {}",
        res.errors.len()
    );
}
