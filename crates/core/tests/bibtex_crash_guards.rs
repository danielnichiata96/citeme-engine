//! BibTeX inputs that crashed or hung the import.
//!
//! Each one is a short paste that reached a panic or an unbounded recursion
//! inside the `biblatex` crate — on Wasm, an abort that poisons the engine
//! instance. The parser now defuses them before `biblatex` reads the input:
//! the entry is kept, the offending field is dropped, and the drop is
//! reported in `errors`.
//!
//! Recursion tests run the parse on a thread with a 1 MiB stack, the size of
//! the Wasm instance's, so a stack overflow here means one there.

use citeme_engine_core::parsers::{bibtex::parse_bibtex, parse_auto, ParseOptions, ParseResult};
use serde_json::json;

fn parse(input: &str) -> ParseResult {
    parse_bibtex(input, &ParseOptions::default())
}

/// Parse on a thread with a Wasm-sized stack; an overflow aborts the test
/// binary, which is the failure.
fn parse_on_wasm_stack(input: String) -> ParseResult {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || parse(&input))
        .unwrap()
        .join()
        .unwrap()
}

fn assert_kept_and_reported(result: &ParseResult, field: &str) {
    assert_eq!(
        result.entries.len(),
        1,
        "entry must be kept: {:?}",
        result.errors
    );
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.error.starts_with(&format!("{field}:"))),
        "dropping {field} must be reported: {:?}",
        result.errors
    );
}

#[test]
fn a_day_past_255_in_month_is_dropped_not_a_panic() {
    // `day.parse::<u8>().unwrap()` on the day read out of `month`.
    let result = parse("@article{k, title = {T}, year = {2020}, month = {may 300}}");
    assert_kept_and_reported(&result, "month");
    assert_eq!(result.entries[0]["issued"]["date-parts"][0], json!([2020]));
}

#[test]
fn a_signed_year_with_a_space_is_dropped_not_a_panic() {
    // The year parser slices sign, space and digits together and unwraps
    // `"- 2020".parse::<i32>()`.
    let result = parse("@article{k, title = {T}, year = {- 2020}}");
    assert_kept_and_reported(&result, "year");
    assert!(result.entries[0].get("issued").is_none());
}

#[test]
fn a_repeated_field_is_checked_where_biblatex_reads_it() {
    // biblatex keeps the last value of a repeated field: checking only the
    // first would let the second through to the panic.
    let result = parse("@article{k, title = {T}, year = {2020}, year = {- 2020}}");
    assert_kept_and_reported(&result, "year");
    assert_eq!(result.entries[0]["issued"]["date-parts"][0], json!([2020]));
}

#[test]
fn non_ascii_digits_in_a_date_are_dropped_not_a_panic() {
    // Digits are scanned with `char::is_numeric` but parsed as ASCII.
    for date in ["²²", "٢٠", "2020-²"] {
        let result = parse(&format!("@article{{k, title = {{T}}, date = {{{date}}}}}"));
        assert_kept_and_reported(&result, "date");
    }
}

#[test]
fn a_date_hazard_is_checked_as_the_parser_will_read_it() {
    // LaTeX in a date can hide the panic from a look at the raw text:
    // `\-` prints nothing, so `-\- 2020` parses as "- 2020". (Found by the
    // property tests, through a rewrite of `\emph{` that did the same.)
    for (field, value) in [
        ("year", r"\emph{- 2020}, title = {}"),
        ("year", r"-\- 2020"),
        ("month", r"may\-{} 300"),
    ] {
        let input = format!("@article{{k, title = {{T}}, year = {{2020}}, {field} = {{{value}}}}}");
        let result = parse(&input);
        assert_eq!(result.entries.len(), 1, "{value}: {:?}", result.errors);
    }
}

#[test]
fn a_crossref_written_with_latex_is_dropped_not_followed() {
    // `J\o rgensen` names the entry `Jørgensen` only once LaTeX is
    // executed; a cycle through it can't be seen beforehand.
    let input = r"@book{Jørgensen, title = {T}, crossref = {J\o rgensen}}";
    let result = parse_on_wasm_stack(input.into());
    assert_kept_and_reported(&result, "crossref");
}

#[test]
fn a_date_from_a_long_macro_is_still_checked() {
    // Past what the checks inspect, a value used to go through unchecked.
    let input = format!(
        "@string{{d = {{²²{}}}}}\n@article{{k, title = {{T}}, date = d}}",
        "a".repeat(5_000)
    );
    let result = parse(&input);
    assert_kept_and_reported(&result, "date");
}

#[test]
fn a_crossref_too_long_to_check_is_dropped_not_followed() {
    let key = "k".repeat(5_000);
    let input = format!("@book{{{key}, title = {{T}}, crossref = {{{key}}}}}");
    let result = parse_on_wasm_stack(input);
    assert_kept_and_reported(&result, "crossref");
}

#[test]
fn a_dropped_field_reports_once() {
    // An unknown @string in a field that is then dropped: one report, not a
    // "kept as text" followed by "imported without it".
    let result = parse("@article{k, title = undefined # {a $ b}, year = {2020}}");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
    assert!(
        result.errors[0].error.starts_with("title: unclosed math"),
        "{:?}",
        result.errors
    );
}

#[test]
fn a_month_panic_is_defused_through_parse_auto_too() {
    let result = parse_auto(
        "@article{k, title = {T}, year = {2020}, month = {may 300}}",
        &ParseOptions::default(),
    );
    assert_eq!(result.entries.len(), 1, "{:?}", result.errors);
}

