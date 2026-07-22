//! Corpus smoke test: every CSL style CiteMe serves in production must load
//! and format a battery of representative items in every supported locale
//! WITHOUT PANICKING.
//!
//! Rust panics cross the Wasm boundary as aborts and poison the engine
//! instance (wasm32-unknown-unknown cannot unwind, so they are uncatchable
//! at the boundary). The class-level defense is making panics unreachable:
//! this test makes the next style edge case (like iso690-fr's multibyte
//! attribute in 0.3.2) fail the engine's CI instead of CiteMe's production.
//!
//! Corpus: `tests/fixtures/styles/corpus/` — see its README for provenance
//! and the refresh command.

use citeme_engine_core::engine::CitationEngine;
use citeme_engine_core::types::FormatOptions;

fn ws_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Items chosen to exercise distinct style macros: multibyte text, patent
/// `submitted` dates (the iso690-fr trigger), literal/institutional authors,
/// missing fields, accessed dates, page ranges.
fn battery() -> Vec<(&'static str, String)> {
    let items = [
        (
            "article",
            serde_json::json!({
                "type": "article-journal",
                "title": "Aspectos jurídicos da citação — déposé über naïve façade",
                "author": [
                    {"family": "García", "given": "María José"},
                    {"family": "Müller", "given": "Jürgen"}
                ],
                "container-title": "Revista de Direito & Tecnologia",
                "issued": {"date-parts": [[2024, 3, 14]]},
                "volume": "42", "issue": "3", "page": "100-115",
                "DOI": "10.1234/test.2024"
            }),
        ),
        (
            "book",
            serde_json::json!({
                "type": "book",
                "title": "A História do Brasil",
                "author": [{"family": "Santos", "given": "João"}],
                "issued": {"date-parts": [[2019]]},
                "publisher": "Editora Nacional",
                "publisher-place": "São Paulo",
                "edition": "3"
            }),
        ),
        (
            "chapter",
            serde_json::json!({
                "type": "chapter",
                "title": "Chapter Title",
                "author": [{"family": "Doe", "given": "Jane"}],
                "editor": [{"family": "Smith", "given": "Ed"}],
                "container-title": "The Big Book",
                "issued": {"date-parts": [[2021]]},
                "page": "55-78",
                "publisher": "Springer"
            }),
        ),
        (
            "patent",
            serde_json::json!({
                "type": "patent",
                "title": "Dispositif de chiffrement homomorphe",
                "author": [{"family": "Dupont", "given": "Marie"}],
                "submitted": {"date-parts": [[2019, 3, 14]]},
                "issued": {"date-parts": [[2021, 7, 2]]},
                "number": "FR3094000"
            }),
        ),
        (
            "thesis",
            serde_json::json!({
                "type": "thesis",
                "title": "Uma Tese sobre Sistemas Distribuídos",
                "author": [{"family": "Oliveira", "given": "Ana"}],
                "issued": {"date-parts": [[2023]]},
                "genre": "Tese de Doutorado",
                "publisher": "Universidade de São Paulo"
            }),
        ),
        (
            "webpage",
            serde_json::json!({
                "type": "webpage",
                "title": "Page « spéciale » — entités & echappées",
                "author": [{"literal": "World Health Organization"}],
                "issued": {"date-parts": [[2025, 1]]},
                "accessed": {"date-parts": [[2026, 6, 12]]},
                "URL": "https://example.org/page?q=1&r=2"
            }),
        ),
        (
            "dataset",
            serde_json::json!({
                "type": "dataset",
                "title": "Dataset X",
                "author": [{"family": "Lab", "given": "Some"}],
                "issued": {"date-parts": [[2023]]},
                "publisher": "Zenodo",
                "DOI": "10.5281/zenodo.1234567"
            }),
        ),
        (
            "no-author-year-only",
            serde_json::json!({
                "type": "report",
                "title": "Anonymous Report",
                "issued": {"date-parts": [[2020]]}
            }),
        ),
    ];
    items
        .iter()
        .map(|(name, v)| (*name, serde_json::to_string(v).unwrap()))
        .collect()
}

#[test]
fn corpus_styles_load_and_format_without_panicking() {
    let root = ws_root();
    let corpus_dir = root.join("tests/fixtures/styles/corpus");
    let locales_dir = root.join("tests/fixtures/locales");

    let mut engine = CitationEngine::new();

    let mut locale_codes: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&locales_dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if let Some(code) = name
            .strip_prefix("locales-")
            .and_then(|n| n.strip_suffix(".xml"))
        {
            engine
                .load_locale(code, &std::fs::read_to_string(&path).unwrap())
                .unwrap();
            locale_codes.push(code.to_string());
        }
    }
    assert!(
        locale_codes.len() >= 7,
        "expected the 7 CiteMe locales, got {locale_codes:?}"
    );

    let mut style_names: Vec<String> = Vec::new();
    let mut load_failures: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&corpus_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "csl") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let xml = std::fs::read_to_string(&path).unwrap();
        // A clean Err is acceptable in principle (never a panic), but every
        // corpus style is served in production, so a load failure is a real
        // regression worth failing on — collected so one run reports all.
        match engine.load_style(&name, &xml) {
            Ok(()) => style_names.push(name),
            Err(e) => load_failures.push(format!("{name}: {e}")),
        }
    }
    assert!(
        load_failures.is_empty(),
        "{} corpus style(s) failed to load:\n  {}",
        load_failures.len(),
        load_failures.join("\n  ")
    );
    assert!(
        style_names.len() >= 59,
        "corpus shrank? loaded {}",
        style_names.len()
    );

    let battery = battery();
    let mut format_failures: Vec<String> = Vec::new();
    for style in &style_names {
        for locale in &locale_codes {
            for (item_name, item_json) in &battery {
                if let Err(e) =
                    engine.format_one(item_json, style, locale, &FormatOptions::default())
                {
                    format_failures.push(format!("{style} × {locale} × {item_name}: {e}"));
                }
            }
        }
    }
    assert!(
        format_failures.is_empty(),
        "{} corpus format call(s) failed:\n  {}",
        format_failures.len(),
        format_failures.join("\n  ")
    );
}
