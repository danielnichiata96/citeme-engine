//! Property tests for the line-oriented importers (RIS, MEDLINE) and format
//! detection.
//!
//! Random strings almost never look like a tagged file, so they never reach
//! the line logic: wrapped lines, tag indentation, byte-order marks, records
//! without terminators. These inputs are built from real tag lines, wrapped
//! and indented the way exporters and pastes do it, with multibyte text
//! next to every boundary the parsers slice at.

use citeme_engine_core::parsers::{
    detect::detect_format, medline::parse_medline, parse_auto, ris::parse_ris, ParseOptions,
};
use proptest::prelude::*;

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("Smith, John".to_string()),
        Just("Smith JA Jr".to_string()),
        Just("05/12/2019".to_string()),
        Just("2019/05/12/".to_string()),
        Just("2023 Jan-Feb".to_string()),
        Just("1476-4687 (Electronic)".to_string()),
        Just("HIV-1 infection".to_string()),
        Just("@WHO (World Health Organization)".to_string()),
        Just("PMC6500000".to_string()),
        Just("déposé — façade".to_string()),
        Just("中文引用".to_string()),
        "[a-zA-Z0-9éü€ ,.()-]{0,24}",
    ]
}

fn line() -> impl Strategy<Value = String> {
    let tag = prop_oneof![
        Just("TY"),
        Just("ER"),
        Just("TI"),
        Just("T2"),
        Just("AU"),
        Just("A2"),
        Just("ED"),
        Just("DA"),
        Just("PY"),
        Just("Y2"),
        Just("SN"),
        Just("SP"),
        Just("EP"),
        Just("ET"),
        Just("KW"),
        Just("AB"),
        Just("PMID"),
        Just("PMC"),
        Just("FAU"),
        Just("CN"),
        Just("DP"),
        Just("JT"),
        Just("LID"),
    ];
    let separator = prop_oneof![
        Just("  - "),
        Just("- "),
        Just(" - "),
        Just("  -"),
        Just("-")
    ];
    let indent = prop_oneof![Just(""), Just("  "), Just("      "), Just("\t")];
    let bom = prop_oneof![9 => Just(""), 1 => Just("\u{FEFF}")];
    prop_oneof![
        6 => (bom, indent.clone(), tag, separator, text())
            .prop_map(|(bom, indent, tag, sep, value)| format!("{bom}{indent}{tag}{sep}{value}")),
        2 => (indent, text()).prop_map(|(indent, value)| format!("{indent}{value}")),
        1 => Just(String::new()),
    ]
}

fn tagged_file() -> impl Strategy<Value = String> {
    proptest::collection::vec(line(), 0..40).prop_map(|lines| lines.join("\n"))
}

proptest! {
    #[test]
    fn tagged_importers_never_panic_and_keep_their_contract(
        input in tagged_file(),
        max in proptest::option::of(0usize..4),
    ) {
        let options = ParseOptions { max_entries: max, ..Default::default() };
        let _ = detect_format(&input);
        for result in [
            parse_ris(&input, &options),
            parse_medline(&input, &options),
            parse_auto(&input, &options),
        ] {
            prop_assert!(result.entries.iter().all(serde_json::Value::is_object));
            if let Some(max) = max {
                prop_assert!(result.entries.len() <= max, "{} > {max}", result.entries.len());
            }
        }
    }
}