#[test]
fn a_crossref_to_itself_is_dropped_not_a_stack_overflow() {
    let result = parse_on_wasm_stack("@book{a, title = {T}, crossref = {a}}".into());
    assert_kept_and_reported(&result, "crossref");
}

#[test]
fn an_xdata_to_itself_is_dropped_not_a_stack_overflow() {
    let result = parse_on_wasm_stack("@book{a, title = {T}, xdata = {a}}".into());
    assert_kept_and_reported(&result, "xdata");
}

#[test]
fn a_crossref_cycle_is_dropped_not_a_stack_overflow() {
    let input = "@book{a, title = {A}, crossref = {b}}\n@book{b, title = {B}, crossref = {a}}";
    let result = parse_on_wasm_stack(input.into());
    assert_eq!(result.entries.len(), 2, "{:?}", result.errors);
    assert_eq!(
        result
            .errors
            .iter()
            .filter(|e| e.error.starts_with("crossref:"))
            .count(),
        2,
        "{:?}",
        result.errors
    );
}

#[test]
fn a_long_crossref_chain_is_cut_not_a_stack_overflow() {
    // Acyclic, just deep: resolution recursed once per link, and resolved
    // the whole chain again for every entry in it (2,000 entries took 8 s).
    let mut input = String::new();
    for i in 0..20_000 {
        input.push_str(&format!(
            "@misc{{a{i}, title={{T{i}}}, crossref={{a{}}}}}\n",
            i + 1
        ));
    }
    let result = parse_on_wasm_stack(input);
    assert_eq!(result.entries.len(), 20_000);
    // The last links of the chain still resolve.
    assert!(result.entries[19_999].get("title").is_some());
}

#[test]
fn a_self_referencing_string_macro_reads_as_empty_not_a_stack_overflow() {
    let input = "@string{a = a}\n@article{x, title = a, year = {2020}}";
    let result = parse_on_wasm_stack(input.into());
    assert_eq!(result.entries.len(), 1, "{:?}", result.errors);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.error.contains("@string \"a\"")),
        "{:?}",
        result.errors
    );
}

#[test]
fn a_string_macro_cycle_reads_as_empty_not_a_stack_overflow() {
    let input = "@string{a = b}\n@string{b = a}\n@article{x, title = a, year = {2020}}";
    let result = parse_on_wasm_stack(input.into());
    assert_eq!(result.entries.len(), 1, "{:?}", result.errors);
}

#[test]
fn a_doubling_string_macro_chain_is_cut_not_exponential() {
    // `m0 = m1 # m1`, `m1 = m2 # m2`, …: each level doubles the expansion.
    let mut input = String::new();
    for i in 0..40 {
        input.push_str(&format!("@string{{m{i} = m{} # m{}}}\n", i + 1, i + 1));
    }
    input.push_str("@string{m40 = {ab}}\n@article{x, title = m0, year = {2020}}");
    let result = parse_on_wasm_stack(input);
    assert_eq!(result.entries.len(), 1, "{:?}", result.errors);
    assert!(!result.errors.is_empty());
}

#[test]
fn deeply_nested_latex_commands_are_dropped_not_a_stack_overflow() {
    // Each command argument is parsed by a recursive call; ~5,000 levels
    // overflow a 1 MiB stack. `\emph` is rewritten to plain braces before
    // parsing; an unknown command keeps its recursion.
    for command in ["\\emph{", "\\foo{"] {
        let title = format!("{}x{}", command.repeat(20_000), "}".repeat(20_000));
        let input = format!("@article{{a, title = {{{title}}}, year = {{2020}}}}");
        let result = parse_on_wasm_stack(input);
        assert_kept_and_reported(&result, "title");
    }
}

#[test]
fn many_duplicate_keys_still_parse_in_one_go() {
    // Every duplicate key used to fail the whole-file parse; parsed entry
    // by entry instead, an inherited booktitle would be lost.
    let mut input = String::from(
        "@proceedings{proc, title = {Proceedings of X}, publisher = {P}, year = {2020}}\n\
         @inproceedings{paper, title = {Paper}, author = {A, B}, crossref = {proc}}\n",
    );
    for i in 0..12 {
        input.push_str(&format!("@misc{{dup, title = {{Twin {i}}}}}\n"));
    }
    let result = parse(&input);
    assert_eq!(result.entries.len(), 14, "{:?}", result.errors);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.entries[1]["container-title"], "Proceedings of X");
    assert!(result.entries[2..].iter().all(|e| e["id"] == "dup"));
}

#[test]
fn many_braces_inside_a_group_are_dropped_not_quadratic() {
    // `{{ {a}{a}… }}`: every inner brace splits a chunk that `biblatex`
    // merges back with `Vec::remove` (40,000 took 7 s).
    let input = format!(
        "@article{{a, title = {{{{{}}}}}, year = {{2020}}}}",
        "{a}".repeat(40_000)
    );
    let result = parse(&input);
    assert_kept_and_reported(&result, "title");
}

#[test]
fn deeply_nested_plain_braces_are_dropped_not_quadratic() {
    let input = format!(
        "@article{{a, title = {{{}x{}}}, year = {{2020}}}}",
        "{".repeat(20_000),
        "}".repeat(20_000)
    );
    let result = parse_on_wasm_stack(input);
    assert_kept_and_reported(&result, "title");
}

#[test]
fn ordinary_entries_report_nothing() {
    let result = parse(include_str!(
        "../../../tests/fixtures/samples/metadata-rich.bib"
    ));
    assert_eq!(result.entries.len(), 3);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}
