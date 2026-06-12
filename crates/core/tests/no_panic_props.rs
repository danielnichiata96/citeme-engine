//! Property tests: no input may panic the engine's input-handling code.
//!
//! A Rust panic crosses the Wasm boundary as an abort and poisons the engine
//! instance — wasm32-unknown-unknown cannot unwind, so the boundary cannot
//! catch it. These properties make the panic CLASS unreachable for
//! user-controlled inputs (uploaded files, pasted text, style XML), rather
//! than chasing instances one production incident at a time. The 0.3.2
//! iso690-fr panic (byte-indexed slice mid-multibyte-char in the CSL
//! normalizer) would have been caught here.

use citeme_engine_core::export;
use citeme_engine_core::normalize::normalize_csl_xml;
use citeme_engine_core::parsers::{self, detect::detect_format, ParseOptions};
use proptest::prelude::*;
use serde_json::Value;

/// XML-ish fragments biased toward the normalizer's trigger grammar,
/// deliberately heavy on multibyte chars adjacent to attribute syntax.
fn xmlish() -> impl Strategy<Value = String> {
    let fragment = prop_oneof![
        Just("<date".to_string()),
        Just("<date-part".to_string()),
        Just("</date>".to_string()),
        Just(" delimiter=".to_string()),
        Just(" prefix=".to_string()),
        Just("\"".to_string()),
        Just("'".to_string()),
        Just("/>".to_string()),
        Just(">".to_string()),
        Just(" ".to_string()),
        Just("déposé".to_string()),
        Just("中文引用".to_string()),
        Just("🧪".to_string()),
        Just("name=\"year\"".to_string()),
        "[a-zA-Z0-9é€ ]{0,8}",
    ];
    proptest::collection::vec(fragment, 0..24).prop_map(|v| v.concat())
}

/// Recursive arbitrary JSON for the exporters (they accept any CSL-JSON-ish
/// value and must degrade gracefully, never panic).
fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| Value::Number(n.into())),
        "\\PC{0,12}".prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            proptest::collection::hash_map(
                prop_oneof![
                    Just("type".to_string()),
                    Just("title".to_string()),
                    Just("author".to_string()),
                    Just("issued".to_string()),
                    Just("date-parts".to_string()),
                    Just("family".to_string()),
                    Just("page".to_string()),
                    Just("keyword".to_string()),
                    "\\PC{0,8}",
                ],
                inner,
                0..4,
            )
            .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    #[test]
    fn normalize_never_panics_on_arbitrary_input(s in "\\PC{0,300}") {
        let _ = normalize_csl_xml(&s);
    }

    #[test]
    fn normalize_never_panics_on_xmlish_input(s in xmlish()) {
        let _ = normalize_csl_xml(&s);
    }

    #[test]
    fn parsers_never_panic_on_arbitrary_input(s in "\\PC{0,300}") {
        let opts = ParseOptions::default();
        let _ = detect_format(&s);
        let _ = parsers::bibtex::parse_bibtex(&s, &opts);
        let _ = parsers::ris::parse_ris(&s, &opts);
        let _ = parsers::medline::parse_medline(&s, &opts);
        let _ = parsers::csl_json::parse_csl_json(&s, &opts);
    }

    #[test]
    fn parsers_never_panic_on_bibtexish_input(
        s in "(@[a-z]{0,12})?[{}(),=\"\\\\a-zA-Zé\u{0301}0-9 \n]{0,200}"
    ) {
        let opts = ParseOptions::default();
        let _ = parsers::bibtex::parse_bibtex(&s, &opts);
    }

    #[test]
    fn exporters_never_panic_on_arbitrary_json(v in arb_json()) {
        let _ = export::bibtex::csl_json_to_bibtex(&v);
        let _ = export::ris::csl_json_to_ris(&v);
        let _ = export::biblatex::csl_json_to_biblatex(&v);
        let _ = export::hayagriva::csl_json_to_hayagriva(&v);
        let items = vec![v];
        let _ = export::bibtex::csl_json_array_to_bibtex(&items);
        let _ = export::ris::csl_json_array_to_ris(&items);
        let _ = export::biblatex::csl_json_array_to_biblatex(&items);
        let _ = export::hayagriva::csl_json_array_to_hayagriva(&items);
    }
}
