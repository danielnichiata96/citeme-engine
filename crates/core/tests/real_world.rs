//! Integration test: parse real-world files from CrossRef, PubMed, Semantic Scholar
//! and format them through the engine.

use citeme_engine_core::engine::CitationEngine;
use citeme_engine_core::parsers::{self, ParseOptions};
use citeme_engine_core::parsers::detect::{detect_format, InputFormat};
use citeme_engine_core::types::FormatOptions;

fn setup_engine() -> CitationEngine {
    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap();

    let mut engine = CitationEngine::new();

    // Load styles
    for style_file in std::fs::read_dir(ws_root.join("tests/fixtures/styles")).unwrap() {
        let path = style_file.unwrap().path();
        if path.extension().map_or(false, |e| e == "csl") {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let xml = std::fs::read_to_string(&path).unwrap();
            engine.load_style(name, &xml).unwrap();
        }
    }

    // Load locales
    for locale_file in std::fs::read_dir(ws_root.join("tests/fixtures/locales")).unwrap() {
        let path = locale_file.unwrap().path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("locales-") && name.ends_with(".xml") {
                let code = name.strip_prefix("locales-").unwrap().strip_suffix(".xml").unwrap();
                let xml = std::fs::read_to_string(&path).unwrap();
                engine.load_locale(code, &xml).unwrap();
            }
        }
    }

    engine
}

// ─── BibTeX from CrossRef API ───

const REAL_BIBTEX: &str = include_str!("../../../tests/fixtures/real-world/crossref.bib");

#[test]
fn test_real_bibtex_detect() {
    assert_eq!(detect_format(REAL_BIBTEX), InputFormat::Bibtex);
}

#[test]
fn test_real_bibtex_parse() {
    let result = parsers::bibtex::parse_bibtex(REAL_BIBTEX, &ParseOptions::default());
    println!("\n=== BibTeX parse: {} entries, {} errors ===", result.entries.len(), result.errors.len());
    for (i, entry) in result.entries.iter().enumerate() {
        println!("  [{}] type={}, title={}", i,
            entry["type"].as_str().unwrap_or("?"),
            entry.get("title").and_then(|t| t.as_str()).unwrap_or("?"));
    }
    for err in &result.errors {
        println!("  ERROR: {} — {}", err.preview, err.error);
    }

    assert!(result.entries.len() >= 3, "should parse at least 3 entries from CrossRef BibTeX: got {}", result.entries.len());
    assert!(result.errors.is_empty(), "should have no parse errors: {:?}", result.errors);
}

#[test]
fn test_real_bibtex_format_apa() {
    let engine = setup_engine();
    let result = parsers::bibtex::parse_bibtex(REAL_BIBTEX, &ParseOptions::default());
    let opts = FormatOptions::default();

    println!("\n=== BibTeX → APA formatting ===");
    for (i, entry) in result.entries.iter().enumerate() {
        let json_str = serde_json::to_string(entry).unwrap();
        match engine.format_one(&json_str, "apa", "en-US", &opts) {
            Ok(fmt) => {
                println!("\n  [{}] reference: {}", i, fmt.reference);
                println!("       in_text:   {}", fmt.in_text);
                assert!(!fmt.reference.is_empty(), "reference should not be empty for entry {i}");
                assert!(!fmt.in_text.is_empty(), "in_text should not be empty for entry {i}");
            }
            Err(e) => panic!("format_one failed for entry {i}: {e}"),
        }
    }
}

// ─── RIS from CrossRef API ───

const REAL_RIS: &str = include_str!("../../../tests/fixtures/real-world/crossref.ris");

#[test]
fn test_real_ris_detect() {
    assert_eq!(detect_format(REAL_RIS), InputFormat::Ris);
}

#[test]
fn test_real_ris_parse() {
    let result = parsers::ris::parse_ris(REAL_RIS, &ParseOptions::default());
    println!("\n=== RIS parse: {} entries, {} errors ===", result.entries.len(), result.errors.len());
    for (i, entry) in result.entries.iter().enumerate() {
        println!("  [{}] type={}, title={}", i,
            entry["type"].as_str().unwrap_or("?"),
            entry.get("title").and_then(|t| t.as_str()).unwrap_or("?"));
    }

    assert_eq!(result.entries.len(), 2, "should parse 2 RIS entries from CrossRef");
}

#[test]
fn test_real_ris_format_apa() {
    let engine = setup_engine();
    let result = parsers::ris::parse_ris(REAL_RIS, &ParseOptions::default());
    let opts = FormatOptions::default();

    println!("\n=== RIS → APA formatting ===");
    for (i, entry) in result.entries.iter().enumerate() {
        let json_str = serde_json::to_string(entry).unwrap();
        match engine.format_one(&json_str, "apa", "en-US", &opts) {
            Ok(fmt) => {
                println!("\n  [{}] reference: {}", i, fmt.reference);
                println!("       in_text:   {}", fmt.in_text);
                assert!(!fmt.reference.is_empty(), "reference should not be empty for entry {i}");
                assert!(!fmt.in_text.is_empty(), "in_text should not be empty for entry {i}");
            }
            Err(e) => panic!("format_one failed for entry {i}: {e}"),
        }
    }
}

