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

/// Normalize to plain text for semantic comparison.
///
/// citation-js (CiteMe default) outputs plain text (no HTML tags).
/// Hayagriva outputs HTML with <span style="..."> and <a href="...">.
/// This function strips ALL HTML to plain text so both can be compared.
fn normalize_to_plain(s: &str) -> String {
    let mut out = s.to_string();

    // Strip <a href="...">TEXT</a> → TEXT (before generic tag stripping)
    out = strip_a_tags(&out);

    // Strip ALL HTML tags: <anything> → empty
    // This handles: <div>, <span style="...">, <i>, <b>, </div>, </span>, </i>, </b>, etc.
    let mut result = String::with_capacity(out.len());
    let mut in_tag = false;
    for ch in out.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }
    out = result;

    // Decode HTML entities
    out = out.replace("&#38;", "&");
    out = out.replace("&#60;", "<");
    out = out.replace("&#62;", ">");
    out = out.replace("&amp;", "&");
    out = out.replace("&lt;", "<");
    out = out.replace("&gt;", ">");

    // Normalize smart quotes to ASCII
    out = out.replace('\u{2018}', "'");
    out = out.replace('\u{2019}', "'");
    out = out.replace('\u{201C}', "\"");
    out = out.replace('\u{201D}', "\"");

    // Normalize whitespace
    out = out.replace('\u{00a0}', " ");
    out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out = out.replace(" .", ".");
    out.trim().to_string()
}

/// Strip <a href="...">TEXT</a> to just TEXT.
fn strip_a_tags(s: &str) -> String {
    let mut out = s.to_string();
    loop {
        if let Some(start) = out.find("<a ") {
            if let Some(close_open) = out[start..].find('>') {
                let after = start + close_open + 1;
                if let Some(end) = out[after..].find("</a>") {
                    let text = out[after..after + end].to_string();
                    out = format!("{}{}{}", &out[..start], text, &out[after + end + "</a>".len()..]);
                    continue;
                }
            }
        }
        break;
    }
    out
}

/// Fuzzy comparison: normalize HTML representation and whitespace.
fn fuzzy_eq(a: &str, b: &str) -> bool {
    normalize_to_plain(a) == normalize_to_plain(b)
}

#[test]
fn test_normalize_to_plain() {
    // Strip all HTML tags
    assert_eq!(
        normalize_to_plain(r#"<div class="csl-bib-body"><div class="csl-entry">Hello</div></div>"#),
        "Hello"
    );

    // Strip span style tags to plain text
    assert_eq!(
        normalize_to_plain(r#"<span style="font-style: italic;">Title</span>"#),
        "Title"
    );
    assert_eq!(
        normalize_to_plain(r#"<span style="font-weight: bold;">Title</span>"#),
        "Title"
    );

    // Decode HTML entities
    assert_eq!(normalize_to_plain("Smith, J. A., &#38; Doe"), "Smith, J. A., & Doe");

    // Strip <a> links to just text
    assert_eq!(
        normalize_to_plain(r#"https://doi.org/<a href="https://doi.org/10.1234">10.1234</a>"#),
        "https://doi.org/10.1234"
    );

    // Combined: citation-js plain text vs Hayagriva HTML should match
    let citation_js_plain = "Smith, J. A., & Doe, J. B. (2024). Title. Journal, 42(3), 100–115.";
    let hayagriva_html = r#"Smith, J. A., & Doe, J. B. (2024). Title. <span style="font-style: italic;">Journal</span>, <span style="font-style: italic;">42</span>(3), 100–115."#;
    assert_eq!(normalize_to_plain(citation_js_plain), normalize_to_plain(hayagriva_html));
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

    // Known divergences from citation-js (tracked, not bugs):
    // - ABNT in-text: our engine uppercases per NBR 10520:2023, citation-js doesn't
    // - Vancouver/IEEE: Hayagriva omits csl-left-margin numbering (1., [1])
    // - ABNT tese: Hayagriva nests italic/bold spans differently
    // Gate: 75% accounts for known Hayagriva rendering limitations.
    // Raise as Hayagriva improves its CSL test suite coverage.
    assert!(pass_rate >= 75.0,
        "GATE FAIL: parity {pass_rate:.0}% < 75% threshold. \
         {failed} fixture(s) diverge from citation-js output.");
}
