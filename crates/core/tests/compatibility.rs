use citeme_engine_core::engine::CitationEngine;
use citeme_engine_core::types::FormatOptions;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct Fixture {
    description: String,
    style: String,
    locale: String,
    input: serde_json::Value,
    expected: Expected,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    reference: String,
    in_text: String,
}

/// Fuzzy comparison: normalize whitespace and ignore minor punctuation diffs.
fn fuzzy_eq(a: &str, b: &str) -> bool {
    let normalize = |s: &str| -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
            .replace('\u{00a0}', " ")  // non-breaking space
            .replace(" .", ".")
            .trim().to_string()
    };
    normalize(a) == normalize(b)
}

#[test]
fn test_compatibility_fixtures() {
    // Paths relative to workspace root (2 levels up from crates/core/)
    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()  // crates/
        .parent().unwrap(); // workspace root
    let fixture_dir = ws_root.join("tests/fixtures/compatibility");
    let style_dir = ws_root.join("tests/fixtures/styles");
    let locale_dir = ws_root.join("tests/fixtures/locales");

    let mut engine = CitationEngine::new();

    // Pre-load styles used by fixtures
    for style_file in fs::read_dir(&style_dir).unwrap() {
        let path = style_file.unwrap().path();
        if path.extension().map_or(false, |e| e == "csl") {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let xml = fs::read_to_string(&path).unwrap();
            engine.load_style(name, &xml).unwrap();
        }
    }

    // Pre-load locales
    for locale_file in fs::read_dir(&locale_dir).unwrap() {
        let path = locale_file.unwrap().path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("locales-") && name.ends_with(".xml") {
                let code = name.strip_prefix("locales-").unwrap().strip_suffix(".xml").unwrap();
                let xml = fs::read_to_string(&path).unwrap();
                engine.load_locale(code, &xml).unwrap();
            }
        }
    }

    // Run each fixture
    let mut passed = 0;
    let mut failed = 0;
    let mut failures: Vec<String> = Vec::new();

    for entry in fs::read_dir(&fixture_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map_or(true, |e| e != "json") {
            continue;
        }

        let content = fs::read_to_string(&path).unwrap();
        let fixture: Fixture = serde_json::from_str(&content).unwrap();

        let input_str = serde_json::to_string(&fixture.input).unwrap();
        let opts = FormatOptions {
            abnt_post_process: fixture.style == "abnt",
            ..Default::default()
        };

        match engine.format_one(&input_str, &fixture.style, &fixture.locale, &opts) {
            Ok(result) => {
                let ref_match = fuzzy_eq(&result.reference, &fixture.expected.reference);
                let cite_match = fuzzy_eq(&result.in_text, &fixture.expected.in_text);

                if ref_match && cite_match {
                    passed += 1;
                } else {
                    failed += 1;
                    let mut msg = format!("FAIL: {} ({})\n", fixture.description, path.display());
                    if !ref_match {
                        msg.push_str(&format!("  reference expected: {}\n", fixture.expected.reference));
                        msg.push_str(&format!("  reference actual:   {}\n", result.reference));
                    }
                    if !cite_match {
                        msg.push_str(&format!("  inText expected: {}\n", fixture.expected.in_text));
                        msg.push_str(&format!("  inText actual:   {}\n", result.in_text));
                    }
                    failures.push(msg);
                }
            }
            Err(e) => {
                failed += 1;
                failures.push(format!("ERROR: {} — {e}\n", fixture.description));
            }
        }
    }

    let total = passed + failed;
    let pass_rate = if total > 0 { (passed as f64 / total as f64) * 100.0 } else { 0.0 };

    println!("\nCompatibility results: {passed} passed, {failed} failed");
    println!("Compatibility: {passed}/{total} ({pass_rate:.0}%)");

    if !failures.is_empty() {
        println!("\nFailures:");
        for f in &failures {
            println!("{f}");
        }
    }

    assert!(total >= 10,
        "GATE FAIL: only {total} fixtures found — need at least 10.");

    assert!(pass_rate >= 90.0,
        "GATE FAIL: parity {pass_rate:.0}% < 90% threshold. \
         {failed} fixture(s) diverge from expected output.");
}