// ─── MEDLINE/NBIB from PubMed ───

const REAL_NBIB: &str = include_str!("../../../tests/fixtures/real-world/pubmed.nbib");

#[test]
fn test_real_nbib_detect() {
    assert_eq!(detect_format(REAL_NBIB), InputFormat::Medline);
}

#[test]
fn test_real_nbib_parse() {
    let result = parsers::medline::parse_medline(REAL_NBIB, &ParseOptions::default());
    println!("\n=== MEDLINE parse: {} entries, {} errors ===", result.entries.len(), result.errors.len());
    for (i, entry) in result.entries.iter().enumerate() {
        println!("  [{}] type={}, title={}", i,
            entry["type"].as_str().unwrap_or("?"),
            entry.get("title").and_then(|t| t.as_str()).unwrap_or("?"));
        if let Some(authors) = entry["author"].as_array() {
            for a in authors {
                println!("       author: {} {}", a["family"].as_str().unwrap_or(""), a["given"].as_str().unwrap_or(""));
            }
        }
        if let Some(doi) = entry["DOI"].as_str() {
            println!("       DOI: {}", doi);
        }
    }

    assert_eq!(result.entries.len(), 1, "should parse 1 PubMed entry");
    let entry = &result.entries[0];
    assert!(entry["title"].as_str().unwrap().contains("Pleomorphic Adenoma"), "title should contain expected text");
    assert_eq!(entry["author"].as_array().unwrap().len(), 7, "should have 7 authors");
    assert_eq!(entry["DOI"].as_str().unwrap(), "10.1055/a-2166-8334");
}

#[test]
fn test_real_nbib_format_apa() {
    let engine = setup_engine();
    let result = parsers::medline::parse_medline(REAL_NBIB, &ParseOptions::default());
    let opts = FormatOptions::default();

    println!("\n=== MEDLINE → APA formatting ===");
    for (i, entry) in result.entries.iter().enumerate() {
        let json_str = serde_json::to_string(entry).unwrap();
        match engine.format_one(&json_str, "apa", "en-US", &opts) {
            Ok(fmt) => {
                println!("\n  [{}] reference: {}", i, fmt.reference);
                println!("       in_text:   {}", fmt.in_text);
                assert!(!fmt.reference.is_empty(), "reference should not be empty");
                assert!(!fmt.in_text.is_empty(), "in_text should not be empty");
            }
            Err(e) => panic!("format_one failed for entry {i}: {e}"),
        }
    }
}

// ─── Auto-detect pipeline ───

#[test]
fn test_real_auto_detect_and_parse() {
    let opts = ParseOptions::default();

    // BibTeX
    let bib = parsers::detect::detect_format(REAL_BIBTEX);
    assert_eq!(bib, InputFormat::Bibtex);

    // RIS
    let ris = parsers::detect::detect_format(REAL_RIS);
    assert_eq!(ris, InputFormat::Ris);

    // MEDLINE
    let nbib = parsers::detect::detect_format(REAL_NBIB);
    assert_eq!(nbib, InputFormat::Medline);

    // CSL-JSON
    let csl = r#"[{"type":"article-journal","title":"Test"}]"#;
    assert_eq!(parsers::detect::detect_format(csl), InputFormat::CslJson);

    println!("\n=== Auto-detect results ===");
    println!("  BibTeX:  {:?}", bib);
    println!("  RIS:     {:?}", ris);
    println!("  MEDLINE: {:?}", nbib);
    println!("  CSL-JSON: CslJson");
}

// ─── Multi-style formatting ───

#[test]
fn test_real_bibtex_multi_style() {
    let engine = setup_engine();
    let result = parsers::bibtex::parse_bibtex(REAL_BIBTEX, &ParseOptions::default());
    let first_entry = serde_json::to_string(&result.entries[0]).unwrap();

    let styles = ["apa", "ieee", "mla", "chicago-author-date", "vancouver", "harvard"];

    println!("\n=== Kucsko et al. (2013) Nature — multi-style ===");
    for style in &styles {
        let opts = FormatOptions::default();
        match engine.format_one(&first_entry, style, "en-US", &opts) {
            Ok(fmt) => {
                println!("\n  {}: {}", style, fmt.reference);
                println!("  {} in-text: {}", style, fmt.in_text);
                assert!(!fmt.reference.is_empty(), "{style} reference should not be empty");
                assert!(!fmt.in_text.is_empty(), "{style} in_text should not be empty");
            }
            Err(e) => println!("  {}: ERROR — {}", style, e),
        }
    }
}
