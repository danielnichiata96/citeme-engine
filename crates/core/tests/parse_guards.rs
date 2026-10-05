//! Input guards on the auto-detect path.
//!
//! `parseAuto` ran format detection *before* any size check, and detection
//! proves JSON-ness by deserializing the whole document into a `Value` — so a
//! 200 MB paste was fully allocated and parsed before `max_input_bytes` was
//! ever consulted. The guard has to come first.
//!
//! The RIS pair is the other half: detection accepted a record shape the
//! parser then skipped line by line, so a real bibliography returned zero
//! entries *and* zero errors. Silence is worse than an error — nothing
//! downstream can tell "empty" from "broken".

use citeme_engine_core::parsers::detect::{detect_format, InputFormat};
use citeme_engine_core::parsers::{parse_auto, ParseOptions};

#[test]
fn parse_auto_rejects_oversized_input_before_detecting() {
    let options = ParseOptions {
        max_input_bytes: 1024,
        ..Default::default()
    };
    // Valid JSON, comfortably over the cap.
    let mut big = String::from("[");
    for i in 0..500 {
        if i > 0 {
            big.push(',');
        }
        big.push_str(r#"{"type":"book","id":"x","title":"padding padding padding"}"#);
    }
    big.push(']');
    assert!(big.len() > options.max_input_bytes);

    let res = parse_auto(&big, &options);
    assert!(res.truncated, "oversized input must be reported truncated");
    assert!(res.entries.is_empty(), "nothing may be parsed past the cap");
    assert_eq!(res.errors.len(), 1, "must say why it refused");
}

#[test]
fn parse_auto_accepts_input_within_the_cap() {
    let options = ParseOptions {
        max_input_bytes: 10_000,
        ..Default::default()
    };
    let small = r#"[{"type":"book","id":"x","title":"Small"}]"#;
    let res = parse_auto(small, &options);
    assert!(!res.truncated, "input under the cap must not be truncated");
    assert_eq!(
        res.entries.len(),
        1,
        "must parse normally: {:?}",
        res.errors
    );
}

#[test]
fn ris_tags_without_a_trailing_space_still_parse() {
    // Real-world exporters emit "TY  -JOUR" as well as "TY  - JOUR".
    // detect_format already accepted this shape; the parser skipped it.
    let tight = "TY  -JOUR\nAU  -Smith, John\nTI  -A Tight Record\nPY  -2024///\nER  -";
    assert_eq!(detect_format(tight), InputFormat::Ris);

    let res = parse_auto(tight, &ParseOptions::default());
    assert_eq!(
        res.entries.len(),
        1,
        "detected as RIS, so it must parse: {:?}",
        res.errors
    );
    assert_eq!(res.entries[0]["title"].as_str(), Some("A Tight Record"));
}

#[test]
fn a_utf8_bom_does_not_hide_a_ris_file() {
    // Windows exporters prefix a byte-order mark; detection read the file
    // as "unknown" and the parser lost the first record.
    let res = parse_auto(
        "\u{FEFF}TY  - JOUR\nTI  - First\nER  - \nTY  - JOUR\nTI  - Second\nER  - \n",
        &ParseOptions::default(),
    );
    assert_eq!(res.format, "ris");
    assert_eq!(res.entries.len(), 2, "{:?}", res.errors);
    assert_eq!(res.entries[0]["title"].as_str(), Some("First"));
}

#[test]
fn an_at_mention_in_an_abstract_does_not_make_a_file_bibtex() {
    // "@WHO (" matched the BibTeX entry opener, so a RIS or MEDLINE file
    // citing a handle imported zero entries with one BibTeX parse error.
    let ris = "TY  - JOUR\nTI  - T\nAB  - Guidance from @WHO (World Health Organization) was followed.\nER  - \n";
    let res = parse_auto(ris, &ParseOptions::default());
    assert_eq!(res.format, "ris");
    assert_eq!(res.entries.len(), 1, "{:?}", res.errors);

    let nbib = "PMID- 1\nTI  - T.\nAB  - Data shared by @CDCgov (Centers for Disease Control).\nFAU - Smith, John\n";
    let res = parse_auto(nbib, &ParseOptions::default());
    assert_eq!(res.format, "medline");
    assert_eq!(res.entries.len(), 1, "{:?}", res.errors);
}

#[test]
fn detected_but_unparseable_input_reports_an_error() {
    // Anything the detector claims but the parser can't turn into a single
    // entry must surface an error rather than an empty, errorless result.
    let res = parse_auto("TY  - \nER  - ", &ParseOptions::default());
    assert!(
        !(res.entries.is_empty() && res.errors.is_empty()),
        "empty *and* errorless is the failure mode being guarded"
    );
}
