//! Property tests: no input may panic the engine's input-handling code.
//!
//! A Rust panic crosses the Wasm boundary as an abort and poisons the engine
//! instance — wasm32-unknown-unknown cannot unwind, so the boundary cannot
//! catch it. These properties make the panic CLASS unreachable for
//! user-controlled inputs (uploaded files, pasted text, style XML), rather
//! than chasing instances one production incident at a time. The 0.3.2
//! iso690-fr panic (byte-indexed slice mid-multibyte-char in the CSL
//! normalizer) would have been caught here.

use std::sync::OnceLock;

use citeme_engine_core::abnt::post_process_abnt;
use citeme_engine_core::engine::CitationEngine;
use citeme_engine_core::export;
use citeme_engine_core::normalize::normalize_csl_xml;
use citeme_engine_core::parsers::{self, detect::detect_format, ParseOptions};
use citeme_engine_core::types::{FormatOptions, OutputFormat};
use proptest::prelude::*;
use serde_json::{json, Map, Value};

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

/// Every fixture style, loaded once: formatting reaches hayagriva code that
/// only runs for the variables a style actually renders.
fn styled_engine() -> &'static (CitationEngine, Vec<String>) {
    static ENGINE: OnceLock<(CitationEngine, Vec<String>)> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let mut engine = CitationEngine::new();
        for code in ["en-US", "fr-FR"] {
            let xml =
                std::fs::read_to_string(root.join(format!("locales/locales-{code}.xml"))).unwrap();
            engine.load_locale(code, &xml).unwrap();
        }
        let mut styles = Vec::new();
        for entry in std::fs::read_dir(root.join("styles")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "csl") {
                let name = path.file_stem().unwrap().to_string_lossy().into_owned();
                engine
                    .load_style(&name, &std::fs::read_to_string(&path).unwrap())
                    .unwrap();
                styles.push(name);
            }
        }
        styles.sort();
        (engine, styles)
    })
}

/// A date-part component: real-looking numbers, numeric strings, and the
/// values citationberg mishandles (empty strings, zero, out-of-range).
fn arb_date_component() -> impl Strategy<Value = Value> {
    prop_oneof![
        (1800i64..2100).prop_map(Value::from),
        (-40i64..40).prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        Just(json!("")),
        Just(json!("2019")),
        Just(json!(" 5 ")),
        Just(Value::Null),
        Just(json!(1.5)),
    ]
}

/// CSL-JSON dates, including ranges, empty dates and `raw`/`literal` forms.
fn arb_date() -> impl Strategy<Value = Value> {
    let parts = proptest::collection::vec(
        proptest::collection::vec(arb_date_component(), 0..5).prop_map(Value::Array),
        0..4,
    )
    .prop_map(Value::Array);
    let raw = prop_oneof![
        Just("2019".to_string()),
        Just("2019-05-03".to_string()),
        Just("2019/2020".to_string()),
        Just("2019-05~".to_string()),
        Just("2019-00-40".to_string()),
        "[0-9~/ -]{0,12}",
        "\\PC{0,8}",
    ];
    prop_oneof![
        4 => (parts, proptest::option::of(any::<u8>()), any::<bool>()).prop_map(|(p, season, circa)| {
            let mut d = Map::new();
            d.insert("date-parts".into(), p);
            if let Some(s) = season { d.insert("season".into(), Value::from(s)); }
            if circa { d.insert("circa".into(), Value::Bool(true)); }
            Value::Object(d)
        }),
        2 => raw.prop_map(|r| json!({ "raw": r })),
        1 => "\\PC{0,10}".prop_map(|l| json!({ "literal": l })),
        1 => Just(Value::Null),
        1 => Just(json!("2019")),
    ]
}

fn arb_name() -> impl Strategy<Value = Value> {
    let part = prop_oneof![
        "\\PC{0,10}".prop_map(Value::String),
        Just(Value::Null),
        any::<i64>().prop_map(Value::from),
    ];
    prop_oneof![
        8 => proptest::collection::btree_map(
            prop_oneof![
                Just("family".to_string()),
                Just("given".to_string()),
                Just("literal".to_string()),
                Just("non-dropping-particle".to_string()),
                Just("dropping-particle".to_string()),
                Just("suffix".to_string()),
                Just("comma-suffix".to_string()),
                Just("sequence".to_string()),
            ],
            part,
            0..5,
        )
        .prop_map(|m| Value::Object(m.into_iter().collect())),
        1 => Just(Value::Null),
        1 => "\\PC{0,10}".prop_map(Value::String),
    ]
}

