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
use citeme_engine_core::export::ris::csl_json_to_ris;
use citeme_engine_core::parsers::bibtex::parse_bibtex;
use citeme_engine_core::parsers::detect::{detect_format, InputFormat};
use citeme_engine_core::parsers::ris::parse_ris;
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

// ── Hostile author names ─────────────────────────────────────────────
//
// The gate above only ever fed hostile *field* values. Author names take a
// different code path (`format_authors`) that skipped escaping entirely, so a
// family name carrying `}` closed the field and injected a second record.

fn hostile_author_items() -> Vec<Value> {
    vec![
        json!({
            "type": "article-journal",
            "id": "inj1",
            "title": "Good title",
            "author": [{"family": "X}, title={pwned}}\n@book{injected", "given": "E"}],
            "issued": {"date-parts": [[2024]]}
        }),
        json!({
            "type": "book",
            "id": "inj2",
            "title": "T",
            "author": [{"literal": "Institute of {Things} & Co. }"}],
            "issued": {"date-parts": [[2020]]}
        }),
        json!({
            "type": "article-journal",
            "id": "inj3",
            "title": "T",
            "editor": [{"family": "A\\b{c}", "given": "D% E$"}],
            "issued": {"date-parts": [[2021]]}
        }),
    ]
}

#[test]
fn bibtex_export_escapes_hostile_author_names() {
    for item in hostile_author_items() {
        let bib = csl_json_to_bibtex(&item);
        // The real test is the round-trip: a `@` inside a properly escaped
        // value is inert, so counting `@` would measure the wrong thing.
        let (entries, errors) = reimport(&bib);
        assert_eq!(
            (entries, errors),
            (1, 0),
            "hostile author must re-import as exactly one clean entry:\n{bib}"
        );
    }
}

#[test]
fn biblatex_export_escapes_hostile_author_names() {
    let items = hostile_author_items();
    let bib = csl_json_array_to_biblatex(&items);
    let (entries, errors) = reimport(&bib);
    assert_eq!(
        (entries, errors),
        (items.len(), 0),
        "hostile authors must re-import cleanly:\n{bib}"
    );
}

// ── RIS round-trip ───────────────────────────────────────────────────
//
// RIS is line-oriented: a newline inside any value ends the field, and a
// crafted one ends the whole record and opens another. Nothing was filtered,
// and the re-import reported zero errors — silent corruption, no fallback.

#[test]
fn ris_export_neutralizes_newline_injection() {
    let item = json!({
        "type": "article-journal",
        "id": "r1",
        "title": "Good\nER  - \nTY  - BOOK\nTI  - Injected Book",
        "author": [{"family": "S\nAU  - Ghost, G", "given": "J"}],
        "abstract": "line one\r\nER  - \r\nTY  - CHAP",
        "issued": {"date-parts": [[2024]]}
    });
    let ris = csl_json_to_ris(&item);
    assert_eq!(
        ris.matches("TY  - ").count(),
        1,
        "exactly one record may be opened:\n{ris}"
    );
    assert_eq!(
        ris.matches("ER  - ").count(),
        1,
        "exactly one record may be closed:\n{ris}"
    );
    let back = parse_ris(&ris, &ParseOptions::default());
    assert_eq!(
        back.entries.len(),
        1,
        "must re-import as exactly one entry:\n{ris}"
    );
}

#[test]
fn ris_export_round_trips_core_fields() {
    let item = json!({
        "type": "article-journal",
        "id": "r2",
        "title": "A Study of Things",
        "author": [{"family": "Souza", "given": "Bruno"}],
        "container-title": "Nature",
        "issued": {"date-parts": [[2023]]},
        "DOI": "10.1234/x"
    });
    let ris = csl_json_to_ris(&item);
    let back = parse_ris(&ris, &ParseOptions::default());
    assert_eq!((back.entries.len(), back.errors.len()), (1, 0), "{ris}");
    let got = &back.entries[0];
    assert_eq!(got["title"].as_str(), Some("A Study of Things"), "{ris}");
    assert_eq!(got["DOI"].as_str(), Some("10.1234/x"), "{ris}");
}

// ── Name particles ───────────────────────────────────────────────────
//
// CiteMe emits `non-dropping-particle` for names like "Maria da Silva"
// (paper-to-csl.ts). Every exporter read only `dropping-particle`, so the
// particle was silently dropped — a visible citation error in pt/es/nl/de.

#[test]
fn exports_preserve_non_dropping_particle() {
    let items = vec![json!({
        "type": "book",
        "id": "p1",
        "title": "T",
        "author": [{
            "family": "Silva",
            "given": "Maria",
            "non-dropping-particle": "da"
        }],
        "issued": {"date-parts": [[2024]]}
    })];

    let bib = csl_json_array_to_bibtex(&items);
    assert!(
        bib.contains("da Silva"),
        "bibtex must keep the particle:\n{bib}"
    );

    let biblatex = csl_json_array_to_biblatex(&items);
    assert!(
        biblatex.contains("da Silva"),
        "biblatex must keep the particle:\n{biblatex}"
    );

    let ris = csl_json_to_ris(&items[0]);
    assert!(
        ris.contains("da Silva"),
        "ris must keep the particle:\n{ris}"
    );

    let yaml = csl_json_array_to_hayagriva(&items);
    assert!(
        yaml.contains("da Silva"),
        "hayagriva must keep the particle:\n{yaml}"
    );
}

// ── Hayagriva keys ───────────────────────────────────────────────────
//
// Items without an `id` all took the literal key `entry`, and `id: ""`
// emitted an empty key. Duplicate YAML keys mean the last item silently wins.

#[test]
fn hayagriva_keys_are_unique_and_non_empty() {
    let items = vec![
        json!({"type": "book", "title": "A"}),
        json!({"type": "book", "title": "B"}),
        json!({"id": "", "type": "book", "title": "C"}),
    ];
    let yaml = csl_json_array_to_hayagriva(&items);
    let lib = hayagriva::io::from_yaml_str(&yaml);
    assert!(lib.is_ok(), "must load: {:?}\n{yaml}", lib.err());
    assert_eq!(
        lib.unwrap().len(),
        items.len(),
        "every item needs its own key:\n{yaml}"
    );
}
