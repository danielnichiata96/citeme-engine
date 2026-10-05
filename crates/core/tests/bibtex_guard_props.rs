//! Property tests: no BibTeX input may panic or overflow the stack.
//!
//! `no_panic_props.rs` feeds the parsers random text, which almost never
//! forms a field: the `biblatex` crate's panics sit behind well-formed
//! syntax — a date field holding "²²", a `crossref` naming its own entry,
//! an `@string` defined through itself. This generator writes real entries
//! out of the pieces those paths need: date fields with signs and non-ASCII
//! digits, keys reused across `crossref`/`xdata`/`ids` and `@string`, and
//! nested LaTeX. Each case parses on a 1 MiB stack, the Wasm instance's.

use citeme_engine_core::parsers::{bibtex::parse_bibtex, parse_auto, ParseOptions};
use proptest::prelude::*;

/// Text fragments biased toward the date, key and LaTeX parsers.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        "[0-9]{1,5}",
        Just("²²".to_string()),
        Just("٢٠".to_string()),
        Just("Ⅳ".to_string()),
        Just("½".to_string()),
        Just("-".to_string()),
        Just("+ ".to_string()),
        Just("- 2020".to_string()),
        Just("/".to_string()),
        Just("..".to_string()),
        Just("XX".to_string()),
        Just("may".to_string()),
        Just(" 300".to_string()),
        Just("T10:00".to_string()),
        Just("~".to_string()),
        Just("k0".to_string()),
        Just("k1".to_string()),
        Just(", ".to_string()),
        Just(r"\emph{".to_string()),
        Just(r"\o{}".to_string()),
        Just(r"\'{e}".to_string()),
        Just(r"\url{".to_string()),
        Just(r"{\it ".to_string()),
        Just("{".to_string()),
        Just("}".to_string()),
        Just("$".to_string()),
        Just(r"\".to_string()),
        Just(" and others".to_string()),
        // Past what the guard inspects as text.
        Just("x".repeat(4_100)),
        "[a-zé ]{0,6}",
    ]
}

fn text() -> impl Strategy<Value = String> {
    proptest::collection::vec(fragment(), 0..8).prop_map(|v| v.concat())
}

/// A field value: braced, quoted, a number, a macro, or a concatenation.
fn value() -> impl Strategy<Value = String> {
    let atom = prop_oneof![
        3 => text().prop_map(|t| format!("{{{t}}}")),
        1 => text().prop_map(|t| format!("\"{}\"", t.replace('"', ""))),
        1 => "[0-9]{1,4}",
        1 => prop_oneof![Just("m0"), Just("m1"), Just("m2"), Just("jan"), Just("may"), Just("undef")]
            .prop_map(str::to_string),
    ];
    proptest::collection::vec(atom, 1..4).prop_map(|parts| parts.join(" # "))
}

fn field() -> impl Strategy<Value = String> {
    let key = prop_oneof![
        Just("title"),
        Just("author"),
        Just("editor"),
        Just("year"),
        Just("month"),
        Just("day"),
        Just("date"),
        Just("urldate"),
        Just("eventdate"),
        Just("origdate"),
        Just("urlyear"),
        Just("urlmonth"),
        Just("eventmonth"),
        Just("crossref"),
        Just("xdata"),
        Just("ids"),
        Just("journal"),
        Just("pages"),
        Just("note"),
        Just("howpublished"),
        Just("volume"),
        Just("url"),
        Just("doi"),
    ];
    (key, value()).prop_map(|(k, v)| format!("{k} = {v}"))
}

fn item() -> impl Strategy<Value = String> {
    let entry = (
        prop_oneof![
            Just("article"),
            Just("book"),
            Just("inproceedings"),
            Just("online"),
            Just("xdata")
        ],
        prop_oneof![Just("k0"), Just("k1"), Just("k2"), Just("k3")],
        proptest::collection::vec(field(), 0..6),
    )
        .prop_map(|(kind, key, fields)| format!("@{kind}{{{key}, {}}}", fields.join(", ")));
    let macro_def = (prop_oneof![Just("m0"), Just("m1"), Just("m2")], value())
        .prop_map(|(name, v)| format!("@string{{{name} = {v}}}"));
    prop_oneof![4 => entry, 1 => macro_def]
}

fn bibliography() -> impl Strategy<Value = String> {
    (
        proptest::collection::vec(item(), 1..6),
        prop_oneof![Just("\n"), Just(" ")],
    )
        .prop_map(|(items, sep)| items.join(sep))
}

/// Parse on a thread with a Wasm-sized stack; a panic or an overflow fails.
fn parses_on_wasm_stack(input: String, max_entries: Option<usize>) {
    let result = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let options = ParseOptions {
                max_entries,
                ..Default::default()
            };
            let bibtex = parse_bibtex(&input, &options);
            let auto = parse_auto(&input, &options);
            (bibtex, auto)
        })
        .unwrap()
        .join();
    let (bibtex, auto) = result.expect("parsing panicked");
    for result in [bibtex, auto] {
        assert!(result.entries.iter().all(|e| e.is_object()));
        if let Some(max) = max_entries {
            assert!(result.entries.len() <= max);
        }
    }
}

proptest! {
    #[test]
    fn bibtex_never_panics_on_structured_input(
        input in bibliography(),
        max_entries in proptest::option::of(0usize..3),
    ) {
        parses_on_wasm_stack(input, max_entries);
    }

    #[test]
    fn bibtex_never_panics_on_nested_structure(depth in 30usize..3000, inner in fragment()) {
        let nested = format!("{}{inner}{}", r"\emph{".repeat(depth), "}".repeat(depth));
        let input = format!("@article{{k0, title = {{{nested}}}, year = {{2020}}}}");
        parses_on_wasm_stack(input, None);
    }
}