/// A CSL-JSON item shaped like real input — typed fields, CSL variable
/// names — but with every value drawn from what users actually paste.
fn arb_csl_item() -> impl Strategy<Value = Value> {
    let text = prop_oneof![
        "\\PC{0,16}".prop_map(Value::String),
        Just(json!("100-115")),
        Just(json!("e1234")),
        Just(json!("S12–S15")),
        any::<i64>().prop_map(Value::from),
        Just(json!(2.5)),
        Just(json!(u64::MAX)),
        Just(Value::Null),
        Just(Value::Bool(true)),
        Just(json!(["a", "b"])),
        Just(json!({"eprint": {"id": "1706.03762", "type": "arxiv"}})),
    ];
    (
        prop_oneof![
            Just("article-journal"),
            Just("book"),
            Just("chapter"),
            Just("thesis"),
            Just("webpage"),
            Just("patent"),
            Just("paper-conference"),
            Just("report"),
            Just("legal_case"),
            Just("motion_picture"),
            Just("dataset"),
            Just("nonsense"),
        ],
        proptest::collection::btree_map(
            prop_oneof![
                Just("id"),
                Just("title"),
                Just("container-title"),
                Just("publisher"),
                Just("volume"),
                Just("issue"),
                Just("page"),
                Just("edition"),
                Just("number"),
                Just("DOI"),
                Just("URL"),
                Just("genre"),
                Just("note"),
                Just("language"),
                Just("keyword"),
                Just("custom"),
                Just("citation-number"),
                Just("locator"),
            ],
            text,
            0..8,
        ),
        proptest::collection::btree_map(
            prop_oneof![
                Just("author"),
                Just("editor"),
                Just("translator"),
                Just("director")
            ],
            prop_oneof![
                8 => proptest::collection::vec(arb_name(), 0..6).prop_map(Value::Array),
                1 => Just(Value::Null),
                1 => "\\PC{0,10}".prop_map(Value::String),
            ],
            0..3,
        ),
        proptest::collection::btree_map(
            prop_oneof![
                Just("issued"),
                Just("accessed"),
                Just("submitted"),
                Just("event-date"),
                Just("original-date")
            ],
            arb_date(),
            0..3,
        ),
    )
        .prop_map(|(kind, fields, names, dates)| {
            let mut item = Map::new();
            item.insert("type".into(), Value::from(kind));
            for (k, v) in fields.into_iter().chain(names).chain(dates) {
                item.insert(k.into(), v);
            }
            Value::Object(item)
        })
}

/// Citation-shaped text for the ABNT post-processor: parentheses in any
/// order, separators, markup and multibyte names.
fn citationish() -> impl Strategy<Value = String> {
    let fragment = prop_oneof![
        Just("(".to_string()),
        Just(")".to_string()),
        Just(";".to_string()),
        Just(",".to_string()),
        Just(" ".to_string()),
        Just("et al.".to_string()),
        Just("<span style=\"font-style: italic;\">".to_string()),
        Just("</span>".to_string()),
        Just("<".to_string()),
        Just(">".to_string()),
        Just("Ünïcödé".to_string()),
        Just("IBGE".to_string()),
        "[a-zA-Z0-9é ]{0,6}",
    ];
    proptest::collection::vec(fragment, 0..16).prop_map(|v| v.concat())
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

    #[test]
    fn abnt_post_processing_never_panics(s in citationish(), in_text in any::<bool>()) {
        let _ = post_process_abnt(&s, in_text);
    }

    // hayagriva panics on a CSL-JSON date range and citationberg on an
    // empty date — both reached through format_one with valid CSL-JSON.
    #[test]
    fn formatting_never_panics_on_arbitrary_items(
        item in arb_csl_item(),
        other in arb_csl_item(),
        style in any::<prop::sample::Index>(),
        plain in any::<bool>(),
        prose in any::<bool>(),
        abnt in any::<bool>(),
        french in any::<bool>(),
    ) {
        let (engine, styles) = styled_engine();
        let style = style.get(styles);
        let locale = if french { "fr-FR" } else { "en-US" };
        let options = FormatOptions {
            output_format: if plain { OutputFormat::Plain } else { OutputFormat::Html },
            abnt_post_process: abnt,
            prose,
        };
        let _ = engine.format_one(&item.to_string(), style, locale, &options);
        let batch = Value::Array(vec![item.clone(), other, item]);
        let _ = engine.format_batch(&batch.to_string(), style, locale, &options);
    }
}
